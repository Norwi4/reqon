use crate::{
    model::Preferences,
    preferences::*,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Sense, Stroke, StrokeKind, TextEdit, pos2, vec2};
pub fn themes(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, legacy_dark: bool) -> bool {
    let mut changed = false;
    ui.add_space(15.);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.;
        for (mode, name, icon) in [
            ("light", "Light", Icon::BrightnessUp),
            ("dark", "Dark", Icon::ThemeMoon),
            ("system", "System", Icon::DeviceDesktop),
        ] {
            let selected = p.appearance.mode == mode
                || p.appearance.mode == "legacy"
                    && mode == if legacy_dark { "dark" } else { "light" };
            let width = w::text_width(ui, name, 13.) + 42.;
            let row = ui.allocate_space(vec2(width, 34.)).1;
            ui.painter().rect(
                row,
                8,
                if selected {
                    t.bg.blend(t.accent.gamma_multiply(0.07))
                } else {
                    t.bg
                },
                Stroke::new(1., if selected { t.accent } else { t.strong_border }),
                StrokeKind::Inside,
            );
            w::icon(
                ui,
                pos2(row.left() + 17., row.center().y),
                icon,
                t.text,
                16.,
            );
            w::label(
                ui,
                pos2(row.left() + 33., row.center().y),
                name,
                13.,
                t.text,
            );
            if ui
                .interact(
                    row,
                    ui.scope_id().with(("theme-mode", mode)),
                    Sense::click(),
                )
                .clicked()
            {
                p.appearance.mode = mode.into();
                changed = true;
            }
        }
    });
    ui.add_space(21.);
    ui.separator();
    ui.add_space(12.);
    let system = p.appearance.mode == "system";
    for (dark, label) in [(false, "Light Theme"), (true, "Dark Theme")] {
        if !system
            && ((p.appearance.mode == "dark" || p.appearance.mode == "legacy" && legacy_dark)
                != dark)
        {
            continue;
        }
        ui.add_space(8.);
        ui.label(egui::RichText::new(label).size(12.).color(t.muted));
        ui.add_space(6.);
        let presets = crate::theme_registry::presets()
            .iter()
            .filter(|x| (x.mode == "dark") == dark)
            .collect::<Vec<_>>();
        let columns = ((ui.available_width() + 12.) / 167.).floor().max(1.) as usize;
        for row in presets.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.;
                for preset in row {
                    let id = if dark {
                        &mut p.appearance.dark
                    } else {
                        &mut p.appearance.light
                    };
                    let selected = *id == preset.id;
                    let r = ui.allocate_space(vec2(155., 94.)).1;
                    let colors = preset.palette();
                    ui.painter().rect(
                        r,
                        8,
                        if selected {
                            t.bg.blend(t.accent.gamma_multiply(0.07))
                        } else {
                            t.bg
                        },
                        Stroke::new(2., if selected { t.accent } else { t.strong_border }),
                        StrokeKind::Inside,
                    );
                    let preview = rect(r.center().x - 30., r.top() + 14., 60., 40.);
                    ui.painter().rect(
                        preview,
                        3,
                        colors.bg,
                        Stroke::new(1., colors.accent.gamma_multiply(0.5)),
                        StrokeKind::Inside,
                    );
                    ui.painter().rect_filled(
                        rect(preview.left() + 1., preview.top() + 1., 14., 38.),
                        0,
                        colors.sidebar,
                    );
                    for (index, width) in [28., 21., 24.5].iter().enumerate() {
                        ui.painter().rect_filled(
                            rect(
                                preview.left() + 19.,
                                preview.top() + 6. + index as f32 * 7.,
                                *width,
                                4.,
                            ),
                            1,
                            colors.accent.gamma_multiply(0.5),
                        );
                    }
                    centered(
                        ui,
                        pos2(r.center().x, r.bottom() - 23.),
                        &preset.name,
                        12.,
                        t.text,
                    );
                    if ui
                        .interact(r, ui.scope_id().with(&preset.id), Sense::click())
                        .clicked()
                    {
                        *id = preset.id.clone();
                        changed = true;
                    }
                }
            });
            ui.add_space(4.);
        }
        if system && !dark {
            let y = ui.cursor().min.y + 20.;
            ui.painter().line_segment(
                [
                    pos2(ui.min_rect().left(), y),
                    pos2(ui.max_rect().right(), y),
                ],
                Stroke::new(1., t.strong_border),
            );
            ui.add_space(44.);
        } else {
            ui.add_space(24.);
        }
    }
    changed
}
pub fn display(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    let origin = ui.cursor().min;
    for (text, offset) in [
        ("Code Editor Font", vec2(0., 23.)),
        ("Font Size", vec2(308., 23.)),
        ("Interface Zoom", vec2(0., 126.)),
    ] {
        w::label(ui, origin + offset, text, 13., t.text);
    }
    let response = ui.put(
        w::rect(origin.x, origin.y + 42., 300., 36.),
        TextEdit::singleline(&mut p.appearance.font).margin(vec2(7., 8.)),
    );
    changed |= response.changed();
    if response.lost_focus() {
        if p.appearance.font.trim().is_empty() {
            p.appearance.font = "default".into();
            changed = true;
        }
        state.error = if p.appearance.font != "default"
            && crate::theme_registry::font_bytes(&p.appearance.font).is_none()
        {
            "Font not found. Using the default code font.".into()
        } else {
            String::new()
        };
    }
    let id = ui.scope_id().with("editor-font-size");
    let mut size = ui
        .ctx()
        .data_mut(|d| d.get_temp::<(f32, String)>(id))
        .unwrap_or((p.editor_font_size, p.editor_font_size.to_string()));
    if size.0 != p.editor_font_size {
        size = (p.editor_font_size, p.editor_font_size.to_string());
    }
    let input = ui.put(
        w::rect(origin.x + 308., origin.y + 42., 75., 36.),
        TextEdit::singleline(&mut size.1)
            .id(id.with("input"))
            .margin(vec2(7., 8.)),
    );
    if input.changed() {
        if let Ok(next) = size.1.parse::<u32>() {
            p.editor_font_size = next.clamp(1, 32) as f32;
            size.0 = p.editor_font_size;
            changed = true;
        }
    }
    if input.lost_focus() {
        size.1 = p.editor_font_size.to_string();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, size));
    if !state.error.is_empty() {
        w::label(ui, origin + vec2(0., 96.), &state.error, 12., t.muted);
    }
    let button = w::rect(origin.x, origin.y + 145., 76., 36.);
    let response = ui.interact(button, ui.scope_id().with("interface-zoom"), Sense::click());
    ui.painter().rect(
        button,
        4,
        t.bg,
        Stroke::new(1., t.strong_border),
        StrokeKind::Inside,
    );
    w::label(
        ui,
        pos2(button.left() + 8., button.center().y),
        &format!("{}%", p.appearance.zoom),
        13.,
        t.text,
    );
    w::icon(
        ui,
        pos2(button.right() - 14., button.center().y),
        Icon::Down,
        t.muted,
        14.,
    );
    egui::Popup::from_toggle_button_response(&response)
        .gap(4.)
        .width(80.)
        .frame(
            egui::Frame::new()
                .fill(t.bg)
                .stroke(Stroke::new(1., t.strong_border))
                .corner_radius(4)
                .inner_margin(0),
        )
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.;
            egui::ScrollArea::vertical()
                .max_height(200.)
                .show(ui, |ui| {
                    for zoom in (50..=150).step_by(10) {
                        let row = ui.allocate_space(vec2(78., 35.5)).1;
                        let option = ui.interact(row, ui.scope_id().with(zoom), Sense::click());
                        if option.hovered() || p.appearance.zoom == zoom {
                            ui.painter().rect_filled(row, 0, t.surface);
                        }
                        w::label(
                            ui,
                            pos2(row.left() + 12., row.center().y),
                            &format!("{zoom}%"),
                            13.,
                            t.text,
                        );
                        if p.appearance.zoom == zoom {
                            if response.clicked() {
                                ui.scroll_to_rect(row, None);
                            }
                            w::icon(
                                ui,
                                pos2(row.right() - 13., row.center().y),
                                Icon::Check,
                                t.blue,
                                12.,
                            );
                        }
                        if option.clicked() {
                            p.appearance.zoom = zoom;
                            changed = true;
                            ui.close();
                        }
                    }
                });
        });
    if p.appearance.zoom != 100
        && w::icon_button(
            ui,
            w::rect(button.right() + 4., button.top(), 36., 36.),
            Icon::Refresh,
            "Reset zoom",
            t,
        )
        .clicked()
    {
        p.appearance.zoom = 100;
        changed = true;
    }
    ui.allocate_rect(w::rect(origin.x, origin.y + 190., 1., 1.), Sense::hover());
    changed
}
pub fn support(ui: &mut egui::Ui, t: Theme) {
    for (group, rows) in [
        (
            "Help",
            vec![
                (
                    Icon::Book,
                    "Documentation",
                    "Guides, references and examples",
                    "https://reqon.ru/docs/",
                ),
                (
                    Icon::Speakerphone,
                    "Report Issues",
                    "File a bug or request a feature",
                    "https://github.com/Norwi4/reqon/issues",
                ),
            ],
        ),
        (
            "Reqon",
            vec![
                (
                    Icon::Github,
                    "GitHub",
                    "Source, releases and discussions",
                    "https://github.com/Norwi4/reqon",
                ),
                (
                    Icon::External,
                    "Website",
                    "News and downloads",
                    "https://reqon.ru/",
                ),
            ],
        ),
    ] {
        ui.add_space(19.);
        ui.label(group);
        ui.add_space(2.);
        let group_rect = rect(
            ui.cursor().min.x,
            ui.cursor().min.y,
            ui.available_width(),
            rows.len() as f32 * 63. + 1.,
        );
        ui.painter()
            .rect_stroke(group_rect, 6, Stroke::new(1., t.border), StrokeKind::Inside);
        for (index, (icon, name, desc, url)) in rows.into_iter().enumerate() {
            let r = ui.allocate_space(vec2(ui.available_width(), 63.)).1;
            if index > 0 {
                w::line(ui, r.left_top(), r.right_top(), t.border);
            }
            let tile = rect(r.left() + 15., r.top() + 19., 26., 26.);
            ui.painter().rect_filled(tile, 6, t.surface);
            w::icon(ui, tile.center(), icon, t.muted, 16.);
            w::label(ui, pos2(r.left() + 53., r.top() + 23.), name, 13., t.text);
            w::label(ui, pos2(r.left() + 53., r.top() + 44.), desc, 11., t.muted);
            w::icon(
                ui,
                pos2(r.right() - 23., r.center().y),
                Icon::External,
                t.muted,
                14.,
            );
            if ui
                .interact(r, ui.scope_id().with(name), Sense::click())
                .clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
            ui.add_space(-8.);
        }
        ui.add_space(1.);
    }
}
pub fn beta(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, tab: &mut usize) -> bool {
    let mut changed = false;
    ui.add_space(12.);
    hint(
        ui,
        "Beta features are experimental previews that may change before full release. Try them and share feedback.",
        t,
    );
    ui.add_space(34.);
    for (name, desc, link, target) in [
        (
            "AI Assistant",
            "Generate scripts, tests, and documentation directly from the request tab. Includes contextual chat, scripting autocomplete, and support for OpenAI, Anthropic, and OpenAI-compatible providers using your own API key.",
            "ai",
            Some(9),
        ),
        (
            "File cache",
            "Speeds up how quickly your collections open by keeping a local cache on disk. Turn it on or clear it anytime from the Cache settings.",
            "",
            Some(10),
        ),
        (
            "Akamai EdgeGrid",
            "Sign requests with the Akamai EdgeGrid authentication scheme. Select it from the Auth type dropdown on any request, folder, or collection.",
            "auth",
            None,
        ),
        (
            "Mock Server",
            "Run a local mock server using response examples defined in your collection. Serve mock API responses for frontend development without a real backend.",
            "mock-server",
            None,
        ),
        (
            "gRPC Scripting",
            "Write scripts for gRPC requests. Inspect messages, metadata, and status across the call lifecycle.",
            "grpc-scripting",
            None,
        ),
    ] {
        title(ui, name, t);
        ui.horizontal_wrapped(|ui| {
            hint(ui, desc, t);
            if name == "Mock Server" {
                changed |= toggle(ui, &mut p.features.mock_server, t);
            }
        });
        ui.horizontal(|ui| {
            if let Some(target) = target {
                if ui
                    .link(if target == 9 {
                        "Go to AI settings  →"
                    } else {
                        "Go to Cache settings  →"
                    })
                    .clicked()
                {
                    *tab = target;
                }
            }
            if !link.is_empty() {
                ui.hyperlink_to("View docs ↗", "https://reqon.ru/docs/");
            }
        });
        separator(ui);
    }
    changed
}
pub fn about(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, root: &std::path::Path) -> bool {
    ui.add_space(10.);
    row_text(ui, "Version", concat!("v", env!("CARGO_PKG_VERSION")), t);
    ui.horizontal(|ui| {
        field_label(ui, "AppData Path", 115., 24.);
        if ui
            .link(root.display().to_string())
            .on_hover_text("Reveal in File Explorer")
            .clicked()
        {
            let _ = std::process::Command::new("explorer.exe").arg(root).spawn();
        }
    });
    ui.add_space(16.);
    ui.label("Software Updates");
    let check_updates = ui.button("Check for updates").clicked();
    hint(ui, "Download and install new versions when you choose to update.", t);
    ui.add_space(24.);
    ui.label("Telemetry");
    ui.add_enabled_ui(false, |ui| {
        toggle_row(
            ui,
            &mut p.about.telemetry,
            "Anonymous telemetry is disabled",
            t,
        );
    });
    ui.add_space(22.);
    hint(ui, "Reqon · API client for HTTP, gRPC and WebSocket.", t);
    check_updates
}
