use crate::{
    app::{Action, App, Event, TabState},
    grpc::MethodInfo,
    model::{Request, resolve},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, FontId, Rect, Sense, Stroke, StrokeKind, TextEdit, Ui, pos2, vec2};

#[derive(Default)]
pub struct DiscoveryState {
    key: String,
    generation: u64,
    task: Option<crate::cancellation::Task>,
    error: String,
    search: String,
    input: String,
    displayed_method: String,
    show_error: bool,
    highlighted: usize,
    resize_popup: bool,
    navigation: (bool, bool, bool),
    popup_was_open: bool,
    pending_example: Option<(String, String)>,
}
impl Drop for DiscoveryState {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

fn method_label(name: &str) -> String {
    name.rsplit_once('/').map_or_else(
        || name.into(),
        |(service, method)| {
            format!(
                "{} / {method}",
                service.rsplit('.').next().unwrap_or(service)
            )
        },
    )
}
fn matches_search(method: &MethodInfo, search: &str) -> bool {
    let name = method.name.to_lowercase();
    search
        .split_whitespace()
        .all(|term| name.contains(&term.to_lowercase()))
}
fn rpc_icon(ui: &Ui, center: egui::Pos2, t: Theme) {
    egui::Image::from_bytes(
        "bytes://icons/GrpcMethod.svg",
        include_bytes!("../assets/icons/GrpcMethod.svg") as &[u8],
    )
    .tint(t.blue)
    .paint_at(ui, Rect::from_center_size(center, vec2(16., 16.)));
}
fn select_method(request: &mut Request, method: &MethodInfo) {
    request.grpc_method = method.name.clone();
    if request.body.trim().is_empty() || request.body.trim() == "{}" {
        request.body = if method.client_streaming {
            format!("[{}]", method.example)
        } else {
            method.example.clone()
        };
    }
}

impl App {
    pub(crate) fn generate_grpc_example(&mut self, id: &str, ctx: &egui::Context) {
        let Some(request) = self
            .ws
            .requests
            .iter()
            .find(|r| r.id == id && r.protocol == "grpc")
            .cloned()
        else {
            return;
        };
        let method_name = resolve(&request.grpc_method, &self.vars(&request));
        let method_name = method_name.trim_start_matches('/');
        if method_name.is_empty() {
            self.notice("Choose a gRPC method before generating an example message.");
            return;
        }
        let key = self.grpc_discovery_key(&request);
        let state = self.tabs.entry(id.to_owned()).or_default();
        if state.grpc_discovery.key == key {
            if let Some(method) = state
                .grpc_methods
                .iter()
                .find(|m| m.name == method_name)
                .cloned()
            {
                self.apply_grpc_example(id, &method);
                return;
            }
        }
        if request.url.trim().is_empty() && request.proto_files.is_empty() {
            self.notice(
                "Enter a server URL or import a .proto file to generate an example message.",
            );
            return;
        }
        if !state.grpc_loading || state.grpc_discovery.key != key {
            self.discover_grpc(&request, ctx);
        }
        self.tabs
            .get_mut(id)
            .unwrap()
            .grpc_discovery
            .pending_example = Some((request.grpc_method, request.body));
        self.notice("Loading service definitions to generate an example message...");
    }

    fn apply_grpc_example(&mut self, id: &str, method: &MethodInfo) {
        let Some(request) = self.ws.requests.iter_mut().find(|r| r.id == id) else {
            return;
        };
        request.body = if method.client_streaming {
            format!("[{}]", method.example)
        } else {
            method.example.clone()
        };
        if request.protocol_messages.len() == 1 {
            request.protocol_messages[0].value = request.body.clone();
        }
        self.dirty_cache.borrow_mut().remove(id);
        self.modified_requests.insert(id.to_owned());
        self.mark_workspace_changed();
        self.notice("Example message generated.");
    }

    fn grpc_discovery_key(&self, request: &Request) -> String {
        let data = serde_json::to_string(&(
            &request.url,
            &request.proto_files,
            &request.proto_import_paths,
            &request.headers,
            &request.auth,
            self.vars(request),
            &self.ws.preferences,
            &self.ws.preferences.credentials,
            &request.path,
            &request.folder,
            (
                request.timeout,
                request.timeout_inherit,
                request.verify_tls,
                request.grpc_tls,
                &request.proxy,
                &request.client_certificate,
                &request.client_key,
                &request.ca_certificate,
            ),
        ))
        .unwrap_or_default();
        crate::disk_sync::fingerprint(&data)
    }

    fn invalidate_grpc_discovery(&mut self, id: &str) {
        if let Some(state) = self.tabs.get_mut(id) {
            state.grpc_loading = false;
            state.grpc_methods.clear();
            let old = &mut state.grpc_discovery;
            let mut next = DiscoveryState::default();
            next.input = std::mem::take(&mut old.input);
            next.displayed_method = std::mem::take(&mut old.displayed_method);
            next.search = std::mem::take(&mut old.search);
            next.popup_was_open = old.popup_was_open;
            state.grpc_discovery = next;
        }
    }

    pub fn discover_grpc(&mut self, request: &Request, ctx: &egui::Context) {
        self.invalidate_grpc_discovery(&request.id);
        if request.url.trim().is_empty() && request.proto_files.is_empty() {
            return;
        }
        let key = self.grpc_discovery_key(request);
        let r = request.clone();
        let preferences = self.ws.preferences.clone();
        let inputs = self.protocol_inputs(request);
        let tx = self.tx.clone();
        let context = ctx.clone();
        let id = request.id.clone();
        self.sequence += 1;
        let generation = self.sequence;
        let token = crate::cancellation::CancellationToken::new();
        let prepare_cancel = token.clone();
        let task = self.runtime.spawn(async move {
            let result = async {
                let network_preferences=preferences.clone();
                let (mut r, vars) = tokio::task::spawn_blocking(move || {
                    crate::protocol_prepare::prepare_protocol_cancellable(r, preferences, inputs, &prepare_cancel)
                }).await.map_err(|e| format!("Cannot prepare gRPC connection: {e}"))?
                    .map_err(|e| format!("Cannot prepare gRPC connection: {e:#}"))?;
                if r.proto_files.is_empty() && resolve(&r.url, &vars).contains("{{") {
                    return Err("The server URL contains an unresolved variable. Select an environment or enter a host and port.".into());
                }
                // Discovery must terminate even when calls have an unlimited timeout.
                r.timeout = if r.timeout == 0 { 30_000 } else { r.timeout.min(30_000) };
                crate::preference_network::prepare(&network_preferences,&mut r,&vars).await.map_err(|e|format!("{e:#}"))?;
                crate::grpc::discover(&r, &vars).await.map_err(|e| format!("{e:#}"))
            };
            let result = tokio::time::timeout(std::time::Duration::from_secs(30), result)
                .await.unwrap_or_else(|_| Err("Loading service definitions timed out after 30 seconds.".into()));
            let _ = tx.send(Event::GrpcMethods(id, generation, result));
            context.request_repaint();
        });
        let state = self.tabs.entry(request.id.clone()).or_default();
        state.grpc_loading = true;
        state.grpc_discovery.key = key;
        state.grpc_discovery.generation = generation;
        state.grpc_discovery.task =
            Some(crate::cancellation::Task::new(task.abort_handle(), token));
        ctx.request_repaint();
    }

    pub fn receive_grpc_methods(
        &mut self,
        id: &str,
        generation: u64,
        result: Result<Vec<MethodInfo>, String>,
    ) {
        let Some(request) = self
            .ws
            .requests
            .iter()
            .find(|r| r.id == id && r.protocol == "grpc")
        else {
            return;
        };
        let key = self.grpc_discovery_key(request);
        let method_name = resolve(&request.grpc_method, &self.vars(request));
        let draft = (request.grpc_method.clone(), request.body.clone());
        let Some(state) = self.tabs.get_mut(id) else {
            return;
        };
        if !state.grpc_loading || state.grpc_discovery.generation != generation {
            return;
        }
        if state.grpc_discovery.key != key {
            self.invalidate_grpc_discovery(id);
            return;
        }
        state.grpc_loading = false;
        state.grpc_discovery.task = None;
        state.grpc_discovery.resize_popup = true;
        match result {
            Ok(mut methods) => {
                methods.sort_by(|a, b| a.name.cmp(&b.name));
                state.grpc_methods = methods;
                state.grpc_discovery.error.clear();
            }
            Err(error) => {
                state.grpc_methods.clear();
                state.grpc_discovery.error = error;
            }
        }
        if let Some(pending) = state.grpc_discovery.pending_example.take() {
            // Discovery is asynchronous: never overwrite edits made while waiting.
            if pending != draft {
                return;
            }
            let method = state
                .grpc_methods
                .iter()
                .find(|m| m.name == method_name.trim_start_matches('/'))
                .cloned();
            let error = state.grpc_discovery.error.clone();
            if let Some(method) = method {
                self.apply_grpc_example(id, &method);
            } else if !error.is_empty() {
                self.notice(format!("Cannot generate example message: {error}"));
            } else {
                self.notice(format!("Cannot generate example message: method {method_name} was not found in the service definitions."));
            }
        }
    }

    pub fn grpc_request_bar(&mut self, ui: &mut Ui, bar: Rect, request: &mut Request) -> bool {
        let t = self.theme;
        let fields_width = bar.width() - 150.;
        let host_width = (fields_width * 0.38).max(150.);
        let host = rect(bar.left(), bar.top(), host_width, bar.height());
        let method = rect(
            host.right() + 8.,
            bar.top(),
            fields_width - host_width - 8.,
            bar.height(),
        );
        ui.painter()
            .rect_stroke(host, 6, Stroke::new(1., t.border), StrokeKind::Inside);
        let mut changed = self.grpc_tls_button(ui, host, request);
        let variables = self.vars(request);
        let host_response = crate::url_variables::editor(
            ui,
            rect(
                host.left() + 38.,
                host.top() + 1.,
                host.width() - 49.,
                host.height() - 2.,
            ),
            ui.scope_id().with(("grpc-host", &request.id)),
            &mut request.url,
            &variables,
            "Enter server URL",
            t,
        );
        if host_response.changed() {
            request.sync_grpc_tls_from_url();
        }
        changed |= host_response.changed();
        if changed {
            self.invalidate_grpc_discovery(&request.id);
        }
        let popup_id = ui.scope_id().with(("grpc-method-popup", &request.id));
        let field_id = ui.scope_id().with(("grpc-method-input", &request.id));
        let was_open = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let navigation = if was_open {
            ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                )
            })
        } else {
            (false, false, false)
        };
        let discovery = &mut self
            .tabs
            .entry(request.id.clone())
            .or_default()
            .grpc_discovery;
        discovery.navigation = navigation;
        if discovery.displayed_method != request.grpc_method
            || (discovery.popup_was_open && !was_open)
            || (!was_open && !ui.memory(|m| m.has_focus(field_id)))
        {
            discovery.input = method_label(&request.grpc_method);
            discovery.displayed_method = request.grpc_method.clone();
            discovery.search.clear();
        }
        let response = ui.put(
            rect(
                method.left() + 30.,
                method.top() + 1.,
                method.width() - 55.,
                method.height() - 2.,
            ),
            TextEdit::singleline(&mut discovery.input)
                .id(field_id)
                .frame(egui::Frame::NONE)
                .font(FontId::proportional(13.))
                .hint_text("Select a method")
                .margin(egui::Margin::ZERO)
                .min_size(vec2(0., method.height() - 2.))
                .align(egui::Align2::LEFT_CENTER),
        );
        let edited = response.changed();
        if edited {
            discovery.search = discovery.input.clone();
            discovery.highlighted = 0;
        }
        response.clone().context_menu(|ui| {
            if ui.button("Copy service/method").clicked() {
                ui.ctx().copy_text(request.grpc_method.clone());
                ui.close();
            }
        });
        let arrow = ui.interact(
            rect(method.right() - 26., method.top(), 26., method.height()),
            field_id.with("arrow"),
            Sense::click(),
        );
        let opening = !was_open && (response.clicked() || edited || arrow.clicked());
        let closing = was_open && arrow.clicked();
        let visible = (was_open || opening) && !closing;
        discovery.popup_was_open = visible;
        if closing {
            discovery.input = method_label(&request.grpc_method);
            discovery.search.clear();
        }
        if opening && !edited {
            discovery.search.clear();
            discovery.highlighted = 0;
        }
        if arrow.clicked() && opening {
            response.request_focus();
        }
        let open = if opening {
            Some(egui::SetOpenCommand::Bool(true))
        } else if closing {
            Some(egui::SetOpenCommand::Bool(false))
        } else {
            None
        };
        let popup = egui::Popup::new(popup_id, ui.ctx().clone(), method, ui.layer_id())
            .open_memory(open)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .gap(4.)
            .accessible_name("gRPC services and methods")
            .frame(
                egui::Frame::popup(ui.style())
                    .fill(if t.dark {
                        crate::theme::hex(0x242424)
                    } else {
                        t.bg
                    })
                    .stroke(Stroke::new(1., t.border))
                    .corner_radius(6)
                    .inner_margin(14.),
            );
        ui.painter().rect_stroke(
            method,
            6,
            Stroke::new(1., if visible { t.accent } else { t.border }),
            StrokeKind::Inside,
        );
        rpc_icon(ui, pos2(method.left() + 15., method.center().y), t);
        w::icon(
            ui,
            pos2(method.right() - 13., method.center().y),
            Icon::Down,
            t.muted,
            12.,
        );
        if !visible {
            response.clone().on_hover_text(&request.grpc_method);
        }

        if visible {
            let key = self.grpc_discovery_key(request);
            let stale = self
                .tabs
                .get(&request.id)
                .is_none_or(|s| s.grpc_discovery.key != key);
            if stale {
                self.discover_grpc(request, ui.ctx());
            }
        }
        let mut refresh = false;
        let mut import = false;
        let mut reflection = false;
        let mut state = self.tabs.remove(&request.id).unwrap_or_default();
        let width = method
            .width()
            .clamp(360., 600.)
            .min(ui.ctx().content_rect().width() - 48.);
        let resize = visible && std::mem::take(&mut state.grpc_discovery.resize_popup);
        popup.width(width + 28.).sizing_pass(resize).show(|ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 8.;
            let ready = !state.grpc_discovery.key.is_empty()
                && !state.grpc_loading
                && state.grpc_discovery.error.is_empty();
            if ready {
                changed |= method_results(ui, width, request, &mut state, t);
                ui.separator();
                let footer = ui.allocate_space(vec2(width, 28.)).1;
                w::label(
                    ui,
                    pos2(footer.left(), footer.center().y),
                    if request.proto_files.is_empty() {
                        "Using server reflection"
                    } else {
                        "Using local proto files"
                    },
                    11.,
                    t.muted,
                );
                if !request.proto_files.is_empty() {
                    reflection |= w::button(
                        ui,
                        rect(footer.left() + 140., footer.top(), 112., 28.),
                        "Use Reflection",
                        false,
                        t,
                    )
                    .clicked();
                }
                refresh |= w::icon_button(
                    ui,
                    rect(footer.right() - 62., footer.top(), 28., 28.),
                    Icon::Refresh,
                    "Reload services and methods",
                    t,
                )
                .clicked();
                import |= w::icon_button(
                    ui,
                    rect(footer.right() - 28., footer.top(), 28., 28.),
                    Icon::Import,
                    "Import a .proto file",
                    t,
                )
                .clicked();
            } else {
                if !state.grpc_discovery.error.is_empty() {
                    let banner = ui.allocate_space(vec2(width, 60.)).1;
                    ui.painter()
                        .rect_filled(banner, 4, t.red.gamma_multiply(0.17));
                    w::medium(
                        ui,
                        pos2(banner.left() + 10., banner.top() + 17.),
                        if request.proto_files.is_empty() {
                            "Could not load server reflection."
                        } else {
                            "Could not load service definition."
                        },
                        12.,
                        t.text,
                    );
                    let details = rect(banner.left() + 10., banner.top() + 30., width - 105., 23.);
                    w::label(
                        ui,
                        pos2(details.left(), details.center().y),
                        "View error details",
                        11.,
                        t.muted,
                    );
                    if ui
                        .interact(
                            details,
                            ui.scope_id().with("grpc-error-details"),
                            Sense::click(),
                        )
                        .clicked()
                    {
                        state.grpc_discovery.show_error = !state.grpc_discovery.show_error;
                    }
                    refresh |= w::button(
                        ui,
                        rect(banner.right() - 87., banner.top() + 25., 77., 26.),
                        "Try Again",
                        false,
                        t,
                    )
                    .clicked();
                    if state.grpc_discovery.show_error {
                        egui::ScrollArea::vertical()
                            .max_height(100.)
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&state.grpc_discovery.error)
                                            .size(11.)
                                            .color(t.muted),
                                    )
                                    .wrap(),
                                );
                            });
                    }
                }
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(
                            "Use a Protobuf schema to load the services and methods.",
                        )
                        .size(12.)
                        .color(t.muted),
                    )
                    .wrap(),
                );
                import |= source_choice(
                    ui,
                    width,
                    "Import a .proto file",
                    "Select a .proto file from your local system.",
                    Icon::Import,
                    false,
                    t,
                );
                ui.separator();
                let note = if state.grpc_loading {
                    if request.proto_files.is_empty() {
                        "Loading server reflection…"
                    } else {
                        "Loading service definition…"
                    }
                } else if request.url.trim().is_empty() {
                    "Enter a server URL to load services and methods."
                } else {
                    "Load methods using server reflection."
                };
                reflection |= source_choice(
                    ui,
                    width,
                    "Use Server Reflection",
                    note,
                    Icon::Refresh,
                    state.grpc_loading,
                    t,
                );
            }
        });
        if state.grpc_discovery.displayed_method != request.grpc_method {
            state.grpc_discovery.input = method_label(&request.grpc_method);
            state.grpc_discovery.displayed_method = request.grpc_method.clone();
            state.grpc_discovery.search.clear();
            response.surrender_focus();
        }
        self.tabs.insert(request.id.clone(), state);
        if import {
            if let Some(paths) = rfd::FileDialog::new()
                .add_filter("Protocol Buffers", &["proto"])
                .pick_files()
            {
                request.proto_files = paths
                    .into_iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
                changed = true;
                refresh = true;
            }
        }
        if reflection {
            request.proto_files.clear();
            changed = true;
            refresh = true;
        }
        if refresh {
            self.discover_grpc(request, ui.ctx());
        }

        if w::icon_button(
            ui,
            rect(bar.right() - 142., bar.top() + 3., 28., 28.),
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
            rect(bar.right() - 108., bar.top() + 3., 28., 28.),
            Icon::Save,
            "Save request (Ctrl+S)",
            t,
        )
        .clicked()
        {
            self.actions.push(Action::Save);
        }
        let running = self.running.contains_key(&request.id);
        let send = ui
            .add_enabled_ui(running || !request.grpc_method.is_empty(), |ui| {
                w::button(
                    ui,
                    rect(bar.right() - 72., bar.top(), 72., bar.height()),
                    if running { "Cancel" } else { "Invoke" },
                    true,
                    t,
                )
            })
            .inner
            .clicked();
        if send
            || (host_response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter))
                && !request.grpc_method.is_empty())
        {
            self.actions.push(if running {
                Action::Cancel
            } else {
                Action::Send
            });
        }
        changed
    }
}

fn source_choice(
    ui: &mut Ui,
    width: f32,
    title: &str,
    note: &str,
    icon: Icon,
    loading: bool,
    t: Theme,
) -> bool {
    let row = ui.allocate_space(vec2(width, 50.)).1;
    let response = ui.interact(
        row,
        ui.scope_id().with(title),
        if loading {
            Sense::hover()
        } else {
            Sense::click()
        },
    );
    if response.hovered() && !loading {
        ui.painter().rect_filled(row, 4, t.hover);
    }
    if loading {
        ui.put(
            rect(row.left() + 1., row.top() + 4., 18., 18.),
            egui::Spinner::new().size(18.).color(t.muted),
        );
    } else {
        w::icon(
            ui,
            pos2(row.left() + 10., row.top() + 14.),
            icon,
            t.muted,
            18.,
        );
    }
    w::medium(
        ui,
        pos2(row.left() + 28., row.top() + 14.),
        title,
        13.,
        t.text,
    );
    w::elided_label(
        ui,
        rect(row.left() + 28., row.top() + 29., width - 28., 18.),
        note,
        12.,
        t.muted,
    );
    response.clicked()
}

fn method_results(
    ui: &mut Ui,
    width: f32,
    request: &mut Request,
    state: &mut TabState,
    t: Theme,
) -> bool {
    let matches: Vec<_> = state
        .grpc_methods
        .iter()
        .filter(|m| matches_search(m, &state.grpc_discovery.search))
        .collect();
    if matches.is_empty() {
        ui.add_space(20.);
        ui.label(
            egui::RichText::new(if state.grpc_methods.is_empty() {
                "The server returned no services or methods."
            } else {
                "No services or methods match your search."
            })
            .size(12.)
            .color(t.muted),
        );
        ui.add_space(20.);
        return false;
    }
    let mut moved = false;
    let (down, up, enter) = std::mem::take(&mut state.grpc_discovery.navigation);
    if down {
        state.grpc_discovery.highlighted = (state.grpc_discovery.highlighted + 1) % matches.len();
        moved = true;
    }
    if up {
        state.grpc_discovery.highlighted =
            (state.grpc_discovery.highlighted + matches.len() - 1) % matches.len();
        moved = true;
    }
    state.grpc_discovery.highlighted = state.grpc_discovery.highlighted.min(matches.len() - 1);
    let mut changed = false;
    egui::ScrollArea::vertical()
        .max_height(300.)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.set_width(width - 10.);
            ui.spacing_mut().item_spacing.y = 0.;
            let mut service = "";
            for (index, method) in matches.iter().enumerate() {
                let (group, name) = method.name.rsplit_once('/').unwrap_or(("", &method.name));
                if group != service {
                    service = group;
                    let heading = ui.allocate_space(vec2(width - 10., 30.)).1;
                    w::elided_medium(ui, heading, group, 11., t.muted);
                }
                let (row, response) =
                    ui.allocate_exact_size(vec2(width - 10., 30.), Sense::click());
                let highlighted = state.grpc_discovery.highlighted == index;
                if highlighted || response.hovered() {
                    ui.painter().rect_filled(row, 4, t.surface);
                }
                if highlighted && moved {
                    response.scroll_to_me(None);
                }
                rpc_icon(ui, pos2(row.left() + 13., row.center().y), t);
                w::elided_label(
                    ui,
                    rect(row.left() + 29., row.top(), row.width() - 55., row.height()),
                    name,
                    13.,
                    t.text,
                );
                if request.grpc_method.trim_start_matches('/') == method.name {
                    w::icon(
                        ui,
                        pos2(row.right() - 12., row.center().y),
                        Icon::Check,
                        t.muted,
                        12.,
                    );
                }
                response.clone().on_hover_text(format!(
                    "{}\n{}",
                    method.name,
                    match (method.client_streaming, method.server_streaming) {
                        (true, true) => "Bidirectional streaming",
                        (true, false) => "Client streaming",
                        (false, true) => "Server streaming",
                        (false, false) => "Unary",
                    }
                ));
                if response.clicked() || (highlighted && enter) {
                    select_method(request, method);
                    changed = true;
                    ui.close();
                }
            }
        });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    const PROTO: &str = "syntax = \"proto3\"; package qa; message Input { string text = 1; } service Echo { rpc Say(Input) returns (Input); rpc Watch(Input) returns (stream Input); }";
    fn sample(name: &str) -> MethodInfo {
        MethodInfo {
            name: name.into(),
            example: r#"{"text":""}"#.into(),
            client_streaming: false,
            server_streaming: false,
        }
    }

    #[test]
    fn tls_changes_invalidate_the_discovery_connection_key() {
        let dir = tempfile::tempdir().unwrap();
        let (app, mut request) = app_request(dir.path(), &dir.path().join("echo.proto"));
        request.url = "localhost:50051".into();
        let plain = app.grpc_discovery_key(&request);
        request.set_grpc_tls(true);
        assert_ne!(plain, app.grpc_discovery_key(&request));
        let tls = app.grpc_discovery_key(&request);
        request.verify_tls = false;
        assert_ne!(tls, app.grpc_discovery_key(&request));
    }
    fn app_request(dir: &std::path::Path, proto: &std::path::Path) -> (App, Request) {
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.into()).unwrap();
        let request = Request {
            protocol: "grpc".into(),
            method: "GRPC".into(),
            proto_files: vec![proto.to_string_lossy().into_owned()],
            ..Default::default()
        };
        app.ws.requests.push(request.clone());
        (app, request)
    }
    fn finish(app: &mut App, id: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        let ctx = app.context.clone();
        while app.tabs[id].grpc_loading && std::time::Instant::now() < deadline {
            app.poll(&ctx);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!app.tabs[id].grpc_loading, "Discovery did not finish");
    }

    #[test]
    fn local_proto_discovery_loads_methods_without_a_host_or_network_connection() {
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("echo.proto");
        std::fs::write(&proto, PROTO).unwrap();
        let (mut app, request) = app_request(dir.path(), &proto);
        app.discover_grpc(&request, &app.context.clone());
        assert!(app.tabs[&request.id].grpc_loading);
        finish(&mut app, &request.id);
        let state = &app.tabs[&request.id];
        assert!(
            state.grpc_discovery.error.is_empty(),
            "{}",
            state.grpc_discovery.error
        );
        assert_eq!(
            state
                .grpc_methods
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["qa.Echo/Say", "qa.Echo/Watch"]
        );
        assert!(state.grpc_methods[1].server_streaming);
    }

    #[test]
    fn failed_discovery_can_be_retried_and_replaces_error_with_methods() {
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("not-yet-imported.proto");
        let (mut app, request) = app_request(dir.path(), &proto);
        app.discover_grpc(&request, &app.context.clone());
        finish(&mut app, &request.id);
        assert!(!app.tabs[&request.id].grpc_discovery.error.is_empty());
        assert!(app.tabs[&request.id].grpc_methods.is_empty());
        std::fs::write(&proto, PROTO).unwrap();
        app.discover_grpc(&request, &app.context.clone());
        assert!(app.tabs[&request.id].grpc_discovery.error.is_empty());
        finish(&mut app, &request.id);
        assert_eq!(app.tabs[&request.id].grpc_methods.len(), 2);
    }

    #[test]
    fn late_results_cannot_replace_methods_after_retry_or_connection_change() {
        let dir = tempfile::tempdir().unwrap();
        let (mut app, mut request) = app_request(dir.path(), &dir.path().join("echo.proto"));
        request.url = "http://localhost:50051".into();
        *app.ws.requests.last_mut().unwrap() = request.clone();
        let key = app.grpc_discovery_key(&request);
        let state = app.tabs.entry(request.id.clone()).or_default();
        state.grpc_loading = true;
        state.grpc_discovery.key = key;
        state.grpc_discovery.generation = 2;
        app.receive_grpc_methods(&request.id, 1, Ok(vec![sample("old.Service/Old")]));
        assert!(app.tabs[&request.id].grpc_loading);
        assert!(app.tabs[&request.id].grpc_methods.is_empty());
        app.ws.requests.last_mut().unwrap().url = "http://localhost:50052".into();
        app.receive_grpc_methods(&request.id, 2, Ok(vec![sample("old.Service/Old")]));
        assert!(!app.tabs[&request.id].grpc_loading);
        assert!(app.tabs[&request.id].grpc_methods.is_empty());
        let current = app.ws.requests.last().unwrap().clone();
        let key = app.grpc_discovery_key(&current);
        let state = app.tabs.get_mut(&request.id).unwrap();
        state.grpc_loading = true;
        state.grpc_discovery.key = key;
        state.grpc_discovery.generation = 3;
        app.receive_grpc_methods(&request.id, 3, Ok(vec![sample("new.Service/New")]));
        assert_eq!(
            app.tabs[&request.id].grpc_methods[0].name,
            "new.Service/New"
        );
        app.receive_grpc_methods(&request.id, 2, Err("old failure".into()));
        assert!(app.tabs[&request.id].grpc_discovery.error.is_empty());
    }

    #[test]
    fn search_matches_service_and_method_and_selection_preserves_custom_messages() {
        let method = sample("company.users.UserService/GetUser");
        assert!(matches_search(&method, "users GET"));
        assert!(!matches_search(&method, "users delete"));
        assert_eq!(method_label(&method.name), "UserService / GetUser");
        let mut request = Request {
            body: r#"{"userId":42}"#.into(),
            ..Default::default()
        };
        select_method(&mut request, &method);
        assert_eq!(request.body, r#"{"userId":42}"#);
        assert_eq!(request.grpc_method, method.name);
        request.body = "{}".into();
        select_method(&mut request, &method);
        assert_eq!(request.body, method.example);
    }

    #[test]
    fn generate_action_loads_schema_for_a_saved_method_without_opening_the_picker() {
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("echo.proto");
        std::fs::write(&proto, PROTO).unwrap();
        let (mut app, request) = app_request(dir.path(), &proto);
        let draft = app.ws.requests.last_mut().unwrap();
        draft.grpc_method = "/{{service}}/Say".into();
        draft.vars.push(Pair::new("service", "qa.Echo"));
        draft.body = "old draft".into();
        app.actions
            .push(Action::GenerateGrpcExample(request.id.clone()));
        app.handle_actions(&app.context.clone());
        assert!(app.tabs[&request.id].grpc_loading);
        finish(&mut app, &request.id);
        assert_eq!(app.status, "Example message generated.");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&app.ws.requests.last().unwrap().body)
                .unwrap(),
            serde_json::json!({"text":""})
        );
        assert!(app.modified_requests.contains(&request.id));
        // A second click uses the loaded schema; no new discovery task is needed.
        let generation = app.tabs[&request.id].grpc_discovery.generation;
        app.generate_grpc_example(&request.id, &app.context.clone());
        assert!(!app.tabs[&request.id].grpc_loading);
        assert_eq!(app.tabs[&request.id].grpc_discovery.generation, generation);
    }

    #[test]
    fn pending_example_preserves_body_or_method_edits_and_reports_discovery_errors() {
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("echo.proto");
        std::fs::write(&proto, PROTO).unwrap();
        let (mut app, request) = app_request(dir.path(), &proto);
        app.ws.requests.last_mut().unwrap().grpc_method = "qa.Echo/Say".into();
        app.generate_grpc_example(&request.id, &app.context.clone());
        app.ws.requests.last_mut().unwrap().body = "edited while loading".into();
        finish(&mut app, &request.id);
        assert_eq!(app.ws.requests.last().unwrap().body, "edited while loading");

        app.invalidate_grpc_discovery(&request.id);
        app.generate_grpc_example(&request.id, &app.context.clone());
        app.ws.requests.last_mut().unwrap().grpc_method = "qa.Echo/Watch".into();
        finish(&mut app, &request.id);
        assert_eq!(app.ws.requests.last().unwrap().body, "edited while loading");

        app.ws.requests.last_mut().unwrap().grpc_method = "qa.Echo/Missing".into();
        app.generate_grpc_example(&request.id, &app.context.clone());
        finish(&mut app, &request.id);
        assert!(app.status.contains("was not found"));
        assert_eq!(app.ws.requests.last().unwrap().body, "edited while loading");

        app.ws.requests.last_mut().unwrap().proto_files = vec![
            dir.path()
                .join("missing.proto")
                .to_string_lossy()
                .into_owned(),
        ];
        app.generate_grpc_example(&request.id, &app.context.clone());
        finish(&mut app, &request.id);
        assert!(app.status.starts_with("Cannot generate example message:"));
        assert_eq!(app.ws.requests.last().unwrap().body, "edited while loading");
    }

    #[test]
    fn generate_reuses_in_flight_discovery_and_wraps_client_streaming_examples() {
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("stream.proto");
        std::fs::write(
            &proto,
            PROTO.replace("rpc Say(Input)", "rpc Say(stream Input)"),
        )
        .unwrap();
        let (mut app, mut request) = app_request(dir.path(), &proto);
        request.grpc_method = "qa.Echo/Say".into();
        *app.ws.requests.last_mut().unwrap() = request.clone();
        app.discover_grpc(&request, &app.context.clone());
        let generation = app.tabs[&request.id].grpc_discovery.generation;
        app.generate_grpc_example(&request.id, &app.context.clone());
        assert_eq!(app.tabs[&request.id].grpc_discovery.generation, generation);
        finish(&mut app, &request.id);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&app.ws.requests.last().unwrap().body)
                .unwrap(),
            serde_json::json!([{"text":""}])
        );
    }
}
