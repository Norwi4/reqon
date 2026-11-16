use crate::{
    app::App,
    widgets::{self as w, Icon, rect},
    workspaces::{Command, Dialog, Job},
};
use eframe::egui::{self, Rect, Response, Sense, Ui, pos2, vec2};
fn menu_row(ui: &mut Ui, label: &str, icon: Icon, t: crate::theme::Theme) -> bool {
    let r = ui.allocate_space(vec2(ui.available_width(), 28.8)).1;
    let response = ui.interact(r, ui.scope_id().with(label), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(r, 4, t.surface);
    }
    w::icon(ui, pos2(r.left() + 14., r.center().y), icon, t.muted, 14.);
    w::label(ui, pos2(r.left() + 30., r.center().y), label, 12., t.text);
    if response.clicked() {
        ui.close();
        true
    } else {
        false
    }
}
impl App {
    pub fn workspace_menu(&mut self, _ui: &mut Ui, response: &Response) {
        let t = self.theme;
        egui::Popup::menu(response)
            .gap(9.)
            .frame(crate::menu::frame(t))
            .show(|ui| {
                let width = self
                    .workspaces
                    .registry
                    .entries
                    .iter()
                    .map(|e| w::text_width(ui, &e.name, 12.) + 54.)
                    .fold(158., f32::max)
                    .min(310.);
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.;
                if self.workspaces.job.is_some() {
                    ui.disable();
                }
                for entry in self.workspaces.registry.ordered() {
                    let r = ui.allocate_space(vec2(ui.available_width(), 28.8)).1;
                    let active = entry.id == self.workspaces.registry.active;
                    let response = ui.interact(
                        rect(r.left(), r.top(), r.width() - 48., r.height()),
                        ui.scope_id().with(&entry.id),
                        Sense::click_and_drag(),
                    );
                    if active || response.hovered() {
                        ui.painter().rect_filled(r, 4, t.surface);
                    }
                    w::elided_label(
                        ui,
                        rect(r.left() + 8., r.top(), r.width() - 58., r.height()),
                        &entry.name,
                        12.,
                        if active { t.accent } else { t.text },
                    );
                    if active {
                        w::icon(
                            ui,
                            pos2(r.right() - 13., r.center().y),
                            Icon::Check,
                            t.accent,
                            16.,
                        );
                    }
                    if !entry.default
                        && (entry.pinned
                            || r.contains(ui.ctx().pointer_hover_pos().unwrap_or_default()))
                    {
                        if w::icon_button(
                            ui,
                            rect(r.right() - 48., r.top() + 3., 23., 23.),
                            if entry.pinned {
                                Icon::Pinned
                            } else {
                                Icon::Pin
                            },
                            if entry.pinned {
                                "Unpin workspace"
                            } else {
                                "Pin workspace"
                            },
                            t,
                        )
                        .clicked()
                        {
                            self.workspaces.command = Some(Command::Pin(entry.id.clone()));
                        }
                    }
                    if response.clicked() {
                        self.workspaces.command = Some(Command::Switch(entry.id.clone()));
                        ui.close();
                    }
                    if response.drag_started() && !entry.default {
                        response.dnd_set_drag_payload(entry.id.clone());
                    }
                    if let Some(source) = response.dnd_release_payload::<String>() {
                        self.workspaces.command =
                            Some(Command::Reorder((*source).clone(), entry.id.clone()));
                    }
                }
                let heading = ui.allocate_space(vec2(ui.available_width(), 29.)).1;
                w::semibold(
                    ui,
                    pos2(heading.left() + 8., heading.center().y + 2.),
                    "Workspaces",
                    11.,
                    t.muted,
                );
                for (label, icon, command) in [
                    ("Create workspace", Icon::Plus, Command::Create),
                    ("Open workspace", Icon::Folder, Command::Open),
                    ("Import workspace", Icon::Download, Command::Import),
                    ("Manage workspaces", Icon::Settings, Command::Manage),
                ] {
                    if menu_row(ui, label, icon, t) {
                        self.workspaces.command = Some(command);
                    }
                }
            });
    }
    pub fn workspace_actions_menu(&mut self, _ui: &mut Ui, response: &Response) {
        let t = self.theme;
        let id = self.workspaces.registry.active.clone();
        let default = id == "default";
        egui::Popup::menu(response)
            .frame(crate::menu::frame(t))
            .show(|ui| {
                ui.set_width(195.);
                ui.spacing_mut().item_spacing.y = 0.;
                if menu_row(ui, "Export workspace", Icon::Download, t) {
                    self.workspaces.command = Some(Command::Export);
                }
                if menu_row(ui, "Reveal in File Explorer", Icon::Folder, t) {
                    self.workspaces.command = Some(Command::Reveal(id.clone()));
                }
                if menu_row(ui, "Open in Terminal", Icon::Terminal, t) {
                    self.workspaces.command = Some(Command::Terminal(id.clone()));
                }
                if !default {
                    if menu_row(ui, "Rename", Icon::Edit, t) {
                        self.workspaces.command = Some(Command::Rename(id.clone()));
                    }
                    if menu_row(ui, "Close workspace", Icon::Close, t) {
                        self.workspaces.command = Some(Command::Remove(id.clone()));
                    }
                }
            });
    }
    pub fn workspace_manager(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        if w::icon_button(
            ui,
            rect(r.left() + 15., r.top() + 12., 25., 29.),
            Icon::ArrowLeft,
            "Back",
            t,
        )
        .clicked()
        {
            self.workspaces.manage = false;
        }
        w::semibold(
            ui,
            pos2(r.left() + 50., r.top() + 26.),
            "Manage Workspace",
            15.,
            t.text,
        );
        if w::button(
            ui,
            rect(r.right() - 167., r.top() + 12., 151., 29.),
            "+  Create Workspace",
            true,
            t,
        )
        .clicked()
        {
            self.workspaces.command = Some(Command::Create);
        }
        w::line(
            ui,
            pos2(r.left(), r.top() + 53.),
            pos2(r.right(), r.top() + 53.),
            t.border,
        );
        let mut list = w::child(
            ui,
            rect(
                r.left() + 16.,
                r.top() + 54.,
                r.width() - 32.,
                r.height() - 54.,
            ),
            "manage-workspaces",
        );
        list.spacing_mut().item_spacing.y = 0.;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut list, |ui| {
                for entry in self.workspaces.registry.ordered() {
                    ui.push_id(&entry.id, |ui| {
                        let row = ui.allocate_space(vec2(ui.available_width(), 65.)).1;
                        w::icon(
                            ui,
                            pos2(row.left() + 7., row.top() + 23.),
                            if entry.default {
                                Icon::Lock
                            } else {
                                Icon::Workspace
                            },
                            if entry.default { t.muted } else { t.accent },
                            14.,
                        );
                        w::elided_medium(
                            ui,
                            rect(row.left() + 20., row.top() + 10., row.width() - 230., 27.),
                            &entry.name,
                            14.,
                            t.text,
                        );
                        if entry.default {
                            let badge = rect(
                                (row.left() + 25. + w::text_width(ui, &entry.name, 14.))
                                    .min(row.right() - 200.),
                                row.top() + 13.,
                                49.,
                                19.,
                            );
                            ui.painter().rect_filled(badge, 3, t.surface);
                            w::label(
                                ui,
                                pos2(badge.left() + 5., badge.center().y),
                                "Default",
                                10.,
                                t.muted,
                            );
                        }
                        w::elided_label(
                            ui,
                            rect(row.left(), row.top() + 35., row.width() - 180., 20.),
                            &entry.path.to_string_lossy(),
                            11.,
                            t.text,
                        );
                        let open = rect(
                            row.right() - if entry.default { 61. } else { 127. },
                            row.top() + 19.,
                            59.,
                            27.,
                        );
                        let response =
                            ui.interact(open, ui.scope_id().with("open"), Sense::click());
                        if response.hovered() {
                            ui.painter().rect_filled(open, 4, t.hover);
                        }
                        w::icon(
                            ui,
                            pos2(open.left() + 12., open.center().y),
                            Icon::Login,
                            t.muted,
                            14.,
                        );
                        w::label(
                            ui,
                            pos2(open.left() + 25., open.center().y),
                            "Open",
                            11.,
                            t.text,
                        );
                        if response.clicked() {
                            self.workspaces.command = Some(Command::Switch(entry.id.clone()));
                        }
                        if !entry.default {
                            if w::icon_button(
                                ui,
                                rect(row.right() - 64., row.top() + 20., 28., 26.),
                                Icon::Folder,
                                "Reveal in File Explorer",
                                t,
                            )
                            .clicked()
                            {
                                self.workspaces.command = Some(Command::Reveal(entry.id.clone()));
                            }
                            let more = w::icon_button(
                                ui,
                                rect(row.right() - 29., row.top() + 20., 28., 26.),
                                Icon::Dots,
                                "Workspace actions",
                                t,
                            );
                            egui::Popup::menu(&more)
                                .frame(crate::menu::frame(t))
                                .show(|ui| {
                                    ui.set_width(170.);
                                    ui.spacing_mut().item_spacing.y = 0.;
                                    for (label, icon, command) in [
                                        (
                                            "Open in Terminal",
                                            Icon::Terminal,
                                            Command::Terminal(entry.id.clone()),
                                        ),
                                        ("Rename", Icon::Edit, Command::Rename(entry.id.clone())),
                                        ("Remove", Icon::Trash, Command::Remove(entry.id.clone())),
                                    ] {
                                        if menu_row(ui, label, icon, t) {
                                            self.workspaces.command = Some(command);
                                        }
                                    }
                                });
                        }
                        w::line(
                            ui,
                            pos2(row.left(), row.bottom()),
                            pos2(row.right(), row.bottom()),
                            t.border,
                        );
                    });
                }
                if !self.workspaces.loading.is_empty() {
                    ui.add_space(16.);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(&self.workspaces.loading);
                    });
                }
                if !self.workspaces.error.is_empty() {
                    ui.add_space(12.);
                    ui.colored_label(t.red, &self.workspaces.error);
                }
            });
    }
    pub fn workspace_dialogs(&mut self, ctx: &egui::Context) {
        let Some(dialog) = self.workspaces.dialog.clone() else {
            return;
        };
        let t = self.theme;
        let title = match &dialog {
            Dialog::Create => "Create Workspace",
            Dialog::Import => "Import Workspace",
            Dialog::Rename(_) => "Rename Workspace",
            Dialog::Remove(_) => "Remove Workspace",
            Dialog::Move(_) => "Move into Workspace",
        };
        let (result, close) = w::modal(ctx, title, 480., t, |ui| -> Option<anyhow::Result<()>> {
            if !self.workspaces.error.is_empty() {
                ui.colored_label(t.red, &self.workspaces.error);
            }
            if !self.workspaces.loading.is_empty() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&self.workspaces.loading);
                });
                ui.disable();
            }
            match &dialog {
                Dialog::Create => {
                    ui.label("Name");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.workspaces.name)
                            .desired_width(f32::INFINITY)
                            .hint_text("Workspace name"),
                    );
                    if response.changed() && !self.workspaces.folder_custom {
                        self.workspaces.folder = crate::model::safe_name(&self.workspaces.name);
                    }
                    ui.label("Folder Name");
                    ui.horizontal(|ui| {
                        ui.add_enabled(
                            self.workspaces.folder_custom,
                            egui::TextEdit::singleline(&mut self.workspaces.folder)
                                .desired_width(ui.available_width() - 33.),
                        );
                        let edit = ui.allocate_space(vec2(25., 25.)).1;
                        if w::icon_button(ui, edit, Icon::Edit, "Edit folder name", t).clicked() {
                            self.workspaces.folder_custom = true;
                        }
                    });
                    ui.label("Location");
                    location(ui, &mut self.workspaces.location);
                    if ui
                        .add_enabled(
                            !self.workspaces.name.trim().is_empty()
                                && !self.workspaces.location.is_empty(),
                            egui::Button::new("Create"),
                        )
                        .clicked()
                    {
                        let result = crate::workspace_store::create(
                            std::path::Path::new(&self.workspaces.location),
                            &self.workspaces.folder,
                            &self.workspaces.name,
                        );
                        return Some(result.map(|path| self.load_workspace(path)));
                    }
                }
                Dialog::Import => {
                    let drop = ui.allocate_space(vec2(ui.available_width(), 106.)).1;
                    ui.painter().rect_stroke(
                        drop,
                        5,
                        egui::Stroke::new(1., t.strong_border),
                        egui::StrokeKind::Inside,
                    );
                    w::icon(
                        ui,
                        pos2(drop.center().x, drop.top() + 28.),
                        Icon::Download,
                        t.muted,
                        24.,
                    );
                    w::label(
                        ui,
                        pos2(drop.left() + 18., drop.top() + 67.),
                        if self.workspaces.file.is_empty() {
                            "Drop a workspace ZIP here or click to choose"
                        } else {
                            "Workspace ZIP selected"
                        },
                        12.,
                        t.text,
                    );
                    if ui
                        .interact(
                            drop,
                            ui.scope_id().with("workspace-zip-drop"),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Workspace ZIP", &["zip"])
                            .pick_file()
                        {
                            self.workspaces.file = path.to_string_lossy().into_owned();
                        }
                    }
                    if let Some(path) =
                        ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_owned()))
                    {
                        self.workspaces.file = path.to_string_lossy().into_owned();
                    }
                    if !self.workspaces.file.is_empty() {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&self.workspaces.file)
                                    .size(11.)
                                    .color(t.muted),
                            )
                            .wrap(),
                        );
                    }
                    ui.label("Extract Location");
                    location(ui, &mut self.workspaces.location);
                    if ui
                        .add_enabled(
                            !self.workspaces.file.is_empty()
                                && !self.workspaces.location.is_empty(),
                            egui::Button::new("Import"),
                        )
                        .clicked()
                    {
                        let file = self.workspaces.file.clone();
                        let parent = self.workspaces.location.clone();
                        self.workspace_job("Importing workspace…", move || {
                            Ok(Job::Imported(crate::workspace_zip::import(
                                std::path::Path::new(&file),
                                std::path::Path::new(&parent),
                            )?))
                        });
                        return Some(Ok(()));
                    }
                }
                Dialog::Rename(id) => {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.workspaces.name)
                            .desired_width(f32::INFINITY),
                    );
                    if ui.button("Rename").clicked() {
                        let name = self.workspaces.name.clone();
                        return Some(self.rename_workspace(id, &name));
                    }
                }
                Dialog::Remove(id) => {
                    let name = self
                        .workspaces
                        .registry
                        .entries
                        .iter()
                        .find(|e| e.id == *id)
                        .map(|e| e.name.as_str())
                        .unwrap_or("Workspace");
                    ui.label(format!("Remove ‘{name}’ from the workspace list?"));
                    ui.label(egui::RichText::new("The workspace and its files will stay on disk. You can open it again at any time.").size(12.).color(t.muted));
                    if ui.button("Remove").clicked() {
                        return Some(self.remove_workspace(id));
                    }
                }
                Dialog::Move(cid) => {
                    ui.label("Choose the destination workspace");
                    let entries = self.workspaces.registry.ordered();
                    let mut any = false;
                    for entry in entries
                        .iter()
                        .filter(|e| e.id != self.workspaces.registry.active)
                    {
                        any = true;
                        if ui.button(&entry.name).clicked() {
                            self.workspaces.command =
                                Some(Command::Move(cid.clone(), entry.id.clone()));
                            return Some(Ok(()));
                        }
                    }
                    if !any {
                        ui.label("Create or open another workspace first.");
                    }
                }
            }
            None
        });
        if let Some(result) = result {
            match result {
                Ok(()) => {
                    self.workspaces.dialog = None;
                    self.workspaces.error.clear();
                }
                Err(e) => self.workspaces.error = format!("{e:#}"),
            }
        } else if close {
            self.workspaces.dialog = None;
            self.workspaces.error.clear();
        }
    }
}
fn location(ui: &mut Ui, value: &mut String) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(value).desired_width((ui.available_width() - 82.).max(100.)),
        );
        if ui.button("Browse").clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                *value = path.to_string_lossy().into_owned();
            }
        }
    });
}
