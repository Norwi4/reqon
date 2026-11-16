use crate::{
    app::App,
    menu,
    model::*,
    storage,
    widgets::{self as w, Icon, rect},
};
use anyhow::{Context, Result, ensure};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Globals,
    Workspace,
    Collection(String),
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub scope: Scope,
    pub name: String,
}
impl Target {
    pub fn id(&self) -> String {
        format!("__environment__{}", serde_json::to_string(self).unwrap())
    }
    pub fn parse(id: &str) -> Option<Self> {
        serde_json::from_str(id.strip_prefix("__environment__")?).ok()
    }
    pub fn collection(&self) -> Option<&str> {
        if let Scope::Collection(cid) = &self.scope {
            Some(cid)
        } else {
            None
        }
    }
}
#[derive(Clone, Default)]
struct Editor {
    name: String,
    focus: bool,
    remove: bool,
    error: String,
}

impl App {
    pub fn environment_toolbar(&mut self, ui: &mut Ui, r: Rect, cid: &str) {
        let t = self.theme;
        w::icon(
            ui,
            pos2(r.left() + 29., r.top() + 23.),
            Icon::Key,
            t.muted,
            18.,
        );
        w::medium(
            ui,
            pos2(r.left() + 54., r.top() + 23.),
            "Environments",
            13.,
            t.text,
        );
        // Only environment actions belong here: request settings cannot act on this tab.
        self.environment_selector(ui, rect(r.right() - 166., r.top() + 10., 150., 26.), cid);
    }
    fn environment_editor_key(&self, id: &str) -> egui::Id {
        egui::Id::unique((&self.root, id, "env-editor"))
    }
    pub fn commit_environment_name(&mut self, id: &str) -> String {
        let Some(target) = Target::parse(id) else {
            return id.into();
        };
        let Some(mut editor) = self
            .context
            .data_mut(|d| d.get_temp::<Editor>(self.environment_editor_key(id)))
        else {
            return id.into();
        };
        if target.scope == Scope::Globals || editor.name == target.name {
            return id.into();
        }
        match self.rename_environment(&target, &editor.name) {
            Ok(next) => {
                editor.name = next.name.clone();
                editor.error.clear();
                let new = next.id();
                self.context
                    .data_mut(|d| d.insert_temp(self.environment_editor_key(&new), editor));
                new
            }
            Err(e) => {
                self.notice(e.to_string());
                id.into()
            }
        }
    }
    pub(crate) fn environment_list(&self, scope: &Scope) -> &[Environment] {
        match scope {
            Scope::Workspace => &self.ws.environments,
            Scope::Collection(cid) => self
                .ws
                .collections
                .iter()
                .find(|c| &c.id == cid)
                .map(|c| c.environments.as_slice())
                .unwrap_or(&[]),
            Scope::Globals => &[],
        }
    }
    fn environment_list_mut(&mut self, scope: &Scope) -> Result<&mut Vec<Environment>> {
        match scope {
            Scope::Workspace => Ok(&mut self.ws.environments),
            Scope::Collection(cid) => Ok(&mut self
                .ws
                .collections
                .iter_mut()
                .find(|c| &c.id == cid)
                .context("Collection no longer exists")?
                .environments),
            Scope::Globals => anyhow::bail!("Global variables cannot be renamed or removed"),
        }
    }
    pub fn environment_rows(&self, target: &Target) -> Option<&Vec<Pair>> {
        if target.scope == Scope::Globals {
            Some(&self.ws.globals)
        } else {
            self.environment_list(&target.scope)
                .iter()
                .find(|e| e.name == target.name)
                .map(|e| &e.vars)
        }
    }
    pub fn set_environment_rows(&mut self, target: &Target, rows: Vec<Pair>) -> Result<()> {
        if target.scope == Scope::Globals {
            self.ws.globals = rows;
        } else {
            self.environment_list_mut(&target.scope)?
                .iter_mut()
                .find(|e| e.name == target.name)
                .context("Environment no longer exists")?
                .vars = rows;
        }
        self.environment_changed(&target.scope);
        Ok(())
    }
    fn environment_changed(&mut self, scope: &Scope) {
        self.runtime_vars.clear();
        if let Scope::Collection(cid) = scope {
            if let Some(c) = self.ws.collections.iter().find(|c| &c.id == cid) {
                self.writer.save_environments(c.clone());
            }
        }
        self.mark_workspace_changed();
    }
    pub fn environment_is_active(&self, target: &Target) -> bool {
        match target.scope {
            Scope::Globals => false,
            Scope::Workspace => self.ws.workspace_environment == target.name,
            Scope::Collection(_) => self.ws.selected_environment == target.name,
        }
    }
    pub fn activate_environment(&mut self, target: &Target, active: bool) {
        match target.scope {
            Scope::Globals => return,
            Scope::Workspace => {
                self.ws.workspace_environment = if active {
                    target.name.clone()
                } else {
                    String::new()
                }
            }
            Scope::Collection(_) => {
                self.ws.selected_environment = if active {
                    target.name.clone()
                } else {
                    String::new()
                }
            }
        }
        self.runtime_vars.clear();
        self.mark_workspace_changed();
    }
    pub fn open_environment(&mut self, target: Target, focus: bool) {
        self.sidebar_panel = crate::navigation::Panel::Environments;
        self.sidebar_visible = true;
        self.environment_tree.reveal(&target.scope);
        if let Some(cid) = target.collection() {
            self.environment_collection = cid.into();
        } else if self.environment_collection.is_empty() {
            self.environment_collection = self.active_collection_id();
        }
        let id = target.id();
        if focus {
            self.context.data_mut(|d| {
                d.insert_temp(
                    self.environment_editor_key(&id),
                    Editor {
                        name: target.name.clone(),
                        focus: true,
                        ..Default::default()
                    },
                )
            });
        }
        self.open(id);
    }
    pub fn open_environment_manager(&mut self, cid: &str) {
        if !cid.is_empty() {
            self.environment_collection = cid.into();
        }
        let scope = Scope::Collection(cid.into());
        let env = self
            .environment_list(&scope)
            .iter()
            .find(|e| e.name == self.ws.selected_environment)
            .or_else(|| self.environment_list(&scope).first());
        let target = if let Some(env) = env {
            Target {
                scope,
                name: env.name.clone(),
            }
        } else if let Some(env) = self
            .ws
            .environments
            .iter()
            .find(|e| e.name == self.ws.workspace_environment)
            .or(self.ws.environments.first())
        {
            Target {
                scope: Scope::Workspace,
                name: env.name.clone(),
            }
        } else {
            Target {
                scope: Scope::Globals,
                name: "Globals".into(),
            }
        };
        self.open_environment(target, false);
    }
    pub fn open_environment_secrets(&mut self, cid: &str) {
        // Text and secret variables now share the same editor and keep their order.
        self.open_environment_manager(cid);
    }
    pub fn create_environment(
        &mut self,
        scope: Scope,
        source: Option<Environment>,
    ) -> Result<Target> {
        let mut env = source.unwrap_or(Environment {
            name: "New Environment".into(),
            vars: vec![],
        });
        let base = env.name.clone();
        let mut n = 2;
        while self
            .environment_list(&scope)
            .iter()
            .any(|e| safe_name(&e.name).eq_ignore_ascii_case(&safe_name(&env.name)))
        {
            env.name = format!("{base} {n}");
            n += 1;
        }
        let target = Target {
            scope: scope.clone(),
            name: env.name.clone(),
        };
        self.environment_list_mut(&scope)?.push(env);
        self.environment_changed(&scope);
        self.env_search.clear();
        self.open_environment(target.clone(), true);
        Ok(target)
    }
    fn environment_files(&self, target: &Target) -> Vec<std::path::PathBuf> {
        target
            .collection()
            .and_then(|cid| self.ws.collections.iter().find(|c| c.id == cid))
            .map(|c| {
                ["req", "bru", "yml", "yaml"]
                    .iter()
                    .map(|ext| {
                        c.path
                            .join("environments")
                            .join(format!("{}.{ext}", safe_name(&target.name)))
                    })
                    .filter(|p| p.exists())
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn rename_environment(&mut self, target: &Target, name: &str) -> Result<Target> {
        let name = name.trim();
        ensure!(!name.is_empty(), "Enter an environment name");
        ensure!(
            safe_name(name) == name,
            "The name contains characters that cannot be used in a file name"
        );
        if name == target.name {
            return Ok(target.clone());
        }
        ensure!(
            !self
                .environment_list(&target.scope)
                .iter()
                .any(|e| e.name != target.name && safe_name(&e.name).eq_ignore_ascii_case(name)),
            "An environment with this name already exists"
        );
        self.writer
            .barrier()
            .recv()
            .context("Could not finish saving environment")?;
        for old in self.environment_files(target) {
            let new = old.with_file_name(format!(
                "{name}.{}",
                old.extension().unwrap().to_string_lossy()
            ));
            storage::move_file(&old, &new)?;
        }
        self.environment_list_mut(&target.scope)?
            .iter_mut()
            .find(|e| e.name == target.name)
            .context("Environment no longer exists")?
            .name = name.into();
        let next = Target {
            scope: target.scope.clone(),
            name: name.into(),
        };
        if self.environment_is_active(target) {
            self.activate_environment(&next, true);
        }
        let (old, new) = (target.id(), next.id());
        for id in &mut self.ws.open_tabs {
            if *id == old {
                *id = new.clone();
            }
        }
        if self.ws.active.as_deref() == Some(&old) {
            self.ws.active = Some(new);
        }
        self.environment_changed(&target.scope);
        Ok(next)
    }
    pub fn remove_environment(&mut self, target: &Target) -> Result<()> {
        ensure!(
            target.scope != Scope::Globals,
            "Global variables cannot be removed"
        );
        self.writer
            .barrier()
            .recv()
            .context("Could not finish saving environment")?;
        let archive = self.root.join("removed-environments").join(id());
        storage::save(&archive, &self.ws)?;
        for path in self.environment_files(target) {
            storage::move_file(&path, &archive.join(path.file_name().unwrap()))?;
        }
        self.environment_list_mut(&target.scope)?
            .retain(|e| e.name != target.name);
        if self.environment_is_active(target) {
            self.activate_environment(target, false);
        }
        self.close(&target.id());
        self.environment_changed(&target.scope);
        self.flush();
        Ok(())
    }
    pub fn environment_selector(&mut self, ui: &mut Ui, r: Rect, cid: &str) {
        let t = self.theme;
        let local = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == cid)
            .and_then(|c| {
                c.environments
                    .iter()
                    .find(|e| e.name == self.ws.selected_environment)
            });
        let workspace = self
            .ws
            .environments
            .iter()
            .find(|e| e.name == self.ws.workspace_environment);
        let name = local
            .or(workspace)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        let response = ui.interact(
            r,
            ui.scope_id().with("environment-selector"),
            Sense::click(),
        );
        if response.hovered() {
            ui.painter().rect_filled(r, 6, t.hover);
        }
        if name.is_empty() {
            dashed_border(ui, r.shrink(0.5), t.strong_border);
        } else {
            ui.painter().rect_stroke(
                r,
                6,
                egui::Stroke::new(1., t.strong_border),
                egui::StrokeKind::Inside,
            );
        }
        w::elided_label(
            ui,
            rect(r.left() + 8., r.top(), r.width() - 26., r.height()),
            if name.is_empty() {
                "No Environment"
            } else {
                &name
            },
            12.,
            if name.is_empty() { t.muted } else { t.text },
        );
        w::icon(
            ui,
            pos2(r.right() - 11., r.center().y),
            Icon::Down,
            t.muted,
            11.,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, "Select environment")
        });
        egui::Popup::menu(&response)
            .gap(10.)
            .frame(menu::frame(t))
            .show(|ui| {
                let width = 320.;
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(0., 0.);
                if selector_row(ui, "No Environment", "EyeOff", name.is_empty(), false, t)
                    .1
                    .clicked()
                {
                    self.ws.selected_environment.clear();
                    self.ws.workspace_environment.clear();
                    self.runtime_vars.clear();
                    self.mark_workspace_changed();
                    ui.close();
                }
                egui::ScrollArea::vertical()
                    .max_height((ui.ctx().content_rect().height() - 240.).clamp(80., 360.))
                    .show(ui, |ui| {
                        ui.set_width(width);
                        for (scope, label) in [
                            (Scope::Workspace, "Workspace"),
                            (Scope::Collection(cid.into()), "Collection"),
                        ] {
                            let entries = self.environment_list(&scope).to_vec();
                            if !entries.is_empty() {
                                menu::separator(ui, width, t);
                                let row = ui.allocate_space(vec2(width, 24.)).1;
                                let heading = match &scope {
                                    Scope::Collection(cid) => self
                                        .ws
                                        .collections
                                        .iter()
                                        .find(|c| &c.id == cid)
                                        .map(|c| format!("Collection · {}", c.name))
                                        .unwrap_or_else(|| label.into()),
                                    _ => label.into(),
                                };
                                w::elided_label(
                                    ui,
                                    row.shrink2(vec2(10., 0.)),
                                    &heading,
                                    11.,
                                    t.muted,
                                );
                                ui.interact(row, ui.scope_id().with(label), Sense::hover())
                                    .on_hover_text(self.environment_scope_hint(&scope));
                            }
                            for env in entries {
                                let target = Target {
                                    scope: scope.clone(),
                                    name: env.name,
                                };
                                ui.push_id(target.id(), |ui| {
                                    let (row, select) = selector_row(
                                        ui,
                                        &target.name,
                                        "Server",
                                        self.environment_is_active(&target),
                                        true,
                                        t,
                                    );
                                    if select.clicked() {
                                        self.activate_environment(&target, true);
                                        ui.close();
                                    }
                                    if menu::icon_button(
                                        ui,
                                        rect(row.right() - 56., row.top(), 26., row.height()),
                                        "Edit",
                                        "Edit environment",
                                        false,
                                        t,
                                    )
                                    .clicked()
                                    {
                                        self.open_environment(target.clone(), false);
                                        ui.close();
                                    }
                                    if menu::icon_button(
                                        ui,
                                        rect(row.right() - 28., row.top(), 26., row.height()),
                                        "Trash",
                                        "Delete environment",
                                        true,
                                        t,
                                    )
                                    .clicked()
                                    {
                                        self.open_environment(target.clone(), false);
                                        ui.ctx().data_mut(|d| {
                                            let key = self.environment_editor_key(&target.id());
                                            let mut editor =
                                                d.get_temp::<Editor>(key).unwrap_or(Editor {
                                                    name: target.name.clone(),
                                                    ..Default::default()
                                                });
                                            editor.remove = true;
                                            d.insert_temp(key, editor);
                                        });
                                        ui.close();
                                    }
                                });
                            }
                        }
                    });
                menu::separator(ui, width, t);
                if selector_row(ui, "Manage environments", "Settings", false, false, t)
                    .1
                    .clicked()
                {
                    self.open_environment_manager(cid);
                    ui.close();
                }
                if selector_row(ui, "New environment", "FilePlus", false, false, t)
                    .1
                    .clicked()
                {
                    let scope = if cid.is_empty() {
                        Scope::Workspace
                    } else {
                        Scope::Collection(cid.into())
                    };
                    if let Err(e) = self.create_environment(scope, None) {
                        self.notice(e.to_string());
                    }
                    ui.close();
                }
            });
    }
    pub(crate) fn environment_scope_hint(&self, scope: &Scope) -> String {
        match scope {
            Scope::Workspace => "Shared by all collections in this workspace.".into(),
            Scope::Collection(cid) => {
                let name = self
                    .ws
                    .collections
                    .iter()
                    .find(|c| &c.id == cid)
                    .map(|c| c.name.as_str())
                    .unwrap_or("this collection");
                format!(
                    "Only applies to {name}. Overrides workspace environment variables with the same name."
                )
            }
            Scope::Globals => "Global variables in this workspace.".into(),
        }
    }
    pub fn environment_document(&mut self, ui: &mut Ui, r: Rect, target: Target) {
        let t = self.theme;
        let Some(rows) = self.environment_rows(&target).cloned() else {
            self.close(&target.id());
            return;
        };
        let body = rect(
            r.left() + 20.,
            r.top() + 18.,
            r.width() - 40.,
            r.height() - 34.,
        );
        let state_id = self.environment_editor_key(&target.id());
        let mut state = ui
            .ctx()
            .data_mut(|d| d.get_temp::<Editor>(state_id))
            .unwrap_or(Editor {
                name: target.name.clone(),
                ..Default::default()
            });
        let mut current = target.clone();
        if target.scope == Scope::Globals {
            w::medium(
                ui,
                pos2(body.left(), body.top() + 16.),
                "Globals",
                18.,
                t.text,
            );
        } else {
            let mut field = w::child(
                ui,
                rect(
                    body.left(),
                    body.top(),
                    (body.width() - 250.).max(120.),
                    34.,
                ),
                ("env-name", target.id()),
            );
            let name_id = state_id.with("name-input");
            let focused = ui.memory(|m| m.has_focus(name_id));
            let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
            let mut output = egui::TextEdit::singleline(&mut state.name)
                .id(name_id)
                .font(egui::FontId::proportional(18.))
                .desired_width(field.available_width())
                .show(&mut field);
            if state.focus {
                output.response.request_focus();
                output
                    .state
                    .cursor
                    .set_char_range(Some(egui::text::CCursorRange::two(
                        egui::text::CCursor::new(0),
                        egui::text::CCursor::new(state.name.chars().count()),
                    )));
                output.state.store(ui.ctx(), output.response.id);
                state.focus = false;
            } else if output.response.lost_focus()
                || focused && (enter || !output.response.has_focus())
            {
                match self.rename_environment(&target, &state.name) {
                    Ok(next) => {
                        current = next;
                        state.error.clear();
                    }
                    Err(e) => state.error = e.to_string(),
                }
            }
            let active = self.environment_is_active(&current);
            if w::button(
                ui,
                rect(body.right() - 231., body.top() + 2., 105., 28.),
                if active { "Active" } else { "Set active" },
                active,
                t,
            )
            .clicked()
            {
                self.activate_environment(&current, !active);
            }
        }
        if w::button(
            ui,
            rect(body.right() - 111., body.top() + 2., 72., 28.),
            "Save",
            false,
            t,
        )
        .clicked()
        {
            self.environment_changed(&current.scope);
            self.flush();
            self.notice("Environment saved");
        }
        let more = w::icon_button(
            ui,
            rect(body.right() - 30., body.top() + 2., 28., 28.),
            Icon::Dots,
            "Environment actions",
            t,
        );
        egui::Popup::menu(&more).show(|ui| {
            if ui.button("Export environment...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(format!("{}.req", safe_name(&current.name)))
                    .save_file()
                {
                    let env = Environment {
                        name: current.name.clone(),
                        vars: rows.clone(),
                    };
                    let source = if crate::yaml::is_yaml(&path) {
                        crate::yaml::environment_source(&env)
                    } else {
                        Ok(crate::environments::bru_environment_source(&env))
                    };
                    if let Err(e) = source.and_then(|s| storage::atomic_write(&path, s)) {
                        self.notice(e.to_string());
                    }
                }
                ui.close();
            }
            if current.scope != Scope::Globals {
                if ui.button("Duplicate").clicked() {
                    if let Err(e) = self.create_environment(
                        current.scope.clone(),
                        Some(Environment {
                            name: format!("{} Copy", current.name),
                            vars: rows.clone(),
                        }),
                    ) {
                        self.notice(e.to_string());
                    }
                    ui.close();
                }
                if ui.button("Delete...").clicked() {
                    state.remove = true;
                    ui.close();
                }
            }
        });
        w::label(
            ui,
            pos2(body.left(), body.top() + 55.),
            "Use {{variable}} in the URL, headers and request body. Changes are saved automatically.",
            12.,
            t.muted,
        );
        if !state.error.is_empty() {
            w::label(
                ui,
                pos2(body.left(), body.top() + 79.),
                &state.error,
                12.,
                t.red,
            );
        }
        let mut shown = rows;
        let mut table = w::child(
            ui,
            rect(
                body.left(),
                body.top() + 98.,
                body.width(),
                (body.height() - 130.).max(40.),
            ),
            ("env-rows", current.id()),
        );
        let changed = egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(&mut table, |ui| {
                crate::environment_rows::draw(ui, &mut shown, t)
            })
            .inner;
        if changed {
            if let Err(e) = self.set_environment_rows(&current, shown.clone()) {
                state.error = e.to_string();
            }
        }
        if shown.iter().any(|p| p.secret) {
            w::label(
                ui,
                pos2(body.left(), body.bottom() - 8.),
                if shown
                    .iter()
                    .any(|p| crate::environment_vault::is_reference(&p.value))
                {
                    "Some local secrets are unavailable. Enter their values here before sending requests."
                } else if current.collection().is_some() {
                    "Secrets are encrypted locally. Collection files store references for CLI; exports omit values."
                } else {
                    "Secrets are encrypted in the local profile; exports omit values."
                },
                11.,
                t.muted,
            );
        }
        if state.remove {
            let (remove, close) = w::modal(ui.ctx(), "Delete Environment", 420., t, |ui| {
                ui.label(format!("Delete \"{}\" and its variables?", current.name));
                ui.label("A local recovery copy will be kept.");
                ui.button("Delete").clicked()
            });
            if remove {
                match self.remove_environment(&current) {
                    Ok(()) => state.remove = false,
                    Err(e) => state.error = e.to_string(),
                }
            } else if close {
                state.remove = false;
            }
        }
        ui.ctx()
            .data_mut(|d| d.insert_temp(self.environment_editor_key(&current.id()), state));
    }
}

fn selector_row(
    ui: &mut Ui,
    label: &str,
    icon: &str,
    active: bool,
    actions: bool,
    t: crate::theme::Theme,
) -> (Rect, egui::Response) {
    let row = ui
        .allocate_space(vec2(ui.available_width(), menu::ROW_HEIGHT))
        .1;
    let select = Rect::from_min_max(
        row.min,
        pos2(row.right() - if actions { 58. } else { 0. }, row.bottom()),
    );
    let response = ui.interact(select, ui.scope_id().with(label), Sense::click());
    if response.hovered() || response.has_focus() {
        menu::highlight(ui, select, false, t);
    }
    menu::icon(
        ui,
        icon,
        pos2(row.left() + 18., row.center().y),
        menu::icon_color(t),
        menu::ICON_SIZE,
    );
    w::elided_label(
        ui,
        rect(
            row.left() + 34.,
            row.top(),
            select.width() - 62.,
            row.height(),
        ),
        label,
        menu::LABEL_SIZE,
        t.text,
    );
    if active {
        w::icon(
            ui,
            pos2(select.right() - 15., row.center().y),
            Icon::Check,
            t.accent,
            13.,
        );
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, ui.is_enabled(), active, label)
    });
    (
        row,
        response
            .on_hover_text(label)
            .on_hover_cursor(egui::CursorIcon::PointingHand),
    )
}

fn dashed_border(ui: &Ui, r: Rect, color: egui::Color32) {
    let radius = 6.;
    let mut points = Vec::new();
    for (center, start) in [
        (pos2(r.right() - radius, r.top() + radius), -90f32),
        (pos2(r.right() - radius, r.bottom() - radius), 0.),
        (pos2(r.left() + radius, r.bottom() - radius), 90.),
        (pos2(r.left() + radius, r.top() + radius), 180.),
    ] {
        for i in 0..=10 {
            let a = (start + i as f32 * 9.).to_radians();
            points.push(center + vec2(a.cos(), a.sin()) * radius);
        }
    }
    points.push(points[0]);
    ui.painter().extend(egui::Shape::dashed_line(
        &points,
        egui::Stroke::new(1., color),
        3.,
        3.,
    ));
}
