//! File-backed tree actions, serialized with saves on the persistence worker.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    Request(String),
    Folder { collection: String, folder: String },
}
#[derive(Clone, Debug)]
pub enum Operation {
    Paste {
        collection: String,
        parent: String,
        copied: crate::tree_menu::Clipboard,
    },
    NewFolder {
        collection: String,
        parent: String,
        name: String,
    },
    Clone(Subject),
    Rename(Subject, String),
    Delete(Subject),
}
pub struct Job {
    pub operation: Operation,
    pub collection: Option<Collection>,
    pub requests: Vec<Request>,
    pub drafts: Vec<Request>,
    pub clone_seq: Option<f64>,
}
#[derive(Default)]
pub struct Outcome {
    pub renamed_request: Option<String>,
    pub updated: Vec<Request>,
    pub added: Vec<Request>,
    pub removed: Vec<String>,
    pub folders: Vec<(String, String)>,
    pub folder_change: Option<(String, String, Option<String>)>,
    pub saved: Vec<(String, String)>,
    pub message: String,
}
pub fn inside(folder: &str, parent: &str) -> bool {
    folder == parent
        || folder
            .strip_prefix(parent)
            .is_some_and(|s| s.starts_with('/'))
}
pub fn folder_id(cid: &str, folder: &str) -> String {
    format!("__folder__{cid}|{folder}")
}
pub fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    ensure!(
        !name.is_empty() && name.len() <= 255,
        "Enter a name between 1 and 255 characters"
    );
    ensure!(
        name != "."
            && name != ".."
            && !name.ends_with(['.', ' '])
            && !name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c)),
        "Invalid file or folder name"
    );
    let stem = name.split('.').next().unwrap().to_uppercase();
    ensure!(
        ![
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"
        ]
        .contains(&stem.as_str()),
        "This name is reserved by Windows"
    );
    ensure!(
        ![".reqon", ".git", "environments", "node_modules"].contains(&name.to_lowercase().as_str()),
        "This name is reserved by the collection format"
    );
    Ok(name.into())
}
pub fn valid_request_name(name: &str) -> Result<String> {
    let name = valid_name(name)?;
    ensure!(
        !["folder", "collection"].contains(&name.to_lowercase().as_str()),
        "This filename is reserved by the collection format"
    );
    Ok(name)
}
pub fn checked_path(collection: &Collection, folder: &str) -> Result<PathBuf> {
    ensure!(
        folder
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':'])),
        "Invalid folder path"
    );
    ensure!(
        !folder
            .split('/')
            .any(|p| [".reqon", ".git", "node_modules", "environments"].contains(&p)),
        "This folder belongs to collection storage"
    );
    let path = collection.path.join(folder);
    crate::tree_move::within_roots(&path, std::slice::from_ref(&collection.path))?;
    ensure!(path != collection.path, "Cannot modify the collection root");
    Ok(path)
}
fn paste_parent(collection: &Collection, folder: &str) -> Result<PathBuf> {
    if folder.is_empty() {
        ensure!(
            collection.path.is_dir(),
            "Collection directory is unavailable"
        );
        Ok(collection.path.clone())
    } else {
        let path = checked_path(collection, folder)?;
        ensure!(path.is_dir(), "Destination folder is unavailable");
        Ok(path)
    }
}
pub fn prepare(ws: &Workspace, operation: Operation) -> Result<Job> {
    if let Operation::Paste {
        collection,
        parent,
        copied,
    } = &operation
    {
        let c = ws
            .collections
            .iter()
            .find(|c| &c.id == collection)
            .cloned()
            .context("Select a collection to paste into")?;
        paste_parent(&c, parent)?;
        return Ok(Job {
            operation: operation.clone(),
            collection: Some(c),
            requests: copied.requests.clone(),
            drafts: copied.settings.clone(),
            clone_seq: None,
        });
    }
    let (cid, subject) = match &operation {
        Operation::NewFolder {
            collection, name, ..
        } => {
            valid_name(name)?;
            (collection.clone(), None)
        }
        Operation::Paste { .. } => unreachable!(),
        Operation::Clone(s) | Operation::Delete(s) | Operation::Rename(s, _) => {
            if let Operation::Rename(target, name) = &operation {
                if matches!(target, Subject::Request(_)) {
                    valid_request_name(name)?;
                } else {
                    valid_name(name)?;
                }
            }
            let cid = match s {
                Subject::Request(id) => ws
                    .requests
                    .iter()
                    .find(|r| r.id == *id)
                    .context("Request no longer exists")?
                    .collection
                    .clone(),
                Subject::Folder { collection, .. } => collection.clone(),
            };
            (cid, Some(s))
        }
    };
    let collection = ws.collections.iter().find(|c| c.id == cid).cloned();
    if !cid.is_empty() {
        ensure!(collection.is_some(), "Collection no longer exists");
    }
    let selected = |r: &&Request| match subject {
        Some(Subject::Request(id)) => r.id == *id,
        Some(Subject::Folder { collection, folder }) => {
            r.collection == *collection && inside(&r.folder, folder)
        }
        None => false,
    };
    let requests: Vec<Request> = ws.requests.iter().filter(selected).cloned().collect();
    let clone_seq = if matches!(operation, Operation::Clone(Subject::Request(_))) {
        let request = &requests[0];
        let next = ws
            .requests
            .iter()
            .filter(|r| {
                r.collection == request.collection
                    && r.folder == request.folder
                    && r.seq > request.seq
            })
            .map(|r| r.seq)
            .min_by(f64::total_cmp)
            .unwrap_or(request.seq + 1.);
        Some(request.seq + (next - request.seq) / 2.)
    } else {
        None
    };
    let drafts: Vec<Request> = ws
        .collection_drafts
        .values()
        .filter(selected)
        .cloned()
        .collect();
    if let Some(Subject::Folder { folder, .. }) = subject {
        let c = collection.as_ref().context("Missing collection")?;
        let base = checked_path(c, folder)?;
        for request in requests.iter().chain(&drafts) {
            if let Some(path) = &request.path {
                ensure!(
                    path.starts_with(&base),
                    "Request path is outside its folder"
                );
                crate::tree_move::within_roots(path, std::slice::from_ref(&c.path))?;
            }
        }
    }
    Ok(Job {
        operation,
        collection,
        requests,
        drafts,
        clone_seq,
    })
}
fn metadata(c: &Collection, folder: &str) -> Request {
    let yaml = c.path.join("opencollection.yml").is_file();
    Request {
        id: folder_id(&c.id, folder),
        name: folder.rsplit('/').next().unwrap_or(folder).into(),
        protocol: "folder".into(),
        collection: c.id.clone(),
        folder: folder.into(),
        path: Some(
            c.path
                .join(folder)
                .join(if yaml { "folder.yml" } else { "folder.req" }),
        ),
        file_format: if yaml { "yml" } else { "req" }.into(),
        auth: Auth {
            kind: "inherit".into(),
            ..Default::default()
        },
        ..Default::default()
    }
}
fn unique_sibling(path: &Path, name: &str, extension: Option<&str>) -> PathBuf {
    let mut suffix = 1;
    loop {
        let stem = if suffix == 1 {
            name.into()
        } else {
            format!("{name} {suffix}")
        };
        let candidate = path.join(match extension {
            Some(ext) => format!("{stem}.{ext}"),
            None => stem,
        });
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}
fn write_new(path: &Path, data: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    if let Err(error) = file.write_all(data).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(error.into());
    }
    Ok(())
}
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    ensure!(
        !fs::symlink_metadata(from)?.file_type().is_symlink(),
        "Cannot clone symbolic links"
    );
    fs::create_dir(to)?;
    for item in fs::read_dir(from)? {
        let item = item?;
        let ty = item.file_type()?;
        ensure!(
            !ty.is_symlink(),
            "Cannot clone symbolic links: {}",
            item.path().display()
        );
        if ty.is_dir() {
            copy_tree(&item.path(), &to.join(item.file_name()))?;
        } else {
            fs::copy(item.path(), to.join(item.file_name()))?;
        }
    }
    Ok(())
}
fn staging(c: &Collection) -> Result<PathBuf> {
    let area = c.path.join(".reqon");
    crate::tree_move::within_roots(&area, std::slice::from_ref(&c.path))?;
    fs::create_dir_all(&area)?;
    Ok(area.join(format!("tree-{}", id())))
}
fn relative(c: &Collection, path: &Path) -> Result<String> {
    Ok(path
        .strip_prefix(&c.path)?
        .to_string_lossy()
        .replace('\\', "/"))
}

fn case_only(a: &Path, b: &Path) -> bool {
    cfg!(windows)
        && a != b
        && a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
}

pub fn execute(job: Job) -> Result<Outcome> {
    let mut out = Outcome::default();
    match &job.operation {
        Operation::Paste { parent, copied, .. } => {
            let c = job.collection.as_ref().context("Select a collection")?;
            let parent_path = paste_parent(c, parent)?;
            let yaml = c.path.join("opencollection.yml").is_file();
            if let Subject::Folder { folder: source, .. } = &copied.subject {
                let name = source.rsplit('/').next().context("Folder has no name")?;
                let final_path = unique_sibling(&parent_path, &valid_name(name)?, None);
                let target = relative(c, &final_path)?;
                let stage = staging(c)?;
                let result = (|| -> Result<()> {
                    let mut folders = copied.folders.clone();
                    if !folders.contains(source) {
                        folders.push(source.clone());
                    }
                    for folder in folders {
                        ensure!(
                            inside(&folder, source),
                            "Copied folder is outside its source"
                        );
                        let suffix = folder.strip_prefix(source).unwrap().trim_start_matches('/');
                        let destination = if suffix.is_empty() {
                            target.clone()
                        } else {
                            format!("{target}/{suffix}")
                        };
                        checked_path(c, &destination)?;
                        fs::create_dir_all(stage.join(suffix))?;
                        let mut draft = copied
                            .settings
                            .iter()
                            .find(|r| r.folder == folder)
                            .cloned()
                            .unwrap_or_else(|| metadata(c, &destination));
                        draft.folder = destination.clone();
                        draft.collection = c.id.clone();
                        draft.name = folder.rsplit('/').next().unwrap_or(&folder).into();
                        draft.file_format = if yaml { "yml" } else { "req" }.into();
                        draft.path = Some(stage.join(suffix).join(if yaml {
                            "folder.yml"
                        } else {
                            "folder.req"
                        }));
                        storage::save_request(&draft)?;
                        out.folders.push((c.id.clone(), destination));
                    }
                    for old in &copied.requests {
                        ensure!(
                            inside(&old.folder, source),
                            "Copied request is outside its folder"
                        );
                        let suffix = old
                            .folder
                            .strip_prefix(source)
                            .unwrap()
                            .trim_start_matches('/');
                        let folder = if suffix.is_empty() {
                            target.clone()
                        } else {
                            format!("{target}/{suffix}")
                        };
                        let mut request = old.clone();
                        request.id = id();
                        request.collection = c.id.clone();
                        request.folder = folder.clone();
                        request.file_format = if yaml { "yml" } else { "req" }.into();
                        fs::create_dir_all(stage.join(suffix))?;
                        let path = unique_sibling(
                            &stage.join(suffix),
                            &safe_name(&request.name),
                            Some(&request.file_format),
                        );
                        request.path = Some(path.clone());
                        write_new(&path, storage::source(&request)?.as_bytes())?;
                        request.path = Some(final_path.join(path.strip_prefix(&stage)?));
                        out.saved
                            .push((request.id.clone(), storage::to_bru(&request)));
                        out.added.push(request);
                    }
                    fs::rename(&stage, &final_path)?;
                    Ok(())
                })();
                if result.is_err() {
                    let _ = fs::remove_dir_all(&stage);
                }
                result?;
            } else {
                for old in &copied.requests {
                    let mut request = old.clone();
                    request.id = id();
                    request.collection = c.id.clone();
                    request.folder = parent.clone();
                    request.file_format = if yaml { "yml" } else { "req" }.into();
                    let path = unique_sibling(
                        &parent_path,
                        &safe_name(&request.name),
                        Some(&request.file_format),
                    );
                    request.path = Some(path.clone());
                    write_new(&path, storage::source(&request)?.as_bytes())?;
                    out.saved
                        .push((request.id.clone(), storage::to_bru(&request)));
                    out.added.push(request);
                }
            }
            out.message = "Pasted".into();
        }
        Operation::NewFolder { parent, name, .. } => {
            let c = job.collection.as_ref().context("Select a collection")?;
            let folder = if parent.is_empty() {
                valid_name(name)?
            } else {
                format!("{parent}/{}", valid_name(name)?)
            };
            let path = checked_path(c, &folder)?;
            ensure!(!path.exists(), "A folder with this name already exists");
            let draft = metadata(c, &folder);
            fs::create_dir(&path)?;
            if let Err(e) = storage::save_request(&draft) {
                let _ = fs::remove_dir(&path);
                return Err(e);
            }
            out.folders.push((c.id.clone(), folder));
            out.message = "Folder created".into();
        }
        Operation::Clone(Subject::Request(_)) => {
            let mut request = job.requests[0].clone();
            request.id = id();
            request.name.push_str(" copy");
            request.seq = job.clone_seq.unwrap_or(request.seq + 0.5);
            if let Some(old) = &request.path {
                let c = job.collection.as_ref().context("Missing collection")?;
                crate::tree_move::within_roots(old, std::slice::from_ref(&c.path))?;
                let path = unique_sibling(
                    old.parent().unwrap(),
                    &safe_name(&request.name),
                    old.extension().and_then(|s| s.to_str()),
                );
                request.path = Some(path.clone());
                write_new(&path, storage::source(&request)?.as_bytes())?;
                out.saved
                    .push((request.id.clone(), storage::to_bru(&request)));
            }
            out.added.push(request);
            out.message = "Request cloned".into();
        }
        Operation::Rename(Subject::Request(_), name) => {
            let mut request = job.requests[0].clone();
            request.name = valid_request_name(name)?;
            if let Some(old) = &job.requests[0].path {
                let c = job.collection.as_ref().context("Missing collection")?;
                crate::tree_move::within_roots(old, std::slice::from_ref(&c.path))?;
                let path = old.with_file_name(format!(
                    "{}.{}",
                    request.name,
                    old.extension().and_then(|s| s.to_str()).unwrap_or("req")
                ));
                if path != *old {
                    ensure!(
                        !path.exists() || case_only(old, &path),
                        "A request with this filename already exists"
                    );
                }
                request.path = Some(path.clone());
                if path == *old {
                    storage::save_request(&request)?;
                } else if case_only(old, &path) && old.exists() {
                    let contents = storage::source(&request)?;
                    let temporary = staging(c)?;
                    fs::rename(old, &temporary)?;
                    if let Err(e) = write_new(&path, contents.as_bytes()) {
                        let _ = fs::rename(&temporary, old);
                        return Err(e);
                    }
                    let _ = fs::remove_file(temporary);
                } else {
                    write_new(&path, storage::source(&request)?.as_bytes())?;
                    if old.exists() {
                        if let Err(e) = fs::remove_file(old) {
                            let _ = fs::remove_file(path);
                            return Err(e.into());
                        }
                    }
                }
                out.saved
                    .push((request.id.clone(), storage::to_bru(&request)));
            }
            out.renamed_request = Some(request.id.clone());
            out.updated.push(request);
            out.message = "Request renamed".into();
        }
        Operation::Delete(Subject::Request(_)) => {
            let request = &job.requests[0];
            if let Some(path) = request.path.as_ref().filter(|p| p.exists()) {
                let c = job.collection.as_ref().context("Missing collection")?;
                crate::tree_move::within_roots(path, std::slice::from_ref(&c.path))?;
                let trash = staging(c)?;
                fs::create_dir(&trash)?;
                fs::rename(path, trash.join(path.file_name().unwrap()))?;
            }
            out.removed.push(request.id.clone());
            out.message = "Request deleted".into();
        }
        Operation::Clone(Subject::Folder { folder, .. }) => {
            let c = job.collection.as_ref().context("Missing collection")?;
            let old = checked_path(c, folder)?;
            let target = unique_sibling(
                old.parent().unwrap(),
                &format!("{} copy", old.file_name().unwrap().to_string_lossy()),
                None,
            );
            let new_folder = relative(c, &target)?;
            let stage = staging(c)?;
            let result = (|| -> Result<()> {
                copy_tree(&old, &stage)?;
                for before in &job.requests {
                    let mut after = before.clone();
                    after.id = id();
                    after.folder = format!("{new_folder}{}", &before.folder[folder.len()..]);
                    if let Some(path) = &before.path {
                        let rel = path.strip_prefix(&old)?;
                        after.path = Some(target.join(rel));
                        storage::atomic_write(&stage.join(rel), storage::source(&after)?)?;
                        out.saved.push((after.id.clone(), storage::to_bru(&after)));
                    }
                    out.added.push(after);
                }
                for before in &job.drafts {
                    if let Some(path) = &before.path {
                        let rel = path.strip_prefix(&old)?;
                        let mut draft = before.clone();
                        if before.folder == *folder {
                            draft.name = target.file_name().unwrap().to_string_lossy().into_owned();
                        }
                        storage::atomic_write(&stage.join(rel), storage::source(&draft)?)?;
                    }
                }
                let mut top = metadata(c, &new_folder);
                let staged_metadata = stage.join(top.path.as_ref().unwrap().file_name().unwrap());
                if let Some(mut original) = crate::inheritance::parent(&staged_metadata)? {
                    original.name = top.name.clone();
                    top = original;
                }
                storage::atomic_write(&staged_metadata, storage::source(&top)?)?;
                ensure!(!target.exists(), "Clone destination already exists");
                fs::rename(&stage, &target)?;
                Ok(())
            })();
            if result.is_err() && stage.exists() {
                let _ = fs::remove_dir_all(&stage);
            }
            result?;
            out.folders.push((c.id.clone(), new_folder.clone()));
            for f in &c.folders {
                if inside(f, folder) {
                    out.folders
                        .push((c.id.clone(), format!("{new_folder}{}", &f[folder.len()..])));
                }
            }
            out.message = "Folder cloned".into();
        }
        Operation::Rename(Subject::Folder { folder, .. }, name) => {
            let c = job.collection.as_ref().context("Missing collection")?;
            let old = checked_path(c, folder)?;
            let target = old.with_file_name(valid_name(name)?);
            let new_folder = relative(c, &target)?;
            if target != old {
                ensure!(
                    !target.exists() || case_only(&old, &target),
                    "A folder with this name already exists"
                );
                fs::rename(&old, &target)?;
            }
            let meta = metadata(c, &new_folder);
            let path = meta.path.as_ref().unwrap();
            let result = (|| -> Result<()> {
                let mut saved = crate::inheritance::parent(path)?.unwrap_or_else(|| meta.clone());
                saved.name = name.trim().into();
                saved.path = Some(path.clone());
                storage::save_request(&saved)?;
                out.saved.push((meta.id.clone(), storage::to_bru(&saved)));
                Ok(())
            })();
            if result.is_err() && target != old {
                let _ = fs::rename(&target, &old);
            }
            result?;
            for before in &job.requests {
                let mut after = before.clone();
                after.folder = format!("{new_folder}{}", &before.folder[folder.len()..]);
                if let Some(path) = &before.path {
                    after.path = Some(target.join(path.strip_prefix(&old)?));
                }
                out.updated.push(after);
            }
            out.folder_change = Some((c.id.clone(), folder.clone(), Some(new_folder)));
            out.message = "Folder renamed".into();
        }
        Operation::Delete(Subject::Folder { folder, .. }) => {
            let c = job.collection.as_ref().context("Missing collection")?;
            let old = checked_path(c, folder)?;
            let trash = staging(c)?;
            fs::create_dir(&trash)?;
            fs::rename(&old, trash.join(old.file_name().unwrap()))?;
            out.removed = job.requests.iter().map(|r| r.id.clone()).collect();
            out.folder_change = Some((c.id.clone(), folder.clone(), None));
            out.message = "Folder deleted".into();
        }
    }
    Ok(out)
}

impl crate::app::App {
    pub fn start_tree_operation(&mut self, operation: Operation) {
        if self.tree_moving {
            return;
        }
        match prepare(&self.ws, operation) {
            Ok(job) => {
                // Stop requests before deleting their rows; queued saves precede this job.
                if matches!(job.operation, Operation::Delete(_)) {
                    for request in &job.requests {
                        self.cancel(&request.id);
                    }
                }
                self.disk_sync = None;
                self.tree_moving = true;
                self.tree_menu.operating = true;
                self.writer.tree_operation(job);
            }
            Err(e) => self.notice(format!("{e:#}")),
        }
    }
    pub fn finish_tree_operations(&mut self) {
        while let Ok(result) = self.writer.operated.try_recv() {
            self.finish_tree_operation(result);
        }
    }
    pub fn finish_tree_operation(&mut self, result: Result<Outcome, String>) {
        self.finish_saves();
        self.tree_moving = false;
        self.tree_menu.operating = false;
        match result {
            Ok(out) => {
                if let Some((cid, old, new)) = &out.folder_change {
                    let collection = self.ws.collections.iter_mut().find(|c| c.id == *cid);
                    let base = collection.as_ref().map(|c| c.path.clone());
                    if let Some(c) = collection {
                        c.folders.retain_mut(|f| {
                            if !inside(f, old) {
                                return true;
                            }
                            if let Some(new) = new {
                                *f = format!("{new}{}", &f[old.len()..]);
                                true
                            } else {
                                false
                            }
                        });
                    }
                    let ids: Vec<_> = self
                        .ws
                        .collection_drafts
                        .values()
                        .filter(|d| d.collection == *cid && inside(&d.folder, old))
                        .map(|d| d.id.clone())
                        .collect();
                    for key in ids {
                        let mut draft = self.ws.collection_drafts.remove(&key).unwrap();
                        if let Some(new) = new {
                            let folder = format!("{new}{}", &draft.folder[old.len()..]);
                            let new_id = folder_id(cid, &folder);
                            if self.tree_menu.deferred_saves.remove(&key) {
                                self.tree_menu.deferred_saves.insert(new_id.clone());
                            }
                            if draft.folder == *old {
                                draft.name = new.rsplit('/').next().unwrap().into();
                            }
                            if let (Some(path), Some(base)) = (&draft.path, &base) {
                                if let Ok(relative) = path.strip_prefix(base.join(old)) {
                                    draft.path = Some(base.join(new).join(relative));
                                }
                            }
                            draft.folder = folder;
                            draft.id = new_id.clone();
                            for tab in &mut self.ws.open_tabs {
                                if *tab == key {
                                    *tab = new_id.clone();
                                }
                            }
                            if self.ws.active.as_deref() == Some(&key) {
                                self.ws.active = Some(new_id.clone());
                            }
                            if let Some(state) = self.tabs.remove(&key) {
                                self.tabs.insert(new_id.clone(), state);
                            }
                            if let Some(saved) = self.saved.remove(&key) {
                                self.saved.insert(new_id.clone(), saved);
                            }
                            if let Some(saved) = self.ws.saved_fingerprints.remove(&key) {
                                self.ws.saved_fingerprints.insert(new_id.clone(), saved);
                            }
                            self.ws.collection_drafts.insert(new_id, draft);
                        } else {
                            self.close(&key);
                            self.tabs.remove(&key);
                            self.saved.remove(&key);
                            self.ws.saved_fingerprints.remove(&key);
                        }
                    }
                    let prefix = format!("{cid}:");
                    self.collapsed = self
                        .collapsed
                        .drain()
                        .filter_map(|key| {
                            let Some(f) = key.strip_prefix(&prefix) else {
                                return Some(key);
                            };
                            if !inside(f, old) {
                                return Some(key);
                            }
                            new.as_ref()
                                .map(|new| format!("{prefix}{new}{}", &f[old.len()..]))
                        })
                        .collect();
                    if let (Some(cwd), Some(base)) = (&self.terminal_cwd, &base) {
                        if let Ok(relative) = cwd.strip_prefix(base.join(old)) {
                            self.terminal_cwd =
                                Some(new.as_ref().map_or_else(
                                    || base.clone(),
                                    |new| base.join(new).join(relative),
                                ));
                        }
                    }
                }
                for update in out.updated {
                    if let Some(current) = self.ws.requests.iter_mut().find(|r| r.id == update.id) {
                        current.path = update.path;
                        current.folder = update.folder;
                        if out.renamed_request.as_deref() == Some(&current.id) {
                            current.name = update.name;
                        }
                        self.modified_requests.insert(current.id.clone());
                    }
                }
                for id in out.removed {
                    self.close(&id);
                    self.ws.requests.retain(|r| r.id != id);
                    self.tabs.remove(&id);
                    self.saved.remove(&id);
                    self.pending_close.remove(&id);
                    self.ws.saved_fingerprints.remove(&id);
                    self.modified_requests.remove(&id);
                }
                for request in out.added {
                    self.modified_requests.insert(request.id.clone());
                    self.ws.requests.push(request);
                }
                for (cid, folder) in out.folders {
                    if let Some(c) = self.ws.collections.iter_mut().find(|c| c.id == cid) {
                        if !c.folders.contains(&folder) {
                            c.folders.push(folder.clone());
                        }
                    }
                    self.collapsed.remove(&cid);
                    let mut path = folder.as_str();
                    while let Some((parent, _)) = path.rsplit_once('/') {
                        self.collapsed.remove(&format!("{cid}:{parent}"));
                        path = parent;
                    }
                }
                for (id, saved) in out.saved {
                    self.ws
                        .saved_fingerprints
                        .insert(id.clone(), crate::disk_sync::fingerprint(&saved));
                    self.saved.insert(id, saved);
                }
                self.dirty_cache.borrow_mut().clear();
                self.mark_changed();
                self.notice(out.message);
            }
            Err(e) => self.notice(format!("Tree action failed: {e}")),
        }
        self.disk_sync = crate::disk_sync::DiskSync::new(self.context.clone()).ok();
        if let Some(sync) = &mut self.disk_sync {
            sync.watch(&self.ws.collections);
            sync.refresh();
        }
        self.finish_deferred_tree_saves();
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    fn fixture(root: &Path, yaml: bool) -> App {
        let ctx = eframe::egui::Context::default();
        let mut app = App::from_context(&ctx, root.join("profile")).unwrap();
        let c = Collection {
            id: "c".into(),
            name: "Collection".into(),
            path: root.join("collection"),
            folders: vec![
                "Parent".into(),
                "Parent/Nested".into(),
                "Parent/Empty".into(),
            ],
            environments: vec![],
            vars: vec![],
        };
        storage::create_collection(&c).unwrap();
        if yaml {
            fs::write(
                c.path.join("opencollection.yml"),
                "opencollection: 1.0.0\ninfo:\n  name: Collection\n",
            )
            .unwrap();
        }
        for folder in &c.folders {
            fs::create_dir_all(c.path.join(folder)).unwrap();
            storage::save_request(&metadata(&c, folder)).unwrap();
        }
        let r = Request {
            id: "one".into(),
            name: "One".into(),
            collection: "c".into(),
            folder: "Parent/Nested".into(),
            path: Some(
                c.path
                    .join("Parent/Nested")
                    .join(if yaml { "One.yml" } else { "One.req" }),
            ),
            file_format: if yaml { "yml" } else { "req" }.into(),
            body_kind: BodyKind::Json,
            body: "{\"saved\":true}".into(),
            ..Default::default()
        };
        storage::save_request(&r).unwrap();
        fs::write(c.path.join("Parent/Nested/payload.bin"), [0, 255, 1]).unwrap();
        app.ws = Workspace {
            collections: vec![c],
            requests: vec![r.clone()],
            open_tabs: vec![r.id.clone()],
            active: Some(r.id.clone()),
            ..Default::default()
        };
        app.saved.clear();
        app.saved.insert(r.id.clone(), storage::to_bru(&r));
        app.persisted_requests.clear();
        app.modified_requests.insert(r.id.clone());
        app
    }
    fn folder(path: &str) -> Subject {
        Subject::Folder {
            collection: "c".into(),
            folder: path.into(),
        }
    }
    fn run(app: &mut App, operation: Operation) {
        app.start_tree_operation(operation);
        assert!(app.tree_moving, "{}", app.status);
        let outcome = app
            .writer
            .operated
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        assert!(outcome.is_ok(), "{}", outcome.as_ref().err().unwrap());
        app.finish_tree_operation(outcome);
    }
    #[test]
    fn folder_clone_preserves_nested_empty_folders_assets_settings_and_dirty_body_in_bru_and_yaml()
    {
        for yaml in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut app = fixture(root.path(), yaml);
            app.open_folder_settings("c", "Parent");
            app.ws
                .collection_drafts
                .get_mut(&folder_id("c", "Parent"))
                .unwrap()
                .headers
                .push(Pair {
                    key: "X-Folder".into(),
                    value: "draft".into(),
                    enabled: true,
                    ..Default::default()
                });
            app.ws.requests[0].body = "{\"dirty\":true}".into();
            run(&mut app, Operation::Clone(folder("Parent")));
            let c = &app.ws.collections[0];
            assert!(c.path.join("Parent copy/Empty").is_dir());
            assert_eq!(
                fs::read(c.path.join("Parent copy/Nested/payload.bin")).unwrap(),
                [0, 255, 1]
            );
            let clone = app.ws.requests.iter().find(|r| r.id != "one").unwrap();
            assert_eq!(clone.folder, "Parent copy/Nested");
            assert_eq!(
                crate::yaml::read_request(clone.path.as_ref().unwrap())
                    .unwrap()
                    .body,
                "{\"dirty\":true}"
            );
            let meta = metadata(c, "Parent copy");
            let saved = crate::inheritance::parent(meta.path.as_ref().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(saved.headers[0].value, "draft");
            assert_eq!(saved.name, "Parent copy");
            run(&mut app, Operation::Clone(folder("Parent")));
            assert!(app.ws.collections[0].path.join("Parent copy 2").is_dir());
        }
    }
    #[test]
    fn rename_folder_rekeys_open_settings_preserves_late_drafts_and_delete_survives_reload() {
        let root = tempfile::tempdir().unwrap();
        let mut app = fixture(root.path(), false);
        app.open_folder_settings("c", "Parent/Nested");
        let old_tab = folder_id("c", "Parent/Nested");
        app.ws.collection_drafts.get_mut(&old_tab).unwrap().docs = "unsaved folder docs".into();
        app.start_tree_operation(Operation::Rename(folder("Parent"), "Renamed".into()));
        app.ws.requests[0].body = "late request edit".into();
        let folder_draft = app.ws.collection_drafts[&old_tab].clone();
        app.save_tree_draft(&folder_draft);
        let result = app
            .writer
            .operated
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
        app.finish_tree_operation(result);
        let new_tab = folder_id("c", "Renamed/Nested");
        assert_eq!(app.ws.active.as_deref(), Some(new_tab.as_str()));
        assert_eq!(
            app.ws.collection_drafts[&new_tab].docs,
            "unsaved folder docs"
        );
        assert_eq!(app.ws.requests[0].body, "late request edit");
        assert_eq!(app.ws.requests[0].folder, "Renamed/Nested");
        assert!(app.ws.requests[0].path.as_ref().unwrap().exists());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let saved_id = loop {
            let (id, _) = app
                .writer
                .saved
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .unwrap();
            // The request save queued before the rename may finish first.
            if id == new_tab {
                break id;
            }
        };
        assert_eq!(saved_id, new_tab);
        assert_eq!(
            crate::inheritance::parent(app.ws.collection_drafts[&new_tab].path.as_ref().unwrap())
                .unwrap()
                .unwrap()
                .docs,
            "unsaved folder docs"
        );
        assert!(!app.ws.collections[0].path.join("Parent").exists());
        run(&mut app, Operation::Delete(folder("Renamed")));
        assert!(app.ws.requests.is_empty());
        assert!(app.ws.open_tabs.is_empty());
        assert!(app.ws.collection_drafts.is_empty());
        let (_, requests, _) = storage::read_collection(&app.ws.collections[0].path).unwrap();
        assert!(requests.is_empty());
        assert!(app.ws.collections[0].path.join(".reqon").exists());
        app.flush();
        let path = app.root.clone();
        drop(app);
        let restored = storage::load(&path).unwrap();
        assert!(restored.requests.is_empty());
        assert!(restored.open_tabs.is_empty());
    }
    #[test]
    fn request_clone_rename_delete_preserve_identity_drafts_and_sequence_and_wait_for_queued_saves()
    {
        let root = tempfile::tempdir().unwrap();
        let mut app = fixture(root.path(), true);
        let old_path = app.ws.requests[0].path.clone().unwrap();
        app.ws.requests[0].body = "{\"queued\":true}".into();
        app.writer.save_request(app.ws.requests[0].clone());
        run(
            &mut app,
            Operation::Rename(Subject::Request("one".into()), "Renamed".into()),
        );
        assert!(!old_path.exists());
        assert_eq!(app.ws.open_tabs, ["one"]);
        let renamed = app.ws.requests[0].path.clone().unwrap();
        assert_eq!(
            crate::yaml::read_request(&renamed).unwrap().body,
            "{\"queued\":true}"
        );
        run(
            &mut app,
            Operation::Rename(Subject::Request("one".into()), "RENAMED".into()),
        );
        assert_eq!(app.ws.requests[0].name, "RENAMED");
        run(&mut app, Operation::Clone(Subject::Request("one".into())));
        let clone = app
            .ws
            .requests
            .iter()
            .find(|r| r.id != "one")
            .unwrap()
            .clone();
        assert_eq!(clone.name, "RENAMED copy");
        assert!(clone.seq > app.ws.requests[0].seq);
        run(&mut app, Operation::Clone(Subject::Request("one".into())));
        let second = app
            .ws
            .requests
            .iter()
            .find(|r| r.id != "one" && r.id != clone.id)
            .unwrap()
            .clone();
        assert!(second.seq > app.ws.requests[0].seq && second.seq < clone.seq);
        assert_ne!(clone.path, second.path);
        run(&mut app, Operation::Delete(Subject::Request(second.id)));
        run(
            &mut app,
            Operation::Delete(Subject::Request(clone.id.clone())),
        );
        assert!(!clone.path.unwrap().exists());
        assert_eq!(app.ws.requests.len(), 1);
    }
    #[test]
    fn nested_new_folder_and_invalid_paths_collisions_do_not_overwrite_existing_data() {
        let root = tempfile::tempdir().unwrap();
        let mut app = fixture(root.path(), false);
        run(
            &mut app,
            Operation::NewFolder {
                collection: "c".into(),
                parent: "Parent".into(),
                name: "Child".into(),
            },
        );
        assert!(
            app.ws.collections[0]
                .path
                .join("Parent/Child/folder.req")
                .is_file()
        );
        for name in ["../escape", "CON", "bad/name", "."] {
            assert!(valid_name(name).is_err());
        }
        assert!(valid_name("Folder").is_ok());
        assert!(valid_request_name("folder").is_err());
        let before = fs::read(app.ws.requests[0].path.as_ref().unwrap()).unwrap();
        let c = &app.ws.collections[0];
        for target in ["../outside", "", ".reqon"] {
            assert!(checked_path(c, target).is_err());
        }
        assert!(
            execute(
                prepare(
                    &app.ws,
                    Operation::NewFolder {
                        collection: "c".into(),
                        parent: "Parent".into(),
                        name: "Child".into()
                    }
                )
                .unwrap()
            )
            .is_err()
        );
        assert!(
            execute(
                prepare(
                    &app.ws,
                    Operation::Rename(folder("Parent/Nested"), "Child".into())
                )
                .unwrap()
            )
            .is_err()
        );
        assert_eq!(
            fs::read(app.ws.requests[0].path.as_ref().unwrap()).unwrap(),
            before
        );
    }
}
