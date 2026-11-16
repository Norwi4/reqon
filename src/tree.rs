use crate::model::{Request, Workspace};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    sync::Arc,
};

#[derive(Clone)]
pub struct Row {
    pub key: String,
    pub name: String,
    pub collection: String,
    pub folder: String,
    pub method: String,
    pub url: String,
    pub depth: usize,
    pub kind: Kind,
    pub expanded: bool,
}
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Collection,
    Folder,
    Request,
    File,
}
#[derive(Default)]
pub struct Cache {
    pub rows: Arc<[Row]>,
    pub stale: bool,
    query: String,
    collapsed: HashSet<String>,
}
impl Cache {
    pub fn rows(&mut self, ws: &Workspace, query: &str, collapsed: &HashSet<String>) -> Arc<[Row]> {
        if self.stale || self.rows.is_empty() || self.query != query || self.collapsed != *collapsed
        {
            self.rows = build(ws, query, collapsed).into();
            self.query = query.into();
            self.collapsed = collapsed.clone();
            self.stale = false;
        }
        self.rows.clone()
    }
}
fn request_row(r: &Request, depth: usize) -> Row {
    Row {
        key: r.id.clone(),
        name: r.name.clone(),
        collection: r.collection.clone(),
        folder: r.folder.clone(),
        method: r.badge(),
        url: r.url.clone(),
        depth,
        kind: Kind::Request,
        expanded: false,
    }
}
pub fn build(ws: &Workspace, query: &str, collapsed: &HashSet<String>) -> Vec<Row> {
    let query = query.to_lowercase();
    let searching = !query.is_empty();
    let mut grouped: BTreeMap<&str, BTreeMap<&str, Vec<&Request>>> = BTreeMap::new();
    for r in &ws.requests {
        if !searching
            || r.name.to_lowercase().contains(&query)
            || r.url.to_lowercase().contains(&query)
        {
            grouped
                .entry(&r.collection)
                .or_default()
                .entry(&r.folder)
                .or_default()
                .push(r);
        }
    }
    for folders in grouped.values_mut() {
        for requests in folders.values_mut() {
            // Equal sequence numbers retain their persisted workspace order.
            // Editing a name must never move a request in the tree.
            requests.sort_by(|a, b| a.seq.total_cmp(&b.seq));
        }
    }
    let mut rows = Vec::new();
    for c in &ws.collections {
        let empty = BTreeMap::new();
        let requests = grouped.get(c.id.as_str()).unwrap_or(&empty);
        if searching && requests.is_empty() && !c.name.to_lowercase().contains(&query) {
            continue;
        }
        let expanded = searching || !collapsed.contains(&c.id);
        rows.push(Row {
            key: c.id.clone(),
            name: c.name.clone(),
            collection: c.id.clone(),
            folder: String::new(),
            method: String::new(),
            url: String::new(),
            depth: 0,
            kind: Kind::Collection,
            expanded,
        });
        if !expanded {
            continue;
        }
        let mut folders = BTreeSet::new();
        for folder in c
            .folders
            .iter()
            .map(String::as_str)
            .chain(requests.keys().copied())
        {
            if folder.is_empty() {
                continue;
            }
            let mut prefix = String::new();
            for part in folder.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                folders.insert(prefix.clone());
            }
        }
        fn children(
            parent: &str,
            cid: &str,
            folders: &BTreeSet<String>,
            requests: &BTreeMap<&str, Vec<&Request>>,
            collapsed: &HashSet<String>,
            searching: bool,
            rows: &mut Vec<Row>,
            depth: usize,
        ) {
            for folder in folders
                .iter()
                .filter(|f| f.rsplit_once('/').map(|p| p.0).unwrap_or("") == parent)
            {
                if searching
                    && !requests
                        .keys()
                        .any(|p| *p == folder || p.starts_with(&format!("{folder}/")))
                {
                    continue;
                }
                let key = format!("{cid}:{folder}");
                let expanded = searching || !collapsed.contains(&key);
                rows.push(Row {
                    key,
                    name: folder.rsplit('/').next().unwrap_or(folder).into(),
                    collection: cid.into(),
                    folder: folder.clone(),
                    method: String::new(),
                    url: String::new(),
                    depth,
                    kind: Kind::Folder,
                    expanded,
                });
                if expanded {
                    children(
                        folder,
                        cid,
                        folders,
                        requests,
                        collapsed,
                        searching,
                        rows,
                        depth + 1,
                    );
                }
            }
            if let Some(items) = requests.get(parent) {
                for r in items {
                    rows.push(request_row(r, depth));
                }
            }
        }
        children(
            "", &c.id, &folders, requests, collapsed, searching, &mut rows, 0,
        );
    }
    if let Some(folders) = grouped.get("") {
        for items in folders.values() {
            for r in items {
                rows.push(request_row(r, 0));
            }
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Collection;
    #[test]
    fn protocol_badges_do_not_show_rpc_paths_or_transport_methods() {
        for method in ["/add.Add/Sum", "/", "", "GRPC"] {
            let request = Request {
                protocol: "grpc".into(),
                method: method.into(),
                ..Default::default()
            };
            assert_eq!(request_row(&request, 0).method, "gRPC");
            assert_eq!(request.method, method);
        }
        for (protocol, method, badge) in [
            ("http", "DELETE", "DELETE"),
            ("graphql", "POST", "GQL"),
            ("ws", "GET", "WS"),
        ] {
            let request = Request {
                protocol: protocol.into(),
                method: method.into(),
                ..Default::default()
            };
            assert_eq!(request_row(&request, 0).method, badge);
        }
    }
    #[test]
    fn collapsed_parent_hides_descendants_and_search_reveals_matches() {
        let mut ws = Workspace::default();
        ws.collections.push(Collection {
            id: "c".into(),
            name: "API".into(),
            path: Default::default(),
            environments: vec![],
            vars: vec![],
            folders: vec!["One/Two".into()],
        });
        ws.requests.push(Request {
            collection: "c".into(),
            folder: "One/Two".into(),
            name: "Needle".into(),
            ..Default::default()
        });
        let collapsed = HashSet::from(["c:One".into()]);
        assert_eq!(build(&ws, "", &collapsed).len(), 2);
        assert_eq!(build(&ws, "needle", &collapsed).len(), 4);
    }
}
