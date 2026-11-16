use crate::{
    app::{Action, App, Dialog},
    storage,
    theme::hex,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};

pub const DEFAULT_DOCS: &str = "Welcome to your collection documentation! This space is designed to help you document your API collection effectively.\n\n## Overview\nUse this section to provide a high-level overview of your collection. You can describe:\n\n- The purpose of these API endpoints\n- Key features and functionalities\n- Target audience or users\n\n## Best Practices\n- Keep documentation up to date\n- Include request/response examples\n- Document error scenarios\n- Add relevant links and references\n\n## Markdown Support\nThis documentation supports Markdown formatting! You can use:\n\n- **Bold** and *italic* text\n- `code blocks` and syntax highlighting\n- Tables and lists\n- [Links](https://reqon.ru/docs/)\n- And more!";

impl App {
    pub fn migrate_collection(&mut self, cid: &str, ctx: &egui::Context) {
        let Some(collection) = self.ws.collections.iter().find(|c| c.id == cid).cloned() else {
            return;
        };
        let requests = self
            .ws
            .requests
            .iter()
            .filter(|r| r.collection == cid)
            .cloned()
            .collect::<Vec<_>>();
        let tx = self.tx.clone();
        let repaint = ctx.clone();
        let cid = cid.to_owned();
        self.runtime.spawn_blocking(move || {
            let result = crate::yaml::migrate(&collection, &requests)
                .map_err(|e| format!("Migration failed: {e:#}"));
            let _ = tx.send(crate::app::Event::Migrated(cid, result));
            repaint.request_repaint();
        });
        self.notice("Converting collection to YAML…");
    }
    pub fn open_collection_settings(&mut self, cid: &str) {
        let id = format!("__collection__{cid}");
        if !self.ws.collection_drafts.contains_key(&id) {
            let Some(c) = self.ws.collections.iter().find(|c| c.id == cid) else {
                return;
            };
            let path = c.path.join(if c.path.join("opencollection.yml").is_file() {
                "opencollection.yml"
            } else {
                "collection.req"
            });
            match crate::inheritance::parent(&path) {
                Ok(request) => {
                    let mut request = request.unwrap_or_default();
                    request.id = id.clone();
                    request.protocol = "collection".into();
                    request.collection = cid.into();
                    request.name = c.name.clone();
                    request.path = Some(path);
                    self.saved.insert(id.clone(), storage::to_bru(&request));
                    self.ws.collection_drafts.insert(id.clone(), request);
                }
                Err(error) => {
                    self.notice(format!("Cannot open collection settings: {error:#}"));
                    return;
                }
            }
        }
        self.open(id);
    }
    pub fn collection_workspace(&mut self, ui: &mut Ui, r: Rect, cid: &str) {
        let id = format!("__collection__{cid}");
        if !self.ws.collection_drafts.contains_key(&id) {
            self.open_collection_settings(cid);
        }
        let Some(mut draft) = self.ws.collection_drafts.remove(&id) else {
            return;
        };
        let t = self.theme;
        let mut selected = self.tabs.entry(id.clone()).or_default().collection_tab;
        let x = r.left() + 16.;
        let names = [
            "Overview",
            "Headers",
            "Vars",
            "Auth",
            "Script",
            "Tests",
            "Presets",
            "Proxy",
            "Client Certificates",
            "Secrets",
            "Protobuf",
        ];
        let mut strip = w::child(
            ui,
            rect(x, r.top() + 16., r.width() - 102., 35.),
            "collection-tabs",
        );
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(&mut strip, |ui| {
                ui.spacing_mut().item_spacing = vec2(19.2, 0.);
                ui.horizontal(|ui| {
                    for (i, name) in names.iter().enumerate() {
                        let row = ui.allocate_space(vec2(w::text_width(ui, name, 13.), 35.)).1;
                        let response = ui.interact(row, ui.scope_id().with(name), Sense::click());
                        w::label(
                            ui,
                            pos2(row.left(), row.top() + 16.),
                            name,
                            13.,
                            if selected == i {
                                t.text
                            } else {
                                hex(if t.dark { 0x999999 } else { 0x666666 })
                            },
                        );
                        if selected == i {
                            ui.painter().rect_filled(
                                rect(row.left(), row.bottom() - 2., row.width(), 2.),
                                0,
                                t.accent,
                            );
                        }
                        if response.clicked() {
                            selected = i;
                        }
                    }
                });
            });
        self.tabs.entry(id.clone()).or_default().collection_tab = selected;
        let mut docs_edit = self.tabs.get(&id).is_some_and(|s| s.docs_edit);
        if selected == 0 {
            let edit = rect(r.right() - 67., r.top() + 16., 51., 30.);
            w::icon(
                ui,
                pos2(edit.left() + 8., edit.center().y),
                Icon::Edit,
                t.muted,
                14.,
            );
            w::label(
                ui,
                pos2(edit.left() + 20., edit.center().y),
                if docs_edit { "Done" } else { "Edit" },
                12.,
                t.muted,
            );
            if ui
                .interact(
                    edit,
                    ui.scope_id().with("collection-edit-docs"),
                    Sense::click(),
                )
                .clicked()
            {
                docs_edit = !docs_edit;
                self.tabs.entry(id.clone()).or_default().docs_edit = docs_edit;
            }
        } else if w::button(
            ui,
            rect(r.right() - 74., r.top() + 16., 58., 30.),
            "Save",
            true,
            t,
        )
        .clicked()
        {
            self.save_tree_draft(&draft);
        }
        let content = rect(
            r.left() + 16.,
            r.top() + 64.,
            r.width() - 32.,
            r.height() - 80.,
        );
        let mut changed = false;
        match selected {
            0 => {
                let split = content.width() * 0.4 + 8.;
                let left = rect(content.left(), content.top(), split - 28., content.height());
                let right = rect(
                    content.left() + split,
                    content.top(),
                    content.width() - split,
                    content.height(),
                );
                let mut info = w::child(ui, left, "collection-info");
                info.spacing_mut().item_spacing.y = 0.;
                egui::ScrollArea::vertical().show(&mut info, |ui| {
                    let row = ui.allocate_space(vec2(ui.available_width(), 38.)).1;
                    w::icon(
                        ui,
                        pos2(row.left() + 9., row.top() + 16.),
                        Icon::Box,
                        t.text,
                        20.,
                    );
                    self.collection_name_editor(
                        ui,
                        rect(row.left() + 28., row.top(), row.width() - 28., 34.),
                        cid,
                        &mut draft.name,
                    );
                    ui.add_space(16.);
                    let (path, env_count) = self
                        .ws
                        .collections
                        .iter()
                        .find(|c| c.id == cid)
                        .map(|c| (c.path.to_string_lossy().into_owned(), c.environments.len()))
                        .unwrap_or_default();
                    let count = self
                        .ws
                        .requests
                        .iter()
                        .filter(|r| r.collection == cid)
                        .count();
                    for (n, (title, value, icon, color, height)) in [
                        ("Location", path, Icon::Folder, hex(0x9ac6e7), 82.),
                        ("Version", "Not Set".into(), Icon::Tag, hex(0xe6c960), 66.),
                        (
                            "Environments",
                            format!("{env_count} collection environments\n0 global environments"),
                            Icon::Globe,
                            hex(0x80ce9b),
                            86.,
                        ),
                        (
                            "Requests",
                            format!("{count} requests in collection"),
                            Icon::Code,
                            hex(0xc397d8),
                            66.,
                        ),
                        (
                            "Share",
                            "Share Collection".into(),
                            Icon::External,
                            hex(0x9ac6e7),
                            66.,
                        ),
                        (
                            "Documentation",
                            "Generate Docs".into(),
                            Icon::File,
                            hex(0xe6c960),
                            66.,
                        ),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let row = ui.allocate_space(vec2(ui.available_width(), height)).1;
                        let square = rect(row.left(), row.top(), 46., 46.);
                        ui.painter().rect(
                            square,
                            7,
                            color.gamma_multiply(0.08),
                            Stroke::new(1., color.gamma_multiply(0.16)),
                            StrokeKind::Inside,
                        );
                        w::icon(ui, square.center(), icon, color, 20.);
                        w::label(
                            ui,
                            pos2(row.left() + 62., row.top() + 10.),
                            title,
                            13.,
                            t.text,
                        );
                        let mut text = w::child(
                            ui,
                            rect(
                                row.left() + 62.,
                                row.top() + 24.,
                                row.width() - 62.,
                                height - 24.,
                            ),
                            ("collection-info", n),
                        );
                        let response = text.add(
                            egui::Label::new(egui::RichText::new(&value).size(13.).color(
                                if matches!(n, 2 | 4 | 5) {
                                    t.blue
                                } else {
                                    t.muted
                                },
                            ))
                            .wrap()
                            .sense(Sense::click()),
                        );
                        if response.clicked() {
                            match n {
                                0 => {
                                    if let Some(path) = draft.path.as_ref().and_then(|p| p.parent())
                                    {
                                        let _ = std::process::Command::new("explorer.exe")
                                            .arg(path)
                                            .spawn();
                                    }
                                }
                                2 => self.actions.push(Action::Dialog(Dialog::Environments)),
                                4 => self.actions.push(Action::ExportPostman(None)),
                                5 => {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .set_file_name("documentation.md")
                                        .save_file()
                                    {
                                        if let Err(e) = storage::atomic_write(
                                            &path,
                                            if draft.docs.is_empty() {
                                                DEFAULT_DOCS
                                            } else {
                                                &draft.docs
                                            },
                                        ) {
                                            self.notice(e.to_string());
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if draft.file_format != "yml" {
                        ui.add_space(44.);
                        let title = ui.allocate_space(vec2(ui.available_width(), 42.)).1;
                        w::icon(
                            ui,
                            pos2(title.left() + 9., title.top() + 16.),
                            Icon::Refresh,
                            t.text,
                            20.,
                        );
                        w::medium(
                            ui,
                            pos2(title.left() + 28., title.top() + 16.),
                            "Migration",
                            18.,
                            t.text,
                        );
                        let row = ui.allocate_space(vec2(ui.available_width(), 108.)).1;
                        let square = rect(row.left(), row.top(), 46., 46.);
                        ui.painter().rect(
                            square,
                            7,
                            t.accent.gamma_multiply(0.08),
                            Stroke::new(1., t.accent.gamma_multiply(0.16)),
                            StrokeKind::Inside,
                        );
                        w::icon(ui, square.center(), Icon::File, t.accent, 20.);
                        w::label(
                            ui,
                            pos2(row.left() + 62., row.top() + 10.),
                            "Migrate to YML file format",
                            13.,
                            t.text,
                        );
                        let mut text = w::child(
                            ui,
                            rect(row.left() + 62., row.top() + 24., row.width() - 62., 48.),
                            "migration-caption",
                        );
                        text.label(
                            egui::RichText::new(
                                "This collection is stored in REQ format. Switch to YML.",
                            )
                            .size(13.)
                            .color(t.muted),
                        );
                        if w::button(
                            ui,
                            rect(row.left() + 62., row.top() + 74., 115., 26.),
                            "Convert to YML",
                            true,
                            t,
                        )
                        .clicked()
                        {
                            self.migrate_collection(cid, ui.ctx());
                        }
                    }
                });
                let mut docs = w::child(ui, right, "collection-docs");
                docs.spacing_mut().item_spacing.y = 0.;
                let row = docs.allocate_space(vec2(docs.available_width(), 38.)).1;
                w::icon(
                    &docs,
                    pos2(row.left() + 9., row.top() + 16.),
                    Icon::File,
                    t.text,
                    20.,
                );
                w::medium(
                    &docs,
                    pos2(row.left() + 28., row.top() + 16.),
                    "Documentation",
                    20.,
                    t.text,
                );
                docs.add_space(8.);
                if docs_edit {
                    if draft.docs.is_empty() {
                        draft.docs = DEFAULT_DOCS.into();
                        changed = true;
                    }
                    changed |= w::editor(&mut docs, &mut draft.docs, false, t);
                } else {
                    egui::ScrollArea::vertical().show(&mut docs, |ui| {
                        crate::markdown::render(
                            ui,
                            if draft.docs.is_empty() {
                                DEFAULT_DOCS
                            } else {
                                &draft.docs
                            },
                            t,
                        )
                    });
                }
            }
            1..=5 => {
                let tab = [0, 2, 4, 3, 5, 7][selected];
                self.tabs.entry(id.clone()).or_default().request_tab = tab;
                let mut body = w::child(ui, content, "collection-editor-clip");
                changed |= self.request_editor(
                    &mut body,
                    rect(
                        content.left(),
                        content.top() - 48.,
                        content.width(),
                        content.height() + 48.,
                    ),
                    &mut draft,
                );
                let save = rect(content.left(), r.bottom() - 48., 64., 30.);
                if w::button(ui, save, "Save", true, t).clicked() {
                    self.save_tree_draft(&draft);
                }
            }
            6 => {
                let mut body = w::child(ui, content, "collection-presets");
                changed |= crate::request_settings::draw(&mut body, &mut draft, t);
            }
            7 => {
                let mut body = w::child(ui, content, "collection-proxy");
                body.label("Proxy settings");
                body.add_space(16.);
                changed |=
                    crate::editors::auth_field(&mut body, "Proxy URL", &mut draft.proxy, false, t);
                body.label(
                    egui::RichText::new(
                        "HTTP and SOCKS5 proxies. Leave blank to use the system proxy.",
                    )
                    .size(12.)
                    .color(t.muted),
                );
            }
            8 => {
                let mut body = w::child(ui, content, "collection-tls");
                body.label("Client Certificates");
                body.add_space(16.);
                changed |= body
                    .checkbox(&mut draft.verify_tls, "Verify SSL certificates")
                    .changed();
                for (label, value) in [
                    ("Certificate PEM", &mut draft.client_certificate),
                    ("Private key PEM", &mut draft.client_key),
                    ("CA certificate PEM", &mut draft.ca_certificate),
                ] {
                    changed |= crate::editors::auth_field(&mut body, label, value, false, t);
                    if body.button(format!("Choose {label}")).clicked() {
                        if let Some(p) = rfd::FileDialog::new().pick_file() {
                            *value = p.to_string_lossy().into_owned();
                            changed = true;
                        }
                    }
                    body.add_space(16.);
                }
            }
            9 => {
                let mut body = w::child(ui, content, "collection-secrets");
                body.label("Secret variables are configured in collection environments.");
                body.add_space(16.);
                if body.button("Configure environments").clicked() {
                    self.open_environment_secrets(cid);
                }
                body.add_space(24.);
                crate::auth_migration_ui::draw(self, &mut body, cid);
            }
            10 => {
                let mut body = w::child(ui, content, "collection-protobuf");
                body.label("Protobuf files");
                body.add_space(16.);
                let mut remove = None;
                for (i, file) in draft.proto_files.iter_mut().enumerate() {
                    body.horizontal(|ui| {
                        changed |= ui
                            .add_sized(
                                vec2(500_f32.min(ui.available_width() - 70.), 29.),
                                TextEdit::singleline(file),
                            )
                            .changed();
                        if ui.button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
                if let Some(i) = remove {
                    draft.proto_files.remove(i);
                    changed = true;
                }
                if body.button("Add proto file").clicked() {
                    if let Some(paths) = rfd::FileDialog::new()
                        .add_filter("Protobuf", &["proto"])
                        .pick_files()
                    {
                        draft
                            .proto_files
                            .extend(paths.into_iter().map(|p| p.to_string_lossy().into_owned()));
                        changed = true;
                    }
                }
            }
            _ => {}
        }
        self.ws.collection_drafts.insert(id, draft);
        if changed {
            self.mark_changed();
        }
    }
}
