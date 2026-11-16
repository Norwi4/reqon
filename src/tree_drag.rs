use crate::{
    app::{Action, App},
    tree::{Kind, Row},
    tree_move::Target,
    widgets::{self as w, Icon},
};
use eframe::egui::{self, Rect, Response, Ui, pos2, vec2};

#[derive(Clone)]
pub struct RequestDrag {
    pub id: String,
    pub name: String,
}

#[derive(Clone)]
struct PendingDrag {
    payload: RequestDrag,
    origin: egui::Pos2,
    started: bool,
}
fn pending_id() -> egui::Id {
    egui::Id::unique("pending-tree-drag")
}
pub fn cancel_pending(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<PendingDrag>(pending_id()));
}
pub fn begin(ui: &Ui) {
    if ui.input(|i| i.pointer.primary_pressed()) {
        cancel_pending(ui.ctx());
    }
}
pub fn source(response: &Response, payload: RequestDrag) {
    let pressed = response.ctx.input(|i| {
        i.events.iter().find_map(|event| match event {
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                ..
            } => Some(*pos),
            _ => None,
        })
    });
    if let Some(origin) = pressed.filter(|p| {
        response.interact_rect.contains(*p)
            && response.ctx.layer_id_at(*p) == Some(response.layer_id)
    }) {
        response.ctx.data_mut(|d| {
            d.insert_temp(
                pending_id(),
                PendingDrag {
                    payload: payload.clone(),
                    origin,
                    started: false,
                },
            )
        });
    }
    response.dnd_set_drag_payload(payload);
}
/// OS input can batch the last movement and release into one rendered frame.
/// Remember the press so a fast drag still works without a drag-start frame.
pub fn prepare_drop(ui: &Ui) {
    let Some(mut pending) = ui.ctx().data(|d| d.get_temp::<PendingDrag>(pending_id())) else {
        return;
    };
    let (position, down, released) = ui.input(|i| {
        (
            i.pointer.hover_pos(),
            i.pointer.primary_down(),
            i.pointer.primary_released(),
        )
    });
    if egui::DragAndDrop::has_payload_of_type::<RequestDrag>(ui.ctx()) {
        pending.started = true;
    }
    if !pending.started
        && (down || released)
        && position.is_some_and(|p| p.distance(pending.origin) > 6.)
    {
        egui::DragAndDrop::set_payload(ui.ctx(), pending.payload.clone());
        pending.started = true;
    }
    ui.ctx().data_mut(|d| d.insert_temp(pending_id(), pending));
}
pub fn finish(ui: &Ui) {
    if ui.input(|i| !i.pointer.primary_down()) {
        cancel_pending(ui.ctx());
    }
}

impl App {
    pub fn tree_drop_zone(&mut self, ui: &Ui, response: &Response, row: &Row, bounds: Rect) {
        if self.tree_moving {
            return;
        }
        // Labels are child UIs for clipping; they must not shield the row.
        if !response.container_contains_pointer() {
            return;
        }
        let Some(payload) = egui::DragAndDrop::payload::<RequestDrag>(ui.ctx()) else {
            return;
        };
        if row.key == payload.id || row.collection.is_empty() {
            return;
        }
        let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
            return;
        };
        let edge = 6.;
        let target = if row.kind != Kind::Request {
            Target::Inside {
                collection: row.collection.clone(),
                folder: row.folder.clone(),
            }
        } else if pointer.y < bounds.top() + edge {
            Target::Before(row.key.clone())
        } else if pointer.y > bounds.bottom() - edge {
            Target::After(row.key.clone())
        } else {
            Target::Group(row.key.clone())
        };
        let accent = self.theme.accent;
        let hint = match &target {
            Target::Before(_) | Target::After(_) => {
                let y = if matches!(target, Target::Before(_)) {
                    bounds.top()
                } else {
                    bounds.bottom()
                };
                ui.painter().line_segment(
                    [pos2(bounds.left() + 12., y), pos2(bounds.right() - 6., y)],
                    egui::Stroke::new(2., accent),
                );
                ui.painter()
                    .circle_filled(pos2(bounds.left() + 12., y), 3., accent);
                "Move request"
            }
            Target::Group(_) => {
                ui.painter().rect_stroke(
                    bounds.shrink2(vec2(5., 0.5)),
                    3,
                    egui::Stroke::new(1., accent),
                    egui::StrokeKind::Inside,
                );
                "Create New folder"
            }
            Target::Inside { .. } => {
                ui.painter().rect_stroke(
                    bounds.shrink2(vec2(5., 0.5)),
                    3,
                    egui::Stroke::new(1., accent),
                    egui::StrokeKind::Inside,
                );
                if !row.expanded {
                    let id = egui::Id::unique("tree-drag-expand");
                    let now = ui.input(|i| i.time);
                    let previous = ui.ctx().data(|data| data.get_temp::<(String, f64)>(id));
                    if previous
                        .as_ref()
                        .is_some_and(|(key, time)| key == &row.key && now - time > 0.65)
                    {
                        self.collapsed.remove(&row.key);
                    } else if previous.as_ref().is_none_or(|(key, _)| key != &row.key) {
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(id, (row.key.clone(), now)));
                    }
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(50));
                }
                "Move into folder"
            }
        };
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if ui.input(|i| i.pointer.primary_released())
            && egui::DragAndDrop::take_payload::<RequestDrag>(ui.ctx()).is_some()
        {
            self.actions
                .push(Action::MoveRequest(payload.id.clone(), target));
        } else {
            egui::Area::new(egui::Id::unique("tree-drag-preview"))
                .order(egui::Order::Tooltip)
                .fixed_pos(pointer + vec2(14., 18.))
                .interactable(false)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let r = ui.allocate_space(vec2(16., 16.)).1;
                            w::icon(ui, r.center(), Icon::File, accent, 14.);
                            ui.label(egui::RichText::new(&payload.name).size(12.));
                        });
                        ui.label(egui::RichText::new(hint).color(self.theme.muted).size(11.));
                    });
                });
        }
    }
}

pub fn autoscroll(ui: &mut Ui) {
    if !egui::DragAndDrop::has_payload_of_type::<RequestDrag>(ui.ctx()) {
        return;
    }
    let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    let viewport = ui.clip_rect();
    if !viewport.contains(pointer) {
        return;
    }
    let delta = if pointer.y < viewport.top() + 28. {
        1.
    } else if pointer.y > viewport.bottom() - 28. {
        -1.
    } else {
        return;
    };
    let step = ui.input(|i| i.stable_dt).min(0.05) * 420.;
    ui.scroll_with_delta(vec2(0., delta * step));
    ui.ctx().request_repaint();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;
    #[test]
    fn dragging_request_labels_uses_the_entire_row_and_distinguishes_edges_from_center() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().to_owned()).unwrap();
        app.ws.collections = vec![Collection {
            id: "c".into(),
            name: "Collection".into(),
            path: root.path().join("collection"),
            environments: vec![],
            vars: vec![],
            folders: vec![],
        }];
        app.ws.requests = (1..=3)
            .map(|i| Request {
                id: format!("r{i}"),
                name: format!("Request {i}"),
                collection: "c".into(),
                seq: i as f64,
                ..Default::default()
            })
            .collect();
        app.ws.open_tabs = vec![];
        app.ws.active = None;
        app.ws.sidebar_width = 250.;
        app.tree.stale = true;
        let mut time = 0.;
        let mut frame = |app: &mut App, events: Vec<egui::Event>| {
            time += 0.02;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(pos2(0., 0.), vec2(1510., 944.))),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| app.draw(ui),
            );
            output.textures_delta.clear();
        };
        for _ in 0..3 {
            frame(&mut app, vec![]);
        }
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        for (target, group, fast) in [
            (pos2(95., 101.), false, false),
            (pos2(95., 111.), true, false),
            (pos2(95., 101.), false, true),
            (pos2(95., 111.), true, true),
        ] {
            let start = pos2(95., 162.);
            frame(&mut app, vec![egui::Event::PointerMoved(start)]);
            frame(&mut app, vec![button(start, true)]);
            if fast {
                frame(
                    &mut app,
                    vec![egui::Event::PointerMoved(target), button(target, false)],
                );
            } else {
                frame(&mut app, vec![egui::Event::PointerMoved(pos2(95., 145.))]);
                assert!(
                    egui::DragAndDrop::has_payload_of_type::<RequestDrag>(&ctx),
                    "Drag did not start on request text"
                );
                frame(&mut app, vec![egui::Event::PointerMoved(target)]);
                frame(&mut app, vec![]);
                frame(&mut app, vec![button(target, false)]);
            }
            assert!(app.actions.iter().any(|a|matches!(a,Action::MoveRequest(id,Target::Group(to)) if group && id=="r3" && to=="r1") || matches!(a,Action::MoveRequest(id,Target::Before(to)) if !group && id=="r3" && to=="r1")),"Missing expected drop action");
            app.actions.clear();
        }
    }
}
