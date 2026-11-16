use eframe::egui::{self, CursorIcon, Pos2, Rect, ResizeDirection, Sense, Ui, pos2};

// Logical points: the native backend converts these to the monitor's DPI.
const BORDER: f32 = 5.;
const CORNER: f32 = 14.;

/// Frameless windows have no OS border hit testing. Invisible grips hand the
/// gesture back to the OS, retaining its minimum size, snapping and live resize.
/// Register these after the content so the title bar cannot steal a border drag.
pub fn resize(ui: &mut Ui) {
    #[cfg(windows)]
    if windows::INSTALLED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if ui.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    }) {
        return;
    }
    let r = ui.max_rect();
    let grips = [
        Rect::from_min_max(r.min, pos2(r.left() + BORDER, r.bottom())),
        Rect::from_min_max(pos2(r.right() - BORDER, r.top()), r.max),
        Rect::from_min_max(r.min, pos2(r.right(), r.top() + BORDER)),
        Rect::from_min_max(pos2(r.left(), r.bottom() - BORDER), r.max),
    ];
    let direction = ui.input(|i| i.pointer.hover_pos().and_then(|p| direction_at(r, p)));
    for (index, grip) in grips.into_iter().enumerate() {
        let response = ui.interact(
            grip,
            ui.scope_id().with(("native-window-resize", index)),
            Sense::click_and_drag(),
        );
        if let Some(direction) = direction {
            if response.hovered() || response.is_pointer_button_down_on() {
                ui.ctx().set_cursor_icon(match direction {
                    ResizeDirection::North | ResizeDirection::South => CursorIcon::ResizeVertical,
                    ResizeDirection::East | ResizeDirection::West => CursorIcon::ResizeHorizontal,
                    ResizeDirection::NorthWest | ResizeDirection::SouthEast => {
                        CursorIcon::ResizeNwSe
                    }
                    ResizeDirection::NorthEast | ResizeDirection::SouthWest => {
                        CursorIcon::ResizeNeSw
                    }
                });
            }
            // Begin on the press itself: waiting for egui's drag threshold can
            // miss the native backend's required left-button-down event.
            if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
            }
        }
    }
}

#[cfg(windows)]
pub mod windows {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM},
        UI::{
            HiDpi::GetDpiForWindow,
            Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
            WindowsAndMessaging::{
                GWL_STYLE, GetWindowLongPtrW, GetWindowRect, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT,
                HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, IsZoomed, WM_NCDESTROY,
                WM_NCHITTEST, WS_THICKFRAME,
            },
        },
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

    pub(super) static INSTALLED: AtomicBool = AtomicBool::new(false);
    const SUBCLASS_ID: usize = 0x5245514F;

    pub fn install(window: &winit::window::Window) -> anyhow::Result<()> {
        let RawWindowHandle::Win32(handle) = window.window_handle()?.as_raw() else {
            anyhow::bail!("Expected a Windows window");
        };
        // Called on winit's UI thread. The callback owns no borrowed state and
        // is removed on WM_NCDESTROY, before Windows destroys the native handle.
        let success = unsafe {
            SetWindowSubclass(handle.hwnd.get() as HWND, Some(window_proc), SUBCLASS_ID, 0)
        };
        anyhow::ensure!(success != 0, "Could not install the resize border");
        INSTALLED.store(true, Ordering::Relaxed);
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        // Keep winit's normal processing, including native min/max constraints.
        // Handling the hit test here lets Windows resize immediately, without
        // waiting for a rendered egui frame or losing fast mouse gestures.
        unsafe {
            if message == WM_NCDESTROY {
                RemoveWindowSubclass(hwnd, Some(window_proc), SUBCLASS_ID);
            }
            let result = DefSubclassProc(hwnd, message, wparam, lparam);
            if message != WM_NCHITTEST
                || result != HTCLIENT as LRESULT
                || IsZoomed(hwnd) != 0
                || (GetWindowLongPtrW(hwnd, GWL_STYLE) as u32 & WS_THICKFRAME) == 0
            {
                return result;
            }
            let mut bounds = RECT::default();
            if GetWindowRect(hwnd, &mut bounds) == 0 {
                return result;
            }
            let scale = (GetDpiForWindow(hwnd) as f32 / 96.).max(1.);
            // LPARAM contains signed screen coordinates, including monitors
            // positioned to the left of or above the primary monitor.
            let x = (lparam as u16 as i16) as f32;
            let y = ((lparam >> 16) as u16 as i16) as f32;
            let p = pos2(
                (x - bounds.left as f32) / scale,
                (y - bounds.top as f32) / scale,
            );
            let rect = Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(
                    (bounds.right - bounds.left) as f32 / scale,
                    (bounds.bottom - bounds.top) as f32 / scale,
                ),
            );
            match direction_at(rect, p) {
                Some(ResizeDirection::North) => HTTOP as LRESULT,
                Some(ResizeDirection::South) => HTBOTTOM as LRESULT,
                Some(ResizeDirection::East) => HTRIGHT as LRESULT,
                Some(ResizeDirection::West) => HTLEFT as LRESULT,
                Some(ResizeDirection::NorthEast) => HTTOPRIGHT as LRESULT,
                Some(ResizeDirection::NorthWest) => HTTOPLEFT as LRESULT,
                Some(ResizeDirection::SouthEast) => HTBOTTOMRIGHT as LRESULT,
                Some(ResizeDirection::SouthWest) => HTBOTTOMLEFT as LRESULT,
                None => result,
            }
        }
    }
}

fn direction_at(r: Rect, p: Pos2) -> Option<ResizeDirection> {
    if !r.contains(p) || r.shrink(BORDER).contains(p) {
        return None;
    }
    let left = p.x < r.left() + CORNER;
    let right = p.x > r.right() - CORNER;
    let top = p.y < r.top() + CORNER;
    let bottom = p.y > r.bottom() - CORNER;
    Some(match (left, right, top, bottom) {
        (true, _, true, _) => ResizeDirection::NorthWest,
        (_, true, true, _) => ResizeDirection::NorthEast,
        (true, _, _, true) => ResizeDirection::SouthWest,
        (_, true, _, true) => ResizeDirection::SouthEast,
        (true, _, _, _) => ResizeDirection::West,
        (_, true, _, _) => ResizeDirection::East,
        (_, _, true, _) => ResizeDirection::North,
        _ => ResizeDirection::South,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, ViewportCommand, ViewportId};

    fn frame(
        ctx: &Context,
        p: Pos2,
        pressed: Option<bool>,
        maximized: bool,
        fullscreen: bool,
    ) -> egui::FullOutput {
        let mut input = RawInput::default();
        input.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1000., 700.)));
        let viewport = input.viewports.get_mut(&ViewportId::ROOT).unwrap();
        viewport.maximized = Some(maximized);
        viewport.fullscreen = Some(fullscreen);
        input.events.push(Event::PointerMoved(p));
        if let Some(pressed) = pressed {
            input.events.push(Event::PointerButton {
                pos: p,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
        }
        let mut output = ctx.run_ui(input, |ui| {
            // A full-window control stands in for the overlapping title bar,
            // close button, status bar and editor. None may also receive the drag.
            let content = ui.interact(
                ui.max_rect(),
                egui::Id::unique("content"),
                Sense::click_and_drag(),
            );
            resize(ui);
            if direction_at(ui.max_rect(), p).is_some() && !maximized && !fullscreen {
                assert!(!content.clicked());
                assert!(!content.drag_started());
            }
        });
        output.textures_delta.clear();
        output
    }

    #[test]
    fn each_edge_and_corner_starts_native_resize_on_press() {
        for (p, expected) in [
            (pos2(2., 350.), ResizeDirection::West),
            (pos2(998., 350.), ResizeDirection::East),
            (pos2(500., 2.), ResizeDirection::North),
            (pos2(500., 698.), ResizeDirection::South),
            (pos2(2., 10.), ResizeDirection::NorthWest),
            (pos2(990., 2.), ResizeDirection::NorthEast),
            (pos2(10., 698.), ResizeDirection::SouthWest),
            (pos2(998., 690.), ResizeDirection::SouthEast),
        ] {
            let ctx = Context::default();
            let _ = frame(&ctx, p, None, false, false);
            let output = frame(&ctx, p, Some(true), false, false);
            assert_eq!(
                output.viewport_output[&ViewportId::ROOT].commands,
                vec![ViewportCommand::BeginResize(expected)]
            );
            let output = frame(&ctx, p, Some(false), false, false);
            assert!(
                output.viewport_output[&ViewportId::ROOT]
                    .commands
                    .is_empty()
            );
        }
    }

    #[test]
    fn content_and_maximized_or_fullscreen_windows_do_not_resize() {
        for (p, maximized, fullscreen) in [
            (pos2(500., 350.), false, false),
            (pos2(20., 20.), false, false),
            (pos2(2., 350.), true, false),
            (pos2(998., 690.), false, true),
        ] {
            let ctx = Context::default();
            let _ = frame(&ctx, p, None, maximized, fullscreen);
            let output = frame(&ctx, p, Some(true), maximized, fullscreen);
            assert!(
                output.viewport_output[&ViewportId::ROOT]
                    .commands
                    .is_empty()
            );
        }
    }

    #[test]
    fn actual_workspace_keeps_window_border_interactive() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context::default();
        let mut app = crate::app::App::from_context(&ctx, root.path().to_owned()).unwrap();
        let p = pos2(1507., 450.);
        for pressed in [false, true] {
            let mut input = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1510., 944.))),
                events: vec![Event::PointerMoved(p)],
                ..Default::default()
            };
            if pressed {
                input.events.push(Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(input, |ui| {
                app.draw(ui);
                resize(ui);
            });
            output.textures_delta.clear();
            if pressed {
                assert!(
                    output.viewport_output[&ViewportId::ROOT]
                        .commands
                        .contains(&ViewportCommand::BeginResize(ResizeDirection::East)),
                    "{:?}",
                    output.viewport_output[&ViewportId::ROOT].commands
                );
            }
        }
    }
}
