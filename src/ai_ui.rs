use crate::{
    app::App,
    model::{Preferences, Request},
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, vec2};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub request: String,
    pub prompt: String,
    pub target: usize,
    pub output: String,
    pub error: String,
    pub job: Option<mpsc::Receiver<Result<String, String>>>,
}
#[derive(Default)]
pub struct Completion {
    key: String,
    changed_at: Option<Instant>,
    job: Option<mpsc::Receiver<(String, usize, Result<String, String>)>>,
    last_sent: String,
}
#[derive(Clone)]
pub struct Cursor {
    pub index: usize,
    pub focused: bool,
}
#[derive(Clone)]
pub struct Ghost {
    pub fingerprint: String,
    pub cursor: usize,
    pub text: String,
}
pub fn field<'a>(r: &'a mut Request, target: usize) -> &'a mut String {
    match target {
        0 => &mut r.pre_script,
        1 => &mut r.post_script,
        2 => &mut r.tests,
        _ => &mut r.docs,
    }
}
pub fn field_ref(r: &Request, target: usize) -> &str {
    match target {
        0 => &r.pre_script,
        1 => &r.post_script,
        2 => &r.tests,
        _ => &r.docs,
    }
}
fn clean(value: String) -> String {
    let s = value.trim();
    if let Some(rest) = s.strip_prefix("```") {
        rest.split_once('\n')
            .map(|(_, s)| s.trim_end_matches("```").trim().to_owned())
            .unwrap_or(value)
    } else {
        value
    }
}
pub fn script_editor(
    ui: &mut egui::Ui,
    r: &mut Request,
    target: usize,
    p: &Preferences,
    state: &mut Completion,
    t: Theme,
) -> bool {
    let id = ui.scope_id().with("source-editor");
    let ghost_id = id.with("ai-ghost");
    if !p.ai.enabled || !p.ai.autocomplete {
        let changed = w::editor(ui, field(r, target), false, t);
        let trigger = ui
            .ctx()
            .data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("editor-completion-trigger")))
            .unwrap_or(false);
        if trigger {
            if let Some(cursor) = ui
                .ctx()
                .data_mut(|d| d.get_temp::<Cursor>(id.with("ai-cursor")))
                .filter(|c| c.focused)
            {
                let source = field_ref(r, target);
                let prefix: String = source.chars().take(cursor.index).collect();
                let word = prefix
                    .rsplit(|c: char| !c.is_alphanumeric() && c != '.' && c != '_')
                    .next()
                    .unwrap_or("");
                if let Some(candidate) = [
                    "bru.getVar(",
                    "bru.setVar(",
                    "bru.getEnvVar(",
                    "bru.setEnvVar(",
                    "bru.getCollectionVar(",
                    "req.getUrl()",
                    "req.setHeader(",
                    "res.getBody()",
                    "res.getStatus()",
                    "expect(",
                    "test(",
                ]
                .into_iter()
                .find(|c| c.starts_with(word) && c.len() > word.len())
                {
                    ui.ctx().data_mut(|d| {
                        d.insert_temp(
                            ghost_id,
                            Ghost {
                                fingerprint: crate::disk_sync::fingerprint(source),
                                cursor: cursor.index,
                                text: candidate[word.len()..].into(),
                            },
                        );
                    });
                }
            }
        }
        return changed;
    }
    let manual = ui.memory(|m| m.has_focus(id))
        && (ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Backslash))
            || ui
                .ctx()
                .data_mut(|d| d.remove_temp::<bool>(egui::Id::unique("editor-completion-trigger")))
                .unwrap_or(false));
    let changed = w::editor(ui, field(r, target), false, t);
    let source = field_ref(r, target);
    let hash = crate::disk_sync::fingerprint(source);
    let cursor = ui
        .ctx()
        .data_mut(|d| d.get_temp::<Cursor>(id.with("ai-cursor")));
    let index = cursor.as_ref().map(|c| c.index).unwrap_or(0);
    let current = format!("{hash}:{index}:{target}");
    if current != state.key {
        state.key = current.clone();
        state.changed_at = changed.then(Instant::now);
        ui.ctx().data_mut(|d| {
            if d.get_temp::<Ghost>(ghost_id)
                .is_none_or(|g| g.fingerprint != hash || g.cursor != index)
            {
                d.remove::<Ghost>(ghost_id);
            }
        });
    }
    if let Some(rx) = &state.job {
        match rx.try_recv() {
            Ok((key, cursor, result)) => {
                state.job = None;
                if key == current {
                    if let Ok(text) = result {
                        let text = clean(text);
                        if !text.is_empty() {
                            ui.ctx().data_mut(|d| {
                                d.insert_temp(
                                    ghost_id,
                                    Ghost {
                                        fingerprint: hash.clone(),
                                        cursor,
                                        text,
                                    },
                                )
                            });
                        }
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => state.job = None,
            _ => {}
        }
    }
    let wait = if p.ai.trigger == "aggressive" {
        Duration::from_millis(150)
    } else {
        Duration::from_millis(650)
    };
    let focused = cursor.as_ref().is_some_and(|c| c.focused);
    let ready = manual
        || p.ai.trigger != "manual"
            && state.changed_at.is_some_and(|at| at.elapsed() >= wait)
            && state.last_sent != current;
    let ghost_visible = ui.ctx().data(|d| d.get_temp::<Ghost>(ghost_id).is_some());
    if ready && focused && state.job.is_none() && !source.is_empty() && (!ghost_visible || manual) {
        let offset = source
            .char_indices()
            .nth(index)
            .map(|(i, _)| i)
            .unwrap_or(source.len());
        let mut prefix = source[..offset].to_owned();
        let mut suffix = source[offset..].to_owned();
        for value in p.credentials.values().filter(|v| !v.is_empty()) {
            prefix = prefix.replace(value, "<redacted>");
            suffix = suffix.replace(value, "<redacted>");
        }
        let prompt = format!(
            "Complete this JavaScript at <CURSOR>. Return only the text to insert, without repeating the prefix or suffix. Prefer a short completion.\n{prefix}<CURSOR>{suffix}"
        );
        let context = crate::integrations::context(p, r, None, &Default::default());
        let p = p.clone();
        let key = current.clone();
        let (tx, rx) = mpsc::channel();
        let ctx = ui.ctx().clone();
        state.job = Some(rx);
        state.last_sent = current;
        std::thread::spawn(move || {
            let result =
                crate::integrations::generate(&p, &p.ai.autocomplete_model, &prompt, &context)
                    .map_err(|e| format!("{e:#}"));
            let _ = tx.send((key, index, result));
            ctx.request_repaint();
        });
    } else if focused
        && state.changed_at.is_some()
        && state.last_sent != current
        && p.ai.trigger != "manual"
    {
        ui.ctx().request_repaint_after(wait);
    }
    changed
}
impl App {
    pub fn ai_preferences(&self, request: &Request) -> Preferences {
        let mut preferences = self.ws.preferences.clone();
        let mut index = 0;
        let mut add = |pair: &crate::model::Pair| {
            if pair.secret && !pair.value.is_empty() {
                preferences
                    .credentials
                    .insert(format!("redact:{index}"), pair.value.clone());
                index += 1;
            }
        };
        self.ws.globals.iter().for_each(&mut add);
        for environment in &self.ws.environments {
            environment.vars.iter().for_each(&mut add);
        }
        if let Some(collection) = self
            .ws
            .collections
            .iter()
            .find(|c| c.id == request.collection)
        {
            collection.vars.iter().for_each(&mut add);
            for environment in &collection.environments {
                environment.vars.iter().for_each(&mut add);
            }
        }
        request.vars.iter().for_each(&mut add);
        preferences
    }
    pub fn ai_button(&mut self, ui: &mut egui::Ui, r: egui::Rect, request: &Request) {
        if self.ws.preferences.ai.enabled
            && w::icon_button(
                ui,
                rect(r.right() - 28., r.center().y - 14., 28., 28.),
                Icon::Sparkles,
                "AI Assistant",
                self.theme,
            )
            .clicked()
        {
            self.ai_ui.open = true;
            self.ai_ui.request = request.id.clone();
            self.ai_ui.error.clear();
        }
    }
    pub fn ai_dialog(&mut self, ctx: &egui::Context) {
        if !self.ai_ui.open {
            return;
        }
        if let Some(rx) = &self.ai_ui.job {
            match rx.try_recv() {
                Ok(result) => {
                    self.ai_ui.job = None;
                    match result {
                        Ok(text) => self.ai_ui.output = clean(text),
                        Err(error) => self.ai_ui.error = error,
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ai_ui.job = None;
                    self.ai_ui.error = "Generation stopped".into();
                }
                _ => {}
            }
        }
        let Some(request) = self
            .ws
            .requests
            .iter()
            .find(|r| r.id == self.ai_ui.request)
            .cloned()
        else {
            self.ai_ui.open = false;
            return;
        };
        let t = self.theme;
        let mut apply = false;
        let (_, close) = w::modal(ctx, "AI Assistant", 700., t, |ui| {
            ui.horizontal(|ui| {
                for (index, label) in ["Pre-Request", "Post-Response", "Tests", "Documentation"]
                    .iter()
                    .enumerate()
                {
                    ui.selectable_value(&mut self.ai_ui.target, index, *label);
                }
            });
            ui.add_sized(
                vec2(660., 80.),
                egui::TextEdit::multiline(&mut self.ai_ui.prompt)
                    .hint_text("Describe what to generate…"),
            );
            if ui
                .add_enabled(
                    self.ai_ui.job.is_none() && !self.ai_ui.prompt.trim().is_empty(),
                    egui::Button::new(if self.ai_ui.job.is_some() {
                        "Generating…"
                    } else {
                        "Generate"
                    }),
                )
                .clicked()
            {
                let p = self.ai_preferences(&request);
                let context = crate::integrations::context(
                    &p,
                    &request,
                    self.tabs.get(&request.id).and_then(|s| s.reply.as_ref()),
                    &self.vars(&request),
                );
                let prompt = format!(
                    "Generate {} for this request. {}",
                    [
                        "a pre-request script",
                        "a post-response script",
                        "tests",
                        "Markdown documentation"
                    ][self.ai_ui.target],
                    self.ai_ui.prompt
                );
                let (tx, rx) = mpsc::channel();
                self.ai_ui.job = Some(rx);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result =
                        crate::integrations::generate(&p, &p.ai.default_model, &prompt, &context)
                            .map_err(|e| format!("{e:#}"));
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            }
            if !self.ai_ui.error.is_empty() {
                ui.colored_label(t.red, &self.ai_ui.error);
            }
            if !self.ai_ui.output.is_empty() {
                egui::ScrollArea::vertical()
                    .max_height(350.)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.ai_ui.output)
                                .font(egui::FontId::monospace(13.))
                                .desired_width(660.),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(self.ai_ui.output.clone());
                    }
                    if ui.button("Apply to request").clicked() {
                        apply = true;
                    }
                });
            }
        });
        if close {
            self.ai_ui.open = false;
        }
        if apply {
            if let Some(r) = self
                .ws
                .requests
                .iter_mut()
                .find(|r| r.id == self.ai_ui.request)
            {
                *field(r, self.ai_ui.target) = self.ai_ui.output.clone();
                self.mark_changed();
            }
            self.ai_ui.open = false;
        }
    }
}
