use crate::{
    app::{Action, App},
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

impl App {
    pub fn workspace_home(&mut self, ui: &mut egui::Ui, r: Rect) {
        if self.workspaces.manage {
            self.workspace_manager(ui, r);
            return;
        }
        let t = self.theme;
        let state_id = ui.scope_id().with("workspace-home");
        let mut selected = self.home_tab;
        let mut edit = ui
            .ctx()
            .data_mut(|d| d.get_temp::<bool>(state_id.with("edit")))
            .unwrap_or(false);
        w::icon(
            ui,
            pos2(r.left() + 29., r.top() + 23.),
            Icon::Workspace,
            t.text,
            18.,
        );
        w::semibold(
            ui,
            pos2(r.left() + 54., r.top() + 23.),
            &self.ws.name,
            14.,
            t.text,
        );
        w::icon(
            ui,
            pos2(
                r.left() + 54. + w::text_width(ui, &self.ws.name, 14.) + 18.,
                r.top() + 23.,
            ),
            Icon::Down,
            t.muted,
            12.,
        );
        let tab_y = r.top() + 48.;
        let workspace_header = ui.interact(
            rect(
                r.left() + 17.,
                r.top() + 6.,
                (w::text_width(ui, &self.ws.name, 14.) + 74.).min(r.width() - 30.),
                35.,
            ),
            state_id.with("workspace-actions"),
            Sense::click(),
        );
        self.workspace_actions_menu(ui, &workspace_header);
        let mut x = r.left() + 11.;
        let mut span = (0., 0.);
        let mut home_tabs = vec![
            ("Overview", Icon::Home, 96.),
            ("Environments", Icon::Globe, 123.),
        ];
        if self.preferences_shown {
            home_tabs.push(("Preferences", Icon::Settings, 113.));
        }
        for (i, (name, icon, width)) in home_tabs.into_iter().enumerate() {
            let tab = rect(x, tab_y, width, 36.);
            let active = selected == i;
            ui.painter().rect(
                tab,
                egui::CornerRadius {
                    nw: 7,
                    ne: 7,
                    sw: if active { 0 } else { 4 },
                    se: if active { 0 } else { 4 },
                },
                if active { t.bg } else { t.surface },
                if active {
                    Stroke::new(1., t.strong_border)
                } else {
                    Stroke::NONE
                },
                StrokeKind::Inside,
            );
            w::icon(
                ui,
                pos2(tab.left() + 15., tab.top() + 17.),
                icon,
                t.accent,
                13.,
            );
            w::label(
                ui,
                pos2(
                    tab.left() + 28.,
                    tab.top() + if active { 16.25 } else { 17.25 },
                ),
                name,
                13.,
                t.text,
            );
            if ui
                .interact(tab, state_id.with(name), Sense::click())
                .clicked()
            {
                selected = i;
            }
            if active {
                span = (tab.left(), tab.right());
            }
            x += width + 3.;
        }
        if w::icon_button(
            ui,
            rect(x + 10., tab_y + 4., 28., 28.),
            Icon::Plus,
            "New collection",
            t,
        )
        .clicked()
        {
            self.actions.push(Action::NewCollection);
        }
        w::line(
            ui,
            pos2(r.left(), tab_y + 36.),
            pos2(span.0, tab_y + 36.),
            t.strong_border,
        );
        w::line(
            ui,
            pos2(span.1, tab_y + 36.),
            pos2(r.right(), tab_y + 36.),
            t.strong_border,
        );
        ui.painter().rect_filled(
            rect(span.0 + 1., tab_y + 35., span.1 - span.0 - 2., 2.),
            0,
            t.bg,
        );
        let content = rect(r.left(), tab_y + 37., r.width(), r.height() - 86.);
        if selected == 2 {
            self.preferences_workspace(ui, content);
        } else if selected == 1 {
            self.open_environment_manager("");
        } else {
            let split = content.left() + content.width() * 0.61;
            w::line(
                ui,
                pos2(split, content.top()),
                pos2(split, content.bottom()),
                t.strong_border,
            );
            let left = rect(
                content.left() + 16.,
                content.top(),
                split - content.left() - 32.,
                content.height(),
            );
            for (x, value, name) in [
                (
                    left.left(),
                    self.ws.collections.len().to_string(),
                    "Collections",
                ),
                (
                    left.left() + 82.,
                    self.ws.environments.len().to_string(),
                    "Environments",
                ),
            ] {
                w::semibold(ui, pos2(x, content.top() + 31.), &value, 22., t.text);
                w::label(ui, pos2(x, content.top() + 52.), name, 11., t.muted);
            }
            w::label(
                ui,
                pos2(left.left(), content.top() + 86.),
                "Quick Actions",
                12.,
                t.text,
            );
            let mut x = left.left();
            for (name, icon, width, action) in [
                ("Create Collection", Icon::Plus, 144., Action::NewCollection),
                (
                    "Open Collection",
                    Icon::Folder,
                    137.,
                    Action::OpenCollection,
                ),
                (
                    "Import Collection",
                    Icon::Download,
                    144.,
                    Action::ImportPostman,
                ),
            ] {
                if w::soft_button(ui, rect(x, content.top() + 103., width, 28.), name, icon, t)
                    .clicked()
                {
                    self.actions.push(action);
                }
                x += width + 8.;
            }
            w::label(
                ui,
                pos2(left.left(), content.top() + 157.),
                "Collections",
                12.,
                t.text,
            );
            let mut list = w::child(
                ui,
                rect(
                    left.left(),
                    content.top() + 178.,
                    left.width(),
                    content.height() - 182.,
                ),
                "home-collections",
            );
            let mut open = None;
            egui::ScrollArea::vertical().show(&mut list, |ui| {
                ui.spacing_mut().item_spacing.y = 10.;
                for c in &self.ws.collections {
                    let row = ui.allocate_space(vec2(ui.available_width(), 58.)).1;
                    w::icon(
                        ui,
                        pos2(row.left() + 9., row.top() + 14.),
                        Icon::Box,
                        t.text,
                        18.,
                    );
                    w::elided_label(
                        ui,
                        rect(row.left() + 26., row.top(), row.width() - 65., 28.),
                        &c.name,
                        13.,
                        t.text,
                    );
                    w::elided_label(
                        ui,
                        rect(row.left(), row.top() + 26., row.width() - 35., 20.),
                        &c.path.to_string_lossy(),
                        11.,
                        t.muted,
                    );
                    if ui
                        .interact(
                            rect(row.left(), row.top(), row.width() - 32., row.height()),
                            ui.scope_id().with(&c.id),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        open = Some(c.id.clone());
                    }
                    let more = w::icon_button(
                        ui,
                        rect(row.right() - 26., row.top() + 10., 26., 26.),
                        Icon::Dots,
                        "Collection actions",
                        t,
                    );
                    egui::Popup::menu(&more).show(|ui| {
                        if ui.button("Collection Settings").clicked() {
                            open = Some(c.id.clone());
                            ui.close();
                        }
                        if ui.button("New request").clicked() {
                            self.actions
                                .push(Action::NewRequest(c.id.clone(), String::new()));
                            ui.close();
                        }
                        if ui.button("Open folder").clicked() {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg(&c.path)
                                .spawn();
                            ui.close();
                        }
                    });
                }
            });
            if let Some(cid) = open {
                self.open_collection_settings(&cid);
            }
            let right = rect(
                split,
                content.top(),
                content.right() - split,
                content.height(),
            );
            w::icon(
                ui,
                pos2(right.left() + 25., right.top() + 21.),
                Icon::File,
                t.text,
                14.,
            );
            w::label(
                ui,
                pos2(right.left() + 39., right.top() + 21.),
                "Documentation",
                12.,
                t.text,
            );
            w::line(
                ui,
                pos2(right.left(), right.top() + 42.),
                pos2(right.right(), right.top() + 42.),
                t.strong_border,
            );
            if !self.ws.docs.is_empty() || edit {
                let edit_r = rect(right.right() - 64., right.top() + 7., 54., 28.);
                if w::button(ui, edit_r, if edit { "Save" } else { "Edit" }, false, t).clicked() {
                    edit = !edit;
                    self.mark_changed();
                }
                let mut docs = w::child(
                    ui,
                    rect(
                        right.left() + 16.,
                        right.top() + 58.,
                        right.width() - 32.,
                        right.height() - 74.,
                    ),
                    "workspace-docs",
                );
                if edit {
                    if w::editor(&mut docs, &mut self.ws.docs, false, t) {
                        self.mark_changed();
                    }
                } else {
                    egui::ScrollArea::vertical().show(&mut docs, |ui| {
                        crate::markdown::render(ui, &self.ws.docs, t)
                    });
                }
            } else {
                let center = right.center() + vec2(0., 21.);
                w::icon(ui, center + vec2(0., -98.), Icon::File, t.muted, 48.);
                for (dy, text) in [
                    (-49., "Add documentation to help your team work smoothly."),
                    (-30., "You can include:"),
                ] {
                    let width = w::text_width(ui, text, 12.);
                    w::label(
                        ui,
                        pos2(center.x - width / 2., center.y + dy),
                        text,
                        12.,
                        t.muted,
                    );
                }
                for (i, text) in [
                    "Project overview",
                    "Setup instructions",
                    "Key workflows",
                    "Resources & FAQs",
                ]
                .into_iter()
                .enumerate()
                {
                    let width = w::text_width(ui, text, 12.);
                    w::label(
                        ui,
                        pos2(center.x - width / 2. - 12., center.y - 2. + i as f32 * 22.),
                        "•",
                        12.,
                        t.accent,
                    );
                    w::label(
                        ui,
                        pos2(center.x - width / 2., center.y - 2. + i as f32 * 22.),
                        text,
                        12.,
                        t.muted,
                    );
                }
                if w::soft_button(
                    ui,
                    rect(center.x - 79., center.y + 95., 158., 28.),
                    "Add Documentation",
                    Icon::Plus,
                    t,
                )
                .clicked()
                {
                    edit = true;
                }
            }
        }
        self.home_tab = selected;
        ui.ctx().data_mut(|d| {
            d.insert_temp(state_id.with("edit"), edit);
        });
    }
}
