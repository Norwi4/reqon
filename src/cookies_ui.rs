use crate::{
    app::{App, Dialog},
    cookies::Entry,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, FontId, Sense, Stroke, StrokeKind, TextEdit, pos2, vec2};
#[derive(Clone)]
enum Delete {
    Cookie(Entry),
    Domain(String),
}
#[derive(Clone, Default)]
struct Form {
    entry: Entry,
    original: Option<Entry>,
    raw: bool,
    error: String,
    focused: bool,
}
impl App {
    pub fn cookies_dialog(&mut self, ctx: &egui::Context) {
        let t = self.theme;
        let editor_id = egui::Id::unique("cookie-form");
        let mut editor = ctx.data_mut(|d| d.get_temp::<Form>(editor_id));
        let delete_id = egui::Id::unique("cookie-delete");
        let mut delete = ctx.data_mut(|d| d.get_temp::<Delete>(delete_id));
        let entries = self.engine.cookies.entries();
        let empty = entries.is_empty();
        let query = self.entry.to_lowercase();
        let mut add_global = false;
        let (_, closed) = w::modal_with_header(
            ctx,
            "Cookies",
            if empty {
                1140.
            } else {
                ctx.content_rect().width() * 0.8
            },
            t,
            if empty { 40. } else { 48. },
            |ui, header| {
                if !empty {
                    ui.put(
                        rect(header.right() - 385., header.center().y - 16., 208., 32.),
                        TextEdit::singleline(&mut self.entry)
                            .hint_text("Search by domain")
                            .font(FontId::proportional(13.))
                            .margin(vec2(8., 6.)),
                    );
                    let add = rect(header.right() - 160., header.center().y - 14., 112., 28.);
                    add_global = w::button(ui, add, "    Add Cookie", true, t).clicked();
                    circle_plus(
                        ui,
                        pos2(add.left() + 19., add.center().y),
                        crate::theme::hex(0x191919),
                    );
                }
            },
            |ui| {
                if entries.is_empty() {
                    let r = ui.allocate_space(vec2(ui.available_width(), 179.)).1;
                    w::icon(
                        ui,
                        pos2(r.center().x, r.top() + 24.),
                        Icon::CookieOff,
                        t.muted,
                        48.,
                    );
                    ui.painter().text(
                        pos2(r.center().x, r.top() + 78.),
                        egui::Align2::CENTER_CENTER,
                        "No cookies found",
                        FontId::new(18., egui::FontFamily::Name("Medium".into())),
                        t.text,
                    );
                    ui.painter().text(
                        pos2(r.center().x, r.top() + 109.),
                        egui::Align2::CENTER_CENTER,
                        "Add cookies to get started",
                        FontId::proportional(13.),
                        t.muted,
                    );
                    if w::button(
                        ui,
                        rect(r.center().x - 56., r.top() + 152., 112., 28.),
                        "    Add Cookie",
                        true,
                        t,
                    )
                    .clicked()
                    {
                        editor = Some(Form {
                            entry: Entry {
                                path: "/".into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        });
                    }
                    circle_plus(
                        ui,
                        pos2(r.center().x - 38., r.top() + 166.),
                        crate::theme::hex(0x191919),
                    );
                } else {
                    let mut groups = std::collections::BTreeMap::<&str, Vec<&Entry>>::new();
                    for entry in entries
                        .iter()
                        .filter(|c| c.domain.to_lowercase().contains(&query))
                    {
                        groups.entry(&entry.domain).or_default().push(entry);
                    }
                    if groups.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(24.);
                            ui.label(egui::RichText::new("No search results").size(18.));
                            ui.add_space(8.);
                            ui.label("Try a different search term");
                            ui.add_space(24.);
                        });
                    }
                    egui::ScrollArea::vertical()
                        .max_height(500.)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.;
                            for (index, (domain, entries)) in groups.into_iter().enumerate() {
                                let id = ui.scope_id().with(("cookie-domain", domain));
                                let mut open = ctx
                                    .data_mut(|d| d.get_temp::<bool>(id))
                                    .unwrap_or(index == 0);
                                let height = 45.
                                    + if open {
                                        16. + 35.5 * (entries.len() + 1) as f32 + 18.
                                    } else {
                                        0.
                                    };
                                let group = ui.allocate_space(vec2(ui.available_width(), height)).1;
                                ui.painter().rect_stroke(
                                    group,
                                    3,
                                    Stroke::new(1., t.strong_border),
                                    StrokeKind::Inside,
                                );
                                let header = rect(group.left(), group.top(), group.width(), 45.);
                                ui.painter().rect_filled(
                                    header,
                                    egui::CornerRadius {
                                        nw: 3,
                                        ne: 3,
                                        sw: 0,
                                        se: 0,
                                    },
                                    if t.dark {
                                        t.strong_border
                                    } else {
                                        crate::theme::hex(0xf6f6f6)
                                    },
                                );
                                w::medium(
                                    ui,
                                    pos2(header.left() + 16., header.center().y),
                                    domain,
                                    13.,
                                    t.text,
                                );
                                w::label(
                                    ui,
                                    pos2(
                                        header.left() + 24. + w::text_width(ui, domain, 13.),
                                        header.center().y,
                                    ),
                                    &format!(
                                        "({} {})",
                                        entries.len(),
                                        if entries.len() == 1 {
                                            "cookie"
                                        } else {
                                            "cookies"
                                        }
                                    ),
                                    12.,
                                    t.muted,
                                );
                                if ui
                                    .interact(
                                        rect(
                                            header.left(),
                                            header.top(),
                                            header.width() - 96.,
                                            45.,
                                        ),
                                        id.with("toggle"),
                                        Sense::click(),
                                    )
                                    .clicked()
                                {
                                    open = !open;
                                }
                                let add = rect(header.right() - 89., header.top() + 10.5, 24., 24.);
                                circle_plus(ui, add.center(), t.muted);
                                if ui
                                    .interact(add, id.with("add"), Sense::click())
                                    .on_hover_text("Add cookie")
                                    .clicked()
                                {
                                    editor = Some(Form {
                                        entry: Entry {
                                            domain: domain.into(),
                                            path: "/".into(),
                                            ..Default::default()
                                        },
                                        ..Default::default()
                                    });
                                }
                                if w::icon_button(
                                    ui,
                                    rect(header.right() - 65., header.top() + 10.5, 24., 24.),
                                    Icon::Trash,
                                    "Clear domain cookies",
                                    t,
                                )
                                .clicked()
                                {
                                    delete = Some(Delete::Domain(domain.into()));
                                }
                                let fold =
                                    rect(header.right() - 39., header.top() + 10.5, 24., 24.);
                                if open {
                                    ui.painter().add(egui::Shape::line(
                                        vec![
                                            fold.center() + vec2(-5., 2.),
                                            fold.center() + vec2(0., -3.),
                                            fold.center() + vec2(5., 2.),
                                        ],
                                        Stroke::new(1.5, t.muted),
                                    ));
                                } else {
                                    w::icon(ui, fold.center(), Icon::Down, t.muted, 16.);
                                }
                                if ui.interact(fold, id.with("fold"), Sense::click()).clicked() {
                                    open = !open;
                                }
                                ctx.data_mut(|d| d.insert_temp(id, open));
                                if open {
                                    let inner = group.shrink2(vec2(16., 0.));
                                    let flex = (inner.width() - 432.) / 4.;
                                    let columns = [
                                        inner.left(),
                                        inner.left() + 128.,
                                        inner.left() + 336.,
                                        inner.left() + 336. + flex,
                                        inner.left() + 336. + 2. * flex,
                                        inner.left() + 336. + 3. * flex,
                                        inner.right() - 96.,
                                        inner.right(),
                                    ];
                                    let top = header.bottom() + 16.;
                                    for (i, title) in [
                                        "Name",
                                        "Value",
                                        "Path",
                                        "Expires",
                                        "Secure",
                                        "HTTP Only",
                                        "Actions",
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    {
                                        let x = if i == 4 || i == 5 {
                                            (columns[i] + columns[i + 1]
                                                - w::text_width(ui, title, 13.))
                                                / 2.
                                        } else if i == 6 {
                                            columns[7] - 16. - w::text_width(ui, title, 13.)
                                        } else {
                                            columns[i] + 16.
                                        };
                                        w::medium(ui, pos2(x, top + 17.75), title, 13., t.text);
                                    }
                                    w::line(
                                        ui,
                                        pos2(inner.left(), top + 35.5),
                                        pos2(inner.right(), top + 35.5),
                                        t.border,
                                    );
                                    for (row, entry) in entries.iter().enumerate() {
                                        let y = top + 35.5 * (row + 1) as f32;
                                        for (i, value) in [
                                            &entry.name,
                                            &entry.value,
                                            &entry.path,
                                            &if entry.expires.is_empty() {
                                                "Session".into()
                                            } else {
                                                entry.expires.clone()
                                            },
                                        ]
                                        .into_iter()
                                        .enumerate()
                                        {
                                            w::elided_label(
                                                ui,
                                                rect(
                                                    columns[i] + 16.,
                                                    y,
                                                    columns[i + 1] - columns[i] - 32.,
                                                    35.5,
                                                ),
                                                value,
                                                13.,
                                                t.text,
                                            );
                                        }
                                        for (i, checked) in
                                            [(4, entry.secure), (5, entry.http_only)]
                                        {
                                            if checked {
                                                w::icon(
                                                    ui,
                                                    pos2(
                                                        (columns[i] + columns[i + 1]) / 2.,
                                                        y + 17.75,
                                                    ),
                                                    Icon::Check,
                                                    t.text,
                                                    14.,
                                                );
                                            }
                                        }
                                        if w::icon_button(
                                            ui,
                                            rect(inner.right() - 60., y + 5.75, 24., 24.),
                                            Icon::Edit,
                                            "Edit cookie",
                                            t,
                                        )
                                        .clicked()
                                        {
                                            editor = Some(Form {
                                                entry: (*entry).clone(),
                                                original: Some((*entry).clone()),
                                                ..Default::default()
                                            });
                                        }
                                        if w::icon_button(
                                            ui,
                                            rect(inner.right() - 36., y + 5.75, 24., 24.),
                                            Icon::Trash,
                                            "Delete cookie",
                                            t,
                                        )
                                        .clicked()
                                        {
                                            delete = Some(Delete::Cookie((*entry).clone()));
                                        }
                                        if row + 1 < entries.len() {
                                            w::line(
                                                ui,
                                                pos2(inner.left(), y + 35.5),
                                                pos2(inner.right(), y + 35.5),
                                                t.border,
                                            );
                                        }
                                    }
                                }
                                ui.add_space(16.);
                            }
                        });
                }
            },
        );
        if add_global {
            editor = Some(Form {
                entry: Entry {
                    path: "/".into(),
                    ..Default::default()
                },
                ..Default::default()
            });
        }
        if closed && editor.is_none() && delete.is_none() {
            self.dialog = Dialog::None;
        }
        if let Some(form) = &mut editor {
            let mut saved = false;
            let mut cancel = false;
            let title = if form.original.is_some() {
                "Edit Cookie"
            } else {
                "Add Cookie"
            };
            let (_, closed) = w::modal(ctx, title, 800., t, |ui| {
                let r = ui.allocate_space(vec2(ui.available_width(), 270.)).1;
                let left = r.left() + 8.;
                let width = r.width() - 16.;
                let half = (width - 16.) / 2.;
                let right = left + half + 16.;
                let toggle = rect(r.right() - 129., r.top() - 44., 100., 20.);
                let clip = ui.clip_rect();
                ui.set_clip_rect(clip.union(toggle));
                if ui
                    .interact(
                        toggle,
                        ui.scope_id().with("edit-raw-cookie"),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    form.raw = !form.raw;
                }
                let pill = rect(toggle.left(), toggle.top() + 2., 32., 16.);
                ui.painter().rect_filled(
                    pill,
                    8,
                    if form.raw {
                        t.accent
                    } else {
                        crate::theme::hex(0x999999)
                    },
                );
                ui.painter().circle_filled(
                    pos2(
                        pill.left() + if form.raw { 24. } else { 8. },
                        pill.center().y,
                    ),
                    6.,
                    egui::Color32::WHITE,
                );
                w::label(
                    ui,
                    pos2(toggle.left() + 40., toggle.center().y),
                    "Edit Raw",
                    13.,
                    t.text,
                );
                ui.set_clip_rect(clip);
                let field = |ui: &mut egui::Ui,
                             x: f32,
                             y: f32,
                             width: f32,
                             label: &str,
                             value: &mut String| {
                    let label_text = label.trim_end_matches('*');
                    w::label(ui, pos2(x, y + 9.), label_text, 13., t.text);
                    if label.ends_with('*') {
                        w::label(
                            ui,
                            pos2(x + w::text_width(ui, label_text, 13.), y + 9.),
                            "*",
                            13.,
                            t.red,
                        );
                    }
                    ui.put(
                        rect(x, y + 24., width, 34.),
                        TextEdit::singleline(value)
                            .font(FontId::proportional(13.))
                            .margin(vec2(8., 8.)),
                    )
                };
                if form.raw {
                    let domain = field(ui, left, r.top(), width, "Domain", &mut form.entry.domain);
                    if !form.focused {
                        domain.request_focus();
                        form.focused = true;
                    }
                    w::label(ui, pos2(left, r.top() + 85.), "Set-Cookie", 13., t.text);
                    ui.put(
                        rect(left, r.top() + 99., width, 110.),
                        TextEdit::multiline(&mut form.entry.raw).font(FontId::monospace(13.)),
                    );
                } else {
                    let domain = field(ui, left, r.top(), half, "Domain*", &mut form.entry.domain);
                    if !form.focused {
                        domain.request_focus();
                        form.focused = true;
                    }
                    field(ui, right, r.top(), half, "Path", &mut form.entry.path);
                    field(ui, left, r.top() + 74., half, "Key*", &mut form.entry.name);
                    field(
                        ui,
                        right,
                        r.top() + 74.,
                        half,
                        "Value*",
                        &mut form.entry.value,
                    );
                    field(
                        ui,
                        left,
                        r.top() + 148.,
                        220.,
                        "Expiration (UTC)",
                        &mut form.entry.expires,
                    );
                    w::checkbox_at(
                        ui,
                        rect(r.right() - 170., r.top() + 190., 75., 20.),
                        &mut form.entry.secure,
                        "Secure",
                        t,
                    );
                    w::checkbox_at(
                        ui,
                        rect(r.right() - 90., r.top() + 190., 90., 20.),
                        &mut form.entry.http_only,
                        "HTTP Only",
                        t,
                    );
                }
                if !form.error.is_empty() {
                    w::label(ui, pos2(left, r.top() + 224.), &form.error, 12., t.red);
                }
                if w::button(
                    ui,
                    rect(r.right() - 62., r.top() + 241., 62., 30.),
                    "Save",
                    true,
                    t,
                )
                .clicked()
                {
                    let result = (|| -> anyhow::Result<()> {
                        anyhow::ensure!(!form.entry.domain.trim().is_empty(), "Enter a domain");
                        let domain = form.entry.domain.trim().trim_start_matches('.');
                        let url = url::Url::parse(&format!(
                            "{}://{domain}{}",
                            if form.entry.secure { "https" } else { "http" },
                            if form.entry.path.starts_with('/') {
                                &form.entry.path
                            } else {
                                "/"
                            }
                        ))?;
                        let source = if form.raw {
                            form.entry.raw.clone()
                        } else {
                            anyhow::ensure!(!form.entry.name.is_empty(), "Enter a cookie key");
                            let mut s = format!(
                                "{}={}; Domain={}; Path={}",
                                form.entry.name,
                                form.entry.value,
                                domain,
                                if form.entry.path.is_empty() {
                                    "/"
                                } else {
                                    &form.entry.path
                                }
                            );
                            if form.entry.secure {
                                s.push_str("; Secure");
                            }
                            if form.entry.http_only {
                                s.push_str("; HttpOnly");
                            }
                            if !form.entry.expires.trim().is_empty() {
                                let date = time::PrimitiveDateTime::parse(
                                    form.entry.expires.trim(),
                                    time::macros::format_description!(
                                        "[year]-[month]-[day] [hour]:[minute]"
                                    ),
                                )
                                .map_err(|_| {
                                    anyhow::anyhow!("Expiration format: YYYY-MM-DD HH:MM (UTC)")
                                })?;
                                s.push_str("; Expires=");
                                s.push_str(&date.format(time::macros::format_description!("[weekday repr:short], [day] [month repr:short] [year] [hour]:[minute]:[second] GMT"))?);
                            }
                            s
                        };
                        self.engine
                            .cookies
                            .replace(&url, &source, form.original.as_ref())?;
                        Ok(())
                    })();
                    match result {
                        Ok(()) => saved = true,
                        Err(e) => form.error = format!("{e:#}"),
                    }
                }
                let cancel_rect = rect(r.right() - 139., r.top() + 241., 67., 30.);
                w::label(
                    ui,
                    pos2(cancel_rect.left() + 12., cancel_rect.center().y),
                    "Cancel",
                    13.,
                    t.text,
                );
                if ui
                    .interact(
                        cancel_rect,
                        ui.scope_id().with("cancel-cookie"),
                        egui::Sense::click(),
                    )
                    .clicked()
                {
                    cancel = true;
                }
            });
            if saved || cancel || closed {
                editor = None;
            }
        }
        if let Some(target) = delete.clone() {
            let (title, question, action) = match &target {
                Delete::Cookie(entry) => (
                    "Delete Cookie",
                    format!("Are you sure you want to delete the cookie {}?", entry.name),
                    "Delete",
                ),
                Delete::Domain(domain) => (
                    "Clear Domain Cookies",
                    format!("Are you sure you want to clear all cookies for the domain {domain}?"),
                    "Clear All",
                ),
            };
            let mut confirm = false;
            let mut cancel = false;
            let (_, closed) = w::modal(ctx, title, 500., t, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Hold on..").size(18.));
                });
                ui.add_space(16.);
                ui.add(egui::Label::new(question).wrap());
                ui.add_space(24.);
                let row = ui.allocate_space(vec2(ui.available_width(), 30.)).1;
                cancel = w::button(ui, rect(row.left(), row.top(), 64., 30.), "Close", false, t)
                    .clicked();
                confirm = w::button(
                    ui,
                    rect(row.right() - 82., row.top(), 82., 30.),
                    action,
                    true,
                    crate::theme::Theme { accent: t.red, ..t },
                )
                .clicked();
            });
            if confirm {
                match target {
                    Delete::Cookie(entry) => self.engine.cookies.remove(&entry),
                    Delete::Domain(domain) => {
                        for entry in &entries {
                            if entry.domain == domain {
                                self.engine.cookies.remove(entry);
                            }
                        }
                    }
                }
            }
            if confirm || cancel || closed {
                delete = None;
            }
        }
        ctx.data_mut(|d| {
            if let Some(delete) = delete {
                d.insert_temp(delete_id, delete);
            } else {
                d.remove::<Delete>(delete_id);
            }
            if let Some(editor) = editor {
                d.insert_temp(editor_id, editor);
            } else {
                d.remove::<Form>(editor_id);
            }
        });
    }
}
fn circle_plus(ui: &egui::Ui, center: egui::Pos2, color: egui::Color32) {
    ui.painter()
        .circle_stroke(center, 5.5, Stroke::new(1., color));
    w::icon(ui, center, Icon::Plus, color, 9.);
}
