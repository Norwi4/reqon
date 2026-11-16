//! Collection scheduling shared by the desktop and command-line clients.
use crate::cancellation::CancellationToken;
use crate::runner_data::Dataset;
use crate::{engine::Engine, model::*};
use anyhow::{Result, bail, ensure};
use futures_util::{StreamExt, stream::FuturesUnordered};
use serde::Serialize;
use std::{collections::BTreeMap, time::Duration};

/// Folder subtrees are ORed; tags are ORed; the two groups are ANDed.
pub fn select_requests(
    requests: Vec<Request>,
    folders: &[String],
    tags: &[String],
) -> Result<Vec<Request>> {
    let normalize = |value: &str| value.replace('\\', "/").trim_matches('/').to_owned();
    let folders: Vec<_> = folders.iter().map(|s| normalize(s)).collect();
    ensure!(
        folders.iter().all(|s| !s.is_empty()
            && s.split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(':'))),
        "Folder filters must be relative collection paths"
    );
    ensure!(
        tags.iter().all(|s| !s.trim().is_empty()),
        "Tag filters must not be empty"
    );
    let selected: Vec<_> = requests
        .into_iter()
        .filter(|r| {
            let folder = normalize(&r.folder);
            (folders.is_empty()
                || folders.iter().any(|f| {
                    folder == *f
                        || folder
                            .strip_prefix(f)
                            .is_some_and(|tail| tail.starts_with('/'))
                }))
                && (tags.is_empty() || tags.iter().any(|t| r.tags.contains(t)))
        })
        .collect();
    ensure!(
        !selected.is_empty(),
        "No requests match the selected folders/tags"
    );
    Ok(selected)
}

#[derive(Clone, Debug)]
pub struct Options {
    pub iterations: u32,
    pub delay_ms: u64,
    pub stop_on_error: bool,
    pub max_steps: usize,
    /// Parallel independent requests per iteration; 1 preserves variable chaining.
    pub concurrency: usize,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            iterations: 1,
            delay_ms: 0,
            stop_on_error: false,
            max_steps: 10_000,
            concurrency: 1,
        }
    }
}

pub struct Plan {
    pub requests: Vec<Request>,
    pub options: Options,
    cursor: usize,
    iteration: u32,
    steps: usize,
    stopped: bool,
    data: Option<Dataset>,
}
impl Plan {
    pub fn new(mut requests: Vec<Request>, options: Options) -> Result<Self> {
        ensure!(!requests.is_empty(), "No requests selected");
        ensure!(
            (1..=10_000).contains(&options.iterations),
            "Iterations must be between 1 and 10000"
        );
        ensure!(
            options.max_steps > 0,
            "The execution step limit must be positive"
        );
        ensure!(
            (1..=64).contains(&options.concurrency),
            "Parallelism must be between 1 and 64"
        );
        ensure!(
            options.delay_ms <= 3_600_000,
            "Delay must not exceed one hour"
        );
        // Stable sort preserves persisted order for requests with equal sequence.
        requests.sort_by(|a, b| a.folder.cmp(&b.folder).then(a.seq.total_cmp(&b.seq)));
        Ok(Self {
            requests,
            options,
            cursor: 0,
            iteration: 0,
            steps: 0,
            stopped: false,
            data: None,
        })
    }
    /// One iteration per row, as in Kurlo/Newman; replaces the iteration count.
    pub fn with_data(mut self, data: Dataset) -> Self {
        self.options.iterations = data.rows().len() as u32;
        self.data = Some(data);
        self
    }
    pub fn next(&mut self) -> Result<Option<(Request, u32)>> {
        if self.stopped {
            return Ok(None);
        }
        if self.cursor == self.requests.len() {
            self.cursor = 0;
            self.iteration += 1;
        }
        if self.iteration >= self.options.iterations {
            return Ok(None);
        }
        if self.steps >= self.options.max_steps {
            self.stopped = true;
            bail!(
                "Collection execution exceeded {} steps; check setNextRequest loops",
                self.options.max_steps
            );
        }
        self.steps += 1;
        let mut request = self.requests[self.cursor].clone();
        request.run_iteration = self.iteration + 1;
        request.run_iteration_count = self.options.iterations;
        if let Some(data) = &self.data {
            request.iteration_data = data.rows()[self.iteration as usize].clone();
        }
        self.cursor += 1;
        Ok(Some((request, self.iteration + 1)))
    }
    pub fn complete(&mut self, reply: &Reply) -> Result<()> {
        if reply.cancelled
            || reply.script_effects.stop
            || (self.options.stop_on_error && !passed(reply))
        {
            self.stopped = true;
        } else if let Some(next) = &reply.script_effects.next {
            let path = [".req", ".bru", ".yml", ".yaml"]
                .iter()
                .find_map(|ext| next.strip_suffix(ext))
                .unwrap_or(next);
            let matches: Vec<_> = self
                .requests
                .iter()
                .enumerate()
                .filter(|(_, r)| {
                    r.id == *next
                        || r.name == *next
                        || format!("{}/{}", r.folder, r.name).trim_start_matches('/') == path
                })
                .map(|(index, _)| index)
                .collect();
            if matches.len() != 1 {
                self.stopped = true;
                bail!("Next request '{next}' must identify exactly one selected request");
            }
            self.cursor = matches[0];
        }
        Ok(())
    }
}

pub fn passed(reply: &Reply) -> bool {
    !reply.cancelled && reply.error.is_empty() && reply.tests.iter().all(|test| test.passed)
}

pub fn apply_values(
    rows: &mut Vec<Pair>,
    values: &BTreeMap<String, serde_json::Value>,
    deleted: &[String],
) {
    rows.retain(|p| !deleted.contains(&p.key));
    for (key, value) in values {
        let index = rows.iter().position(|p| p.key == *key).unwrap_or_else(|| {
            rows.push(Pair::new(key, ""));
            rows.len() - 1
        });
        let row = &mut rows[index];
        row.enabled = true;
        row.kind = match value {
            serde_json::Value::Null => "null",
            serde_json::Value::Bool(_) => "boolean",
            serde_json::Value::Number(_) => "number",
            serde_json::Value::String(_) => "",
            serde_json::Value::Array(_) => "array",
            serde_json::Value::Object(_) => "object",
        }
        .into();
        row.value = value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string());
    }
}

#[derive(Clone)]
pub struct Session {
    pub engine: Engine,
    pub globals: Vec<Pair>,
    pub environment: Vec<Pair>,
    pub runtime: BTreeMap<String, String>,
    collection_values: BTreeMap<String, serde_json::Value>,
}
impl Session {
    pub fn new(engine: Engine, globals: Vec<Pair>, environment: Vec<Pair>) -> Self {
        Self {
            engine,
            globals,
            environment,
            runtime: BTreeMap::new(),
            collection_values: BTreeMap::new(),
        }
    }
    pub async fn execute(&mut self, request: Request) -> Reply {
        let token = self.engine.cancellation_token();
        self.execute_cancellable(request, &token).await
    }
    pub async fn execute_cancellable(
        &mut self,
        request: Request,
        token: &CancellationToken,
    ) -> Reply {
        // prepare_scoped owns collection/folder inheritance for GUI and CLI.
        let reply = self
            .engine
            .clone()
            .with_cancellation(token.clone())
            .execute_with_collection(
                request,
                self.globals.clone(),
                self.environment.clone(),
                self.runtime.clone(),
                self.collection_values.clone(),
            )
            .await;
        if reply.cancelled {
            return reply;
        }
        apply_values(
            &mut self.environment,
            &reply.script_effects.environment,
            &reply.script_effects.deleted_environment,
        );
        apply_values(
            &mut self.globals,
            &reply.script_effects.globals,
            &reply.script_effects.deleted_globals,
        );
        self.collection_values
            .extend(reply.script_effects.collection.clone());
        for key in &reply.deleted_vars {
            self.runtime.remove(key);
        }
        self.runtime.extend(reply.runtime_vars.clone());
        reply
    }
}

#[derive(Serialize)]
pub struct Item {
    pub name: String,
    pub folder: String,
    pub iteration: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_row: Option<u32>,
    pub passed: bool,
    pub response: Reply,
}
#[derive(Default, Serialize)]
pub struct Report {
    pub results: Vec<Item>,
    pub error: String,
    pub cancelled: bool,
}
impl Report {
    pub fn success(&self) -> bool {
        !self.cancelled
            && self.error.is_empty()
            && !self.results.is_empty()
            && self.results.iter().all(|r| r.passed)
    }
    pub fn public_json(&self, secrets: &[String]) -> Result<serde_json::Value> {
        let mut value = serde_json::to_value(self)?;
        let mut secrets = secrets.to_vec();
        secrets.extend(
            self.results
                .iter()
                .flat_map(|r| r.response.redaction_values.iter().cloned()),
        );
        crate::auth_vault::redact(&mut value, &secrets);
        Ok(value)
    }
}

pub async fn run(plan: &mut Plan, session: &mut Session) -> Report {
    let token = session.engine.cancellation_token();
    run_cancellable(plan, session, &token).await
}

/// Cancellation preserves completed results and discards the current request's
/// uncommitted variable changes. A fresh Plan/token starts a new run.
pub async fn run_cancellable(
    plan: &mut Plan,
    session: &mut Session,
    token: &CancellationToken,
) -> Report {
    if plan.options.concurrency > 1 {
        return run_parallel(plan, session, token).await;
    }
    let mut report = Report::default();
    loop {
        if token.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let next = match plan.next() {
            Ok(next) => next,
            Err(error) => {
                report.error = error.to_string();
                break;
            }
        };
        let Some((request, iteration)) = next else {
            break;
        };
        if !report.results.is_empty() && plan.options.delay_ms > 0 {
            tokio::select! {
                biased;
                _ = token.cancelled() => { report.cancelled = true; break; }
                _ = tokio::time::sleep(Duration::from_millis(plan.options.delay_ms)) => {}
            }
        }
        let name = request.name.clone();
        let folder = request.folder.clone();
        let mut response = session.execute_cancellable(request, token).await;
        response.bytes.clear();
        report.cancelled |= response.cancelled;
        let status = plan.complete(&response);
        report.results.push(Item {
            name,
            folder,
            iteration,
            data_row: plan.data.as_ref().map(|_| iteration),
            passed: passed(&response),
            response,
        });
        if let Err(error) = status {
            report.error = error.to_string();
            break;
        }
        if report.cancelled {
            break;
        }
    }
    if report.cancelled {
        plan.stopped = true;
        report.error = "Collection run cancelled".into();
    }
    report
}

/// Each parallel request sees the same initial variables. Changes stay local;
/// committing them in completion order would make subsequent runs nondeterministic.
async fn run_parallel(plan: &mut Plan, session: &Session, token: &CancellationToken) -> Report {
    let mut report = Report::default();
    let mut started = 0usize;
    let mut stop = plan.stopped;
    let mut next_start = tokio::time::Instant::now();
    for iteration in 1..=plan.options.iterations {
        if stop || token.is_cancelled() {
            break;
        }
        let mut pending = FuturesUnordered::new();
        let mut cursor = 0usize;
        let mut completed = Vec::new();
        loop {
            if token.is_cancelled() {
                stop = true;
            }
            while !stop
                && pending.len() < plan.options.concurrency
                && cursor < plan.requests.len()
                && tokio::time::Instant::now() >= next_start
            {
                if started >= plan.options.max_steps {
                    report.error = format!(
                        "Collection execution exceeded {} steps",
                        plan.options.max_steps
                    );
                    stop = true;
                    break;
                }
                let index = cursor;
                cursor += 1;
                started += 1;
                let mut request = plan.requests[index].clone();
                request.run_iteration = iteration;
                request.run_iteration_count = plan.options.iterations;
                if let Some(data) = &plan.data {
                    request.iteration_data = data.rows()[(iteration - 1) as usize].clone();
                }
                let mut isolated = session.clone();
                let token = token.clone();
                let data_row = plan.data.as_ref().map(|_| iteration);
                pending.push(Box::pin(async move {
                    let name = request.name.clone();
                    let folder = request.folder.clone();
                    let mut response = isolated.execute_cancellable(request, &token).await;
                    response.bytes.clear();
                    if response.script_effects.next.is_some() {
                        response.error =
                            "setNextRequest requires sequential execution (--parallel 1)".into();
                    }
                    (
                        index,
                        Item {
                            name,
                            folder,
                            iteration,
                            data_row,
                            passed: passed(&response),
                            response,
                        },
                    )
                }));
                next_start =
                    tokio::time::Instant::now() + Duration::from_millis(plan.options.delay_ms);
            }
            if pending.is_empty() && (stop || cursor == plan.requests.len()) {
                break;
            }
            let can_start =
                !stop && cursor < plan.requests.len() && pending.len() < plan.options.concurrency;
            tokio::select! {
                biased;
                _ = token.cancelled(), if !stop => { stop = true; }
                Some((index, item)) = pending.next(), if !pending.is_empty() => {
                    report.cancelled |= item.response.cancelled;
                    stop |= item.response.cancelled || item.response.script_effects.stop
                        || item.response.script_effects.next.is_some()
                        || (plan.options.stop_on_error && !item.passed);
                    completed.push((index, item));
                }
                _ = tokio::time::sleep_until(next_start), if can_start => {}
            }
        }
        completed.sort_by_key(|(index, _)| *index);
        report
            .results
            .extend(completed.into_iter().map(|(_, item)| item));
    }
    report.cancelled |= token.is_cancelled();
    if report.cancelled {
        report.error = "Collection run cancelled".into();
    }
    plan.stopped = true;
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loops_are_bounded_and_equal_sequences_keep_input_order() {
        let a = Request {
            name: "Z".into(),
            ..Default::default()
        };
        let b = Request {
            name: "A".into(),
            ..Default::default()
        };
        let mut plan = Plan::new(
            vec![a, b],
            Options {
                max_steps: 2,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.next().unwrap().unwrap().0.name, "Z");
        let reply = Reply {
            script_effects: ScriptEffects {
                next: Some("Z".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        plan.complete(&reply).unwrap();
        assert_eq!(plan.next().unwrap().unwrap().0.name, "Z");
        plan.complete(&reply).unwrap();
        assert!(plan.next().is_err());
    }
    #[tokio::test]
    async fn headless_runner_carries_environment_and_runtime_and_reports_failure() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(crate::mock::serve(listener, Default::default(), true));
        let request = Request { name: "Echo".into(), url: format!("http://{address}/echo"),
            pre_script: "bru.setVar('runs',Number(bru.getVar('runs')||0)+1);".into(),
            tests: "pm.test('status',()=>pm.response.to.have.status(200)); pm.environment.set('count', Number(pm.environment.get('count')||0)+1);".into(), ..Default::default() };
        let mut plan = Plan::new(
            vec![request],
            Options {
                iterations: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let mut session = Session::new(Engine::default(), vec![], vec![]);
        let report = run(&mut plan, &mut session).await;
        assert!(
            report.success(),
            "{}",
            serde_json::to_string(&report).unwrap()
        );
        assert_eq!(report.results.len(), 2);
        assert_eq!(session.runtime["runs"], "2");
        assert_eq!(
            session
                .environment
                .iter()
                .find(|p| p.key == "count")
                .unwrap()
                .value,
            "2"
        );
        server.abort();
    }
}
