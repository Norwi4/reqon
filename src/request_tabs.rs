use crate::{
    app::{Action, App},
    model::RequestKind,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use std::collections::HashMap;

const GAP: f32 = 3.;

struct Tab {
    id: String,
    name: String,
    badge: String,
    badge_width: f32,
    width: f32,
    active: bool,
    dirty: bool,
}

#[derive(Clone)]
struct Drag {
    id: String,
    origin: Pos2,
    grab: f32,
    target: usize,
    started: bool,
    epoch: u64,
}

#[derive(Clone, Default)]
struct Animation {
    drag: Option<Drag>,
    positions: HashMap<String, f32>,
    velocities: HashMap<String, f32>,
    floating: Option<String>,
    scroll: f32,
    scroll_target: Option<f32>,
}

pub fn cancel_drag(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let key = Id::unique("tab-drag-cancel-epoch");
        let epoch = d.get_temp::<u64>(key).unwrap_or(0);
        d.insert_temp(key, epoch.wrapping_add(1));
    });
}

fn order(tabs: &[Tab], drag: Option<&Drag>) -> Vec<usize> {
    let mut indices: Vec<_> = (0..tabs.len()).collect();
    if let Some(drag) = drag.filter(|d| d.started) {
        if let Some(from) = tabs.iter().position(|t| t.id == drag.id) {
            indices.remove(from);
            indices.insert(drag.target.min(indices.len()), from);
        }
    }
    indices
}

fn targets(tabs: &[Tab], order: &[usize]) -> Vec<f32> {
    let mut x = 0.;
    order
        .iter()
        .map(|&i| {
            let left = x;
            x += tabs[i].width + GAP;
            left
        })
        .collect()
}

fn approach(current: &mut f32, target: f32, dt: f32) -> bool {
    if (*current - target).abs() < 0.08 {
        *current = target;
        false
    } else {
        *current += (target - *current) * (1. - (-dt.min(0.05) / 0.045).exp());
        true
    }
}

fn spring(current: &mut f32, velocity: &mut f32, target: f32, dt: f32) -> bool {
    if (*current - target).abs() < 0.08 && velocity.abs() < 0.8 {
        *current = target;
        *velocity = 0.;
        return false;
    }
    // Analytic damped spring: neighbours retain momentum on direction changes,
    // cover most of the distance in ~120 ms and have only a tiny soft overshoot.
    let frequency = 32_f32;
    let damping = 0.82 * frequency;
    let oscillation = (frequency * frequency - damping * damping).sqrt();
    let dt = dt.clamp(0., 0.05);
    let decay = (-damping * dt).exp();
    let (sin, cos) = (oscillation * dt).sin_cos();
    let offset = *current - target;
    let next = decay * (offset * cos + (*velocity + damping * offset) / oscillation * sin);
    *velocity = decay
        * (*velocity * cos
            - (damping * *velocity + frequency * frequency * offset) / oscillation * sin);
    *current = target + next;
    true
}

impl App {
    pub(crate) fn request_tabs(&mut self, ui: &mut Ui, r: Rect) {
        let t = self.theme;
        let key = ui.scope_id().with("request-tab-animation");
        let mut animation = ui
            .ctx()
            .data_mut(|d| d.get_temp::<Animation>(key))
            .unwrap_or_default();
        let tabs: Vec<_> = self
            .ws
            .open_tabs
            .iter()
            .filter_map(|id| {
                let (name, badge) = if id == crate::preferences::TAB_ID {
                    ("Preferences".into(), String::new())
                } else if let Some((_, path)) = crate::file_explorer::parse_id(id) {
                    (
                        path.rsplit('/').next().unwrap_or(path).into(),
                        String::new(),
                    )
                } else if let Some(target) = crate::environment_tabs::Target::parse(id) {
                    (target.name, String::new())
                } else if id.starts_with("__environments__") {
                    ("Environments".into(), String::new())
                } else if id.starts_with("__git__") {
                    ("Git UI".into(), String::new())
                } else if id.starts_with("__collection__") {
                    ("Collection".into(), String::new())
                } else if let Some(draft) = self.ws.collection_drafts.get(id) {
                    (draft.name.clone(), String::new())
                } else {
                    let request = self.ws.requests.iter().find(|q| &q.id == id)?;
                    (request.name.clone(), request.badge())
                };
                let badge_width = if id.starts_with("__") {
                    14.
                } else {
                    ((w::text_width(ui, &badge, 11.) + badge.chars().count() as f32 * 0.22) * 64.)
                        .ceil()
                        / 64.
                };
                let name_width = (w::text_width(ui, &name, 13.) * 64.).ceil() / 64.;
                Some(Tab {
                    id: id.clone(),
                    name,
                    badge,
                    badge_width,
                    width: (name_width + badge_width + 24.).clamp(80., 180.),
                    active: self.ws.active.as_deref() == Some(id),
                    dirty: self.dirty(id),
                })
            })
            .collect();
        animation
            .positions
            .retain(|id, _| tabs.iter().any(|tab| &tab.id == id));
        animation
            .velocities
            .retain(|id, _| tabs.iter().any(|tab| &tab.id == id));
        let (pointer, pressed, down, released, escape, focused, dt) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.events.iter().find_map(|e| match e {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                }),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.key_pressed(egui::Key::Escape),
                i.focused,
                i.stable_dt,
            )
        });
        let epoch = ui
            .ctx()
            .data(|d| d.get_temp::<u64>(Id::unique("tab-drag-cancel-epoch")))
            .unwrap_or(0);
        let mut suppress_click = animation.drag.as_ref().is_some_and(|d| d.started);
        if animation
            .drag
            .as_ref()
            .is_some_and(|d| d.epoch != epoch || !tabs.iter().any(|t| t.id == d.id))
            || escape
            || !focused
            || (!down && !released)
        {
            animation.drag = None;
        }
        let active_key = self.ws.active.clone().unwrap_or_default();
        let reveal_active = ui.ctx().data_mut(|d| {
            let key = key.with("active");
            let changed = d.get_temp::<String>(key).as_deref() != Some(&active_key);
            if changed {
                d.insert_temp(key, active_key);
            }
            changed
        });
        let mut active_span = None;
        let mut reordered = None;
        let width = tabs.iter().map(|tab| tab.width + GAP).sum::<f32>();
        let overflow = width + 32. > r.width() - 11.;
        let controls = if overflow { 64. } else { 0. };
        let viewport_width = width
            .min((r.width() - 11. - 32. - controls).max(1.))
            .max(1.);
        let viewport_rect = rect(
            r.left() + 11. + if overflow { 32. } else { 0. },
            r.top(),
            viewport_width,
            r.height(),
        );
        let add_rect = rect(viewport_rect.right() + 3., r.top(), 28., 35.);
        let max_scroll = (width - viewport_width).max(0.);
        animation.scroll = animation.scroll.clamp(0., max_scroll);
        if overflow {
            for (left, button) in [
                (true, rect(r.left() + 8., r.top(), 28., 35.)),
                (false, rect(add_rect.right() + 4., r.top(), 28., 35.)),
            ] {
                let enabled = if left {
                    animation.scroll > 0.5
                } else {
                    animation.scroll < max_scroll - 0.5
                };
                let response = ui.interact(
                    button,
                    key.with(if left { "scroll-left" } else { "scroll-right" }),
                    Sense::click(),
                );
                if response.hovered() && enabled {
                    ui.painter()
                        .rect_filled(button.shrink2(vec2(1., 5.)), 4, t.hover);
                }
                let center = button.center() - vec2(0., 1.5);
                let sign = if left { -1. } else { 1. };
                ui.painter().add(egui::Shape::line(
                    vec![
                        center + vec2(-sign * 2.25, -4.5),
                        center + vec2(sign * 2.25, 0.),
                        center + vec2(-sign * 2.25, 4.5),
                    ],
                    Stroke::new(
                        1.125,
                        if enabled {
                            t.muted
                        } else {
                            t.faint.gamma_multiply(0.4)
                        },
                    ),
                ));
                if response.clicked() && enabled && !suppress_click {
                    animation.scroll_target = Some(
                        (animation.scroll_target.unwrap_or(animation.scroll)
                            + if left { -120. } else { 120. })
                        .clamp(0., max_scroll),
                    );
                }
                response.on_hover_text(if left {
                    "Scroll tabs left"
                } else {
                    "Scroll tabs right"
                });
            }
        }
        if reveal_active && !suppress_click {
            if let Some(index) = tabs.iter().position(|tab| tab.active) {
                let left: f32 = tabs[..index].iter().map(|tab| tab.width + GAP).sum();
                let right = left + tabs[index].width + GAP;
                if left < animation.scroll {
                    animation.scroll_target = Some(left);
                } else if right > animation.scroll + viewport_width {
                    animation.scroll_target = Some(right - viewport_width);
                }
            }
        }
        if ui.input(|i| {
            i.events
                .iter()
                .any(|e| matches!(e, egui::Event::MouseWheel { .. }))
        }) || suppress_click
        {
            animation.scroll_target = None;
        }
        if let Some(target) = animation.scroll_target {
            if approach(&mut animation.scroll, target.clamp(0., max_scroll), dt) {
                ui.ctx().request_repaint();
            } else {
                animation.scroll_target = None;
            }
        }
        let mut child = w::child(ui, viewport_rect, "request-tabs");
        let output = egui::ScrollArea::horizontal()
            .horizontal_scroll_offset(animation.scroll)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .scroll_source(egui::scroll_area::ScrollSource {
                drag: egui::scroll_area::DragScroll::Never,
                ..Default::default()
            })
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                let all = ui.allocate_space(vec2(width, 37.)).1;
                let viewport = ui.clip_rect().intersect(r);
                if let (Some(drag), Some(pointer)) = (&mut animation.drag, pointer) {
                    drag.target = drag.target.min(tabs.len().saturating_sub(1));
                    if !drag.started && pointer.distance(drag.origin) > 6. && (down || released) {
                        drag.started = true;
                        animation.floating = Some(drag.id.clone());
                    }
                    if drag.started {
                        suppress_click = true;
                        // Cross the centre of the next neighbour before changing the
                        // slot. This hysteresis also works for unequal tab widths.
                        for _ in 0..tabs.len() {
                            let indices = order(&tabs, Some(drag));
                            let positions = targets(&tabs, &indices);
                            let source = indices[drag.target];
                            let center =
                                pointer.x - all.left() - drag.grab + tabs[source].width * 0.5;
                            if drag.target + 1 < indices.len()
                                && center
                                    > positions[drag.target + 1]
                                        + tabs[indices[drag.target + 1]].width * 0.5
                            {
                                drag.target += 1;
                            } else if drag.target > 0
                                && center
                                    < positions[drag.target - 1]
                                        + tabs[indices[drag.target - 1]].width * 0.5
                            {
                                drag.target -= 1;
                            } else {
                                break;
                            }
                        }
                    }
                }
                let mut indices = order(&tabs, animation.drag.as_ref());
                if released {
                    if let Some(drag) = animation.drag.take().filter(|d| d.started) {
                        if pointer.is_some_and(|p| r.expand2(vec2(12., 32.)).contains(p)) {
                            reordered = Some((
                                drag.id.clone(),
                                indices
                                    .iter()
                                    .map(|&i| tabs[i].id.clone())
                                    .collect::<Vec<_>>(),
                            ));
                        } else {
                            indices = order(&tabs, None);
                        }
                    }
                }
                let dragging = animation.drag.as_ref().is_some_and(|d| d.started);
                let mut moving = false;
                let positions = targets(&tabs, &indices);
                let mut floating = None;
                for (slot, &index) in indices.iter().enumerate() {
                    let tab = &tabs[index];
                    let target = positions[slot];
                    let x = animation.positions.entry(tab.id.clone()).or_insert(target);
                    let velocity = animation.velocities.entry(tab.id.clone()).or_default();
                    let dragged = animation
                        .drag
                        .as_ref()
                        .filter(|d| d.started && d.id == tab.id);
                    if let (Some(drag), Some(pointer)) = (dragged, pointer) {
                        *x = (pointer.x - drag.grab).clamp(
                            viewport.left(),
                            (viewport.right() - tab.width).max(viewport.left()),
                        ) - all.left();
                        *velocity = 0.;
                        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                    } else {
                        moving |= spring(x, velocity, target, dt);
                    }
                    let row = rect(all.left() + *x, all.top(), tab.width, 37.);
                    let response =
                        ui.interact(row, ui.scope_id().with(&tab.id), Sense::click_and_drag());
                    let close = rect(row.right() - 26., row.top() + 7., 22., 22.);
                    if let Some(origin) = pressed.filter(|p| {
                        row.intersect(viewport).contains(*p)
                            && !close.contains(*p)
                            && ui.ctx().layer_id_at(*p) == Some(ui.layer_id())
                    }) {
                        if animation.drag.is_none() {
                            animation.drag = Some(Drag {
                                id: tab.id.clone(),
                                origin,
                                grab: origin.x - row.left(),
                                target: slot,
                                started: false,
                                epoch,
                            });
                        }
                    }
                    let hovered =
                        !suppress_click && (response.hovered() || ui.rect_contains_pointer(row));
                    if !suppress_click {
                        if response.middle_clicked() {
                            self.actions.push(Action::Close(tab.id.clone()));
                        }
                        if response.clicked() {
                            self.actions.push(Action::Select(tab.id.clone()));
                        }
                    }
                    if animation.floating.as_deref() == Some(&tab.id) {
                        if !dragging && *x == target && *velocity == 0. {
                            animation.floating = None;
                        } else {
                            floating = Some((index, row, hovered));
                            continue;
                        }
                    }
                    if paint_tab(ui, tab, row, hovered, !suppress_click, t) {
                        self.actions.push(Action::Close(tab.id.clone()));
                    }
                    if tab.active && row.intersects(viewport) {
                        active_span = Some((
                            row.left().max(viewport.left()),
                            row.right().min(viewport.right()),
                        ));
                    }
                }
                if let Some((index, row, hovered)) = floating {
                    // Move the complete tab along the same baseline, preserving
                    // its active outline and the curved joins to the separator.
                    let mut overlay = w::child(ui, viewport, "floating-tab");
                    overlay.set_clip_rect(viewport);
                    if paint_tab(&mut overlay, &tabs[index], row, hovered, !suppress_click, t) {
                        self.actions.push(Action::Close(tabs[index].id.clone()));
                    }
                    if tabs[index].active && row.intersects(viewport) {
                        active_span = Some((
                            row.left().max(viewport.left()),
                            row.right().min(viewport.right()),
                        ));
                    }
                }
                if moving {
                    ui.ctx().request_repaint();
                }
                if dragging {
                    if let Some(p) =
                        pointer.filter(|p| p.y >= r.top() - 32. && p.y <= r.bottom() + 32.)
                    {
                        let edge = 36.;
                        let speed = if p.x < viewport.left() + edge {
                            ((viewport.left() + edge - p.x) / edge).min(1.)
                        } else if p.x > viewport.right() - edge {
                            -((p.x - viewport.right() + edge) / edge).min(1.)
                        } else {
                            0.
                        };
                        if speed != 0. {
                            ui.scroll_with_delta(vec2(speed * dt.min(0.05) * 650., 0.));
                            ui.ctx().request_repaint();
                        }
                    }
                }
            });
        animation.scroll = output.state.clamped_offset().x;
        let add = w::icon_button(ui, add_rect, Icon::Plus, "New tab (Ctrl+T)", t);
        if add.clicked() && !suppress_click {
            self.actions
                .push(Action::NewTab(self.active_collection_id(), None));
        }
        add.context_menu(|ui| {
            for kind in RequestKind::ALL {
                if ui.button(kind.label()).clicked() {
                    self.actions
                        .push(Action::NewTab(self.active_collection_id(), Some(kind)));
                    ui.close();
                }
            }
        });
        if let Some((from, order)) = reordered {
            let old = self.ws.open_tabs.clone();
            if let Some(index) = self.ws.open_tabs.iter().position(|id| id == &from) {
                self.ws.open_tabs.remove(index);
                let next = order
                    .iter()
                    .position(|id| id == &from)
                    .and_then(|i| order.get(i + 1));
                let target = next
                    .and_then(|next| self.ws.open_tabs.iter().position(|id| id == next))
                    .unwrap_or(self.ws.open_tabs.len());
                self.ws.open_tabs.insert(target, from);
                if self.ws.open_tabs != old {
                    self.mark_changed();
                }
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(key, animation));
        separator(ui, r, active_span, t);
    }
}

fn paint_tab(
    ui: &mut Ui,
    tab: &Tab,
    row: Rect,
    hovered: bool,
    interactive: bool,
    t: Theme,
) -> bool {
    let active = tab.active;
    let bg = rect(
        row.left(),
        row.top() + if active { 1. } else { 2. },
        row.width(),
        if active { 38. } else { 32. },
    );
    let color = if active { t.bg } else { t.surface };
    ui.painter().rect(
        bg,
        egui::CornerRadius {
            nw: 7,
            ne: 7,
            sw: if active { 0 } else { 4 },
            se: if active { 0 } else { 4 },
        },
        color,
        Stroke::new(
            1.,
            if active {
                t.strong_border
            } else {
                Color32::TRANSPARENT
            },
        ),
        StrokeKind::Inside,
    );
    let tt = rect(
        row.left() + 9.,
        row.top() + if active { 2. } else { 3. },
        row.width() - 18.,
        30.,
    );
    let sub = w::child(ui, tt, (&tab.id, "tab-label"));
    if tab.id.starts_with("__") {
        w::icon(
            &sub,
            pos2(tt.left() + 6., tt.center().y),
            if tab.id.starts_with("__collection__") || tab.id == crate::preferences::TAB_ID {
                Icon::Settings
            } else if tab.id.starts_with("__git__") {
                Icon::GitBrand
            } else if tab.id.starts_with("__folder__") {
                Icon::Folder
            } else if tab.id.starts_with("__file__") {
                Icon::File
            } else {
                Icon::Globe
            },
            t.accent,
            13.,
        );
    }
    w::tracked_label(
        &sub,
        pos2(tt.left(), tt.center().y),
        &tab.badge,
        11.,
        t.method(&tab.badge),
        0.22,
    );
    w::elided_styled_label(
        &sub,
        rect(
            tt.left() + tab.badge_width + 4.,
            tt.top(),
            row.right() - 11. - tt.left() - tab.badge_width - 4.,
            tt.height(),
        ),
        &tab.name,
        13.,
        t.text,
        tab.id.starts_with("__collection__") || tab.id.starts_with("__folder__"),
    );
    if hovered || tab.dirty {
        let fade = rect(row.right() - 44., row.top() + 3., 43., 30.);
        let solid_x = fade.left() + 17.6;
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(fade.left_top(), Color32::TRANSPARENT);
        mesh.colored_vertex(fade.left_bottom(), Color32::TRANSPARENT);
        mesh.colored_vertex(pos2(solid_x, fade.top()), color);
        mesh.colored_vertex(pos2(solid_x, fade.bottom()), color);
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(1, 3, 2);
        mesh.add_colored_rect(
            Rect::from_min_max(pos2(solid_x, fade.top()), fade.right_bottom()),
            color,
        );
        ui.painter().add(egui::Shape::mesh(mesh));
    }
    if tab.dirty && !hovered {
        ui.painter()
            .circle_filled(pos2(row.right() - 15., row.top() + 18.), 3., t.accent);
    }
    interactive
        && hovered
        && w::tab_close_button(
            ui,
            rect(row.right() - 26., row.top() + 7., 22., 22.),
            &tab.name,
            t,
        )
        .clicked()
}

fn separator(ui: &Ui, r: Rect, active: Option<(f32, f32)>, t: Theme) {
    if let Some((left, right)) =
        active.filter(|(left, right)| *right > r.left() && *left < r.right())
    {
        w::line(
            ui,
            r.left_bottom(),
            pos2((left - 6.).max(r.left()), r.bottom()),
            t.strong_border,
        );
        w::line(
            ui,
            pos2((right + 6.).min(r.right()), r.bottom()),
            r.right_bottom(),
            t.strong_border,
        );
        for (x, direction) in [(left, -1.), (right, 1.)] {
            ui.painter()
                .rect_filled(rect(x - 1., r.bottom() - 6., 2., 8.), 0, t.bg);
            let points = (0..=12)
                .map(|i| {
                    let theta = i as f32 / 12. * std::f32::consts::FRAC_PI_2;
                    pos2(
                        x + direction * 6. - direction * 6. * theta.cos(),
                        r.bottom() - 6. + 6. * theta.sin(),
                    )
                })
                .collect();
            ui.painter()
                .add(egui::Shape::line(points, Stroke::new(1., t.strong_border)));
        }
    } else {
        w::line(ui, r.left_bottom(), r.right_bottom(), t.strong_border);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Request;

    struct Harness {
        app: App,
        ctx: egui::Context,
        time: f64,
        shapes: Vec<egui::epaint::ClippedShape>,
        _root: tempfile::TempDir,
    }
    impl Harness {
        fn new(count: usize) -> Self {
            let root = tempfile::tempdir().unwrap();
            let ctx = egui::Context::default();
            let mut app = App::from_context(&ctx, root.path().to_owned()).unwrap();
            app.disk_sync = None;
            app.ws.preferences.autosave = false;
            app.ws.requests = (0..count)
                .map(|i| Request {
                    id: format!("t{i}"),
                    name: format!("Tab {i}"),
                    ..Default::default()
                })
                .collect();
            app.ws.open_tabs = app.ws.requests.iter().map(|r| r.id.clone()).collect();
            app.ws.active = Some("t0".into());
            let mut this = Self {
                app,
                ctx,
                time: 0.,
                shapes: vec![],
                _root: root,
            };
            for _ in 0..3 {
                this.frame(vec![]);
            }
            this
        }
        fn frame(&mut self, events: Vec<egui::Event>) -> Animation {
            self.time += 1. / 60.;
            let mut state = Animation::default();
            let mut output = self.ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect(0., 0., 800., 500.)),
                    time: Some(self.time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    self.app.request_tabs(ui, rect(0., 0., 520., 38.));
                    state = ui
                        .ctx()
                        .data(|d| {
                            d.get_temp::<Animation>(ui.scope_id().with("request-tab-animation"))
                        })
                        .unwrap();
                },
            );
            output.textures_delta.clear();
            self.shapes = output.shapes;
            state
        }
        fn click(&mut self, p: Pos2) {
            self.frame(vec![egui::Event::PointerMoved(p)]);
            self.frame(vec![button(p, true)]);
            self.frame(vec![button(p, false)]);
        }
    }
    fn button(pos: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        }
    }
    #[test]
    fn drag_animates_neighbours_and_commits_only_on_release() {
        let mut h = Harness::new(3);
        h.app.ws.active = Some("t1".into());
        let before = h.frame(vec![]);
        let start = pos2(42., 18.);
        h.frame(vec![egui::Event::PointerMoved(start)]);
        h.frame(vec![button(start, true)]);
        let moved = h.frame(vec![egui::Event::PointerMoved(pos2(260., 18.))]);
        assert_eq!(h.app.ws.open_tabs, ["t0", "t1", "t2"]);
        assert_eq!(moved.floating.as_deref(), Some("t0"));
        assert!(moved.velocities["t1"] < 0.);
        assert!(moved.positions["t1"] > 0. && moved.positions["t1"] < before.positions["t1"]);
        h.frame(vec![button(pos2(260., 18.), false)]);
        assert_eq!(h.app.ws.open_tabs, ["t1", "t2", "t0"]);
        assert_eq!(h.app.ws.active.as_deref(), Some("t1"));
        let mut state = moved;
        for _ in 0..45 {
            state = h.frame(vec![]);
        }
        assert!(state.floating.is_none());
        // Fast input can coalesce the last movement and release into one frame.
        let start = pos2(11. + state.positions["t0"] + 30., 18.);
        h.frame(vec![egui::Event::PointerMoved(start)]);
        h.frame(vec![button(start, true)]);
        h.frame(vec![
            egui::Event::PointerMoved(pos2(35., 18.)),
            button(pos2(35., 18.), false),
        ]);
        assert_eq!(h.app.ws.open_tabs, ["t0", "t1", "t2"]);
    }
    #[test]
    fn active_outline_follows_drag_without_lifting_and_neighbours_settle_quickly() {
        let mut h = Harness::new(3);
        let outline = |h: &Harness| {
            h.shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Rect(rect)
                        if rect.stroke.color == h.app.theme.strong_border
                            && rect.rect.height() == 38. =>
                    {
                        Some(rect.clone())
                    }
                    _ => None,
                })
                .expect("Active tab outline must remain visible")
        };
        let original = outline(&h);
        let start = pos2(42., 18.);
        h.frame(vec![egui::Event::PointerMoved(start)]);
        h.frame(vec![button(start, true)]);
        let pointer = pos2(210., 27.);
        let moved = h.frame(vec![egui::Event::PointerMoved(pointer)]);
        let dragged = outline(&h);
        assert_eq!(
            dragged.rect.top(),
            original.rect.top(),
            "The tab stays on its baseline"
        );
        assert_eq!(dragged.rect.size(), original.rect.size());
        assert_eq!(dragged.corner_radius, original.corner_radius);
        assert_eq!(dragged.stroke, original.stroke);
        assert_eq!(dragged.rect.left(), 11. + moved.positions["t0"]);
        assert_eq!(
            dragged.rect.left(),
            original.rect.left() + pointer.x - start.x
        );
        for shape in &h.shapes {
            if let egui::Shape::LineSegment {
                points: [a, b],
                stroke,
            } = &shape.shape
            {
                if a.y == 38. && b.y == 38. && stroke.color == h.app.theme.strong_border {
                    assert!(
                        b.x <= dragged.rect.left() || a.x >= dragged.rect.right(),
                        "The separator must not run under the moving active tab"
                    );
                }
            }
        }
        // Neighbours glide into the vacant slot; they neither snap nor trail
        // behind the pointer for a prolonged animation.
        assert!(moved.positions["t1"] > 0.);
        for _ in 0..11 {
            h.frame(vec![]);
        }
        let settled = h.frame(vec![]);
        assert!(settled.positions["t1"].abs() < 1.);
        h.frame(vec![button(pointer, false)]);
        let landing = outline(&h);
        assert_eq!(landing.rect.top(), original.rect.top());
        assert_eq!(landing.stroke, original.stroke);
        assert_eq!(h.app.ws.active.as_deref(), Some("t0"));
        for _ in 0..45 {
            h.frame(vec![]);
        }
        assert_eq!(outline(&h).rect.top(), original.rect.top());
    }
    #[test]
    fn cancelled_drag_restores_order_and_close_remains_clickable() {
        let mut h = Harness::new(3);
        for outside in [false, true] {
            let p = pos2(42., 18.);
            h.frame(vec![egui::Event::PointerMoved(p)]);
            h.frame(vec![button(p, true)]);
            h.frame(vec![egui::Event::PointerMoved(pos2(260., 18.))]);
            if outside {
                h.frame(vec![
                    egui::Event::PointerMoved(pos2(260., 150.)),
                    button(pos2(260., 150.), false),
                ]);
            } else {
                cancel_drag(&h.ctx);
                h.frame(vec![]);
                h.frame(vec![button(pos2(260., 18.), false)]);
            }
            for _ in 0..45 {
                h.frame(vec![]);
            }
            assert_eq!(h.app.ws.open_tabs, ["t0", "t1", "t2"]);
        }
        h.app.actions.clear();
        let state = h.frame(vec![]);
        h.click(pos2(11. + state.positions["t1"] - GAP - 15., 18.));
        assert!(
            h.app
                .actions
                .iter()
                .any(|a| matches!(a, Action::Close(id) if id=="t0"))
        );
    }
    #[test]
    fn overflow_keeps_plus_visible_and_reveals_new_last_tab() {
        let mut h = Harness::new(18);
        h.click(pos2(505., 18.));
        let first = h.frame(vec![]).scroll;
        for _ in 0..20 {
            h.frame(vec![]);
        }
        let later = h.frame(vec![]).scroll;
        assert!(first > 0. && first < later && (later - 120.).abs() < 1.);
        h.click(pos2(472., 18.));
        assert!(
            h.app
                .actions
                .iter()
                .any(|a| matches!(a, Action::NewTab(..)))
        );
        h.app.actions.clear();
        h.app.ws.requests.push(Request {
            id: "last".into(),
            name: "New last request".into(),
            ..Default::default()
        });
        h.app.ws.open_tabs.push("last".into());
        h.app.ws.active = Some("last".into());
        for _ in 0..40 {
            h.frame(vec![]);
        }
        assert!(h.frame(vec![]).scroll > 1000.);
        h.click(pos2(472., 18.));
        assert!(
            h.app
                .actions
                .iter()
                .any(|a| matches!(a, Action::NewTab(..)))
        );
        let before = h.frame(vec![]).scroll;
        h.click(pos2(22., 18.));
        for _ in 0..20 {
            h.frame(vec![]);
        }
        assert!(h.frame(vec![]).scroll < before - 100.);
    }
}
