use crate::{
    app::App,
    model::Workspace,
    tree::{Kind, Row},
    widgets::{self as w, Icon},
};
use eframe::egui::{self, pos2, vec2};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};
#[derive(Default)]
pub struct State {
    signature: String,
    rows: Vec<Row>,
    scan: Option<mpsc::Receiver<Vec<Row>>>,
    documents: HashMap<String, Document>,
    merged: Arc<[Row]>,
    last_base: Option<Arc<[Row]>>,
    last_query: String,
    last_collapsed: HashSet<String>,
}
#[derive(Default)]
struct Document {
    text: String,
    error: String,
    loading: Option<mpsc::Receiver<Result<String, String>>>,
}
pub use crate::file_ids::{file_id, parse_id};
fn scan(ws: Vec<(String, PathBuf)>, known: HashSet<PathBuf>, show_bruno: bool) -> Vec<Row> {
    fn visit(
        root: &Path,
        dir: &Path,
        cid: &str,
        known: &HashSet<PathBuf>,
        show: bool,
        out: &mut Vec<Row>,
        depth: usize,
    ) {
        if depth > 32 || out.len() > 10000 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if [".git", ".reqon", ".trash", "node_modules", "environments"].contains(&name.as_str())
            {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                visit(root, &path, cid, known, show, out, depth + 1);
                continue;
            }
            if known.contains(&path)
                || crate::native_format::is_source(&path)
                || [
                    "folder.req",
                    "collection.req",
                    "folder.yml",
                    "opencollection.yml",
                ]
                .contains(&name.as_str())
                || (!show && name == "reqon.json")
                || name == "bruno.json"
            {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let relative = relative.to_string_lossy().replace('\\', "/");
            out.push(Row {
                key: file_id(cid, &relative),
                name,
                collection: cid.into(),
                folder: relative
                    .rsplit_once('/')
                    .map(|(p, _)| p)
                    .unwrap_or("")
                    .into(),
                method: String::new(),
                url: path.display().to_string(),
                depth,
                kind: Kind::File,
                expanded: false,
            });
        }
    }
    let mut out = vec![];
    for (cid, path) in ws {
        visit(&path, &path, &cid, &known, show_bruno, &mut out, 0);
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_hides_migrated_bru_sources_and_known_requests() {
        let dir = tempfile::tempdir().unwrap();
        for name in [
            "Legacy.req",
            "Legacy.bru",
            "Request.yml",
            "reqon.json",
            "README.md",
        ] {
            std::fs::write(dir.path().join(name), "fixture").unwrap();
        }
        let roots = vec![("collection".into(), dir.path().to_owned())];
        let known = HashSet::from([dir.path().join("Request.yml")]);
        let hidden = scan(roots.clone(), known.clone(), false);
        assert_eq!(
            hidden.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["README.md"]
        );
        let shown = scan(roots, known, true);
        assert_eq!(shown.len(), 2);
        assert!(shown.iter().any(|r| r.name == "reqon.json"));
        assert!(dir.path().join("Legacy.req").exists());
    }
}
impl State {
    pub fn rows(
        &mut self,
        ctx: &egui::Context,
        ws: &Workspace,
        base: Arc<[Row]>,
        query: &str,
        collapsed: &HashSet<String>,
        refresh: bool,
    ) -> Arc<[Row]> {
        if !ws.preferences.features.file_explorer {
            return base;
        }
        let signature = format!(
            "{}:{:?}",
            ws.preferences.features.reqon_json,
            ws.collections
                .iter()
                .map(|c| (&c.id, &c.path))
                .collect::<Vec<_>>()
        );
        let mut loaded = false;
        if let Some(rx) = &self.scan {
            match rx.try_recv() {
                Ok(rows) => {
                    self.rows = rows;
                    self.scan = None;
                    loaded = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scan = None,
                _ => {}
            }
        }
        if self.scan.is_none() && (signature != self.signature || refresh) {
            self.signature = signature;
            let collections = ws
                .collections
                .iter()
                .map(|c| (c.id.clone(), c.path.clone()))
                .collect();
            let known = ws.requests.iter().filter_map(|r| r.path.clone()).collect();
            let show = ws.preferences.features.reqon_json;
            let (tx, rx) = mpsc::channel();
            self.scan = Some(rx);
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(scan(collections, known, show));
                ctx.request_repaint();
            });
        }
        if self.scan.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if !loaded
            && self
                .last_base
                .as_ref()
                .is_some_and(|old| Arc::ptr_eq(old, &base))
            && self.last_query == query
            && self.last_collapsed == *collapsed
        {
            return self.merged.clone();
        }
        self.last_base = Some(base.clone());
        self.last_query = query.into();
        self.last_collapsed = collapsed.clone();
        let mut result = base.to_vec();
        let query = query.to_lowercase();
        for file in &self.rows {
            if (!query.is_empty() && !file.name.to_lowercase().contains(&query))
                || collapsed.contains(&file.collection)
            {
                continue;
            }
            if file
                .folder
                .split('/')
                .scan(String::new(), |p, part| {
                    if !p.is_empty() {
                        p.push('/');
                    }
                    p.push_str(part);
                    Some(p.clone())
                })
                .any(|p| collapsed.contains(&format!("{}:{p}", file.collection)))
            {
                continue;
            }
            let parent = if file.folder.is_empty() {
                file.collection.clone()
            } else {
                format!("{}:{}", file.collection, file.folder)
            };
            if !query.is_empty() && !result.iter().any(|r| r.key == parent) {
                if !result.iter().any(|r| r.key == file.collection) {
                    if let Some(c) = ws.collections.iter().find(|c| c.id == file.collection) {
                        result.push(Row {
                            key: c.id.clone(),
                            name: c.name.clone(),
                            collection: c.id.clone(),
                            folder: String::new(),
                            method: String::new(),
                            url: String::new(),
                            depth: 0,
                            kind: Kind::Collection,
                            expanded: true,
                        });
                    }
                }
                let mut prefix = String::new();
                for (depth, part) in file.folder.split('/').filter(|p| !p.is_empty()).enumerate() {
                    if !prefix.is_empty() {
                        prefix.push('/');
                    }
                    prefix.push_str(part);
                    let key = format!("{}:{prefix}", file.collection);
                    if !result.iter().any(|r| r.key == key) {
                        result.push(Row {
                            key,
                            name: part.into(),
                            collection: file.collection.clone(),
                            folder: prefix.clone(),
                            method: String::new(),
                            url: String::new(),
                            depth,
                            kind: Kind::Folder,
                            expanded: true,
                        });
                    }
                }
            }
            if let Some(index) = result.iter().position(|r| r.key == parent) {
                let mut at = index + 1;
                while at < result.len()
                    && result[at].collection == file.collection
                    && result[at].kind != Kind::Collection
                    && (file.folder.is_empty()
                        || result[at].folder == file.folder
                        || crate::tree_ops::inside(&result[at].folder, &file.folder))
                {
                    at += 1;
                }
                result.insert(at, file.clone());
            }
        }
        self.merged = result.into();
        self.merged.clone()
    }
}
impl App {
    pub fn file_workspace(&mut self, ui: &mut egui::Ui, r: egui::Rect, id: &str) {
        let Some((cid, relative)) = parse_id(id) else {
            return;
        };
        let Some(collection) = self.ws.collections.iter().find(|c| c.id == cid) else {
            return;
        };
        let path = collection.path.join(relative);
        if !self.file_explorer.documents.contains_key(id) {
            let root = collection.path.clone();
            let path = path.clone();
            let (tx, rx) = mpsc::channel();
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let result = (|| -> anyhow::Result<String> {
                    crate::tree_move::within_roots(&path, &[root])?;
                    anyhow::ensure!(
                        std::fs::metadata(&path)?.len() <= 4 * 1024 * 1024,
                        "File exceeds the 4 MB preview limit"
                    );
                    let text = std::fs::read_to_string(path)?;
                    anyhow::ensure!(!text.contains('\0'), "Binary file cannot be previewed");
                    Ok(text)
                })();
                let _ = tx.send(result.map_err(|e| format!("{e:#}")));
                ctx.request_repaint();
            });
            self.file_explorer.documents.insert(
                id.into(),
                Document {
                    loading: Some(rx),
                    ..Default::default()
                },
            );
        }
        let doc = self.file_explorer.documents.get_mut(id).unwrap();
        if let Some(rx) = &doc.loading {
            match rx.try_recv() {
                Ok(result) => {
                    doc.loading = None;
                    match result {
                        Ok(text) => doc.text = text,
                        Err(e) => doc.error = e,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    doc.loading = None;
                    doc.error = "Cannot load file".into()
                }
                _ => {
                    ui.ctx().request_repaint_after(Duration::from_millis(100));
                }
            }
        }
        let mut body = w::child(ui, r.shrink(16.), ("file-preview", id));
        body.horizontal(|ui| {
            w::icon(
                ui,
                ui.cursor().min + vec2(8., 12.),
                Icon::File,
                self.theme.muted,
                16.,
            );
            ui.add_space(24.);
            ui.label(relative);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Reveal in File Explorer").clicked() {
                    let _ = crate::tree_menu::reveal(&path);
                }
                if ui.button("Reload").clicked() {
                    doc.error = "reload".into();
                }
            });
        });
        body.separator();
        if doc.loading.is_some() {
            body.label("Loading file…");
        } else if !doc.error.is_empty() {
            body.colored_label(self.theme.muted, &doc.error);
        } else {
            w::editor(&mut body, &mut doc.text, true, self.theme);
        }
        if doc.error == "reload" {
            self.file_explorer.documents.remove(id);
        }
    }
    pub fn file_tree_row(&mut self, ui: &mut egui::Ui, row: &Row) {
        let r = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
        let response = ui.interact(r, ui.scope_id().with(&row.key), egui::Sense::click());
        if response.hovered() || self.ws.active.as_deref() == Some(&row.key) {
            ui.painter().rect_filled(r, 0, self.theme.surface);
        }
        let x = r.left() + 49. + row.depth as f32 * 18.;
        w::icon(ui, pos2(x, r.center().y), Icon::File, self.theme.muted, 15.);
        w::elided_label(
            ui,
            w::rect(x + 13., r.top(), r.right() - x - 20., r.height()),
            &row.name,
            13.,
            self.theme.text,
        );
        if response.clicked() {
            self.open(row.key.clone());
        }
    }
}
