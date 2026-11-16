//! Request-tree moves. Plans keep identity/drafts; disk changes run on the writer
//! after pending saves, and roll back together if any filesystem operation fails.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub enum Target {
    Inside { collection: String, folder: String },
    Before(String),
    After(String),
    Group(String),
}
#[derive(Clone)]
pub struct Change {
    pub before: Request,
    pub after: Request,
}
pub struct Plan {
    pub changes: Vec<Change>,
    pub folder: Option<Request>,
    pub roots: Vec<PathBuf>,
}
pub struct Outcome {
    pub plan: Plan,
    pub saved: Vec<(String, String)>,
}

fn safe_folder(folder: &str) -> bool {
    folder.is_empty()
        || folder
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(['\\', ':']))
}
fn unique_path(
    ws: &Workspace,
    root: &Path,
    folder: &str,
    r: &Request,
    used: &mut HashSet<PathBuf>,
    yaml: bool,
) -> PathBuf {
    let stem = safe_name(&r.name);
    let extension = if yaml { "yml" } else { "req" };
    let base = root.join(folder);
    let mut path = base.join(format!("{stem}.{extension}"));
    let mut suffix = 2;
    while path.exists()
        || used.contains(&path)
        || ws
            .requests
            .iter()
            .any(|q| q.id != r.id && q.path.as_ref() == Some(&path))
    {
        path = base.join(format!("{stem} {suffix}.{extension}"));
        suffix += 1;
    }
    used.insert(path.clone());
    path
}

pub fn plan(ws: &Workspace, source: &str, target: Target) -> Result<Plan> {
    let source = ws
        .requests
        .iter()
        .find(|r| r.id == source)
        .context("Request no longer exists")?;
    let reference = match &target {
        Target::Inside { .. } => None,
        Target::Before(id) | Target::After(id) | Target::Group(id) => {
            ensure!(*id != source.id, "Cannot drop a request onto itself");
            Some(
                ws.requests
                    .iter()
                    .find(|r| r.id == *id)
                    .context("Destination request no longer exists")?,
            )
        }
    };
    let (cid, parent) = match &target {
        Target::Inside { collection, folder } => (collection.as_str(), folder.as_str()),
        _ => {
            let r = reference.unwrap();
            (r.collection.as_str(), r.folder.as_str())
        }
    };
    let collection = ws
        .collections
        .iter()
        .find(|c| c.id == cid)
        .context("Choose a collection as the destination")?;
    ensure!(safe_folder(parent), "Invalid destination folder");
    let yaml = collection.path.join("opencollection.yml").is_file();
    let mut folder = None;
    let destination = if matches!(target, Target::Group(_)) {
        let base = if parent.is_empty() {
            "New folder".into()
        } else {
            format!("{parent}/New folder")
        };
        let mut name = base.clone();
        let mut suffix = 2;
        while collection.path.join(&name).exists()
            || collection.folders.contains(&name)
            || ws
                .requests
                .iter()
                .any(|r| r.collection == cid && r.folder == name)
        {
            name = format!("{base} {suffix}");
            suffix += 1;
        }
        folder = Some(Request {
            protocol: "folder".into(),
            name: name.rsplit('/').next().unwrap().into(),
            collection: cid.into(),
            folder: name.clone(),
            path: Some(collection.path.join(&name).join(if yaml {
                "folder.yml"
            } else {
                "folder.req"
            })),
            file_format: if yaml { "yml" } else { "req" }.into(),
            auth: Auth {
                kind: "inherit".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        name
    } else {
        parent.into()
    };
    let mut order = ws
        .requests
        .iter()
        .filter(|r| r.collection == cid && r.folder == destination && r.id != source.id)
        .collect::<Vec<_>>();
    order.sort_by(|a, b| a.seq.total_cmp(&b.seq));
    match &target {
        Target::Group(_) => {
            order.push(reference.unwrap());
            order.push(source);
        }
        Target::Before(_) | Target::After(_) => {
            let at = order
                .iter()
                .position(|r| r.id == reference.unwrap().id)
                .unwrap();
            order.insert(at + usize::from(matches!(target, Target::After(_))), source);
        }
        Target::Inside { .. } => order.push(source),
    }
    // Most drops update one file. Renumber siblings only if no float gap remains.
    let at = order.iter().position(|r| r.id == source.id).unwrap();
    let previous = at.checked_sub(1).map(|i| order[i].seq);
    let next = order.get(at + 1).map(|r| r.seq);
    let sequence = match (previous, next) {
        (Some(a), Some(b)) => a + (b - a) / 2.,
        (Some(a), None) => a + 1.,
        (None, Some(b)) => b - 1.,
        (None, None) => 1.,
    };
    let renumber = matches!(target, Target::Group(_))
        || !sequence.is_finite()
        || previous.is_some_and(|v| sequence <= v)
        || next.is_some_and(|v| sequence >= v);
    let mut changes = vec![];
    let mut used = HashSet::new();
    for (i, before) in order.into_iter().enumerate() {
        if !renumber && before.id != source.id {
            continue;
        }
        let mut after = before.clone();
        after.seq = if renumber { (i + 1) as f64 } else { sequence };
        if before.collection != cid || before.folder != destination || before.path.is_none() {
            let path = unique_path(ws, &collection.path, &destination, before, &mut used, yaml);
            after.collection = cid.into();
            after.folder = destination.clone();
            after.file_format = if yaml { "yml" } else { "req" }.into();
            after.path = Some(path);
        }
        if after.path != before.path || after.seq != before.seq {
            changes.push(Change {
                before: before.clone(),
                after,
            });
        }
    }
    let roots = ws
        .collections
        .iter()
        .filter(|c| c.id == cid || changes.iter().any(|x| x.before.collection == c.id))
        .map(|c| c.path.clone())
        .collect();
    Ok(Plan {
        changes,
        folder,
        roots,
    })
}

/// Keep file references attached to their original files when their request moves.
fn rebase(r: &mut Request, old: Option<&Path>, new: Option<&Path>) {
    if old.and_then(Path::parent) == new.and_then(Path::parent) {
        return;
    }
    let Some(base) = old.and_then(Path::parent) else {
        return;
    };
    fn path(value: &mut String, base: &Path) {
        if !value.is_empty() && !value.contains("{{") && Path::new(value).is_relative() {
            *value = base.join(&*value).to_string_lossy().into_owned();
        }
    }
    fn body(kind: &BodyKind, value: &mut String, form: &mut [Pair], base: &Path) {
        if *kind == BodyKind::Binary {
            path(value, base);
        }
        for row in form {
            if row.kind == "file" || (row.kind != "text" && row.value.starts_with('@')) {
                let mut file = row.value.strip_prefix('@').unwrap_or(&row.value).to_owned();
                path(&mut file, base);
                row.value = format!("@{file}");
            }
        }
    }
    body(&r.body_kind, &mut r.body, &mut r.form, base);
    for value in r
        .proto_files
        .iter_mut()
        .chain(r.proto_import_paths.iter_mut())
        .chain([
            &mut r.ca_certificate,
            &mut r.client_certificate,
            &mut r.client_key,
        ])
    {
        path(value, base);
    }
    for draft in r.protocol_drafts.values_mut() {
        body(&draft.body_kind, &mut draft.body, &mut draft.form, base);
        for file in &mut draft.proto_files {
            path(file, base);
        }
    }
}

pub fn relocate(r: &mut Request, change: &Change) {
    rebase(
        r,
        change.before.path.as_deref(),
        change.after.path.as_deref(),
    );
    r.path = change.after.path.clone();
    r.collection = change.after.collection.clone();
    r.folder = change.after.folder.clone();
    r.seq = change.after.seq;
    r.file_format = change.after.file_format.clone();
}

pub(crate) fn within_roots(path: &Path, roots: &[PathBuf]) -> Result<()> {
    ensure!(
        path.is_absolute()
            && !path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "Invalid collection path"
    );
    let ancestor = path
        .ancestors()
        .find(|p| p.exists())
        .context("Missing destination directory")?
        .canonicalize()?;
    ensure!(
        roots.iter().any(|root| root
            .canonicalize()
            .is_ok_and(|root| ancestor.starts_with(root))),
        "Request path is outside the collection"
    );
    Ok(())
}

pub fn execute(plan: Plan) -> Result<Outcome> {
    let mut writes = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut old_paths = HashSet::new();
    let mut saved = vec![];
    for change in &plan.changes {
        let mut disk = if let Some(old) = change.before.path.as_ref().filter(|p| p.exists()) {
            within_roots(old, &plan.roots)?;
            old_paths.insert(old.clone());
            crate::yaml::read_request(old)
                .with_context(|| format!("Cannot move {}", old.display()))?
        } else {
            change.before.clone()
        };
        relocate(&mut disk, change);
        let path = disk.path.as_ref().context("Missing destination path")?;
        within_roots(path, &plan.roots)?;
        if change.before.path.as_ref() != Some(path) {
            ensure!(
                !path.exists(),
                "Destination already exists: {}",
                path.display()
            );
        }
        writes.insert(path.clone(), storage::source(&disk)?.into_bytes());
        saved.push((change.before.id.clone(), storage::to_bru(&disk)));
    }
    if let Some(folder) = &plan.folder {
        let path = folder.path.as_ref().unwrap();
        within_roots(path, &plan.roots)?;
        ensure!(
            !path.parent().unwrap().exists(),
            "Destination folder already exists"
        );
        writes.insert(path.clone(), storage::source(folder)?.into_bytes());
    }
    let mut backups = BTreeMap::new();
    for path in old_paths.iter().chain(writes.keys()) {
        backups.insert(
            path.clone(),
            if path.exists() {
                Some(fs::read(path)?)
            } else {
                None
            },
        );
    }
    let mut created_dirs = vec![];
    let mut touched = vec![];
    let result = (|| -> Result<()> {
        for (path, bytes) in &writes {
            let parent = path.parent().unwrap();
            let missing = parent
                .ancestors()
                .take_while(|p| !p.exists())
                .map(Path::to_path_buf)
                .collect::<Vec<_>>();
            for dir in missing.iter().rev() {
                fs::create_dir(dir)?;
                created_dirs.push(dir.clone());
            }
            if backups[path].is_some() {
                touched.push(path.clone());
                storage::atomic_write(path, bytes)?;
            } else {
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)?;
                touched.push(path.clone());
                file.write_all(bytes)?;
                file.sync_all()?;
            }
        }
        for old in &old_paths {
            if !writes.contains_key(old) {
                fs::remove_file(old)?;
                touched.push(old.clone());
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_errors = vec![];
        for path in touched.into_iter().rev() {
            let restored = match &backups[&path] {
                Some(bytes) => storage::atomic_write(&path, bytes),
                None => fs::remove_file(&path)
                    .or_else(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            Ok(())
                        } else {
                            Err(e)
                        }
                    })
                    .map_err(Into::into),
            };
            if let Err(e) = restored {
                rollback_errors.push(format!("{}: {e}", path.display()));
            }
        }
        for dir in created_dirs.into_iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        ensure!(
            rollback_errors.is_empty(),
            "Move failed: {error:#}; recovery errors: {}",
            rollback_errors.join("; ")
        );
        return Err(error.context("Move cancelled; original files restored"));
    }
    Ok(Outcome { plan, saved })
}

pub fn apply(ws: &mut Workspace, outcome: &Outcome) -> Vec<String> {
    let mut modified = vec![];
    for change in &outcome.plan.changes {
        if let Some(r) = ws.requests.iter_mut().find(|r| r.id == change.before.id) {
            relocate(r, change);
            modified.push(r.id.clone());
        }
    }
    if let Some(folder) = &outcome.plan.folder {
        if let Some(c) = ws
            .collections
            .iter_mut()
            .find(|c| c.id == folder.collection)
        {
            c.folders.push(folder.folder.clone());
        }
    }
    modified
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> Workspace {
        let mut ws = Workspace::default();
        for name in ["A", "B"] {
            let c = Collection {
                id: name.into(),
                name: name.into(),
                path: root.join(name),
                folders: vec![],
                environments: vec![],
                vars: vec![],
            };
            storage::create_collection(&c).unwrap();
            ws.collections.push(c);
        }
        for (name, seq) in [("One", 1.), ("Two", 2.), ("Three", 3.)] {
            let r = Request {
                id: name.into(),
                name: name.into(),
                collection: "A".into(),
                seq,
                path: Some(root.join("A").join(format!("{name}.req"))),
                body_kind: BodyKind::Json,
                body: "{\"saved\":true}".into(),
                ..Default::default()
            };
            storage::save_request(&r).unwrap();
            ws.requests.push(r);
        }
        ws.open_tabs = vec!["One".into(), "Two".into()];
        ws.active = Some("One".into());
        ws
    }
    fn move_to(ws: &mut Workspace, id: &str, target: Target) -> Outcome {
        let result = execute(plan(ws, id, target).unwrap()).unwrap();
        apply(ws, &result);
        result
    }
    #[test]
    fn reorder_and_group_preserve_tabs_dirty_drafts_and_saved_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut ws = fixture(temp.path());
        ws.requests[0].body = "unsaved before drag".into();
        move_to(&mut ws, "Three", Target::Before("One".into()));
        let (_, disk, _) = storage::read_collection(&ws.collections[0].path).unwrap();
        assert!(
            disk.iter().find(|r| r.name == "Three").unwrap().seq
                < disk.iter().find(|r| r.name == "One").unwrap().seq
        );
        let planned = plan(&ws, "One", Target::Group("Two".into())).unwrap();
        ws.requests[0].body = "edited while disk operation is running".into();
        let outcome = execute(planned).unwrap();
        apply(&mut ws, &outcome);
        assert_eq!(ws.open_tabs, ["One", "Two"]);
        assert_eq!(
            ws.requests[0].body,
            "edited while disk operation is running"
        );
        assert_eq!(ws.requests[0].folder, "New folder");
        assert!(!temp.path().join("A/One.req").exists());
        let disk = crate::yaml::read_request(ws.requests[0].path.as_ref().unwrap()).unwrap();
        assert_eq!(disk.body, "{\"saved\":true}");
        let folder = crate::inheritance::parent(&temp.path().join("A/New folder/folder.req"))
            .unwrap()
            .unwrap();
        assert_eq!(folder.auth.kind, "inherit");
        let mut saved = outcome.saved.into_iter().collect();
        let (_, disk, _) = storage::read_collection(&ws.collections[0].path).unwrap();
        crate::disk_sync::merge_requests(&mut ws, "A", disk, &mut saved);
        assert_eq!(ws.requests.len(), 3);
        assert_eq!(ws.open_tabs, ["One", "Two"]);
        assert_eq!(
            ws.requests.iter().find(|r| r.id == "One").unwrap().body,
            "edited while disk operation is running"
        );
    }
    #[test]
    fn cross_collection_moves_convert_yaml_resolve_filename_collisions_and_rebase_files() {
        let temp = tempfile::tempdir().unwrap();
        let mut ws = fixture(temp.path());
        let target = &ws.collections[1];
        storage::atomic_write(
            &target.path.join("opencollection.yml"),
            "opencollection: 1.0.0\ninfo:\n  name: B\n",
        )
        .unwrap();
        fs::create_dir(target.path.join("New folder")).unwrap();
        let mut other = ws.requests[0].clone();
        other.id = "Other".into();
        other.collection = "B".into();
        other.path = Some(target.path.join("One.yml"));
        other.file_format = "yml".into();
        storage::save_request(&other).unwrap();
        ws.requests.push(other);
        let old = ws.requests[0].path.clone().unwrap();
        ws.requests[0].body_kind = BodyKind::Binary;
        ws.requests[0].body = "payload.bin".into();
        ws.requests[0].proto_files = vec!["api.proto".into()];
        storage::save_request(&ws.requests[0]).unwrap();
        let result = move_to(&mut ws, "One", Target::Group("Other".into()));
        assert_eq!(result.plan.folder.unwrap().folder, "New folder 2");
        assert_eq!(ws.requests[0].collection, "B");
        assert_ne!(ws.requests[0].path, ws.requests[3].path);
        assert_eq!(
            ws.requests[0].body,
            old.parent().unwrap().join("payload.bin").to_string_lossy()
        );
        assert_eq!(
            ws.requests[0].proto_files[0],
            old.parent().unwrap().join("api.proto").to_string_lossy()
        );
        let (_, requests, warnings) = storage::read_collection(&ws.collections[1].path).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r.file_format == "yml"));
        assert!(!old.exists());
    }
    #[test]
    fn partial_write_failure_restores_every_original_and_rejects_escaped_destinations() {
        let temp = tempfile::tempdir().unwrap();
        let ws = fixture(temp.path());
        let mut plan = plan(&ws, "One", Target::Group("Two".into())).unwrap();
        let blocked = temp.path().join("B/blocked");
        fs::write(&blocked, "unrelated").unwrap();
        plan.changes[1].after.path = Some(blocked.join("Fail.req"));
        let first = plan.changes[0].after.path.clone().unwrap();
        assert!(execute(plan).is_err());
        assert!(!first.exists());
        assert!(temp.path().join("A/One.req").exists());
        assert!(temp.path().join("A/Two.req").exists());
        assert_eq!(fs::read_to_string(&blocked).unwrap(), "unrelated");
        assert!(
            super::plan(
                &ws,
                "One",
                Target::Inside {
                    collection: "B".into(),
                    folder: "../escape".into()
                }
            )
            .is_err()
        );
        assert!(super::plan(&ws, "One", Target::Group("One".into())).is_err());
    }
    #[cfg(feature = "desktop")]
    #[test]
    fn queued_save_finishes_before_move_and_new_folder_opens_in_main_pane() {
        let temp = tempfile::tempdir().unwrap();
        let ws = fixture(temp.path());
        storage::save(temp.path(), &ws).unwrap();
        let mut app = crate::app::App::from_context(
            &eframe::egui::Context::default(),
            temp.path().to_owned(),
        )
        .unwrap();
        app.ws.requests[0].body = "{\"queued\":true}".into();
        app.writer.save_request(app.ws.requests[0].clone());
        app.move_tree_request("One".into(), Target::Group("Two".into()));
        let outcome = app
            .writer
            .moved
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        app.finish_tree_move(outcome);
        assert_eq!(app.ws.active.as_deref(), Some("__folder__A|New folder"));
        assert!(
            app.ws
                .collection_drafts
                .contains_key("__folder__A|New folder")
        );
        let request = app.ws.requests.iter().find(|r| r.id == "One").unwrap();
        assert_eq!(
            crate::yaml::read_request(request.path.as_ref().unwrap())
                .unwrap()
                .body,
            "{\"queued\":true}"
        );
        assert!(!temp.path().join("A/One.req").exists());
        drop(app);
        let restored = storage::load(temp.path()).unwrap();
        assert_eq!(
            restored
                .requests
                .iter()
                .find(|r| r.id == "One")
                .unwrap()
                .folder,
            "New folder"
        );
    }
}
