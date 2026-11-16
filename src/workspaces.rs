use crate::{
    app::App,
    model::{Pair, Preferences},
    workspace_store::{self, Entry, Registry},
};
use anyhow::{Context, Result};
use std::{collections::HashMap, path::PathBuf, sync::mpsc};

#[derive(Clone)]
pub enum Command {
    Create,
    Open,
    Import,
    Manage,
    Switch(String),
    Pin(String),
    Reorder(String, String),
    Rename(String),
    Remove(String),
    Reveal(String),
    Terminal(String),
    Export,
    Move(String, String),
}
#[derive(Clone)]
pub enum Dialog {
    Create,
    Import,
    Rename(String),
    Remove(String),
    Move(String),
}
pub enum Job {
    Loaded(Entry, Box<App>),
    Imported(PathBuf),
    Exported(PathBuf),
}
#[derive(Default)]
pub struct State {
    pub updates: crate::updates_ui::State,
    pub data: PathBuf,
    pub registry: Registry,
    pub inactive: HashMap<String, Box<App>>,
    pub command: Option<Command>,
    pub dialog: Option<Dialog>,
    pub manage: bool,
    pub name: String,
    pub folder: String,
    pub location: String,
    pub file: String,
    pub folder_custom: bool,
    pub error: String,
    pub loading: String,
    pub job: Option<mpsc::Receiver<Result<Job, String>>>,
    pub remove_after_switch: Option<String>,
}
impl App {
    pub fn workspace_globals(&self) -> Vec<Pair> {
        let mut vars = self.ws.globals.clone();
        if let Some(env) = self
            .ws
            .environments
            .iter()
            .find(|e| e.name == self.ws.workspace_environment)
        {
            vars.extend(env.vars.iter().filter(|p| p.enabled).cloned());
        }
        vars
    }
    pub fn workspace_folder(&self) -> PathBuf {
        if self.root.file_name().is_some_and(|n| n == ".reqon") {
            self.root.parent().unwrap_or(&self.root).into()
        } else {
            self.root.clone()
        }
    }
    pub fn workspace_tick(&mut self, ctx: &eframe::egui::Context) {
        let completed = self
            .workspaces
            .job
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(result) = completed {
            self.workspaces.job = None;
            self.workspaces.loading.clear();
            match result {
                Ok(Job::Loaded(entry, app)) => {
                    self.activate_workspace(entry, app);
                }
                Ok(Job::Imported(path)) => self.load_workspace(path),
                Ok(Job::Exported(path)) => {
                    self.notice(format!("Workspace exported to {}", path.display()))
                }
                Err(error) => {
                    self.workspaces.remove_after_switch = None;
                    self.workspaces.error = error.clone();
                    self.notice(error);
                }
            }
        }
        if self.workspaces.job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(command) = self.workspaces.command.take() {
            if let Err(error) = self.workspace_command(command) {
                self.workspaces.error = format!("{error:#}");
                self.notice(format!("Workspace: {error:#}"));
            }
        }
    }
    pub fn workspace_job(
        &mut self,
        label: &str,
        task: impl FnOnce() -> Result<Job> + Send + 'static,
    ) {
        let (tx, rx) = mpsc::channel();
        let ctx = self.context.clone();
        self.workspaces.job = Some(rx);
        self.workspaces.loading = label.into();
        self.workspaces.error.clear();
        std::thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task))
                .map_err(|_| "Workspace operation failed unexpectedly".to_owned())
                .and_then(|r| r.map_err(|e| format!("{e:#}")));
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
    pub fn load_workspace(&mut self, path: PathBuf) {
        if self.workspaces.job.is_some() {
            return;
        }
        if let Ok(canonical) = std::fs::canonicalize(&path) {
            if let Some(entry) = self
                .workspaces
                .registry
                .entries
                .iter()
                .find(|e| e.path == canonical)
                .cloned()
            {
                if let Err(e) = self.workspace_command(Command::Switch(entry.id)) {
                    self.notice(format!("{e:#}"));
                }
                return;
            }
        }
        let context = self.context.clone();
        let preferences = self.ws.preferences.clone();
        self.workspace_job("Opening workspace…", move || {
            let path = std::fs::canonicalize(path)?;
            let m = workspace_store::read_manifest(&path)?;
            let entry = Entry {
                id: crate::model::id(),
                name: m.name,
                path,
                default: false,
                pinned: false,
            };
            load_session(entry, &context, &preferences)
        });
    }
    fn activate_workspace(&mut self, entry: Entry, mut next: Box<App>) {
        self.flush();
        let mut state = std::mem::take(&mut self.workspaces);
        let previous = state.registry.active.clone();
        if !state.registry.entries.iter().any(|e| e.id == entry.id) {
            state.registry.entries.push(entry.clone());
        }
        state.registry.active = entry.id.clone();
        state.manage = false;
        state.dialog = None;
        state.error.clear();
        next.ws.preferences = self.ws.preferences.clone();
        next.ws.dark = self.ws.dark;
        next.engine.configure(next.ws.preferences.clone());
        next.theme =
            crate::theme_registry::current(&next.ws.preferences, next.ws.dark, &next.context);
        next.theme.apply(&next.context);
        next.mark_workspace_changed();
        std::mem::swap(self, &mut next);
        if !previous.is_empty() {
            state.inactive.insert(previous, next);
        }
        if let Some(remove) = state.remove_after_switch.take() {
            state.inactive.remove(&remove);
            let _ = state.registry.remove(&remove);
        }
        if let Err(e) = state.registry.save(&state.data) {
            self.notice(format!("Could not save workspace list: {e:#}"));
        }
        self.workspaces = state;
        crate::file_cache::configure(&self.workspaces.data, self.ws.preferences.cache.file);
        crate::git::configure_provider(&self.ws.preferences);
        self.context.request_repaint();
    }
    fn workspace_command(&mut self, command: Command) -> Result<()> {
        if self.workspaces.job.is_some() {
            return Ok(());
        }
        use Command::*;
        match command {
            Create => {
                self.workspaces.name.clear();
                self.workspaces.folder.clear();
                self.workspaces.folder_custom = false;
                self.workspaces.location = self.ws.preferences.default_location.clone();
                self.workspaces.dialog = Some(Dialog::Create);
            }
            Open => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Open workspace")
                    .pick_folder()
                {
                    self.load_workspace(path);
                }
            }
            Import => {
                self.workspaces.file.clear();
                self.workspaces.location = self.ws.preferences.default_location.clone();
                self.workspaces.dialog = Some(Dialog::Import);
            }
            Manage => {
                self.workspaces.manage = true;
            }
            Switch(id) => {
                if id == self.workspaces.registry.active {
                    self.workspaces.manage = false;
                    return Ok(());
                }
                let entry = self
                    .workspaces
                    .registry
                    .entries
                    .iter()
                    .find(|e| e.id == id)
                    .cloned()
                    .context("Workspace no longer exists")?;
                if let Some(next) = self.workspaces.inactive.remove(&id) {
                    self.activate_workspace(entry, next);
                } else {
                    let context = self.context.clone();
                    let preferences = self.ws.preferences.clone();
                    self.workspace_job("Opening workspace…", move || {
                        load_session(entry, &context, &preferences)
                    });
                }
            }
            Pin(id) => {
                if let Some(e) = self
                    .workspaces
                    .registry
                    .entries
                    .iter_mut()
                    .find(|e| e.id == id && !e.default)
                {
                    e.pinned = !e.pinned;
                }
                self.workspaces.registry.save(&self.workspaces.data)?;
            }
            Reorder(source, target) => {
                self.workspaces.registry.reorder(&source, &target);
                self.workspaces.registry.save(&self.workspaces.data)?;
            }
            Rename(id) => {
                let entry = self
                    .workspaces
                    .registry
                    .entries
                    .iter()
                    .find(|e| e.id == id && !e.default)
                    .context("The default workspace cannot be renamed")?;
                self.workspaces.name = entry.name.clone();
                self.workspaces.dialog = Some(Dialog::Rename(id));
            }
            Remove(id) => {
                self.workspaces.dialog = Some(Dialog::Remove(id));
            }
            Reveal(ref id) | Terminal(ref id) => {
                let entry = self
                    .workspaces
                    .registry
                    .entries
                    .iter()
                    .find(|e| &e.id == id)
                    .context("Workspace not found")?;
                let path = entry.path.clone();
                if matches!(command, Terminal(_)) {
                    self.open_terminal(path);
                } else {
                    crate::tree_menu::reveal(&path)?;
                }
            }
            Export => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Export workspace")
                    .add_filter("Workspace ZIP", &["zip"])
                    .set_file_name(format!("{}.zip", crate::model::safe_name(&self.ws.name)))
                    .save_file()
                {
                    self.flush();
                    let barrier = self.writer.barrier();
                    let ws = self.ws.clone();
                    self.workspace_job("Exporting workspace…", move || {
                        barrier.recv().context("Could not finish pending saves")?;
                        crate::workspace_zip::export(&ws, &path)?;
                        Ok(Job::Exported(path))
                    });
                }
            }
            Move(cid, target) => self.move_workspace_collection(&cid, &target)?,
        }
        Ok(())
    }
    pub fn rename_workspace(&mut self, id: &str, name: &str) -> Result<()> {
        anyhow::ensure!(!name.trim().is_empty(), "Enter a workspace name");
        let entry = self
            .workspaces
            .registry
            .entries
            .iter_mut()
            .find(|e| e.id == id && !e.default)
            .context("Workspace cannot be renamed")?;
        let mut manifest = workspace_store::read_manifest(&entry.path)?;
        manifest.name = name.trim().into();
        workspace_store::write_manifest(&entry.path, &manifest)?;
        entry.name = manifest.name.clone();
        if id == self.workspaces.registry.active {
            self.ws.name = manifest.name;
            self.mark_workspace_changed();
            self.flush();
        } else if let Some(app) = self.workspaces.inactive.get_mut(id) {
            app.ws.name = manifest.name;
            app.mark_workspace_changed();
            app.flush();
        } else if entry.state_root().join("workspace.json").exists() {
            let mut ws = crate::storage::load(&entry.state_root())?;
            ws.name = manifest.name;
            crate::storage::save(&entry.state_root(), &ws)?;
        }
        self.workspaces.registry.save(&self.workspaces.data)
    }
    pub fn remove_workspace(&mut self, id: &str) -> Result<()> {
        anyhow::ensure!(id != "default", "The default workspace cannot be removed");
        if id == self.workspaces.registry.active {
            self.workspaces.remove_after_switch = Some(id.into());
            self.workspace_command(Command::Switch("default".into()))?;
        } else {
            self.workspaces.inactive.remove(id);
            self.workspaces.registry.remove(id)?;
            self.workspaces.registry.save(&self.workspaces.data)?;
        }
        Ok(())
    }
    fn move_workspace_collection(&mut self, cid: &str, target: &str) -> Result<()> {
        anyhow::ensure!(
            target != self.workspaces.registry.active,
            "Choose another workspace"
        );
        anyhow::ensure!(
            !self.running_context.values().any(|(c, _, _)| c == cid),
            "Wait for this collection's running requests to finish"
        );
        anyhow::ensure!(
            !self.tree_moving,
            "Wait for the current collection operation to finish"
        );
        let entry = self
            .workspaces
            .registry
            .entries
            .iter()
            .find(|e| e.id == target)
            .cloned()
            .context("Workspace not found")?;
        let collection = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == cid)
            .cloned()
            .context("Collection not found")?;
        workspace_store::prepare(&entry)?;
        let mut ws = if let Some(app) = self.workspaces.inactive.get(target) {
            app.ws.clone()
        } else {
            crate::storage::load(&entry.state_root())?
        };
        anyhow::ensure!(
            !ws.collections.iter().any(|c| c.path == collection.path),
            "This collection is already in the destination workspace"
        );
        ws.collections.push(collection);
        ws.requests.extend(
            self.ws
                .requests
                .iter()
                .filter(|r| r.collection == cid)
                .cloned(),
        );
        let transferred: std::collections::HashSet<_> = self
            .ws
            .requests
            .iter()
            .filter(|r| r.collection == cid)
            .map(|r| r.id.clone())
            .collect();
        ws.saved_fingerprints.extend(
            self.ws
                .saved_fingerprints
                .iter()
                .filter(|(id, _)| transferred.contains(*id))
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        ws.collection_drafts.extend(
            self.ws
                .collection_drafts
                .iter()
                .filter(|(id, _)| id.contains(cid))
                .map(|(k, v)| (k.clone(), v.clone())),
        );
        ws.open_tabs.extend(
            self.ws
                .open_tabs
                .iter()
                .filter(|id| transferred.contains(*id) || id.contains(cid))
                .cloned(),
        );
        // Save the destination before removing the source registration. Files stay at their original paths.
        if let Some(app) = self.workspaces.inactive.get_mut(target) {
            app.saved.extend(
                self.saved
                    .iter()
                    .filter(|(id, _)| transferred.contains(*id) || id.contains(cid))
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
            app.ws = ws;
            app.modified_requests
                .extend(app.ws.requests.iter().map(|r| r.id.clone()));
            app.mark_changed();
            app.flush();
            app.writer.barrier().recv()?;
        } else {
            crate::storage::save(&entry.state_root(), &ws)?;
        }
        let removed: std::collections::HashSet<_> = self
            .ws
            .requests
            .iter()
            .filter(|r| r.collection == cid)
            .map(|r| r.id.clone())
            .collect();
        self.ws.collections.retain(|c| c.id != cid);
        self.ws.requests.retain(|r| r.collection != cid);
        self.ws.collection_drafts.retain(|id, _| !id.contains(cid));
        self.ws
            .saved_fingerprints
            .retain(|id, _| !removed.contains(id) && !id.contains(cid));
        self.ws
            .open_tabs
            .retain(|id| !removed.contains(id) && !id.contains(cid));
        if self
            .ws
            .active
            .as_ref()
            .is_some_and(|id| removed.contains(id) || id.contains(cid))
        {
            self.ws.active = None;
        }
        self.tree.stale = true;
        self.mark_workspace_changed();
        self.flush();
        self.notice("Collection moved to workspace");
        Ok(())
    }
}
pub fn load_session(
    entry: Entry,
    ctx: &eframe::egui::Context,
    preferences: &Preferences,
) -> Result<Job> {
    workspace_store::prepare(&entry)?;
    let mut app = App::from_workspace_preferences(ctx, entry.state_root(), Some(preferences))?;
    app.ws.preferences = preferences.clone();
    app.engine.configure(preferences.clone());
    Ok(Job::Loaded(entry, Box::new(app)))
}
