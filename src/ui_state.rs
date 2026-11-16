//! Serializable desktop state, with no windowing or rendering dependencies.
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollapsedPane {
    Request,
    Response,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneLayout {
    pub height: Option<f32>,
    pub width: Option<f32>,
    pub collapsed: Option<CollapsedPane>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum GitView {
    #[default]
    Home,
    File(String, bool),
    Commit(String),
    Working(String),
    CommitFile(String, String),
    Compare(String, String),
    CompareRefs(String, String, String),
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct GitDraft {
    pub message: String,
    pub selected: BTreeSet<String>,
    pub amend: bool,
    pub commit_width: f32,
    pub bottom_height: f32,
    pub branches_width: f32,
    pub details_width: f32,
    pub branch_filter: String,
    pub view: GitView,
}
impl Default for GitDraft {
    fn default() -> Self {
        Self {
            message: String::new(),
            selected: BTreeSet::new(),
            amend: false,
            commit_width: 250.,
            bottom_height: 265.,
            branches_width: 195.,
            details_width: 250.,
            branch_filter: String::new(),
            view: GitView::Home,
        }
    }
}
