use crate::{
    app::App,
    environment_tabs::{Scope, Target},
    menu::{self, Item},
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};
use std::collections::HashSet;

#[derive(Default)]
pub struct State {
    collapsed: HashSet<String>,
    search_open: bool,
    selected_scope: Option<Scope>,
    keyboard_row: Option<usize>,
}

#[derive(Clone, Copy)]
enum Command {
    Create,
    Import,
    Expand,
    Collapse,
}

fn scope_key(scope: &Scope) -> String {
    Target {
        scope: scope.clone(),
        name: String::new(),
    }
    .id()
}

impl State {
    pub fn reveal(&mut self, scope: &Scope) {
        self.collapsed.remove(&scope_key(scope));
        self.selected_scope = Some(scope.clone());
    }
}

impl App {
    pub(crate) fn environment_sidebar(&mut self, ui: &mut Ui, bounds: Rect) {
        let t = self.theme;
        let mut panel = w::child(ui, bounds, "environment-sidebar");
        panel.painter().rect_filled(bounds, 0, t.sidebar);
        w::icon(
            &panel,
            pos2(bounds.left() + 20., bounds.top() + 15.),
            Icon::Key,
            t.muted,
            15.,
        );
        w::elided_medium(
            &panel,
            rect(
                bounds.left() + 38.,
                bounds.top(),
                bounds.width() - 120.,
                30.,
            ),
            "Environments",
            12.,
            t.text,
        );
        let search = w::icon_button(
            &mut panel,
            rect(bounds.right() - 79., bounds.top() + 2., 25., 26.),
            Icon::Search,
            "Find environment",
            t,
        );
        if search.clicked() {
            self.environment_tree.search_open = !self.environment_tree.search_open;
            if !self.environment_tree.search_open {
                self.env_search.clear();
            }
        }
        let create_tip = match self.environment_tree_scope() {
            Scope::Collection(cid) => format!("New environment in {}", self.collection_name(&cid)),
            _ => "New workspace environment".into(),
        };
        if w::icon_button(
            &mut panel,
            rect(bounds.right() - 52., bounds.top() + 2., 25., 26.),
            Icon::Plus,
            &create_tip,
            t,
        )
        .clicked()
        {
            self.environment_tree_command(Command::Create);
        }
        let more = w::icon_button(
            &mut panel,
            rect(bounds.right() - 26., bounds.top() + 2., 23., 26.),
            Icon::DotsVertical,
            "Environment options",
            t,
        );
        self.environment_tree_menu(&mut panel, &more);
        let mut top = bounds.top() + 37.;
        if self.environment_tree.search_open {
            let field = panel.put(
                rect(bounds.left() + 10., top, bounds.width() - 20., 28.),
                egui::TextEdit::singleline(&mut self.env_search)
                    .hint_text("Search environments...")
                    .font(egui::FontId::proportional(12.)),
            );
            if search.clicked() {
                field.request_focus();
            }
            top += 36.;
        }
        let query = self.env_search.trim().to_lowercase();
        let mut tree = w::child(
            &mut panel,
            rect(
                bounds.left(),
                top,
                bounds.width() - 1.,
                bounds.bottom() - top,
            ),
            "environment-tree",
        );
        tree.spacing_mut().item_spacing = vec2(0., 0.);
        egui::ScrollArea::vertical()
            .id_salt("environment-tree-scroll")
            .auto_shrink([false, false])
            .show(&mut tree, |ui| {
                let mut found = false;
                if "globals".contains(&query) {
                    self.environment_tree_item(
                        ui,
                        Target {
                            scope: Scope::Globals,
                            name: "Globals".into(),
                        },
                        false,
                    );
                    found = true;
                }
                found |= self.environment_tree_group(ui, Scope::Workspace, "Workspace", &query);
                let collections: Vec<_> = self
                    .ws
                    .collections
                    .iter()
                    .map(|c| (c.id.clone(), c.name.clone()))
                    .collect();
                for (id, name) in collections {
                    found |= self.environment_tree_group(ui, Scope::Collection(id), &name, &query);
                }
                if !found {
                    let row = ui.allocate_space(vec2(ui.available_width(), 30.)).1;
                    w::label(
                        ui,
                        pos2(row.left() + 12., row.center().y),
                        "No environments found",
                        12.,
                        t.muted,
                    );
                }
            });
    }

    fn environment_tree_scope(&self) -> Scope {
        let scope = self
            .environment_tree
            .selected_scope
            .clone()
            .or_else(|| {
                self.ws
                    .active
                    .as_deref()
                    .and_then(Target::parse)
                    .map(|target| target.scope)
            })
            .unwrap_or_else(|| {
                let cid = self.active_collection_id();
                if cid.is_empty() {
                    Scope::Workspace
                } else {
                    Scope::Collection(cid)
                }
            });
        match scope {
            Scope::Collection(ref cid) if self.ws.collections.iter().any(|c| &c.id == cid) => scope,
            _ => Scope::Workspace,
        }
    }

    fn environment_tree_menu(&mut self, ui: &mut Ui, response: &egui::Response) {
        let popup_id = egui::Popup::default_response_id(response);
        if !response.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            return;
        }
        if response.clicked() {
            self.environment_tree.keyboard_row = None;
        }
        let entries = [
            Item::new("New Environment", "Plus", Some(Command::Create)),
            Item::new("Import Environment...", "Import", Some(Command::Import)),
            Item::separator(),
            Item::new("Expand All", "Expand", Some(Command::Expand)),
            Item::new("Collapse All", "Collapse", Some(Command::Collapse)),
        ];
        let width = menu::width(ui, &entries);
        let popup = egui::Popup::menu(response)
            .gap(10.)
            .frame(menu::frame(self.theme))
            .show(|ui| {
                menu::rows(
                    ui,
                    &entries,
                    width,
                    self.theme,
                    &mut self.environment_tree.keyboard_row,
                )
            });
        if let Some(command) = popup.and_then(|popup| popup.inner) {
            self.environment_tree_command(command);
        }
    }

    fn environment_tree_command(&mut self, command: Command) {
        match command {
            Command::Create => {
                if let Err(error) = self.create_environment(self.environment_tree_scope(), None) {
                    self.notice(error.to_string());
                }
            }
            Command::Import => self.import_environment_into(self.environment_tree_scope()),
            Command::Expand => self.environment_tree.collapsed.clear(),
            Command::Collapse => {
                self.env_search.clear();
                self.environment_tree
                    .collapsed
                    .insert(scope_key(&Scope::Workspace));
                self.environment_tree.collapsed.extend(
                    self.ws
                        .collections
                        .iter()
                        .map(|c| scope_key(&Scope::Collection(c.id.clone()))),
                );
            }
        }
    }

    fn import_environment_into(&mut self, scope: Scope) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Import environment")
            .add_filter("Environment", &["req", "bru", "json", "env", "yml", "yaml"])
            .pick_file()
        {
            if let Err(error) = crate::environments::import_environment(&path)
                .and_then(|env| self.create_environment(scope, Some(env)))
            {
                self.notice(format!("Import failed: {error:#}"));
            }
        }
    }

    fn environment_tree_group(
        &mut self,
        ui: &mut Ui,
        scope: Scope,
        name: &str,
        query: &str,
    ) -> bool {
        let names: Vec<_> = self
            .environment_list(&scope)
            .iter()
            .filter(|env| {
                name.to_lowercase().contains(query) || env.name.to_lowercase().contains(query)
            })
            .map(|env| env.name.clone())
            .collect();
        if !query.is_empty() && names.is_empty() && !name.to_lowercase().contains(query) {
            return false;
        }
        let t = self.theme;
        let key = scope_key(&scope);
        ui.push_id(&key, |ui| {
            let row = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
            let expanded = !query.is_empty() || !self.environment_tree.collapsed.contains(&key);
            let response = ui.interact(
                rect(row.left(), row.top(), row.width() - 54., row.height()),
                ui.scope_id().with("expand"),
                Sense::click(),
            );
            response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
            if response.hovered() {
                ui.painter().rect_filled(row, 0, t.hover);
            }
            w::icon(
                ui,
                pos2(row.left() + 16., row.center().y),
                if expanded { Icon::Down } else { Icon::Chevron },
                t.muted,
                13.,
            );
            w::elided_label(
                ui,
                rect(row.left() + 28., row.top(), row.width() - 82., row.height()),
                name,
                13.,
                t.text,
            );
            if response.clicked() {
                self.environment_tree.selected_scope = Some(scope.clone());
                if expanded {
                    self.environment_tree.collapsed.insert(key.clone());
                } else {
                    self.environment_tree.collapsed.remove(&key);
                }
            }
            response.on_hover_text(self.environment_scope_hint(&scope));
            if w::icon_button(
                ui,
                rect(row.right() - 52., row.top(), 25., 25.),
                Icon::Import,
                "Import environment",
                t,
            )
            .clicked()
            {
                self.import_environment_into(scope.clone());
            }
            if w::icon_button(
                ui,
                rect(row.right() - 26., row.top(), 25., 25.),
                Icon::Plus,
                "New environment",
                t,
            )
            .clicked()
            {
                if let Err(error) = self.create_environment(scope.clone(), None) {
                    self.notice(error.to_string());
                }
            }
            if expanded {
                if names.is_empty() {
                    let row = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
                    w::label(
                        ui,
                        pos2(row.left() + 32., row.center().y),
                        "No environments",
                        12.,
                        t.muted,
                    );
                }
                for name in names {
                    self.environment_tree_item(
                        ui,
                        Target {
                            scope: scope.clone(),
                            name,
                        },
                        true,
                    );
                }
            }
        });
        true
    }

    fn environment_tree_item(&mut self, ui: &mut Ui, target: Target, nested: bool) {
        let t = self.theme;
        let row = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
        let response = ui.interact(row, ui.scope_id().with(target.id()), Sense::click());
        let selected = self.ws.active.as_deref() == Some(target.id().as_str());
        response.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::Button, true, selected, &target.name)
        });
        if selected || response.hovered() {
            ui.painter()
                .rect_filled(row, 0, if selected { t.surface } else { t.hover });
        }
        let x = row.left() + if nested { 38. } else { 18. };
        if nested {
            w::line(
                ui,
                pos2(row.left() + 17., row.top()),
                pos2(row.left() + 17., row.bottom()),
                t.surface,
            );
        }
        w::icon(
            ui,
            pos2(x, row.center().y),
            if self.environment_is_active(&target) {
                Icon::Check
            } else {
                Icon::Globe
            },
            t.muted,
            14.,
        );
        w::elided_label(
            ui,
            rect(x + 14., row.top(), row.right() - x - 20., row.height()),
            &target.name,
            13.,
            t.text,
        );
        if response.clicked() {
            self.open_environment(target.clone(), false);
        }
        response.on_hover_text(format!(
            "{}\n{}",
            target.name,
            self.environment_scope_hint(&target.scope)
        ));
    }
}
