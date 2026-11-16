use crate::protocol_prepare::{ProtocolInputs, prepare_protocol_cancellable};
use crate::{
    app::{Action, App, Event, TabState},
    model::*,
    websocket::{self, Command, SocketEvent},
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, pos2, vec2};

impl App {
    pub(crate) fn protocol_inputs(&self, request: &Request) -> ProtocolInputs {
        let environment = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == request.collection)
            .and_then(|c| {
                c.environments
                    .iter()
                    .find(|e| e.name == self.ws.selected_environment)
            })
            .map(|e| e.vars.clone())
            .unwrap_or_default();
        (
            self.workspace_globals(),
            environment,
            self.runtime_vars
                .get(&request.collection)
                .cloned()
                .unwrap_or_default(),
        )
    }
    pub fn connect_socket(&mut self, request: Request, ctx: &egui::Context) {
        let preferences = self.ws.preferences.clone();
        let inputs = self.protocol_inputs(&request);
        let id = request.id.clone();
        let tx = self.tx.clone();
        let context = ctx.clone();
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.sockets.insert(id.clone(), sender);
        self.sequence += 1;
        let sequence = self.sequence;
        let key = id.clone();
        let token = crate::cancellation::CancellationToken::new();
        let script_cancel = token.clone();
        let handle = self.runtime.spawn(async move {
            let network_preferences = preferences.clone();
            let emit = |event| {
                let _ = tx.send(Event::Socket(key.clone(), sequence, event));
                context.request_repaint();
            };
            let prepared = tokio::task::spawn_blocking(move || {
                let (request, values) =
                    prepare_protocol_cancellable(request, preferences, inputs, &script_cancel)?;
                if request.pre_script.trim().is_empty() {
                    Ok((request, values))
                } else {
                    crate::scripts::run_cancellable(
                        &request.pre_script,
                        &request,
                        &Reply::default(),
                        &values,
                        &script_cancel,
                    )
                    .map(|out| (out.request, out.vars))
                }
            })
            .await;
            match prepared {
                Ok(Ok((mut request, vars))) => {
                    if let Err(e) = crate::preference_network::prepare(
                        &network_preferences,
                        &mut request,
                        &vars,
                    )
                    .await
                    {
                        let _ = tx.send(Event::Socket(
                            key.clone(),
                            sequence,
                            SocketEvent::Closed(format!("{e:#}")),
                        ));
                        context.request_repaint();
                        return;
                    }
                    if let Err(e) = websocket::run(request, vars, receiver, emit).await {
                        let _ = tx.send(Event::Socket(
                            key,
                            sequence,
                            SocketEvent::Closed(format!("{e:#}")),
                        ));
                        context.request_repaint();
                    }
                }
                error => {
                    let _ = tx.send(Event::Socket(
                        key,
                        sequence,
                        SocketEvent::Closed(format!("Cannot prepare WebSocket: {error:?}")),
                    ));
                    context.request_repaint();
                }
            }
        });
        self.running.insert(
            id,
            (
                sequence,
                crate::cancellation::Task::new(handle.abort_handle(), token),
            ),
        );
    }
    pub fn receive_socket(&mut self, id: String, sequence: u64, event: SocketEvent) {
        if self.running.get(&id).is_none_or(|(s, _)| *s != sequence) {
            return;
        }
        let state = self.tabs.entry(id.clone()).or_default();
        match event {
            SocketEvent::Connected(headers) => {
                state.socket_connected = true;
                state.reply = Some(Reply {
                    status: 101,
                    reason: "Switching Protocols".into(),
                    headers,
                    ..Default::default()
                });
                state.socket_messages.push_back(websocket::Record::new(
                    false,
                    "Connected",
                    "Connection established".into(),
                ));
            }
            SocketEvent::Message(record) => {
                state.socket_messages.push_back(record);
                state.socket_selected = state.socket_messages.len().saturating_sub(1);
            }
            SocketEvent::Closed(message) => {
                state.socket_connected = false;
                state.socket_messages.push_back(websocket::Record::new(
                    false,
                    "Disconnected",
                    message,
                ));
                state.socket_selected = state.socket_messages.len() - 1;
                self.running.remove(&id);
                self.sockets.remove(&id);
            }
        }
        let mut bytes = state
            .socket_messages
            .iter()
            .map(|m| m.body.len())
            .sum::<usize>();
        while state.socket_messages.len() > 500 || bytes > 64 * 1024 * 1024 {
            if let Some(old) = state.socket_messages.pop_front() {
                bytes = bytes.saturating_sub(old.body.len());
                state.socket_selected = state.socket_selected.saturating_sub(1);
            } else {
                break;
            }
        }
    }
    pub(crate) fn grpc_message_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        header: Rect,
        request: &mut Request,
        state: &TabState,
    ) -> bool {
        let t = self.theme;
        let mut changed = false;
        if w::icon_button(
            ui,
            rect(header.right() - 64., header.top() + 3., 24., 24.),
            Icon::Wand,
            "Prettify message",
            t,
        )
        .clicked()
        {
            if let Ok(value) = crate::json_comments::format(&request.body) {
                request.body = value;
                changed = true;
            }
        }
        if w::icon_button(
            ui,
            rect(header.right() - 32., header.top() + 3., 24., 24.),
            Icon::MessageRefresh,
            if state.grpc_loading {
                "Loading service definitions..."
            } else {
                "Generate example message"
            },
            t,
        )
        .clicked()
        {
            // Apply after the editor puts its detached tab/request state back.
            self.actions
                .push(Action::GenerateGrpcExample(request.id.clone()));
        }
        changed
    }
    pub fn protocol_message_editor(
        &mut self,
        ui: &mut egui::Ui,
        request: &mut Request,
        state: &mut TabState,
    ) -> bool {
        let t = self.theme;
        let mut changed = false;
        if request.protocol_messages.is_empty() {
            let mut message = Pair::new("message 1", &request.body);
            message.kind = "json".into();
            request.protocol_messages.push(message);
        }
        let selected = request
            .protocol_messages
            .iter()
            .position(|m| m.enabled)
            .unwrap_or(0);
        if request.protocol == "grpc" && request.protocol_messages.len() == 1 {
            changed |=
                w::editor_with_placeholder(ui, &mut request.body, false, t, "Compose message");
            return changed;
        }
        ui.horizontal(|ui| {
            let label = request.protocol_messages[selected].key.clone();
            egui::ComboBox::from_id_salt("protocol-message")
                .selected_text(label)
                .width(160.)
                .show_ui(ui, |ui| {
                    for i in 0..request.protocol_messages.len() {
                        if ui
                            .selectable_label(i == selected, &request.protocol_messages[i].key)
                            .clicked()
                        {
                            request.protocol_messages[selected].value = request.body.clone();
                            for m in &mut request.protocol_messages {
                                m.enabled = false;
                            }
                            request.protocol_messages[i].enabled = true;
                            request.body = request.protocol_messages[i].value.clone();
                            changed = true;
                        }
                    }
                });
            if ui.small_button("+").on_hover_text("Add message").clicked() {
                request.protocol_messages[selected].value = request.body.clone();
                for m in &mut request.protocol_messages {
                    m.enabled = false;
                }
                let mut message = Pair::new(
                    format!("message {}", request.protocol_messages.len() + 1),
                    "",
                );
                message.kind = "json".into();
                request.protocol_messages.push(message);
                request.body.clear();
                changed = true;
            }
            if ui.small_button("Prettify").clicked() {
                if let Ok(value) = crate::json_comments::format(&request.body) {
                    request.body = value;
                    changed = true;
                }
            }
            if request.protocol == "ws" {
                if ui
                    .add_enabled(state.socket_connected, egui::Button::new("Send"))
                    .clicked()
                {
                    if let Some(sender) = self.sockets.get(&request.id) {
                        let text = resolve(&request.body, &self.vars(request));
                        let _ = sender.send(Command::Text(text));
                    }
                }
                if ui
                    .add_enabled(state.socket_connected, egui::Button::new("Ping"))
                    .clicked()
                {
                    if let Some(sender) = self.sockets.get(&request.id) {
                        let _ = sender.send(Command::Ping);
                    }
                }
                if ui
                    .add_enabled(
                        state.socket_connected,
                        egui::Button::new("Send binary file"),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new().pick_file() {
                        match std::fs::read(path) {
                            Ok(bytes) => {
                                if let Some(sender) = self.sockets.get(&request.id) {
                                    let _ = sender.send(Command::Binary(bytes));
                                }
                            }
                            Err(e) => self.notice(e.to_string()),
                        }
                    }
                }
            }
        });
        ui.add_space(12.);
        let placeholder = if request.protocol == "grpc" {
            "Compose message"
        } else {
            ""
        };
        changed |= w::editor_with_placeholder(ui, &mut request.body, false, t, placeholder);
        changed
    }
    pub fn socket_response(&mut self, ui: &mut egui::Ui, r: Rect, request: &Request) {
        let t = self.theme;
        let mut state = self.tabs.remove(&request.id).unwrap_or_default();
        let connected = state.socket_connected;
        w::label(ui, pos2(r.left(), r.top() + 16.), "Messages", 12., t.text);
        ui.painter().circle_filled(
            pos2(r.right() - 104., r.top() + 16.),
            3.,
            if connected { t.green } else { t.muted },
        );
        w::label(
            ui,
            pos2(r.right() - 94., r.top() + 16.),
            if connected {
                "Connected"
            } else {
                "Disconnected"
            },
            12.,
            t.muted,
        );
        if w::icon_button(
            ui,
            rect(r.right() - 150., r.top() + 4., 24., 24.),
            Icon::Trash,
            "Clear messages",
            t,
        )
        .clicked()
        {
            state.socket_messages.clear();
            state.socket_selected = 0;
        }
        w::line(
            ui,
            pos2(r.left(), r.top() + 34.),
            pos2(r.right(), r.top() + 34.),
            t.border,
        );
        if state.socket_messages.is_empty() {
            w::label(
                ui,
                pos2(r.left() + 16., r.top() + 70.),
                "Connect to a WebSocket server to start sending messages",
                13.,
                t.muted,
            );
        } else {
            let list_width = (r.width() * 0.38).clamp(180., 420.);
            let mut list = w::child(
                ui,
                rect(r.left(), r.top() + 44., list_width, r.height() - 44.),
                "socket-list",
            );
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(&mut list, 32., state.socket_messages.len(), |ui, range| {
                    for i in range {
                        let m = &state.socket_messages[i];
                        let row = ui.allocate_space(vec2(ui.available_width(), 32.)).1;
                        let response =
                            ui.interact(row, ui.scope_id().with(i), egui::Sense::click());
                        if state.socket_selected == i || response.hovered() {
                            ui.painter().rect_filled(row, 3, t.surface);
                        }
                        w::icon(
                            ui,
                            pos2(row.left() + 12., row.center().y),
                            if m.outgoing {
                                Icon::Send
                            } else {
                                Icon::Download
                            },
                            if m.outgoing { t.accent } else { t.green },
                            14.,
                        );
                        w::label(
                            ui,
                            pos2(row.left() + 27., row.center().y),
                            &m.kind,
                            12.,
                            t.text,
                        );
                        let sec = m.at / 1000;
                        w::mono(
                            ui,
                            pos2(row.right() - 87., row.center().y),
                            &format!(
                                "{:02}:{:02}:{:02}",
                                sec / 3600 % 24,
                                sec / 60 % 60,
                                sec % 60
                            ),
                            11.,
                            t.muted,
                        );
                        if response.clicked() {
                            state.socket_selected = i;
                        }
                    }
                });
            w::line(
                ui,
                pos2(r.left() + list_width + 8., r.top() + 35.),
                pos2(r.left() + list_width + 8., r.bottom()),
                t.border,
            );
            if let Some(record) = state
                .socket_messages
                .get(state.socket_selected.min(state.socket_messages.len() - 1))
            {
                let mut detail = w::child(
                    ui,
                    rect(
                        r.left() + list_width + 20.,
                        r.top() + 46.,
                        r.width() - list_width - 20.,
                        r.height() - 46.,
                    ),
                    "socket-message",
                );
                w::viewer(&mut detail, &record.body, &record.lines, t);
            }
        }
        self.tabs.insert(request.id.clone(), state);
    }
}
