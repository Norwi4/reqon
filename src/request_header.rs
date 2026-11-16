use crate::{
    app::{Action, App, TabState},
    model::{Request, RequestKind},
    theme::{Theme, hex},
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, FontId, Rect, Sense, TextEdit, Ui, pos2, vec2};

fn protocol_icon(ui: &Ui, center: egui::Pos2, kind: RequestKind, t: Theme) {
    let (asset, bytes, color): (_, &[u8], _) = match kind {
        RequestKind::Http => (
            "bytes://icons/RequestHttp.svg",
            include_bytes!("../assets/icons/RequestHttp.svg"),
            hex(if t.dark { 0x74c6cf } else { 0x267e88 }),
        ),
        RequestKind::Grpc => (
            "bytes://icons/RequestGrpc.svg",
            include_bytes!("../assets/icons/RequestGrpc.svg"),
            t.method("gRPC"),
        ),
        RequestKind::Graphql => {
            w::icon(ui, center, Icon::Code, t.pink, 20.);
            return;
        }
        RequestKind::Ws => {
            w::icon(ui, center, Icon::ArrowRight, t.orange, 20.);
            return;
        }
    };
    egui::Image::from_bytes(asset, bytes)
        .tint(color)
        .paint_at(ui, Rect::from_center_size(center, vec2(20., 20.)));
}

impl App {
    fn can_switch_request_type(&self, id: &str) -> bool {
        !self.running.contains_key(id)
            && !self.sockets.contains_key(id)
            && !self.tabs.get(id).is_some_and(|s| s.grpc_loading)
    }

    pub fn switch_request_type(&mut self, id: &str, kind: RequestKind) {
        if !self.can_switch_request_type(id) {
            self.notice("Finish or cancel the current request before changing its type");
            return;
        }
        let Some(request) = self.ws.requests.iter_mut().find(|r| r.id == id) else {
            return;
        };
        if !request.switch_kind(kind) {
            return;
        }
        self.ws
            .last_request_kind
            .insert(request.collection.clone(), kind);
        self.tabs.insert(
            id.into(),
            TabState {
                request_tab: usize::from(kind != RequestKind::Http),
                ..Default::default()
            },
        );
        self.dirty_cache.borrow_mut().remove(id);
        self.modified_requests.insert(id.into());
        self.tree.stale = true;
        self.mark_workspace_changed();
        self.context.request_repaint();
    }

    pub fn request_header(&mut self, ui: &mut Ui, r: Rect, request: &mut Request) -> bool {
        let t = self.theme;
        let kind = RequestKind::of(request);
        let label_width = ui
            .painter()
            .layout_no_wrap(kind.label().into(), FontId::proportional(12.), t.text)
            .size()
            .x
            .ceil();
        let button = rect(r.left(), r.center().y - 14., label_width + 49., 28.);
        let enabled = self.can_switch_request_type(&request.id);
        let response = ui
            .add_enabled_ui(enabled, |ui| {
                ui.interact(
                    button,
                    ui.scope_id().with(("request-type", &request.id)),
                    Sense::click(),
                )
            })
            .inner;
        response.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::Role::Button,
                enabled,
                format!("Request type: {}", kind.label()),
            )
        });
        if response.hovered() || response.has_focus() {
            ui.painter().rect_filled(button, 4, t.hover);
        }
        protocol_icon(ui, pos2(button.left() + 13., button.center().y), kind, t);
        w::label(
            ui,
            pos2(button.left() + 28., button.center().y),
            kind.label(),
            12.,
            t.text,
        );
        w::icon(
            ui,
            pos2(button.right() - 10., button.center().y),
            Icon::Down,
            t.muted,
            12.,
        );
        egui::Popup::menu(&response).show(|ui| {
            ui.set_width(180.);
            ui.spacing_mut().item_spacing.y = 0.;
            for option in RequestKind::ALL {
                let (row, selection) = ui.allocate_exact_size(vec2(180., 32.), Sense::click());
                selection.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, option.label())
                });
                if selection.hovered() || option == kind {
                    ui.painter().rect_filled(row, 4, t.hover);
                }
                protocol_icon(ui, pos2(row.left() + 16., row.center().y), option, t);
                w::label(
                    ui,
                    pos2(row.left() + 34., row.center().y),
                    option.label(),
                    13.,
                    t.text,
                );
                if option == kind {
                    w::icon(
                        ui,
                        pos2(row.right() - 14., row.center().y),
                        Icon::Check,
                        t.muted,
                        14.,
                    );
                }
                if selection.clicked() {
                    self.actions
                        .push(Action::ChangeRequestType(request.id.clone(), option));
                    ui.close();
                }
            }
        });
        response.on_hover_text(if enabled {
            "Change request type"
        } else {
            "Finish or cancel the current request before changing its type"
        });
        w::line(
            ui,
            pos2(button.right() + 8., r.center().y - 8.),
            pos2(button.right() + 8., r.center().y + 8.),
            t.border,
        );
        let name_rect = rect(
            button.right() + 20.,
            button.top(),
            (r.right()
                - button.right()
                - 20.
                - if self.ws.preferences.ai.enabled {
                    36.
                } else {
                    0.
                })
            .max(40.),
            28.,
        );
        let name = ui.put(
            name_rect,
            TextEdit::singleline(&mut request.name)
                .id(ui.scope_id().with(("request-name", &request.id)))
                .font(FontId::new(13., egui::FontFamily::Name("Medium".into())))
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .min_size(vec2(0., 28.))
                .align(egui::Align2::LEFT_CENTER)
                .hint_text("Untitled"),
        );
        let mut changed = name.changed();
        if name.lost_focus() && request.name.trim().is_empty() {
            request.name = "Untitled".into();
            changed = true;
        }
        name.on_hover_text("Request name");
        self.ai_button(ui, r, request);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn switching_changes_current_tab_and_new_tab_default_without_opening_a_dialog() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        let cid = app.active_collection_id();
        app.new_transient_request(cid.clone(), Some(RequestKind::Http));
        let id = app.active().unwrap().id.clone();
        let tabs = app.ws.open_tabs.clone();
        app.tabs.entry(id.clone()).or_default().set_reply(Reply {
            body: "old response".into(),
            ..Default::default()
        });
        app.actions
            .push(Action::ChangeRequestType(id.clone(), RequestKind::Grpc));
        app.handle_actions(&ctx);
        assert_eq!(app.ws.active.as_deref(), Some(id.as_str()));
        assert_eq!(app.ws.open_tabs, tabs);
        assert_eq!(app.active().unwrap().protocol, "grpc");
        assert_eq!(app.tabs[&id].request_tab, 1);
        assert!(app.tabs[&id].reply.is_none());
        assert!(app.dirty(&id));
        assert!(matches!(app.dialog, crate::app::Dialog::None));
        app.new_transient_request(cid, None);
        assert_eq!(app.active().unwrap().protocol, "grpc");
        assert!(app.active().unwrap().protocol_drafts.is_empty());
    }

    #[test]
    fn switching_is_blocked_during_requests_discovery_or_a_live_socket() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        app.new_transient_request(app.active_collection_id(), Some(RequestKind::Http));
        let id = app.active().unwrap().id.clone();
        let task = app.runtime.spawn(std::future::pending::<()>());
        app.running.insert(
            id.clone(),
            (
                1,
                crate::cancellation::Task::new(
                    task.abort_handle(),
                    crate::cancellation::CancellationToken::new(),
                ),
            ),
        );
        app.switch_request_type(&id, RequestKind::Grpc);
        assert_eq!(app.active().unwrap().protocol, "http");
        app.running.remove(&id);
        task.abort();
        app.tabs.get_mut(&id).unwrap().grpc_loading = true;
        app.switch_request_type(&id, RequestKind::Grpc);
        assert_eq!(app.active().unwrap().protocol, "http");
        app.tabs.get_mut(&id).unwrap().grpc_loading = false;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        app.sockets.insert(id.clone(), tx);
        app.switch_request_type(&id, RequestKind::Grpc);
        assert_eq!(app.active().unwrap().protocol, "http");
        app.sockets.remove(&id);
        app.switch_request_type(&id, RequestKind::Grpc);
        assert_eq!(app.active().unwrap().protocol, "grpc");
    }
}
