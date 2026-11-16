//! Topological lanes derived from real parent hashes, including merge commits.
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default, Debug)]
pub struct Row {
    pub lane: usize,
    pub lanes: usize,
    pub incoming: Vec<(usize, usize)>,
    pub outgoing: Vec<(usize, usize)>,
}

pub fn layout(commits: &[crate::git::Commit]) -> Vec<Row> {
    let mut lanes: Vec<String> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    for commit in commits {
        let inherited_lanes = lanes.len();
        let lane = lanes
            .iter()
            .position(|h| h == &commit.hash)
            .unwrap_or_else(|| {
                lanes.push(commit.hash.clone());
                lanes.len() - 1
            });
        let before = lanes.clone();
        lanes.remove(lane);
        for (offset, parent) in commit.parents.iter().enumerate() {
            if !lanes.contains(parent) {
                lanes.insert((lane + offset).min(lanes.len()), parent.clone());
            }
        }
        let incoming = (0..inherited_lanes).map(|i| (i, i)).collect();
        let mut outgoing = Vec::new();
        for (i, hash) in before.iter().enumerate() {
            if i != lane {
                if let Some(j) = lanes.iter().position(|h| h == hash) {
                    outgoing.push((i, j));
                }
            }
        }
        for parent in &commit.parents {
            if let Some(j) = lanes.iter().position(|h| h == parent) {
                outgoing.push((lane, j));
            }
        }
        rows.push(Row {
            lane,
            lanes: before.len().max(lanes.len()),
            incoming,
            outgoing,
        });
    }
    rows
}

pub fn ancestors(commits: &[crate::git::Commit], tip: &str) -> HashSet<String> {
    let map: HashMap<_, _> = commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let mut found = HashSet::new();
    let mut pending = vec![tip.to_owned()];
    while let Some(hash) = pending.pop() {
        if found.insert(hash.clone()) {
            if let Some(c) = map.get(hash.as_str()) {
                pending.extend(c.parents.clone());
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    fn commit(hash: &str, parents: &[&str]) -> crate::git::Commit {
        crate::git::Commit {
            hash: hash.into(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }
    #[test]
    fn merge_graph_has_separate_lanes_that_join_at_common_parent() {
        let commits = vec![
            commit("merge", &["left", "right"]),
            commit("left", &["base"]),
            commit("right", &["base"]),
            commit("base", &[]),
        ];
        let rows = layout(&commits);
        assert_eq!(rows[0].outgoing, vec![(0, 0), (0, 1)]);
        assert_eq!(rows[2].lane, 1);
        assert!(rows[2].outgoing.contains(&(1, 0)));
        assert!(rows[3].outgoing.is_empty());
        assert_eq!(
            ancestors(&commits, "left"),
            HashSet::from(["left".into(), "base".into()])
        );
    }
}
