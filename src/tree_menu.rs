use crate::{
    app::{Action, App, Dialog},
    model::Request,
    tree::{Kind, Row},
    tree_ops::{self, Operation, Subject},
    widgets::{self as w, rect},
};
use eframe::egui::{self, Rect, Response, Sense, Ui, vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    NewRequest,
    NewFolder,
    Clone,
    Copy,
    Rename,
    Reveal,
    Info,
    Settings,
    Terminal,
    Delete,
}
#[derive(Default)]
pub struct State {
    pub selected: Option<Subject>,
    pub copied: Option<Clipboard>,
    pub operating: bool,
    pub active: Option<(egui::Id, Subject)>,
    pub keyboard_row: Option<usize>,
    pub deferred_saves: std::collections::HashSet<String>,
}
#[derive(Clone, Debug)]
pub struct Clipboard {
    pub subject: Subject,
    pub requests: Vec<Request>,
    pub folders: Vec<String>,
    pub settings: Vec<Request>,
}
impl Clipboard {
    fn has_data(&self) -> bool {
        match &self.subject {
            Subject::Folder { folder, .. } => {
                !self.folders.is_empty()
                    || !self.settings.is_empty()
                    || !self.requests.is_empty()
                    || !folder.is_empty()
            }
            Subject::Request(_) => !self.requests.is_empty(),
        }
    }
}
type Item = crate::menu::Item<Command>;
fn item(
    label: &'static str,
    icon: &'static str,
    shortcut: &'static str,
    command: Option<Command>,
) -> Item {
    Item {
        label,
        icon,
        shortcut,
        danger: command == Some(Command::Delete),
        command,
    }
}
fn items(folder: bool, protocol: &str, copied: bool) -> Vec<Item> {
    use Command::*;
    let mut list = vec![];
    if folder {
        list.extend([
            item("New Request", "FilePlus", "Ctrl+N", Some(NewRequest)),
            item("New Folder", "FolderPlus", "", Some(NewFolder)),
            item("New App", "AppWindow", "", None),
            item("New JS File", "FileCode", "", None),
            item("Run", "Run", "", None),
        ]);
    }
    list.extend([
        item("Clone", "Copy", "Ctrl+D", Some(Clone)),
        item("Copy", "Copy", "Ctrl+C", Some(Copy)),
    ]);
    if folder && copied {
        list.push(item("Paste", "Clipboard", "Ctrl+V", None));
    }
    list.push(item("Rename", "Edit", "Ctrl+R", Some(Rename)));
    if !folder {
        if matches!(protocol, "grpc" | "ws") {
            list.push(item("Run", "Run", "", None));
        } else {
            list.push(item("Generate Code", "Code", "", None));
            if protocol != "graphql" {
                list.push(item("Create Example", "Example", "", None));
            }
        }
    }
    list.push(item(
        "Reveal in File Explorer",
        "Folder",
        "",
        folder.then_some(Reveal),
    ));
    if folder {
        list.push(item("Ignore", "EyeOff", "", None));
    }
    list.push(item("", "", "", None));
    list.push(item("Info", "Info", "", folder.then_some(Info)));
    if folder {
        list.extend([
            item("Settings", "Settings", "", Some(Settings)),
            item("Open in Terminal", "Terminal", "Ctrl+T", Some(Terminal)),
        ]);
    }
    list.push(item("Delete", "Trash", "", Some(Delete)));
    list
}
pub fn subject(row: &Row) -> Subject {
    if row.kind == Kind::Folder {
        Subject::Folder {
            collection: row.collection.clone(),
            folder: row.folder.clone(),
        }
    } else {
        Subject::Request(row.key.clone())
    }
}
impl App {
    pub fn save_tree_draft(&mut self, draft: &Request) {
        if self.tree_moving {
            self.tree_menu.deferred_saves.insert(draft.id.clone());
        } else {
            self.writer.save_request(draft.clone());
        }
    }
    pub fn finish_deferred_tree_saves(&mut self) {
        for id in std::mem::take(&mut self.tree_menu.deferred_saves) {
            if let Some(draft) = self
                .ws
                .collection_drafts
                .get(&id)
                .or_else(|| self.ws.requests.iter().find(|r| r.id == id))
            {
                self.writer.save_request(draft.clone());
            }
        }
    }
    /// Both mouse entry points use the dots button's bottom-left anchor, as Bruno does.
    pub fn tree_context_menu(&mut self, ui: &mut Ui, response: &Response, bounds: Rect, row: &Row) {
        let target = subject(row);
        if response.secondary_clicked() {
            self.tree_menu.selected = Some(target.clone());
        }
        let popup_id = egui::Id::unique(("tree-item-menu", &row.key));
        let opened = egui::Popup::is_id_open(ui.ctx(), popup_id);
        let hover = response.container_contains_pointer();
        let button_rect = rect(bounds.right() - 31., bounds.center().y - 12., 24., 24.);
        let dots = ui.interact(button_rect, response.id.with("dots"), Sense::click());
        dots.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} menu", row.name))
        });
        if hover || opened || dots.hovered() {
            ui.painter().rect_filled(
                button_rect,
                0,
                if self.ws.active.as_deref() == Some(&row.key) {
                    self.theme.surface
                } else {
                    self.theme.hover
                },
            );
            crate::menu::icon(ui, "Dots", button_rect.center(), self.theme.text, 18.);
        }
        if dots.is_pointer_button_down_on() {
            crate::tree_drag::cancel_pending(ui.ctx());
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
            self.tree_menu.active = Some((popup_id, target.clone()));
            self.tree_menu.keyboard_row = None;
        }
        // Most rows have no open menu: avoid constructing labels and measuring
        // text for every visible row on every frame of a large collection.
        if !opened && open.is_none() {
            return;
        }
        let protocol = self
            .ws
            .requests
            .iter()
            .find(|r| r.id == row.key)
            .map(|r| r.protocol.as_str())
            .unwrap_or("http");
        let entries = items(
            row.kind == Kind::Folder,
            protocol,
            self.tree_menu
                .copied
                .as_ref()
                .is_some_and(Clipboard::has_data),
        );
        let width = crate::menu::width(ui, &entries);
        let t = self.theme;
        let popup = egui::Popup::new(popup_id, ui.ctx().clone(), button_rect, ui.layer_id())
            .kind(egui::PopupKind::Menu)
            .open_memory(open)
            .gap(10.)
            .frame(crate::menu::frame(t))
            .show(|ui| crate::menu::rows(ui, &entries, width, t, &mut self.tree_menu.keyboard_row));
        if let Some(command) = popup.and_then(|p| p.inner) {
            self.actions.push(Action::TreeItem(target, command));
        }
    }

    pub fn tree_menu_shortcuts(&mut self, ctx: &egui::Context) {
        let Some((id, subject)) = self.tree_menu.active.clone() else {
            return;
        };
        if !egui::Popup::is_id_open(ctx, id) {
            return;
        }
        let folder = matches!(subject, Subject::Folder { .. });
        let command = {
            [
                ("cloneItem", Command::Clone),
                ("copyItem", Command::Copy),
                ("renameItem", Command::Rename),
                ("newRequest", Command::NewRequest),
                ("openTerminal", Command::Terminal),
            ]
            .into_iter()
            .filter(|(_, c)| folder || !matches!(c, Command::NewRequest | Command::Terminal))
            .find_map(|(k, c)| {
                crate::keybindings::pressed(ctx, &self.ws.preferences.keybindings, k).then_some(c)
            })
        };
        if let Some(command) = command {
            egui::Popup::close_all(ctx);
            self.actions.push(Action::TreeItem(subject, command));
        }
    }

    pub fn subject_name(&self, target: &Subject) -> String {
        match target {
            Subject::Request(id) => self
                .ws
                .requests
                .iter()
                .find(|r| r.id == *id)
                .map(|r| r.name.clone())
                .unwrap_or_default(),
            Subject::Folder { folder, .. } => folder.rsplit('/').next().unwrap_or(folder).into(),
        }
    }
    pub fn tree_item_action(&mut self, target: Subject, command: Command) {
        use Command::*;
        match command {
            Clone => self.start_tree_operation(Operation::Clone(target)),
            Copy => match tree_ops::prepare(&self.ws, Operation::Clone(target.clone())) {
                Ok(job) => {
                    let folders = if let Subject::Folder { folder, .. } = &target {
                        job.collection
                            .as_ref()
                            .map(|c| {
                                c.folders
                                    .iter()
                                    .filter(|f| tree_ops::inside(f, folder))
                                    .cloned()
                                    .collect()
                            })
                            .unwrap_or_default()
                    } else {
                        vec![]
                    };
                    self.tree_menu.copied = Some(Clipboard {
                        subject: target,
                        requests: job.requests,
                        folders,
                        settings: job.drafts,
                    });
                    self.notice("Copied");
                }
                Err(e) => self.notice(format!("Copy failed: {e:#}")),
            },
            Rename => {
                let name = self.subject_name(&target);
                self.show_dialog(Dialog::TreeRename(target));
                self.entry = name;
            }
            Delete => self.show_dialog(Dialog::TreeDelete(target)),
            Info => self.show_dialog(Dialog::TreeInfo(target)),
            NewRequest | NewFolder | Settings | Reveal | Terminal => {
                let Subject::Folder { collection, folder } = target else {
                    return;
                };
                match command {
                    NewRequest => self.actions.push(Action::NewRequest(collection, folder)),
                    NewFolder => {
                        self.entry.clear();
                        self.show_dialog(Dialog::TreeNewFolder(collection, folder));
                    }
                    Settings => self.open_folder_settings(&collection, &folder),
                    Reveal | Terminal => {
                        let path = self
                            .ws
                            .collections
                            .iter()
                            .find(|c| c.id == collection)
                            .context("Collection not found")
                            .and_then(|c| tree_ops::checked_path(c, &folder));
                        match path {
                            Ok(path) if command == Terminal => {
                                self.open_terminal(path);
                            }
                            Ok(path) => {
                                if let Err(e) = reveal(&path) {
                                    self.notice(format!("Cannot open File Explorer: {e}"));
                                }
                            }
                            Err(e) => self.notice(format!("{e:#}")),
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}
use anyhow::Context;

impl App {
    pub fn tree_item_dialog(&mut self, ctx: &egui::Context) -> bool {
        let dialog = self.dialog.clone();
        let (title, target) = match &dialog {
            Dialog::TreeRename(s) => (
                if matches!(s, Subject::Folder { .. }) {
                    "Rename Folder"
                } else {
                    "Rename Request"
                },
                Some(s),
            ),
            Dialog::TreeDelete(s) => (
                if matches!(s, Subject::Folder { .. }) {
                    "Delete Folder"
                } else {
                    "Delete Request"
                },
                Some(s),
            ),
            Dialog::TreeInfo(s) => ("Info", Some(s)),
            Dialog::TreeNewFolder(_, _) => ("New Folder", None),
            _ => return false,
        };
        let name = target.map(|s| self.subject_name(s)).unwrap_or_default();
        let folder = target.is_none_or(|s| matches!(s, Subject::Folder { .. }));
        let mut action = None;
        let mut cancel = false;
        let t = self.theme;
        let (_, closed) = w::modal(ctx, title, 600., t, |ui| {
            if matches!(dialog, Dialog::TreeInfo(_)) {
                egui::Grid::new("tree-item-info")
                    .spacing(vec2(16., 16.))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Folder Name").color(t.muted));
                        ui.label(format!(": {name}"));
                        ui.end_row();
                        ui.label(egui::RichText::new("Folder Name (on filesystem)").color(t.muted));
                        ui.label(format!(": {name}"));
                        ui.end_row();
                    });
                return;
            }
            let deleting = matches!(dialog, Dialog::TreeDelete(_));
            if deleting {
                ui.label(format!("Are you sure you want to delete \"{name}\"?"));
                if folder {
                    ui.label(
                        egui::RichText::new(
                            "All requests and subfolders inside this folder will be deleted.",
                        )
                        .color(t.muted),
                    );
                }
            } else {
                ui.label(
                    egui::RichText::new(if folder {
                        "Folder Name"
                    } else {
                        "Request Name"
                    })
                    .size(13.),
                );
                let edit = ui.add_sized(
                    [ui.available_width(), 30.],
                    egui::TextEdit::singleline(&mut self.entry)
                        .font(egui::FontId::proportional(13.)),
                );
                if self.dialog_first_frame {
                    edit.request_focus();
                }
            }
            ui.add_space(16.);
            let valid = deleting
                || if folder {
                    tree_ops::valid_name(&self.entry).is_ok()
                } else {
                    tree_ops::valid_request_name(&self.entry).is_ok()
                };
            let confirm = if deleting {
                "Delete"
            } else if matches!(dialog, Dialog::TreeRename(_)) {
                "Rename"
            } else {
                "Create"
            };
            let mut submit = false;
            ui.allocate_ui_with_layout(
                vec2(ui.available_width(), 30.),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    let r = ui
                        .allocate_space(vec2(w::text_width(ui, confirm, 12.) + 24., 30.))
                        .1;
                    let mut button_theme = t;
                    if deleting {
                        button_theme.accent = t.red;
                    }
                    if w::button(ui, r, confirm, true, button_theme).clicked() && valid {
                        submit = true;
                    }
                    let r = ui.allocate_space(vec2(68., 30.)).1;
                    if w::button(ui, r, "Cancel", false, t).clicked() {
                        cancel = true;
                    }
                },
            );
            if !deleting && valid && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                submit = true;
            }
            if submit {
                action = Some(match &dialog {
                    Dialog::TreeRename(s) => Operation::Rename(s.clone(), self.entry.trim().into()),
                    Dialog::TreeNewFolder(collection, parent) => Operation::NewFolder {
                        collection: collection.clone(),
                        parent: parent.clone(),
                        name: self.entry.trim().into(),
                    },
                    Dialog::TreeDelete(s) => Operation::Delete(s.clone()),
                    _ => unreachable!(),
                });
            }
        });
        self.dialog_first_frame = false;
        if let Some(operation) = action {
            self.actions.push(Action::TreeOperation(operation));
            self.dialog = Dialog::None;
        }
        if cancel || closed {
            self.dialog = Dialog::None;
        }
        true
    }
}

pub fn reveal(path: &std::path::Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Explorer parses /select,"path" itself; Rust's normal argument quoting
        // would quote the switch together with the path and open Documents.
        let path: std::path::PathBuf = path.components().collect();
        let mut argument = std::ffi::OsString::from("/select,\"");
        argument.push(path.as_os_str());
        argument.push("\"");
        std::process::Command::new("explorer.exe")
            .raw_arg(argument)
            .creation_flags(0x08000000)
            .spawn()?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn menus_retain_reference_order_protocol_variants_and_only_the_requested_actions() {
        let folder = items(true, "", false);
        assert_eq!(
            folder.iter().map(|i| i.label).collect::<Vec<_>>(),
            [
                "New Request",
                "New Folder",
                "New App",
                "New JS File",
                "Run",
                "Clone",
                "Copy",
                "Rename",
                "Reveal in File Explorer",
                "Ignore",
                "",
                "Info",
                "Settings",
                "Open in Terminal",
                "Delete"
            ]
        );
        assert_eq!(folder.iter().filter(|i| i.command.is_some()).count(), 10);
        let http = items(false, "http", false);
        assert_eq!(
            http.iter().map(|i| i.label).collect::<Vec<_>>(),
            [
                "Clone",
                "Copy",
                "Rename",
                "Generate Code",
                "Create Example",
                "Reveal in File Explorer",
                "",
                "Info",
                "Delete"
            ]
        );
        assert_eq!(http.iter().filter(|i| i.command.is_some()).count(), 4);
        let grpc = items(false, "grpc", false);
        assert_eq!(
            grpc.iter().map(|i| i.label).collect::<Vec<_>>(),
            [
                "Clone",
                "Copy",
                "Rename",
                "Run",
                "Reveal in File Explorer",
                "",
                "Info",
                "Delete"
            ]
        );
        assert_eq!(http[0].icon, http[1].icon);
        assert!(
            items(true, "", true)
                .iter()
                .any(|i| i.label == "Paste" && i.command.is_none())
        );
    }
    #[test]
    fn copy_keeps_an_independent_request_snapshot_and_rename_dialog_uses_current_name() {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, root.path().into()).unwrap();
        let request = Request {
            id: "copy-me".into(),
            name: "Draft request".into(),
            body: "unsaved body".into(),
            ..Default::default()
        };
        app.ws.requests = vec![request];
        let target = Subject::Request("copy-me".into());
        app.tree_item_action(target.clone(), Command::Copy);
        app.ws.requests[0].body = "edited later".into();
        assert_eq!(
            app.tree_menu.copied.as_ref().unwrap().requests[0].body,
            "unsaved body"
        );
        app.tree_item_action(target, Command::Rename);
        assert_eq!(app.entry, "Draft request");
    }
}
