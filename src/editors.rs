use crate::{
    app::{Action, App},
    model::*,
    storage,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, TextEdit, Ui, pos2, vec2};

impl App {
    pub fn request_editor(&mut self, ui: &mut Ui, r: Rect, request: &mut Request) -> bool {
        let t = self.theme;
        let mut state = self.tabs.remove(&request.id).unwrap_or_default();
        let mut changed = false;
        let http_names = [
            "Params", "Body", "Headers", "Auth", "Vars", "Script", "Assert", "Tests", "Docs",
            "File", "Settings", "History",
        ];
        let names: Vec<(usize, &str)> = match request.protocol.as_str() {
            "grpc" => vec![
                (1, "Message"),
                (2, "Metadata"),
                (3, "Auth"),
                (5, "Script"),
                (8, "Docs"),
                (10, "Settings"),
                (11, "History"),
            ],
            "ws" => vec![
                (1, "Message"),
                (2, "Headers"),
                (3, "Auth"),
                (10, "Settings"),
                (8, "Docs"),
                (11, "History"),
            ],
            _ => http_names.into_iter().enumerate().collect(),
        };
        if matches!(request.protocol.as_str(), "grpc" | "ws") && state.request_tab == 0 {
            state.request_tab = 1;
        }
        let single_grpc_message = request.protocol == "grpc"
            && state.request_tab == 1
            && request.protocol_messages.len() <= 1;
        let mut head = w::child(
            ui,
            rect(
                r.left(),
                r.top(),
                r.width()
                    - if single_grpc_message {
                        64.
                    } else if state.request_tab == 1
                        && !matches!(request.protocol.as_str(), "grpc" | "ws")
                    {
                        crate::body_ui::toolbar_width(ui, request)
                    } else {
                        0.
                    },
                // 32 px tabs plus the 6 px space reserved for a scrollbar.
                38.,
            ),
            "request-editor-tabs",
        );
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(&mut head, |ui| {
                ui.spacing_mut().item_spacing = vec2(19.2, 0.);
                ui.horizontal(|ui| {
                    for &(i, name) in &names {
                        let count = match i {
                            0 => request
                                .params
                                .iter()
                                .filter(|p| p.enabled && !p.key.is_empty())
                                .count(),
                            2 => {
                                request
                                    .headers
                                    .iter()
                                    .filter(|p| p.enabled && !p.key.is_empty())
                                    .count()
                                    + if matches!(request.protocol.as_str(), "grpc" | "ws") {
                                        0
                                    } else {
                                        crate::generated_headers::enabled_count(
                                            request,
                                            &self.vars(request),
                                        )
                                    }
                            }
                            4 => request
                                .vars
                                .iter()
                                .chain(&request.response_vars)
                                .filter(|p| p.enabled && !p.key.is_empty())
                                .count(),
                            6 => request
                                .assertions
                                .iter()
                                .filter(|p| p.enabled && !p.key.is_empty())
                                .count(),
                            _ => 0,
                        };
                        let dot = match i {
                            1 => {
                                request.body_kind != BodyKind::None
                                    || matches!(request.protocol.as_str(), "grpc" | "ws")
                            }
                            3 => !matches!(request.auth.kind.as_str(), "" | "none" | "inherit"),
                            5 => !request.pre_script.is_empty() || !request.post_script.is_empty(),
                            7 => !request.tests.is_empty(),
                            8 => !request.docs.is_empty(),
                            _ => false,
                        };
                        let width = w::text_width(ui, name, 12.)
                            + if count > 0 {
                                w::text_width(ui, &count.to_string(), 10.) + 8.
                            } else if dot {
                                14.
                            } else {
                                0.
                            };
                        let row = ui.allocate_space(vec2(width, 32.)).1;
                        if w::tab(ui, row, name, state.request_tab == i, t).clicked() {
                            state.request_tab = i;
                        }
                        if count > 0 {
                            w::label(
                                ui,
                                pos2(
                                    row.right() - w::text_width(ui, &count.to_string(), 10.),
                                    row.top() + 11.,
                                ),
                                &count.to_string(),
                                10.,
                                t.muted,
                            );
                        } else if dot {
                            ui.painter().circle_filled(
                                pos2(row.right() - 3., row.top() + 10.5),
                                2.,
                                t.muted,
                            );
                        }
                    }
                });
            });
        if state.request_tab == 1 && !matches!(request.protocol.as_str(), "grpc" | "ws") {
            let (edited, error) = crate::body_ui::toolbar(ui, r, request, t);
            changed |= edited;
            if let Some(error) = error {
                self.notice(error);
            }
        }
        if single_grpc_message {
            changed |= self.grpc_message_toolbar(ui, r, request, &state);
        }
        let top = if self.ws.side_by_side && state.request_tab == 1 {
            54.
        } else {
            48.
        };
        let content = rect(r.left(), r.top() + top, r.width(), r.height() - top);
        let mut child = w::child(
            ui,
            content,
            (&request.id, "request-editor", state.request_tab),
        );
        match state.request_tab {
            0 => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(&mut child, |ui| {
                        ui.spacing_mut().item_spacing.y = 0.;
                        let heading = ui.allocate_space(vec2(ui.available_width(), 29.)).1;
                        w::label(
                            ui,
                            pos2(heading.left(), heading.top() + 8.),
                            "Query",
                            12.,
                            t.muted,
                        );
                        let params_changed =
                            w::pairs_with_bulk(ui, &mut request.params, ["Name", "Value"], t);
                        if params_changed {
                            crate::query::update_url(request);
                        }
                        changed |= params_changed;
                        let path_heading = ui.allocate_space(vec2(ui.available_width(), 29.)).1;
                        w::label(
                            ui,
                            pos2(path_heading.left(), path_heading.top() + 8.),
                            "Path",
                            12.,
                            t.muted,
                        );
                        w::icon(
                            ui,
                            pos2(path_heading.left() + 40., path_heading.top() + 7.),
                            Icon::Help,
                            t.muted,
                            14.,
                        );
                        if !request.path_params.is_empty() {
                            changed |= ui
                                .push_id("path-params", |ui| {
                                    w::pairs(ui, &mut request.path_params, ["Name", "Value"], t)
                                })
                                .inner;
                        }
                    });
            }
            1 if matches!(request.protocol.as_str(), "grpc" | "ws") => {
                changed |= self.protocol_message_editor(&mut child, request, &mut state);
            }
            1 => match request.body_kind {
                BodyKind::None => {
                    child.label("No Body");
                }
                BodyKind::Form | BodyKind::Multipart => {
                    if request.body_kind == BodyKind::Multipart {
                        child.label(
                            egui::RichText::new("Use @C:/path/to/file for file fields")
                                .size(11.)
                                .color(t.muted),
                        );
                        child.add_space(8.);
                    }
                    egui::ScrollArea::vertical().show(&mut child, |ui| {
                        changed |= w::pairs(ui, &mut request.form, ["Name", "Value"], t);
                    });
                }
                BodyKind::Binary => {
                    child.horizontal(|ui| {
                        changed |= ui
                            .add(
                                TextEdit::singleline(&mut request.body)
                                    .desired_width(420.)
                                    .hint_text("Path to binary file"),
                            )
                            .changed();
                        if ui.button("Choose file").clicked() {
                            if let Some(path) = rfd::FileDialog::new().pick_file() {
                                request.body = path.to_string_lossy().into_owned();
                                changed = true;
                            }
                        }
                    });
                }
                BodyKind::Graphql => {
                    let avail = child.available_rect_before_wrap();
                    let half = avail.width() * 0.6;
                    let mut query = w::child(
                        &mut child,
                        rect(avail.left(), avail.top(), half - 8., avail.height()),
                        "gql-query",
                    );
                    query.label("Query");
                    changed |= w::editor_with_placeholder(
                        &mut query,
                        &mut request.body,
                        false,
                        t,
                        "Enter request body",
                    );
                    let mut vars = w::child(
                        &mut child,
                        rect(
                            avail.left() + half,
                            avail.top(),
                            avail.width() - half,
                            avail.height(),
                        ),
                        "gql-variables",
                    );
                    vars.label("Variables (JSON)");
                    changed |= w::editor(&mut vars, &mut request.graphql_variables, false, t);
                }
                _ => {
                    changed |= w::editor_with_placeholder(
                        &mut child,
                        &mut request.body,
                        false,
                        t,
                        "Enter request body",
                    );
                }
            },
            2 => {
                let variables = self.vars(request);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(&mut child, |ui| {
                        if !matches!(request.protocol.as_str(), "grpc" | "ws") {
                            changed |= crate::generated_headers::draw(ui, request, &variables, t);
                        } else {
                            changed |=
                                w::pairs_with_bulk(ui, &mut request.headers, ["Name", "Value"], t);
                        }
                    });
            }
            3 => {
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.;
                    let before = request.auth.clone();
                    let auth_options = [
                        ("inherit", "Inherit"),
                        ("none", "No Auth"),
                        ("basic", "Basic Auth"),
                        ("bearer", "Bearer Token"),
                        ("apikey", "API Key"),
                        ("oauth2", "OAuth 2.0"),
                        ("oauth1", "OAuth 1.0"),
                        ("digest", "Digest Auth"),
                        ("awsv4", "AWS Sig v4"),
                        ("wsse", "WSSE Auth"),
                        ("edgegrid", "Akamai EdgeGrid"),
                        ("ntlm", "NTLM Auth"),
                    ];
                    let label = auth_options
                        .iter()
                        .find(|(value, _)| *value == request.auth.kind)
                        .map(|(_, label)| *label)
                        .unwrap_or("No Auth");
                    let select_rect = ui
                        .allocate_space(vec2(w::text_width(ui, label, 12.) + 32., 26.))
                        .1;
                    let select = w::dropdown(ui, select_rect, label, true, true, t);
                    egui::Popup::menu(&select).show(|ui| {
                        for (value, label) in auth_options {
                            if ui
                                .selectable_label(request.auth.kind == value, label)
                                .clicked()
                            {
                                request.auth.kind = value.into();
                                ui.close();
                            }
                        }
                    });
                    ui.add_space(16.);
                    match request.auth.kind.as_str() {
                        "bearer" => {
                            auth_field(ui, "Token", &mut request.auth.token, true, t);
                        }
                        "basic" | "digest" | "wsse" | "ntlm" => {
                            auth_field(ui, "Username", &mut request.auth.username, false, t);
                            auth_field(ui, "Password", &mut request.auth.password, true, t);
                            if request.auth.kind == "ntlm" {
                                for (key, label) in
                                    [("domain", "Domain"), ("workstation", "Workstation")]
                                {
                                    auth_field(
                                        ui,
                                        label,
                                        request.auth.fields.entry(key.into()).or_default(),
                                        false,
                                        t,
                                    );
                                }
                            }
                        }
                        "apikey" => {
                            auth_field(ui, "Key", &mut request.auth.key, false, t);
                            auth_field(ui, "Value", &mut request.auth.value, true, t);
                            ui.checkbox(&mut request.auth.in_query, "Add to query parameters");
                        }
                        "oauth2" => {
                            let grant = request
                                .auth
                                .fields
                                .entry("grantType".into())
                                .or_insert("client_credentials".into());
                            egui::ComboBox::from_id_salt("oauth2-grant")
                                .selected_text(match grant.as_str() {
                                    "password" => "Password Credentials",
                                    "authorization_code" => "Authorization Code",
                                    "refresh_token" => "Refresh Token",
                                    _ => "Client Credentials",
                                })
                                .show_ui(ui, |ui| {
                                    for (value, label) in [
                                        ("client_credentials", "Client Credentials"),
                                        ("password", "Password Credentials"),
                                        ("authorization_code", "Authorization Code"),
                                        ("refresh_token", "Refresh Token"),
                                    ] {
                                        ui.selectable_value(grant, value.into(), label);
                                    }
                                });
                            ui.add_space(16.);
                            for (key, label, secret) in [
                                ("accessTokenUrl", "Access Token URL", false),
                                ("clientId", "Client ID", false),
                                ("clientSecret", "Client Secret", true),
                                ("scope", "Scope", false),
                            ] {
                                auth_field(
                                    ui,
                                    label,
                                    request.auth.fields.entry(key.into()).or_default(),
                                    secret,
                                    t,
                                );
                            }
                            match request
                                .auth
                                .fields
                                .get("grantType")
                                .map(String::as_str)
                                .unwrap_or("")
                            {
                                "password" => {
                                    auth_field(
                                        ui,
                                        "Username",
                                        &mut request.auth.username,
                                        false,
                                        t,
                                    );
                                    auth_field(ui, "Password", &mut request.auth.password, true, t);
                                }
                                "refresh_token" => {
                                    auth_field(
                                        ui,
                                        "Refresh Token",
                                        request
                                            .auth
                                            .fields
                                            .entry("refreshToken".into())
                                            .or_default(),
                                        true,
                                        t,
                                    );
                                }
                                "authorization_code" => {
                                    let mut pkce = request
                                        .auth
                                        .fields
                                        .get("pkce")
                                        .is_some_and(|s| s == "true");
                                    if w::checkbox(ui, &mut pkce, "Use PKCE", t) {
                                        request.auth.fields.insert("pkce".into(), pkce.to_string());
                                    }
                                    ui.add_space(12.);
                                    for (key, label, secret) in [
                                        ("authorizeUrl", "Authorization URL", false),
                                        ("callbackUrl", "Callback URL", false),
                                        ("code", "Authorization Code", true),
                                        ("codeVerifier", "PKCE Code Verifier", true),
                                    ] {
                                        auth_field(
                                            ui,
                                            label,
                                            request.auth.fields.entry(key.into()).or_default(),
                                            secret,
                                            t,
                                        );
                                    }
                                }
                                _ => {}
                            }
                            let placement = request
                                .auth
                                .fields
                                .entry("credentialsPlacement".into())
                                .or_insert("header".into());
                            egui::ComboBox::from_id_salt("oauth2-credentials")
                                .selected_text(if placement == "body" {
                                    "Credentials in Body"
                                } else {
                                    "Credentials in Header"
                                })
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        placement,
                                        "header".into(),
                                        "Credentials in Header",
                                    );
                                    ui.selectable_value(
                                        placement,
                                        "body".into(),
                                        "Credentials in Body",
                                    );
                                });
                            ui.add_space(16.);
                            auth_field(
                                ui,
                                "Access Token",
                                request.auth.fields.entry("accessToken".into()).or_default(),
                                true,
                                t,
                            );
                        }
                        "edgegrid" => {
                            for (key, label, secret) in [
                                ("accessToken", "Access Token", true),
                                ("clientToken", "Client Token", true),
                                ("clientSecret", "Client Secret", true),
                                ("baseURL", "Base URL", false),
                                ("headersToSign", "Headers to Sign", false),
                                ("maxBodySize", "Maximum Body Size", false),
                            ] {
                                auth_field(
                                    ui,
                                    label,
                                    request.auth.fields.entry(key.into()).or_default(),
                                    secret,
                                    t,
                                );
                            }
                        }
                        "awsv4" => {
                            for (key, label, secret) in [
                                ("accessKeyId", "Access Key ID", false),
                                ("secretAccessKey", "Secret Access Key", true),
                                ("sessionToken", "Session Token", true),
                                ("region", "Region", false),
                                ("service", "Service", false),
                            ] {
                                auth_field(
                                    ui,
                                    label,
                                    request.auth.fields.entry(key.into()).or_default(),
                                    secret,
                                    t,
                                );
                            }
                        }
                        "oauth1" => {
                            for (key, label, secret) in [
                                ("consumerKey", "Consumer Key", false),
                                ("consumerSecret", "Consumer Secret", true),
                                ("accessToken", "Token", false),
                                ("accessTokenSecret", "Token Secret", true),
                            ] {
                                auth_field(
                                    ui,
                                    label,
                                    request.auth.fields.entry(key.into()).or_default(),
                                    secret,
                                    t,
                                );
                            }
                            let signature = request
                                .auth
                                .fields
                                .entry("signatureMethod".into())
                                .or_insert("HMAC-SHA1".into());
                            egui::ComboBox::from_id_salt("oauth1-signature")
                                .selected_text(&*signature)
                                .show_ui(ui, |ui| {
                                    for value in ["HMAC-SHA1", "HMAC-SHA256", "PLAINTEXT"] {
                                        ui.selectable_value(signature, value.into(), value);
                                    }
                                });
                        }
                        _ => {
                            ui.label(egui::RichText::new(if request.auth.kind == "inherit" {
                                "Inherit authorization from the parent collection or folder"
                            } else {
                                "No Auth"
                            }));
                        }
                    }
                    changed |= before != request.auth;
                });
            }
            4 => {
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    let heading = ui.allocate_space(vec2(ui.available_width(), 29.)).1;
                    w::label(
                        ui,
                        pos2(heading.left(), heading.top() + 8.),
                        "Pre Request",
                        12.,
                        t.muted,
                    );
                    changed |= w::pairs(ui, &mut request.vars, ["Name", "Value"], t);
                    ui.add_space(12.);
                    let heading = ui.allocate_space(vec2(ui.available_width(), 29.)).1;
                    w::label(
                        ui,
                        pos2(heading.left(), heading.top() + 8.),
                        "Post Response",
                        12.,
                        t.muted,
                    );
                    changed |= w::pairs(ui, &mut request.response_vars, ["Name", "Expr"], t);
                });
            }
            5 => {
                w::segments(
                    &mut child,
                    &mut state.post_script,
                    ["Pre Request", "Post Response"],
                    t,
                );
                child.add_space(8.);
                changed |= crate::ai_ui::script_editor(
                    &mut child,
                    request,
                    usize::from(state.post_script),
                    &self.ai_preferences(request),
                    &mut state.ai_completion,
                    t,
                );
            }
            6 => {
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    changed |= w::assertions(ui, &mut request.assertions, t);
                });
            }
            7 => {
                changed |= crate::ai_ui::script_editor(
                    &mut child,
                    request,
                    2,
                    &self.ai_preferences(request),
                    &mut state.ai_completion,
                    t,
                );
            }
            8 => {
                child.horizontal(|ui| {
                    ui.label(egui::RichText::new("Documentation").size(13.).color(t.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button(if state.docs_edit { "Preview" } else { "Edit" })
                            .clicked()
                        {
                            state.docs_edit = !state.docs_edit;
                        }
                    });
                });
                child.add_space(12.);
                if state.docs_edit {
                    changed |= w::editor(&mut child, &mut request.docs, false, t);
                } else {
                    egui::ScrollArea::vertical().show(&mut child, |ui| {
                        crate::markdown::render(ui, &request.docs, t)
                    });
                }
            }
            9 => {
                child.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(
                            request
                                .path
                                .as_ref()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_else(|| "Unsaved request".into()),
                        )
                        .size(11.)
                        .color(t.muted),
                    );
                });
                child.add_space(10.);
                let mut source = storage::source(&crate::auth_vault::preview(request))
                    .unwrap_or_else(|e| e.to_string());
                w::editor(&mut child, &mut source, true, t);
            }
            10 => {
                changed |= crate::request_settings::draw(&mut child, request, t);
            }
            11 => {
                egui::ScrollArea::vertical().show(&mut child, |ui| {
                    let entries: Vec<_> = self
                        .ws
                        .history
                        .iter()
                        .rev()
                        .filter(|h| h.request.id == request.id)
                        .collect();
                    if entries.is_empty() {
                        ui.label(
                            egui::RichText::new("No history for this request yet").color(t.muted),
                        );
                    }
                    for history in entries {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    history.reply.status, history.reply.reason
                                ))
                                .color(
                                    if history.reply.status < 400 {
                                        t.green
                                    } else {
                                        t.red
                                    },
                                ),
                            );
                            ui.label(format!(
                                "{} ms · {} bytes",
                                history.reply.elapsed_ms, history.reply.size
                            ));
                            if ui.small_button("View response").clicked() {
                                state.set_reply(history.reply.clone());
                            }
                        });
                        ui.label(
                            egui::RichText::new(&history.request.url)
                                .size(11.)
                                .color(t.muted),
                        );
                        ui.separator();
                    }
                });
            }
            _ => {}
        }
        self.tabs.insert(request.id.clone(), state);
        changed
    }
    pub fn response_pane(&mut self, ui: &mut Ui, r: Rect, request: &Request) {
        if request.protocol == "ws" {
            self.socket_response(ui, r, request);
            return;
        }
        let t = self.theme;
        let mut state = self.tabs.remove(&request.id).unwrap_or_default();
        let reply = state.reply.take();
        if reply.is_none() && !self.running.contains_key(&request.id) {
            let mut placeholder = w::child(ui, r, ("response-placeholder", &request.id));
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(&mut placeholder, |ui| {
                    let origin = ui.cursor().min;
                    let icon_size = if self.ws.side_by_side { 150. } else { 80. };
                    let total = icon_size + if self.ws.side_by_side { 16. } else { 8. } + 106.5;
                    let y = if self.ws.side_by_side {
                        origin.y + r.width() * 0.20
                    } else {
                        origin.y + (r.height() - total) * 0.5 + 4.
                    };
                    ui.allocate_space(vec2(
                        ui.available_width(),
                        (y - origin.y + total + 16.).max(r.height()),
                    ));
                    w::icon(
                        ui,
                        pos2(r.center().x, y + icon_size * 0.5),
                        Icon::Send,
                        crate::theme::hex(if t.dark { 0x525252 } else { 0xd6d6d6 }),
                        icon_size,
                    );
                    for (i, (label, key)) in [
                        ("Send Request", "Ctrl + Enter"),
                        ("New Request", "Ctrl + N"),
                        ("Edit Environments", "Ctrl + E"),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let cy = y
                            + icon_size
                            + if self.ws.side_by_side { 16. } else { 8. }
                            + 17.75
                            + i as f32 * 35.5;
                        let width = w::text_width(ui, label, 13.);
                        w::label(ui, pos2(r.center().x - 8. - width, cy), label, 13., t.text);
                        w::label(ui, pos2(r.center().x + 8., cy), key, 13., t.text);
                    }
                });
            self.tabs.insert(request.id.clone(), state);
            return;
        }
        let mut clear_response = false;
        let headers = reply.as_ref().map(|r| r.headers.len()).unwrap_or(0);
        let grpc = request.protocol == "grpc";
        let names: Vec<(usize, String)> = if grpc {
            vec![
                (0, "Response".into()),
                (1, "Metadata".into()),
                (5, "Trailers".into()),
                (2, "Timeline".into()),
                (3, "Tests".into()),
            ]
        } else {
            vec![
                (0, "Response".to_owned()),
                (
                    1,
                    if headers > 0 {
                        format!("Headers {headers}")
                    } else {
                        "Headers".into()
                    },
                ),
                (2, "Timeline".into()),
                (3, "Tests".into()),
            ]
        };
        let mut x = r.left();
        let tab_top = r.top() + if self.ws.side_by_side { 0. } else { 7. };
        let tab_height = if self.ws.side_by_side { 32. } else { 34. };
        let toolbar_y = r.top() + if self.ws.side_by_side { 15. } else { 23. };
        let label_y = r.top() + if self.ws.side_by_side { 15. } else { 24. };
        for (i, name) in &names {
            let width = w::text_width(ui, name, 12.);
            if x + width > r.right() - 185. && *i > 1 {
                break;
            }
            if w::tab(
                ui,
                rect(x, tab_top, width, tab_height),
                name,
                state.response_tab == *i,
                t,
            )
            .clicked()
            {
                state.response_tab = *i;
            }
            x += width + 20.;
        }
        let mut right = r.right();
        if w::response_layout_button(
            ui,
            rect(right - 25., toolbar_y - 13., 24., 26.),
            self.ws.side_by_side,
            t,
            "response-pane-layout",
        )
        .clicked()
        {
            self.actions.push(Action::ToggleSplit);
        }
        right -= 30.;
        if let Some(reply) = &reply {
            if w::icon_button(
                ui,
                rect(right - 25., toolbar_y - 13., 24., 26.),
                Icon::Trash,
                "Clear response",
                t,
            )
            .clicked()
            {
                clear_response = true;
                state.pretty.clear();
            }
            right -= 28.;
            if w::icon_button(
                ui,
                rect(right - 25., toolbar_y - 13., 24., 26.),
                Icon::Download,
                "Save response to file",
                t,
            )
            .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name(
                        if serde_json::from_str::<serde_json::Value>(&reply.body).is_ok() {
                            "response.json"
                        } else {
                            "response.txt"
                        },
                    )
                    .save_file()
                {
                    let data = if reply.bytes.is_empty() {
                        reply.body.as_bytes()
                    } else {
                        &reply.bytes
                    };
                    if let Err(e) = storage::atomic_write(&path, data) {
                        self.notice(format!("Save failed: {e}"));
                    } else {
                        self.notice("Response saved");
                    }
                }
            }
            right -= 28.;
            if w::icon_button(
                ui,
                rect(right - 25., toolbar_y - 13., 24., 26.),
                Icon::Copy,
                "Copy response",
                t,
            )
            .clicked()
            {
                ui.ctx().copy_text(if state.raw {
                    reply.body.clone()
                } else {
                    state.pretty.clone()
                });
                self.notice("Response copied");
            }
            right -= 34.;
            let status = format!("{} {}", reply.status, reply.reason);
            let size = if reply.size < 1024 {
                format!("{}B", reply.size)
            } else {
                format!("{:.2}KB", reply.size as f64 / 1024.)
            };
            let metrics = format!("{}ms  {size}", reply.elapsed_ms);
            let metric_width = w::text_width(ui, &metrics, 11.);
            let status_width = w::text_width(ui, &status, 12.);
            if right - metric_width - status_width - 12. > x {
                w::label(
                    ui,
                    pos2(right - metric_width, label_y),
                    &metrics,
                    11.,
                    t.text,
                );
                right -= metric_width + 10.;
                w::label(
                    ui,
                    pos2(right - status_width, label_y),
                    &status,
                    12.,
                    if (grpc && reply.status == 0 && reply.error.is_empty())
                        || (!grpc && reply.status < 400 && reply.status > 0)
                    {
                        t.green
                    } else {
                        t.red
                    },
                );
                right -= status_width + 15.;
            }
            if right - 92. > x {
                let toggle = rect(right - 90., toolbar_y - 11.5, 88., 23.);
                let image = state.preview_image.is_some() || state.preview_texture.is_some();
                let content_type = reply
                    .headers
                    .iter()
                    .find(|(key, _)| key.eq_ignore_ascii_case("content-type"))
                    .map(|(_, value)| value.as_str())
                    .unwrap_or("");
                let format = if image {
                    "Image"
                } else if content_type.contains("json")
                    || state.pretty.trim_start().starts_with(['{', '['])
                {
                    "{ } JSON"
                } else if content_type.contains("xml") {
                    "XML"
                } else if content_type.contains("html") {
                    "HTML"
                } else {
                    "Text"
                };
                let response = w::button(
                    ui,
                    toggle,
                    &format!("{}     ", if state.raw { "Raw" } else { format }),
                    false,
                    t,
                );
                egui::Popup::menu(&response).show(|ui| {
                    if ui.selectable_label(!state.raw, format).clicked() {
                        state.raw = false;
                        ui.close();
                    }
                    if ui.selectable_label(state.raw, "Raw").clicked() {
                        state.raw = true;
                        ui.close();
                    }
                });
                w::icon(
                    ui,
                    pos2(toggle.right() - 10., toggle.center().y),
                    Icon::Down,
                    t.muted,
                    12.,
                );
            }
        }
        let content = rect(r.left(), r.top() + 54., r.width(), r.height() - 58.);
        let mut child = w::child(ui, content, (&request.id, "response", state.response_tab));
        if self.running.contains_key(&request.id) {
            child.vertical_centered(|ui| {
                ui.add_space((content.height() * 0.3).max(8.));
                ui.spinner();
                ui.add_space(12.);
                ui.label("Sending request...");
            });
        } else if let Some(reply) = &reply {
            if !reply.error.is_empty() && state.response_tab == 0 {
                child.add_space(16.);
                child.label(egui::RichText::new("Request failed").size(16.).color(t.red));
                child.add_space(12.);
                child.add(egui::Label::new(&reply.error).wrap());
            } else {
                match state.response_tab {
                    0 => {
                        if !state.raw
                            && (state.preview_image.is_some() || state.preview_texture.is_some())
                        {
                            if let Some(image) = state.preview_image.take() {
                                state.preview_texture = Some(ui.ctx().load_texture(
                                    format!("response-{}", request.id),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                ));
                            }
                            if let Some(texture) = &state.preview_texture {
                                egui::ScrollArea::both().show(&mut child, |ui| {
                                    ui.add(
                                        egui::Image::from_texture(texture)
                                            .fit_to_original_size(1.)
                                            .max_width(ui.available_width()),
                                    );
                                });
                            }
                        } else {
                            w::viewer(
                                &mut child,
                                if state.raw {
                                    &reply.body
                                } else {
                                    &state.pretty
                                },
                                if state.raw {
                                    &state.raw_lines
                                } else {
                                    &state.pretty_lines
                                },
                                t,
                            );
                        }
                    }
                    1 | 5 => {
                        egui::ScrollArea::both().show(&mut child, |ui| {
                            egui::Grid::new("response_headers")
                                .num_columns(2)
                                .striped(true)
                                .min_col_width(160.)
                                .spacing(vec2(20., 12.))
                                .show(ui, |ui| {
                                    for (k, v) in if state.response_tab == 5 {
                                        &reply.trailers
                                    } else {
                                        &reply.headers
                                    } {
                                        ui.label(egui::RichText::new(k).color(t.blue));
                                        ui.add(egui::Label::new(v).selectable(true));
                                        ui.end_row();
                                    }
                                });
                        });
                    }
                    2 => {
                        egui::ScrollArea::vertical().show(&mut child, |ui| {
                            ui.label(egui::RichText::new("Network timeline").size(13.));
                            ui.add_space(16.);
                            for line in &reply.timeline {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("›").color(t.accent));
                                    ui.label(egui::RichText::new(line).monospace().size(12.));
                                });
                                ui.add_space(8.);
                            }
                        });
                    }
                    3 => {
                        egui::ScrollArea::vertical().show(&mut child,|ui|{if reply.tests.is_empty(){ui.label(egui::RichText::new("No tests were run. Add assertions or a test script to this request.").color(t.muted));}else{let passed=reply.tests.iter().filter(|t|t.passed).count();ui.label(format!("{passed} / {} tests passed",reply.tests.len()));ui.add_space(14.);for test in &reply.tests{ui.horizontal(|ui|{ui.label(egui::RichText::new(if test.passed{"PASS"}else{"FAIL"}).size(11.).color(if test.passed{t.green}else{t.red}));ui.label(&test.name);});if !test.error.is_empty(){ui.label(egui::RichText::new(&test.error).color(t.red).size(12.));}ui.add_space(8.);}}});
                    }
                    4 => {
                        egui::ScrollArea::vertical().show(&mut child, |ui| {
                            if reply.logs.is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "No console output. Use console.log() in a script.",
                                    )
                                    .color(t.muted),
                                );
                            }
                            for log in &reply.logs {
                                ui.add(
                                    egui::Label::new(egui::RichText::new(log).monospace())
                                        .selectable(true),
                                );
                            }
                        });
                    }
                    _ => {}
                }
            }
        } else {
            child.vertical_centered(|ui| {
                ui.add_space((content.height() * 0.35).max(8.));
                ui.label(
                    egui::RichText::new("Send a request to get a response")
                        .size(13.)
                        .color(t.muted),
                );
                ui.add_space(8.);
                ui.label(egui::RichText::new("Ctrl + Enter").size(12.).color(t.faint));
            });
        }
        if !clear_response {
            state.reply = reply;
        }
        self.tabs.insert(request.id.clone(), state);
    }
}
pub fn auth_field(
    ui: &mut Ui,
    label: &str,
    value: &mut String,
    password: bool,
    t: crate::theme::Theme,
) -> bool {
    let heading = ui
        .allocate_space(vec2(400_f32.min(ui.available_width()), 18.))
        .1;
    w::label(
        ui,
        pos2(heading.left(), heading.center().y),
        label,
        12.,
        t.muted,
    );
    ui.add_space(4.);
    let reveal_id = ui.scope_id().with(("reveal", label));
    let mut reveal = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(reveal_id))
        .unwrap_or(false);
    let response = ui.add_sized(
        vec2(400_f32.min(ui.available_width()), 29.),
        TextEdit::singleline(value)
            .password(password && !reveal)
            .margin(egui::Margin {
                left: 8,
                right: if password { 30 } else { 8 },
                top: 5,
                bottom: 5,
            }),
    );
    let changed = response.changed();
    if password {
        let button = rect(
            response.rect.right() - 28.,
            response.rect.top() + 1.,
            26.,
            27.,
        );
        if w::icon_button(
            ui,
            button,
            Icon::Eye,
            if reveal { "Hide value" } else { "Show value" },
            t,
        )
        .clicked()
        {
            reveal = !reveal;
            ui.ctx().data_mut(|d| d.insert_temp(reveal_id, reveal));
        }
    }
    ui.add_space(12.);
    changed
}
pub fn field(ui: &mut Ui, label: &str, value: &mut String, password: bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(vec2(112., 26.), egui::Label::new(label));
        changed = ui
            .add_sized(
                vec2(ui.available_width().min(430.), 28.),
                TextEdit::singleline(value)
                    .password(password)
                    .hint_text(if password {
                        "Value or {{variable}}"
                    } else {
                        "Value"
                    }),
            )
            .changed();
    });
    changed
}
