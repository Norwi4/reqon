use crate::{
    app::{Action, App, Dialog},
    menu::{self, Item},
};
use eframe::egui::{self, Response, Ui};

#[derive(Clone, Copy)]
enum Command {
    NewRequest,
    NewCollection,
    OpenCollection,
    ImportCollection,
    ImportCurl,
    ExportCollection,
    Preferences,
    Updates,
    About,
}

fn items() -> Vec<Item<Command>> {
    use Command::*;
    let mut entries = vec![
        Item::new("New request", "FilePlus", Some(NewRequest)),
        Item::new("Create collection", "FolderPlus", Some(NewCollection)),
        Item::new("Open collection", "FolderOpen", Some(OpenCollection)),
        Item::separator(),
        Item::new("Import collection", "Import", Some(ImportCollection)),
        Item::new("Import cURL", "Terminal", Some(ImportCurl)),
        Item::new("Export collection", "Export", Some(ExportCollection)),
        Item::separator(),
        Item::new("Preferences", "Settings", Some(Preferences)),
        Item::new("Check for updates", "Update", Some(Updates)),
        Item::new("About Reqon", "Info", Some(About)),
    ];
    entries[0].shortcut = "Ctrl+N";
    entries[2].shortcut = "Ctrl+O";
    entries
}

impl App {
    pub fn main_menu(&mut self, ui: &mut Ui, response: &Response) {
        let popup_id = egui::Popup::default_response_id(response);
        if !response.clicked() && !egui::Popup::is_id_open(ui.ctx(), popup_id) {
            return;
        }
        let selection_id = popup_id.with("keyboard-row");
        let mut keyboard_row = if response.clicked() {
            None
        } else {
            ui.ctx()
                .data_mut(|data| data.get_temp::<usize>(selection_id))
        };
        let entries = items();
        let width = menu::width(ui, &entries).max(244.);
        let popup = egui::Popup::menu(response)
            .gap(9.)
            .frame(menu::frame(self.theme))
            .show(|ui| menu::rows(ui, &entries, width, self.theme, &mut keyboard_row));
        ui.ctx().data_mut(|data| {
            if let Some(row) = keyboard_row {
                data.insert_temp(selection_id, row);
            } else {
                data.remove::<usize>(selection_id);
            }
        });
        if let Some(command) = popup.and_then(|popup| popup.inner) {
            match command {
                Command::NewRequest => self.actions.push(Action::NewRequest(
                    self.active_collection_id(),
                    String::new(),
                )),
                Command::NewCollection => self.actions.push(Action::NewCollection),
                Command::OpenCollection => self.actions.push(Action::OpenCollection),
                Command::ImportCollection => self.actions.push(Action::ImportPostman),
                Command::ImportCurl => self.actions.push(Action::Dialog(Dialog::ImportCurl)),
                Command::ExportCollection => self.actions.push(Action::ExportPostman(None)),
                Command::Preferences => self.actions.push(Action::Dialog(Dialog::Settings)),
                Command::Updates => self.open_updates(),
                Command::About => self.actions.push(Action::Dialog(Dialog::About)),
            }
        }
    }
}
