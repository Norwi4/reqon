use crate::{
    model::resolve,
    theme::Theme,
    widgets::{self as w},
};
use eframe::egui::{self, FontId, Rect, TextFormat, Ui, vec2};
use std::collections::BTreeMap;

/// Keep the template editable; substitutions are applied only when preparing a request.
pub fn editor(
    ui: &mut Ui,
    rect: Rect,
    id: egui::Id,
    value: &mut String,
    vars: &BTreeMap<String, String>,
    hint: &str,
    t: Theme,
) -> egui::Response {
    let popup_id = id.with("variable-completion");
    let keys = if egui::Popup::is_id_open(ui.ctx(), popup_id) && ui.memory(|m| m.has_focus(id)) {
        ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        })
    } else {
        (false, false, false)
    };
    let mut child = w::child(ui, rect, id);
    let mut layout = |ui: &Ui, buffer: &dyn egui::TextBuffer, _: f32| {
        let text = buffer.as_str();
        let mut job = egui::text::LayoutJob::default();
        let mut offset = 0;
        while let Some(start) = text[offset..].find("{{").map(|i| i + offset) {
            job.append(
                &text[offset..start],
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: t.text,
                    ..Default::default()
                },
            );
            let end = text[start + 2..]
                .find("}}")
                .map(|i| start + 4 + i)
                .unwrap_or(text.len());
            let token = &text[start..end];
            let resolved = resolve(token, vars);
            job.append(
                token,
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: if resolved.contains("{{") {
                        t.red
                    } else {
                        t.accent
                    },
                    ..Default::default()
                },
            );
            offset = end;
        }
        job.append(
            &text[offset..],
            0.,
            TextFormat {
                font_id: FontId::proportional(13.),
                color: t.text,
                ..Default::default()
            },
        );
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };
    let mut output = egui::TextEdit::singleline(value)
        .id(id)
        .frame(egui::Frame::NONE)
        .font(FontId::proportional(13.))
        .hint_text(hint)
        .margin(egui::Margin::ZERO)
        .min_size(vec2(0., rect.height()))
        .desired_width(rect.width())
        .align(egui::Align2::LEFT_CENTER)
        .layouter(&mut layout)
        .show(&mut child);
    let cursor = output
        .cursor_range
        .or_else(|| output.state.cursor.char_range())
        .map(|r| r.primary.index)
        .unwrap_or_default();
    let byte = value
        .char_indices()
        .nth(cursor.into())
        .map(|(i, _)| i)
        .unwrap_or(value.len());
    let prefix = &value[..byte];
    let query = prefix
        .rfind("{{")
        .filter(|&start| !prefix[start..].contains("}}"));
    if query.is_none() {
        egui::Popup::close_id(ui.ctx(), popup_id);
    }
    if let Some(start) = query {
        let filter = prefix[start + 2..].trim().to_lowercase();
        let options: Vec<_> = vars
            .keys()
            .filter(|key| key.to_lowercase().contains(&filter))
            .take(8)
            .cloned()
            .collect();
        let choice_id = id.with("variable-choice");
        let mut choice = ui
            .ctx()
            .data_mut(|d| d.get_temp::<usize>(choice_id))
            .unwrap_or_default();
        if output.response.changed() {
            choice = 0;
        }
        if !options.is_empty() {
            if keys.0 {
                choice = (choice + 1) % options.len();
            }
            if keys.1 {
                choice = (choice + options.len() - 1) % options.len();
            }
            choice = choice.min(options.len() - 1);
        }
        ui.ctx().data_mut(|d| d.insert_temp(choice_id, choice));
        let mut selected = if keys.2 {
            options.get(choice).cloned()
        } else {
            None
        };
        egui::Popup::from_response(&output.response)
            .id(popup_id)
            .open_memory(
                if output.response.has_focus()
                    && (output.response.changed() || output.response.gained_focus())
                {
                    Some(egui::SetOpenCommand::Bool(true))
                } else {
                    None
                },
            )
            .width(rect.width().clamp(220., 400.))
            .show(|ui| {
                ui.set_min_width(220.);
                ui.label(
                    egui::RichText::new("Environment variables")
                        .size(11.)
                        .color(t.muted),
                );
                let mut count = 0;
                for (i, key) in options.iter().enumerate() {
                    if ui
                        .selectable_label(choice == i, format!("{{{{{key}}}}}"))
                        .clicked()
                    {
                        selected = Some(key.clone());
                    }
                    count += 1;
                }
                if count == 0 {
                    ui.weak("No matching variables. Add one in Environments.");
                }
            });
        if let Some(key) = selected {
            egui::Popup::close_id(ui.ctx(), popup_id);
            let end = if value[byte..].starts_with("}}") {
                byte + 2
            } else {
                byte
            };
            let replacement = format!("{{{{{key}}}}}");
            value.replace_range(start..end, &replacement);
            let cursor = value[..start + replacement.len()].chars().count();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::one(
                    egui::text::CCursor::new(cursor),
                )));
            output.state.store(ui.ctx(), id);
            output.response.request_focus();
            output.response.mark_changed();
        }
    }
    egui::Response::clone(&output.response).on_hover_text(
        "Use {{variable}} from the active environment. Undefined variables are shown in red.",
    )
}
