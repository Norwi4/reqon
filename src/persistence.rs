use crate::{
    model::{Collection, History, Request, Workspace},
    storage,
};
use std::{
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, mpsc},
    thread,
};

#[derive(Default)]
struct State {
    pending: Option<Workspace>,
    request_order: Vec<String>,
    patches: std::collections::HashMap<String, Request>,
    history: Option<Vec<History>>,
    requests: std::collections::BTreeMap<PathBuf, Request>,
    environments: std::collections::BTreeMap<String, Collection>,
    moves: Vec<crate::tree_move::Plan>,
    operations: Vec<crate::tree_ops::Job>,
    auth_jobs: Vec<crate::auth_migration::Job>,
    stopping: bool,
    barriers: Vec<mpsc::Sender<()>>,
}
pub struct Writer {
    shared: Arc<(Mutex<State>, Condvar)>,
    worker: Option<thread::JoinHandle<()>>,
    pub errors: mpsc::Receiver<String>,
    pub saved: mpsc::Receiver<(String, String)>,
    pub moved: mpsc::Receiver<Result<crate::tree_move::Outcome, String>>,
    pub operated: mpsc::Receiver<Result<crate::tree_ops::Outcome, String>>,
}
impl Writer {
    pub fn new(root: PathBuf, repaint: eframe::egui::Context) -> Self {
        let shared = Arc::new((Mutex::new(State::default()), Condvar::new()));
        let state = shared.clone();
        let (tx, errors) = mpsc::channel();
        let (saved_tx, saved) = mpsc::channel();
        let (moved_tx, moved) = mpsc::channel();
        let (operated_tx, operated) = mpsc::channel();
        let worker = thread::Builder::new()
            .name("workspace-writer".into())
            .spawn(move || {
                let mut cached = storage::load(&root).unwrap_or_default();
                loop {
                    let (
                        snapshot,
                        order,
                        patches,
                        history,
                        requests,
                        environments,
                        moves,
                        operations,
                        auth_jobs,
                        barriers,
                    ) = {
                        let (lock, cv) = &*state;
                        let mut guard = lock.lock().unwrap();
                        while guard.pending.is_none()
                            && guard.requests.is_empty()
                            && guard.environments.is_empty()
                            && guard.moves.is_empty()
                            && guard.operations.is_empty()
                            && guard.auth_jobs.is_empty()
                            && guard.barriers.is_empty()
                            && !guard.stopping
                        {
                            guard = cv.wait(guard).unwrap();
                        }
                        if guard.pending.is_none()
                            && guard.requests.is_empty()
                            && guard.environments.is_empty()
                            && guard.moves.is_empty()
                            && guard.operations.is_empty()
                            && guard.auth_jobs.is_empty()
                            && guard.barriers.is_empty()
                            && guard.stopping
                        {
                            break;
                        }
                        (
                            guard.pending.take(),
                            std::mem::take(&mut guard.request_order),
                            std::mem::take(&mut guard.patches),
                            guard.history.take(),
                            std::mem::take(&mut guard.requests),
                            std::mem::take(&mut guard.environments),
                            std::mem::take(&mut guard.moves),
                            std::mem::take(&mut guard.operations),
                            std::mem::take(&mut guard.auth_jobs),
                            std::mem::take(&mut guard.barriers),
                        )
                    };
                    for (path, request) in requests {
                        let source = storage::to_bru(&request);
                        match storage::source(&request)
                            .and_then(|data| storage::atomic_write(&path, data))
                        {
                            Ok(()) => {
                                let _ = saved_tx.send((request.id, source));
                            }
                            Err(error) => {
                                let _ = tx
                                    .send(format!("Save failed for {}: {error:#}", path.display()));
                            }
                        }
                        repaint.request_repaint();
                    }
                    for plan in moves {
                        let result = crate::tree_move::execute(plan)
                            .map_err(|e| format!("Move failed: {e:#}"));
                        let _ = moved_tx.send(result);
                        repaint.request_repaint();
                    }
                    for job in operations {
                        let result = crate::tree_ops::execute(job).map_err(|e| format!("{e:#}"));
                        let _ = operated_tx.send(result);
                        repaint.request_repaint();
                    }
                    for (_, collection) in environments {
                        if let Err(error) = storage::save_environments(&collection) {
                            let _ = tx.send(format!("Environment save failed: {error:#}"));
                            repaint.request_repaint();
                        }
                    }
                    if let Some(mut snapshot) = snapshot {
                        let mut requests: std::collections::HashMap<_, _> = cached
                            .requests
                            .drain(..)
                            .map(|r| (r.id.clone(), r))
                            .collect();
                        requests.extend(patches);
                        snapshot.requests = order
                            .into_iter()
                            .filter_map(|id| requests.remove(&id))
                            .collect();
                        snapshot.history =
                            history.unwrap_or_else(|| std::mem::take(&mut cached.history));
                        cached = snapshot;
                        if let Err(error) = storage::save(&root, &cached) {
                            let _ = tx.send(format!("Workspace save failed: {error:#}"));
                            repaint.request_repaint();
                        }
                    }
                    // Serialized behind saves in this batch. Applying a stale
                    // preview fails instead of replacing newer editor changes.
                    for job in auth_jobs {
                        let result = crate::auth_migration::execute(job.operation)
                            .map_err(|e| format!("{e:#}"));
                        let _ = job.reply.send(result);
                        repaint.request_repaint();
                    }
                    for barrier in barriers {
                        let _ = barrier.send(());
                    }
                }
            })
            .expect("workspace writer thread");
        Self {
            shared,
            worker: Some(worker),
            errors,
            saved,
            moved,
            operated,
        }
    }
    pub fn submit(
        &self,
        snapshot: Workspace,
        order: Vec<String>,
        patches: Vec<Request>,
        history: Option<Vec<History>>,
    ) {
        let (lock, cv) = &*self.shared;
        let mut guard = lock.lock().unwrap();
        guard.pending = Some(snapshot);
        guard.request_order = order;
        guard
            .patches
            .extend(patches.into_iter().map(|r| (r.id.clone(), r)));
        if history.is_some() {
            guard.history = history;
        }
        drop(guard);
        cv.notify_one();
    }
    pub fn barrier(&self) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel();
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap().barriers.push(tx);
        cv.notify_one();
        rx
    }
    pub fn save_request(&self, request: Request) {
        if let Some(path) = request.path.clone() {
            let (lock, cv) = &*self.shared;
            lock.lock().unwrap().requests.insert(path, request);
            cv.notify_one();
        }
    }
    pub fn move_requests(&self, plan: crate::tree_move::Plan) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap().moves.push(plan);
        cv.notify_one();
    }
    pub fn tree_operation(&self, job: crate::tree_ops::Job) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap().operations.push(job);
        cv.notify_one();
    }
    pub fn auth_migration(
        &self,
        operation: crate::auth_migration::Operation,
    ) -> mpsc::Receiver<Result<crate::auth_migration::Outcome, String>> {
        let (reply, receiver) = mpsc::channel();
        let (lock, cv) = &*self.shared;
        lock.lock()
            .unwrap()
            .auth_jobs
            .push(crate::auth_migration::Job { operation, reply });
        cv.notify_one();
        receiver
    }
    pub fn save_environments(&self, collection: Collection) {
        let (lock, cv) = &*self.shared;
        lock.lock()
            .unwrap()
            .environments
            .insert(collection.id.clone(), collection);
        cv.notify_one();
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let (lock, cv) = &*self.shared;
        lock.lock().unwrap().stopping = true;
        cv.notify_one();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coalesced_updates_keep_other_requests_and_flush_latest_edits() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = storage::load(dir.path()).unwrap();
        let untouched = ws.requests[1].body.clone();
        let first = ws.requests[0].id.clone();
        let writer = Writer::new(dir.path().to_owned(), eframe::egui::Context::default());
        for i in 0..4 {
            ws.requests[0].url = format!("http://localhost/edited-{i}");
            writer.submit(
                ws.metadata_snapshot(),
                ws.requests.iter().map(|r| r.id.clone()).collect(),
                vec![ws.requests[0].clone()],
                None,
            );
        }
        drop(writer);
        let saved: Workspace =
            serde_json::from_slice(&std::fs::read(dir.path().join("workspace.json")).unwrap())
                .unwrap();
        assert_eq!(saved.requests.len(), 2);
        assert_eq!(
            saved.requests.iter().find(|r| r.id == first).unwrap().url,
            "http://localhost/edited-3"
        );
        assert_eq!(saved.requests[1].body, untouched);
    }
}
