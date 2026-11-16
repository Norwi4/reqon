//! Reqon disk names, with read compatibility and backed-up migration from Bruno.
use crate::model::{Request, Workspace};
use anyhow::{Context, Result, ensure};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn is_source(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("req") || e.eq_ignore_ascii_case("bru"))
}
pub fn is_legacy(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("bru"))
}
pub fn manifest(root: &Path) -> PathBuf {
    let native = root.join("reqon.json");
    if native.exists() || !root.join("bruno.json").exists() {
        native
    } else {
        root.join("bruno.json")
    }
}
pub fn is_collection(root: &Path) -> bool {
    manifest(root).is_file() || root.join("opencollection.yml").is_file()
}
pub fn source_path(path: &Path) -> PathBuf {
    if path.exists() {
        return path.to_owned();
    }
    if is_legacy(path) {
        return path.with_extension("req");
    }
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("req"))
    {
        let old = path.with_extension("bru");
        if old.exists() {
            return old;
        }
    }
    path.to_owned()
}
/// Prefer .req over a legacy file with the same stem, avoiding duplicate rows.
pub fn readable_source(path: &Path) -> bool {
    is_source(path) && !(is_legacy(path) && path.with_extension("req").exists())
}

pub fn migrate_collection(root: &Path) -> Result<usize> {
    ensure!(root.is_dir(), "Collection directory is unavailable");
    let mut plan = Vec::<(PathBuf, PathBuf)>::new();
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        plan: &mut Vec<(PathBuf, PathBuf)>,
    ) -> Result<()> {
        ensure!(depth <= 32, "Collection nesting exceeds 32 levels");
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            if [".git", ".reqon", ".trash", "node_modules"]
                .iter()
                .any(|n| name.eq_ignore_ascii_case(n))
            {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                walk(root, &path, depth + 1, plan)?;
            } else if is_legacy(&path) {
                plan.push((path.clone(), path.with_extension("req")));
            } else if dir == root && name.eq_ignore_ascii_case("bruno.json") {
                plan.push((path, root.join("reqon.json")));
            }
        }
        Ok(())
    }
    walk(root, root, 0, &mut plan)?;
    if plan.is_empty() {
        return Ok(0);
    }
    plan.sort_by_key(|(old, _)| {
        old.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("bruno.json"))
    });
    let mut targets = std::collections::HashSet::new();
    for (old, new) in &plan {
        crate::tree_move::within_roots(old, &[root.to_owned()])?;
        crate::tree_move::within_roots(new, &[root.to_owned()])?;
        ensure!(
            targets.insert(new.to_string_lossy().to_lowercase()),
            "Multiple files map to {}",
            new.display()
        );
        if new.exists() {
            ensure!(
                new.is_file() && fs::read(old)? == fs::read(new)?,
                "Cannot migrate: {} already contains different data",
                new.display()
            );
        }
    }
    let backup = root
        .join(".reqon")
        .join(format!("native-format-backup-{}", crate::model::id()));
    crate::tree_move::within_roots(&backup, &[root.to_owned()])?;
    // Complete every backup before changing any source path.
    for (old, _) in &plan {
        let dest = backup.join(old.strip_prefix(root)?);
        fs::create_dir_all(dest.parent().unwrap())?;
        fs::copy(old, dest).with_context(|| format!("Cannot back up {}", old.display()))?;
    }
    let mut completed = Vec::<(PathBuf, PathBuf, bool)>::new();
    for (old, new) in &plan {
        let existed = new.exists();
        let result = (|| -> Result<()> {
            let bytes = fs::read(old)?;
            ensure!(
                bytes == fs::read(backup.join(old.strip_prefix(root)?))?,
                "Source changed during migration: {}",
                old.display()
            );
            if existed {
                ensure!(
                    bytes == fs::read(new)?,
                    "Destination changed during migration: {}",
                    new.display()
                );
                fs::remove_file(old)?;
            } else {
                fs::rename(old, new)?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback_errors = vec![];
            for (old, new, existed) in completed.into_iter().rev() {
                let restored = if existed {
                    fs::copy(backup.join(old.strip_prefix(root)?), &old).map(|_| ())
                } else {
                    fs::rename(&new, &old)
                };
                if let Err(e) = restored {
                    rollback_errors.push(e.to_string());
                }
            }
            anyhow::bail!(
                "Migration failed: {error}. Backup: {}. {}",
                backup.display(),
                rollback_errors.join("; ")
            );
        }
        completed.push((old.clone(), new.clone(), existed));
    }
    Ok(plan.len())
}

pub fn migrate_workspace(ws: &mut Workspace) -> (bool, Vec<String>) {
    let mut changed = false;
    let mut warnings = vec![];
    for collection in &ws.collections {
        if !is_collection(&collection.path) {
            continue;
        }
        match migrate_collection(&collection.path) {
            Ok(count) => changed |= count > 0,
            Err(error) => {
                warnings.push(format!("{}: {error:#}", collection.name));
                continue;
            }
        }
        let update = |r: &mut Request| -> bool {
            let mut changed = false;
            if r.collection == collection.id {
                if r.file_format == "bru" {
                    r.file_format = "req".into();
                    changed = true;
                }
                if let Some(path) = &mut r.path {
                    if path.starts_with(&collection.path) && is_legacy(path) {
                        path.set_extension("req");
                        changed = true;
                    }
                }
            }
            changed
        };
        for r in &mut ws.requests {
            changed |= update(r);
        }
        for r in ws.collection_drafts.values_mut() {
            changed |= update(r);
        }
        for h in &mut ws.history {
            changed |= update(&mut h.request);
        }
        for tab in &mut ws.open_tabs {
            let old = tab.clone();
            let replacement = crate::file_ids::parse_id(&old).and_then(|(cid, relative)| {
                if cid != collection.id {
                    return None;
                }
                let next = if relative == "bruno.json" {
                    PathBuf::from("reqon.json")
                } else if is_legacy(Path::new(relative)) {
                    Path::new(relative).with_extension("req")
                } else {
                    return None;
                };
                if !collection.path.join(&next).is_file() {
                    return None;
                }
                Some(crate::file_ids::file_id(
                    cid,
                    &next.to_string_lossy().replace('\\', "/"),
                ))
            });
            if let Some(next) = replacement {
                *tab = next;
                if ws.active.as_deref() == Some(&old) {
                    ws.active = Some(tab.clone());
                }
                changed = true;
            }
        }
    }
    (changed, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn legacy(root: &Path) {
        fs::create_dir_all(root.join("Folder")).unwrap();
        fs::create_dir_all(root.join("environments")).unwrap();
        fs::write(
            root.join("bruno.json"),
            r#"{"version":"1","name":"Legacy","unknown":{"keep":true}}"#,
        )
        .unwrap();
        fs::write(
            root.join("collection.bru"),
            "headers {\n X-Collection: inherited\n}\n",
        )
        .unwrap();
        fs::write(
            root.join("Folder/folder.bru"),
            "auth {\n mode: bearer\n}\nauth:bearer {\n token: inherited-token\n}\n",
        )
        .unwrap();
        fs::write(root.join("Folder/Request.bru"), "meta {\n name: Request\n type: http\n seq: 1\n}\nget {\n url: http://localhost\n auth: inherit\n body: json\n}\nbody:json {\n // comment\n {\"value\":7}\n}\ncustom {\n preserve: yes\n}\n").unwrap();
        fs::write(
            root.join("environments/Local.bru"),
            "vars {\n baseUrl: http://localhost\n}\nvars:secret [\n TOKEN\n]\n",
        )
        .unwrap();
    }
    #[test]
    fn legacy_collection_migrates_byte_for_byte_and_reopens_with_inheritance() {
        let dir = tempfile::tempdir().unwrap();
        legacy(dir.path());
        let (_, before, warnings) = crate::storage::read_collection(dir.path()).unwrap();
        assert_eq!(before.len(), 1);
        assert!(warnings.is_empty());
        let old = fs::read(dir.path().join("Folder/Request.bru")).unwrap();
        let (collection, requests, warnings) = crate::storage::open_collection(dir.path()).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].file_format, "req");
        assert_eq!(
            fs::read(dir.path().join("Folder/Request.req")).unwrap(),
            old
        );
        assert!(!dir.path().join("bruno.json").exists());
        assert!(!dir.path().join("Folder/Request.bru").exists());
        let config: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.path().join("reqon.json")).unwrap()).unwrap();
        assert_eq!(config["unknown"]["keep"], true);
        assert!(
            collection.environments[0]
                .vars
                .iter()
                .any(|v| v.key == "TOKEN" && v.secret)
        );
        let prepared = crate::inheritance::prepare(requests[0].clone()).unwrap();
        assert_eq!(prepared.auth.token, "inherited-token");
        assert!(
            prepared
                .headers
                .iter()
                .any(|v| v.key == "X-Collection" && v.value == "inherited")
        );
        let backup = fs::read_dir(dir.path().join(".reqon"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(backup.join("Folder/Request.bru")).unwrap(), old);
        assert!(backup.join("bruno.json").is_file());
        assert_eq!(migrate_collection(dir.path()).unwrap(), 0);
        assert_eq!(
            crate::yaml::read_request(&dir.path().join("Folder/Request.bru"))
                .unwrap()
                .body,
            requests[0].body
        );
    }
    #[test]
    fn migration_preflights_conflicts_without_changing_any_source() {
        let dir = tempfile::tempdir().unwrap();
        legacy(dir.path());
        fs::write(dir.path().join("Folder/Request.req"), "different data").unwrap();
        assert!(migrate_collection(dir.path()).is_err());
        assert!(dir.path().join("bruno.json").is_file());
        assert!(dir.path().join("collection.bru").is_file());
        assert!(!dir.path().join("collection.req").exists());
        assert!(!dir.path().join(".reqon").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("Folder/Request.req")).unwrap(),
            "different data"
        );
    }
    #[cfg(feature = "desktop")]
    #[test]
    fn startup_migration_preserves_drafts_ids_tabs_and_saved_request_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("collection");
        legacy(&root);
        let (collection, mut requests, _) = crate::storage::read_collection(&root).unwrap();
        let saved = crate::storage::to_bru(&requests[0]);
        requests[0].body = "unsaved draft with // comments".into();
        let id = requests[0].id.clone();
        let file_tab = crate::file_ids::file_id(&collection.id, "bruno.json");
        let mut ws = Workspace::default();
        ws.collections.push(collection.clone());
        ws.requests = requests;
        ws.open_tabs = vec![
            id.clone(),
            file_tab.clone(),
            crate::file_ids::file_id(&collection.id, "Folder/Request.bru"),
        ];
        ws.active = Some(file_tab);
        ws.saved_fingerprints
            .insert(id.clone(), crate::disk_sync::fingerprint(&saved));
        crate::storage::save(dir.path(), &ws).unwrap();
        let ctx = eframe::egui::Context::default();
        let app = crate::app::App::from_context(&ctx, dir.path().into()).unwrap();
        assert_eq!(app.ws.requests[0].id, id);
        assert_eq!(app.ws.requests[0].body, "unsaved draft with // comments");
        assert_eq!(
            app.ws.requests[0]
                .path
                .as_ref()
                .unwrap()
                .extension()
                .unwrap(),
            "req"
        );
        assert_eq!(app.ws.open_tabs[0], id);
        assert_eq!(
            app.ws.open_tabs[2],
            crate::file_ids::file_id(&collection.id, "Folder/Request.req")
        );
        assert_eq!(
            app.ws.active,
            Some(crate::file_ids::file_id(&collection.id, "reqon.json"))
        );
        assert_eq!(
            crate::storage::to_bru(
                &crate::yaml::read_request(&root.join("Folder/Request.req")).unwrap()
            ),
            saved
        );
        let loaded = crate::storage::load(dir.path()).unwrap();
        assert_eq!(loaded.requests[0].body, app.ws.requests[0].body);
        assert_eq!(loaded.requests[0].path, app.ws.requests[0].path);
    }
    #[test]
    fn old_feature_setting_is_read_but_saved_under_reqon_name() {
        let features: crate::preference_model::Features =
            serde_json::from_str(r#"{"bruno_json":true}"#).unwrap();
        assert!(features.reqon_json);
        let json = serde_json::to_value(features).unwrap();
        assert_eq!(json["reqon_json"], true);
        assert!(json.get("bruno_json").is_none());
    }
}
