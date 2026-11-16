use crate::{model::Pair, theme::Theme};
use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextEdit, Ui,
    Vec2, pos2, vec2,
};

pub fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(pos2(x, y), vec2(w.max(0.), h.max(0.)))
}
pub fn label(ui: &Ui, pos: Pos2, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(size),
        color,
    );
}
pub fn tracked_label(ui: &Ui, pos: Pos2, text: &str, size: f32, color: Color32, tracking: f32) {
    let mut job =
        egui::text::LayoutJob::simple_singleline(text.into(), FontId::proportional(size), color);
    for section in &mut job.sections {
        section.format.extra_letter_spacing = tracking;
    }
    let galley = ui.painter().layout_job(job);
    ui.painter()
        .galley(pos2(pos.x, pos.y - galley.size().y / 2.), galley, color);
}
pub fn elided_medium(ui: &Ui, r: Rect, text: &str, size: f32, color: Color32) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.into(),
        FontId::new(size, egui::FontFamily::Name("Medium".into())),
        color,
    );
    job.wrap = egui::text::TextWrapping {
        max_width: r.width(),
        max_rows: 1,
        break_anywhere: true,
        ..Default::default()
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(r.left(), r.center().y - galley.size().y / 2.),
        galley,
        color,
    );
}
pub fn elided_label(ui: &Ui, r: Rect, text: &str, size: f32, color: Color32) {
    elided_styled_label(ui, r, text, size, color, false);
}
pub fn elided_styled_label(ui: &Ui, r: Rect, text: &str, size: f32, color: Color32, italic: bool) {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        FontId::proportional(size),
        color,
    );
    for section in &mut job.sections {
        section.format.italics = italic;
    }
    job.wrap = egui::text::TextWrapping {
        max_width: r.width(),
        max_rows: 1,
        break_anywhere: true,
        ..Default::default()
    };
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(r.left(), r.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}
pub fn soft_button(ui: &mut Ui, r: Rect, text: &str, icon_kind: Icon, t: Theme) -> Response {
    let response = ui.interact(r, ui.scope_id().with(text), Sense::click());
    ui.painter().rect(
        r,
        5,
        t.accent
            .gamma_multiply(if response.hovered() { 0.15 } else { 0.07 }),
        Stroke::new(1., t.accent.gamma_multiply(0.12)),
        StrokeKind::Inside,
    );
    icon(
        ui,
        pos2(r.left() + 15., r.center().y),
        icon_kind,
        t.accent,
        14.,
    );
    label(ui, pos2(r.left() + 30., r.center().y), text, 12., t.accent);
    response
}
pub fn toolbar_chip(ui: &mut Ui, r: Rect, text: &str, kind: Icon, t: Theme) -> Response {
    let response = ui.interact(
        r,
        ui.scope_id().with(("toolbar-chip", text)),
        Sense::click(),
    );
    ui.painter().rect(
        r,
        4,
        if response.hovered() { t.hover } else { t.bg },
        Stroke::new(1., t.border),
        StrokeKind::Inside,
    );
    icon(ui, pos2(r.left() + 13., r.center().y), kind, t.accent, 14.);
    label(ui, pos2(r.left() + 25., r.center().y), text, 13., t.text);
    response
}
pub fn checkbox(ui: &mut Ui, value: &mut bool, text: &str, t: Theme) -> bool {
    let r = ui.allocate_space(vec2(ui.available_width(), 20.)).1;
    checkbox_at(ui, r, value, text, t)
}
pub fn checkbox_at(ui: &mut Ui, r: Rect, value: &mut bool, text: &str, t: Theme) -> bool {
    let response = ui.interact(r, ui.scope_id().with(text), Sense::click());
    if response.clicked() {
        *value = !*value;
    }
    let square = rect(r.left(), r.center().y - 6.5, 13., 13.);
    ui.painter().rect(
        square,
        1,
        if *value { t.accent } else { Color32::WHITE },
        Stroke::new(1., crate::theme::hex(0x777777)),
        StrokeKind::Inside,
    );
    if *value {
        icon(
            ui,
            square.center(),
            Icon::Check,
            crate::theme::hex(0x555555),
            12.,
        );
    }
    label(ui, pos2(r.left() + 21., r.center().y), text, 13., t.text);
    response.clicked()
}
pub fn semibold(ui: &Ui, pos: Pos2, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::new(size, egui::FontFamily::Name("Semibold".into())),
        color,
    );
}
pub fn medium(ui: &Ui, pos: Pos2, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::new(size, egui::FontFamily::Name("Medium".into())),
        color,
    );
}
pub fn mono(ui: &Ui, pos: Pos2, text: &str, size: f32, color: Color32) {
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::monospace(size),
        color,
    );
}
pub fn line(ui: &Ui, a: Pos2, b: Pos2, color: Color32) {
    ui.painter().line_segment([a, b], Stroke::new(1., color));
}

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum Icon {
    Reqon,
    Pin,
    Upload,
    Pinned,
    Login,
    ArrowLeft,
    GitBrand,
    Network,
    Dashboard,
    Menu,
    Home,
    Box,
    Plus,
    Search,
    Dots,
    Chevron,
    Down,
    Folder,
    File,
    Save,
    Code,
    Send,
    ArrowRight,
    Close,
    TabClose,
    Minimize,
    Maximize,
    Settings,
    Sun,
    Moon,
    Copy,
    Edit,
    Download,
    Trash,
    Split,
    SplitRows,
    Panel,
    Clock,
    Globe,
    Play,
    Check,
    Key,
    Shield,
    Import,
    Terminal,
    Refresh,
    Wand,
    MessageRefresh,
    External,
    BottomPanel,
    Run,
    Palette,
    Bell,
    Github,
    TerminalBox,
    GitCommit,
    GitFork,
    Undo,
    GitMerge,
    Cookie,
    CookieOff,
    Book,
    Speakerphone,
    Discord,
    Twitter,
    Linkedin,
    Eraser,

    Tool,
    DotsVertical,
    Help,
    Flask,
    Tag,
    Eye,
    Workspace,
    Browser,
    BrightnessUp,
    ThemeMoon,
    DeviceDesktop,
    UserCircle,
    Certificate,
    Star,
    Lock,
    GitBranch,
    Keyboard,
    Sparkles,
    Database,
    Support,
    Beta,
    Info,
}
pub fn icon(ui: &Ui, center: Pos2, kind: Icon, color: Color32, size: f32) {
    if let Some((name, bytes)) = crate::icons::svg(kind) {
        egui::Image::from_bytes(name, bytes)
            .tint(color)
            .paint_at(ui, Rect::from_center_size(center, vec2(size, size)));
        return;
    }
    let p = ui.painter();
    let k = size / 24.;
    let point = |x: f32, y: f32| center + vec2((x - 12.) * k, (y - 12.) * k);
    let stroke = Stroke::new(1.2, color);
    let path = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|(x, y)| point(*x, *y)).collect(),
            stroke,
        ));
    };
    let box_at = |x: f32, y: f32, w: f32, h: f32, r: u8| {
        p.rect_stroke(
            Rect::from_min_size(point(x, y), vec2(w * k, h * k)),
            r,
            stroke,
            StrokeKind::Inside,
        );
    };
    match kind {
        Icon::Reqon | Icon::GitBrand | Icon::Network | Icon::Dashboard => {
            unreachable!("embedded SVG")
        }
        Icon::Eye => {
            path(&[
                (2., 12.),
                (5., 7.),
                (9., 5.),
                (15., 5.),
                (19., 7.),
                (22., 12.),
                (19., 17.),
                (15., 19.),
                (9., 19.),
                (5., 17.),
                (2., 12.),
            ]);
            p.circle_stroke(center, 3. * k, stroke);
        }
        Icon::Tag => {
            path(&[
                (3., 12.),
                (3., 5.),
                (5., 3.),
                (12., 3.),
                (21., 12.),
                (12., 21.),
                (3., 12.),
            ]);
            p.circle_stroke(point(7.5, 7.5), 1. * k, stroke);
        }
        Icon::Menu => {
            for y in [6., 12., 18.] {
                path(&[(4., y), (20., y)]);
            }
        }
        Icon::Home => {
            path(&[(3., 11.), (12., 3.), (21., 11.)]);
            path(&[(6., 9.), (6., 21.), (18., 21.), (18., 9.)]);
            path(&[(10., 21.), (10., 15.), (14., 15.), (14., 21.)]);
        }
        Icon::Box => {
            path(&[
                (12., 3.),
                (21., 8.),
                (21., 17.),
                (12., 22.),
                (3., 17.),
                (3., 8.),
                (12., 3.),
            ]);
            path(&[(3., 8.), (12., 13.), (21., 8.)]);
            path(&[(12., 13.), (12., 22.)]);
        }
        Icon::Plus => {
            path(&[(5., 12.), (19., 12.)]);
            path(&[(12., 5.), (12., 19.)]);
        }
        Icon::Search => {
            p.circle_stroke(point(10., 10.), 6. * k, stroke);
            path(&[(15., 15.), (21., 21.)]);
        }
        Icon::Dots => {
            for x in [5., 12., 19.] {
                p.circle_filled(point(x, 12.), 1. * k, color);
            }
        }
        Icon::Chevron => path(&[(9., 6.), (15., 12.), (9., 18.)]),
        Icon::Down => path(&[(6., 9.), (12., 15.), (18., 9.)]),
        Icon::Folder => path(&[
            (3., 7.),
            (3., 20.),
            (21., 20.),
            (21., 6.),
            (12., 6.),
            (9., 3.),
            (3., 3.),
            (3., 7.),
        ]),
        Icon::File => {
            path(&[
                (5., 3.),
                (14., 3.),
                (20., 9.),
                (20., 21.),
                (5., 21.),
                (5., 3.),
            ]);
            path(&[(14., 3.), (14., 9.), (20., 9.)]);
        }
        Icon::Save => {
            path(&[
                (4., 3.),
                (17., 3.),
                (21., 7.),
                (21., 21.),
                (3., 21.),
                (3., 3.),
                (4., 3.),
            ]);
            box_at(7., 3., 9., 6., 0);
            box_at(7., 14., 10., 7., 0);
        }
        Icon::Code => {
            path(&[(7., 7.), (2., 12.), (7., 17.)]);
            path(&[(17., 7.), (22., 12.), (17., 17.)]);
            path(&[(14., 3.), (10., 21.)]);
        }
        Icon::Send => {
            path(&[(3., 3.), (22., 12.), (3., 21.), (7., 12.), (3., 3.)]);
            path(&[(7., 12.), (22., 12.)]);
        }
        Icon::Play => path(&[(7., 4.), (20., 12.), (7., 20.), (7., 4.)]),
        Icon::Close | Icon::TabClose => {
            path(&[(6., 6.), (18., 18.)]);
            path(&[(18., 6.), (6., 18.)]);
        }
        Icon::Minimize => path(&[(6., 12.), (18., 12.)]),
        Icon::Maximize => box_at(5., 5., 14., 14., 0),
        Icon::Settings => {
            p.circle_stroke(center, 6. * k, stroke);
            p.circle_stroke(center, 2. * k, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let dir = vec2(a.cos(), a.sin());
                p.line_segment([center + dir * 7. * k, center + dir * 10. * k], stroke);
            }
        }
        Icon::Sun => {
            p.circle_stroke(center, 4. * k, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let d = vec2(a.cos(), a.sin());
                p.line_segment([center + d * 7. * k, center + d * 10. * k], stroke);
            }
        }
        Icon::Moon => {
            path(&[
                (18., 4.),
                (14., 4.),
                (10., 7.),
                (9., 12.),
                (11., 16.),
                (16., 18.),
                (20., 16.),
                (18., 20.),
                (12., 22.),
                (6., 19.),
                (3., 13.),
                (5., 7.),
                (10., 3.),
                (15., 2.),
            ]);
        }
        Icon::ArrowRight => {
            path(&[(4., 12.), (20., 12.)]);
            path(&[(14., 6.), (20., 12.), (14., 18.)]);
        }
        Icon::Edit => {
            path(&[
                (4., 20.),
                (8., 19.),
                (20., 7.),
                (17., 4.),
                (5., 16.),
                (4., 20.),
            ]);
            path(&[(14., 7.), (17., 10.)]);
        }
        Icon::Copy => {
            box_at(8., 8., 13., 13., 1);
            path(&[(16., 5.), (16., 3.), (3., 3.), (3., 16.), (5., 16.)]);
        }
        Icon::Download => {
            path(&[(12., 3.), (12., 16.)]);
            path(&[(7., 11.), (12., 16.), (17., 11.)]);
            path(&[(3., 16.), (3., 21.), (21., 21.), (21., 16.)]);
        }
        Icon::Trash => {
            path(&[(3., 6.), (21., 6.)]);
            path(&[(6., 6.), (7., 21.), (17., 21.), (18., 6.)]);
            path(&[(9., 6.), (9., 3.), (15., 3.), (15., 6.)]);
        }
        Icon::Split => {
            box_at(3., 4., 18., 16., 1);
            path(&[(12., 4.), (12., 20.)]);
        }
        Icon::SplitRows => {
            box_at(4., 4., 16., 16., 2);
            path(&[(4., 12.), (20., 12.)]);
        }
        Icon::Panel => {
            box_at(3., 4., 18., 16., 1);
            path(&[(9., 4.), (9., 20.)]);
        }
        Icon::Clock => {
            p.circle_stroke(center, 9. * k, stroke);
            path(&[(12., 6.), (12., 12.), (16., 15.)]);
        }
        Icon::Globe => {
            p.circle_stroke(center, 9. * k, stroke);
            path(&[(3., 12.), (21., 12.)]);
            p.add(egui::Shape::ellipse_stroke(
                center,
                vec2(4. * k, 9. * k),
                stroke,
            ));
        }
        Icon::Check => path(&[(4., 12.), (9., 17.), (20., 6.)]),
        Icon::Key => {
            p.circle_stroke(point(8., 8.), 5. * k, stroke);
            path(&[(12., 12.), (21., 21.), (21., 17.), (17., 17.)]);
        }
        Icon::Shield => {
            path(&[
                (12., 2.),
                (21., 6.),
                (20., 15.),
                (16., 20.),
                (12., 23.),
                (8., 20.),
                (4., 15.),
                (3., 6.),
                (12., 2.),
            ]);
            path(&[(8., 12.), (11., 15.), (16., 9.)]);
        }
        Icon::Import => {
            path(&[(12., 3.), (12., 16.)]);
            path(&[(7., 11.), (12., 16.), (17., 11.)]);
            box_at(3., 18., 18., 4., 0);
        }
        Icon::Terminal => {
            path(&[(4., 6.), (10., 12.), (4., 18.)]);
            path(&[(12., 18.), (21., 18.)]);
        }
        Icon::Refresh => {
            path(&[
                (20., 8.),
                (17., 4.),
                (11., 3.),
                (5., 6.),
                (3., 12.),
                (6., 19.),
                (13., 21.),
                (19., 17.),
            ]);
            path(&[(20., 3.), (20., 8.), (15., 8.)]);
        }
        Icon::External => {
            path(&[(14., 3.), (21., 3.), (21., 10.)]);
            path(&[(21., 3.), (10., 14.)]);
            path(&[(10., 5.), (4., 5.), (4., 21.), (19., 21.), (19., 14.)]);
        }
        Icon::BottomPanel
        | Icon::Run
        | Icon::Bell
        | Icon::Github
        | Icon::TerminalBox
        | Icon::GitCommit
        | Icon::GitFork
        | Icon::Undo
        | Icon::GitMerge
        | Icon::Palette
        | Icon::Book
        | Icon::Speakerphone
        | Icon::Discord
        | Icon::Twitter
        | Icon::Linkedin
        | Icon::Eraser
        | Icon::CookieOff
        | Icon::Cookie
        | Icon::Tool
        | Icon::DotsVertical
        | Icon::Help
        | Icon::Flask
        | Icon::Workspace
        | Icon::Pin
        | Icon::Upload
        | Icon::Pinned
        | Icon::Login
        | Icon::ArrowLeft
        | Icon::Browser
        | Icon::BrightnessUp
        | Icon::ThemeMoon
        | Icon::DeviceDesktop
        | Icon::UserCircle
        | Icon::Certificate
        | Icon::Star
        | Icon::Lock
        | Icon::GitBranch
        | Icon::Keyboard
        | Icon::Sparkles
        | Icon::Database
        | Icon::Support
        | Icon::Beta
        | Icon::Info
        | Icon::Wand
        | Icon::MessageRefresh => unreachable!("embedded icon"),
    }
}
pub fn icon_button(ui: &mut Ui, r: Rect, kind: Icon, tip: &str, t: Theme) -> Response {
    let resp = ui.interact(r, ui.scope_id().with(tip), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r.shrink(2.), 4, t.surface);
    }
    icon(ui, r.center(), kind, t.muted, 16.);
    resp.on_hover_text(tip)
}
/// Table controls use an explicit ID and a shared vertical center. Their appearance
/// must not shift the auto-ID of a neighbouring editor when a blank row is filled.
pub fn table_checkbox(ui: &mut Ui, r: Rect, value: &mut bool, tip: &str, t: Theme) -> bool {
    let mut response = ui.interact(r, ui.scope_id().with("row-enabled"), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *value, tip)
    });
    let square = Rect::from_center_size(r.center(), vec2(14., 14.));
    ui.painter().rect(
        square,
        2,
        if *value { t.accent } else { t.bg },
        Stroke::new(
            1.,
            if *value || response.has_focus() {
                t.accent
            } else if response.hovered() {
                t.muted
            } else {
                t.strong_border
            },
        ),
        StrokeKind::Inside,
    );
    if *value {
        icon(
            ui,
            square.center(),
            Icon::Check,
            crate::theme::hex(0x191919),
            12.,
        );
    }
    let changed = response.changed();
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    changed
}
pub fn table_delete_button(ui: &mut Ui, r: Rect, tip: &str, t: Theme) -> Response {
    let response = ui.interact(r, ui.scope_id().with("row-delete"), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tip));
    // Bruno's table action: 18px Tabler trash, muted -> danger on hover (150ms).
    let hover = ui.ctx().animate_bool_with_time(
        response.id.with("hover"),
        response.hovered() || response.has_focus(),
        0.15,
    );
    let color = egui::Color32::from(
        egui::Rgba::from(t.muted) * (1. - hover) + egui::Rgba::from(t.red) * hover,
    );
    egui::Image::from_bytes(
        "bytes://icons/table-trash.svg",
        include_bytes!("../assets/icons/tree-menu/Trash.svg") as &[u8],
    )
    .tint(color)
    .paint_at(ui, Rect::from_center_size(r.center(), vec2(18., 18.)));
    response
        .on_hover_text(tip)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}
pub fn table_text<'a>(
    ui: &Ui,
    id: &'static str,
    text: &'a mut String,
    height: f32,
) -> TextEdit<'a> {
    TextEdit::singleline(text)
        .id(ui.scope_id().with(id))
        .frame(egui::Frame::NONE)
        .margin(egui::Margin::ZERO)
        .font(FontId::proportional(13.))
        .min_size(vec2(0., height))
        .align(Align2::LEFT_CENTER)
}
pub fn response_layout_button(
    ui: &mut Ui,
    r: Rect,
    side_by_side: bool,
    t: Theme,
    id: &'static str,
) -> Response {
    // Bruno shows the layout this action will switch to, in both toolbars.
    let (icon, title) = if side_by_side {
        (Icon::SplitRows, "Switch to vertical layout")
    } else {
        (Icon::Split, "Switch to horizontal layout")
    };
    ui.push_id(id, |ui| icon_button(ui, r, icon, title, t))
        .inner
}
pub fn tab_close_button(ui: &mut Ui, r: Rect, name: &str, t: Theme) -> Response {
    let response = ui.interact(r, ui.scope_id().with(("close-tab", name)), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(
            r,
            4,
            if t.dark {
                crate::theme::hex(0x1e1e1e)
            } else {
                t.surface
            },
        );
    }
    let color = if t.dark {
        crate::theme::hex(if response.hovered() {
            0xcccccc
        } else {
            0x9f9f9f
        })
    } else if response.hovered() {
        t.text
    } else {
        t.muted
    };
    icon(ui, r.center(), Icon::TabClose, color, 12.);
    response.on_hover_text(format!("Close {name}"))
}
pub fn button(ui: &mut Ui, r: Rect, text: &str, primary: bool, t: Theme) -> Response {
    let resp = ui.interact(r, ui.scope_id().with(text), Sense::click());
    let fill = if primary {
        t.accent
    } else if resp.hovered() {
        t.surface
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect(
        r,
        5,
        fill,
        Stroke::new(1., if primary { t.accent } else { t.border }),
        StrokeKind::Inside,
    );
    ui.painter().text(
        r.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(12.),
        if primary {
            crate::theme::hex(0x191919)
        } else {
            t.text
        },
    );
    resp
}
pub fn tab(ui: &mut Ui, r: Rect, name: &str, active: bool, t: Theme) -> Response {
    let response = ui.interact(r, ui.scope_id().with(name), Sense::click());
    label(
        ui,
        pos2(r.left(), r.center().y - 1.),
        name,
        12.,
        if active {
            t.text
        } else {
            crate::theme::hex(if t.dark { 0x999999 } else { 0x666666 })
        },
    );
    if active {
        line(
            ui,
            pos2(r.left(), r.bottom() - 1.),
            pos2(r.right(), r.bottom() - 1.),
            t.accent,
        );
        line(
            ui,
            pos2(r.left(), r.bottom() - 2.),
            pos2(r.right(), r.bottom() - 2.),
            t.accent,
        );
    }
    response
}
pub fn text_width(ui: &Ui, s: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(s.into(), FontId::proportional(size), Color32::WHITE)
        .size()
        .x
}
pub fn dropdown(
    ui: &mut Ui,
    r: Rect,
    label: &str,
    accent: bool,
    border: bool,
    t: Theme,
) -> Response {
    let response = ui.interact(r, ui.scope_id().with(("dropdown", label)), Sense::click());
    if border {
        ui.painter()
            .rect_stroke(r, 2, Stroke::new(1., t.strong_border), StrokeKind::Inside);
    }
    if response.hovered() {
        ui.painter().rect_filled(r.shrink(1.), 2, t.hover);
    }
    self::label(
        ui,
        pos2(r.left() + if border { 9. } else { 0. }, r.center().y),
        label,
        12.,
        if accent { t.accent } else { t.text },
    );
    icon(
        ui,
        pos2(r.right() - 12., r.center().y),
        Icon::Down,
        t.muted,
        12.,
    );
    response
}
pub fn child(ui: &mut Ui, r: Rect, id: impl std::hash::Hash + std::fmt::Debug) -> Ui {
    let mut child = ui.new_child(egui::UiBuilder::new().id_salt(id).max_rect(r));
    child.set_clip_rect(r.intersect(ui.clip_rect()));
    child
}

pub fn modal<R>(
    ctx: &egui::Context,
    title: &str,
    width: f32,
    t: Theme,
    content: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    modal_with_header(ctx, title, width, t, 40., |_, _| {}, content)
}
pub fn modal_with_header<R>(
    ctx: &egui::Context,
    title: &str,
    width: f32,
    t: Theme,
    header_height: f32,
    header_content: impl FnOnce(&mut Ui, Rect),
    content: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let id = egui::Id::unique(("dialog", title));
    let width = width.min(ctx.content_rect().width() - 32.);
    let mut close = false;
    let response = egui::Modal::new(id)
        .area(egui::Modal::default_area(id).anchor(Align2::CENTER_TOP, vec2(0., 50.)))
        .backdrop_color(Color32::from_black_alpha(128))
        .frame(
            egui::Frame::NONE
                .fill(crate::theme::hex(if t.dark { 0x222224 } else { 0xffffff }))
                .stroke(Stroke::new(1., t.border))
                .corner_radius(5),
        )
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            ui.visuals_mut().text_edit_bg_color = Some(Color32::TRANSPARENT);
            ui.visuals_mut().widgets.inactive.bg_stroke = Stroke::new(1., t.strong_border);
            ui.set_width(width - 2.);
            // Area remembers its previous size. Give scrolling content the current
            // viewport height so a dialog can grow after loading more results.
            ui.set_max_height((ctx.content_rect().height() - 100.).max(120.));
            let header = ui.allocate_space(vec2(width - 2., header_height)).1;
            ui.painter().rect_filled(
                header,
                egui::CornerRadius {
                    nw: 5,
                    ne: 5,
                    sw: 0,
                    se: 0,
                },
                t.bg,
            );
            (if header_height > 40. {
                medium
            } else {
                semibold
            })(
                ui,
                pos2(header.left() + 16., header.center().y),
                title,
                if header_height > 40. { 12. } else { 14. },
                t.text,
            );
            header_content(ui, header);
            ui.advance_cursor_after_rect(header);
            if icon_button(
                ui,
                rect(header.right() - 34., header.center().y - 12., 24., 24.),
                Icon::Close,
                "Close dialog",
                t,
            )
            .clicked()
            {
                close = true;
            }
            egui::Frame::NONE
                .inner_margin(16)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.;
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 150.).max(120.))
                        .show(ui, content)
                        .inner
                })
                .inner
        });
    let should_close = close || response.should_close();
    (response.inner, should_close)
}

pub fn segments(ui: &mut Ui, selected: &mut bool, labels: [&str; 2], t: Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(4., 0.);
        for (i, label) in labels.into_iter().enumerate() {
            let r = ui
                .allocate_space(vec2(text_width(ui, label, 12.) + 18., 32.))
                .1;
            let response = ui.interact(r, ui.scope_id().with(label), Sense::click());
            if *selected == (i == 1) || response.hovered() {
                ui.painter().rect_filled(r, 4, t.surface);
            }
            self::label(
                ui,
                pos2(r.left() + 9., r.center().y),
                label,
                12.,
                if *selected == (i == 1) {
                    t.text
                } else {
                    t.muted
                },
            );
            if response.clicked() {
                *selected = i == 1;
            }
        }
    });
}

pub fn assertions(ui: &mut Ui, rows: &mut Vec<Pair>, t: Theme) -> bool {
    let table_id = ui.scope_id().with("assertions-table");
    let operators = [
        ("eq", "equals"),
        ("neq", "not equals"),
        ("gt", "greater than"),
        ("gte", "greater than or equal"),
        ("lt", "less than"),
        ("lte", "less than or equal"),
        ("contains", "contains"),
        ("notContains", "not contains"),
        ("matches", "matches regex"),
        ("startsWith", "starts with"),
        ("endsWith", "ends with"),
        ("isJson", "is JSON"),
        ("isNumber", "is number"),
        ("isString", "is string"),
        ("isBoolean", "is boolean"),
        ("isArray", "is array"),
        ("isObject", "is object"),
        ("isNull", "is null"),
        ("isUndefined", "is undefined"),
        ("isEmpty", "is empty"),
        ("isNotEmpty", "is not empty"),
        ("length", "length"),
    ];
    ui.spacing_mut().item_spacing.y = 0.;
    if rows
        .last()
        .is_none_or(|p| !p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty())
    {
        rows.push(Pair::default());
    }
    let width = ui.available_width();
    let x = ui.cursor().min.x;
    let y = ui.cursor().min.y;
    let columns = [
        0.,
        (width * 0.222).max(120.),
        (width * 0.325).max(230.),
        width * 0.625,
        width * 0.875,
        width,
    ];
    let header = ui.allocate_space(vec2(width, 30.)).1;
    for (i, name) in ["Expr", "Operator", "Value", "Description"]
        .iter()
        .enumerate()
    {
        label(
            ui,
            pos2(x + columns[i] + if i == 0 { 37. } else { 11. }, y + 15.),
            name,
            13.,
            t.text,
        );
    }
    let mut changed = false;
    let mut remove = None;
    for (i, p) in rows.iter_mut().enumerate() {
        let r = ui.allocate_space(vec2(width, 35.5)).1;
        ui.push_id((table_id, i), |ui| {
            if !p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty() {
                changed |= table_checkbox(
                    ui,
                    rect(x + 3., r.top(), 24., r.height()),
                    &mut p.enabled,
                    "Include assertion",
                    t,
                );
            }
            changed |= ui
                .put(
                    rect(x + 37., r.top() + 1., columns[1] - 47., r.height() - 2.),
                    table_text(ui, "expression", &mut p.key, r.height() - 2.).hint_text("Expr"),
                )
                .changed();
            let (operator, value) = p.value.split_once(' ').unwrap_or(if p.value.is_empty() {
                ("eq", "")
            } else {
                (p.value.as_str(), "")
            });
            let mut operator = operator.to_owned();
            let mut value = value.to_owned();
            if !operators.iter().any(|(key, _)| *key == operator) {
                value = p.value.clone();
                operator = "eq".into();
            }
            let label = operators
                .iter()
                .find(|(key, _)| *key == operator)
                .map(|(_, label)| *label)
                .unwrap_or("equals");
            let select = dropdown(
                ui,
                rect(
                    x + columns[1] + 11.,
                    r.top() + 3.,
                    columns[2] - columns[1] - 20.,
                    29.,
                ),
                label,
                false,
                false,
                t,
            );
            let before = operator.clone();
            egui::Popup::menu(&select).show(|ui| {
                for (key, label) in operators {
                    if ui
                        .selectable_value(&mut operator, key.into(), label)
                        .clicked()
                    {
                        ui.close();
                    }
                }
            });
            let value_changed = ui
                .put(
                    rect(
                        x + columns[2] + 11.,
                        r.top() + 1.,
                        columns[3] - columns[2] - 21.,
                        r.height() - 2.,
                    ),
                    table_text(ui, "assertion-value", &mut value, r.height() - 2.),
                )
                .changed();
            if before != operator || value_changed {
                p.value = if value.is_empty() {
                    operator
                } else {
                    format!("{operator} {value}")
                };
                changed = true;
            }
            changed |= ui
                .put(
                    rect(
                        x + columns[3] + 11.,
                        r.top() + 1.,
                        columns[4] - columns[3] - 21.,
                        r.height() - 2.,
                    ),
                    table_text(ui, "description", &mut p.description, r.height() - 2.)
                        .hint_text("Description"),
                )
                .changed();
            if (!p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty())
                && table_delete_button(
                    ui,
                    Rect::from_center_size(
                        pos2((r.right() + x + columns[4]) * 0.5, r.center().y),
                        vec2(26., 26.),
                    ),
                    "Remove assertion",
                    t,
                )
                .clicked()
            {
                remove = Some(i);
            }
        });
        line(ui, r.left_bottom(), r.right_bottom(), t.border);
    }
    let bottom = ui.cursor().min.y;
    ui.painter().rect_stroke(
        Rect::from_min_max(header.min, pos2(x + width, bottom)),
        5,
        Stroke::new(1., t.border),
        StrokeKind::Inside,
    );
    line(ui, header.left_bottom(), header.right_bottom(), t.border);
    for col in &columns[1..5] {
        line(ui, pos2(x + col, y), pos2(x + col, bottom), t.border);
    }
    if let Some(index) = remove {
        rows.remove(index);
        changed = true;
    }
    changed
}

pub fn pairs(ui: &mut Ui, rows: &mut Vec<Pair>, labels: [&str; 2], t: Theme) -> bool {
    pairs_styled(ui, rows, labels, t, true)
}
fn pairs_styled(
    ui: &mut Ui,
    rows: &mut Vec<Pair>,
    labels: [&str; 2],
    t: Theme,
    bold: bool,
) -> bool {
    // Auto IDs also include ancestor widget counts (notifications, badges, etc.).
    pairs_table(ui, rows, labels, t, bold, &mut [])
}

fn pairs_table(
    ui: &mut Ui,
    rows: &mut Vec<Pair>,
    labels: [&str; 2],
    t: Theme,
    bold: bool,
    generated: &mut [crate::generated_headers::HeaderRow],
) -> bool {
    // Keep table identity inside the persistent scope; callers scope sibling tables.
    let table_id = ui.scope_id().with(("pairs-table", labels));
    let mut changed = false;
    ui.spacing_mut().item_spacing.y = 0.;
    if rows
        .last()
        .is_none_or(|p| !p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty())
    {
        rows.push(Pair::default());
    }
    let width = ui.available_width().max(280.);
    let first = 25. + (width - 2.) * 0.20;
    let second = width - 58. - (width - 2.) * 0.25;
    let start = ui.cursor().min;
    let h = 30.;
    let header = rect(start.x, start.y, width, h);
    if bold {
        ui.painter()
            .rect_stroke(header, 3, Stroke::new(1., t.border), StrokeKind::Inside);
    }
    let header_label = if bold { semibold } else { label };
    header_label(
        ui,
        pos2(start.x + 36., start.y + 15.),
        labels[0],
        13.,
        t.text,
    );
    header_label(
        ui,
        pos2(start.x + first + 10., start.y + 15.),
        labels[1],
        13.,
        t.text,
    );
    header_label(
        ui,
        pos2(start.x + second + 10., start.y + 15.),
        "Description",
        13.,
        t.text,
    );
    line(ui, header.left_bottom(), header.right_bottom(), t.border);
    ui.allocate_space(vec2(width, h));
    changed |= crate::generated_headers_ui::table_rows(ui, generated, width, first, second, t);
    let mut remove = None;
    let mut bottom = header.bottom();
    for (i, p) in rows.iter_mut().enumerate() {
        let r = ui.allocate_space(vec2(width, 35.5)).1;
        bottom = r.bottom();
        ui.push_id((table_id, i), |ui| {
            if !p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty() {
                changed |= table_checkbox(
                    ui,
                    rect(r.left() + 3., r.top(), 24., r.height()),
                    &mut p.enabled,
                    "Include row",
                    t,
                );
            }
            changed |= ui
                .put(
                    rect(r.left() + 36., r.top() + 1., first - 46., r.height() - 2.),
                    table_text(ui, "name", &mut p.key, r.height() - 2.).hint_text(labels[0]),
                )
                .changed();
            changed |= ui
                .put(
                    rect(
                        r.left() + first + 10.,
                        r.top() + 1.,
                        second - first - 20.,
                        r.height() - 2.,
                    ),
                    table_text(ui, "value", &mut p.value, r.height() - 2.)
                        .password(p.secret)
                        .hint_text(labels[1]),
                )
                .changed();
            changed |= ui
                .put(
                    rect(
                        r.left() + second + 10.,
                        r.top() + 1.,
                        (width - second - 68.).max(30.),
                        r.height() - 2.,
                    ),
                    table_text(ui, "description", &mut p.description, r.height() - 2.)
                        .hint_text(if bold { "Description" } else { "" }),
                )
                .changed();
            if (!p.key.is_empty() || !p.value.is_empty() || !p.description.is_empty())
                && table_delete_button(
                    ui,
                    Rect::from_center_size(pos2(r.right() - 29., r.center().y), vec2(26., 26.)),
                    "Remove row",
                    t,
                )
                .clicked()
            {
                remove = Some(i);
            }
        });
        line(
            ui,
            pos2(start.x, r.bottom()),
            pos2(start.x + width, r.bottom()),
            t.border,
        );
    }
    // Draw the shared grid once in the table's coordinate system, after editors.
    // Per-row child UIs may expand their bounds; their right edge is not the table edge.
    for x in [start.x + first, start.x + second, start.x + width - 58.] {
        line(ui, pos2(x, header.top()), pos2(x, bottom), t.border);
    }
    ui.painter().rect_stroke(
        Rect::from_min_max(header.min, pos2(header.right(), bottom)),
        if bold { 3 } else { 6 },
        Stroke::new(1., t.border),
        StrokeKind::Inside,
    );
    if let Some(i) = remove {
        rows.remove(i);
        changed = true;
    }
    changed
}

pub fn pairs_with_bulk(ui: &mut Ui, rows: &mut Vec<Pair>, labels: [&str; 2], t: Theme) -> bool {
    pairs_with_bulk_styled(ui, rows, labels, t, true)
}
pub fn folder_headers(ui: &mut Ui, rows: &mut Vec<Pair>, t: Theme) -> bool {
    pairs_with_bulk_styled(ui, rows, ["Name", "Value"], t, false)
}
fn pairs_with_bulk_styled(
    ui: &mut Ui,
    rows: &mut Vec<Pair>,
    labels: [&str; 2],
    t: Theme,
    bold: bool,
) -> bool {
    pairs_with_generated(ui, rows, labels, t, bold, &mut [])
}

pub fn pairs_with_generated(
    ui: &mut Ui,
    rows: &mut Vec<Pair>,
    labels: [&str; 2],
    t: Theme,
    bold: bool,
    generated: &mut [crate::generated_headers::HeaderRow],
) -> bool {
    let id = ui.scope_id().with(("bulk", labels));
    let mut state = ui
        .ctx()
        .data_mut(|d| d.get_temp::<(bool, String)>(id))
        .unwrap_or_default();
    let mut changed = false;
    if state.0 {
        if !generated.is_empty() {
            // Keep generated rows separate from the custom-header bulk buffer.
            let width = ui.available_width().max(280.);
            let first = 25. + (width - 2.) * 0.20;
            let second = width - 58. - (width - 2.) * 0.25;
            changed |=
                crate::generated_headers_ui::table_rows(ui, generated, width, first, second, t);
            ui.add_space(8.);
        }
        let bulk_changed = ui
            .add_sized(
                vec2(ui.available_width(), 120.),
                TextEdit::multiline(&mut state.1)
                    .font(FontId::monospace(13.))
                    .hint_text("Name: Value"),
            )
            .changed();
        changed |= bulk_changed;
        if bulk_changed {
            let old = std::mem::take(rows);
            *rows = state
                .1
                .lines()
                .filter_map(|s| s.split_once(':'))
                .map(|(key, value)| {
                    let enabled = !key.starts_with('~');
                    let key = key.trim_start_matches('~').trim();
                    let mut p = old
                        .iter()
                        .find(|p| p.key == key)
                        .cloned()
                        .unwrap_or_default();
                    p.key = key.to_owned();
                    p.value = value.trim().to_owned();
                    p.enabled = enabled;
                    p
                })
                .collect();
        }
    } else {
        changed |= pairs_table(ui, rows, labels, t, bold, generated);
    }
    let row = ui.allocate_space(vec2(ui.available_width(), 39.)).1;
    let label = if state.0 { "Table Edit" } else { "Bulk Edit" };
    let width = text_width(ui, label, 13.);
    let button = rect(
        row.right() - width,
        row.top() + if bold { 14. } else { 7. },
        width,
        20.,
    );
    self::label(
        ui,
        button.center() - vec2(width * 0.5, 0.),
        label,
        13.,
        t.blue,
    );
    if ui
        .interact(button, id.with("toggle"), Sense::click())
        .clicked()
    {
        state.0 = !state.0;
        if state.0 {
            state.1 = rows
                .iter()
                .filter(|p| !p.key.is_empty())
                .map(|p| format!("{}{}: {}", if p.enabled { "" } else { "~" }, p.key, p.value))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, state));
    changed
}

pub fn highlight(text: &str, t: Theme) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let mut i = 0;
    while i < text.len() {
        let start = i;
        let c = text[i..].chars().next().unwrap();
        let mut color = t.text;
        if c == '"' || c == '\'' || c == '`' {
            let quote = c;
            i += c.len_utf8();
            let mut escaped = false;
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == quote && !escaped {
                    break;
                }
                escaped = ch == '\\' && !escaped;
            }
            color = if text[i..].trim_start().starts_with(':') {
                t.syntax[0]
            } else {
                t.syntax[1]
            };
        } else if c.is_ascii_digit() || c == '-' {
            i += 1;
            while i < text.len()
                && matches!(
                    text.as_bytes()[i],
                    b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-'
                )
            {
                i += 1;
            }
            color = t.syntax[2];
        } else if text[i..].starts_with("/*") {
            i = text[i + 2..]
                .find("*/")
                .map(|n| i + n + 4)
                .unwrap_or(text.len());
            color = t.syntax[3];
        } else if text[i..].starts_with("//") {
            i = text[i..].find('\n').map(|n| i + n).unwrap_or(text.len());
            color = t.syntax[3];
        } else if c.is_alphabetic() || c == '_' {
            i += c.len_utf8();
            while i < text.len() {
                let ch = text[i..].chars().next().unwrap();
                if !ch.is_alphanumeric() && ch != '_' {
                    break;
                }
                i += ch.len_utf8();
            }
            if [
                "true", "false", "null", "const", "let", "var", "return", "function", "if", "else",
                "test",
            ]
            .contains(&&text[start..i])
            {
                color = t.syntax[4];
            }
        } else {
            i += c.len_utf8();
        }
        job.append(
            &text[start..i],
            0.,
            egui::TextFormat {
                font_id: FontId::monospace(13.),
                color,
                line_height: Some(20.),
                ..Default::default()
            },
        );
    }
    job
}
#[derive(Clone)]
struct EditorLayout {
    source: std::sync::Arc<str>,
    galley: std::sync::Arc<egui::Galley>,
    lines: usize,
    dark: bool,
    font: f32,
    generation: u64,
}
fn editor_layout(ui: &Ui, id: egui::Id, text: &str, t: Theme, font: f32) -> EditorLayout {
    let generation = ui
        .ctx()
        .data(|d| d.get_temp::<u64>(egui::Id::unique("editor-style-generation")))
        .unwrap_or(0);
    if let Some(cache) = ui.ctx().data_mut(|d| d.get_temp::<EditorLayout>(id)) {
        if cache.source.as_ref() == text
            && cache.dark == t.dark
            && cache.font == font
            && cache.generation == generation
        {
            return cache;
        }
    }
    let cache = EditorLayout {
        source: text.into(),
        galley: ui.fonts_mut(|f| f.layout_job(highlight_sized(text, t, font))),
        lines: text.lines().count().max(1),
        dark: t.dark,
        font,
        generation,
    };
    ui.ctx().data_mut(|d| {
        let key = egui::Id::unique("editor-layout-recent");
        let mut recent = d
            .get_temp::<std::collections::VecDeque<egui::Id>>(key)
            .unwrap_or_default();
        recent.retain(|old| *old != id);
        recent.push_back(id);
        while recent.len() > 8 {
            if let Some(old) = recent.pop_front() {
                d.remove::<EditorLayout>(old);
            }
        }
        d.insert_temp(key, recent);
        d.insert_temp(id, cache.clone());
    });
    cache
}
pub fn editor(ui: &mut Ui, text: &mut String, readonly: bool, t: Theme) -> bool {
    editor_with_placeholder(ui, text, readonly, t, "")
}

pub fn editor_with_placeholder(
    ui: &mut Ui,
    text: &mut String,
    readonly: bool,
    t: Theme,
    placeholder: &str,
) -> bool {
    let mut changed = false;
    let editor_id = ui.scope_id().with("source-editor");
    let ghost_id = editor_id.with("ai-ghost");
    if !readonly && ui.memory(|m| m.has_focus(editor_id)) {
        if let Some(mut ghost) = ui
            .ctx()
            .data_mut(|d| d.get_temp::<crate::ai_ui::Ghost>(ghost_id))
        {
            let cursor = egui::TextEdit::load_state(ui.ctx(), editor_id)
                .and_then(|s| s.cursor.char_range())
                .map(|r| r.primary.index.0);
            if ghost.fingerprint != crate::disk_sync::fingerprint(text)
                || cursor != Some(ghost.cursor)
            {
                ui.ctx()
                    .data_mut(|d| d.remove::<crate::ai_ui::Ghost>(ghost_id));
            } else {
                let accept = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                let word =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::ArrowRight));
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
                    ui.ctx()
                        .data_mut(|d| d.remove::<crate::ai_ui::Ghost>(ghost_id));
                } else if accept || word {
                    let count = if accept {
                        ghost.text.len()
                    } else {
                        ghost
                            .text
                            .char_indices()
                            .skip_while(|(_, c)| c.is_whitespace())
                            .find(|(_, c)| c.is_whitespace())
                            .map(|(i, c)| i + c.len_utf8())
                            .unwrap_or(ghost.text.len())
                    };
                    let insert = ghost.text[..count].to_owned();
                    let byte = text
                        .char_indices()
                        .nth(ghost.cursor)
                        .map(|(i, _)| i)
                        .unwrap_or(text.len());
                    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), editor_id) {
                        let mut undo = state.undoer();
                        let previous =
                            egui::text::CCursorRange::one(egui::text::CCursor::new(ghost.cursor));
                        undo.add_undo(&(previous, text.clone()));
                        text.insert_str(byte, &insert);
                        ghost.cursor += insert.chars().count();
                        let cursor =
                            egui::text::CCursorRange::one(egui::text::CCursor::new(ghost.cursor));
                        undo.add_undo(&(cursor, text.clone()));
                        state.set_undoer(undo);
                        state.cursor.set_char_range(Some(cursor));
                        state.store(ui.ctx(), editor_id);
                    }
                    ghost.text.drain(..count);
                    ghost.fingerprint = crate::disk_sync::fingerprint(text);
                    ui.ctx().data_mut(|d| {
                        if ghost.text.is_empty() {
                            d.remove::<crate::ai_ui::Ghost>(ghost_id);
                        } else {
                            d.insert_temp(ghost_id, ghost);
                        }
                    });
                    changed = true;
                }
            }
        }
    }
    let toggle_comment = !readonly && ui.memory(|m| m.has_focus(editor_id)) && ui.input_mut(|i| {
        let mut toggle = false;
        i.events.retain(|event| {
            if matches!(event, egui::Event::Key { key, physical_key, pressed: true, modifiers, .. }
                if modifiers.ctrl && !modifiers.alt && (*key == egui::Key::Slash || *physical_key == Some(egui::Key::Slash))) {
                toggle = true; false
            } else { true }
        });
        toggle
    });
    let font_size = code_font_size(ui);
    let line_height = font_size * 20. / 13.;
    if readonly {
        viewer(ui, text, &line_offsets(text), t);
        return false;
    }
    let cache_id = ui.scope_id().with("editor-highlight-layout");
    let cache = editor_layout(ui, cache_id, text, t, font_size);
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            ui.horizontal_top(|ui| {
                let count = cache.lines;
                let gutter = 36.;
                let origin = ui.cursor().min;
                let height = (count as f32 * line_height).max(ui.available_height());
                let (gutter_rect, _) = ui.allocate_exact_size(vec2(gutter, height), Sense::hover());
                let visible = ui.clip_rect();
                let first = ((visible.top() - origin.y) / line_height).floor().max(0.) as usize;
                let last = (((visible.bottom() - origin.y) / line_height).ceil().max(0.) as usize
                    + 1)
                .min(count);
                for n in first.min(count)..last {
                    let y = origin.y + n as f32 * line_height + line_height * 0.45;
                    if y >= visible.top() - line_height && y < visible.bottom() + line_height {
                        mono(
                            ui,
                            pos2(gutter_rect.left() + 4., y),
                            &(n + 1).to_string(),
                            font_size,
                            t.muted,
                        );
                    }
                }
                if readonly {
                    let job = highlight_sized(text, t, font_size);
                    ui.add(
                        egui::Label::new(job)
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                } else {
                    let mut layouter = |ui: &Ui, buffer: &dyn egui::TextBuffer, _width: f32| {
                        editor_layout(ui, cache_id, buffer.as_str(), t, font_size).galley
                    };
                    let mut output = TextEdit::multiline(text)
                        .id(editor_id)
                        .font(FontId::monospace(font_size))
                        .hint_text(
                            egui::RichText::new(placeholder).font(FontId::monospace(font_size)),
                        )
                        .frame(egui::Frame::NONE)
                        .desired_width(f32::INFINITY)
                        .min_size(vec2(ui.available_width().max(400.), height))
                        .layouter(&mut layouter)
                        .code_editor()
                        .lock_focus(true)
                        .margin(vec2(4., 0.))
                        .show(ui);
                    changed |= output.response.changed();
                    if let Some(range) = output.cursor_range {
                        let position = output.galley_pos
                            + output.galley.pos_from_cursor(range.primary).min.to_vec2();
                        let focused = output.response.has_focus();
                        ui.ctx().data_mut(|d| {
                            d.insert_temp(
                                editor_id.with("ai-cursor"),
                                crate::ai_ui::Cursor {
                                    index: range.primary.index.0,
                                    focused,
                                },
                            )
                        });
                        if output.response.has_focus() {
                            if let Some(ghost) = ui
                                .ctx()
                                .data_mut(|d| d.get_temp::<crate::ai_ui::Ghost>(ghost_id))
                            {
                                if ghost.cursor == range.primary.index.0
                                    && ghost.fingerprint == crate::disk_sync::fingerprint(text)
                                {
                                    ui.painter().with_clip_rect(output.text_clip_rect).text(
                                        position,
                                        egui::Align2::LEFT_TOP,
                                        &ghost.text,
                                        FontId::monospace(font_size),
                                        t.muted.gamma_multiply(0.5),
                                    );
                                    ui.ctx().data_mut(|d| {
                                        d.insert_temp(egui::Id::unique("ai-active-ghost"), ghost_id)
                                    });
                                }
                            }
                        }
                    }
                    if toggle_comment {
                        if let Some(range) = output.cursor_range {
                            let mut undoer = output.state.undoer();
                            undoer.add_undo(&(range, text.clone()));
                            let (updated, selection) = crate::json_comments::toggle_lines(
                                text,
                                range.as_sorted_char_range().start.0
                                    ..range.as_sorted_char_range().end.0,
                            );
                            *text = updated;
                            let selected = egui::text::CCursorRange::two(
                                egui::text::CCursor::new(selection.start),
                                egui::text::CCursor::new(selection.end),
                            );
                            undoer.add_undo(&(selected, text.clone()));
                            output.state.set_undoer(undoer);
                            output.state.cursor.set_char_range(Some(selected));
                            output.state.store(ui.ctx(), output.response.id);
                            changed = true;
                            ui.ctx().request_repaint();
                        }
                    }
                }
            });
        });
    changed
}

pub use crate::text::line_offsets;
pub fn viewer(ui: &mut Ui, text: &str, offsets: &[usize], t: Theme) {
    let font_size = code_font_size(ui);
    let line_height = font_size * 20. / 13.;
    ui.spacing_mut().item_spacing = Vec2::ZERO;
    ui.spacing_mut().interact_size.y = line_height;
    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_rows(ui, line_height, offsets.len().max(1), |ui, range| {
            for i in range {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(36., line_height), Sense::hover());
                    mono(
                        ui,
                        pos2(r.left() + 4., r.top() + line_height * 0.45),
                        &(i + 1).to_string(),
                        font_size,
                        t.muted,
                    );
                    let a = offsets.get(i).copied().unwrap_or(0);
                    let b = offsets.get(i + 1).copied().unwrap_or(text.len());
                    let line = text.get(a..b).unwrap_or("").trim_end_matches(['\r', '\n']);
                    ui.add(
                        egui::Label::new(highlight_sized(line, t, font_size))
                            .selectable(true)
                            .wrap_mode(egui::TextWrapMode::Extend),
                    );
                });
            }
        });
}

fn code_font_size(ui: &Ui) -> f32 {
    ui.ctx()
        .data_mut(|d| d.get_temp::<f32>(egui::Id::unique("editor-font-size")))
        .unwrap_or(13.)
}
fn highlight_sized(text: &str, t: Theme, size: f32) -> egui::text::LayoutJob {
    let mut job = highlight(text, t);
    for section in &mut job.sections {
        section.format.font_id.size = size;
        section.format.line_height = Some(size * 20. / 13.);
    }
    job
}
