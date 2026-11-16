use crate::{theme::Theme, widgets as w};
use eframe::egui::{self, FontId, TextFormat, Ui};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub fn render(ui: &mut Ui, source: &str, t: Theme) {
    ui.spacing_mut().item_spacing.y = 0.;
    let mut job = egui::text::LayoutJob::default();
    let mut strong = false;
    let mut italic = false;
    let mut strike = false;
    let mut heading = 0;
    let mut code = false;
    let mut list_depth: usize = 0;
    let mut links: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut url: Option<String> = None;
    let mut link_start = 0;
    fn flush(
        ui: &mut Ui,
        job: &mut egui::text::LayoutJob,
        links: &mut Vec<(std::ops::Range<usize>, String)>,
    ) {
        if !job.text.is_empty() {
            let mut layout = std::mem::take(job);
            layout.wrap.max_width = ui.available_width();
            let galley = ui.fonts_mut(|f| f.layout_job(layout.clone()));
            let response = ui.add(
                egui::Label::new(layout)
                    .selectable(true)
                    .wrap()
                    .sense(egui::Sense::click()),
            );
            if let Some(pointer) = response.hover_pos() {
                let index = galley.cursor_from_pos(pointer - response.rect.min).index.0;
                if let Some((_, link)) = links.iter().find(|(range, _)| range.contains(&index)) {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    response.clone().on_hover_text(link);
                    if response.clicked() {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(link));
                    }
                }
            }
        }
        links.clear();
    }
    for event in Parser::new_ext(
        source,
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS,
    ) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                flush(ui, &mut job, &mut links);
                heading = level as usize;
                ui.add_space(0.);
            }
            Event::End(TagEnd::Heading(_)) => {
                flush(ui, &mut job, &mut links);
                if heading <= 2 {
                    ui.separator();
                }
                heading = 0;
                ui.add_space(6.);
            }
            Event::Start(Tag::Strong) => strong = true,
            Event::End(TagEnd::Strong) => strong = false,
            Event::Start(Tag::Emphasis) => italic = true,
            Event::End(TagEnd::Emphasis) => italic = false,
            Event::Start(Tag::Strikethrough) => strike = true,
            Event::End(TagEnd::Strikethrough) => strike = false,
            Event::Start(Tag::CodeBlock(_)) => {
                flush(ui, &mut job, &mut links);
                code = true;
                ui.add_space(6.);
            }
            Event::End(TagEnd::CodeBlock) => {
                flush(ui, &mut job, &mut links);
                code = false;
                ui.add_space(6.);
            }
            Event::Start(Tag::List(_)) => {
                list_depth += 1;
            }
            Event::End(TagEnd::List(_)) => {
                flush(ui, &mut job, &mut links);
                list_depth -= 1;
                ui.add_space(4.);
            }
            Event::Start(Tag::Item) => {
                flush(ui, &mut job, &mut links);
                job.append(
                    &format!("{}•  ", "  ".repeat(list_depth.saturating_sub(1))),
                    0.,
                    TextFormat {
                        font_id: FontId::proportional(13.),
                        color: t.text,
                        ..Default::default()
                    },
                );
            }
            Event::End(TagEnd::Item) => {
                flush(ui, &mut job, &mut links);
                ui.add_space(7.);
            }
            Event::End(TagEnd::Paragraph) => {
                flush(ui, &mut job, &mut links);
                ui.add_space(if list_depth > 0 { 2. } else { 10. });
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_start = job.text.chars().count();
                url = Some(dest_url.to_string());
            }
            Event::End(TagEnd::Link) => {
                if let Some(link) = url.take() {
                    links.push((link_start..job.text.chars().count(), link));
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                if code {
                    let mut highlighted = w::highlight(&text, t);
                    job.text.push_str(&highlighted.text);
                    let offset = job.text.len() - highlighted.text.len();
                    for section in &mut highlighted.sections {
                        section.byte_range.start += offset;
                        section.byte_range.end += offset;
                    }
                    job.sections.extend(highlighted.sections);
                } else {
                    job.append(
                        &text,
                        0.,
                        TextFormat {
                            font_id: if strong || heading > 0 {
                                FontId::new(
                                    match heading {
                                        1 => 24.,
                                        2 => 20.,
                                        3 => 18.,
                                        4 => 16.,
                                        _ => 13.,
                                    },
                                    egui::FontFamily::Name("Semibold".into()),
                                )
                            } else {
                                FontId::proportional(13.)
                            },
                            color: if url.is_some() { t.blue } else { t.text },
                            italics: italic,
                            strikethrough: if strike {
                                egui::Stroke::new(1., t.text)
                            } else {
                                egui::Stroke::NONE
                            },
                            line_height: Some(if heading == 1 {
                                30.
                            } else if heading > 0 {
                                24.
                            } else {
                                20.
                            }),
                            ..Default::default()
                        },
                    );
                }
            }
            Event::Code(text) => job.append(
                &text,
                0.,
                TextFormat {
                    font_id: FontId::monospace(12.),
                    color: t.text,
                    background: t.surface,
                    ..Default::default()
                },
            ),
            Event::SoftBreak => job.append(
                " ",
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: t.text,
                    ..Default::default()
                },
            ),
            Event::HardBreak => job.append(
                "\n",
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: t.text,
                    ..Default::default()
                },
            ),
            Event::Rule => {
                flush(ui, &mut job, &mut links);
                ui.separator();
            }
            Event::TaskListMarker(checked) => job.append(
                if checked { "☑ " } else { "☐ " },
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: t.text,
                    ..Default::default()
                },
            ),
            Event::End(TagEnd::TableCell) => job.append(
                "     ",
                0.,
                TextFormat {
                    font_id: FontId::proportional(13.),
                    color: t.text,
                    ..Default::default()
                },
            ),
            Event::End(TagEnd::TableRow) => {
                flush(ui, &mut job, &mut links);
                ui.separator();
            }
            _ => {}
        }
    }
    flush(ui, &mut job, &mut links);
}
