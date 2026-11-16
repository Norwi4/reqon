#[cfg(feature = "desktop")]
pub use crate::file_watch::DiskSync;
use crate::{model::*, storage};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, path::PathBuf};

pub fn fingerprint(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
/// Refresh clean requests while preserving local drafts and stable tab identifiers.
#[cfg(test)]
pub fn merge_requests(
    ws: &mut Workspace,
    cid: &str,
    disk: Vec<Request>,
    saved: &mut HashMap<String, String>,
) -> (usize, usize) {
    let (count, conflicts, _) = merge_prepared(
        ws,
        cid,
        disk.into_iter()
            .map(|r| {
                let source = storage::to_bru(&r);
                (r, source)
            })
            .collect(),
        saved,
    );
    (count, conflicts)
}
pub fn merge_prepared(
    ws: &mut Workspace,
    cid: &str,
    disk: Vec<(Request, String)>,
    saved: &mut HashMap<String, String>,
) -> (usize, usize, Vec<String>) {
    let mut modified = vec![];
    let mut incoming: HashMap<PathBuf, (Request, String)> = disk
        .into_iter()
        .filter_map(|(r, s)| r.path.clone().map(|p| (p, (r, s))))
        .collect();
    let mut refreshed = 0;
    let mut conflicts = 0;
    ws.requests.retain_mut(|r| {
        if r.collection != cid {
            return true;
        }
        let Some(path) = &r.path else {
            return true;
        };
        match incoming.remove(path) {
            Some((mut next, content)) => {
                if saved.get(&r.id) == Some(&content) {
                    return true;
                }
                let current = storage::to_bru(r);
                let clean = saved.get(&r.id).is_some_and(|base| *base == current);
                if clean {
                    next.id = r.id.clone();
                    next.protocol_drafts = std::mem::take(&mut r.protocol_drafts);
                    *r = next;
                    modified.push(r.id.clone());
                    ws.saved_fingerprints
                        .insert(r.id.clone(), fingerprint(&content));
                    saved.insert(r.id.clone(), content);
                    refreshed += 1;
                } else if current == content {
                    ws.saved_fingerprints
                        .insert(r.id.clone(), fingerprint(&content));
                    saved.insert(r.id.clone(), content);
                } else {
                    conflicts += 1;
                }
                true
            }
            None if saved
                .get(&r.id)
                .is_some_and(|base| *base == storage::to_bru(r)) =>
            {
                saved.remove(&r.id);
                ws.saved_fingerprints.remove(&r.id);
                refreshed += 1;
                false
            }
            None => {
                conflicts += 1;
                true
            }
        }
    });
    for (_, (r, source)) in incoming {
        modified.push(r.id.clone());
        ws.saved_fingerprints
            .insert(r.id.clone(), fingerprint(&source));
        saved.insert(r.id.clone(), source);
        ws.requests.push(r);
        refreshed += 1;
    }
    ws.open_tabs
        .retain(|id| id.starts_with("__") || ws.requests.iter().any(|r| r.id == *id));
    if ws
        .active
        .as_ref()
        .is_some_and(|id| !ws.open_tabs.contains(id))
    {
        ws.active = ws.open_tabs.last().cloned();
    }
    (refreshed, conflicts, modified)
}

pub fn restore(ws: &mut Workspace) {
    if ws.saved_fingerprints.is_empty() {
        return;
    }
    for r in &mut ws.requests {
        if ws.saved_fingerprints.get(&r.id) != Some(&fingerprint(&storage::to_bru(r))) {
            continue;
        }
        let Some(path) = &r.path else {
            continue;
        };
        if let Ok(mut fresh) = crate::yaml::read_request(path) {
            fresh.id = r.id.clone();
            fresh.collection = r.collection.clone();
            fresh.folder = r.folder.clone();
            fresh.path = r.path.clone();
            fresh.protocol_drafts = std::mem::take(&mut r.protocol_drafts);
            *r = fresh;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_changes_reload_clean_requests_but_preserve_unsaved_drafts() {
        let mut clean = Request {
            name: "clean".into(),
            collection: "c".into(),
            path: Some("clean.req".into()),
            ..Default::default()
        };
        let mut dirty = Request {
            name: "dirty".into(),
            body_kind: BodyKind::Text,
            collection: "c".into(),
            path: Some("dirty.req".into()),
            ..Default::default()
        };
        let clean_id = clean.id.clone();
        let dirty_id = dirty.id.clone();
        let mut saved = HashMap::from([
            (clean.id.clone(), storage::to_bru(&clean)),
            (dirty.id.clone(), storage::to_bru(&dirty)),
        ]);
        let mut ws = Workspace::default();
        dirty.body = "local draft".into();
        ws.requests = vec![clean.clone(), dirty.clone()];
        clean.url = "https://new.example".into();
        dirty.body = "external edit".into();
        assert_eq!(
            merge_requests(&mut ws, "c", vec![clean, dirty], &mut saved),
            (1, 1)
        );
        assert_eq!(
            ws.requests.iter().find(|r| r.id == clean_id).unwrap().url,
            "https://new.example"
        );
        assert_eq!(
            ws.requests.iter().find(|r| r.id == dirty_id).unwrap().body,
            "local draft"
        );
    }
}
