use crate::{
    generated_headers::preview,
    model::*,
    theme::Theme,
    widgets::{self as w, rect},
};
use eframe::egui::{self, Stroke, StrokeKind, Ui, pos2, vec2};
use std::collections::BTreeMap;

pub fn draw(ui: &mut Ui, request: &Request, vars: &BTreeMap<String, String>, t: Theme) {
    let rows = preview(request, vars);
    egui::CollapsingHeader::new(format!("Auto-generated headers ({})", rows.len()))
        .id_salt("generated-headers")
        .default_open(true)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            let width = ui.available_width();
            let name_width = (width * 0.38).clamp(110., 205.);
            let start = ui.cursor().min;
            let mut bottom = start.y;
            for (key, value) in rows {
                let row = ui.allocate_space(vec2(width, 30.)).1;
                bottom = row.bottom();
                for (text, x, wid) in [
                    (&key, row.left() + 10., name_width - 20.),
                    (
                        &value,
                        row.left() + name_width + 10.,
                        width - name_width - 20.,
                    ),
                ] {
                    let mut cell = w::child(
                        ui,
                        rect(x, row.top() + 5., wid.max(20.), 22.),
                        (&key, x.to_bits()),
                    );
                    cell.add(
                        egui::Label::new(egui::RichText::new(text).size(12.).color(t.muted))
                            .truncate(),
                    )
                    .on_hover_text(text);
                }
                w::line(ui, row.left_bottom(), row.right_bottom(), t.border);
            }
            ui.painter().rect_stroke(
                egui::Rect::from_min_max(start, pos2(start.x + width, bottom)),
                3,
                Stroke::new(1., t.border),
                StrokeKind::Inside,
            );
            w::line(
                ui,
                pos2(start.x + name_width, start.y),
                pos2(start.x + name_width, bottom),
                t.border,
            );
            ui.add_space(8.);
            ui.label(
                egui::RichText::new("Add a custom header below to override a generated value.")
                    .size(11.)
                    .color(t.muted),
            );
        });
    ui.add_space(12.);
}
