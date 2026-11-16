//! Variable editor: stable rows, a persistent blank tail and local metadata actions.
use crate::{
    model::Pair,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

#[derive(Clone, Default)]
struct RowsState {
    ids: Vec<u64>,
    next: u64,
    drag: Option<usize>,
}

fn populated(p: &Pair) -> bool {
    !p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty()
}

pub fn draw(ui: &mut Ui, rows: &mut Vec<Pair>, t: Theme) -> bool {
    ui.spacing_mut().item_spacing.y = 0.;
    if rows.last().is_none_or(populated) {
        rows.push(Pair::default());
    }
    let state_id = ui.scope_id().with("variable-row-state");
    let mut state = ui
        .ctx()
        .data_mut(|d| d.get_temp::<RowsState>(state_id))
        .unwrap_or_default();
    while state.ids.len() < rows.len() {
        state.ids.push(state.next);
        state.next += 1;
    }
    state.ids.truncate(rows.len());
    let width = ui.available_width().max(280.);
    let split = (width * 0.42).max(145.);
    let header = ui.allocate_space(vec2(width, 33.)).1;
    ui.painter().rect_filled(header, 0, t.hover);
    for (text, x) in [("Variable", 48.), ("Value", split + 10.)] {
        w::semibold(
            ui,
            pos2(header.left() + x, header.center().y),
            text,
            13.,
            t.text,
        );
    }
    w::line(ui, header.left_bottom(), header.right_bottom(), t.border);
    let mut changed = false;
    let mut remove = None;
    let mut scene = Vec::new();
    let mut bottom = header.bottom();
    let mut drag_stopped = false;
    for (index, p) in rows.iter_mut().enumerate() {
        let row = ui.allocate_space(vec2(width, 35.)).1;
        bottom = row.bottom();
        let has_content = populated(p);
        scene.push((row, has_content));
        ui.push_id(state.ids[index], |ui| {
            let hovered = ui.rect_contains_pointer(row);
            let key_id = ui.scope_id().with("variable-name");
            let value_id = ui.scope_id().with("variable-value");
            let active = hovered
                || ui.memory(|m| m.has_focus(key_id) || m.has_focus(value_id))
                || state.drag == Some(index)
                || egui::Popup::is_id_open(
                    ui.ctx(),
                    ui.scope_id().with("Variable actions").with("popup"),
                );
            if hovered || state.drag == Some(index) {
                ui.painter().rect_filled(row.shrink(0.5), 0, t.hover);
            }
            if has_content {
                if active || !p.enabled {
                    changed |= w::table_checkbox(
                        ui,
                        rect(row.left() + 21., row.top(), 22., row.height()),
                        &mut p.enabled,
                        "Include variable",
                        t,
                    );
                }
                let handle_rect = rect(row.left(), row.top(), 20., row.height());
                let handle = ui.interact(
                    handle_rect,
                    ui.scope_id().with("move-variable"),
                    Sense::drag(),
                );
                handle.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Move variable")
                });
                if active {
                    for x in [8., 12.] {
                        for y in [-4., 0., 4.] {
                            ui.painter().circle_filled(
                                pos2(row.left() + x, row.center().y + y),
                                1.,
                                t.muted,
                            );
                        }
                    }
                }
                if handle.drag_started() {
                    state.drag = Some(index);
                }
                drag_stopped |= handle.drag_stopped();
                handle
                    .on_hover_text("Drag to move variable")
                    .on_hover_cursor(egui::CursorIcon::Grab);
            }
            changed |= ui
                .put(
                    rect(
                        row.left() + 48.,
                        row.top() + 1.,
                        split - 56.,
                        row.height() - 2.,
                    ),
                    w::table_text(ui, "variable-name", &mut p.key, row.height() - 2.)
                        .hint_text(if has_content { "" } else { "Add variable" }),
                )
                .on_hover_text(&p.description)
                .changed();
            let value_left = row.left() + split + 10.;
            let reserve = if p.secret { 80. } else { 64. };
            changed |= ui
                .put(
                    rect(
                        value_left,
                        row.top() + 1.,
                        row.right() - value_left - reserve,
                        row.height() - 2.,
                    ),
                    w::table_text(ui, "variable-value", &mut p.value, row.height() - 2.)
                        .password(p.secret)
                        .hint_text("Value"),
                )
                .changed();
            if p.secret {
                w::icon(
                    ui,
                    pos2(row.right() - 68., row.center().y),
                    Icon::Lock,
                    t.muted,
                    12.,
                );
            }
            if populated(p) && active {
                let more = w::icon_button(
                    ui,
                    rect(row.right() - 56., row.center().y - 13., 26., 26.),
                    Icon::Dots,
                    "Variable actions",
                    t,
                );
                egui::Popup::menu(&more)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.label("Variable type");
                        changed |= ui.selectable_value(&mut p.secret, false, "Text").changed();
                        changed |= ui
                            .selectable_value(&mut p.secret, true, "Secret (local only)")
                            .changed();
                        ui.separator();
                        ui.label("Description");
                        changed |= ui
                            .add(
                                egui::TextEdit::singleline(&mut p.description)
                                    .id(ui.scope_id().with("description"))
                                    .desired_width(260.),
                            )
                            .changed();
                    });
                if w::table_delete_button(
                    ui,
                    rect(row.right() - 28., row.center().y - 13., 26., 26.),
                    "Delete variable",
                    t,
                )
                .clicked()
                {
                    remove = Some(index);
                }
            }
        });
        w::line(ui, row.left_bottom(), row.right_bottom(), t.border);
    }
    ui.painter().rect_stroke(
        Rect::from_min_max(header.min, pos2(header.right(), bottom)),
        3,
        Stroke::new(1., t.border),
        StrokeKind::Inside,
    );
    w::line(
        ui,
        pos2(header.left() + split, header.top()),
        pos2(header.left() + split, bottom),
        t.border,
    );
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.drag = None;
    }
    if let Some(index) = state.drag {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            if let Some((target, (r, content))) =
                scene.iter().enumerate().find(|(_, (r, _))| r.contains(pos))
            {
                let after = *content && pos.y > r.center().y;
                let before = target + usize::from(after);
                let y = if after { r.bottom() } else { r.top() };
                ui.painter()
                    .hline(r.x_range(), y, Stroke::new(2., t.accent));
                if drag_stopped && index < rows.len() {
                    let target = before.saturating_sub(usize::from(before > index));
                    if target != index {
                        let value = rows.remove(index);
                        rows.insert(target, value);
                        let id = state.ids.remove(index);
                        state.ids.insert(target, id);
                        changed = true;
                    }
                }
            }
        }
    }
    if drag_stopped || ui.input(|i| i.pointer.primary_released()) {
        state.drag = None;
    }
    if let Some(index) = remove {
        rows.remove(index);
        state.ids.remove(index);
        state.drag = None;
        changed = true;
    }
    ui.ctx().data_mut(|d| d.insert_temp(state_id, state));
    changed
}
