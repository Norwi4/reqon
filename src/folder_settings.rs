use crate::{
    app::App,
    model::Request,
    storage,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, TextEdit, Ui, pos2, vec2};

impl App {
    pub fn open_folder_settings(&mut self, cid: &str, folder: &str) {
        let id = format!("__folder__{cid}|{folder}");
        if !self.ws.collection_drafts.contains_key(&id) {
            let Some(collection) = self.ws.collections.iter().find(|c| c.id == cid) else {
                return;
            };
            let yaml = collection.path.join("opencollection.yml").is_file();
            let path =
                collection
                    .path
                    .join(folder)
                    .join(if yaml { "folder.yml" } else { "folder.req" });
            let original = match crate::inheritance::parent(&path) {
                Ok(r) => r,
                Err(e) => {
                    self.notice(format!("Cannot open folder settings: {e:#}"));
                    return;
                }
            };
            let mut request = original.unwrap_or_else(|| Request {
                auth: crate::model::Auth {
                    kind: "inherit".into(),
                    ..Default::default()
                },
                ..Default::default()
            });
            request.id = id.clone();
            request.protocol = "folder".into();
            request.collection = cid.into();
            request.folder = folder.into();
            request.name = folder.rsplit('/').next().unwrap_or(folder).into();
            request.path = Some(path);
            if yaml {
                request.file_format = "yml".into();
            }
            self.saved.insert(id.clone(), storage::to_bru(&request));
            self.ws.collection_drafts.insert(id.clone(), request);
        }
        self.open(id);
    }

    pub fn folder_workspace(&mut self, ui: &mut Ui, r: Rect, id: &str) {
        let Some(mut draft) = self.ws.collection_drafts.remove(id) else {
            return;
        };
        let t = self.theme;
        let mut selected = self.tabs.entry(id.into()).or_default().collection_tab;
        let names = [
            "Headers", "Script", "Test", "Vars", "Auth", "Docs", "Settings",
        ];
        let mapping = [2, 5, 7, 4, 3, 8, 10];
        let mut head = w::child(
            ui,
            rect(r.left() + 16., r.top() + 16., r.width() - 32., 35.),
            "folder-tabs",
        );
        egui::ScrollArea::horizontal()
            .auto_shrink([false, false])
            .show(&mut head, |ui| {
                ui.spacing_mut().item_spacing = vec2(19.2, 0.);
                ui.horizontal(|ui| {
                    for (index, name) in names.iter().enumerate() {
                        let count = match index {
                            0 => draft
                                .headers
                                .iter()
                                .filter(|p| p.enabled && !p.key.is_empty())
                                .count(),
                            3 => draft
                                .vars
                                .iter()
                                .chain(&draft.response_vars)
                                .filter(|p| p.enabled && !p.key.is_empty())
                                .count(),
                            _ => 0,
                        };
                        let dot = match index {
                            1 => !draft.pre_script.is_empty() || !draft.post_script.is_empty(),
                            2 => !draft.tests.is_empty(),
                            4 => !matches!(draft.auth.kind.as_str(), "" | "none" | "inherit"),
                            6 => !draft.tags.is_empty(),
                            _ => false,
                        };
                        let width = w::text_width(ui, name, 13.)
                            + if count > 0 {
                                w::text_width(ui, &count.to_string(), 10.) + 8.
                            } else if dot {
                                14.
                            } else {
                                0.
                            };
                        let row = ui.allocate_space(vec2(width, 35.)).1;
                        if ui
                            .interact(row, ui.scope_id().with(name), Sense::click())
                            .clicked()
                        {
                            selected = index;
                        }
                        w::label(
                            ui,
                            pos2(row.left(), row.top() + 16.),
                            name,
                            13.,
                            if selected == index { t.text } else { t.muted },
                        );
                        if selected == index {
                            ui.painter().rect_filled(
                                rect(row.left(), row.bottom() - 2., width, 2.),
                                0,
                                t.accent,
                            );
                        }
                        if count > 0 {
                            w::label(
                                ui,
                                pos2(
                                    row.right() - w::text_width(ui, &count.to_string(), 10.),
                                    row.top() + 10.,
                                ),
                                &count.to_string(),
                                10.,
                                t.muted,
                            );
                        } else if dot {
                            ui.painter().circle_filled(
                                pos2(row.right() - 3., row.top() + 10.),
                                2.,
                                t.muted,
                            );
                        }
                    }
                });
            });
        self.tabs.entry(id.into()).or_default().collection_tab = selected;
        let content = rect(
            r.left() + 16.,
            r.top() + 67.,
            r.width() - 32.,
            r.height() - 125.,
        );
        let mut changed = false;
        if selected == 0 {
            let mut body = w::child(ui, content, "folder-headers");
            body.spacing_mut().item_spacing.y = 0.;
            egui::ScrollArea::vertical().show(&mut body, |ui| {
                ui.label(
                    egui::RichText::new(
                        "Request headers that will be sent with every request inside this folder.",
                    )
                    .size(12.)
                    .color(t.muted),
                );
                ui.add_space(17.);
                changed |= w::folder_headers(ui, &mut draft.headers, t);
                ui.add_space(12.);
                let save = ui.allocate_space(vec2(54., 26.)).1;
                if w::button(ui, save, "Save", true, t).clicked() {
                    self.save_tree_draft(&draft);
                }
            });
        } else if selected == 6 {
            let mut body = w::child(ui, content, "folder-settings");
            w::label(
                &body,
                pos2(content.left(), content.top() + 8.),
                "Configure settings for this folder.",
                12.,
                t.muted,
            );
            w::icon(
                &body,
                pos2(content.left() + 8., content.top() + 40.),
                Icon::Tag,
                t.text,
                16.,
            );
            w::medium(
                &body,
                pos2(content.left() + 20., content.top() + 40.),
                "Tags",
                12.,
                t.text,
            );
            let mut tags = draft.tags.join(", ");
            if body
                .put(
                    rect(content.left(), content.top() + 64., content.width(), 32.),
                    TextEdit::singleline(&mut tags)
                        .hint_text("e.g., smoke, regression")
                        .margin(vec2(12., 8.)),
                )
                .changed()
            {
                draft.tags = tags
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect();
                changed = true;
            }
        } else {
            self.tabs.entry(id.into()).or_default().request_tab = mapping[selected.min(6)];
            let mut body = w::child(ui, content, "folder-editor-clip");
            changed |= self.request_editor(
                &mut body,
                rect(
                    content.left(),
                    content.top() - 48.,
                    content.width(),
                    content.height() + 48.,
                ),
                &mut draft,
            );
        }
        if selected != 0
            && w::button(
                ui,
                rect(content.left(), r.bottom() - 48., 64., 30.),
                "Save",
                true,
                t,
            )
            .clicked()
        {
            self.save_tree_draft(&draft);
        }
        self.ws.collection_drafts.insert(id.into(), draft);
        if changed {
            self.mark_edited();
        }
    }
}
