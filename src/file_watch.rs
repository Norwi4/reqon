use crate::{model::*, storage};
use notify::{RecursiveMode, Watcher};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

pub struct Update {
    pub id: String,
    pub collection: Collection,
    pub requests: Vec<(Request, String)>,
    pub warnings: Vec<String>,
}
pub struct DiskSync {
    watcher: notify::RecommendedWatcher,
    events: mpsc::Sender<notify::Result<notify::Event>>,
    roots: Arc<Mutex<BTreeMap<PathBuf, String>>>,
    pub updates: mpsc::Receiver<Update>,
}
impl DiskSync {
    pub fn new(ctx: eframe::egui::Context) -> anyhow::Result<Self> {
        let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
        let events = tx.clone();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })?;
        let roots = Arc::new(Mutex::new(BTreeMap::<PathBuf, String>::new()));
        let watched = roots.clone();
        let (out, updates) = mpsc::channel();
        std::thread::Builder::new()
            .name("collection-files".into())
            .spawn(move || {
                while let Ok(first) = rx.recv() {
                    let mut events = vec![first];
                    while let Ok(next) = rx.recv_timeout(Duration::from_millis(180)) {
                        events.push(next);
                    }
                    let mut changed = BTreeMap::new();
                    let roots = watched.lock().unwrap().clone();
                    for event in events.into_iter().flatten() {
                        if matches!(event.kind, notify::EventKind::Access(_)) {
                            continue;
                        }
                        for path in event.paths {
                            if ignored(&path) {
                                continue;
                            }
                            for (root, id) in &roots {
                                if path.starts_with(root) {
                                    changed.insert(root.clone(), id.clone());
                                }
                            }
                        }
                    }
                    for (path, id) in changed {
                        match storage::read_collection(&path) {
                            Ok((mut collection, mut requests, warnings)) => {
                                collection.id = id.clone();
                                for r in &mut requests {
                                    r.collection = id.clone();
                                }
                                let requests = requests
                                    .into_iter()
                                    .map(|r| {
                                        let source = storage::to_bru(&r);
                                        (r, source)
                                    })
                                    .collect();
                                if out
                                    .send(Update {
                                        id,
                                        collection,
                                        requests,
                                        warnings,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                                ctx.request_repaint();
                            }
                            Err(error) => {
                                eprintln!("Collection reload {}: {error:#}", path.display())
                            }
                        }
                    }
                }
            })?;
        Ok(Self {
            watcher,
            events,
            roots,
            updates,
        })
    }
    pub fn refresh(&self) {
        for root in self.roots.lock().unwrap().keys() {
            let _ = self
                .events
                .send(Ok(notify::Event::new(notify::EventKind::Modify(
                    notify::event::ModifyKind::Any,
                ))
                .add_path(root.clone())));
        }
    }
    pub fn watch(&mut self, collections: &[Collection]) {
        let mut roots = self.roots.lock().unwrap();
        let wanted: BTreeMap<_, _> = collections
            .iter()
            .map(|c| (c.path.clone(), c.id.clone()))
            .collect();
        for old in roots
            .keys()
            .filter(|p| !wanted.contains_key(*p))
            .cloned()
            .collect::<Vec<_>>()
        {
            let _ = self.watcher.unwatch(&old);
            roots.remove(&old);
        }
        for (path, id) in wanted {
            if !roots.contains_key(&path)
                && self.watcher.watch(&path, RecursiveMode::Recursive).is_ok()
            {
                roots.insert(path, id);
            }
        }
    }
}
fn ignored(path: &Path) -> bool {
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some(".git" | "node_modules" | ".reqon")
        )
    }) || path.extension().is_some_and(|ext| ext == "tmp")
}
