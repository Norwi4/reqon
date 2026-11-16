use crate::{
    git::{State as Repository, View},
    git_diff::{Kind, Row},
    git_ui::State,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Ui, pos2, vec2};

pub fn show(ui: &mut Ui, r: Rect, repo: &Repository, form: &mut State, t: Theme) {
    if r.width() < 40. || r.height() < 40. {
        return;
    }
    let mut child = w::child(ui, r, "git-diff-panel");
    let ui = &mut child;
    if matches!(repo.view, View::Home) {
        w::label(
            ui,
            pos2(r.center().x - 130., r.center().y - 8.),
            "Select a file to view its changes",
            13.,
            t.muted,
        );
        return;
    }
    let review = &repo.review;
    let key = format!("{}:{}:{}", review.hash, review.left_title, review.path);
    if form.diff_key != key {
        form.diff_key = key;
        form.diff_scroll = 0.;
        form.diff_hunk = 0;
    }
    let bar = rect(r.left(), r.top(), r.width(), 32.);
    w::elided_medium(
        ui,
        rect(
            bar.left() + 12.,
            bar.top(),
            (bar.width() - 200.).max(20.),
            32.,
        ),
        &review.path,
        12.,
        t.text,
    );
    w::label(
        ui,
        pos2(bar.right() - 185., bar.center().y),
        &format!("+{}  −{}", review.additions, review.deletions),
        11.,
        t.muted,
    );
    let previous = w::icon_button(
        ui,
        rect(bar.right() - 109., bar.top() + 3., 24., 26.),
        Icon::Undo,
        "Previous difference",
        t,
    )
    .clicked();
    let next = w::icon_button(
        ui,
        rect(bar.right() - 83., bar.top() + 3., 24., 26.),
        Icon::Down,
        "Next difference",
        t,
    )
    .clicked();
    if w::icon_button(
        ui,
        rect(bar.right() - 57., bar.top() + 3., 24., 26.),
        Icon::Split,
        "Toggle unified / side-by-side diff",
        t,
    )
    .clicked()
    {
        form.unified = !form.unified;
    }
    if w::icon_button(
        ui,
        rect(bar.right() - 31., bar.top() + 3., 24., 26.),
        Icon::Copy,
        "Copy diff",
        t,
    )
    .clicked()
    {
        ui.ctx().copy_text(review.unified.clone());
    }
    w::line(ui, bar.left_bottom(), bar.right_bottom(), t.border);
    let size = ui
        .ctx()
        .data_mut(|d| d.get_temp::<f32>(egui::Id::unique("editor-font-size")))
        .unwrap_or(13.);
    let line_height = row_height(size);
    let hunks: Vec<_> = review
        .rows
        .iter()
        .enumerate()
        .filter(|(i, row)| {
            row.kind != Kind::Context
                && row.kind != Kind::Separator
                && (*i == 0 || matches!(review.rows[*i - 1].kind, Kind::Context | Kind::Separator))
        })
        .map(|(i, _)| i)
        .collect();
    if !hunks.is_empty() && (next || previous) {
        form.diff_hunk = if previous {
            (form.diff_hunk + hunks.len() - 1) % hunks.len()
        } else {
            (form.diff_hunk + 1) % hunks.len()
        };
        form.diff_scroll = (hunks[form.diff_hunk] as f32 * line_height - 35.).max(0.);
    }
    let title_h = 27.;
    let body = rect(
        r.left(),
        bar.bottom() + title_h,
        r.width(),
        (r.height() - 32. - title_h).max(0.),
    );
    if form.unified {
        w::label(
            ui,
            pos2(r.left() + 12., bar.bottom() + 13.),
            "Unified diff",
            11.,
            t.muted,
        );
        let mut view = w::child(ui, body, "git-unified");
        view.spacing_mut().item_spacing = vec2(0., 0.);
        let offsets = crate::widgets::line_offsets(&review.unified);
        egui::ScrollArea::both()
            .auto_shrink([false, false])
            .show_rows(&mut view, line_height, offsets.len(), |ui, rows| {
                for index in rows {
                    let text = review
                        .unified
                        .get(
                            offsets[index]
                                ..offsets
                                    .get(index + 1)
                                    .copied()
                                    .unwrap_or(review.unified.len()),
                        )
                        .unwrap_or("")
                        .trim_end_matches(['\r', '\n']);
                    let color = if text.starts_with('+') {
                        t.green
                    } else if text.starts_with('-') {
                        t.red
                    } else {
                        t.text
                    };
                    let mut job = egui::text::LayoutJob::simple_singleline(
                        text.into(),
                        egui::FontId::monospace(size),
                        color,
                    );
                    for section in &mut job.sections {
                        section.format.line_height = Some(line_height);
                    }
                    let galley = ui.painter().layout_job(job);
                    let (r, _) = ui.allocate_exact_size(
                        vec2(galley.size().x.max(ui.available_width()), line_height),
                        Sense::hover(),
                    );
                    text_row(ui, r, galley);
                }
            });
        return;
    }
    let mid = r.center().x;
    w::elided_label(
        ui,
        rect(r.left() + 10., bar.bottom(), r.width() * 0.5 - 20., title_h),
        &review.left_title,
        11.,
        t.muted,
    );
    w::elided_label(
        ui,
        rect(mid + 10., bar.bottom(), r.width() * 0.5 - 20., title_h),
        &review.right_title,
        11.,
        t.muted,
    );
    w::line(ui, pos2(mid, bar.bottom()), pos2(mid, r.bottom()), t.border);
    w::line(
        ui,
        pos2(r.left(), body.top()),
        pos2(r.right(), body.top()),
        t.border,
    );
    if review.rows.is_empty() {
        w::elided_label(ui, body.shrink(22.), &review.notice, 13., t.muted);
        return;
    }
    for right in [false, true] {
        let pane = rect(
            if right { mid + 1. } else { r.left() },
            body.top(),
            r.width() * 0.5 - 1.,
            body.height(),
        );
        let mut pane_ui = w::child(ui, pane, if right { "diff-after" } else { "diff-before" });
        pane_ui.spacing_mut().item_spacing = vec2(0., 0.);
        let output = egui::ScrollArea::both()
            .id_salt((&form.diff_key, right))
            .vertical_scroll_offset(form.diff_scroll)
            .auto_shrink([false, false])
            .show_rows(&mut pane_ui, line_height, review.rows.len(), |ui, range| {
                for index in range {
                    line(ui, &review.rows[index], right, size, line_height, t);
                }
            });
        if ui.rect_contains_pointer(pane) {
            form.diff_scroll = output.state.clamped_offset().y;
        }
    }
}

fn line(ui: &mut Ui, row: &Row, right: bool, size: f32, height: f32, t: Theme) {
    let text = if right { &row.right } else { &row.left };
    let number = if right {
        row.right_number
    } else {
        row.left_number
    };
    let text = &text[..text
        .char_indices()
        .nth(4096)
        .map(|(i, _)| i)
        .unwrap_or(text.len())];
    let width = (ui
        .painter()
        .layout_no_wrap(text.into(), egui::FontId::monospace(size), t.text)
        .size()
        .x
        + 55.)
        .max(ui.available_width());
    let (r, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
    let fill = match row.kind {
        Kind::Added if right => t.green.gamma_multiply(0.10),
        Kind::Deleted if !right => t.red.gamma_multiply(0.12),
        Kind::Modified => t.blue.gamma_multiply(0.12),
        Kind::Separator => t.hover,
        _ => egui::Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(r, 0, fill);
    if let Some(number) = number {
        w::mono(
            ui,
            pos2(r.left() + 6., r.center().y),
            &number.to_string(),
            size - 1.,
            t.faint,
        );
    }
    if (right && row.kind == Kind::Deleted) || (!right && row.kind == Kind::Added) {
        return;
    }
    let mut job = w::highlight(text, t);
    for section in &mut job.sections {
        section.format.font_id.size = size;
        section.format.line_height = Some(height);
    }
    if row.kind == Kind::Modified {
        let (a, b) = crate::git_diff::changed_ranges(&row.left, &row.right);
        let changed = if right { b } else { a };
        let mut sections = Vec::new();
        for section in std::mem::take(&mut job.sections) {
            let start = section
                .byte_range
                .start
                .max(egui::text::ByteIndex(changed.start));
            let end = section
                .byte_range
                .end
                .min(egui::text::ByteIndex(changed.end));
            if start < end {
                if section.byte_range.start < start {
                    let mut s = section.clone();
                    s.byte_range.end = start;
                    sections.push(s);
                }
                let mut middle = section.clone();
                middle.byte_range = start..end;
                middle.format.background = t.blue.gamma_multiply(0.24);
                sections.push(middle);
                if end < section.byte_range.end {
                    let mut s = section;
                    s.byte_range.start = end;
                    sections.push(s);
                }
            } else {
                sections.push(section);
            }
        }
        job.sections = sections;
    }
    let text_rect = rect(r.left() + 45., r.top(), r.width() - 45., height);
    let mut text_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(text_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    // A blank label has a zero-height min_rect. A layout scope would advance
    // the parent cursor backwards after we allocated the fixed-height row.
    // This independent child cannot change the next line's vertical position.
    text_ui.set_clip_rect(text_rect.intersect(ui.clip_rect()));
    job.wrap.max_width = f32::INFINITY;
    job.wrap.max_rows = 1;
    text_ui.add(
        egui::Label::new(job)
            .halign(egui::Align::Min)
            .selectable(true)
            .extend(),
    );
}

fn text_row(ui: &mut Ui, r: Rect, galley: std::sync::Arc<egui::Galley>) {
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(r)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(r.intersect(ui.clip_rect()));
    child.add(
        egui::Label::new(galley)
            .halign(egui::Align::Min)
            .selectable(true)
            .extend(),
    );
}

fn row_height(size: f32) -> f32 {
    // Match egui's 1/32-point layout grid; fractional font sizes must not
    // accumulate a difference between virtual rows and their painted positions.
    (size * 20. / 13. * 32.).ceil() / 32.
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn new_file_blank_lines_never_move_the_next_row_backwards() {
        let ctx = egui::Context::default();
        for size in [13., 18., 24.] {
            let height = row_height(size);
            let theme = Theme::new(true);
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect(0., 0., 700., 700.)),
                    ..Default::default()
                },
                |ui| {
                    ui.spacing_mut().item_spacing = vec2(0., 0.);
                    for (i, text) in [
                        "meta {",
                        "",
                        "",
                        "  name: test",
                        "}",
                        "   ",
                        "",
                        "body:json {",
                        "\t\tlong line",
                    ]
                    .iter()
                    .enumerate()
                    {
                        let before = ui.cursor().top();
                        line(
                            ui,
                            &Row {
                                right: (*text).into(),
                                right_number: Some(i + 1),
                                kind: Kind::Added,
                                ..Default::default()
                            },
                            true,
                            size,
                            height,
                            theme,
                        );
                        assert!(
                            (ui.cursor().top() - before - height).abs() < 0.01,
                            "line {i}, font {size}: reserved {}, expected {height}",
                            ui.cursor().top() - before
                        );
                    }
                },
            );
            output.textures_delta.clear();
        }
    }
}
