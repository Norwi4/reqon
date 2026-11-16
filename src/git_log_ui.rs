use crate::{
    git::{Action, Branch, State as Repository, View},
    git_ops::BranchAction as B,
    git_ui::{self, Dialog, State},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, Ui, pos2, vec2};
use std::collections::{HashMap, HashSet};

pub fn show(
    ui: &mut Ui,
    r: Rect,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    let mut outer = w::child(ui, r, "git-log-panel");
    let ui = &mut outer;
    w::medium(ui, pos2(r.left() + 12., r.top() + 16.), "Git", 12., t.text);
    w::label(ui, pos2(r.left() + 45., r.top() + 16.), "Log", 12., t.text);
    w::line(
        ui,
        pos2(r.left() + 44., r.top() + 30.),
        pos2(r.left() + 69., r.top() + 30.),
        t.blue,
    );
    if repo.operation.is_empty() {
        let label = if repo.upstream.is_empty() {
            repo.branch.clone()
        } else {
            format!("{}    ↑ {}   ↓ {}", repo.branch, repo.ahead, repo.behind)
        };
        w::elided_label(
            ui,
            rect(r.left() + 86., r.top(), r.width() - 292., 32.),
            &label,
            11.,
            t.muted,
        );
    } else {
        w::label(
            ui,
            pos2(r.left() + 86., r.top() + 16.),
            &format!("{} in progress", repo.operation),
            12.,
            t.orange,
        );
        if w::button(
            ui,
            rect(r.left() + 225., r.top() + 3., 75., 26.),
            "Continue",
            false,
            t,
        )
        .clicked()
        {
            *action = Some(Action::Branch(B::Continue(repo.operation.clone())));
        }
        if w::button(
            ui,
            rect(r.left() + 307., r.top() + 3., 57., 26.),
            "Abort",
            false,
            t,
        )
        .clicked()
        {
            form.confirm = Some((
                format!(
                    "Abort the current {} and restore its starting state?",
                    repo.operation
                ),
                Action::Branch(B::Abort(repo.operation.clone())),
            ));
        }
    }
    let mut tools = w::child(
        ui,
        rect(r.right() - 192., r.top() + 2., 188., 28.),
        "git-log-actions",
    );
    if repo.busy {
        tools.disable();
    }
    for (i, (icon, title)) in [
        (Icon::Refresh, "Fetch all remotes"),
        (Icon::Download, "Update current branch"),
        (Icon::ArrowRight, "Push…"),
        (Icon::Plus, "New branch…"),
        (Icon::Box, "Stashes"),
        (Icon::Settings, "Manage remotes"),
    ]
    .into_iter()
    .enumerate()
    {
        if w::icon_button(
            &mut tools,
            rect(r.right() - 190. + i as f32 * 31., r.top() + 2., 27., 28.),
            icon,
            title,
            t,
        )
        .clicked()
        {
            match i {
                0 => *action = Some(Action::Fetch),
                1 => *action = Some(Action::Pull),
                2 => form.dialog = Some(Dialog::Push(repo.branch.clone())),
                3 => {
                    form.branch.clear();
                    form.dialog = Some(Dialog::NewBranch("HEAD".into()));
                }
                4 => form.dialog = Some(Dialog::Stashes),
                _ => form.dialog = Some(Dialog::Remotes),
            }
        }
    }
    w::line(
        ui,
        pos2(r.left(), r.top() + 32.),
        pos2(r.right(), r.top() + 32.),
        t.border,
    );
    form.branches_width = form
        .branches_width
        .clamp(145., (r.width() * 0.28).max(145.));
    form.details_width = form.details_width.clamp(165., (r.width() * 0.35).max(165.));
    let left = r.left() + form.branches_width;
    let right = r.right() - form.details_width;
    git_ui::splitter(
        ui,
        rect(left - 3., r.top() + 32., 6., r.height() - 32.),
        "git-branches-split",
        true,
        &mut form.branches_width,
        1.,
    );
    git_ui::splitter(
        ui,
        rect(right - 3., r.top() + 32., 6., r.height() - 32.),
        "git-details-split",
        true,
        &mut form.details_width,
        -1.,
    );
    branches(
        ui,
        rect(
            r.left(),
            r.top() + 34.,
            form.branches_width - 2.,
            r.height() - 34.,
        ),
        repo,
        form,
        action,
        t,
    );
    log(
        ui,
        rect(
            left + 2.,
            r.top() + 34.,
            (right - left - 4.).max(60.),
            r.height() - 34.,
        ),
        repo,
        form,
        action,
        t,
    );
    details(
        ui,
        rect(
            right + 2.,
            r.top() + 34.,
            form.details_width - 2.,
            r.height() - 34.,
        ),
        repo,
        form,
        action,
        t,
    );
    w::line(
        ui,
        pos2(left, r.top() + 32.),
        pos2(left, r.bottom()),
        t.border,
    );
    w::line(
        ui,
        pos2(right, r.top() + 32.),
        pos2(right, r.bottom()),
        t.border,
    );
}

fn branches(
    ui: &mut Ui,
    r: Rect,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    let mut body = w::child(ui, r, "git-branches");
    let ui = &mut body;
    ui.put(
        rect(r.left() + 7., r.top() + 3., r.width() - 14., 25.),
        egui::TextEdit::singleline(&mut form.branch_search)
            .hint_text("Branch or tag")
            .font(egui::FontId::proportional(12.)),
    );
    let mut list = w::child(
        ui,
        rect(
            r.left() + 5.,
            r.top() + 33.,
            r.width() - 10.,
            r.height() - 33.,
        ),
        "branches-list",
    );
    list.spacing_mut().item_spacing.y = 0.;
    let needle = form.branch_search.to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut list, |ui| {
            for (label, filter) in [
                ("All branches", ""),
                ("HEAD (Current Branch)", repo.branch.as_str()),
            ] {
                let row = ui.allocate_space(vec2(ui.available_width(), 24.)).1;
                let response = ui.interact(row, ui.scope_id().with(label), Sense::click());
                if form.branch_filter == filter {
                    ui.painter()
                        .rect_filled(row, 3, t.blue.gamma_multiply(0.18));
                }
                w::elided_label(ui, row.shrink2(vec2(9., 0.)), label, 12., t.text);
                if response.clicked() {
                    form.branch_filter = filter.into();
                }
            }
            for (group, kind) in [("Local", 0), ("Remote", 1), ("Tags", 2)] {
                let files: Vec<_> = repo
                    .refs
                    .iter()
                    .filter(|b| match kind {
                        0 => b.full_ref.starts_with("refs/heads/"),
                        1 => b.remote,
                        _ => b.full_ref.starts_with("refs/tags/"),
                    })
                    .filter(|b| needle.is_empty() || b.name.to_lowercase().contains(&needle))
                    .collect();
                let row = ui.allocate_space(vec2(ui.available_width(), 25.)).1;
                let response = ui.interact(row, ui.scope_id().with(group), Sense::click());
                let closed = form.collapsed.contains(group) && needle.is_empty();
                w::icon(
                    ui,
                    pos2(row.left() + 8., row.center().y),
                    if closed { Icon::Chevron } else { Icon::Down },
                    t.muted,
                    10.,
                );
                w::label(
                    ui,
                    pos2(row.left() + 23., row.center().y),
                    group,
                    12.,
                    t.text,
                );
                if response.clicked() {
                    if !form.collapsed.remove(group) {
                        form.collapsed.insert(group.into());
                    }
                }
                if closed {
                    continue;
                }
                let mut shown = HashSet::new();
                for branch in files {
                    let parts: Vec<_> = branch.name.split('/').collect();
                    let mut hidden = false;
                    for depth in 0..parts.len().saturating_sub(1) {
                        let prefix = parts[..=depth].join("/");
                        let key = format!("{group}/{prefix}");
                        if shown.insert(key.clone()) {
                            let row = ui.allocate_space(vec2(ui.available_width(), 23.)).1;
                            let response =
                                ui.interact(row, ui.scope_id().with(&key), Sense::click());
                            w::icon(
                                ui,
                                pos2(row.left() + 22. + depth as f32 * 13., row.center().y),
                                if form.collapsed.contains(&key) {
                                    Icon::Chevron
                                } else {
                                    Icon::Down
                                },
                                t.muted,
                                9.,
                            );
                            w::icon(
                                ui,
                                pos2(row.left() + 36. + depth as f32 * 13., row.center().y),
                                Icon::Folder,
                                t.muted,
                                13.,
                            );
                            w::elided_label(
                                ui,
                                rect(
                                    row.left() + 47. + depth as f32 * 13.,
                                    row.top(),
                                    row.width() - 49. - depth as f32 * 13.,
                                    23.,
                                ),
                                parts[depth],
                                12.,
                                t.text,
                            );
                            if response.clicked() {
                                if !form.collapsed.remove(&key) {
                                    form.collapsed.insert(key.clone());
                                }
                            }
                        }
                        if needle.is_empty() && form.collapsed.contains(&key) {
                            hidden = true;
                            break;
                        }
                    }
                    if hidden {
                        continue;
                    }
                    let row = ui.allocate_space(vec2(ui.available_width(), 24.)).1;
                    if !ui.is_rect_visible(row) {
                        continue;
                    }
                    let response =
                        ui.interact(row, ui.scope_id().with(&branch.full_ref), Sense::click());
                    if form.branch_filter == branch.name || response.hovered() {
                        ui.painter().rect_filled(
                            row,
                            3,
                            if form.branch_filter == branch.name {
                                t.blue.gamma_multiply(0.22)
                            } else {
                                t.hover
                            },
                        );
                    }
                    let indent = 26. + parts.len().saturating_sub(1) as f32 * 13.;
                    w::icon(
                        ui,
                        pos2(row.left() + indent, row.center().y),
                        if branch.current || kind == 2 {
                            Icon::Tag
                        } else {
                            Icon::GitBranch
                        },
                        if branch.current { t.accent } else { t.muted },
                        13.,
                    );
                    w::elided_label(
                        ui,
                        rect(
                            row.left() + indent + 13.,
                            row.top(),
                            row.width() - indent - 15.,
                            24.,
                        ),
                        parts.last().copied().unwrap_or(&branch.name),
                        12.,
                        t.text,
                    );
                    response.clone().on_hover_text(format!(
                        "{}{}",
                        branch.name,
                        if branch.upstream.is_empty() {
                            String::new()
                        } else {
                            format!("\nTracks {}", branch.upstream)
                        }
                    ));
                    if response.clicked() {
                        form.branch_filter = branch.name.clone();
                    }
                    if response.double_clicked() && !branch.current && kind != 2 {
                        *action = Some(checkout(branch));
                    }
                    response.context_menu(|ui| branch_menu(ui, branch, repo, form, action, t));
                }
            }
        });
}

fn checkout(branch: &Branch) -> Action {
    if branch.remote {
        Action::Branch(B::CheckoutRemote(branch.name.clone()))
    } else {
        Action::Checkout(branch.name.clone())
    }
}

fn branch_menu(
    ui: &mut Ui,
    b: &Branch,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    ui.set_width(320.);
    ui.spacing_mut().item_spacing.y = 0.;
    let enabled = !repo.busy;
    if b.full_ref.starts_with("refs/tags/") {
        if git_ui::menu_row(
            ui,
            &format!("New Branch from '{}'…", b.name),
            Icon::Plus,
            enabled,
            t,
        ) {
            form.branch.clear();
            form.dialog = Some(Dialog::NewBranch(b.full_ref.clone()));
        }
        if git_ui::menu_row(
            ui,
            &format!("Compare with '{}'", repo.branch),
            Icon::Split,
            enabled,
            t,
        ) {
            *action = Some(Action::Preview(View::CompareRefs(
                b.full_ref.clone(),
                repo.branch.clone(),
                String::new(),
            )));
        }
        if git_ui::menu_row(ui, "Show Diff with Working Tree", Icon::Split, enabled, t) {
            *action = Some(Action::Preview(View::Compare(
                b.full_ref.clone(),
                String::new(),
            )));
        }
        return;
    }
    if !b.current && git_ui::menu_row(ui, "Checkout", Icon::GitBranch, enabled, t) {
        *action = Some(checkout(b));
    }
    if git_ui::menu_row(
        ui,
        &format!("New Branch from '{}'…", b.name),
        Icon::Plus,
        enabled,
        t,
    ) {
        form.branch.clear();
        form.dialog = Some(Dialog::NewBranch(b.name.clone()));
    }
    if !b.current && !b.remote {
        if git_ui::menu_row(
            ui,
            &format!("Checkout and Rebase onto '{}'", repo.branch),
            Icon::GitMerge,
            enabled,
            t,
        ) {
            form.confirm = Some((
                format!("Check out {} and rebase it onto {}?", b.name, repo.branch),
                Action::Branch(B::CheckoutRebase {
                    branch: b.name.clone(),
                    onto: repo.branch.clone(),
                }),
            ));
        }
        if git_ui::menu_row(
            ui,
            "Checkout and Update",
            Icon::Download,
            enabled && !b.upstream.is_empty(),
            t,
        ) {
            *action = Some(Action::Branch(B::CheckoutUpdate(b.name.clone())));
        }
    }
    if b.remote
        && git_ui::menu_row(
            ui,
            &format!("Checkout and Rebase onto '{}'", repo.branch),
            Icon::GitMerge,
            enabled,
            t,
        )
    {
        form.confirm = Some((
            format!("Check out {} and rebase it onto {}?", b.name, repo.branch),
            Action::Branch(B::CheckoutRemoteRebase {
                branch: b.name.clone(),
                onto: repo.branch.clone(),
            }),
        ));
    }
    ui.separator();
    if !b.current
        && git_ui::menu_row(
            ui,
            &format!("Compare with '{}'", repo.branch),
            Icon::Split,
            enabled,
            t,
        )
    {
        *action = Some(Action::Preview(View::CompareRefs(
            b.name.clone(),
            repo.branch.clone(),
            String::new(),
        )));
    }
    if git_ui::menu_row(ui, "Show Diff with Working Tree", Icon::Split, enabled, t) {
        *action = Some(Action::Preview(View::Compare(
            b.name.clone(),
            String::new(),
        )));
    }
    if !b.current {
        ui.separator();
        if git_ui::menu_row(
            ui,
            &format!("Rebase '{}' onto '{}'", repo.branch, b.name),
            Icon::GitMerge,
            enabled,
            t,
        ) {
            form.confirm = Some((
                format!(
                    "Rebase {} onto {}? This rewrites the current branch's commits.",
                    repo.branch, b.name
                ),
                Action::Branch(B::Rebase(b.name.clone())),
            ));
        }
        if git_ui::menu_row(
            ui,
            &format!("Merge '{}' into '{}'", b.name, repo.branch),
            Icon::GitMerge,
            enabled,
            t,
        ) {
            *action = Some(Action::Branch(B::Merge(b.name.clone())));
        }
    }
    ui.separator();
    if git_ui::menu_row(
        ui,
        &format!("New Worktree from '{}'…", b.name),
        Icon::Folder,
        enabled,
        t,
    ) {
        form.branch.clear();
        form.path.clear();
        form.dialog = Some(Dialog::Worktree(b.name.clone()));
    }
    ui.separator();
    if !b.remote {
        if git_ui::menu_row(
            ui,
            "Update",
            Icon::Download,
            enabled && !b.upstream.is_empty(),
            t,
        ) {
            *action = Some(Action::Branch(B::Update(b.name.clone())));
        }
        if git_ui::menu_row(ui, "Push…", Icon::ArrowRight, enabled, t) {
            form.remote_name = repo.push_remote.clone();
            form.dialog = Some(Dialog::Push(b.name.clone()));
        }
        if !b.upstream.is_empty() {
            ui.menu_button(format!("Tracked Branch '{}'", b.upstream), |ui| {
                if ui.button("Checkout").clicked() {
                    *action = Some(Action::Branch(B::CheckoutRemote(b.upstream.clone())));
                    ui.close();
                }
                if ui.button("Compare with Current").clicked() {
                    *action = Some(Action::Preview(View::CompareRefs(
                        b.upstream.clone(),
                        repo.branch.clone(),
                        String::new(),
                    )));
                    ui.close();
                }
            });
        }
        ui.separator();
        if git_ui::menu_row(ui, "Rename…", Icon::Edit, enabled, t) {
            form.branch = b.name.clone();
            form.dialog = Some(Dialog::Rename(b.name.clone()));
        }
        if !b.current && git_ui::menu_row(ui, "Delete…", Icon::Trash, enabled, t) {
            form.confirm = Some((
                format!(
                    "Delete local branch {}? Git will refuse if it contains unmerged commits.",
                    b.name
                ),
                Action::DeleteBranch(b.name.clone()),
            ));
        }
    } else {
        if let Some((remote, branch)) = b.name.split_once('/') {
            for rebase in [true, false] {
                if git_ui::menu_row(
                    ui,
                    &format!(
                        "Pull into '{}' Using {}",
                        repo.branch,
                        if rebase { "Rebase" } else { "Merge" }
                    ),
                    Icon::Download,
                    enabled,
                    t,
                ) {
                    let operation = Action::Branch(B::PullBranch {
                        remote: remote.into(),
                        branch: branch.into(),
                        rebase,
                    });
                    if rebase {
                        form.confirm = Some((
                            format!(
                                "Pull {} into {} and rebase local commits?",
                                b.name, repo.branch
                            ),
                            operation,
                        ));
                    } else {
                        *action = Some(operation);
                    }
                }
            }
            ui.separator();
        }
        if git_ui::menu_row(ui, "Fetch", Icon::Refresh, enabled, t) {
            *action = Some(Action::Fetch);
        }
        if git_ui::menu_row(ui, "Delete Remote Branch…", Icon::Trash, enabled, t) {
            if let Some((remote, branch)) = b.name.split_once('/') {
                form.confirm = Some((
                    format!("Delete {} from remote {}?", branch, remote),
                    Action::Branch(B::DeleteRemote {
                        remote: remote.into(),
                        branch: branch.into(),
                    }),
                ));
            }
        }
    }
}

fn log(
    ui: &mut Ui,
    r: Rect,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    let mut body = w::child(ui, r, "git-log-table");
    let ui = &mut body;
    ui.put(
        rect(r.left() + 6., r.top() + 3., r.width() - 12., 25.),
        egui::TextEdit::singleline(&mut form.log_search)
            .hint_text("Text, author or hash")
            .font(egui::FontId::proportional(12.)),
    );
    let key = format!(
        "{:?}:{}:{}",
        repo.updated, form.branch_filter, form.log_search
    );
    if key != form.log_cache_key {
        let allowed = repo
            .refs
            .iter()
            .find(|b| b.name == form.branch_filter)
            .map(|b| crate::git_graph::ancestors(&repo.commits, &b.hash));
        let needle = form.log_search.to_lowercase();
        form.log_rows = repo
            .commits
            .iter()
            .enumerate()
            .filter(|(_, c)| allowed.as_ref().is_none_or(|set| set.contains(&c.hash)))
            .filter(|(_, c)| {
                needle.is_empty()
                    || format!("{} {} {} {}", c.subject, c.author, c.hash, c.refs)
                        .to_lowercase()
                        .contains(&needle)
            })
            .map(|(i, _)| i)
            .collect();
        if form.log_rows.len() == repo.commits.len() {
            form.log_graph = repo.graph.clone();
        } else {
            form.log_graph = project_graph(&repo.commits, &form.log_rows);
        }
        form.log_cache_key = key;
    }
    let graph_w = (form
        .log_graph
        .iter()
        .map(|row| row.lanes)
        .max()
        .unwrap_or(1) as f32
        * 12.
        + 14.)
        .min(r.width() * 0.32)
        .max(30.);
    let columns = (r.width() - graph_w).max(60.);
    let date_w = (columns * 0.19).min(127.);
    let author_w = (columns * 0.19).min(128.);
    let ref_w = (columns * 0.23).min(145.);
    let date_x = r.right() - date_w;
    let author_x = date_x - author_w;
    let ref_x = author_x - ref_w;
    let subject_x = r.left() + graph_w;
    let y = r.top() + 30.;
    for (x, label) in [
        (subject_x, "Commit"),
        (ref_x, "Branch"),
        (author_x, "Author"),
        (date_x, "Date"),
    ] {
        w::label(ui, pos2(x + 4., y + 11.), label, 11., t.muted);
    }
    let footer = if repo.commits.len() >= repo.history_limit && repo.history_limit > 0 {
        25.
    } else {
        0.
    };
    let mut rows = w::child(
        ui,
        rect(r.left(), y + 24., r.width(), r.bottom() - y - 24. - footer),
        "commit-rows",
    );
    rows.spacing_mut().item_spacing.y = 0.;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show_rows(&mut rows, 24., form.log_rows.len(), |ui, range| {
            for i in range {
                let c = &repo.commits[form.log_rows[i]];
                let row = ui.allocate_space(vec2(ui.available_width(), 24.)).1;
                let response = ui.interact(row, ui.scope_id().with(&c.hash), Sense::click());
                if form.selected_commit == c.hash || response.hovered() {
                    ui.painter().rect_filled(
                        row,
                        0,
                        if form.selected_commit == c.hash {
                            t.blue.gamma_multiply(0.18)
                        } else {
                            t.hover
                        },
                    );
                }
                draw_graph(
                    ui,
                    rect(row.left(), row.top(), graph_w, row.height()),
                    &form.log_graph[i],
                    t,
                );
                w::elided_label(
                    ui,
                    rect(
                        subject_x + 4.,
                        row.top(),
                        (ref_x - subject_x - 7.).max(20.),
                        24.,
                    ),
                    &c.subject,
                    12.,
                    t.text,
                );
                w::elided_label(
                    ui,
                    rect(ref_x + 4., row.top(), ref_w - 8., 24.),
                    &c.refs.replace("HEAD -> ", ""),
                    11.,
                    t.blue,
                );
                w::elided_label(
                    ui,
                    rect(author_x + 4., row.top(), author_w - 8., 24.),
                    &c.author,
                    11.,
                    t.muted,
                );
                w::elided_label(
                    ui,
                    rect(date_x + 4., row.top(), date_w - 8., 24.),
                    &c.date,
                    11.,
                    t.muted,
                );
                response.clone().on_hover_text(format!(
                    "{}\n{} <{}>\n{}\n{}",
                    c.subject, c.author, c.email, c.date, c.hash
                ));
                if response.clicked() {
                    form.selected_commit = c.hash.clone();
                    *action = Some(Action::Preview(View::Commit(c.hash.clone())));
                }
                response.context_menu(|ui| {
                    ui.set_width(230.);
                    if git_ui::menu_row(ui, "Show Changes", Icon::Split, true, t) {
                        form.selected_commit = c.hash.clone();
                        *action = Some(Action::Preview(View::Commit(c.hash.clone())));
                    }
                    if git_ui::menu_row(ui, "Copy Revision", Icon::Copy, true, t) {
                        ui.ctx().copy_text(c.hash.clone());
                    }
                    if git_ui::menu_row(ui, "New Branch…", Icon::GitBranch, !repo.busy, t) {
                        form.branch.clear();
                        form.dialog = Some(Dialog::NewBranch(c.hash.clone()));
                    }
                });
            }
            if form.log_rows.is_empty() {
                ui.label(
                    egui::RichText::new("No commits found")
                        .size(12.)
                        .color(t.muted),
                );
            }
        });
    if footer > 0.
        && w::button(
            ui,
            rect(r.left() + 6., r.bottom() - 24., r.width() - 12., 23.),
            "Load more commits",
            false,
            t,
        )
        .clicked()
    {
        *action = Some(Action::History(repo.history_limit + 500));
    }
}

fn project_graph(commits: &[crate::git::Commit], visible: &[usize]) -> Vec<crate::git_graph::Row> {
    let map: HashMap<_, _> = commits.iter().map(|c| (c.hash.as_str(), c)).collect();
    let keep: HashSet<_> = visible.iter().map(|i| commits[*i].hash.as_str()).collect();
    let filtered: Vec<_> = visible
        .iter()
        .map(|i| {
            let mut c = commits[*i].clone();
            let mut pending = c.parents.clone();
            let mut seen = HashSet::new();
            c.parents.clear();
            while let Some(p) = pending.pop() {
                if !seen.insert(p.clone()) {
                    continue;
                }
                if keep.contains(p.as_str()) {
                    c.parents.push(p);
                } else if let Some(parent) = map.get(p.as_str()) {
                    pending.extend(parent.parents.clone());
                }
            }
            c
        })
        .collect();
    crate::git_graph::layout(&filtered)
}

fn draw_graph(ui: &Ui, r: Rect, row: &crate::git_graph::Row, t: Theme) {
    let painter = ui.painter().with_clip_rect(r.intersect(ui.clip_rect()));
    let colors = [0x6c95eb, 0x6aab73, 0xc77dff, 0xe4ad49, 0x56b6c2, 0xe38591];
    let x = |lane: usize| r.left() + 12. + lane as f32 * 12.;
    let color = |lane: usize| {
        if t.dark {
            crate::theme::hex(colors[lane % colors.len()])
        } else {
            crate::theme::hex(colors[lane % colors.len()]).gamma_multiply(0.8)
        }
    };
    for &(a, b) in &row.incoming {
        painter.line_segment(
            [pos2(x(a), r.top()), pos2(x(b), r.center().y)],
            Stroke::new(1.4, color(a)),
        );
    }
    for &(a, b) in &row.outgoing {
        painter.line_segment(
            [pos2(x(a), r.center().y), pos2(x(b), r.bottom())],
            Stroke::new(1.4, color(a)),
        );
    }
    painter.circle_filled(pos2(x(row.lane), r.center().y), 3.6, color(row.lane));
}

fn details(
    ui: &mut Ui,
    r: Rect,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    let mut body = w::child(ui, r, "git-commit-details");
    let ui = &mut body;
    w::label(
        ui,
        pos2(r.left() + 10., r.top() + 15.),
        "Changed Files",
        12.,
        t.text,
    );
    let summary_h = (r.height() * 0.30).clamp(45., 90.);
    let mut list = w::child(
        ui,
        rect(
            r.left() + 6.,
            r.top() + 31.,
            r.width() - 12.,
            (r.height() - 31. - summary_h).max(0.),
        ),
        "committed-files",
    );
    list.spacing_mut().item_spacing.y = 0.;
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut list, |ui| {
            let mut dir = String::new();
            for f in &form.details {
                let (parent, name) = f.path.rsplit_once('/').unwrap_or(("", &f.path));
                if !parent.is_empty() && parent != dir {
                    let row = ui.allocate_space(vec2(ui.available_width(), 23.)).1;
                    w::icon(
                        ui,
                        pos2(row.left() + 9., row.center().y),
                        Icon::Folder,
                        t.muted,
                        13.,
                    );
                    w::elided_label(
                        ui,
                        rect(row.left() + 22., row.top(), row.width() - 24., 23.),
                        parent,
                        11.,
                        t.muted,
                    );
                    dir = parent.into();
                }
                let row = ui.allocate_space(vec2(ui.available_width(), 24.)).1;
                let response = ui.interact(row, ui.scope_id().with(&f.path), Sense::click());
                if repo.review.path == f.path
                    && (repo.review.hash == form.details_ref || form.details_compare)
                    || response.hovered()
                {
                    ui.painter().rect_filled(row, 3, t.surface);
                }
                let indent = if parent.is_empty() { 10. } else { 24. };
                let color = git_ui::file_color(f.status, t);
                w::icon(
                    ui,
                    pos2(row.left() + indent, row.center().y),
                    Icon::File,
                    color,
                    13.,
                );
                w::elided_label(
                    ui,
                    rect(
                        row.left() + indent + 13.,
                        row.top(),
                        row.width() - indent - 16.,
                        24.,
                    ),
                    name,
                    12.,
                    color,
                );
                response.clone().on_hover_text(&f.path);
                if response.clicked() {
                    *action = Some(Action::Preview(if !form.details_compare {
                        View::CommitFile(form.details_ref.clone(), f.path.clone())
                    } else if form.details_right.is_empty() {
                        View::Compare(form.details_ref.clone(), f.path.clone())
                    } else {
                        View::CompareRefs(
                            form.details_ref.clone(),
                            form.details_right.clone(),
                            f.path.clone(),
                        )
                    }));
                }
            }
            if form.details.is_empty() {
                ui.label(
                    egui::RichText::new("Select a commit to see its files")
                        .size(12.)
                        .color(t.muted),
                );
            }
        });
    let summary = rect(
        r.left() + 8.,
        r.bottom() - summary_h,
        r.width() - 16.,
        summary_h,
    );
    w::line(ui, summary.left_top(), summary.right_top(), t.border);
    let mut text = w::child(ui, summary.shrink2(vec2(0., 5.)), "commit-message");
    egui::ScrollArea::vertical().show(&mut text, |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(&form.details_text)
                    .size(11.)
                    .color(t.muted),
            )
            .selectable(true)
            .wrap(),
        );
    });
}
