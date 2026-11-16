use eframe::egui::{self, Color32, FontFamily, FontId, Stroke};

pub fn hex(n: u32) -> Color32 {
    Color32::from_rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
}
#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub bg: Color32,
    pub sidebar: Color32,
    pub surface: Color32,
    pub hover: Color32,
    pub border: Color32,
    pub strong_border: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub accent: Color32,
    pub green: Color32,
    pub red: Color32,
    pub blue: Color32,
    pub orange: Color32,
    pub pink: Color32,
    pub methods: [Color32; 10],
    pub syntax: [Color32; 6],
}
impl Theme {
    pub fn new(dark: bool) -> Self {
        if dark {
            Self {
                dark,
                bg: hex(0x1a1a1a),
                sidebar: hex(0x1a1a1a),
                surface: hex(0x26292b),
                hover: hex(0x222224),
                border: hex(0x333333),
                strong_border: hex(0x444444),
                text: hex(0xcccccc),
                muted: hex(0xaaaaaa),
                faint: hex(0x999999),
                accent: hex(0xe4ad49),
                green: hex(0x73e59a),
                red: hex(0xe36451),
                blue: hex(0x8bc4f7),
                orange: hex(0xf6bd89),
                pink: hex(0xe496d9),
                methods: [
                    0x73d3a6, 0xffe47e, 0x74aef6, 0xf69d90, 0xc0a8e1, 0xef63a3, 0x73d3a6, 0x8ab2e0,
                    0xf6ab79, 0xe496d9,
                ]
                .map(hex),
                syntax: [0x8bc2f9, 0xe6bf89, 0xe496dd, 0x999999, 0xf38172, 0xcccccc].map(hex),
            }
        } else {
            Self {
                dark,
                bg: hex(0xffffff),
                sidebar: hex(0xf8f8f8),
                surface: hex(0xeaeaea),
                hover: hex(0xf1f1f1),
                border: hex(0xe5e5e5),
                strong_border: hex(0xcccccc),
                text: hex(0x343434),
                muted: hex(0x838383),
                faint: hex(0x9b9b9b),
                accent: hex(0xd37f17),
                green: hex(0x2e8a54),
                red: hex(0xce4f3b),
                blue: hex(0x346ab2),
                orange: hex(0xc67a10),
                pink: hex(0xb83d7e),
                methods: [
                    0x146b3a, 0x835700, 0x1e61b6, 0xb23132, 0x7541a0, 0xa82975, 0x146b3a, 0x3c65a6,
                    0xc67a10, 0xb83d7e,
                ]
                .map(hex),
                syntax: [0x346ab2, 0xc67a10, 0xb83d7e, 0x9b9b9b, 0xb83d7e, 0x343434].map(hex),
            }
        }
    }
    pub fn method(&self, m: &str) -> Color32 {
        [
            "GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD", "GRPC", "WS", "GQL",
        ]
        .iter()
        .position(|v| {
            v.eq_ignore_ascii_case(if m == "DEL" {
                "DELETE"
            } else if m == "OPT" {
                "OPTIONS"
            } else {
                m
            })
        })
        .map(|i| self.methods[i])
        .unwrap_or(self.blue)
    }
    pub fn apply(&self, ctx: &egui::Context) {
        let mut s = egui::Style {
            visuals: if self.dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            },
            ..Default::default()
        };
        s.visuals.override_text_color = Some(self.text);
        s.visuals.weak_text_color = Some(hex(if self.dark { 0x666666 } else { 0xaaaaaa }));
        s.visuals.panel_fill = self.bg;
        s.visuals.window_fill = if self.dark { self.hover } else { self.bg };
        s.visuals.extreme_bg_color = self.bg;
        s.visuals.faint_bg_color = self.hover;
        s.visuals.window_stroke = Stroke::new(1., self.border);
        s.visuals.selection.bg_fill = hex(if self.dark { 0x2d2d2d } else { 0xd9d9d9 });
        s.visuals.selection.stroke = Stroke::new(1., self.text);
        s.visuals.selection.preserve_text_color = true;
        s.visuals.selection.extend_to_line_end = true;
        for v in [
            &mut s.visuals.widgets.inactive,
            &mut s.visuals.widgets.noninteractive,
        ] {
            v.bg_fill = self.bg;
            v.weak_bg_fill = self.bg;
            v.bg_stroke = Stroke::new(1., self.border);
            v.fg_stroke = Stroke::new(1., self.text);
            v.corner_radius = 4.into();
        }
        s.visuals.widgets.hovered.bg_fill = self.surface;
        s.visuals.widgets.hovered.weak_bg_fill = self.surface;
        s.visuals.widgets.hovered.bg_stroke = Stroke::new(1., self.strong_border);
        s.visuals.widgets.hovered.fg_stroke = Stroke::new(1., self.text);
        s.visuals.widgets.active.bg_fill = self.surface;
        s.visuals.widgets.active.weak_bg_fill = self.surface;
        s.visuals.widgets.active.bg_stroke = Stroke::new(1., self.accent);
        s.visuals.widgets.active.fg_stroke = Stroke::new(1., self.text);
        for name in [egui::TextStyle::Body, egui::TextStyle::Button] {
            s.text_styles.insert(name, FontId::proportional(13.));
        }
        s.text_styles
            .insert(egui::TextStyle::Small, FontId::proportional(11.));
        s.text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(13.));
        s.text_styles
            .insert(egui::TextStyle::Heading, FontId::proportional(20.));
        s.spacing.item_spacing = egui::vec2(8., 8.);
        s.spacing.button_padding = egui::vec2(10., 6.);
        s.spacing.interact_size.y = 28.;
        s.spacing.icon_width = 14.;
        s.spacing.icon_width_inner = 10.;
        // Bruno's narrow scrollbars and CodeMirror thumb colors, shared by
        // every pane. Keep width, opacity and color constant on hover/drag.
        s.spacing.scroll = egui::style::ScrollStyle {
            floating: true,
            bar_width: 5.,
            floating_width: 5.,
            floating_allocated_width: 6.,
            bar_inner_margin: 1.,
            bar_outer_margin: 0.,
            handle_color: Some(hex(if self.dark { 0x444444 } else { 0xd2d7db })),
            dormant_handle_opacity: 1.,
            active_handle_opacity: 1.,
            interact_handle_opacity: 1.,
            dormant_background_opacity: 0.,
            active_background_opacity: 0.,
            interact_background_opacity: 0.,
            ..egui::style::ScrollStyle::floating()
        };
        s.animation_time = 0.;
        ctx.set_theme(if self.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.set_style_of(
            if self.dark {
                egui::Theme::Dark
            } else {
                egui::Theme::Light
            },
            s,
        );
    }
}
pub fn fonts(ctx: &egui::Context) {
    editor_fonts(ctx, "default");
}
pub fn editor_fonts(ctx: &egui::Context, family: &str) {
    let mut f = egui::FontDefinitions::default();
    f.font_data.insert(
        "Inter 400 latin".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Bruno-Inter-latin-400.ttf"))
            .into(),
    );
    f.font_data.insert(
        "Inter 400 cyrillic".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-cyrillic-400.ttf"
        ))
        .into(),
    );
    f.font_data.insert(
        "Inter 400 latin-ext".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-latin-ext-400.ttf"
        ))
        .into(),
    );
    f.font_data.insert(
        "Inter 500 latin".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Bruno-Inter-latin-500.ttf"))
            .into(),
    );
    f.font_data.insert(
        "Inter 500 cyrillic".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-cyrillic-500.ttf"
        ))
        .into(),
    );
    f.font_data.insert(
        "Inter 500 latin-ext".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-latin-ext-500.ttf"
        ))
        .into(),
    );
    f.font_data.insert(
        "Inter 600 latin".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Bruno-Inter-latin-600.ttf"))
            .into(),
    );
    f.font_data.insert(
        "Inter 600 cyrillic".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-cyrillic-600.ttf"
        ))
        .into(),
    );
    f.font_data.insert(
        "Inter 600 latin-ext".into(),
        egui::FontData::from_static(include_bytes!(
            "../assets/fonts/Bruno-Inter-latin-ext-600.ttf"
        ))
        .into(),
    );
    let (bytes, index) = crate::theme_registry::font_bytes(family).unwrap_or_else(|| {
        (
            std::fs::read("C:/Windows/Fonts/consola.ttf").unwrap_or_else(|_| {
                include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf").to_vec()
            }),
            0,
        )
    });
    let mut code = egui::FontData::from_owned(bytes);
    code.index = index;
    f.font_data.insert("Code".into(), code.into());
    let regular = vec![
        "Inter 400 latin".into(),
        "Inter 400 cyrillic".into(),
        "Inter 400 latin-ext".into(),
    ];
    f.families
        .get_mut(&FontFamily::Proportional)
        .unwrap()
        .splice(0..0, regular);
    f.families
        .get_mut(&FontFamily::Monospace)
        .unwrap()
        .insert(0, "Code".into());
    f.families.insert(
        FontFamily::Name("Medium".into()),
        vec![
            "Inter 500 latin".into(),
            "Inter 500 cyrillic".into(),
            "Inter 500 latin-ext".into(),
        ],
    );
    f.families.insert(
        FontFamily::Name("Semibold".into()),
        vec![
            "Inter 600 latin".into(),
            "Inter 600 cyrillic".into(),
            "Inter 600 latin-ext".into(),
        ],
    );
    let terminal_font = std::env::var_os("WINDIR")
        .and_then(|p| std::fs::read(std::path::PathBuf::from(p).join("Fonts/cour.ttf")).ok());
    let terminal_fonts = if let Some(data) = terminal_font {
        f.font_data.insert(
            "Courier New".into(),
            egui::FontData::from_owned(data).into(),
        );
        vec!["Courier New".into(), "Code".into()]
    } else {
        vec!["Code".into()]
    };
    f.families
        .insert(FontFamily::Name("Terminal".into()), terminal_fonts);
    ctx.set_fonts(f);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_preserves_syntax_colors_background_and_copied_text() {
        use egui::text::{CCursor, CCursorRange};
        for dark in [true, false] {
            let ctx = egui::Context::default();
            fonts(&ctx);
            let theme = Theme::new(dark);
            theme.apply(&ctx);
            let original =
                "{\n  \"text\": \"Привет\", // comment\n\n  \"number\": 42,\n  \"flag\": true\n}";
            let mut text = original.to_owned();
            let end_byte = original.find("42").unwrap() + 2;
            let range = CCursorRange::two(
                CCursor::new(4),
                CCursor::new(original[..end_byte].chars().count()),
            );
            let id = egui::Id::unique("selection-regression");
            let mut selected_galley = None;
            let mut baseline = None;
            let mut bg = Color32::TRANSPARENT;
            for selected in [false, true] {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(700., 500.),
                        )),
                        events: if selected {
                            vec![egui::Event::Copy]
                        } else {
                            vec![]
                        },
                        ..Default::default()
                    },
                    |ui| {
                        bg = ui.visuals().selection.bg_fill;
                        ui.memory_mut(|m| m.request_focus(id));
                        let mut layouter = |ui: &egui::Ui, value: &dyn egui::TextBuffer, _: f32| {
                            ui.fonts_mut(|f| {
                                f.layout_job(crate::widgets::highlight(value.as_str(), theme))
                            })
                        };
                        let mut edit = egui::TextEdit::multiline(&mut text)
                            .id(id)
                            .frame(egui::Frame::NONE)
                            .desired_width(600.)
                            .min_size(egui::vec2(600., 200.))
                            .layouter(&mut layouter)
                            .show(ui);
                        if selected {
                            selected_galley = Some(edit.galley);
                        } else {
                            baseline = Some(edit.galley);
                            edit.state.cursor.set_char_range(Some(range));
                            edit.state.store(&ctx, id);
                        }
                    },
                );
                if selected {
                    assert!(output.platform_output.commands.iter().any(|command| {
                        matches!(command, egui::OutputCommand::CopyText(value) if value == &original[4..end_byte])
                    }));
                }
                output.textures_delta.clear();
            }
            assert_eq!(text, original);
            let baseline = baseline.unwrap();
            let selected = selected_galley.unwrap();
            assert_eq!(baseline.rows.len(), selected.rows.len());
            for (before, after) in baseline.rows.iter().zip(&selected.rows) {
                let glyphs = before.visuals.glyph_vertex_range.clone();
                for index in glyphs {
                    assert_eq!(
                        before.visuals.mesh.vertices[index].color,
                        after.visuals.mesh.vertices[index].color
                    );
                }
            }
            for row in [1, 2] {
                let vertices = &selected.rows[row].visuals.mesh.vertices;
                let rect: Vec<_> = vertices.iter().filter(|v| v.color == bg).collect();
                assert_eq!(
                    rect.len(),
                    4,
                    "a background rectangle must include blank lines"
                );
                assert!(
                    rect.iter().any(|v| v.pos.x >= 590.),
                    "selected newline must fill the editor width: {rect:?}"
                );
            }
            let last = &selected.rows[3].visuals.mesh.vertices;
            assert!(
                last.iter()
                    .filter(|v| v.color == bg)
                    .all(|v| v.pos.x < 300.),
                "last partial line must stop at the selection endpoint"
            );
            assert!(
                selected.rows[4]
                    .visuals
                    .mesh
                    .vertices
                    .iter()
                    .all(|v| v.color != bg)
            );
        }
    }
    #[test]
    fn inter_matches_reference_label_widths() {
        let ctx = egui::Context::default();
        fonts(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |ctx| {
            for (label, expected) in [
                ("Params", 42.515625_f32),
                ("Body", 29.140625),
                ("Headers", 47.859375),
                ("Auth", 26.40625),
                ("Vars", 25.875),
                ("Script", 33.265625),
            ] {
                let width = ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            label.into(),
                            FontId::proportional(12.),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                });
                eprintln!("{label}: Rust={width:.4}, Bruno={expected:.4}");
                assert!(
                    (width - expected).abs() < 0.04,
                    "{label}: {width} versus {expected}"
                );
            }
        });
        output.textures_delta.clear();
    }
}
