use crate::{
    app::{Action as AppAction, App},
    git::{Action, View},
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub enum Dialog {
    Settings,
    Remotes,
    Stashes,
    NewBranch(String),
    Rename(String),
    Worktree(String),
    Push(String),
}

pub use crate::ui_state::GitDraft as Draft;
impl State {
    pub fn draft(&self, view: View) -> Draft {
        Draft {
            message: self.message.clone(),
            selected: self.selected.clone(),
            amend: self.amend,
            commit_width: self.commit_width,
            bottom_height: self.bottom_height,
            branches_width: self.branches_width,
            details_width: self.details_width,
            branch_filter: self.branch_filter.clone(),
            view,
        }
    }
    pub fn restore(draft: &Draft) -> Self {
        Self {
            message: draft.message.clone(),
            selected: draft.selected.clone(),
            amend: draft.amend,
            commit_width: draft.commit_width,
            bottom_height: draft.bottom_height,
            branches_width: draft.branches_width,
            details_width: draft.details_width,
            branch_filter: draft.branch_filter.clone(),
            selected_commit: match &draft.view {
                View::Commit(hash) | View::CommitFile(hash, _) => hash.clone(),
                _ => String::new(),
            },
            ..Default::default()
        }
    }
}

pub struct State {
    pub message: String,
    pub selected: BTreeSet<String>,
    pub amend: bool,
    pub remote_name: String,
    pub remote_url: String,
    pub branch: String,
    pub path: String,
    pub stash: String,
    pub name: String,
    pub email: String,
    pub clone_open: bool,
    pub clone_url: String,
    pub clone_path: String,
    pub dialog: Option<Dialog>,
    pub confirm: Option<(String, Action)>,
    pub commit_width: f32,
    pub bottom_height: f32,
    pub branches_width: f32,
    pub details_width: f32,
    pub branch_search: String,
    pub log_search: String,
    pub branch_filter: String,
    pub selected_commit: String,
    pub details: Vec<crate::git_diff::ChangedFile>,
    pub details_text: String,
    pub details_ref: String,
    pub details_compare: bool,
    pub details_right: String,
    pub log_cache_key: String,
    pub log_rows: Vec<usize>,
    pub log_graph: Vec<crate::git_graph::Row>,
    pub seen_update: Option<Instant>,
    pub diff_scroll: f32,
    pub diff_key: String,
    pub diff_hunk: usize,
    pub unified: bool,
    pub collapsed: BTreeSet<String>,
}
impl Default for State {
    fn default() -> Self {
        let draft = Draft::default();
        Self {
            message: draft.message,
            selected: draft.selected,
            amend: draft.amend,
            remote_name: "origin".into(),
            remote_url: String::new(),
            branch: String::new(),
            path: String::new(),
            stash: String::new(),
            name: String::new(),
            email: String::new(),
            clone_open: false,
            clone_url: String::new(),
            clone_path: String::new(),
            dialog: None,
            confirm: None,
            commit_width: draft.commit_width,
            bottom_height: draft.bottom_height,
            branches_width: draft.branches_width,
            details_width: draft.details_width,
            branch_search: String::new(),
            log_search: String::new(),
            branch_filter: draft.branch_filter,
            selected_commit: String::new(),
            details: vec![],
            details_text: String::new(),
            details_ref: String::new(),
            details_compare: false,
            details_right: String::new(),
            log_cache_key: String::new(),
            log_rows: vec![],
            log_graph: vec![],
            seen_update: None,
            diff_scroll: 0.,
            diff_key: String::new(),
            diff_hunk: 0,
            unified: false,
            collapsed: BTreeSet::from(["Tags".into()]),
        }
    }
}

impl App {
    pub fn git_workspace(&mut self, ui: &mut Ui, r: Rect, cid: &str) {
        let mut scoped = w::child(ui, r, ("git-workspace", cid));
        let ui = &mut scoped;
        let t = self.theme;
        let mut state = self.git.remove(cid).unwrap_or_default();
        if state.updated.is_none() && matches!(state.view, View::Home) {
            if let Some(draft) = self.ws.git_drafts.get(cid) {
                state.view = draft.view.clone();
            }
        }
        let mut form = self.git_ui.remove(cid).unwrap_or_else(|| {
            self.ws
                .git_drafts
                .get(cid)
                .map(State::restore)
                .unwrap_or_default()
        });
        let mut action = None;
        if form.name.is_empty() {
            form.name.clone_from(&state.name);
        }
        if form.email.is_empty() {
            form.email.clone_from(&state.email);
        }
        if state.updated != form.seen_update {
            form.seen_update = state.updated;
            if state.updated.is_some() {
                form.selected
                    .retain(|p| state.files.iter().any(|f| &f.path == p));
            }
            match &state.view {
                View::Commit(hash) | View::CommitFile(hash, _) => {
                    form.details.clone_from(&state.review.files);
                    form.details_text.clone_from(&state.review.summary);
                    form.details_ref.clone_from(hash);
                    form.details_compare = false;
                }
                View::Compare(branch, _) | View::CompareRefs(branch, _, _) => {
                    form.details.clone_from(&state.review.files);
                    form.details_text.clone_from(&state.review.summary);
                    form.details_ref.clone_from(branch);
                    form.details_compare = true;
                    form.details_right = if let View::CompareRefs(_, right, _) = &state.view {
                        right.clone()
                    } else {
                        String::new()
                    };
                }
                _ => {}
            }
        }
        let error_h = if state.error.is_empty() { 0. } else { 57. };
        let body = rect(r.left(), r.top(), r.width(), r.height() - error_h);
        if state.root.is_none() {
            let mut welcome = w::child(ui, body.shrink(24.), "git-welcome");
            welcome.vertical_centered(|ui| {
                ui.add_space(55.);
                let logo = ui.allocate_space(vec2(48., 48.)).1;
                w::icon(ui, logo.center(), Icon::GitBrand, t.muted, 44.);
                ui.add_space(20.);
                ui.label(
                    egui::RichText::new("Version control for your collection")
                        .size(18.)
                        .strong(),
                );
                ui.add_space(12.);
                ui.label(
                    "Initialize Git here, open an existing collection, or clone a repository.",
                );
                ui.add_space(20.);
                if ui
                    .add_enabled(!state.busy, egui::Button::new("Initialize Git"))
                    .clicked()
                {
                    action = Some(Action::Init);
                }
                if ui.button("Open existing collection…").clicked() {
                    self.actions.push(AppAction::OpenCollection);
                }
                if ui.button("Clone repository…").clicked() {
                    form.clone_open = true;
                }
            });
        } else {
            form.bottom_height = form
                .bottom_height
                .clamp(150., (body.height() * 0.64).max(150.));
            let split_y = body.bottom() - form.bottom_height;
            splitter(
                ui,
                rect(body.left(), split_y - 3., body.width(), 6.),
                "git-bottom-split",
                false,
                &mut form.bottom_height,
                -1.,
            );
            let top = rect(
                body.left(),
                body.top(),
                body.width(),
                (split_y - body.top() - 2.).max(0.),
            );
            form.commit_width = form
                .commit_width
                .clamp(205., (body.width() * 0.46).max(205.));
            let split_x = body.left() + form.commit_width;
            splitter(
                ui,
                rect(split_x - 3., top.top(), 6., top.height()),
                "git-commit-split",
                true,
                &mut form.commit_width,
                1.,
            );
            crate::git_commit_ui::show(
                ui,
                rect(top.left(), top.top(), form.commit_width - 1., top.height()),
                &state,
                &mut form,
                &mut action,
                t,
            );
            crate::git_diff_ui::show(
                ui,
                rect(
                    split_x + 2.,
                    top.top(),
                    (top.right() - split_x - 2.).max(0.),
                    top.height(),
                ),
                &state,
                &mut form,
                t,
            );
            w::line(
                ui,
                pos2(split_x, top.top()),
                pos2(split_x, top.bottom()),
                t.border,
            );
            w::line(
                ui,
                pos2(body.left(), split_y),
                pos2(body.right(), split_y),
                t.border,
            );
            crate::git_log_ui::show(
                ui,
                rect(
                    body.left(),
                    split_y + 2.,
                    body.width(),
                    form.bottom_height - 2.,
                ),
                &state,
                &mut form,
                &mut action,
                t,
            );
        }
        if !state.error.is_empty() {
            let err = rect(
                r.left() + 6.,
                r.bottom() - error_h + 3.,
                r.width() - 12.,
                error_h - 6.,
            );
            ui.painter().rect_filled(err, 4, t.red.gamma_multiply(0.12));
            let mut child = w::child(ui, err.shrink(7.), "git-error");
            egui::ScrollArea::vertical().show(&mut child, |ui| {
                ui.add(
                    egui::Label::new(egui::RichText::new(&state.error).color(t.red).size(12.))
                        .selectable(true)
                        .wrap(),
                );
            });
        }
        if state.busy && !state.refreshing {
            let mut busy = w::child(
                ui,
                rect(r.right() - 215., r.top() + 2., 210., 25.),
                "git-progress",
            );
            busy.horizontal(|ui| {
                ui.spinner();
                ui.label("Running Git…");
                if ui.small_button("Cancel").clicked() {
                    self.git_pending.retain(|(id, _)| id != cid);
                    if let Some(cancel) = &state.cancel {
                        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            });
        }
        crate::git_dialogs::show(self, ui.ctx(), cid, &state, &mut form, &mut action);
        if !state.busy
            && action.is_none()
            && (state.updated.is_none()
                || state
                    .updated
                    .is_some_and(|at| at.elapsed() > Duration::from_secs(5)))
            && form.confirm.is_none()
            && form.dialog.is_none()
        {
            action = Some(Action::Refresh);
        }
        ui.ctx().request_repaint_after(Duration::from_secs(5));
        let view = if let Some(Action::Preview(view)) = &action {
            view.clone()
        } else {
            state.view.clone()
        };
        let draft = form.draft(view);
        if self.ws.git_drafts.get(cid) != Some(&draft) {
            self.ws.git_drafts.insert(cid.into(), draft);
            self.mark_workspace_changed();
        }
        self.git.insert(cid.into(), state);
        self.git_ui.insert(cid.into(), form);
        if let Some(action) = action {
            self.git_action(cid, action);
        }
    }
}

pub fn splitter(ui: &mut Ui, r: Rect, id: &str, horizontal: bool, value: &mut f32, sign: f32) {
    let response = ui.interact(r, ui.scope_id().with(id), Sense::drag());
    if response.hovered() || response.dragged() {
        ui.ctx().set_cursor_icon(if horizontal {
            egui::CursorIcon::ResizeHorizontal
        } else {
            egui::CursorIcon::ResizeVertical
        });
    }
    if response.dragged() {
        *value += sign
            * if horizontal {
                response.drag_delta().x
            } else {
                response.drag_delta().y
            };
    }
}

pub fn file_color(status: char, t: crate::theme::Theme) -> egui::Color32 {
    crate::theme::hex(match status {
        'A' | '?' => {
            if t.dark {
                0x6aab73
            } else {
                0x2f7d32
            }
        }
        'D' => {
            if t.dark {
                0x9b9292
            } else {
                0x777777
            }
        }
        'U' | '!' => {
            if t.dark {
                0xf27575
            } else {
                0xc62828
            }
        }
        _ => {
            if t.dark {
                0x6c95eb
            } else {
                0x2458a6
            }
        }
    })
}

pub fn menu_row(
    ui: &mut Ui,
    label: &str,
    icon: Icon,
    enabled: bool,
    t: crate::theme::Theme,
) -> bool {
    let row = ui
        .allocate_space(vec2(ui.available_width().max(270.), 27.))
        .1;
    let response = ui.interact(row, ui.scope_id().with(label), Sense::click());
    if response.hovered() && enabled {
        ui.painter().rect_filled(row, 4, t.surface);
    }
    let color = if enabled {
        t.text
    } else {
        t.faint.gamma_multiply(0.55)
    };
    w::icon(ui, pos2(row.left() + 12., row.center().y), icon, color, 14.);
    w::label(
        ui,
        pos2(row.left() + 28., row.center().y),
        label,
        12.,
        color,
    );
    if enabled && response.clicked() {
        ui.close();
        return true;
    }
    false
}
