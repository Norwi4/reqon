//! Shared Bruno-style popup rows for the collection tree and sidebar.
use crate::{
    theme::Theme,
    widgets::{self as w},
};
use eframe::egui::{self, Color32, Rect, Sense, Stroke, Ui, pos2, vec2};

pub const ROW_HEIGHT: f32 = 28.8;
pub const LABEL_SIZE: f32 = 12.;
pub const ICON_SIZE: f32 = 16.;

pub fn icon_color(t: Theme) -> Color32 {
    crate::theme::hex(if t.dark { 0xbbbbbb } else { 0x666666 }).gamma_multiply(0.8)
}
pub fn danger_color(t: Theme) -> Color32 {
    crate::theme::hex(if t.dark { 0xda4630 } else { 0xb91c1c })
}
pub fn highlight(ui: &Ui, r: Rect, danger: bool, t: Theme) {
    ui.painter().rect_filled(
        r.shrink2(vec2(0., 1.)),
        6,
        if danger {
            danger_color(t).gamma_multiply(0.04)
        } else {
            t.surface
        },
    );
}
pub fn separator(ui: &mut Ui, width: f32, t: Theme) {
    let r = ui.allocate_space(vec2(width, 9.)).1;
    w::line(
        ui,
        pos2(r.left(), r.center().y),
        pos2(r.right(), r.center().y),
        t.border,
    );
}
pub fn icon_button(
    ui: &mut Ui,
    r: Rect,
    name: &str,
    tip: &str,
    danger: bool,
    t: Theme,
) -> egui::Response {
    let response = ui.interact(r, ui.scope_id().with(tip), Sense::click());
    if response.hovered() || response.has_focus() {
        highlight(ui, r, danger, t);
    }
    icon(
        ui,
        name,
        r.center(),
        if danger {
            danger_color(t).gamma_multiply(0.8)
        } else {
            icon_color(t)
        },
        ICON_SIZE,
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tip));
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub struct Item<C> {
    pub label: &'static str,
    pub icon: &'static str,
    pub shortcut: &'static str,
    pub command: Option<C>,
    pub danger: bool,
}
impl<C> Item<C> {
    pub fn new(label: &'static str, icon: &'static str, command: Option<C>) -> Self {
        Self {
            label,
            icon,
            shortcut: "",
            command,
            danger: false,
        }
    }
    pub fn separator() -> Self {
        Self::new("", "", None)
    }
}
pub fn icon(ui: &Ui, name: &str, at: egui::Pos2, color: Color32, size: f32) {
    macro_rules! asset {
        ($n:literal) => {
            (
                concat!("bytes://icons/tree-menu/", $n, ".svg"),
                include_bytes!(concat!("../assets/icons/tree-menu/", $n, ".svg")) as &[u8],
            )
        };
    }
    let (uri, bytes) = match name {
        "Plus" => (
            "bytes://icons/Plus.svg",
            include_bytes!("../assets/icons/Plus.svg") as &[u8],
        ),
        "FilePlus" => asset!("FilePlus"),
        "FolderPlus" => asset!("FolderPlus"),
        "AppWindow" => asset!("AppWindow"),
        "FileCode" => asset!("FileCode"),
        "Run" => asset!("Run"),
        "Copy" => asset!("Copy"),
        "Clipboard" => asset!("Clipboard"),
        "Edit" => asset!("Edit"),
        "Folder" => asset!("Folder"),
        "EyeOff" => asset!("EyeOff"),
        "Info" => asset!("Info"),
        "Settings" => asset!("Settings"),
        "Terminal" => asset!("Terminal"),
        "Trash" => asset!("Trash"),
        "Code" => asset!("Code"),
        "Example" => asset!("Example"),
        "Dots" => asset!("Dots"),
        "Import" => asset!("Import"),
        "Export" => asset!("Export"),
        "Share" => asset!("Share"),
        "Book" => asset!("Book"),
        "Collapse" => asset!("Collapse"),
        "Expand" => asset!("Expand"),
        "Close" => asset!("Close"),
        "Server" => asset!("Server"),
        "Move" => asset!("Move"),
        "Box" => asset!("Box"),
        "OpenAPI" => asset!("OpenAPI"),
        _ => return,
    };
    egui::Image::from_bytes(uri, bytes)
        .tint(color)
        .paint_at(ui, Rect::from_center_size(at, vec2(size, size)));
}

pub fn width<C>(ui: &Ui, entries: &[Item<C>]) -> f32 {
    let label_width = entries
        .iter()
        .map(|i| w::text_width(ui, i.label, LABEL_SIZE))
        .fold(0., f32::max);
    let shortcut_width = entries
        .iter()
        .map(|i| w::text_width(ui, i.shortcut, 11.))
        .fold(0., f32::max);
    (20. + 16. + 16. + label_width + 16. + shortcut_width).max(150.)
}
pub fn frame(t: Theme) -> egui::Frame {
    egui::Frame::NONE
        .fill(if t.dark { t.hover } else { t.bg })
        .stroke(Stroke::new(1., t.border))
        .inner_margin(4.)
        .corner_radius(6)
}
pub fn rows<C: Clone>(
    ui: &mut Ui,
    entries: &[Item<C>],
    width: f32,
    t: Theme,
    keyboard_row: &mut Option<usize>,
) -> Option<C> {
    let mut selected = None;
    ui.set_width(width);
    ui.spacing_mut().item_spacing = vec2(0., 0.);
    let key = ui.input_mut(|i| {
        [
            egui::Key::ArrowDown,
            egui::Key::ArrowUp,
            egui::Key::Home,
            egui::Key::End,
        ]
        .into_iter()
        .find(|k| i.consume_key(egui::Modifiers::NONE, *k))
    });
    if let Some(key) = key {
        let choices: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(_, v)| !v.label.is_empty())
            .map(|(i, _)| i)
            .collect();
        let index = (*keyboard_row).and_then(|r| choices.iter().position(|v| *v == r));
        let next = match key {
            egui::Key::Home => 0,
            egui::Key::End => choices.len() - 1,
            egui::Key::ArrowUp => index.map_or(choices.len() - 1, |i| {
                (i + choices.len() - 1) % choices.len()
            }),
            _ => index.map_or(0, |i| (i + 1) % choices.len()),
        };
        *keyboard_row = Some(choices[next]);
    }
    let enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
    for (index, entry) in entries.iter().enumerate() {
        if entry.label.is_empty() {
            separator(ui, width, t);
            continue;
        }
        let r = ui.allocate_space(vec2(width, ROW_HEIGHT)).1;
        let response = ui.interact(r, ui.scope_id().with(entry.label), Sense::click());
        let hover = response.hovered() || *keyboard_row == Some(index);
        let danger = entry.danger;
        let red = danger_color(t);
        if hover {
            highlight(ui, r, danger, t);
        }
        icon(
            ui,
            entry.icon,
            pos2(r.left() + 18., r.center().y),
            if danger {
                red.gamma_multiply(0.8)
            } else {
                icon_color(t)
            },
            ICON_SIZE,
        );
        w::label(
            ui,
            pos2(r.left() + 34., r.center().y),
            entry.label,
            LABEL_SIZE,
            if danger { red } else { t.text },
        );
        w::label(
            ui,
            pos2(
                r.right() - 10. - w::text_width(ui, entry.shortcut, 11.),
                r.center().y,
            ),
            entry.shortcut,
            11.,
            t.muted,
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, entry.command.is_some(), entry.label)
        });
        if response.clicked() || (enter && *keyboard_row == Some(index)) {
            if let Some(command) = entry.command.clone() {
                selected = Some(command);
                ui.close();
            }
        }
    }
    selected
}
