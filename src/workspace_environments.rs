use crate::{
    app::App,
    model::Environment,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};
impl App {
    pub fn workspace_environments(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        let width = 230f32.min(r.width() * 0.4);
        let side = rect(r.left(), r.top(), width, r.height());
        w::line(ui, side.right_top(), side.right_bottom(), t.border);
        w::icon(
            ui,
            pos2(side.left() + 22., side.top() + 24.),
            Icon::Down,
            t.muted,
            11.,
        );
        w::semibold(
            ui,
            pos2(side.left() + 35., side.top() + 24.),
            "ENVIRONMENTS",
            11.,
            t.text,
        );
        for (i, (icon, title)) in [
            (Icon::Plus, "Create environment"),
            (Icon::Download, "Export environment"),
            (Icon::Upload, "Import environment"),
        ]
        .into_iter()
        .enumerate()
        {
            if w::icon_button(
                ui,
                rect(
                    side.right() - 81. + i as f32 * 24.,
                    side.top() + 12.,
                    22.,
                    24.,
                ),
                icon,
                title,
                t,
            )
            .clicked()
            {
                match i {
                    0 => {
                        self.workspaces.env_edit = Some(usize::MAX);
                        self.workspaces.env_name.clear();
                    }
                    1 => {
                        let env = self
                            .workspaces
                            .environment_index
                            .and_then(|i| self.ws.environments.get(i))
                            .cloned()
                            .unwrap_or(Environment {
                                name: "Global Variables".into(),
                                vars: self.ws.globals.clone(),
                            });
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!("{}.req", crate::model::safe_name(&env.name)))
                            .add_filter("Reqon environment", &["req"])
                            .save_file()
                        {
                            if let Err(e) = crate::storage::atomic_write(
                                &path,
                                crate::environments::bru_environment_source(&env),
                            ) {
                                self.notice(format!("Export failed: {e:#}"));
                            }
                        }
                    }
                    _ => {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter(
                                "Environment",
                                &["req", "bru", "yml", "yaml", "json", "env"],
                            )
                            .pick_file()
                        {
                            match crate::environments::import_environment(&path) {
                                Ok(mut env) => {
                                    env.name = unique_name(&self.ws.environments, &env.name);
                                    self.ws.environments.push(env);
                                    self.workspaces.environment_index =
                                        Some(self.ws.environments.len() - 1);
                                    self.mark_workspace_changed();
                                }
                                Err(e) => self.notice(format!("Import failed: {e:#}")),
                            }
                        }
                    }
                }
            }
        }
        ui.put(
            rect(side.left() + 12., side.top() + 45., side.width() - 24., 28.),
            egui::TextEdit::singleline(&mut self.workspaces.env_search)
                .hint_text("Search environments…")
                .font(egui::FontId::proportional(12.)),
        );
        let mut list = w::child(
            ui,
            rect(
                side.left() + 12.,
                side.top() + 82.,
                side.width() - 24.,
                side.height() - 94.,
            ),
            "workspace-env-list",
        );
        list.spacing_mut().item_spacing.y = 1.;
        egui::ScrollArea::vertical().show(&mut list, |ui| {
            let mut entries = vec![(None, "Global Variables".to_owned())];
            entries.extend(
                self.ws
                    .environments
                    .iter()
                    .enumerate()
                    .map(|(i, e)| (Some(i), e.name.clone())),
            );
            for (index, name) in entries {
                if !name
                    .to_lowercase()
                    .contains(&self.workspaces.env_search.to_lowercase())
                {
                    continue;
                }
                let row = ui.allocate_space(vec2(ui.available_width(), 32.)).1;
                let response = ui.interact(row, ui.scope_id().with(index), Sense::click());
                if self.workspaces.environment_index == index || response.hovered() {
                    ui.painter().rect_filled(row, 4, t.surface);
                }
                let active = index.is_some() && self.ws.workspace_environment == name;
                if active {
                    w::icon(
                        ui,
                        pos2(row.left() + 10., row.center().y),
                        Icon::Check,
                        t.accent,
                        13.,
                    );
                }
                w::elided_label(
                    ui,
                    rect(row.left() + 24., row.top(), row.width() - 26., row.height()),
                    &name,
                    13.,
                    if active { t.accent } else { t.text },
                );
                if response.clicked() {
                    self.workspaces.environment_index = index;
                    self.open_environment(
                        crate::environment_tabs::Target {
                            scope: if index.is_some() {
                                crate::environment_tabs::Scope::Workspace
                            } else {
                                crate::environment_tabs::Scope::Globals
                            },
                            name: if index.is_some() {
                                name.clone()
                            } else {
                                "Globals".into()
                            },
                        },
                        false,
                    );
                }
                if let Some(i) = index {
                    response.context_menu(|ui| {
                        if ui
                            .button(if active { "Deactivate" } else { "Activate" })
                            .clicked()
                        {
                            self.ws.workspace_environment =
                                if active { String::new() } else { name.clone() };
                            self.mark_workspace_changed();
                            ui.close();
                        }
                        if ui.button("Rename").clicked() {
                            self.workspaces.env_edit = Some(i);
                            self.workspaces.env_name = name.clone();
                            ui.close();
                        }
                        if ui.button("Duplicate").clicked() {
                            let mut copy = self.ws.environments[i].clone();
                            copy.name =
                                unique_name(&self.ws.environments, &format!("{} Copy", copy.name));
                            self.ws.environments.push(copy);
                            self.workspaces.environment_index =
                                Some(self.ws.environments.len() - 1);
                            self.mark_workspace_changed();
                            ui.close();
                        }
                        if ui.button("Delete").clicked() {
                            self.workspaces.env_remove = Some(i);
                            ui.close();
                        }
                    });
                }
            }
        });
        let body = rect(
            side.right() + 20.,
            r.top() + 8.,
            r.right() - side.right() - 40.,
            r.height() - 16.,
        );
        let index = self
            .workspaces
            .environment_index
            .filter(|&i| i < self.ws.environments.len());
        self.workspaces.environment_index = index;
        let name = index
            .map(|i| self.ws.environments[i].name.clone())
            .unwrap_or("Global Variables".into());
        w::label(ui, pos2(body.left(), body.top() + 16.), &name, 13., t.text);
        if index.is_some() {
            let active = self.ws.workspace_environment == name;
            if w::button(
                ui,
                rect(body.right() - 106., body.top() + 2., 106., 28.),
                if active { "Deactivate" } else { "Activate" },
                false,
                t,
            )
            .clicked()
            {
                self.ws.workspace_environment = if active { String::new() } else { name };
                self.mark_workspace_changed();
            }
        }
        for (tab, label) in ["Variables", "Secrets"].iter().enumerate() {
            if w::tab(
                ui,
                rect(body.left() + tab as f32 * 93., body.top() + 37., 75., 32.),
                label,
                self.workspaces.env_tab == tab,
                t,
            )
            .clicked()
            {
                self.workspaces.env_tab = tab;
            }
        }
        let secret = self.workspaces.env_tab == 1;
        let vars = if let Some(i) = index {
            &mut self.ws.environments[i].vars
        } else {
            &mut self.ws.globals
        };
        let mut shown: Vec<_> = vars
            .iter()
            .filter(|p| p.secret == secret)
            .cloned()
            .collect();
        let mut table = w::child(
            ui,
            rect(
                body.left(),
                body.top() + 83.,
                body.width(),
                body.height() - 140.,
            ),
            "workspace-env-table",
        );
        let changed = egui::ScrollArea::vertical()
            .show(&mut table, |ui| {
                w::pairs(ui, &mut shown, ["Name", "Value"], t)
            })
            .inner;
        if changed {
            for p in &mut shown {
                p.secret = secret;
            }
            vars.retain(|p| p.secret != secret);
            vars.extend(shown);
            self.mark_workspace_changed();
        }
        if secret {
            w::label(
                ui,
                pos2(body.left(), body.bottom() - 45.),
                "Secret values are stored locally and omitted from exports.",
                11.,
                t.muted,
            );
        }
        if w::button(
            ui,
            rect(body.left(), body.bottom() - 28., 67., 28.),
            "Save",
            true,
            t,
        )
        .clicked()
        {
            self.mark_workspace_changed();
            self.flush();
            self.notice("Workspace environment saved");
        }
        if let Some(edit) = self.workspaces.env_edit {
            let (submit, close) = w::modal(
                ui.ctx(),
                if edit == usize::MAX {
                    "Create Environment"
                } else {
                    "Rename Environment"
                },
                400.,
                t,
                |ui| {
                    ui.label("Name");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.workspaces.env_name)
                            .desired_width(f32::INFINITY),
                    );
                    let name = self.workspaces.env_name.trim();
                    let duplicate = self
                        .ws
                        .environments
                        .iter()
                        .enumerate()
                        .any(|(i, e)| i != edit && e.name.eq_ignore_ascii_case(name));
                    if duplicate {
                        ui.colored_label(t.red, "An environment with this name already exists");
                    }
                    ui.add_enabled(!name.is_empty() && !duplicate, egui::Button::new("Save"))
                        .clicked()
                },
            );
            if submit {
                let name = self.workspaces.env_name.trim().to_owned();
                if edit == usize::MAX {
                    self.ws
                        .environments
                        .push(Environment { name, vars: vec![] });
                    self.workspaces.environment_index = Some(self.ws.environments.len() - 1);
                } else if let Some(env) = self.ws.environments.get_mut(edit) {
                    if self.ws.workspace_environment == env.name {
                        self.ws.workspace_environment = name.clone();
                    }
                    env.name = name;
                }
                self.workspaces.env_edit = None;
                self.mark_workspace_changed();
            } else if close {
                self.workspaces.env_edit = None;
            }
        }
        if let Some(index) = self.workspaces.env_remove {
            let (remove, close) = w::modal(ui.ctx(), "Delete Environment", 400., t, |ui| {
                ui.label("Delete this workspace environment and its variables?");
                ui.button("Delete").clicked()
            });
            if remove {
                if index < self.ws.environments.len() {
                    let env = self.ws.environments.remove(index);
                    if self.ws.workspace_environment == env.name {
                        self.ws.workspace_environment.clear();
                    }
                }
                self.workspaces.environment_index = None;
                self.workspaces.env_remove = None;
                self.mark_workspace_changed();
            } else if close {
                self.workspaces.env_remove = None;
            }
        }
    }
}
fn unique_name(environments: &[Environment], name: &str) -> String {
    let mut candidate = name.to_owned();
    let mut n = 2;
    while environments
        .iter()
        .any(|e| e.name.eq_ignore_ascii_case(&candidate))
    {
        candidate = format!("{name} {n}");
        n += 1;
    }
    candidate
}
