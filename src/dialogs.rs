use crate::{
    app::{Action, App, Dialog},
    editors::field,
    model::*,
    storage, widgets as w,
};
use eframe::egui::{self, vec2};

impl App {
    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let dialog = self.dialog.clone();
        if dialog == Dialog::None {
            return;
        }
        if self.tree_item_dialog(ctx) {
            return;
        }
        if dialog == Dialog::ExportCollections {
            self.export_collections_dialog(ctx);
            return;
        }
        if dialog == Dialog::Cookies {
            self.cookies_dialog(ctx);
            return;
        }
        if let Dialog::RemoveCollection(cid) = &dialog {
            self.remove_collection_dialog(ctx, cid);
            return;
        }
        if let Dialog::NewRequest(cid, folder) = &dialog {
            if folder != "__folder__" {
                self.new_request_dialog(ctx, cid, folder);
                return;
            }
        }
        let t = self.theme;
        let title = match &dialog {
            Dialog::NewCollection => "Create Collection",
            Dialog::NewRequest(_, f) if f == "__folder__" => "New Folder",
            Dialog::NewRequest(_, _) => "New Request",
            Dialog::Environments => "Environments",
            Dialog::Settings => "Preferences",
            Dialog::Snippet => "Generate Code",
            Dialog::ImportCurl => "Import cURL",
            Dialog::Runner => "Collection Runner",
            Dialog::History => "Request History",
            Dialog::Cookies => "Cookies",
            Dialog::About => "About Reqon",
            Dialog::ApiSpecs => "API Specs",
            Dialog::MockServers => "Mock Servers",
            Dialog::CloseTab(_) => "Unsaved changes",
            _ => "Reqon",
        };
        let mut close = false;
        let width = match dialog {
            Dialog::Environments
            | Dialog::Snippet
            | Dialog::Runner
            | Dialog::History
            | Dialog::ImportCurl => 720.,
            _ => 500.,
        };
        let (_, modal_closed) = w::modal(ctx, title, width, t, |ui| {
            match &dialog {
                Dialog::RemoveCollection(_) => unreachable!("dedicated collection dialog"),
                Dialog::NewCollection => {
                    field(ui, "Name", &mut self.entry, false);
                    ui.add_space(10.);
                    ui.label(
                        egui::RichText::new(
                            "Collections are folders on disk. Requests are plain-text .req files.",
                        )
                        .size(12.)
                        .color(t.muted),
                    );
                    ui.horizontal(|ui| {
                        ui.label("Location");
                        let default_location = self
                            .workspace_folder()
                            .join("collections")
                            .to_string_lossy()
                            .into_owned();
                        ui.add(
                            egui::TextEdit::singleline(&mut self.entry2)
                                .hint_text(default_location),
                        );
                        if ui.button("Browse").clicked() {
                            if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                self.entry2 = p.to_string_lossy().into_owned();
                            }
                        }
                    });
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.entry.trim().is_empty(), egui::Button::new("Create"))
                            .clicked()
                        {
                            let base = if self.entry2.trim().is_empty() {
                                self.workspace_folder().join("collections")
                            } else {
                                std::path::PathBuf::from(&self.entry2)
                            };
                            let path = base.join(safe_name(&self.entry));
                            if path.exists() {
                                self.notice(
                                    "A folder with this name already exists. Choose another name.",
                                );
                            } else {
                                let c = Collection {
                                    id: id(),
                                    name: self.entry.trim().into(),
                                    path,
                                    environments: vec![],
                                    vars: vec![],
                                    folders: vec![],
                                };
                                match storage::create_collection(&c) {
                                    Ok(()) => {
                                        let cid = c.id.clone();
                                        self.ws.collections.push(c);
                                        self.mark_changed();
                                        self.notice("Collection created");
                                        self.actions.push(Action::NewRequest(cid, String::new()));
                                        close = true;
                                    }
                                    Err(e) => self.notice(format!("Create failed: {e:#}")),
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::NewRequest(cid, folder) => {
                    if folder != "__folder__" {
                        ui.horizontal(|ui| {
                            ui.label("Type");
                            ui.label("HTTP Request");
                            ui.add_space(30.);
                            egui::ComboBox::from_id_salt("new-method")
                                .selected_text(&self.entry2)
                                .show_ui(ui, |ui| {
                                    for method in
                                        ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                                    {
                                        ui.selectable_value(
                                            &mut self.entry2,
                                            method.into(),
                                            method,
                                        );
                                    }
                                });
                        });
                        ui.add_space(10.);
                    }
                    field(ui, "Name", &mut self.entry, false);
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(!self.entry.trim().is_empty(), egui::Button::new("Create"))
                            .clicked()
                        {
                            if folder == "__folder__" {
                                if let Some(c) =
                                    self.ws.collections.iter_mut().find(|c| &c.id == cid)
                                {
                                    let name = safe_name(&self.entry);
                                    match std::fs::create_dir_all(c.path.join(&name)) {
                                        Ok(()) => {
                                            if !c.folders.contains(&name) {
                                                c.folders.push(name);
                                            }
                                            self.mark_changed();
                                            close = true;
                                        }
                                        Err(e) => self.notice(e.to_string()),
                                    }
                                }
                            } else {
                                self.add_request(
                                    cid.clone(),
                                    folder.clone(),
                                    self.entry.trim().into(),
                                    self.entry2.clone(),
                                );
                                close = true;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Environments => {
                    let cid = self.active_collection_id();
                    let idx = self.ws.collections.iter().position(|c| c.id == cid);
                    let mut names =
                        vec!["Global variables".to_owned(), "Collection variables".into()];
                    if let Some(index) = idx {
                        names.extend(
                            self.ws.collections[index]
                                .environments
                                .iter()
                                .map(|e| e.name.clone()),
                        );
                    }
                    self.env_index = self.env_index.min(names.len() - 1);
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("edit-environment")
                            .selected_text(&names[self.env_index])
                            .width(220.)
                            .show_ui(ui, |ui| {
                                for (i, name) in names.iter().enumerate() {
                                    ui.selectable_value(&mut self.env_index, i, name);
                                }
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut self.entry)
                                .desired_width(170.)
                                .hint_text("New environment name"),
                        );
                        if ui
                            .add_enabled(
                                idx.is_some() && !self.entry.trim().is_empty(),
                                egui::Button::new("Add"),
                            )
                            .clicked()
                        {
                            let c = &mut self.ws.collections[idx.unwrap()];
                            if !c.environments.iter().any(|e| e.name == self.entry.trim()) {
                                c.environments.push(Environment {
                                    name: self.entry.trim().into(),
                                    vars: vec![],
                                });
                                self.env_index = c.environments.len() + 1;
                                self.entry.clear();
                                self.mark_changed();
                            }
                        }
                    });
                    ui.add_space(14.);
                    egui::ScrollArea::vertical()
                        .max_height(400.)
                        .min_scrolled_height(180.)
                        .show(ui, |ui| {
                            let rows = if self.env_index == 0 {
                                Some(&mut self.ws.globals)
                            } else if let Some(index) = idx {
                                if self.env_index == 1 {
                                    Some(&mut self.ws.collections[index].vars)
                                } else {
                                    self.ws.collections[index]
                                        .environments
                                        .get_mut(self.env_index - 2)
                                        .map(|e| &mut e.vars)
                                }
                            } else {
                                None
                            };
                            if let Some(rows) = rows {
                                if w::pairs(ui, rows, ["Name", "Value"], t) {
                                    self.mark_changed();
                                }
                            }
                        });
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            let result = if let Some(index) = idx {
                                storage::save_environments(&self.ws.collections[index])
                            } else {
                                Ok(())
                            };
                            match result {
                                Ok(()) => {
                                    self.runtime_vars.clear();
                                    self.flush();
                                    self.notice("Environments saved");
                                    close = true;
                                }
                                Err(e) => self.notice(format!("Save failed: {e:#}")),
                            }
                        }
                        if ui.button("Close").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Settings => {
                    ui.label(egui::RichText::new("Appearance").strong());
                    ui.add_space(8.);
                    ui.horizontal(|ui| {
                        ui.label("Theme");
                        if ui.selectable_label(self.ws.dark, "Dark").clicked() && !self.ws.dark {
                            self.actions.push(Action::ToggleTheme);
                        }
                        if ui.selectable_label(!self.ws.dark, "Light").clicked() && self.ws.dark {
                            self.actions.push(Action::ToggleTheme);
                        }
                    });
                    let before = self.ws.side_by_side;
                    ui.checkbox(
                        &mut self.ws.side_by_side,
                        "Show request and response side by side",
                    );
                    if before != self.ws.side_by_side {
                        self.mark_changed();
                    }
                    ui.add_space(18.);
                    ui.separator();
                    ui.label(egui::RichText::new("Workspace").strong());
                    ui.add_space(8.);
                    if field(ui, "Name", &mut self.ws.name, false) {
                        self.mark_changed();
                    }
                    ui.label(
                        egui::RichText::new(self.root.to_string_lossy())
                            .size(11.)
                            .color(t.muted),
                    );
                    ui.add_space(18.);
                    ui.label("Keyboard shortcuts");
                    for (a, b) in [
                        ("Ctrl+Enter", "Send request"),
                        ("Ctrl+S", "Save request"),
                        ("Ctrl+N", "New request"),
                        ("Ctrl+O", "Open collection"),
                        ("Ctrl+W", "Close tab"),
                        ("Ctrl+K", "Search requests"),
                    ] {
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                vec2(140., 20.),
                                egui::Label::new(egui::RichText::new(a).monospace()),
                            );
                            ui.label(b);
                        });
                    }
                    ui.add_space(14.);
                    if ui.button("Done").clicked() {
                        self.flush();
                        close = true;
                    }
                }
                Dialog::Snippet => {
                    ui.label("cURL (POSIX shell)");
                    ui.add_space(10.);
                    ui.set_min_height(250.);
                    w::editor(ui, &mut self.snippet, true, t);
                    ui.add_space(12.);
                    if ui.button("Copy to clipboard").clicked() {
                        ctx.copy_text(self.snippet.clone());
                        self.notice("cURL copied");
                    }
                }
                Dialog::ImportCurl => {
                    ui.label("Paste a cURL or gRPCurl command");
                    egui::ComboBox::from_id_salt("import-command-shell")
                        .selected_text(match self.entry2.as_str() {
                            "bash" => "Bash",
                            "cmd" => "CMD",
                            "powershell" => "PowerShell",
                            _ => "Auto-detect shell",
                        })
                        .show_ui(ui, |ui| {
                            for (key, label) in [
                                ("auto", "Auto-detect shell"),
                                ("bash", "Bash"),
                                ("cmd", "CMD"),
                                ("powershell", "PowerShell"),
                            ] {
                                ui.selectable_value(&mut self.entry2, key.into(), label);
                            }
                        });
                    ui.add_space(8.);
                    ui.add_sized(
                        vec2(ui.available_width(), 220.),
                        egui::TextEdit::multiline(&mut self.entry)
                            .font(egui::FontId::monospace(13.))
                            .code_editor(),
                    );
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        if ui.button("Import").clicked() {
                            match crate::import::command(
                                &self.entry,
                                if self.entry2.is_empty() {
                                    "auto"
                                } else {
                                    &self.entry2
                                },
                            ) {
                                Ok(mut r) => {
                                    r.collection = self.active_collection_id();
                                    r.seq = self.ws.next_request_seq(&r.collection, "");
                                    let key = r.id.clone();
                                    self.ws.requests.push(r);
                                    self.tree.stale = true;
                                    self.open(key);
                                    self.notice("cURL imported. Save to add it to the collection.");
                                    close = true;
                                }
                                Err(e) => self.notice(format!("Import failed: {e:#}")),
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                }
                Dialog::Runner => {
                    ui.set_min_height((ctx.content_rect().height() - 180.).clamp(260., 580.));
                    let cid = self.active_collection_id();
                    ui.label(egui::RichText::new(self.collection_name(&cid)).size(16.));
                    ui.label(format!(
                        "Environment: {}",
                        if self.ws.selected_environment.is_empty() {
                            "No Environment"
                        } else {
                            &self.ws.selected_environment
                        }
                    ));
                    ui.add_space(12.);
                    ui.add_enabled_ui(!self.runner_active, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Iterations");
                            ui.add(
                                egui::DragValue::new(&mut self.runner_iterations).range(1..=10000),
                            );
                            ui.add_space(20.);
                            ui.label("Delay (ms)");
                            ui.add(egui::DragValue::new(&mut self.runner_delay).range(0..=3600000));
                        });
                        ui.checkbox(
                            &mut self.runner_stop_on_error,
                            "Stop on failed tests or request errors",
                        );
                    });
                    if self.runner_active {
                        ui.label(format!(
                            "Iteration {} of {}",
                            self.runner_iteration, self.runner_iterations
                        ));
                    }
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !self.runner_active && self.running.is_empty(),
                                egui::Button::new("Run collection"),
                            )
                            .clicked()
                        {
                            self.start_runner(&cid, ctx);
                        }
                        if self.runner_active && ui.button("Stop run").clicked() {
                            self.stop_runner();
                        }
                    });
                    ui.add_space(14.);
                    if !self.runner_results.is_empty() {
                        let passed = self.runner_results.iter().filter(|r| r.2).count();
                        ui.label(format!(
                            "{} requests · {} passed · {} failed",
                            self.runner_results.len(),
                            passed,
                            self.runner_results.len() - passed
                        ));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(400.)
                        .min_scrolled_height(150.)
                        .show_rows(ui, 26., self.runner_results.len(), |ui, range| {
                            ui.spacing_mut().item_spacing.y = 0.;
                            for i in range {
                                let (name, status, passed, time) = &self.runner_results[i];
                                let row = ui.allocate_space(vec2(ui.available_width(), 26.)).1;
                                if i % 2 == 1 {
                                    ui.painter().rect_filled(row, 0, t.hover);
                                }
                                w::label(
                                    ui,
                                    egui::pos2(row.left() + 5., row.center().y),
                                    if *passed { "PASS" } else { "FAIL" },
                                    12.,
                                    if *passed { t.green } else { t.red },
                                );
                                w::elided_label(
                                    ui,
                                    w::rect(
                                        row.left() + 66.,
                                        row.top(),
                                        row.width() - 245.,
                                        row.height(),
                                    ),
                                    name,
                                    13.,
                                    t.text,
                                );
                                w::label(
                                    ui,
                                    egui::pos2(row.right() - 155., row.center().y),
                                    &status.to_string(),
                                    12.,
                                    t.muted,
                                );
                                w::label(
                                    ui,
                                    egui::pos2(row.right() - 85., row.center().y),
                                    &format!("{time} ms"),
                                    12.,
                                    t.muted,
                                );
                            }
                        });
                    if self.runner_active {
                        ui.spinner();
                    } else if self.runner_cancelled {
                        ui.label("Collection run cancelled. Completed results are preserved.");
                    } else if self.runner_results.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "Requests execute sequentially. Runtime variables carry forward.",
                            )
                            .color(t.muted),
                        );
                    }
                }
                Dialog::History => {
                    let mut selected = None;
                    egui::ScrollArea::vertical()
                        .max_height(500.)
                        .min_scrolled_height(200.)
                        .show(ui, |ui| {
                            for (index, h) in self.ws.history.iter().enumerate().rev() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(&h.request.method)
                                            .color(t.method(&h.request.method))
                                            .size(11.),
                                    );
                                    if ui.selectable_label(false, &h.request.name).clicked() {
                                        selected = Some(index);
                                    }
                                    ui.label(format!(
                                        "{} · {}ms",
                                        h.reply.status, h.reply.elapsed_ms
                                    ));
                                });
                                ui.label(
                                    egui::RichText::new(&h.request.url).size(11.).color(t.muted),
                                );
                                ui.separator();
                            }
                            if self.ws.history.is_empty() {
                                ui.label("No requests have been sent yet");
                            }
                        });
                    if let Some(index) = selected {
                        let h = &self.ws.history[index];
                        let id = h.request.id.clone();
                        let reply = h.reply.clone();
                        if !self.ws.requests.iter().any(|r| r.id == id) {
                            self.ws.requests.push(h.request.clone());
                            self.tree.stale = true;
                        }
                        self.open(id.clone());
                        self.tabs.entry(id).or_default().set_reply(reply);
                        close = true;
                    }
                }
                Dialog::Cookies => {
                    ui.label("Session cookies for the current request URL");
                    ui.add_space(12.);
                    if let Some(r) = self.active() {
                        use reqwest::cookie::CookieStore;
                        if let Ok(url) =
                            url::Url::parse(&crate::model::resolve(&r.url, &self.vars(r)))
                        {
                            if let Some(cookies) = self.engine.cookies.cookies(&url) {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(cookies.to_str().unwrap_or(""))
                                            .monospace(),
                                    )
                                    .selectable(true)
                                    .wrap(),
                                );
                            } else {
                                ui.label(
                                    egui::RichText::new("No cookies for this URL").color(t.muted),
                                );
                            }
                        }
                    }
                    ui.add_space(18.);
                    ui.label(
                        egui::RichText::new(
                            "Cookies are shared by requests for this application session.",
                        )
                        .size(12.)
                        .color(t.muted),
                    );
                    if ui.button("Clear session cookies").clicked() {
                        self.engine.cookies.clear();
                        self.notice("Session cookies cleared");
                    }
                }
                Dialog::About => {
                    ui.label(
                        egui::RichText::new(concat!("Reqon ", env!("CARGO_PKG_VERSION"))).size(24.),
                    );
                    ui.add_space(12.);
                    ui.label("API client for HTTP, gRPC and WebSocket");
                    ui.hyperlink_to("reqon.ru", "https://reqon.ru/");
                    ui.add_space(12.);
                    if ui.button("Check for updates").clicked() {
                        self.open_updates();
                        close = true;
                    }
                }
                Dialog::ApiSpecs => {
                    ui.label("Import an OpenAPI 3 or Swagger 2 specification.");
                    ui.add_space(12.);
                    if ui.button("Import OpenAPI / Swagger").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("API specification", &["json", "yaml", "yml"])
                            .pick_file()
                        {
                            let target = self
                                .root
                                .join("collections")
                                .join(format!("OpenAPI-{}", &id()[..8]));
                            let tx = self.tx.clone();
                            let repaint = ctx.clone();
                            self.runtime.spawn_blocking(move || {
                                let result = (|| -> anyhow::Result<_> {
                                    let spec = crate::openapi::read(&path)?;
                                    let (c, requests) = crate::openapi::import(&spec, &target)?;
                                    Ok(crate::app::LoadedCollection::new((c, requests, vec![])))
                                })();
                                let _ = tx.send(crate::app::Event::Collection(
                                    result.map_err(|e| format!("Import failed: {e:#}")),
                                ));
                                repaint.request_repaint();
                            });
                            self.notice("Importing specification…");
                            close = true;
                        }
                    }
                    ui.add_space(12.);
                    ui.label(
                        egui::RichText::new(
                            "JSON and YAML · Parameters · Authentication · Request examples",
                        )
                        .size(12.)
                        .color(t.muted),
                    );
                }
                Dialog::MockServers => {
                    ui.label("Local mock server");
                    ui.add_space(8.);
                    ui.label("Serve the latest recorded responses from request history. Routes match the HTTP method and URL path.");
                    ui.add_space(12.);
                    ui.horizontal(|ui| {
                        ui.label("Port");
                        ui.add(egui::DragValue::new(&mut self.mock_port).range(1024..=65535));
                    });
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        if self.mock_task.is_none() {
                            if ui.button("Start server").clicked() {
                                self.start_mock();
                            }
                        } else {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Listening on 127.0.0.1:{}",
                                    self.mock_port
                                ))
                                .color(t.green),
                            );
                            if ui.button("Stop server").clicked() {
                                if let Some(task) = self.mock_task.take() {
                                    task.abort();
                                }
                                self.notice("Mock server stopped");
                            }
                        }
                    });
                    ui.add_space(12.);
                    ui.label(
                        egui::RichText::new(
                            "Only listens on localhost. Start again to pick up new responses.",
                        )
                        .size(11.)
                        .color(t.muted),
                    );
                }
                Dialog::CloseTab(id) => {
                    ui.label("Save this request before closing the tab?");
                    ui.add_space(18.);
                    ui.horizontal(|ui| {
                        if ui.button("Save and close").clicked() {
                            self.ws.active = Some(id.clone());
                            self.pending_close.insert(id.clone());
                            self.save_active();
                        }
                        if ui.button("Don't save").clicked() {
                            self.discard_and_close(id);
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            self.preference_ui.closing_tabs.clear();
                            close = true;
                        }
                    });
                }
                Dialog::None
                | Dialog::ExportCollections
                | Dialog::TreeRename(_)
                | Dialog::TreeNewFolder(_, _)
                | Dialog::TreeInfo(_)
                | Dialog::TreeDelete(_) => {}
            }
            if !matches!(self.dialog, Dialog::CloseTab(_))
                && !self.status.is_empty()
                && self.status_at.elapsed().as_secs() < 15
            {
                ui.add_space(14.);
                ui.separator();
                ui.add(
                    egui::Label::new(egui::RichText::new(&self.status).size(12.).color(t.accent))
                        .wrap(),
                );
            }
            ui.add_space(8.);
        });
        if modal_closed || close {
            if modal_closed {
                self.preference_ui.closing_tabs.clear();
            }
            if let Dialog::CloseTab(id) = &self.dialog {
                self.pending_close.remove(id);
            }
            self.dialog = Dialog::None;
        }
    }
}
