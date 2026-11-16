use crate::{
    app::{Action, App, Dialog},
    model::*,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{
    self, Color32, FontId, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2,
};

impl App {
    pub fn draw(&mut self, ui: &mut Ui) {
        ui.ctx().data_mut(|d| {
            d.insert_temp(
                egui::Id::unique("editor-font-size"),
                self.ws.preferences.editor_font_size,
            )
        });
        let screen = ui.max_rect();
        let t = self.theme;
        let devtools_height = if self.devtools_open {
            self.devtools_height.min((screen.height() - 180.).max(120.))
        } else {
            0.
        };
        ui.painter().rect_filled(screen, 0, t.bg);
        self.titlebar(ui, rect(screen.left(), screen.top(), screen.width(), 36.));
        self.statusbar(
            ui,
            rect(screen.left(), screen.bottom() - 24., screen.width(), 24.),
        );
        let sidebar_width = self.ws.sidebar_width.min(screen.width() * 0.4);
        let sidebar =
            sidebar_width * crate::panes::sidebar_openness(ui.ctx(), self.sidebar_visible);
        if sidebar > 0. {
            // Bruno animates the outer width while keeping its contents at their full width.
            let old_clip = ui.clip_rect();
            ui.set_clip_rect(old_clip.intersect(rect(
                screen.left(),
                screen.top() + 36.,
                sidebar,
                screen.height() - 60. - devtools_height,
            )));
            self.sidebar(
                ui,
                rect(
                    screen.left(),
                    screen.top() + 36.,
                    sidebar_width,
                    screen.height() - 60. - devtools_height,
                ),
            );
            ui.set_clip_rect(old_clip);
            w::line(
                ui,
                pos2(screen.left() + sidebar, screen.top() + 36.),
                pos2(
                    screen.left() + sidebar,
                    screen.bottom() - 24. - devtools_height,
                ),
                t.border,
            );
        }
        if self.sidebar_visible && sidebar >= sidebar_width - 0.1 {
            let drag = ui.interact(
                rect(
                    screen.left() + sidebar - 3.,
                    screen.top() + 36.,
                    6.,
                    screen.height() - 60. - devtools_height,
                ),
                ui.scope_id().with("sidebar_resize"),
                Sense::drag(),
            );
            if drag.hovered() || drag.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if drag.dragged() {
                self.ws.sidebar_width =
                    (self.ws.sidebar_width + drag.drag_delta().x).clamp(190., 450.);
                self.mark_changed();
            }
        }
        let main = rect(
            screen.left() + sidebar,
            screen.top() + 36.,
            screen.width() - sidebar,
            screen.height() - 60. - devtools_height,
        );
        if self.ws.active.is_some() {
            self.request_workspace(ui, main);
        } else {
            self.home(ui, main);
        }
        if self.devtools_open {
            let resize = ui.interact(
                rect(
                    screen.left(),
                    screen.bottom() - 26. - devtools_height,
                    screen.width(),
                    5.,
                ),
                ui.scope_id().with("devtools-resize"),
                Sense::drag(),
            );
            if resize.hovered() || resize.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            if resize.dragged() {
                self.devtools_height = (self.devtools_height - resize.drag_delta().y)
                    .clamp(120., (screen.height() - 180.).max(120.));
            }
            self.devtools(
                ui,
                rect(
                    screen.left(),
                    screen.bottom() - 24. - devtools_height,
                    screen.width(),
                    devtools_height,
                ),
            );
        }
    }
    fn titlebar(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        ui.painter().rect_filled(r, 0, t.sidebar);
        let drag = ui.interact(
            rect(
                r.left() + 235.,
                r.top(),
                (r.width() - 550.).max(0.),
                r.height(),
            ),
            ui.scope_id().with("title-drag"),
            Sense::click_and_drag(),
        );
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        let menu = w::icon_button(
            ui,
            rect(r.left() + 6., r.top() + 3., 28., 30.),
            Icon::Menu,
            "Menu",
            t,
        );
        egui::Popup::menu(&menu).show(|ui| {
            if ui.button("New request                 Ctrl+N").clicked() {
                self.actions.push(Action::NewRequest(
                    self.active_collection_id(),
                    String::new(),
                ));
                ui.close();
            }
            if ui.button("Create collection").clicked() {
                self.actions.push(Action::NewCollection);
                ui.close();
            }
            if ui.button("Open collection              Ctrl+O").clicked() {
                self.actions.push(Action::OpenCollection);
                ui.close();
            }
            ui.separator();
            if ui.button("Import collection").clicked() {
                self.actions.push(Action::ImportPostman);
                ui.close();
            }
            if ui.button("Import cURL").clicked() {
                self.actions.push(Action::Dialog(Dialog::ImportCurl));
                ui.close();
            }
            if ui.button("Export collection").clicked() {
                self.actions.push(Action::ExportPostman(None));
                ui.close();
            }
            ui.separator();
            if ui.button("Preferences").clicked() {
                self.actions.push(Action::Dialog(Dialog::Settings));
                ui.close();
            }
            if ui.button("About Reqon").clicked() {
                self.actions.push(Action::Dialog(Dialog::About));
                ui.close();
            }
        });
        if w::icon_button(
            ui,
            rect(r.left() + 35., r.top() + 3., 27., 30.),
            Icon::Home,
            "Home",
            t,
        )
        .clicked()
        {
            self.actions.push(Action::Home);
        }
        let workspace = ui.interact(
            rect(r.left() + 65., r.top() + 3., 174., 30.),
            ui.scope_id().with("workspace-menu"),
            Sense::click(),
        );
        if workspace.hovered() {
            ui.painter().rect_filled(workspace.rect, 5, t.hover);
        }
        w::elided_medium(
            ui,
            rect(r.left() + 72., r.top(), 104., r.height()),
            &self.ws.name,
            13.,
            t.text,
        );
        w::icon(
            ui,
            pos2(r.left() + 181., r.center().y),
            Icon::Down,
            t.muted,
            12.,
        );
        self.workspace_menu(ui, &workspace);
        w::icon(
            ui,
            pos2(r.center().x - 30., r.center().y),
            Icon::Reqon,
            Color32::WHITE,
            24.,
        );
        w::label(
            ui,
            pos2(r.center().x - 13., r.center().y),
            "Reqon",
            13.,
            t.text,
        );
        let right = r.right();
        if w::icon_button(
            ui,
            rect(right - 230., r.top() + 4., 28., 28.),
            Icon::Panel,
            "Toggle sidebar",
            t,
        )
        .clicked()
        {
            self.sidebar_visible = !self.sidebar_visible;
        }
        if w::icon_button(
            ui,
            rect(right - 202., r.top() + 4., 28., 28.),
            Icon::BottomPanel,
            "Toggle Dev Tools",
            t,
        )
        .clicked()
        {
            self.devtools_open = !self.devtools_open;
        }
        if w::response_layout_button(
            ui,
            rect(right - 174., r.top() + 4., 28., 28.),
            self.ws.side_by_side,
            t,
            "titlebar-response-layout",
        )
        .clicked()
        {
            self.actions.push(Action::ToggleSplit);
        }
        if w::icon_button(
            ui,
            rect(right - 138., r.top(), 46., 36.),
            Icon::Minimize,
            "Minimize",
            t,
        )
        .clicked()
        {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if w::icon_button(
            ui,
            rect(right - 92., r.top(), 46., 36.),
            Icon::Maximize,
            "Maximize",
            t,
        )
        .clicked()
        {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        if w::icon_button(
            ui,
            rect(right - 46., r.top(), 46., 36.),
            Icon::Close,
            "Close window",
            t,
        )
        .clicked()
        {
            self.flush();
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        w::line(ui, r.left_bottom(), r.right_bottom(), t.hover);
    }
    fn statusbar(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        ui.painter().rect_filled(r, 0, t.sidebar);
        w::line(ui, r.left_top(), r.right_top(), t.border);
        for (i, (icon, name)) in [
            (Icon::Settings, "Preferences"),
            (Icon::Key, "Environments"),
            (Icon::Palette, "Theme"),
            (Icon::Bell, "Notifications"),
            (Icon::Github, "Git"),
        ]
        .into_iter()
        .enumerate()
        {
            if name == "Git" && !self.ws.preferences.features.git {
                continue;
            }
            let response = w::icon_button(
                ui,
                rect(r.left() + 17. + i as f32 * 26., r.top() + 1., 22., 22.),
                icon,
                name,
                t,
            );
            match i {
                0 if response.clicked() => self.actions.push(Action::Dialog(Dialog::Settings)),
                1 if response.clicked() => self.actions.push(Action::Dialog(Dialog::Environments)),
                2 => {
                    egui::Popup::menu(&response).show(|ui| {
                        for (title, dark) in [("Dark", true), ("Light", false)] {
                            if ui.selectable_label(t.dark == dark, title).clicked() {
                                if t.dark != dark {
                                    self.actions.push(Action::ToggleTheme);
                                }
                                ui.close();
                            }
                        }
                    });
                }
                3 => {
                    egui::Popup::menu(&response).show(|ui| {
                        ui.set_width(280.);
                        ui.strong("Notifications");
                        ui.separator();
                        if self.status.is_empty() {
                            ui.label("No new notifications");
                        } else {
                            ui.add(egui::Label::new(&self.status).selectable(true).wrap());
                        }
                    });
                }
                4 if response.clicked() => {
                    let cid = self.active_collection_id();
                    self.open_git(&cid);
                }
                _ => {}
            }
        }
        if !self.status.is_empty() && self.status_at.elapsed().as_secs() < 15 {
            let rr = rect(
                r.left() + 161.,
                r.top(),
                (r.width() - 540.).max(0.),
                r.height(),
            );
            let child = w::child(ui, rr, "status-message");
            w::label(
                &child,
                pos2(rr.left(), rr.center().y),
                &self.status,
                11.,
                t.muted,
            );
        } else {
            w::label(
                ui,
                pos2(r.left() + 161., r.center().y),
                "Local workspace",
                11.,
                t.accent,
            );
        }
        let search = rect(r.right() - 345., r.top(), 78., 24.);
        w::icon(
            ui,
            pos2(search.left() + 9., search.center().y),
            Icon::Search,
            t.muted,
            14.,
        );
        w::label(
            ui,
            pos2(search.left() + 23., search.center().y),
            "Search",
            12.,
            t.muted,
        );
        if ui
            .interact(search, ui.scope_id().with("global_search"), Sense::click())
            .clicked()
        {
            self.search_open = !self.search_open;
        }
        let cookies = rect(r.right() - 262., r.top(), 85., 24.);
        w::icon(
            ui,
            pos2(cookies.left() + 10., cookies.center().y),
            Icon::Cookie,
            t.muted,
            14.,
        );
        w::label(
            ui,
            pos2(cookies.left() + 24., cookies.center().y),
            "Cookies",
            12.,
            t.muted,
        );
        if ui
            .interact(cookies, ui.scope_id().with("cookies"), Sense::click())
            .clicked()
        {
            self.actions.push(Action::Dialog(Dialog::Cookies));
        }
        let console = rect(r.right() - 170., r.top(), 96., 24.);
        w::icon(
            ui,
            pos2(console.left() + 9., console.center().y),
            Icon::Tool,
            t.muted,
            14.,
        );
        w::label(
            ui,
            pos2(console.left() + 24., console.center().y),
            "Dev Tools",
            12.,
            t.muted,
        );
        if ui
            .interact(console, ui.scope_id().with("console"), Sense::click())
            .clicked()
        {
            self.devtools_open = !self.devtools_open;
        }
        w::label(
            ui,
            pos2(r.right() - 64., r.center().y),
            concat!("v", env!("CARGO_PKG_VERSION")),
            12.,
            t.muted,
        );
    }
    fn sidebar(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        ui.painter().rect_filled(r, 0, t.sidebar);
        w::line(ui, r.right_top(), r.right_bottom(), t.border);
        w::icon(
            ui,
            pos2(r.left() + 20., r.top() + 15.),
            Icon::Box,
            t.muted,
            15.,
        );
        w::semibold(
            ui,
            pos2(r.left() + 38., r.top() + 15.),
            "Collections",
            12.,
            t.text,
        );
        if w::icon_button(
            ui,
            rect(r.right() - 79., r.top() + 2., 25., 26.),
            Icon::Search,
            "Find request",
            t,
        )
        .clicked()
        {
            self.search_open = !self.search_open;
        }
        let add = w::icon_button(
            ui,
            rect(r.right() - 52., r.top() + 2., 25., 26.),
            Icon::Plus,
            "Add collection",
            t,
        );
        if add.clicked() {
            self.actions.push(Action::NewCollection);
        }
        let more = w::icon_button(
            ui,
            rect(r.right() - 26., r.top() + 2., 23., 26.),
            Icon::DotsVertical,
            "Collection options",
            t,
        );
        self.collections_menu(ui, &more);
        let mut top = r.top() + 37.;
        if self.search_open {
            let search = ui.put(
                rect(r.left() + 10., top, r.width() - 20., 28.),
                TextEdit::singleline(&mut self.search)
                    .hint_text("Search requests...")
                    .font(FontId::proportional(12.)),
            );
            if !search.has_focus() && self.search.is_empty() {
                search.request_focus();
            }
            top += 36.;
        }
        let mut child = w::child(
            ui,
            rect(
                r.left(),
                top,
                r.width() - 1.,
                r.bottom()
                    - top
                    - 29.
                        * (u8::from(self.ws.preferences.features.api_spec)
                            + u8::from(self.ws.preferences.features.mock_server))
                            as f32,
            ),
            "collection-tree",
        );
        let refresh = self.tree.stale;
        let rows = self.tree.rows(&self.ws, &self.search, &self.collapsed);
        let rows = self.file_explorer.rows(
            ui.ctx(),
            &self.ws,
            rows,
            &self.search,
            &self.collapsed,
            refresh,
        );
        child.spacing_mut().item_spacing = vec2(0., 0.);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show_rows(&mut child, 25.6, rows.len(), |ui, range| {
                crate::tree_drag::begin(ui);
                crate::tree_drag::autoscroll(ui);
                let mut drop_zones = vec![];
                for index in range {
                    let row = &rows[index];
                    if row.kind == crate::tree::Kind::File {
                        self.file_tree_row(ui, row);
                        continue;
                    }
                    if row.kind == crate::tree::Kind::Request {
                        let (response, bounds) = self.tree_request(ui, row, row.depth, t);
                        drop_zones.push((index, response, bounds));
                        continue;
                    }
                    let rr = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
                    let response = ui.interact(rr, ui.scope_id().with(&row.key), Sense::click());
                    if response.container_contains_pointer() {
                        ui.painter().rect_filled(rr, 0, t.hover);
                    }
                    let is_collection = row.kind == crate::tree::Kind::Collection;
                    let x = rr.left()
                        + if is_collection {
                            16.
                        } else {
                            35. + row.depth as f32 * 18.
                        };
                    w::icon(
                        ui,
                        pos2(x, rr.center().y),
                        if row.expanded {
                            Icon::Down
                        } else {
                            Icon::Chevron
                        },
                        t.muted,
                        13.,
                    );
                    if !is_collection {
                        crate::menu::icon(ui, "Folder", pos2(x + 20., rr.center().y), t.muted, 15.);
                    }
                    let label_x = x + if is_collection { 12. } else { 33. };
                    let label = w::child(
                        ui,
                        rect(
                            label_x,
                            rr.top(),
                            (rr.right() - label_x - 32.).max(0.),
                            rr.height(),
                        ),
                        (&row.key, "name"),
                    );
                    w::label(&label, pos2(label_x, rr.center().y), &row.name, 13., t.text);
                    if response.clicked()
                        && !ui.input(|i| {
                            i.pointer
                                .interact_pos()
                                .is_some_and(|p| p.x >= rr.right() - 31.)
                        })
                    {
                        if row.expanded {
                            self.tree_menu.selected = Some(crate::tree_ops::Subject::Folder {
                                collection: row.collection.clone(),
                                folder: row.folder.clone(),
                            });
                            self.collapsed.insert(row.key.clone());
                        } else {
                            self.tree_menu.selected = Some(crate::tree_ops::Subject::Folder {
                                collection: row.collection.clone(),
                                folder: row.folder.clone(),
                            });
                            self.collapsed.remove(&row.key);
                        }
                    }
                    drop_zones.push((index, response.clone(), rr));
                    if !is_collection
                        && response.double_clicked()
                        && !ui.input(|i| {
                            i.pointer
                                .interact_pos()
                                .is_some_and(|p| p.x >= rr.right() - 31.)
                        })
                    {
                        self.open_folder_settings(&row.collection, &row.folder);
                    }
                    if is_collection {
                        self.collection_context_menu(ui, &response, rr, row);
                    } else {
                        self.tree_context_menu(ui, &response, rr, row);
                    }
                }
                crate::tree_drag::prepare_drop(ui);
                for (index, response, bounds) in drop_zones {
                    self.tree_drop_zone(ui, &response, &rows[index], bounds);
                }
                crate::tree_drag::finish(ui);
            });
        let sections = [
            (
                "API Specs",
                Icon::File,
                Dialog::ApiSpecs,
                self.ws.preferences.features.api_spec,
            ),
            (
                "Mock Servers",
                Icon::Panel,
                Dialog::MockServers,
                self.ws.preferences.features.mock_server,
            ),
        ]
        .into_iter()
        .filter(|(_, _, _, enabled)| *enabled)
        .collect::<Vec<_>>();
        for (i, (name, icon, dialog, _)) in sections.iter().cloned().enumerate() {
            let row = rect(
                r.left(),
                r.bottom() - sections.len() as f32 * 29. + i as f32 * 29.,
                r.width(),
                29.,
            );
            w::line(ui, row.left_top(), row.right_top(), t.hover);
            w::icon(
                ui,
                pos2(row.left() + 20., row.center().y),
                icon,
                t.muted,
                15.,
            );
            w::semibold(
                ui,
                pos2(row.left() + 38., row.center().y),
                name,
                12.,
                t.text,
            );
            w::icon(
                ui,
                pos2(row.right() - 16., row.center().y),
                Icon::Plus,
                t.muted,
                14.,
            );
            if ui
                .interact(row, ui.scope_id().with(name), Sense::click())
                .clicked()
            {
                self.actions.push(Action::Dialog(dialog));
            }
        }
    }
    fn tree_request(
        &mut self,
        ui: &mut Ui,
        r: &crate::tree::Row,
        depth: usize,
        t: crate::theme::Theme,
    ) -> (egui::Response, Rect) {
        let row = ui.allocate_space(vec2(ui.available_width(), 25.6)).1;
        let response = ui.interact(row, ui.scope_id().with(&r.key), Sense::click_and_drag());
        if !self.tree_moving {
            crate::tree_drag::source(
                &response,
                crate::tree_drag::RequestDrag {
                    id: r.key.clone(),
                    name: r.name.clone(),
                },
            );
        }
        let active = self.ws.active.as_deref() == Some(&r.key);
        if active || response.container_contains_pointer() {
            ui.painter()
                .rect_filled(row, 0, if active { t.surface } else { t.hover });
        }
        let x = row.left() + 30. + depth as f32 * 18.;
        w::line(
            ui,
            pos2(x - 13., row.top()),
            pos2(x - 13., row.bottom()),
            t.surface,
        );
        let badge = if r.method.chars().count() > 4 {
            r.method.chars().take(3).collect::<String>()
        } else {
            r.method.clone()
        };
        w::label(
            ui,
            pos2(x, row.center().y),
            &badge,
            11.,
            t.method(&r.method),
        );
        let rr = rect(
            x + 35.,
            row.top(),
            (row.right() - x - 40.).max(0.),
            row.height(),
        );
        let child = w::child(ui, rr, (&r.key, "text"));
        w::label(&child, pos2(rr.left(), rr.center().y), &r.name, 13., t.text);
        if response.clicked()
            && !ui.input(|i| {
                i.pointer
                    .interact_pos()
                    .is_some_and(|p| p.x >= row.right() - 31.)
            })
        {
            self.actions.push(Action::Select(r.key.clone()));
            self.tree_menu.selected = Some(crate::tree_ops::Subject::Request(r.key.clone()));
        }
        response.clone().on_hover_text(&r.url);
        self.tree_context_menu(ui, &response, row, r);
        (response, row)
    }
    fn collection_toolbar(&mut self, ui: &mut Ui, r: Rect, cid: &str, request_id: Option<&str>) {
        let t = self.theme;
        w::icon(
            ui,
            pos2(r.left() + 29., r.top() + 23.),
            Icon::Box,
            t.muted,
            18.,
        );
        let col_rect = rect(r.left() + 54., r.top() + 8., 150., 30.);
        let c_ui = w::child(ui, col_rect, "collection-title");
        w::elided_medium(
            &c_ui,
            rect(
                col_rect.left(),
                col_rect.top() - 1.,
                124.,
                col_rect.height(),
            ),
            &self.collection_name(&cid),
            13.,
            t.text,
        );
        w::icon(
            ui,
            pos2(col_rect.right() - 11., col_rect.center().y),
            Icon::Down,
            t.muted,
            12.,
        );
        let col_menu = ui.interact(
            col_rect,
            ui.scope_id().with("collection_title"),
            Sense::click(),
        );
        egui::Popup::menu(&col_menu).show(|ui| {
            if ui.button("New request").clicked() {
                self.actions
                    .push(Action::NewRequest(cid.to_owned(), String::new()));
                ui.close();
            }
            if ui.button("Run collection").clicked() {
                self.actions.push(Action::RunCollection);
                ui.close();
            }
            if ui.button("Export collection").clicked() {
                self.actions.push(Action::ExportPostman(None));
                ui.close();
            }
        });
        let env_width = 122.;
        let env_rect = rect(r.right() - env_width - 16., r.top() + 11., env_width, 24.);
        self.environment_selector(ui, env_rect, &cid);
        let more = w::icon_button(
            ui,
            rect(env_rect.left() - 36., r.top() + 9., 28., 28.),
            Icon::Dots,
            "More collection actions",
            t,
        );
        egui::Popup::menu(&more).show(|ui| {
            for (name, dialog) in [
                ("History", Dialog::History),
                ("Variables", Dialog::Environments),
                ("OpenAPI", Dialog::ApiSpecs),
                ("Mock Server", Dialog::MockServers),
            ] {
                if ui.button(name).clicked() {
                    self.actions.push(Action::Dialog(dialog));
                    ui.close();
                }
            }
            ui.separator();
            if ui.button("Collection Settings").clicked() {
                self.open_collection_settings(&cid);
                ui.close();
            }
        });
        if w::icon_button(
            ui,
            rect(env_rect.left() - 69., r.top() + 9., 28., 28.),
            Icon::Shield,
            "Request settings",
            t,
        )
        .clicked()
        {
            if let Some(id) = request_id {
                self.tabs.entry(id.into()).or_default().request_tab = 10;
            }
        }
        if w::icon_button(
            ui,
            rect(env_rect.left() - 98., r.top() + 9., 28., 28.),
            Icon::Run,
            "Run collection",
            t,
        )
        .clicked()
        {
            self.actions.push(Action::RunCollection);
        }
        if r.width() > 780. && self.ws.preferences.features.git {
            let has_git = self
                .ws
                .collections
                .iter()
                .find(|c| c.id == cid)
                .is_some_and(|c| c.path.ancestors().any(|p| p.join(".git").exists()));
            let git_rect = rect(env_rect.left() - 203., r.top() + 11., 102., 23.);
            let label = if has_git {
                self.git
                    .get(cid)
                    .map(|s| s.branch.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Git")
            } else {
                "Initialize Git"
            };
            if w::toolbar_chip(ui, git_rect, label, Icon::GitBranch, t).clicked() {
                self.open_git(cid);
            }
            let migrated = self
                .ws
                .collections
                .iter()
                .find(|c| c.id == cid)
                .is_some_and(|c| c.path.join("opencollection.yml").is_file());
            let dismiss = ui.scope_id().with(("migration-dismissed", cid));
            if !migrated
                && !ui
                    .ctx()
                    .data_mut(|d| d.get_temp::<bool>(dismiss))
                    .unwrap_or(false)
            {
                let migration = rect(env_rect.left() - 353., r.top() + 11., 145., 23.);
                let chip = ui.interact(
                    migration,
                    ui.scope_id().with("migrate-to-yml"),
                    Sense::click(),
                );
                ui.painter().rect(
                    migration,
                    12,
                    if chip.hovered() { t.hover } else { t.bg },
                    Stroke::new(1., t.strong_border),
                    StrokeKind::Inside,
                );
                w::icon(
                    ui,
                    pos2(migration.left() + 15., migration.center().y),
                    Icon::GitBranch,
                    t.muted,
                    14.,
                );
                w::medium(
                    ui,
                    pos2(migration.left() + 29., migration.center().y),
                    "Migrate to YML",
                    12.,
                    t.text,
                );
                if chip.clicked()
                    && !ui.input(|i| {
                        i.pointer
                            .interact_pos()
                            .is_some_and(|p| p.x >= migration.right() - 22.)
                    })
                {
                    self.migrate_collection(cid, ui.ctx());
                }
                if w::icon_button(
                    ui,
                    rect(migration.right() - 22., migration.top(), 22., 23.),
                    Icon::Close,
                    "Dismiss migration notice",
                    t,
                )
                .clicked()
                {
                    ui.ctx().data_mut(|d| d.insert_temp(dismiss, true));
                }
            }
        }
    }
    fn request_workspace(&mut self, ui: &mut Ui, r: Rect) {
        if self.workspaces.manage {
            self.workspace_manager(ui, r);
            return;
        }
        let t = self.theme;
        self.request_tabs(ui, rect(r.left(), r.top() + 44., r.width(), 37.));
        if let Some(target) = self
            .ws
            .active
            .as_deref()
            .and_then(crate::environment_tabs::Target::parse)
        {
            let cid = self.active_collection_id();
            self.environment_toolbar(ui, r, &cid);
            self.environment_document(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
                target,
            );
            return;
        }

        if let Some(id) = self
            .ws
            .active
            .clone()
            .filter(|id| id.starts_with("__file__"))
        {
            let cid = crate::file_explorer::parse_id(&id)
                .map(|(c, _)| c.to_owned())
                .unwrap_or_default();
            self.collection_toolbar(ui, r, &cid, None);
            self.file_workspace(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
                &id,
            );
            return;
        }
        if let Some(cid) = self
            .ws
            .active
            .as_deref()
            .and_then(|id| id.strip_prefix("__git__"))
            .map(str::to_owned)
        {
            self.collection_toolbar(ui, r, &cid, None);
            self.git_workspace(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
                &cid,
            );
            return;
        }
        if self.ws.active.as_deref() == Some(crate::preferences::TAB_ID) {
            let cid = self.active_collection_id();
            self.collection_toolbar(ui, r, &cid, None);
            self.preferences_workspace(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
            );
            return;
        }
        if let Some(id) = self
            .ws
            .active
            .as_ref()
            .filter(|id| id.starts_with("__folder__"))
            .cloned()
        {
            let cid = self.active_collection_id();
            self.collection_toolbar(ui, r, &cid, None);
            self.folder_workspace(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
                &id,
            );
            return;
        }
        if let Some(cid) = self
            .ws
            .active
            .as_deref()
            .and_then(|id| id.strip_prefix("__collection__"))
            .map(str::to_owned)
        {
            self.collection_toolbar(ui, r, &cid, None);
            self.collection_workspace(
                ui,
                rect(r.left(), r.top() + 81., r.width(), r.height() - 81.),
                &cid,
            );
            return;
        }
        if let Some(cid) = self
            .ws
            .active
            .as_deref()
            .and_then(|id| id.strip_prefix("__environments__"))
            .map(str::to_owned)
        {
            self.collection_toolbar(ui, r, &cid, None);
            self.close(&format!("__environments__{cid}"));
            self.open_environment_manager(&cid);
            return;
        }
        let Some(index) = self
            .ws
            .active
            .as_ref()
            .and_then(|id| self.ws.requests.iter().position(|q| &q.id == id))
        else {
            return;
        };
        // Temporarily own only the active request; large bodies stay in their allocation.
        let mut request = self.ws.requests.swap_remove(index);
        let tree_fields = (
            request.method.clone(),
            request.url.clone(),
            request.name.clone(),
        );
        let cid = request.collection.clone();
        self.collection_toolbar(ui, r, &cid, Some(&request.id));
        let mut changed = self.request_header(
            ui,
            rect(r.left() + 16., r.top() + 81., r.width() - 32., 40.),
            &mut request,
        );
        if request.protocol == "grpc" {
            changed |= self.grpc_request_bar(
                ui,
                rect(r.left() + 16., r.top() + 134., r.width() - 32., 34.),
                &mut request,
            );
        } else {
            let url = rect(
                r.left() + 16.,
                r.top() + 134.,
                r.width()
                    - if request.protocol == "grpc" {
                        32.
                    } else if request.protocol == "ws" {
                        130.
                    } else {
                        106.
                    },
                34.,
            );
            ui.painter()
                .rect_stroke(url, 6, Stroke::new(1., t.border), StrokeKind::Inside);
            let method_label = if request.protocol == "grpc" {
                "gRPC"
            } else {
                &request.method
            };
            let method_size = if request.protocol == "grpc" { 13. } else { 12. };
            let method_width = (ui
                .painter()
                .layout_no_wrap(
                    method_label.into(),
                    FontId::new(method_size, egui::FontFamily::Name("Medium".into())),
                    t.text,
                )
                .size()
                .x
                .ceil()
                + 22.)
                .max(45.);
            let method_rect = rect(url.left() + 6., url.top() + 3., method_width - 9., 28.);
            w::medium(
                ui,
                pos2(url.left() + 11., url.center().y),
                method_label,
                method_size,
                if request.protocol == "grpc" {
                    t.green
                } else {
                    t.method(&request.method)
                },
            );
            let method_response = ui.interact(
                method_rect,
                ui.scope_id().with("http-method"),
                Sense::click(),
            );
            if !matches!(request.protocol.as_str(), "grpc" | "ws") {
                egui::Popup::menu(&method_response).show(|ui| {
                    for m in [
                        "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE",
                        "CONNECT",
                    ] {
                        if ui
                            .selectable_label(
                                request.method == m,
                                egui::RichText::new(m).color(t.method(m)),
                            )
                            .clicked()
                        {
                            request.method = m.into();
                            changed = true;
                            ui.close();
                        }
                    }
                });
            }
            let url_before = request.url.clone();
            let variables = self.vars(&request);
            let url_edit = crate::url_variables::editor(
                ui,
                rect(
                    url.left() + method_width,
                    url.top() + 1.,
                    url.width() - method_width - 68.,
                    url.height() - 2.,
                ),
                ui.scope_id().with(("url", &request.id)),
                &mut request.url,
                &variables,
                "Enter request URL",
                t,
            );
            changed |= url_edit.changed();
            if url_edit.changed() && crate::curl_editor::looks_like_command(&request.url) {
                let source = request.url.clone();
                match crate::import::curl(&source) {
                    Ok(parsed) => {
                        crate::curl_editor::apply(&mut request, parsed);
                    }
                    Err(error) => {
                        request.url = url_before;
                        self.curl.open = true;
                        let draft = self.curl.drafts.entry(request.id.clone()).or_default();
                        draft.source = source;
                        draft.edit();
                        self.notice(format!("{error:#}"));
                    }
                }
            }
            if url_edit.changed() {
                crate::query::update_rows(&mut request);
                let path = request.url.split('?').next().unwrap_or("");
                let names = path
                    .split('/')
                    .filter_map(|s| s.strip_prefix(':'))
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>();
                request
                    .path_params
                    .retain(|p| names.contains(&p.key.as_str()));
                for name in names {
                    if !request.path_params.iter().any(|p| p.key == name) {
                        request.path_params.push(Pair::new(name, ""));
                    }
                }
            }
            if url_edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.actions.push(Action::Send);
            }
            if w::icon_button(
                ui,
                rect(
                    url.right() - if request.protocol == "grpc" { 97. } else { 63. },
                    url.top() + 3.,
                    28.,
                    28.,
                ),
                Icon::Code,
                "Generate code",
                t,
            )
            .clicked()
            {
                self.curl.open = !self.curl.open;
            }
            if w::icon_button(
                ui,
                rect(
                    url.right() - if request.protocol == "grpc" { 65. } else { 31. },
                    url.top() + 3.,
                    28.,
                    28.,
                ),
                Icon::Save,
                "Save request (Ctrl+S)",
                t,
            )
            .clicked()
            {
                self.actions.push(Action::Save);
            }
            let running = self.running.contains_key(&request.id);
            let send = if request.protocol == "grpc" {
                w::icon_button(
                    ui,
                    rect(url.right() - 33., url.top() + 3., 28., 28.),
                    if running {
                        Icon::Close
                    } else {
                        Icon::ArrowRight
                    },
                    if running {
                        "Cancel call"
                    } else {
                        "Send gRPC request"
                    },
                    t,
                )
                .clicked()
            } else {
                w::button(
                    ui,
                    rect(
                        r.right() - if request.protocol == "ws" { 106. } else { 82. },
                        url.top(),
                        if request.protocol == "ws" { 90. } else { 66. },
                        34.,
                    ),
                    if request.protocol == "ws" {
                        if running { "Disconnect" } else { "Connect" }
                    } else if running {
                        "Cancel"
                    } else {
                        "Send"
                    },
                    true,
                    t,
                )
                .clicked()
            };
            if send {
                self.actions.push(if running {
                    Action::Cancel
                } else {
                    Action::Send
                });
            }
        }
        let mut r = r;
        let mut command_panel = None;
        if self.curl.open && request.protocol != "ws" {
            let panel_width = (r.width() * 0.36).clamp(300., 460.).min(r.width() - 320.);
            let panel = rect(
                r.right() - panel_width,
                r.top() + 178.,
                panel_width,
                r.height() - 178.,
            );
            command_panel = Some(panel);
            r.set_right(r.right() - panel_width);
        }
        let content = rect(
            r.left() + 16.,
            r.top() + 183.6,
            r.width() - 32.,
            r.height() - 199.6,
        );
        changed |= self.request_response_panes(ui, content, &mut request);
        if let Some(panel) = command_panel {
            changed |= self.curl_panel(ui, panel, &mut request, changed);
        }
        if tree_fields.0 != request.method
            || tree_fields.1 != request.url
            || tree_fields.2 != request.name
        {
            self.tree.stale = true;
        }
        self.ws.requests.push(request);
        let last = self.ws.requests.len() - 1;
        self.ws.requests.swap(index, last);
        if changed {
            self.mark_edited();
        }
    }
    fn home(&mut self, ui: &mut Ui, r: Rect) {
        self.workspace_home(ui, r);
    }
}
