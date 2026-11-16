use crate::app::App;
use std::time::{Duration, Instant};
impl App {
    pub fn start_runner(&mut self, cid: &str, ctx: &eframe::egui::Context) {
        let requests = self
            .ws
            .requests
            .iter()
            .filter(|r| r.collection == cid)
            .cloned()
            .collect();
        let options = crate::collection_runner::Options {
            iterations: self.runner_iterations.clamp(1, 10000),
            delay_ms: self.runner_delay,
            stop_on_error: self.runner_stop_on_error,
            ..Default::default()
        };
        match crate::collection_runner::Plan::new(requests, options) {
            Ok(plan) => self.runner_execution = Some(plan),
            Err(error) => {
                self.notice(error.to_string());
                return;
            }
        }
        self.runner_results.clear();
        self.runner_cancelled = false;
        self.runner_active = true;
        self.runner_current = None;
        self.runner_wait = None;
        self.advance_runner(ctx, false);
    }
    pub fn advance_runner(&mut self, ctx: &eframe::egui::Context, delay: bool) {
        self.runner_current = None;
        if !self.runner_active {
            return;
        }
        let next = self.runner_execution.as_mut().map(|p| p.next()).transpose();
        match next {
            Ok(Some(Some((request, iteration)))) => {
                self.runner_iteration = iteration;
                let next = request.id;
                self.runner_current = Some(next.clone());
                if delay && self.runner_delay > 0 {
                    let wait = Duration::from_millis(self.runner_delay.min(3600000));
                    self.runner_wait = Some((Instant::now() + wait, next));
                    ctx.request_repaint_after(wait);
                } else {
                    self.send(next, ctx);
                }
            }
            result => {
                self.runner_active = false;
                self.runner_execution = None;
                match result {
                    Err(error) => {
                        self.runner_results
                            .push((format!("Runner: {error}"), 0, false, 0));
                        self.notice(format!("Runner: {error}"));
                    }
                    _ => self.notice("Collection run finished"),
                }
            }
        }
    }
    pub fn stop_runner(&mut self) {
        self.runner_cancelled |= self.runner_active;
        self.runner_active = false;
        self.runner_execution = None;
        self.runner_wait = None;
        if let Some(id) = self.runner_current.take() {
            self.cancel(&id);
        }
        if self.runner_cancelled {
            self.notice("Collection run cancelled");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_cancels_scripts_and_restarts_without_late_effects() {
        use std::io::{Read, Write};
        for protocol in ["http", "ws"] {
            let dir = tempfile::tempdir().unwrap();
            let ctx = eframe::egui::Context::default();
            let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
            app.disk_sync = None;
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.set_nonblocking(true).unwrap();
            let address = listener.local_addr().unwrap();
            let (ready, received) = std::sync::mpsc::channel();
            let server = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(8);
                for expected in ["/ready", "/echo"] {
                    let mut stream = loop {
                        match listener.accept() {
                            Ok((stream, _)) => break stream,
                            Err(e)
                                if e.kind() == std::io::ErrorKind::WouldBlock
                                    && Instant::now() < deadline =>
                            {
                                std::thread::sleep(Duration::from_millis(5))
                            }
                            Err(e) => panic!("fixture accept: {e}"),
                        }
                    };
                    // Windows accepted sockets inherit the listener's mode.
                    stream.set_nonblocking(false).unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut bytes = Vec::new();
                    let mut buffer = [0; 2048];
                    while !bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                        let n = stream.read(&mut buffer).unwrap();
                        assert!(n > 0);
                        bytes.extend_from_slice(&buffer[..n]);
                    }
                    assert!(
                        String::from_utf8_lossy(&bytes).starts_with(&format!("GET {expected} "))
                    );
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                        )
                        .unwrap();
                    ready.send(()).unwrap();
                }
            });
            let request = &mut app.ws.requests[0];
            request.protocol = protocol.into();
            request.url = format!("http://{address}/echo");
            request.pre_script = format!(
                "await pm.sendRequest({{url:'http://{address}/ready'}}); pm.environment.set('LATE','discarded'); await bru.sleep(10000);"
            );
            request.tests.clear();
            let id = request.id.clone();
            app.send(id.clone(), &ctx);
            received.recv_timeout(Duration::from_secs(6)).unwrap();
            let cancelled_at = Instant::now();
            app.cancel(&id);
            assert!(!app.running.contains_key(&id));
            assert!(!app.running_context.contains_key(&id));
            assert!(!app.sockets.contains_key(&id));
            if protocol == "http" {
                assert!(app.tabs[&id].reply.as_ref().unwrap().cancelled);
            }
            app.ws.requests[0].protocol = "http".into();
            app.ws.requests[0].pre_script.clear();
            app.send(id.clone(), &ctx);
            while app.running.contains_key(&id) && cancelled_at.elapsed() < Duration::from_secs(5) {
                app.poll(&ctx);
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(!app.running.contains_key(&id));
            assert_eq!(app.tabs[&id].reply.as_ref().unwrap().status, 200);
            assert!(
                !app.ws
                    .collections
                    .iter()
                    .flat_map(|c| &c.environments)
                    .flat_map(|e| &e.vars)
                    .any(|p| p.key == "LATE")
            );
            server.join().unwrap();
            drop(app);
            assert!(
                cancelled_at.elapsed() < Duration::from_secs(3),
                "desktop shutdown waited for a cancelled script"
            );
        }
    }

    #[test]
    fn stopping_desktop_runner_during_delay_keeps_completed_results() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = eframe::egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        app.runner_active = true;
        app.runner_current = Some("pending".into());
        app.runner_wait = Some((Instant::now() + Duration::from_secs(30), "pending".into()));
        app.runner_results.push(("Completed".into(), 200, true, 10));
        app.stop_runner();
        app.poll(&ctx);
        assert!(app.runner_cancelled);
        assert!(!app.runner_active);
        assert!(app.runner_wait.is_none() && app.runner_current.is_none());
        assert!(app.running.is_empty());
        assert_eq!(app.runner_results.len(), 1);
    }
    #[test]
    fn desktop_and_headless_match_order_iterations_and_variable_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = eframe::egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().to_owned()).unwrap();
        app.disk_sync = None;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let _server = app.runtime.spawn(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            crate::mock::serve(listener, Default::default(), true).await;
        });
        let cid = app.ws.collections[0].id.clone();
        for r in &mut app.ws.requests {
            r.url = format!("http://{address}/echo");
            r.tests.clear();
            r.pre_script = "bru.setVar('runs',Number(bru.getVar('runs')||0)+1); pm.collectionVariables.set('count',Number(pm.collectionVariables.get('count')||0)+1);".into();
            r.tests = "pm.test('scopes',()=>expect(Number(pm.collectionVariables.get('count'))).to.equal(Number(bru.getVar('runs'))));".into();
        }
        let mut plan = crate::collection_runner::Plan::new(
            app.ws.requests.clone(),
            crate::collection_runner::Options {
                iterations: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let mut session = crate::collection_runner::Session::new(
            crate::engine::Engine::default(),
            vec![],
            vec![],
        );
        let report = app
            .runtime
            .block_on(crate::collection_runner::run(&mut plan, &mut session));
        assert!(
            report.success(),
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        app.runner_iterations = 2;
        app.runner_delay = 1;
        app.start_runner(&cid, &ctx);
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.runner_active && Instant::now() < deadline {
            app.poll(&ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.runner_active);
        assert_eq!(app.runner_results.len(), 4);
        assert!(
            app.runner_results
                .iter()
                .all(|(_, status, passed, _)| (200..300).contains(status) && *passed),
            "{:?}",
            app.runner_results
        );
        assert_eq!(app.runtime_vars[&cid]["runs"], "4");
        assert_eq!(app.runner_iteration, 2);
        assert_eq!(session.runtime["runs"], "4");
        assert_eq!(
            app.ws.collections[0]
                .vars
                .iter()
                .find(|p| p.key == "count")
                .unwrap()
                .value,
            "4"
        );
        let desktop: Vec<_> = app
            .runner_results
            .iter()
            .map(|(name, status, passed, _)| (name, *status, *passed))
            .collect();
        let headless: Vec<_> = report
            .results
            .iter()
            .map(|r| (&r.name, r.response.status, r.passed))
            .collect();
        assert_eq!(desktop, headless);
    }
}
