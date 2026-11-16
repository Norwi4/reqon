use crate::{
    model::{BodyKind, Request},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};

pub fn toolbar_width(ui: &Ui, request: &Request) -> f32 {
    w::text_width(ui, request.body_kind.label(), 12.)
        + 36.
        + if request.body_kind == BodyKind::Json {
            32.
        } else {
            0.
        }
}

fn body_icon(ui: &Ui, at: egui::Pos2, kind: &BodyKind, color: egui::Color32) {
    let svg: Option<(&str, &[u8])> = match kind {
        BodyKind::Form | BodyKind::Multipart => Some((
            "bytes://icons/BodyForms.svg",
            include_bytes!("../assets/icons/BodyForms.svg"),
        )),
        BodyKind::Json => Some((
            "bytes://icons/BodyBraces.svg",
            include_bytes!("../assets/icons/BodyBraces.svg"),
        )),
        BodyKind::Text => Some((
            "bytes://icons/BodyText.svg",
            include_bytes!("../assets/icons/BodyText.svg"),
        )),
        _ => None,
    };
    if let Some((name, bytes)) = svg {
        egui::Image::from_bytes(name, bytes)
            .tint(color)
            .paint_at(ui, Rect::from_center_size(at, vec2(16., 16.)));
    } else {
        w::icon(
            ui,
            at,
            match kind {
                BodyKind::Xml | BodyKind::Graphql => Icon::Code,
                BodyKind::Sparql => Icon::Database,
                BodyKind::Binary => Icon::File,
                _ => Icon::Close,
            },
            color,
            16.,
        );
    }
}

pub fn toolbar(ui: &mut Ui, r: Rect, request: &mut Request, t: Theme) -> (bool, Option<String>) {
    let mut changed = false;
    let mut error = None;
    let json = request.body_kind == BodyKind::Json;
    let width = w::text_width(ui, request.body_kind.label(), 12.) + 24.;
    let end = r.right() - if json { 32. } else { 0. };
    let select = w::dropdown(
        ui,
        rect(end - width, r.top() + 3., width, 26.),
        request.body_kind.label(),
        true,
        false,
        t,
    );
    egui::Popup::menu(&select)
        .align(egui::RectAlign::BOTTOM_END)
        .show(|ui| {
            ui.set_width(162.);
            ui.spacing_mut().item_spacing.y = 0.;
            for (group, kinds) in [
                ("FORM", vec![BodyKind::Multipart, BodyKind::Form]),
                (
                    "RAW",
                    vec![
                        BodyKind::Json,
                        BodyKind::Xml,
                        BodyKind::Text,
                        BodyKind::Sparql,
                    ],
                ),
                ("OTHER", vec![BodyKind::Binary, BodyKind::None]),
            ] {
                let heading = ui
                    .allocate_space(vec2(162., if group == "FORM" { 24. } else { 28. }))
                    .1;
                w::semibold(
                    ui,
                    pos2(heading.left() + 6., heading.center().y),
                    group,
                    10.,
                    t.muted,
                );
                for kind in kinds {
                    let row = ui.allocate_space(vec2(162., 28.2)).1;
                    let response =
                        ui.interact(row, ui.scope_id().with(kind.label()), Sense::click());
                    let selected = request.body_kind == kind;
                    if selected || response.hovered() {
                        ui.painter().rect_filled(
                            row,
                            4,
                            if selected {
                                t.accent.gamma_multiply(0.07)
                            } else {
                                t.hover
                            },
                        );
                    }
                    let color = if selected { t.accent } else { t.text };
                    body_icon(
                        ui,
                        pos2(row.left() + 17., row.center().y),
                        &kind,
                        if selected { color } else { t.muted },
                    );
                    w::label(
                        ui,
                        pos2(row.left() + 34., row.center().y),
                        kind.label(),
                        12.,
                        color,
                    );
                    if selected {
                        w::icon(
                            ui,
                            pos2(row.right() - 10., row.center().y),
                            Icon::Check,
                            t.accent,
                            14.,
                        );
                    }
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, kind.label())
                    });
                    if response.clicked() {
                        request.body_kind = kind;
                        changed = true;
                        ui.close();
                    }
                }
            }
        });
    if json {
        let area = rect(r.right() - 26., r.top() + 3., 26., 26.);
        let response = ui.interact(
            area,
            ui.scope_id().with("format-request-json"),
            Sense::click(),
        );
        response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Format JSON"));
        if response.hovered() {
            ui.painter().rect_filled(area, 4, t.hover);
        }
        egui::Image::from_bytes(
            "bytes://icons/FormatJson.svg",
            include_bytes!("../assets/icons/FormatJson.svg") as &[u8],
        )
        .tint(t.muted)
        .paint_at(ui, Rect::from_center_size(area.center(), vec2(16., 16.)));
        if response.on_hover_text("Format JSON").clicked() {
            match format_json(&request.body) {
                Ok(formatted) => {
                    changed |= request.body != formatted;
                    request.body = formatted;
                }
                Err(e) => error = Some(format!("Cannot format JSON: {e}")),
            }
        }
    }
    (changed, error)
}

/// Reindent tokens instead of reserializing values, preserving number precision,
/// string escapes, duplicate keys and key order exactly as entered.
pub fn format_json(source: &str) -> anyhow::Result<String> {
    crate::json_comments::format(source)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn formatting_preserves_tokens_and_rejects_invalid_input() {
        let raw = r#"{"n":123456789012345678901234567890,"n":1.2300e+04,"s":"\u0041\\\"","empty":[],"o":{},"nested":[true,{"v":null}]}"#;
        let formatted = format_json(raw).unwrap();
        assert!(formatted.contains("123456789012345678901234567890"));
        assert!(formatted.contains("1.2300e+04"));
        assert!(formatted.contains(r#""s": "\u0041\\\"""#));
        assert!(formatted.contains("\n  \"nested\": [\n    true,"));
        assert_eq!(format_json(&formatted).unwrap(), formatted);
        assert!(format_json("{invalid}").is_err());
        assert!(format_json("{} {}").is_err());
        assert_eq!(format_json(" ").unwrap(), " ");
    }
}
