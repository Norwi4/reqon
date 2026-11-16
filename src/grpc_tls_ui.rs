use crate::{app::App, model::Request, widgets::rect};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, Ui, vec2};

impl App {
    pub fn grpc_tls_button(&mut self, ui: &mut Ui, host: Rect, request: &mut Request) -> bool {
        let t = self.theme;
        let mut enabled = request.grpc_tls_enabled(&self.vars(request));
        let area = rect(host.left() + 3., host.top() + 2., 30., host.height() - 4.);
        let response = ui.interact(
            area,
            ui.scope_id().with(("grpc-tls", &request.id)),
            Sense::click(),
        );
        if response.clicked() {
            enabled = !enabled;
            request.set_grpc_tls(enabled);
            ui.ctx().request_repaint();
        }
        let action = if enabled { "Disable TLS" } else { "Enable TLS" };
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, action));
        if response.hovered() {
            ui.painter().rect_filled(area, 4, t.hover);
        }
        if response.has_focus() {
            ui.painter()
                .rect_stroke(area, 4, Stroke::new(1., t.accent), StrokeKind::Inside);
        }
        let (name, bytes): (_, &[u8]) = if enabled {
            (
                "bytes://icons/GrpcTls.svg",
                include_bytes!("../assets/icons/GrpcTls.svg"),
            )
        } else {
            (
                "bytes://icons/GrpcTlsOff.svg",
                include_bytes!("../assets/icons/GrpcTlsOff.svg"),
            )
        };
        egui::Image::from_bytes(name, bytes)
            .tint(if enabled { t.text } else { t.muted })
            .paint_at(ui, Rect::from_center_size(area.center(), vec2(16., 16.)));
        let changed = response.clicked();
        response.on_hover_ui(|ui| {
            ui.set_width(270.);
            ui.label(egui::RichText::new(action).strong().size(13.));
            ui.label(
                egui::RichText::new(if enabled {
                    "TLS is enabled for this request."
                } else {
                    "TLS is disabled for this request."
                })
                .size(12.),
            );
            ui.add(egui::Label::new(egui::RichText::new(if enabled {
                "Disable TLS to invoke the method over an unsecured connection."
            } else {
                "Enable TLS to use a secured connection for server reflection and method calls."
            }).size(12.).color(t.muted)).wrap());
            ui.add_space(5.);
            if ui
                .link("Configure additional CA and client certificates")
                .clicked()
            {
                self.tabs.entry(request.id.clone()).or_default().request_tab = 10;
                ui.close();
            }
        });
        changed
    }
}
