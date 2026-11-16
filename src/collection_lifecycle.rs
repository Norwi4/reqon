use crate::{
    app::{App, Dialog},
    model::*,
    storage, widgets as w,
};
use anyhow::{Context, Result, ensure};
use eframe::egui::{self, Rect, TextEdit, Ui};
use std::collections::HashSet;

impl App {
    pub fn create_collection_inline(&mut self) {
        let base = self.workspace_folder().join("collections");
        let mut number = 1;
        let (name, path) = loop {
            let name = if number == 1 {
                "New Collection".into()
            } else {
                format!("New Collection {number}")
            };
            let path = base.join(&name);
            if !path.exists()
                && !self
                    .ws
                    .collections
                    .iter()
                    .any(|c| c.name.eq_ignore_ascii_case(&name))
            {
                break (name, path);
            }
            number += 1;
        };
        let collection = Collection {
            id: id(),
            name,
            path,
            environments: vec![],
            vars: vec![],
            folders: vec![],
        };
        if let Err(error) = storage::create_collection(&collection) {
            self.notice(format!("Cannot create collection: {error:#}"));
            return;
        }
        let cid = collection.id.clone();
        self.ws.collections.push(collection);
        self.tree.stale = true;
        self.mark_workspace_changed();
        self.begin_collection_rename(&cid);
    }
    pub fn begin_collection_rename(&mut self, cid: &str) {
        let Some(c) = self.ws.collections.iter().find(|c| c.id == cid) else {
            return;
        };
        self.collection_ui.rename = Some((cid.into(), c.name.clone(), true));
        self.open_collection_settings(cid);
        self.tabs
            .entry(format!("__collection__{cid}"))
            .or_default()
            .collection_tab = 0;
    }
    pub fn rename_collection(&mut self, cid: &str, name: &str) -> Result<()> {
        let name = name.trim();
        ensure!(!name.is_empty(), "Enter a collection name");
        let c = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == cid)
            .context("Collection no longer exists")?;
        let path = c.path.join("opencollection.yml");
        if path.is_file() {
            let mut parent = crate::yaml::read_request(&path)?;
            parent.name = name.into();
            storage::atomic_write(&path, crate::yaml::serialize(&parent)?)?;
        } else {
            let path = crate::native_format::manifest(&c.path);
            let mut value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            value["name"] = name.into();
            storage::atomic_write(&path, serde_json::to_vec_pretty(&value)?)?;
        }
        self.ws
            .collections
            .iter_mut()
            .find(|c| c.id == cid)
            .unwrap()
            .name = name.into();
        if let Some(draft) = self
            .ws
            .collection_drafts
            .get_mut(&format!("__collection__{cid}"))
        {
            draft.name = name.into();
            self.modified_requests.insert(draft.id.clone());
        }
        self.tree.stale = true;
        self.mark_workspace_changed();
        Ok(())
    }
    pub fn collection_name_editor(&mut self, ui: &mut Ui, r: Rect, cid: &str, name: &mut String) {
        if let Some((editing, mut text, focus)) = self.collection_ui.rename.take() {
            if editing == cid {
                let mut output = TextEdit::singleline(&mut text)
                    .id_salt(("collection-name", cid))
                    .font(egui::FontId::proportional(18.))
                    .desired_width(r.width())
                    .show(&mut w::child(ui, r, ("collection-name-area", cid)));
                if focus {
                    output.response.request_focus();
                    output
                        .state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::two(
                            egui::text::CCursor::new(0),
                            egui::text::CCursor::new(text.chars().count()),
                        )));
                    output.state.store(ui.ctx(), output.response.id);
                }
                let cancel =
                    output.response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape));
                let commit = !focus
                    && (output.response.lost_focus()
                        || output.response.has_focus()
                            && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                if !cancel && commit {
                    match self.rename_collection(cid, &text) {
                        Ok(()) => *name = text.trim().into(),
                        Err(error) => {
                            self.notice(format!("Cannot rename collection: {error:#}"));
                            self.collection_ui.rename = Some((editing, text, true));
                        }
                    }
                } else if !cancel {
                    self.collection_ui.rename = Some((editing, text, false));
                }
                return;
            }
            self.collection_ui.rename = Some((editing, text, focus));
        }
        w::medium(
            ui,
            egui::pos2(r.left(), r.top() + 16.),
            name,
            18.,
            self.theme.text,
        );
        if ui
            .interact(
                r,
                ui.scope_id().with(("rename-collection", cid)),
                egui::Sense::click(),
            )
            .on_hover_text("Rename collection")
            .clicked()
        {
            self.collection_ui.rename = Some((cid.into(), name.clone(), true));
        }
    }
    pub fn remove_collection(&mut self, cid: &str) -> Result<()> {
        ensure!(
            self.ws.collections.iter().any(|c| c.id == cid),
            "Collection no longer exists"
        );
        self.curl_flush();
        // Archive the complete local state, including unsaved tabs, before detaching it.
        let mut archive = self.ws.clone();
        archive.collections.retain(|c| c.id == cid);
        archive.requests.retain(|r| r.collection == cid);
        archive.collection_drafts.retain(|_, r| r.collection == cid);
        archive.history.retain(|h| h.request.collection == cid);
        let owned: HashSet<_> = archive
            .requests
            .iter()
            .map(|r| r.id.clone())
            .chain(archive.collection_drafts.keys().cloned())
            .chain(
                self.ws
                    .open_tabs
                    .iter()
                    .filter(|id| id.contains(cid))
                    .cloned(),
            )
            .collect();
        archive.open_tabs.retain(|id| owned.contains(id));
        if archive
            .active
            .as_ref()
            .is_some_and(|id| !owned.contains(id))
        {
            archive.active = None;
        }
        storage::save(
            &self
                .root
                .join("removed-collections")
                .join(format!("{cid}-{}", id())),
            &archive,
        )?;
        for id in &owned {
            self.cancel(id);
        }
        self.ws.collections.retain(|c| c.id != cid);
        self.ws.requests.retain(|r| r.collection != cid);
        self.ws.collection_drafts.retain(|_, r| r.collection != cid);
        self.ws.open_tabs.retain(|id| !owned.contains(id));
        if self.ws.active.as_ref().is_some_and(|id| owned.contains(id)) {
            self.ws.active = self.ws.open_tabs.last().cloned();
        }
        self.ws
            .saved_fingerprints
            .retain(|id, _| !owned.contains(id));
        self.saved.retain(|id, _| !owned.contains(id));
        self.tabs.retain(|id, _| !owned.contains(id));
        self.curl.drafts.retain(|id, _| !owned.contains(id));
        self.dirty_cache
            .borrow_mut()
            .retain(|id, _| !owned.contains(id));
        self.modified_requests.retain(|id| !owned.contains(id));
        self.git.remove(cid);
        self.runtime_vars.remove(cid);
        self.ws.last_request_kind.remove(cid);
        if let Some(sync) = &mut self.disk_sync {
            sync.watch(&self.ws.collections);
        }
        self.tree.stale = true;
        self.mark_workspace_changed();
        self.flush();
        self.notice("Collection removed from workspace");
        Ok(())
    }
    pub fn remove_collection_dialog(&mut self, ctx: &egui::Context, cid: &str) {
        let Some(c) = self.ws.collections.iter().find(|c| c.id == cid) else {
            self.dialog = Dialog::None;
            return;
        };
        let (name, path) = (c.name.clone(), c.path.display().to_string());
        let (confirmed, closed) = w::modal(ctx, "Remove Collection", 500., self.theme, |ui| {
            ui.label(format!("Remove \"{name}\" from this workspace?"));
            ui.add_space(8.);
            ui.weak(path);
            ui.add_space(8.);
            ui.label("Collection files stay on disk and can be opened again. Local drafts are kept in the workspace archive.");
            ui.add_space(12.);
            ui.horizontal(|ui| {
                let remove = ui.button("Remove").clicked();
                if ui.button("Cancel").clicked() {
                    ui.close();
                }
                remove
            })
            .inner
        });
        if confirmed {
            match self.remove_collection(cid) {
                Ok(()) => self.dialog = Dialog::None,
                Err(e) => self.notice(format!("Cannot remove collection: {e:#}")),
            }
        } else if closed {
            self.dialog = Dialog::None;
        }
    }
}
