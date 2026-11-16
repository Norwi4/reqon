use crate::{
    app::{App, Dialog},
    model::*,
    theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Align2, FontId, Sense, TextEdit, pos2, vec2};

impl App {
    pub fn new_transient_request(&mut self, collection: String, kind: Option<RequestKind>) {
        // A tab is an unsaved request, not the named-file creation dialog.
        let kind = kind
            .or_else(|| {
                self.active()
                    .filter(|r| r.collection == collection)
                    .map(RequestKind::of)
            })
            .or_else(|| self.ws.last_request_kind.get(&collection).copied())
            .unwrap_or_default();
        let number = self
            .ws
            .requests
            .iter()
            .filter(|r| r.collection == collection && r.path.is_none())
            .filter_map(|r| r.name.strip_prefix("Untitled ")?.parse::<u64>().ok())
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let (protocol, method, body_kind) = match kind {
            RequestKind::Http => ("http", "GET", BodyKind::None),
            RequestKind::Graphql => ("graphql", "POST", BodyKind::Graphql),
            RequestKind::Grpc => ("grpc", "GRPC", BodyKind::Json),
            RequestKind::Ws => ("ws", "WS", BodyKind::None),
        };
        let request = Request {
            name: format!("Untitled {number}"),
            seq: self.ws.next_request_seq(&collection, ""),
            collection,
            protocol: protocol.into(),
            method: method.into(),
            body_kind,
            graphql_variables: String::new(),
            ..Default::default()
        };
        let id = request.id.clone();
        self.ws.requests.push(request);
        self.tabs.entry(id.clone()).or_default().request_tab =
            usize::from(kind != RequestKind::Http);
        self.tree.stale = true;
        self.open(id);
    }

    pub fn new_request_dialog(&mut self, ctx: &egui::Context, cid: &str, folder: &str) {
        let t = self.theme;
        let mut close = false;
        let mut create = false;
        let id = egui::Id::unique("new-request-modal");
        let width = 500_f32.min(ctx.content_rect().width() - 32.);
        let is_curl = self.new_request_kind == 4;
        let height = if is_curl { 450. } else { 364. };
        let response = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0., 50.)))
            .backdrop_color(egui::Color32::from_black_alpha(128))
            .frame(
                egui::Frame::NONE
                    .fill(theme::hex(if t.dark { 0x222224 } else { 0xffffff }))
                    .stroke(egui::Stroke::new(1., t.border))
                    .corner_radius(5),
            )
            .show(ctx, |ui| {
                ui.visuals_mut().text_edit_bg_color = Some(egui::Color32::TRANSPARENT);
                let r = ui.allocate_space(vec2(width - 2., height - 2.)).1;
                ui.painter().rect_filled(
                    rect(r.left(), r.top(), r.width(), 40.),
                    egui::CornerRadius {
                        nw: 5,
                        ne: 5,
                        sw: 0,
                        se: 0,
                    },
                    t.bg,
                );
                w::semibold(
                    ui,
                    pos2(r.left() + 16., r.top() + 20.),
                    "New Request",
                    14.,
                    t.text,
                );
                if w::icon_button(
                    ui,
                    rect(r.right() - 34., r.top() + 8., 24., 24.),
                    Icon::Close,
                    "Close New Request",
                    t,
                )
                .clicked()
                {
                    close = true;
                }
                w::semibold(ui, pos2(r.left() + 16., r.top() + 65.), "Type", 13., t.text);
                for (kind, label, col, row) in [
                    (0, "HTTP", 0., 0.),
                    (1, "GraphQL", 0., 1.),
                    (2, "gRPC", 1., 0.),
                    (3, "WebSocket", 1., 1.),
                    (4, "From cURL", 2., 0.),
                ] {
                    let rr = rect(
                        r.left() + 16. + col * (r.width() - 32.) / 3.,
                        r.top() + 82. + row * 27.,
                        145.,
                        22.,
                    );
                    let selected = self.new_request_kind == kind;
                    let center = pos2(rr.left() + 6., rr.top() + 11.);
                    ui.painter().circle_filled(
                        center,
                        6.,
                        if selected {
                            t.accent
                        } else {
                            egui::Color32::WHITE
                        },
                    );
                    ui.painter()
                        .circle_stroke(center, 6., egui::Stroke::new(1., t.muted));
                    if selected {
                        ui.painter()
                            .circle_stroke(center, 4., egui::Stroke::new(1., t.bg));
                    }
                    w::label(ui, pos2(rr.left() + 24., center.y), label, 13., t.text);
                    if ui.interact(rr, id.with(kind), Sense::click()).clicked() {
                        self.new_request_kind = kind;
                        if kind == 1 {
                            self.entry2 = "POST".into();
                        }
                    }
                }
                w::semibold(
                    ui,
                    pos2(r.left() + 16., r.top() + 156.),
                    "Request Name",
                    13.,
                    t.text,
                );
                let input = ui.put(
                    rect(r.left() + 16., r.top() + 174., r.width() - 32., 34.),
                    TextEdit::singleline(&mut self.entry)
                        .id(id.with("name"))
                        .hint_text("Request Name")
                        .font(FontId::proportional(13.))
                        .margin(vec2(7., 8.)),
                );
                if self.dialog_first_frame {
                    input.request_focus();
                    self.dialog_first_frame = false;
                }
                if is_curl {
                    w::semibold(
                        ui,
                        pos2(r.left() + 16., r.top() + 233.),
                        "cURL",
                        13.,
                        t.text,
                    );
                    ui.put(
                        rect(r.left() + 16., r.top() + 251., r.width() - 32., 124.),
                        TextEdit::multiline(&mut self.entry3)
                            .font(FontId::monospace(13.))
                            .hint_text("curl https://api.example.com")
                            .margin(vec2(7., 7.)),
                    );
                } else {
                    w::semibold(ui, pos2(r.left() + 16., r.top() + 233.), "URL", 13., t.text);
                    let url = rect(r.left() + 16., r.top() + 251., r.width() - 32., 34.);
                    ui.painter().rect_stroke(
                        url,
                        5,
                        egui::Stroke::new(1., t.strong_border),
                        egui::StrokeKind::Inside,
                    );
                    let is_http = matches!(self.new_request_kind, 0 | 1);
                    if is_http {
                        let method = w::dropdown(
                            ui,
                            rect(url.left() + 1., url.top() + 1., 66., 32.),
                            &self.entry2,
                            true,
                            false,
                            t,
                        );
                        egui::Popup::menu(&method).show(|ui| {
                            for method in
                                ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"]
                            {
                                if ui.selectable_label(self.entry2 == method, method).clicked() {
                                    self.entry2 = method.into();
                                    ui.close();
                                }
                            }
                        });
                        w::line(
                            ui,
                            pos2(url.left() + 67., url.top()),
                            pos2(url.left() + 67., url.bottom()),
                            t.strong_border,
                        );
                    }
                    ui.put(
                        rect(
                            url.left() + if is_http { 79. } else { 12. },
                            url.top() + 7.,
                            url.width() - if is_http { 87. } else { 24. },
                            23.,
                        ),
                        TextEdit::singleline(&mut self.entry3)
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::ZERO)
                            .hint_text("Request URL")
                            .font(FontId::proportional(13.)),
                    );
                }
                let y = r.bottom() - 45.;
                let options = rect(r.left() + 16., y, 70., 30.);
                w::label(
                    ui,
                    pos2(options.left(), options.center().y),
                    "Options",
                    12.,
                    t.blue,
                );
                w::icon(
                    ui,
                    pos2(options.right() - 10., options.center().y),
                    Icon::Down,
                    t.blue,
                    12.,
                );
                let menu = ui.interact(options, id.with("options"), Sense::click());
                egui::Popup::menu(&menu).show(|ui| {
                    let extension = if self
                        .ws
                        .collections
                        .iter()
                        .find(|c| c.id == cid)
                        .is_some_and(|c| c.path.join("opencollection.yml").is_file())
                    {
                        "yml"
                    } else {
                        "req"
                    };
                    ui.label(format!("File: {}.{}", safe_name(&self.entry), extension));
                    ui.label(if folder.is_empty() {
                        "Collection root"
                    } else {
                        folder
                    });
                });
                let cancel = rect(r.right() - 155., y, 65., 30.);
                w::label(
                    ui,
                    pos2(cancel.left() + 10., cancel.center().y),
                    "Cancel",
                    12.,
                    t.text,
                );
                if ui
                    .interact(cancel, id.with("cancel"), Sense::click())
                    .clicked()
                {
                    close = true;
                }
                if w::button(ui, rect(r.right() - 88., y, 72., 30.), "Create", true, t).clicked() {
                    create = true;
                }
                if ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) && !is_curl {
                    create = true;
                }
            });
        if create && !self.entry.trim().is_empty() {
            let request = if is_curl {
                crate::import::curl(&self.entry3)
            } else {
                Ok(Request {
                    name: self.entry.trim().to_owned(),
                    method: match self.new_request_kind {
                        2 => "GRPC".into(),
                        3 => "WS".into(),
                        _ => self.entry2.clone(),
                    },
                    url: self.entry3.trim().to_owned(),
                    protocol: match self.new_request_kind {
                        2 => "grpc",
                        3 => "ws",
                        1 => "graphql",
                        _ => "http",
                    }
                    .into(),
                    body_kind: if self.new_request_kind == 1 {
                        BodyKind::Graphql
                    } else {
                        BodyKind::None
                    },
                    ..Default::default()
                })
            };
            match request {
                Ok(mut request) => {
                    request.name = self.entry.trim().into();
                    request.collection = cid.into();
                    request.folder = folder.into();
                    request.seq = self.ws.next_request_seq(cid, folder);
                    if let Some(c) = self.ws.collections.iter().find(|c| c.id == cid) {
                        request.path = Some(self.unique_path(&c.path, folder, &request.name));
                    }
                    let rid = request.id.clone();
                    self.ws.requests.push(request);
                    self.tree.stale = true;
                    self.open(rid);
                    self.save_active();
                    close = true;
                }
                Err(e) => self.notice(format!("Invalid cURL: {e:#}")),
            }
        }
        if close || response.should_close() {
            self.dialog = Dialog::None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Action;

    #[test]
    fn new_tab_creates_blank_requests_without_a_dialog_or_collection_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        let cid = app.active_collection_id();
        for (kind, protocol, method, body) in [
            (RequestKind::Http, "http", "GET", BodyKind::None),
            (RequestKind::Graphql, "graphql", "POST", BodyKind::Graphql),
            (RequestKind::Grpc, "grpc", "GRPC", BodyKind::Json),
            (RequestKind::Ws, "ws", "WS", BodyKind::None),
        ] {
            app.actions.push(Action::NewTab(cid.clone(), Some(kind)));
            app.handle_actions(&ctx);
            let request = app.active().unwrap();
            assert!(app.dialog == Dialog::None);
            assert_eq!(request.protocol, protocol);
            assert_eq!(request.method, method);
            assert_eq!(request.body_kind, body);
            assert!(request.path.is_none());
            assert!(request.url.is_empty() && request.body.is_empty());
            assert!(request.headers.is_empty() && request.tests.is_empty());
            assert_eq!(
                app.tabs[&request.id].request_tab,
                usize::from(kind != RequestKind::Http)
            );
            assert!(app.dirty(&request.id));
        }
        assert_eq!(app.active().unwrap().name, "Untitled 4");
        let collection = &app.ws.collections[0];
        assert!(!collection.path.join("Untitled 4.req").exists());
    }

    #[test]
    fn new_tab_reuses_last_opened_type_and_keeps_it_across_workspace_restore() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        let cid = app.active_collection_id();
        let http = app.ws.active.clone().unwrap();
        app.new_transient_request(cid.clone(), Some(RequestKind::Graphql));
        let graphql = app.ws.active.clone().unwrap();
        app.new_transient_request(cid.clone(), None);
        assert_eq!(app.active().unwrap().protocol, "graphql");
        app.open(http);
        app.new_transient_request(cid.clone(), None);
        assert_eq!(app.active().unwrap().protocol, "http");
        app.open(graphql);
        app.open(format!("__collection__{cid}"));
        let snapshot = app.ws.metadata_snapshot();
        assert_eq!(snapshot.last_request_kind[&cid], RequestKind::Graphql);
        crate::storage::save(dir.path(), &app.ws).unwrap();
        drop(app);
        let mut restored = App::from_context(&ctx, dir.path().into()).unwrap();
        restored.new_transient_request(cid.clone(), None);
        assert_eq!(restored.active().unwrap().protocol, "graphql");
        restored.new_transient_request("another-collection".into(), None);
        assert_eq!(restored.active().unwrap().protocol, "http");
    }

    #[test]
    fn new_tab_uses_the_active_type_in_an_old_workspace_without_history() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, dir.path().into()).unwrap();
        let cid = app.active_collection_id();
        let active = app.ws.active.clone().unwrap();
        let request = app.ws.requests.iter_mut().find(|r| r.id == active).unwrap();
        request.protocol = "ws".into();
        request.url = "wss://example.test/private".into();
        request.auth.token = "must-not-copy".into();
        assert!(app.ws.last_request_kind.is_empty());
        app.actions.push(Action::NewTab(cid, None));
        app.handle_actions(&ctx);
        let request = app.active().unwrap();
        assert_eq!(request.protocol, "ws");
        assert!(request.url.is_empty() && request.auth.token.is_empty());
    }
}
