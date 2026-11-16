use crate::{
    app::{Action, App, Dialog, Event, LoadedCollection},
    menu::{self, Item},
    model::id,
    tree::{Kind, Row},
    widgets::{self as w, rect},
};
use eframe::egui::{self, Rect, Response, Sense, Ui, vec2};
use std::collections::HashSet;

#[derive(Default)]
pub struct State {
    pub rename: Option<(String, String, bool)>,
    pub transferring: bool,
    selected: HashSet<String>,
    preview: Vec<Row>,
    collapsed: HashSet<String>,
    keyboard_row: Option<usize>,
    active_menu: Option<(egui::Id, String)>,
}

#[derive(Clone, Copy)]
enum Command {
    Create,
    Open,
    Import,
    Export,
    Curl,
    Expand,
    Collapse,
    NewRequest,
    NewFolder,
    Run,
    Reveal,
    Settings,
    Environments,
    Terminal,
    MoveWorkspace,
    Rename,
    Remove,
}

fn global_items() -> Vec<Item<Command>> {
    use Command::*;
    vec![
        Item::new("Create Collection", "Box", Some(Create)),
        Item::new("Open Collection", "Folder", Some(Open)),
        Item::separator(),
        Item::new("Import Collections...", "Import", Some(Import)),
        Item::new("Export Collections...", "Export", Some(Export)),
        Item::new("Import cURL", "Code", Some(Curl)),
        Item::separator(),
        Item::new("Expand All", "Expand", Some(Expand)),
        Item::new("Collapse All", "Collapse", Some(Collapse)),
    ]
}

fn collection_items() -> Vec<Item<Command>> {
    use Command::*;
    // Preserve Bruno's grouping and names, with direct export next to Share.
    let mut entries = vec![
        Item::new("New Request", "FilePlus", Some(NewRequest)),
        Item::new("New Folder", "FolderPlus", Some(NewFolder)),
        Item::new("New App", "AppWindow", None),
        Item::new("New JS File", "FileCode", None),
        Item::new("Run", "Run", Some(Run)),
        Item::new("Clone", "Copy", None),
        Item::new("OpenAPI", "OpenAPI", None),
        Item::new("Rename", "Edit", Some(Rename)),
        Item::new("Share", "Share", None),
        Item::new("Export Collection...", "Export", Some(Export)),
        Item::new("Generate Docs", "Book", None),
        Item::new("Collapse", "Collapse", Some(Collapse)),
        Item::new("Reveal in File Explorer", "Folder", Some(Reveal)),
        Item::new("Create Mock server", "Server", None),
        Item::separator(),
        Item::new("Environments", "Server", Some(Environments)),
        Item::new("Settings", "Settings", Some(Settings)),
        Item::new("Open in Terminal", "Terminal", Some(Terminal)),
        Item::new("Move into Workspace", "Move", Some(MoveWorkspace)),
        Item::new("Remove", "Close", Some(Remove)),
    ];
    for entry in &mut entries {
        entry.shortcut = match entry.label {
            "New Request" => "Ctrl+N",
            "Clone" => "Ctrl+D",
            "Rename" => "Ctrl+R",
            "Open in Terminal" => "Ctrl+T",
            _ => "",
        };
    }
    entries
}

impl App {
    pub fn collections_menu(&mut self, ui: &mut Ui, response: &Response) {
        let popup_id = egui::Popup::default_response_id(response);
        if !response.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            return;
        }
        if response.clicked() {
            self.collection_ui.keyboard_row = None;
        }
        let entries = global_items();
        let width = menu::width(ui, &entries);
        let popup = egui::Popup::menu(response)
            .gap(10.)
            .frame(menu::frame(self.theme))
            .show(|ui| {
                menu::rows(
                    ui,
                    &entries,
                    width,
                    self.theme,
                    &mut self.collection_ui.keyboard_row,
                )
            });
        if let Some(command) = popup.and_then(|p| p.inner) {
            self.collection_command(command, None);
        }
    }

    pub fn collection_context_menu(
        &mut self,
        ui: &mut Ui,
        response: &Response,
        bounds: Rect,
        row: &Row,
    ) {
        let popup_id = egui::Id::unique(("collection-menu", &row.key));
        let opened = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let button_rect = rect(bounds.right() - 31., bounds.center().y - 12., 24., 24.);
        let dots = ui.interact(button_rect, response.id.with("dots"), Sense::click());
        dots.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} menu", row.name))
        });
        if response.container_contains_pointer() || opened || dots.hovered() {
            ui.painter().rect_filled(button_rect, 0, self.theme.hover);
            menu::icon(ui, "Dots", button_rect.center(), self.theme.text, 18.);
        }
        let open = if response.secondary_clicked() {
            Some(egui::SetOpenCommand::Bool(true))
        } else if dots.clicked() {
            Some(egui::SetOpenCommand::Toggle)
        } else {
            None
        };
        if open.is_some() {
            crate::tree_drag::cancel_pending(ui.ctx());
            self.collection_ui.keyboard_row = None;
            self.collection_ui.active_menu = Some((popup_id, row.collection.clone()));
        }
        if !opened && open.is_none() {
            return;
        }
        let entries = collection_items();
        let width = menu::width(ui, &entries);
        let popup = egui::Popup::new(popup_id, ui.ctx().clone(), button_rect, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .open_memory(open)
            .gap(10.)
            .frame(menu::frame(self.theme))
            .show(|ui| {
                menu::rows(
                    ui,
                    &entries,
                    width,
                    self.theme,
                    &mut self.collection_ui.keyboard_row,
                )
            });
        if let Some(command) = popup.and_then(|p| p.inner) {
            self.collection_command(command, Some(&row.collection));
        }
    }

    fn collection_command(&mut self, command: Command, cid: Option<&str>) {
        use Command::*;
        match command {
            Create => self.actions.push(Action::NewCollection),
            Rename => {
                if let Some(cid) = cid {
                    self.begin_collection_rename(cid);
                }
            }
            Remove => {
                if let Some(cid) = cid {
                    self.show_dialog(Dialog::RemoveCollection(cid.into()));
                }
            }
            Open => self.actions.push(Action::OpenCollection),
            Import => self.actions.push(Action::ImportPostman),
            Export => {
                if let Some(cid) = cid {
                    self.actions.push(Action::ExportPostman(Some(cid.into())));
                } else {
                    self.show_export_collections();
                }
            }
            Curl => self.show_dialog(Dialog::ImportCurl),
            Expand => self.collapsed.clear(),
            Collapse => {
                let rows = crate::tree::build(&self.ws, "", &HashSet::new());
                self.collapsed.extend(
                    rows.into_iter()
                        .filter(|r| {
                            r.kind != Kind::Request && cid.is_none_or(|id| id == r.collection)
                        })
                        .map(|r| r.key),
                );
            }
            NewRequest => {
                if let Some(cid) = cid {
                    self.actions
                        .push(Action::NewRequest(cid.into(), String::new()));
                }
            }
            NewFolder => {
                if let Some(cid) = cid {
                    self.actions.push(Action::NewFolder(cid.into()));
                }
            }
            Run => {
                if let Some(cid) = cid {
                    self.open_collection_settings(cid);
                    self.show_dialog(Dialog::Runner);
                }
            }
            Settings => {
                if let Some(cid) = cid {
                    self.open_collection_settings(cid);
                }
            }
            Environments => {
                if let Some(cid) = cid {
                    self.env_index = 0;
                    self.open_environment_manager(&cid);
                }
            }
            MoveWorkspace => {
                if let Some(cid) = cid {
                    self.workspaces.dialog = Some(crate::workspaces::Dialog::Move(cid.into()));
                }
            }
            Reveal | Terminal => {
                if let Some(c) = self
                    .ws
                    .collections
                    .iter()
                    .find(|c| Some(c.id.as_str()) == cid)
                {
                    let path = c.path.clone();
                    if matches!(command, Terminal) {
                        self.open_terminal(path);
                    } else if let Err(error) = crate::tree_menu::reveal(&path) {
                        self.notice(format!("Cannot open File Explorer: {error}"));
                    }
                }
            }
        }
    }

    pub fn collection_menu_shortcuts(&mut self, ctx: &egui::Context) {
        let Some((popup, cid)) = self.collection_ui.active_menu.clone() else {
            return;
        };
        if !egui::Popup::is_id_open(ctx, popup) {
            return;
        }
        let command = {
            [
                ("newRequest", Some(Command::NewRequest)),
                ("openTerminal", Some(Command::Terminal)),
                ("rename", Some(Command::Rename)),
            ]
            .into_iter()
            .find_map(|(key, command)| {
                crate::keybindings::pressed(ctx, &self.ws.preferences.keybindings, key)
                    .then_some(command)
            })
        };
        if let Some(Some(command)) = command {
            egui::Popup::close_all(ctx);
            self.collection_command(command, Some(&cid));
        }
    }

    fn show_export_collections(&mut self) {
        self.collection_ui.selected = self.ws.collections.iter().map(|c| c.id.clone()).collect();
        self.collection_ui.collapsed = self.collection_ui.selected.clone();
        self.collection_ui.preview = crate::tree::build(&self.ws, "", &HashSet::new());
        self.show_dialog(Dialog::ExportCollections);
    }

    pub fn export_collections_dialog(&mut self, ctx: &egui::Context) {
        let t = self.theme;
        let mut export = false;
        let mut cancel = false;
        let (_, closed) = w::modal(ctx, "Export Collections", 640., t, |ui| {
            ui.label(egui::RichText::new("Select collections to include in one file. All folders, requests and examples in each selected collection will be exported.").size(12.).color(t.muted));
            ui.add_space(16.);
            ui.horizontal(|ui| {
                if ui.button("Select all").clicked() {
                    self.collection_ui.selected =
                        self.ws.collections.iter().map(|c| c.id.clone()).collect();
                }
                if ui.button("Clear selection").clicked() {
                    self.collection_ui.selected.clear();
                }
            });
            ui.add_space(12.);
            let height = (ctx.content_rect().height() - 350.).clamp(120., 340.);
            egui::Frame::NONE
                .stroke(egui::Stroke::new(1., t.border))
                .inner_margin(8.)
                .corner_radius(4)
                .show(ui, |ui| {
                    ui.set_min_height(height);
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let mut hidden_depth = None;
                            for row in &self.collection_ui.preview {
                                let depth = if row.kind == Kind::Collection {
                                    0
                                } else {
                                    row.depth + 1
                                };
                                if hidden_depth.is_some_and(|d| depth > d) {
                                    continue;
                                }
                                hidden_depth = None;
                                let is_collection = row.kind == Kind::Collection;
                                let expanded = !self.collection_ui.collapsed.contains(&row.key);
                                ui.horizontal(|ui| {
                                    ui.add_space(depth as f32 * 18.);
                                    if row.kind != Kind::Request {
                                        let (r, response) =
                                            ui.allocate_exact_size(vec2(16., 24.), Sense::click());
                                        w::icon(
                                            ui,
                                            r.center(),
                                            if expanded {
                                                w::Icon::Down
                                            } else {
                                                w::Icon::Chevron
                                            },
                                            t.muted,
                                            12.,
                                        );
                                        if response.clicked() {
                                            if expanded {
                                                self.collection_ui
                                                    .collapsed
                                                    .insert(row.key.clone());
                                            } else {
                                                self.collection_ui.collapsed.remove(&row.key);
                                            }
                                        }
                                    } else {
                                        ui.add_space(16.);
                                    }
                                    if is_collection {
                                        let mut selected =
                                            self.collection_ui.selected.contains(&row.key);
                                        if ui.checkbox(&mut selected, "").changed() {
                                            if selected {
                                                self.collection_ui.selected.insert(row.key.clone());
                                            } else {
                                                self.collection_ui.selected.remove(&row.key);
                                            }
                                        }
                                    }
                                    let r = ui.allocate_space(vec2(16., 24.)).1;
                                    menu::icon(
                                        ui,
                                        if is_collection {
                                            "Box"
                                        } else if row.kind == Kind::Folder {
                                            "Folder"
                                        } else {
                                            "FileCode"
                                        },
                                        r.center(),
                                        t.muted,
                                        16.,
                                    );
                                    ui.label(egui::RichText::new(&row.name).size(12.));
                                });
                                if row.kind != Kind::Request
                                    && !self.collection_ui.collapsed.contains(&row.key)
                                {
                                    continue;
                                }
                                if row.kind != Kind::Request {
                                    hidden_depth = Some(depth);
                                }
                            }
                            if self.ws.collections.is_empty() {
                                ui.label("No collections to export");
                            }
                        });
                });
            ui.add_space(12.);
            ui.label(
                egui::RichText::new(format!(
                    "{} of {} collections selected",
                    self.collection_ui.selected.len(),
                    self.ws.collections.len()
                ))
                .size(12.)
                .color(t.muted),
            );
            ui.add_space(14.);
            ui.horizontal(|ui| {
                let enabled =
                    !self.collection_ui.selected.is_empty() && !self.collection_ui.transferring;
                export = ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(
                            egui::RichText::new("Export selected")
                                .color(crate::theme::hex(0x1a1a1a)),
                        )
                        .fill(t.accent),
                    )
                    .clicked();
                cancel = ui.button("Cancel").clicked();
            });
        });
        if export {
            let selected = self
                .ws
                .collections
                .iter()
                .filter(|c| self.collection_ui.selected.contains(&c.id))
                .map(|c| c.id.clone())
                .collect();
            self.actions.push(Action::ExportCollections(selected));
            self.dialog = Dialog::None;
        } else if closed || cancel {
            self.dialog = Dialog::None;
        }
    }

    pub fn import_collections(&mut self, ctx: &egui::Context) {
        if self.collection_ui.transferring {
            return;
        }
        self.collection_ui.transferring = true;
        self.notice("Choose collections to import");
        let root = self
            .root
            .join("collections")
            .join(format!("Imported-{}", id()));
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| -> anyhow::Result<Option<String>> {
                let Some(paths) = rfd::FileDialog::new()
                    .set_title("Import Collections")
                    .add_filter("Collections", &["json"])
                    .pick_files()
                else {
                    return Ok(None);
                };
                let collections = crate::interchange::import_files(&paths, &root)?;
                let count = collections.len();
                for (c, requests) in collections {
                    let _ = tx.send(Event::Collection(Ok(LoadedCollection::new((
                        c,
                        requests,
                        vec![],
                    )))));
                }
                Ok(Some(format!("Imported {count} collection(s)")))
            })()
            .map_err(|error| format!("Import failed: {error:#}"));
            let _ = tx.send(Event::CollectionTransferFinished(result));
            ctx.request_repaint();
        });
    }

    pub fn export_collections(&mut self, selected: Vec<String>, bundle: bool, ctx: &egui::Context) {
        if self.collection_ui.transferring {
            return;
        }
        let collections: Vec<_> = self
            .ws
            .collections
            .iter()
            .filter(|c| selected.contains(&c.id))
            .cloned()
            .collect();
        if collections.is_empty() {
            self.notice("Select at least one collection");
            return;
        }
        let requests = self
            .ws
            .requests
            .iter()
            .filter(|r| selected.contains(&r.collection))
            .cloned()
            .collect::<Vec<_>>();
        self.collection_ui.transferring = true;
        self.notice("Choose where to export collections");
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        self.runtime.spawn_blocking(move || {
            let result = (|| -> anyhow::Result<Option<String>> {
                let name = if bundle {
                    "Reqon.reqon_collections.json".into()
                } else {
                    format!(
                        "{}.collection.json",
                        crate::model::safe_name(&collections[0].name)
                    )
                };
                let Some(path) = rfd::FileDialog::new()
                    .set_title("Export Collections")
                    .set_file_name(name)
                    .add_filter("Collections JSON", &["json"])
                    .save_file()
                else {
                    return Ok(None);
                };
                let document = if bundle {
                    crate::interchange::export_documents(&collections, &requests, &selected)?
                } else {
                    crate::interchange::export_document(&collections[0], &requests)?
                };
                crate::storage::atomic_write(&path, serde_json::to_vec_pretty(&document)?)?;
                Ok(Some(format!(
                    "Exported {} collection(s)",
                    collections.len()
                )))
            })()
            .map_err(|error| format!("Export failed: {error:#}"));
            let _ = tx.send(Event::CollectionTransferFinished(result));
            ctx.request_repaint();
        });
    }
}
