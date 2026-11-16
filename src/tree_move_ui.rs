use crate::tree_move::{Outcome, Target, apply, plan};
use anyhow::Result;

impl crate::app::App {
    pub fn move_tree_request(&mut self, source: String, target: Target) {
        match plan(&self.ws, &source, target) {
            Ok(plan) if !plan.changes.is_empty() => {
                // Drop the old watcher/receiver, preventing snapshots from the
                // middle of a transaction from deleting moved tabs or drafts.
                self.disk_sync = None;
                self.tree_moving = true;
                self.writer.move_requests(plan);
            }
            Ok(_) => {}
            Err(e) => self.notice(format!("Cannot move request: {e:#}")),
        }
    }
    pub fn finish_tree_moves(&mut self) {
        while let Ok(result) = self.writer.moved.try_recv() {
            self.finish_tree_move(result);
        }
    }
    pub fn finish_tree_move(&mut self, result: Result<Outcome, String>) {
        self.finish_saves();
        self.tree_moving = false;
        match result {
            Ok(outcome) => {
                self.modified_requests.extend(apply(&mut self.ws, &outcome));
                for (id, source) in outcome.saved {
                    self.ws
                        .saved_fingerprints
                        .insert(id.clone(), crate::disk_sync::fingerprint(&source));
                    self.saved.insert(id, source);
                }
                self.dirty_cache.borrow_mut().clear();
                for change in &outcome.plan.changes {
                    self.collapsed.remove(&change.after.collection);
                    let mut prefix = String::new();
                    for part in change.after.folder.split('/').filter(|p| !p.is_empty()) {
                        if !prefix.is_empty() {
                            prefix.push('/');
                        }
                        prefix.push_str(part);
                        self.collapsed
                            .remove(&format!("{}:{prefix}", change.after.collection));
                    }
                }
                if let Some(folder) = outcome.plan.folder {
                    self.open_folder_settings(&folder.collection, &folder.folder);
                }
                self.mark_changed();
                self.notice("Request moved");
            }
            Err(e) => self.notice(e),
        }
        self.disk_sync = crate::disk_sync::DiskSync::new(self.context.clone()).ok();
        if let Some(sync) = &mut self.disk_sync {
            sync.watch(&self.ws.collections);
            sync.refresh();
        }
        self.finish_deferred_tree_saves();
        self.flush();
    }
}
