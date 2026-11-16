use crate::{
    app::App,
    auth_migration::{Operation, Outcome, Plan, Report},
};
use std::sync::mpsc::{Receiver, TryRecvError};

#[derive(Default)]
pub struct State {
    pending: Option<Receiver<Result<Outcome, String>>>,
    plan: Option<Plan>,
    report: Option<Report>,
    error: String,
    environments: bool,
}
pub fn draw(app: &mut App, ui: &mut eframe::egui::Ui, cid: &str) {
    let Some(root) = app
        .ws
        .collections
        .iter()
        .find(|c| c.id == cid)
        .map(|c| c.path.clone())
    else {
        return;
    };
    let state = app.auth_migrations.entry(cid.into()).or_default();
    if let Some(receiver) = &state.pending {
        match receiver.try_recv() {
            Ok(result) => {
                state.pending = None;
                match result {
                    Ok(Outcome::Preview(plan)) => {
                        state.report = Some(plan.report.clone());
                        state.plan = Some(plan);
                    }
                    Ok(Outcome::Applied(report)) => {
                        state.report = Some(report);
                        state.plan = None;
                    }
                    Err(error) => {
                        state.error = error;
                        state.plan = None;
                    }
                }
            }
            Err(TryRecvError::Disconnected) => {
                state.pending = None;
                state.error = "Migration worker disconnected".into();
            }
            Err(TryRecvError::Empty) => {}
        }
    }
    ui.heading("Protect saved secrets");
    ui.label("Scan saved requests and collection/folder settings for passwords, tokens and API keys. Values are stored locally with Windows DPAPI; shared files contain references.");
    ui.label("Git history and old backups are outside this migration.");
    ui.add_enabled_ui(state.pending.is_none(), |ui| {
        if ui
            .checkbox(
                &mut state.environments,
                "Include environment variables marked Secret",
            )
            .changed()
        {
            state.plan = None;
            state.report = None;
        }
    });
    ui.label("For existing local-only environment values, save the environment once to enable CLI access.");
    ui.add_space(8.);
    if ui
        .add_enabled(
            state.pending.is_none(),
            eframe::egui::Button::new("Scan saved secrets"),
        )
        .clicked()
    {
        state.plan = None;
        state.report = None;
        state.error.clear();
        state.pending = Some(
            app.writer
                .auth_migration(Operation::Scan(root, state.environments)),
        );
    }
    if state.pending.is_some() {
        ui.spinner();
        ui.label("Working on collection files…");
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
    if !state.error.is_empty() {
        ui.colored_label(app.theme.red, &state.error);
    }
    if let Some(report) = &state.report {
        ui.add_space(8.);
        ui.label(format!(
            "Scanned: {} files · To change: {} files · Secret values: {}",
            report.files_scanned, report.files_to_change, report.fields_to_protect
        ));
        if report.status == "preview" {
            if report.files_to_change == 0 {
                ui.label("No plaintext secrets found in the scanned fields.");
            }
            if report.yaml_files > 0 {
                ui.label("YAML files will be reformatted; YAML comments are not preserved. The encrypted backup keeps the original files.");
            }
            eframe::egui::ScrollArea::vertical()
                .max_height(150.)
                .show(ui, |ui| {
                    for path in &report.paths {
                        ui.label(path.display().to_string());
                    }
                });
            ui.label("An encrypted recovery backup is created before changing files. Files edited after this scan require a new scan.");
            if ui
                .add_enabled(
                    state.pending.is_none() && report.files_to_change > 0,
                    eframe::egui::Button::new("Protect scanned secrets"),
                )
                .clicked()
            {
                if let Some(plan) = state.plan.take() {
                    state.error.clear();
                    state.pending = Some(app.writer.auth_migration(Operation::Apply(plan)));
                }
            }
        } else if report.status == "protected" || report.status == "unchanged" {
            ui.label("Secret migration completed.");
            if let Some(id) = &report.backup_id {
                ui.label(format!("Recovery backup: {id}"));
            }
        }
    }
}
