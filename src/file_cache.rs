//! An optional cache of parsed on-disk collections. Drafts are never cached here.
use crate::model::{Collection, Request};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};
type Loaded = (Collection, Vec<Request>, Vec<String>);
#[derive(Default)]
struct Config {
    directory: PathBuf,
    enabled: bool,
}
fn config() -> &'static Mutex<Config> {
    static C: OnceLock<Mutex<Config>> = OnceLock::new();
    C.get_or_init(Default::default)
}
pub fn configure(root: &Path, enabled: bool) {
    *config().lock().unwrap() = Config {
        directory: root.join("cache/collections"),
        enabled,
    };
}
#[derive(Serialize, Deserialize)]
struct Entry {
    version: u8,
    stamp: String,
    data: Loaded,
}
fn stamp(path: &Path) -> Result<String> {
    fn walk(root: &Path, path: &Path, out: &mut Vec<String>) -> Result<()> {
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if matches!(name.as_ref(), ".git" | "node_modules" | ".trash") {
                continue;
            }
            if entry.file_type()?.is_symlink() {
                continue;
            }
            let meta = entry.metadata()?;
            if meta.is_dir() {
                walk(root, &entry.path(), out)?;
            } else {
                out.push(format!(
                    "{}:{}:{:?}",
                    entry.path().strip_prefix(root)?.display(),
                    meta.len(),
                    meta.modified()?
                ));
            }
        }
        Ok(())
    }
    let mut rows = vec![];
    walk(path, path, &mut rows)?;
    rows.sort();
    Ok(crate::disk_sync::fingerprint(&rows.join("\n")))
}
pub fn read(path: &Path, load: impl FnOnce() -> Result<Loaded>) -> Result<Loaded> {
    let (enabled, dir) = {
        let c = config().lock().unwrap();
        (c.enabled, c.directory.clone())
    };
    if !enabled {
        return load();
    }
    read_cached(path, &dir, load)
}
fn read_cached(path: &Path, dir: &Path, load: impl FnOnce() -> Result<Loaded>) -> Result<Loaded> {
    let before = stamp(path).ok();
    let cache = dir.join(format!(
        "{}.rqcache",
        crate::disk_sync::fingerprint(&path.to_string_lossy())
    ));
    if let Some(stamp) = &before {
        if let Ok(bytes) = std::fs::read(&cache) {
            if let Ok(plain) = crate::secrets::unprotect(&bytes) {
                if let Ok(entry) = serde_json::from_slice::<Entry>(&plain) {
                    if entry.version == 2 && entry.stamp == *stamp {
                        return Ok(entry.data);
                    }
                }
            }
        }
    }
    let data = load()?;
    // Pair deliberately omits plaintext secrets from serialization. Cache only
    // protected references or empty secret declarations; keep legacy data live.
    if data.0.environments.iter().flat_map(|e| &e.vars).any(|p| {
        p.secret && !p.value.is_empty() && crate::auth_vault::reference_id(&p.value).is_none()
    }) {
        return Ok(data);
    }
    if let Some(before) = before {
        if stamp(path).ok().as_ref() == Some(&before) {
            let entry = Entry {
                version: 2,
                stamp: before,
                data: data.clone(),
            };
            if let Ok(raw) = serde_json::to_vec(&entry) {
                if let Ok(encrypted) = crate::secrets::protect(&raw) {
                    let _ = crate::storage::atomic_write(&cache, encrypted);
                }
            }
        }
    }
    Ok(data)
}
pub fn size() -> u64 {
    let dir = config().lock().unwrap().directory.clone();
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|s| s == "rqcache"))
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum()
}
pub fn clear() {
    let dir = config().lock().unwrap().directory.clone();
    clear_directory(&dir);
}
fn clear_directory(dir: &Path) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_type().is_ok_and(|t| t.is_file())
                && e.path().extension().is_some_and(|s| s == "rqcache")
            {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_environment_references_keep_secret_markers_and_resolve_after_cache_hit() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("collection");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("reqon.json"), "{}").unwrap();
        let reference = crate::auth_vault::store_at(
            &crate::auth_vault::root().unwrap(),
            "cached-env-private-842930",
        )
        .unwrap();
        let row = crate::model::Pair {
            secret: true,
            ..crate::model::Pair::new("TOKEN", &reference)
        };
        let collection = crate::model::Collection {
            id: "cache-env".into(),
            name: "Cache env".into(),
            path: root.clone(),
            environments: vec![crate::model::Environment {
                name: "Local".into(),
                vars: vec![row],
            }],
            vars: vec![],
            folders: vec![],
        };
        read_cached(&root, &dir.path().join("cache"), || {
            Ok((collection, vec![], vec![]))
        })
        .unwrap();
        let (mut cached, _, _) = read_cached(&root, &dir.path().join("cache"), || {
            panic!("expected cache hit")
        })
        .unwrap();
        assert!(cached.environments[0].vars[0].secret);
        assert_eq!(cached.environments[0].vars[0].value, reference);
        crate::environment_vault::resolve_rows(&mut cached.environments[0].vars, true).unwrap();
        assert_eq!(
            cached.environments[0].vars[0].value,
            "cached-env-private-842930"
        );
    }
    #[test]
    fn cache_reuses_parsing_invalidates_changes_and_never_clears_source_files() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("collection");
        let cache = temp.path().join("cache");
        std::fs::create_dir(&source).unwrap();
        let file = source.join("request.req");
        std::fs::write(&file, "first").unwrap();
        let calls = std::cell::Cell::new(0);
        let load = || {
            calls.set(calls.get() + 1);
            Ok((
                Collection {
                    id: "fixture".into(),
                    name: "Fixture".into(),
                    path: source.clone(),
                    folders: vec![],
                    environments: vec![],
                    vars: vec![],
                },
                vec![Request {
                    body: std::fs::read_to_string(&file).unwrap(),
                    ..Default::default()
                }],
                vec![],
            ))
        };
        assert_eq!(
            read_cached(&source, &cache, load).unwrap().1[0].body,
            "first"
        );
        assert_eq!(
            read_cached(&source, &cache, load).unwrap().1[0].body,
            "first"
        );
        assert_eq!(calls.get(), 1);
        std::fs::write(&file, "second version").unwrap();
        assert_eq!(
            read_cached(&source, &cache, load).unwrap().1[0].body,
            "second version"
        );
        assert_eq!(calls.get(), 2);
        std::fs::write(cache.join("keep.txt"), "keep").unwrap();
        clear_directory(&cache);
        assert_eq!(std::fs::read_to_string(file).unwrap(), "second version");
        assert!(cache.join("keep.txt").is_file());
        assert!(
            !std::fs::read_dir(cache)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "rqcache"))
        );
    }
}
