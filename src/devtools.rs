use crate::{
    app::App,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Ui, pos2, vec2};

impl App {
    pub fn devtools(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        ui.painter().rect_filled(
            r,
            0,
            crate::theme::hex(if t.dark { 0x1f1f1f } else { 0xfafafa }),
        );
        w::line(ui, r.left_top(), r.right_top(), t.border);
        let mut x = r.left() + 16.;
        for (i, (name, icon)) in [
            ("Console", Icon::TerminalBox),
            ("Network", Icon::Network),
            ("Performance", Icon::Dashboard),
            ("Terminal", Icon::TerminalBox),
        ]
        .into_iter()
        .enumerate()
        {
            let width = w::text_width(ui, name, 12.) + 26.;
            w::icon(
                ui,
                pos2(x + 8., r.top() + 14.),
                icon,
                if self.devtools_tab == i {
                    t.accent
                } else {
                    t.muted
                },
                16.,
            );
            if w::tab(
                ui,
                rect(x + 22., r.top(), width - 22., 29.),
                name,
                self.devtools_tab == i,
                t,
            )
            .clicked()
            {
                self.devtools_tab = i;
            }
            x += width + 16.;
        }
        if w::icon_button(
            ui,
            rect(r.right() - 32., r.top() + 1., 26., 26.),
            Icon::Close,
            "Close Dev Tools",
            t,
        )
        .clicked()
        {
            self.devtools_open = false;
        }
        w::line(
            ui,
            pos2(r.left(), r.top() + 30.),
            pos2(r.right(), r.top() + 30.),
            t.border,
        );
        if self.devtools_tab == 3 {
            self.terminal_panel(
                ui,
                rect(r.left(), r.top() + 30., r.width(), r.height() - 30.),
            );
            return;
        }
        let mut child = w::child(
            ui,
            rect(
                r.left() + 12.,
                r.top() + 40.,
                r.width() - 24.,
                r.height() - 48.,
            ),
            "devtools-body",
        );
        match self.devtools_tab {
            0 => {
                egui::ScrollArea::both()
                    .stick_to_bottom(true)
                    .show(&mut child, |ui| {
                        let mut any = false;
                        if let Some(id) = &self.ws.active {
                            if let Some(reply) = self.tabs.get(id).and_then(|s| s.reply.as_ref()) {
                                for log in &reply.logs {
                                    ui.add(
                                        egui::Label::new(egui::RichText::new(log).monospace())
                                            .selectable(true),
                                    );
                                    any = true;
                                }
                            }
                        }
                        if !any {
                            ui.label(egui::RichText::new("No console output").color(t.muted));
                        }
                    });
            }
            1 => {
                egui::ScrollArea::both().show(&mut child, |ui| {
                    egui::Grid::new("network-grid")
                        .spacing(vec2(24., 12.))
                        .striped(true)
                        .show(ui, |ui| {
                            for title in ["Method", "URL", "Status", "Time", "Size"] {
                                ui.strong(title);
                            }
                            ui.end_row();
                            for h in self.ws.history.iter().rev() {
                                ui.label(
                                    egui::RichText::new(&h.request.method)
                                        .color(t.method(&h.request.method)),
                                );
                                ui.add(egui::Label::new(&h.request.url).selectable(true));
                                ui.label(h.reply.status.to_string());
                                ui.label(format!("{} ms", h.reply.elapsed_ms));
                                ui.label(format!("{} B", h.reply.size));
                                ui.end_row();
                            }
                        });
                });
            }
            2 => {
                let mut times = self.frame_times.iter().copied().collect::<Vec<_>>();
                times.sort_by(f32::total_cmp);
                let p95 = times.get(times.len() * 95 / 100).copied().unwrap_or(0.);
                let median = times.get(times.len() / 2).copied().unwrap_or(0.);
                child.label(format!("UI frame: {median:.2} ms median · {p95:.2} ms p95"));
                child.label(format!(
                    "{} requests · {} open tabs · {} active requests",
                    self.ws.requests.len(),
                    self.ws.open_tabs.len(),
                    self.running.len()
                ));
                child.label("Measurements cover layout and interaction processing in the last 120 rendered frames.");
            }
            _ => {}
        }
    }
}
