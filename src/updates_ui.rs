use crate::{
    app::App,
    update_install::{self, Handoff, Installation, Pending},
    updates::{self, Download, Release},
    widgets,
};
use eframe::egui::{self, RichText};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default, PartialEq)]
pub enum Stage {
    #[default]
    Idle,
    Checking,
    Current,
    Available,
    Downloading,
    Ready,
    Saving,
    Updated,
    Error,
}
enum Event {
    Checked(Result<Option<Release>, String>),
    Progress(u64),
    Downloaded(Result<Download, String>),
}
struct Barrier {
    root: PathBuf,
    errors: u64,
    receiver: mpsc::Receiver<()>,
    done: bool,
}
pub struct State {
    pub shown: bool,
    pub stage: Stage,
    pub handoff: Handoff,
    release: Option<Release>,
    download: Option<Download>,
    installation: Option<Installation>,
    unsupported: String,
    message: String,
    received: u64,
    cancel: Arc<AtomicBool>,
    worker: Option<mpsc::Receiver<Event>>,
    barriers: Vec<Barrier>,
    saving_since: Option<Instant>,
    auto_install: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            shown: false,
            stage: Stage::Idle,
            handoff: Default::default(),
            release: None,
            download: None,
            installation: None,
            unsupported: String::new(),
            message: String::new(),
            received: 0,
            cancel: Default::default(),
            worker: None,
            barriers: vec![],
            saving_since: None,
            auto_install: false,
        }
    }
}
impl Drop for State {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if self.handoff.lock().is_ok_and(|h| h.is_none()) {
            if let Some(download) = &self.download {
                discard(download);
            }
        }
    }
}
fn discard(download: &Download) {
    // Only remove the fresh, uniquely named directory created by our downloader.
    if let Some(directory) = download.installer.parent() {
        if directory
            .file_name()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix("reqon-update-"))
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        {
            let _ = std::fs::remove_dir_all(directory);
        }
    }
}
impl State {
    pub fn restore_outcome(&mut self, root: &std::path::Path) {
        if let Some(outcome) = update_install::take_outcome(root) {
            self.stage = if outcome.success {
                Stage::Updated
            } else {
                Stage::Error
            };
            self.message = outcome.message;
            self.shown = !outcome.success;
        }
    }
    pub fn locked(&self) -> bool {
        self.stage == Stage::Saving
    }
    fn check(&mut self, ctx: &egui::Context) {
        if matches!(
            self.stage,
            Stage::Checking | Stage::Downloading | Stage::Saving
        ) {
            return;
        }
        if let Some(download) = self.download.take() {
            discard(&download);
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Default::default();
        self.stage = Stage::Checking;
        self.message.clear();
        self.release = None;
        match update_install::installed() {
            Ok(installation) => {
                self.installation = Some(installation);
                self.unsupported.clear();
            }
            Err(error) => {
                self.installation = None;
                self.unsupported = format!("{error:#}");
            }
        }
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            update_install::cleanup_cache();
            let _ = tx.send(Event::Checked(
                updates::check(env!("CARGO_PKG_VERSION")).map_err(|e| format!("{e:#}")),
            ));
            ctx.request_repaint();
        });
    }
    fn download(&mut self, ctx: &egui::Context) {
        let Some(release) = self.release.clone() else {
            return;
        };
        if self.installation.is_none() {
            return;
        }
        let cache = match update_install::cache_root() {
            Ok(path) => path,
            Err(e) => {
                self.message = format!("{e:#}");
                self.stage = Stage::Error;
                return;
            }
        };
        self.stage = Stage::Downloading;
        self.received = 0;
        self.message.clear();
        self.cancel = Default::default();
        let cancel = self.cancel.clone();
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let result = updates::download(release, &cache, &cancel, |received| {
                let _ = tx.send(Event::Progress(received));
                ctx.request_repaint();
            })
            .map_err(|e| format!("{e:#}"));
            if let Err(mpsc::SendError(Event::Downloaded(Ok(download)))) =
                tx.send(Event::Downloaded(result))
            {
                discard(&download);
            }
            ctx.request_repaint();
        });
    }
    fn poll(&mut self) {
        let Some(receiver) = &self.worker else { return };
        let mut finished = false;
        loop {
            match receiver.try_recv() {
                Ok(Event::Progress(received)) => self.received = received,
                Ok(Event::Checked(result)) => {
                    finished = true;
                    match result {
                        Ok(Some(release)) => {
                            self.release = Some(release);
                            self.stage = Stage::Available;
                        }
                        Ok(None) => self.stage = Stage::Current,
                        Err(error) => {
                            self.message = error;
                            self.stage = Stage::Error;
                        }
                    }
                    break;
                }
                Ok(Event::Downloaded(result)) => {
                    finished = true;
                    match result {
                        Ok(download) => {
                            self.download = Some(download);
                            self.stage = Stage::Ready;
                            self.auto_install = true;
                        }
                        Err(error) => {
                            self.message = error;
                            self.stage = Stage::Error;
                        }
                    }
                    break;
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = "The update worker stopped. Please try again.".into();
                    self.stage = Stage::Error;
                    finished = true;
                    break;
                }
            }
        }
        if finished {
            self.worker = None;
        }
    }
}
impl App {
    pub fn open_updates(&mut self) {
        self.workspaces.updates.shown = true;
        if matches!(
            self.workspaces.updates.stage,
            Stage::Idle | Stage::Current | Stage::Error | Stage::Updated
        ) {
            self.workspaces.updates.check(&self.context);
        }
    }
    fn update_busy(&self) -> bool {
        let busy = |app: &App| {
            !app.running.is_empty()
                || app.runner_active
                || app.tree_moving
                || app.terminal_running
                || app.terminal.has_running_sessions()
                || app.git.values().any(|g| g.busy)
                || app.git_clone.is_some()
                || !app.git_pending.is_empty()
                || !app.preference_ui.jobs.is_empty()
                || app.auth_migrations.values().any(|state| state.is_pending())
        };
        self.workspaces.job.is_some()
            || busy(self)
            || self.workspaces.inactive.values().any(|app| busy(app))
    }
    fn save_for_update(&mut self) {
        if self.update_busy() {
            self.workspaces.updates.message="The update is ready. Finish active requests, terminal sessions and file operations, then restart.".into();
            self.workspaces.updates.auto_install = false;
            self.workspaces.updates.shown = true;
            return;
        }
        let root = self.workspaces.data.clone();
        if let Err(error) = self.workspaces.registry.save(&root) {
            self.workspaces.updates.message =
                format!("Could not save the workspace list: {error:#}");
            self.workspaces.updates.auto_install = false;
            return;
        }
        let mut barriers = vec![];
        let mut save = |app: &mut App| -> anyhow::Result<()> {
            app.curl_flush();
            app.flush();
            app.engine.cookies.save(&app.root)?;
            barriers.push(Barrier {
                root: app.root.clone(),
                errors: app.save_error_generation,
                receiver: app.writer.barrier(),
                done: false,
            });
            Ok(())
        };
        let result = save(self).and_then(|_| {
            for app in self.workspaces.inactive.values_mut() {
                save(app)?;
            }
            Ok(())
        });
        if let Err(error) = result {
            self.workspaces.updates.message =
                format!("Could not save before restarting: {error:#}");
            self.workspaces.updates.auto_install = false;
            return;
        }
        let state = &mut self.workspaces.updates;
        state.stage = Stage::Saving;
        state.barriers = barriers;
        state.saving_since = Some(Instant::now());
        state.message.clear();
    }
    pub fn updates_tick(&mut self, ctx: &egui::Context) {
        self.workspaces.updates.poll();
        if self.workspaces.updates.stage == Stage::Ready && self.workspaces.updates.auto_install {
            self.save_for_update();
        }
        if self.workspaces.updates.stage == Stage::Saving {
            let mut failed = false;
            for barrier in &mut self.workspaces.updates.barriers {
                if !barrier.done {
                    match barrier.receiver.try_recv() {
                        Ok(()) => barrier.done = true,
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            failed = true;
                            barrier.done = true;
                        }
                    }
                }
            }
            let saved = self
                .workspaces
                .updates
                .barriers
                .iter()
                .all(|barrier| barrier.done);
            if saved {
                // A writer may send its error after App::poll but before the barrier.
                // Drain again *after* the barrier before deciding it is safe to close.
                let drain = |app: &mut App| {
                    while let Ok(error) = app.writer.errors.try_recv() {
                        app.save_error_generation = app.save_error_generation.wrapping_add(1);
                        app.notice(error);
                    }
                };
                drain(self);
                for app in self.workspaces.inactive.values_mut() {
                    drain(app);
                }
            }
            let errors: std::collections::HashMap<_, _> =
                std::iter::once((&self.root, self.save_error_generation))
                    .chain(
                        self.workspaces
                            .inactive
                            .values()
                            .map(|app| (&app.root, app.save_error_generation)),
                    )
                    .map(|(root, count)| (root.clone(), count))
                    .collect();
            let state = &mut self.workspaces.updates;
            failed |= state
                .barriers
                .iter()
                .any(|barrier| errors.get(&barrier.root) != Some(&barrier.errors));
            if state
                .saving_since
                .is_some_and(|at| at.elapsed() > Duration::from_secs(30))
            {
                failed = true;
            }
            if failed {
                state.stage = Stage::Ready;
                state.auto_install = false;
                state.barriers.clear();
                state.message="Could not finish saving your work. Resolve the save error and retry the update.".into();
                state.shown = true;
            } else if saved {
                if let (Some(download), Some(installation)) =
                    (state.download.clone(), state.installation.clone())
                {
                    *state.handoff.lock().unwrap() = Some(Pending {
                        download,
                        installation,
                        data_root: self.workspaces.data.clone(),
                        parent_id: std::process::id(),
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
        if matches!(
            self.workspaces.updates.stage,
            Stage::Checking | Stage::Downloading | Stage::Saving
        ) {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
    pub fn updates_dialog(&mut self, ctx: &egui::Context) {
        if !self.workspaces.updates.shown {
            return;
        }
        let busy = self.update_busy();
        let state = &mut self.workspaces.updates;
        let mut install = false;
        let (_, closed) = widgets::modal(ctx, "Software Updates", 480., self.theme, |ui| {
            ui.label(RichText::new("Reqon").size(24.));
            ui.label(format!("Installed version {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(16.);
            match state.stage {
                Stage::Idle => {
                    ui.label("Check for the latest stable version of Reqon.");
                }
                Stage::Checking => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Checking for updates…");
                    });
                }
                Stage::Current => {
                    ui.label("You're up to date.");
                }
                Stage::Updated => {
                    ui.label(&state.message);
                }
                Stage::Available | Stage::Downloading | Stage::Ready => {
                    if let Some(release) = &state.release {
                        ui.label(
                            RichText::new(format!("Reqon {} is available", release.version))
                                .strong(),
                        );
                        ui.hyperlink_to("What's new", &release.notes_url);
                        ui.add_space(10.);
                        if state.stage == Stage::Downloading {
                            let fraction = state.received as f32 / release.installer.size as f32;
                            ui.add(egui::ProgressBar::new(fraction).show_percentage());
                            ui.label(format!(
                                "Downloading {:.1} / {:.1} MB",
                                state.received as f64 / 1048576.,
                                release.installer.size as f64 / 1048576.
                            ));
                        } else if state.stage == Stage::Ready {
                            ui.label("Download verified. Ready to restart.");
                        } else {
                            ui.label(format!(
                                "Download size: {:.1} MB",
                                release.installer.size as f64 / 1048576.
                            ));
                        }
                        if !state.unsupported.is_empty() {
                            ui.label(&state.unsupported);
                            ui.hyperlink_to("Download release", &release.notes_url);
                        } else {
                            ui.label("Your workspaces and drafts will be saved. Reqon will reopen after the update.");
                        }
                    }
                }
                Stage::Saving => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Saving your work and restarting…");
                    });
                }
                Stage::Error => {
                    ui.label(RichText::new(&state.message).color(self.theme.red));
                }
            }
            if !state.message.is_empty() && matches!(state.stage, Stage::Ready | Stage::Available) {
                ui.add_space(8.);
                ui.label(&state.message);
            }
            ui.add_space(18.);
            ui.horizontal(|ui| {
                match state.stage {
                    Stage::Idle | Stage::Current | Stage::Error | Stage::Updated => {
                        if ui.button("Check for updates").clicked() {
                            state.check(ctx);
                        }
                    }
                    Stage::Available => {
                        if state.installation.is_some() && ui.button("Update and restart").clicked()
                        {
                            state.download(ctx);
                        }
                    }
                    Stage::Downloading => {
                        if ui.button("Cancel download").clicked() {
                            state.cancel.store(true, Ordering::Relaxed);
                            state.worker = None;
                            state.stage = Stage::Available;
                        }
                    }
                    Stage::Ready => {
                        if ui
                            .add_enabled(!busy, egui::Button::new("Restart and update"))
                            .on_disabled_hover_text("Finish active operations before restarting")
                            .clicked()
                        {
                            install = true;
                        }
                    }
                    _ => {}
                }
                if !state.locked() && ui.button("Close").clicked() {
                    state.shown = false;
                }
            });
        });
        if closed && !state.locked() {
            state.shown = false;
        }
        if install {
            self.save_for_update();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn prepare(app: &mut App, root: &std::path::Path) {
        app.ws.preferences.autosave = false;
        let id = app.ws.requests[0].id.clone();
        app.ws.requests[0].url = "https://example.test/unsaved-update-draft".into();
        app.modified_requests.insert(id);
        let asset = updates::Asset {
            name: "setup.exe".into(),
            url: String::new(),
            size: 1,
            sha256: "a".repeat(64),
        };
        app.workspaces.updates.release = Some(Release {
            version: "99.0.0".into(),
            notes_url: String::new(),
            installer: asset.clone(),
            checksums: asset,
        });
        app.workspaces.updates.download = Some(Download {
            release: app.workspaces.updates.release.clone().unwrap(),
            installer: root.join("setup.exe"),
        });
        app.workspaces.updates.installation = Some(Installation {
            directory: root.into(),
            executable: root.join("Reqon.exe"),
            version: "0.0.0".into(),
        });
        app.workspaces.updates.stage = Stage::Ready;
    }
    fn finish_saving(app: &mut App, ctx: &egui::Context) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while app.workspaces.updates.stage == Stage::Saving
            && app.workspaces.updates.handoff.lock().unwrap().is_none()
        {
            assert!(Instant::now() < deadline, "save barrier timed out");
            app.updates_tick(ctx);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn update_preserves_unsaved_drafts_before_handoff() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, directory.path().into()).unwrap();
        prepare(&mut app, directory.path());
        app.save_for_update();
        finish_saving(&mut app, &ctx);
        assert!(app.workspaces.updates.handoff.lock().unwrap().is_some());
        let saved: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.path().join("workspace.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            saved["requests"][0]["url"],
            "https://example.test/unsaved-update-draft"
        );
    }
    #[test]
    fn failed_save_never_hands_control_to_installer() {
        let directory = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = App::from_context(&ctx, directory.path().into()).unwrap();
        app.writer
            .barrier()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let workspace = directory.path().join("workspace.json");
        std::fs::remove_file(&workspace).unwrap();
        std::fs::create_dir(&workspace).unwrap();
        prepare(&mut app, directory.path());
        app.save_for_update();
        finish_saving(&mut app, &ctx);
        assert!(app.workspaces.updates.handoff.lock().unwrap().is_none());
        assert!(app.workspaces.updates.stage == Stage::Ready);
        assert!(
            app.workspaces
                .updates
                .message
                .contains("Could not finish saving")
        );
    }
}
