use crate::{
    app::App,
    model::Request,
    theme::Theme,
    widgets::{self as w, rect},
};
use eframe::egui::{self, FontFamily, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

const COLLAPSE_EDGE: f32 = 80.;
const EXPAND_EDGE: f32 = 100.;

pub use crate::ui_state::{CollapsedPane, PaneLayout};

// CSS ease-in-out: cubic-bezier(0.42, 0, 0.58, 1), as used by Bruno's Sidebar.
fn sidebar_easing(x: f32) -> f32 {
    if x <= 0. || x >= 1. {
        return x.clamp(0., 1.);
    }
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..16 {
        let t = (lo + hi) * 0.5;
        let bx = 3. * (1. - t) * (1. - t) * t * 0.42 + 3. * (1. - t) * t * t * 0.58 + t * t * t;
        if bx < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    let t = (lo + hi) * 0.5;
    t * t * (3. - 2. * t)
}

pub fn sidebar_openness(ctx: &egui::Context, visible: bool) -> f32 {
    ctx.animate_bool_with_time_and_easing(
        egui::Id::unique("sidebar-width-animation"),
        visible,
        0.2,
        sidebar_easing,
    )
}

fn extent(content: Rect, side_by_side: bool) -> f32 {
    if side_by_side {
        content.width()
    } else {
        content.height()
    }
}

fn clamp_split(value: f32, length: f32, side_by_side: bool) -> f32 {
    let (min_request, min_response) = if side_by_side {
        (350., 490.)
    } else {
        (150., 150.)
    };
    // A very small window or a tall Dev Tools panel must never create inverted rectangles.
    let scale = (length / (min_request + min_response)).clamp(0., 1.);
    let low = min_request * scale;
    let high = (length - min_response * scale).max(low);
    if value.is_finite() {
        value.clamp(low, high)
    } else {
        low
    }
}

impl PaneLayout {
    fn resize(&mut self, distance: f32, length: f32, side_by_side: bool) {
        let remaining = length - distance;
        if distance < COLLAPSE_EDGE.min(length * 0.25) {
            self.collapsed = Some(CollapsedPane::Request);
        } else if remaining < COLLAPSE_EDGE.min(length * 0.25) {
            self.collapsed = Some(CollapsedPane::Response);
        } else if !matches!(self.collapsed, Some(CollapsedPane::Request) if distance < EXPAND_EDGE.min(length * 0.35))
            && !matches!(self.collapsed, Some(CollapsedPane::Response) if remaining < EXPAND_EDGE.min(length * 0.35))
        {
            self.collapsed = None;
            let value = clamp_split(distance, length, side_by_side);
            if side_by_side {
                self.width = Some(value);
            } else {
                self.height = Some((value - 6.).max(0.));
            }
        }
    }

    fn expand(&mut self, content: Rect, side_by_side: bool, default_height: f32) {
        if self.collapsed == Some(CollapsedPane::Response) && !side_by_side {
            let height = self.height.unwrap_or(default_height);
            // Bruno leaves enough space for the placeholder after click-expanding Response.
            self.height = Some(height.min((content.height() - 300.).max(150.)));
        }
        self.collapsed = None;
    }
}

#[derive(Clone, Copy, Debug)]
struct PaneRects {
    request: Option<Rect>,
    response: Option<Rect>,
    handle: Rect,
}

fn pane_rects(
    content: Rect,
    side: bool,
    state: PaneLayout,
    default_height: f32,
    default_width: f32,
) -> PaneRects {
    let outer = content.expand2(vec2(16., 16.));
    match state.collapsed {
        Some(CollapsedPane::Request) => {
            if side {
                let handle = rect(outer.left(), outer.top(), 32., outer.height());
                PaneRects {
                    request: None,
                    response: Some(rect(
                        handle.right() + 16.,
                        content.top(),
                        content.right() - handle.right() - 16.,
                        outer.bottom() - content.top(),
                    )),
                    handle,
                }
            } else {
                let handle = rect(outer.left(), outer.top(), outer.width(), 28.);
                PaneRects {
                    request: None,
                    response: Some(rect(
                        content.left(),
                        handle.bottom(),
                        content.width(),
                        outer.bottom() - handle.bottom(),
                    )),
                    handle,
                }
            }
        }
        Some(CollapsedPane::Response) => {
            if side {
                let handle = rect(outer.right() - 32., outer.top(), 32., outer.height());
                PaneRects {
                    request: Some(rect(
                        content.left(),
                        content.top(),
                        handle.left() - content.left() - 16.,
                        outer.bottom() - content.top(),
                    )),
                    response: None,
                    handle,
                }
            } else {
                let handle = rect(outer.left(), outer.bottom() - 28., outer.width(), 28.);
                PaneRects {
                    request: Some(rect(
                        content.left(),
                        content.top(),
                        content.width(),
                        handle.top() - content.top(),
                    )),
                    response: None,
                    handle,
                }
            }
        }
        None => {
            let desired = if side {
                state.width.unwrap_or(default_width)
            } else {
                state.height.unwrap_or(default_height) + 6.
            };
            let split = clamp_split(desired, extent(content, side), side);
            if side {
                let x = content.left() + split;
                PaneRects {
                    request: Some(rect(
                        content.left(),
                        content.top(),
                        split - 8.,
                        content.height(),
                    )),
                    response: Some(rect(
                        x + 9.,
                        content.top(),
                        content.right() - x - 9.,
                        content.height(),
                    )),
                    handle: rect(x - 6., content.top(), 12., content.height()),
                }
            } else {
                let y = content.top() + split;
                PaneRects {
                    request: Some(rect(
                        content.left(),
                        content.top(),
                        content.width(),
                        split - 6.,
                    )),
                    response: Some(rect(
                        content.left(),
                        y + 4.,
                        content.width(),
                        content.bottom() - y - 4.,
                    )),
                    handle: rect(content.left(), y - 6., content.width(), 12.),
                }
            }
        }
    }
}

fn paint_indicator(ui: &Ui, r: Rect, side: bool, pane: CollapsedPane, hovered: bool, t: Theme) {
    ui.painter()
        .rect_filled(r, 0., if hovered { t.hover } else { t.bg });
    let request = pane == CollapsedPane::Request;
    let (a, b) = if side {
        if request {
            (r.right_top(), r.right_bottom())
        } else {
            (r.left_top(), r.left_bottom())
        }
    } else if request {
        (r.left_bottom(), r.right_bottom())
    } else {
        (r.left_top(), r.right_top())
    };
    w::line(ui, a, b, t.border);
    let color = if hovered { t.text } else { t.muted };
    let mut job = egui::text::LayoutJob::simple_singleline(
        if request { "REQUEST" } else { "RESPONSE" }.into(),
        FontId::new(10., FontFamily::Name("Medium".into())),
        color,
    );
    job.sections[0].format.extra_letter_spacing = 0.5;
    let galley = ui.painter().layout_job(job);
    let total = 14. + 6. + galley.size().x;
    let rotate = |v: egui::Vec2| if side { vec2(v.y, -v.x) } else { v };
    let origin = r.center();
    let icon_center = vec2(-total * 0.5 + 7., 0.);
    let direction = if request { 1. } else { -1. };
    // Tabler chevron-down / chevron-up, 14 px, strokeWidth 2; rotate with the label.
    let points = [
        vec2(-3.5, -1.75 * direction),
        vec2(0., 1.75 * direction),
        vec2(3.5, -1.75 * direction),
    ]
    .map(|p| origin + rotate(icon_center + p));
    ui.painter().add(egui::Shape::line(
        points.to_vec(),
        Stroke::new(
            14. / 12.,
            if hovered {
                color
            } else {
                color.gamma_multiply(0.6)
            },
        ),
    ));
    let text_offset = vec2(-total * 0.5 + 20., -galley.size().y * 0.5);
    let mut text = egui::epaint::TextShape::new(origin + rotate(text_offset), galley, color);
    if side {
        text.angle = -std::f32::consts::FRAC_PI_2;
    }
    ui.painter().add(text);
}

impl App {
    pub fn request_response_panes(
        &mut self,
        ui: &mut Ui,
        content: Rect,
        request: &mut Request,
    ) -> bool {
        let side = self.ws.side_by_side;
        let default_width = if self.ws.request_width > 0. {
            self.ws.request_width
        } else {
            (content.width() + 32.) / 2.2 - 16.
        };
        let mut layout = self
            .ws
            .pane_layouts
            .get(&request.id)
            .copied()
            .unwrap_or_default();
        let before = layout;
        let geometry = pane_rects(content, side, layout, self.ws.request_height, default_width);
        let id = ui.scope_id().with(("response-resize", &request.id));
        let response = ui.interact(geometry.handle, id, Sense::click_and_drag());
        if response.double_clicked() && layout.collapsed.is_none() {
            layout = PaneLayout {
                height: Some(380.),
                width: Some((content.width() + 32.) / 2.2 - 16.),
                collapsed: None,
            };
        } else if response.clicked() && layout.collapsed.is_some() {
            layout.expand(content, side, self.ws.request_height);
        } else if response.dragged() || response.drag_stopped() {
            if let Some(pointer) = response.interact_pointer_pos() {
                let distance = if side {
                    pointer.x - content.left()
                } else {
                    pointer.y - content.top()
                };
                layout.resize(distance, extent(content, side), side);
            }
        }
        if response.hovered() || response.dragged() {
            ui.ctx()
                .set_cursor_icon(if layout.collapsed.is_some() && !response.dragged() {
                    egui::CursorIcon::PointingHand
                } else if side {
                    egui::CursorIcon::ResizeHorizontal
                } else {
                    egui::CursorIcon::ResizeVertical
                });
        }
        if before != layout {
            self.ws.pane_layouts.insert(request.id.clone(), layout);
            self.mark_workspace_changed();
            ui.ctx().request_repaint();
        }
        let geometry = pane_rects(content, side, layout, self.ws.request_height, default_width);
        let clip = ui.clip_rect();
        let mut edited = false;
        if let Some(r) = geometry.request.filter(|r| r.is_positive()) {
            ui.set_clip_rect(clip.intersect(r));
            edited = self.request_editor(ui, r, request);
        }
        if let Some(r) = geometry.response.filter(|r| r.is_positive()) {
            ui.set_clip_rect(clip.intersect(r));
            self.response_pane(ui, r, request);
        }
        ui.set_clip_rect(clip);
        if let Some(pane) = layout.collapsed {
            paint_indicator(
                ui,
                geometry.handle,
                side,
                pane,
                response.hovered(),
                self.theme,
            );
            response.on_hover_text(format!(
                "Click to expand {} pane, or drag to resize",
                if pane == CollapsedPane::Request {
                    "Request"
                } else {
                    "Response"
                }
            ));
        } else {
            let r = geometry.handle;
            let (a, b) = if side {
                (pos2(r.center().x, r.top()), pos2(r.center().x, r.bottom()))
            } else {
                (pos2(r.left(), r.center().y), pos2(r.right(), r.center().y))
            };
            w::line(
                ui,
                a,
                b,
                if response.hovered() || response.dragged() {
                    self.theme.strong_border
                } else {
                    self.theme.border
                },
            );
        }
        edited
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn side_by_side_tabs_and_single_message_bodies_share_baselines() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().into()).unwrap();
        app.disk_sync = None;
        app.ws.side_by_side = true;
        for protocol in ["http", "grpc"] {
            let mut request = Request {
                id: "aligned-panes".into(),
                protocol: protocol.into(),
                body: "request_marker".into(),
                body_kind: crate::model::BodyKind::Json,
                ..Default::default()
            };
            let state = app.tabs.entry(request.id.clone()).or_default();
            state.request_tab = 1;
            state.apply_reply(crate::app::PreparedReply::new(crate::model::Reply {
                body: "response_marker".into(),
                status: 200,
                reason: "OK".into(),
                elapsed_ms: 7,
                size: 15,
                ..Default::default()
            }));
            for (size, width) in [(13., 2000.), (13., 1100.), (18., 1100.)] {
                ctx.data_mut(|d| d.insert_temp(egui::Id::unique("editor-font-size"), size));
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(rect(0., 0., 2100., 800.)),
                        ..Default::default()
                    },
                    |ui| {
                        app.request_response_panes(ui, rect(16., 16., width, 700.), &mut request);
                    },
                );
                output.textures_delta.clear();
                let text = |name: &str| {
                    output
                        .shapes
                        .iter()
                        .find_map(|s| match &s.shape {
                            egui::Shape::Text(t) if t.galley.job.text == name => Some(t),
                            _ => None,
                        })
                        .unwrap_or_else(|| panic!("Missing painted text: {name}"))
                };
                let request_tab = text(if protocol == "grpc" {
                    "Message"
                } else {
                    "Body"
                });
                assert_eq!(
                    request_tab.pos.y,
                    text("Response").pos.y,
                    "{protocol} tab labels"
                );
                assert!(
                    (text("request_marker").pos.y - text("response_marker").pos.y).abs() < 0.1,
                    "{protocol} body at font size {size}: request={}, response={}",
                    text("request_marker").pos.y,
                    text("response_marker").pos.y
                );
                assert!(!output.shapes.iter().any(|s| matches!(&s.shape,
                    egui::Shape::Text(t) if t.galley.job.text.to_lowercase() == "message 1")));
                let underlines: Vec<_> = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::LineSegment { points, stroke }
                            if stroke.color == app.theme.accent && points[0].y == points[1].y =>
                        {
                            Some((s.clip_rect, points))
                        }
                        _ => None,
                    })
                    .collect();
                assert_eq!(underlines.len(), 4);
                for (clip, points) in underlines {
                    assert!(
                        points[0].y + 0.5 <= clip.bottom(),
                        "Active tab underline must fit above the horizontal scrollbar"
                    );
                }
            }
        }
    }

    #[test]
    fn sidebar_animation_has_intermediate_frames_and_finishes() {
        let ctx = egui::Context::default();
        let frame = |time, visible| {
            let mut value = 0.;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    ..Default::default()
                },
                |ui| {
                    value = sidebar_openness(ui.ctx(), visible);
                },
            );
            output.textures_delta.clear();
            value
        };
        assert_eq!(frame(0., true), 1.);
        let first = frame(0.02, false);
        let middle = frame(0.10, false);
        assert!(first > middle && middle > 0. && middle < 1.);
        assert_eq!(frame(0.30, false), 0.);
        let first = frame(0.32, true);
        let middle = frame(0.42, true);
        assert!(first < middle && middle > 0. && middle < 1.);
        assert_eq!(frame(0.60, true), 1.);
    }

    #[test]
    fn pane_drag_collapse_click_expand_and_drag_back_work_in_both_layouts() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().to_owned()).unwrap();
        let mut request = Request {
            id: "panes-test".into(),
            ..Default::default()
        };
        let content = rect(16., 16., 1200., 700.);
        let mut time = 0.;
        let mut frame = |app: &mut App, events| {
            time += 0.02;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect(0., 0., 1300., 800.)),
                    time: Some(time),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.request_response_panes(ui, content, &mut request);
                },
            );
            output.textures_delta.clear();
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            pressed,
            button: egui::PointerButton::Primary,
            modifiers: egui::Modifiers::NONE,
        };
        for side in [false, true] {
            app.ws.side_by_side = side;
            app.ws.pane_layouts.clear();
            frame(&mut app, vec![]);
            frame(&mut app, vec![]);
            let point = |distance| {
                if side {
                    pos2(content.left() + distance, content.center().y)
                } else {
                    pos2(content.center().x, content.top() + distance)
                }
            };
            for collapse in [CollapsedPane::Request, CollapsedPane::Response] {
                let state = app
                    .ws
                    .pane_layouts
                    .get("panes-test")
                    .copied()
                    .unwrap_or_default();
                let before = pane_rects(content, side, state, 380., (1232. / 2.2) - 16.);
                let start = before.handle.center();
                let target = point(if collapse == CollapsedPane::Request {
                    20.
                } else {
                    extent(content, side) - 20.
                });
                frame(&mut app, vec![egui::Event::PointerMoved(start)]);
                frame(&mut app, vec![button(start, true)]);
                frame(&mut app, vec![egui::Event::PointerMoved(target)]);
                frame(&mut app, vec![button(target, false)]);
                let state = app.ws.pane_layouts["panes-test"];
                assert_eq!(state.collapsed, Some(collapse));
                let collapsed = pane_rects(content, side, state, 380., (1232. / 2.2) - 16.);
                assert_eq!(
                    if side {
                        collapsed.handle.width()
                    } else {
                        collapsed.handle.height()
                    },
                    if side { 32. } else { 28. }
                );
                let pos = collapsed.handle.center();
                frame(&mut app, vec![egui::Event::PointerMoved(pos)]);
                frame(&mut app, vec![button(pos, true)]);
                frame(&mut app, vec![button(pos, false)]);
                assert_eq!(app.ws.pane_layouts["panes-test"].collapsed, None);
            }
            // Pull the collapsed Request strip back into the workspace without releasing capture.
            app.ws.pane_layouts.get_mut("panes-test").unwrap().collapsed =
                Some(CollapsedPane::Request);
            frame(&mut app, vec![]);
            let collapsed =
                pane_rects(content, side, app.ws.pane_layouts["panes-test"], 380., 544.);
            let start = collapsed.handle.center();
            let target = point(extent(content, side) * 0.45);
            frame(&mut app, vec![egui::Event::PointerMoved(start)]);
            frame(&mut app, vec![button(start, true)]);
            frame(&mut app, vec![egui::Event::PointerMoved(target)]);
            frame(&mut app, vec![button(target, false)]);
            let layout = app.ws.pane_layouts["panes-test"];
            assert_eq!(layout.collapsed, None);
            let restored = pane_rects(content, side, layout, 380., 544.);
            assert!(
                (if side {
                    restored.handle.center().x - target.x
                } else {
                    restored.handle.center().y - target.y
                })
                .abs()
                    < 1.
            );
            let saved = serde_json::to_string(&app.ws.metadata_snapshot()).unwrap();
            let loaded: crate::model::Workspace = serde_json::from_str(&saved).unwrap();
            assert_eq!(loaded.pane_layouts, app.ws.pane_layouts);
        }
        assert_eq!(request.body, "");
    }

    #[test]
    fn response_placeholder_is_clipped_and_tiny_workspaces_do_not_invert_panes() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().to_owned()).unwrap();
        let mut request = Request::default();
        let content = rect(16., 16., 900., 360.);
        let geometry = pane_rects(content, false, PaneLayout::default(), 380., 400.);
        let response = geometry.response.unwrap();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect(0., 0., 950., 430.)),
                ..Default::default()
            },
            |ui| {
                app.request_response_panes(ui, content, &mut request);
            },
        );
        let mut found = 0;
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape {
                if matches!(
                    text.galley.job.text.as_str(),
                    "Send Request" | "Ctrl + Enter" | "New Request" | "Edit Environments"
                ) {
                    assert!(
                        response.contains_rect(shape.clip_rect),
                        "placeholder escaped its pane: {:?}",
                        shape.clip_rect
                    );
                    found += 1;
                }
            }
        }
        assert!(found > 0);
        output.textures_delta.clear();
        for width in [0., 10., 100., 320., 1200.] {
            for height in [0., 10., 100., 300., 900.] {
                for side in [false, true] {
                    let bounds = rect(16., 16., width, height);
                    let panes = pane_rects(bounds, side, PaneLayout::default(), 10000., 10000.);
                    for r in [panes.request.unwrap(), panes.response.unwrap()] {
                        assert!(r.is_finite() && r.width() >= 0. && r.height() >= 0.);
                        if r.is_positive() {
                            assert!(bounds.contains_rect(r));
                        }
                    }
                }
            }
        }
    }
}
