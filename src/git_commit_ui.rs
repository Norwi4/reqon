use crate::{
    git::{Action, State as Repository, View},
    git_ops::Selection,
    git_ui::{self, Dialog, State},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

pub fn checkbox(ui: &mut Ui, r: Rect, selected: &mut bool, t: Theme) -> bool {
    let response = ui.interact(r, ui.scope_id().with("include"), Sense::click());
    if response.clicked() {
        *selected = !*selected;
    }
    let square = Rect::from_center_size(r.center(), vec2(13., 13.));
    ui.painter().rect(
        square,
        2,
        if *selected {
            t.blue.gamma_multiply(0.22)
        } else {
            t.bg
        },
        Stroke::new(
            1.,
            if *selected {
                t.blue
            } else {
                t.faint.gamma_multiply(0.65)
            },
        ),
        StrokeKind::Inside,
    );
    if *selected {
        w::icon(ui, square.center(), Icon::Check, t.text, 11.);
    }
    response.clicked()
}

pub fn show(
    ui: &mut Ui,
    r: Rect,
    repo: &Repository,
    form: &mut State,
    action: &mut Option<Action>,
    t: Theme,
) {
    let mut child = w::child(ui, r, "git-commit-panel");
    let ui = &mut child;
    w::semibold(
        ui,
        pos2(r.left() + 12., r.top() + 17.),
        "Commit",
        13.,
        t.text,
    );
    if w::icon_button(
        ui,
        rect(r.right() - 59., r.top() + 3., 25., 27.),
        Icon::Refresh,
        "Refresh changes",
        t,
    )
    .clicked()
    {
        *action = Some(Action::Refresh);
    }
    if w::icon_button(
        ui,
        rect(r.right() - 31., r.top() + 3., 25., 27.),
        Icon::Settings,
        "Git settings",
        t,
    )
    .clicked()
    {
        form.dialog = Some(Dialog::Settings);
    }
    w::line(
        ui,
        pos2(r.left(), r.top() + 34.),
        pos2(r.right(), r.top() + 34.),
        t.border,
    );
    let message_h = (r.height() * 0.30).clamp(82., 170.);
    let message_top = (r.bottom() - message_h - 70.).max(r.top() + 100.);
    let list = rect(
        r.left() + 6.,
        r.top() + 38.,
        r.width() - 12.,
        (message_top - r.top() - 43.).max(0.),
    );
    let mut list_ui = w::child(ui, list, "git-included-files");
    list_ui.spacing_mut().item_spacing.y = 0.;
    egui::ScrollArea::vertical().auto_shrink([false,false]).show(&mut list_ui,|ui| {
        for (group, untracked) in [("Changes",false),("Unversioned Files",true)] {
            let files:Vec<_>=repo.files.iter().filter(|f| (f.index=='?')==untracked).collect();
            if files.is_empty() && untracked { continue; }
            ui.push_id(group,|ui| {
                let header=ui.allocate_space(vec2(ui.available_width(),27.)).1;
                let mut all=!files.is_empty() && files.iter().all(|f|form.selected.contains(&f.path));
                if checkbox(ui,rect(header.left()+13.,header.top(),20.,27.),&mut all,t) {
                    for file in &files { if all {form.selected.insert(file.path.clone());} else {form.selected.remove(&file.path);} }
                }
                let group_click=ui.interact(rect(header.left()+35.,header.top(),header.width()-35.,27.),ui.scope_id().with("group"),Sense::click());
                let arrow_click=ui.interact(rect(header.left(),header.top(),13.,27.),ui.scope_id().with("group-arrow"),Sense::click());
                let collapsed=form.collapsed.contains(group);
                w::icon(ui,pos2(header.left()+6.,header.center().y),if collapsed{Icon::Chevron}else{Icon::Down},t.muted,10.);
                w::medium(ui,pos2(header.left()+36.,header.center().y),group,12.,t.text);
                w::label(ui,pos2(header.right()-24.,header.center().y),&files.len().to_string(),11.,t.muted);
                if group_click.clicked() || arrow_click.clicked() { if collapsed {form.collapsed.remove(group);} else {form.collapsed.insert(group.into());} }
                if !collapsed {
                    let mut last_dir=String::new();
                    for file in files {
                        let path=file.path.replace('\\',"/");
                        let (dir,name)=path.rsplit_once('/').unwrap_or(("",&path));
                        let folder_key=format!("{group}/{dir}");
                        if !dir.is_empty() && dir!=last_dir {
                            let row=ui.allocate_space(vec2(ui.available_width(),24.)).1;
                            let response=ui.interact(row,ui.scope_id().with(&folder_key),Sense::click());
                            w::icon(ui,pos2(row.left()+24.,row.center().y),if form.collapsed.contains(&folder_key){Icon::Chevron}else{Icon::Down},t.muted,10.);
                            w::icon(ui,pos2(row.left()+41.,row.center().y),Icon::Folder,t.muted,14.);
                            w::elided_label(ui,rect(row.left()+54.,row.top(),row.width()-57.,24.),dir,11.,t.muted);
                            if response.clicked() { if !form.collapsed.remove(&folder_key) {form.collapsed.insert(folder_key.clone());} }
                            last_dir=dir.into();
                        }
                        if form.collapsed.contains(&folder_key) { continue; }
                        let row=ui.allocate_space(vec2(ui.available_width(),24.)).1;
                        if !ui.is_rect_visible(row) {continue;}
                        ui.push_id(&file.path,|ui| {
                            let indent=if dir.is_empty(){29.}else{46.};
                            let selected=matches!(&repo.view,View::Working(p) if p==&file.path);
                            let response=ui.interact(rect(row.left()+indent+21.,row.top(),(row.width()-indent-21.).max(0.),24.),ui.scope_id().with("file"),Sense::click());
                            if selected || response.hovered() {ui.painter().rect_filled(row,3,if selected {t.blue.gamma_multiply(0.15)}else{t.hover});}
                            let mut included=form.selected.contains(&file.path);
                            if checkbox(ui,rect(row.left()+indent,row.top(),20.,24.),&mut included,t) {if included {form.selected.insert(file.path.clone());}else{form.selected.remove(&file.path);}}
                            let code=if file.conflict(){'!'}else if file.index=='?' || file.index=='A' {'A'}else if file.index=='D'||file.work=='D' {'D'}else{'M'};
                            let color=git_ui::file_color(code,t);
                            w::icon(ui,pos2(row.left()+indent+30.,row.center().y),Icon::File,color,13.);
                            w::elided_label(ui,rect(row.left()+indent+42.,row.top(),row.width()-indent-44.,24.),name,12.,color);
                            response.clone().on_hover_text(&file.path);
                            if response.clicked() { *action=Some(Action::Preview(View::Working(file.path.clone()))); }
                            response.context_menu(|ui| {
                                ui.set_width(245.);ui.spacing_mut().item_spacing.y=0.;
                                if git_ui::menu_row(ui,"Show Diff",Icon::Split,true,t) {*action=Some(Action::Preview(View::Working(file.path.clone())));}
                                if git_ui::menu_row(ui,"Copy Path",Icon::Copy,true,t) {ui.ctx().copy_text(file.path.clone());}
                                ui.separator();
                                if git_ui::menu_row(ui,if file.conflict(){"Mark Resolved"}else{"Stage File"},Icon::Plus,!repo.busy,t) {*action=Some(Action::StageFile(file.path.clone()));}
                                if git_ui::menu_row(ui,"Unstage File",Icon::Undo,file.staged()&&!repo.busy,t) {*action=Some(Action::Unstage(Some(file.path.clone())));}
                                if git_ui::menu_row(ui,"Rollback…",Icon::Undo,!untracked&&!repo.busy,t) {form.confirm=Some((format!("Discard unstaged changes in {}? This cannot be undone.",file.path),Action::Discard(file.path.clone())));}
                            });
                        });
                    }
                    if repo.files.is_empty() {ui.add_space(12.);ui.label(egui::RichText::new("No changes").size(12.).color(t.muted));}
                }
            });
        }
    });
    w::line(
        ui,
        pos2(r.left(), message_top),
        pos2(r.right(), message_top),
        t.border,
    );
    let amend = rect(r.left() + 8., message_top + 5., 20., 25.);
    if checkbox(ui, amend, &mut form.amend, t) && form.amend && form.message.trim().is_empty() {
        if let Some(commit) = repo.commits.iter().find(|c| c.refs.contains("HEAD")) {
            form.message.clone_from(&commit.subject);
        }
    }
    w::label(
        ui,
        pos2(amend.right() + 2., amend.center().y),
        "Amend last commit",
        12.,
        t.text,
    );
    let message = rect(
        r.left() + 10.,
        message_top + 33.,
        r.width() - 20.,
        (r.bottom() - message_top - 72.).max(24.),
    );
    ui.put(
        message,
        egui::TextEdit::multiline(&mut form.message)
            .hint_text("Commit message")
            .font(egui::FontId::proportional(12.))
            .desired_width(message.width())
            .margin(vec2(8., 7.)),
    );
    let enabled = !repo.busy
        && (!form.selected.is_empty() || form.amend)
        && !form.message.trim().is_empty()
        && repo.operation.is_empty()
        && !repo.files.iter().any(|f| f.conflict());
    let buttons = rect(r.left() + 10., r.bottom() - 33., r.width() - 20., 27.);
    let mut buttons_ui = w::child(ui, buttons, "git-commit-buttons");
    if !enabled {
        buttons_ui.disable();
    }
    let width = (buttons.width() * 0.34).max(62.);
    let commit = w::button(
        &mut buttons_ui,
        rect(buttons.left(), buttons.top(), width, 27.),
        "Commit",
        false,
        t,
    )
    .clicked();
    let push = w::button(
        &mut buttons_ui,
        rect(
            buttons.left() + width + 6.,
            buttons.top(),
            buttons.width() - width - 6.,
            27.,
        ),
        "Commit and Push",
        false,
        t,
    )
    .clicked();
    if commit || push {
        if push && repo.remotes.is_empty() {
            form.dialog = Some(Dialog::Remotes);
        } else {
            let selection = Selection {
                files: form.selected.iter().cloned().collect(),
                message: form.message.clone(),
                amend: form.amend,
                push: push.then(|| repo.push_remote.clone()),
            };
            if form.amend {
                form.confirm=Some(("Replace the last commit with the selected changes and message? This rewrites that commit.".into(),Action::CommitSelected(selection)));
            } else {
                *action = Some(Action::CommitSelected(selection));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn group_arrow_and_text_toggle_but_checkbox_only_selects_files() {
        let ctx = egui::Context::default();
        let mut form = State::default();
        let mut action = None;
        let repo = Repository {
            files: vec![crate::git::File {
                path: "file.req".into(),
                index: ' ',
                work: 'M',
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut time = 0.;
        let mut frame = |form: &mut State, events| {
            time += 0.02;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect(0., 0., 700., 700.)),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    show(
                        ui,
                        rect(0., 0., 300., 500.),
                        &repo,
                        form,
                        &mut action,
                        Theme::new(true),
                    );
                },
            );
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut form, vec![]);
        }
        for (point, collapsed, included) in [
            (pos2(12., 51.), true, false),
            (pos2(75., 51.), false, false),
            (pos2(29., 51.), false, true),
            (pos2(12., 51.), true, true),
        ] {
            frame(&mut form, vec![egui::Event::PointerMoved(point)]);
            for pressed in [true, false] {
                frame(
                    &mut form,
                    vec![egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            assert_eq!(form.collapsed.contains("Changes"), collapsed);
            assert_eq!(form.selected.contains("file.req"), included);
        }
    }
}
