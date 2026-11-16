use crate::{
    app::{Action, App, Dialog},
    preference_model::Keybindings,
    preferences::{State, hint, toggle},
    theme::Theme,
    widgets::{self as w},
};
use eframe::egui::{self, Key, Modifiers, vec2};
pub struct Binding {
    pub section: &'static str,
    pub id: &'static str,
    pub name: &'static str,
    pub keys: &'static str,
    pub readonly: bool,
}
include!("keybinding_defaults.rs");
pub fn value<'a>(p: &'a Keybindings, id: &str) -> &'a str {
    p.bindings.get(id).map(String::as_str).unwrap_or_else(|| {
        DEFAULTS
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.keys)
            .unwrap_or("")
    })
}
pub fn parse(value: &str) -> Option<(Modifiers, Key)> {
    let mut modifiers = Modifiers::NONE;
    let mut key = None;
    for piece in value.split('+') {
        match piece.to_ascii_lowercase().as_str() {
            "ctrl" => modifiers.ctrl = true,
            "shift" => modifiers.shift = true,
            "alt" => modifiers.alt = true,
            "win" => modifiers.mac_cmd = true,
            "" => {}
            other => {
                let name = match other {
                    "enter" => "Enter",
                    "esc" => "Escape",
                    "space" => "Space",
                    "[" => "OpenBracket",
                    "]" => "CloseBracket",
                    "\\" => "Backslash",
                    "," => "Comma",
                    "=" => "Equals",
                    "-" => "Minus",
                    "tab" => "Tab",
                    "escape" => "Escape",
                    "arrowleft" => "ArrowLeft",
                    "arrowright" => "ArrowRight",
                    other => other,
                };
                key = Key::from_name(name).or_else(|| Key::from_name(&name.to_uppercase()));
            }
        }
    }
    key.map(|k| (modifiers, k))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overrides_disable_actions_and_extra_modifiers_do_not_trigger_shorter_binding() {
        let ctx = egui::Context::default();
        let mut p = Keybindings::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::W,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            assert!(!pressed(ui.ctx(), &p, "closeTab"));
            assert!(pressed(ui.ctx(), &p, "closeAllTabs"));
        });
        output.textures_delta.clear();
        p.bindings.insert("sendRequest".into(), "Alt+Enter".into());
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::ALT,
            }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(input.clone(), |ui| {
            assert!(pressed(ui.ctx(), &p, "sendRequest"))
        });
        output.textures_delta.clear();
        p.enabled = false;
        let mut output = ctx.run_ui(input, |ui| assert!(!pressed(ui.ctx(), &p, "sendRequest")));
        output.textures_delta.clear();
        assert_eq!(parse("Ctrl+\\"), Some((Modifiers::CTRL, Key::Backslash)));
    }
}
pub fn pressed(ctx: &egui::Context, p: &Keybindings, id: &str) -> bool {
    p.enabled && parse(value(p, id)).is_some_and(|(m, k)| consume(ctx, m, k))
}
fn consume(ctx: &egui::Context, m: Modifiers, k: Key) -> bool {
    ctx.input_mut(|input| {
        let mut pressed = false;
        input.events.retain(|event| {
            let matched = matches!(event, egui::Event::Key { key, pressed: true, modifiers, .. }
                if *key == k && modifiers.ctrl == m.ctrl && modifiers.alt == m.alt && modifiers.shift == m.shift && modifiers.mac_cmd == m.mac_cmd);
            pressed |= matched; !matched
        });
        pressed
    })
}
fn format(m: Modifiers, k: Key) -> String {
    let mut parts = vec![];
    if m.ctrl {
        parts.push("Ctrl".into());
    }
    if m.alt {
        parts.push("Alt".into());
    }
    if m.shift {
        parts.push("Shift".into());
    }
    parts.push(k.name().to_owned());
    parts.join("+")
}
pub fn settings(ui: &mut egui::Ui, p: &mut Keybindings, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    let origin = ui.cursor().min;
    let width = ui.available_width();
    let mut controls = w::child(
        ui,
        egui::Rect::from_min_size(origin + vec2((width - 154.).max(0.), -34.), vec2(154., 28.)),
        "keybinding-controls",
    );
    controls.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.;
        changed |= toggle(ui, &mut p.enabled, t);
        if ui
            .add_enabled(
                !p.bindings.is_empty(),
                egui::Button::new(egui::RichText::new("Reset Default").size(12.)),
            )
            .clicked()
        {
            p.bindings.clear();
            changed = true;
        }
    });
    ui.spacing_mut().item_spacing.y = 0.;
    let header = ui.allocate_space(vec2(width, 32.)).1;
    ui.painter().rect_stroke(
        header,
        4,
        egui::Stroke::new(1., t.border),
        egui::StrokeKind::Inside,
    );
    w::label(
        ui,
        header.left_center() + vec2(11., 0.),
        "Command",
        13.,
        t.text,
    );
    w::label(
        ui,
        header.left_center() + vec2(377., 0.),
        "Keybinding",
        13.,
        t.text,
    );
    let mut section = "";
    for b in DEFAULTS {
        if b.section != section {
            if !section.is_empty() {
                ui.add_space(8.);
            }
            section = b.section;
            let row = ui.allocate_space(vec2(width, 32.)).1;
            ui.painter().rect_filled(row, 0, t.surface);
            w::semibold(ui, row.left_center() + vec2(11., 0.), section, 12., t.muted);
        }
        let row = ui.allocate_space(vec2(width, 30.)).1;
        w::line(ui, row.left_top(), row.left_bottom(), t.border);
        w::line(ui, row.right_top(), row.right_bottom(), t.border);
        let response = ui.interact(row, ui.scope_id().with(b.id), egui::Sense::click());
        if response.hovered() {
            ui.painter().rect_filled(row.shrink(1.), 0, t.hover);
        }
        w::label(ui, row.left_center() + vec2(11., 0.), b.name, 13., t.text);
        let keys = value(p, b.id).to_owned();
        let mut x = row.left() + 377.;
        if keys.is_empty() {
            w::label(
                ui,
                egui::pos2(x, row.center().y),
                "Unassigned",
                11.,
                t.faint,
            );
        }
        let range = keys.replace(" - ", "+–+");
        for key in range.split('+').filter(|s| !s.is_empty()) {
            let label = if key.len() == 1 {
                key.to_uppercase()
            } else if key == "enter" {
                "Enter".into()
            } else if key == "space" {
                "Space".into()
            } else {
                key.to_owned()
            };
            let width = w::text_width(ui, &label, 10.) + 12.;
            let cap = w::rect(x, row.center().y - 10., width, 20.);
            ui.painter().rect(
                cap,
                3,
                t.bg,
                egui::Stroke::new(1., t.strong_border),
                egui::StrokeKind::Inside,
            );
            w::label(
                ui,
                cap.left_center() + vec2(6., 0.),
                &label,
                10.,
                if p.enabled { t.text } else { t.faint },
            );
            x += width + 4.;
        }
        if !b.readonly && p.enabled {
            if response.hovered() {
                crate::menu::icon(
                    ui,
                    "Edit",
                    egui::pos2(row.right() - 20., row.center().y),
                    t.muted,
                    14.,
                );
            }
            if response.clicked() {
                state.key_capture = Some(b.id.into());
                state.key_draft = keys;
                state.error.clear();
            }
        }
    }
    let end = ui.cursor().min;
    w::line(ui, end, end + vec2(width, 0.), t.border);
    if let Some(id) = state.key_capture.clone() {
        let mut close = false;
        let mut save = false;
        let events = ui.input(|i| i.events.clone());
        for event in events {
            if let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                repeat: false,
                ..
            } = event
            {
                if !matches!(key, Key::Escape | Key::Enter)
                    || modifiers.ctrl
                    || modifiers.alt
                    || modifiers.shift
                {
                    state.key_draft = format(modifiers, key);
                    state.error.clear();
                }
            }
        }
        let (_, dismiss) = w::modal(ui.ctx(), "Edit Keybinding", 440., t, |ui| {
            ui.label(
                DEFAULTS
                    .iter()
                    .find(|b| b.id == id)
                    .map(|b| b.name)
                    .unwrap_or("Shortcut"),
            );
            hint(ui, "Press your desired key combination", t);
            ui.add_sized(
                vec2(400., 42.),
                egui::Label::new(egui::RichText::new(&state.key_draft).monospace()),
            );
            if !state.error.is_empty() {
                ui.colored_label(t.red, &state.error);
            }
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    let combo = parse(&state.key_draft);
                    if combo.is_none_or(|(m, k)| {
                        !m.ctrl
                            && !m.alt
                            && !m.shift
                            && !matches!(
                                k,
                                Key::F1
                                    | Key::F2
                                    | Key::F3
                                    | Key::F4
                                    | Key::F5
                                    | Key::F6
                                    | Key::F7
                                    | Key::F8
                                    | Key::F9
                                    | Key::F10
                                    | Key::F11
                                    | Key::F12
                            )
                    }) {
                        state.error = "Use a modifier key or F1–F12".into();
                    } else if let Some(other) = DEFAULTS
                        .iter()
                        .find(|b| b.id != id && parse(value(p, b.id)) == combo)
                    {
                        state.error = format!("Already assigned to {}", other.name);
                    } else {
                        save = true;
                    }
                }
                if ui.button("Remove").clicked() {
                    state.key_draft.clear();
                    save = true;
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if save {
            p.bindings.insert(id, state.key_draft.clone());
            changed = true;
        }
        if save || close || dismiss {
            state.key_capture = None;
        }
    }
    changed
}
impl App {
    pub fn configurable_shortcuts(&mut self, ctx: &egui::Context) {
        if self.preference_ui.key_capture.is_some() {
            return;
        }
        let p = self.ws.preferences.keybindings.clone();
        if !p.enabled {
            return;
        }
        let pressed: Vec<_> = DEFAULTS
            .iter()
            .filter(|b| {
                !b.readonly
                    && !(ctx.egui_wants_keyboard_input()
                        && matches!(b.id, "copyItem" | "pasteItem" | "cloneItem" | "renameItem"))
                    && !(b.id == "collapseSidebar"
                        && ctx.egui_wants_keyboard_input()
                        && self.ws.preferences.ai.enabled
                        && self.ws.preferences.ai.autocomplete)
                    && pressed(ctx, &p, b.id)
            })
            .map(|b| b.id)
            .collect();
        for action in pressed {
            match action {
                "closeTab" => {
                    if let Some(id) = self.ws.active.clone() {
                        self.actions.push(Action::Close(id));
                    }
                }
                "closeAllTabs" => {
                    self.preference_ui.closing_tabs = self.ws.open_tabs.clone().into();
                }
                "save" => self.actions.push(Action::Save),
                "saveAllTabs" => {
                    let active = self.ws.active.clone();
                    for id in self.ws.open_tabs.clone() {
                        self.ws.active = Some(id);
                        self.save_active();
                    }
                    self.ws.active = active;
                }
                "reopenLastClosedTab" => {
                    if let Some(id) = self.preference_ui.closed_tabs.pop() {
                        self.open(id);
                    }
                }
                "switchToLastTab" => {
                    if let Some(id) = self.ws.open_tabs.last().cloned() {
                        self.open(id);
                    }
                }
                "switchToPreviousTab" | "switchToNextTab" | "moveTabLeft" | "moveTabRight" => {
                    let n = self.ws.open_tabs.len();
                    if n > 0 {
                        let index = self
                            .ws
                            .active
                            .as_ref()
                            .and_then(|id| self.ws.open_tabs.iter().position(|x| x == id))
                            .unwrap_or(0);
                        let back = action == "switchToPreviousTab" || action == "moveTabLeft";
                        let target = if back {
                            (index + n - 1) % n
                        } else {
                            (index + 1) % n
                        };
                        if action.starts_with("move") {
                            self.ws.open_tabs.swap(index, target);
                            self.mark_workspace_changed();
                        } else {
                            self.open(self.ws.open_tabs[target].clone());
                        }
                    }
                }
                "sidebarSearch" | "globalSearch" => {
                    self.search_open = true;
                    self.sidebar_visible = true;
                    self.sidebar_panel = crate::navigation::Panel::Collections;
                }
                "collapseSidebar" => self.sidebar_visible = !self.sidebar_visible,
                "sendRequest" => self.actions.push(Action::Send),
                "triggerAutocomplete" => ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::unique("editor-completion-trigger"), true);
                }),
                "changeLayout" => self.actions.push(Action::ToggleSplit),
                "prettify" => {
                    if let Some(id) = self.ws.active.clone() {
                        if let Some(r) = self.ws.requests.iter_mut().find(|r| r.id == id) {
                            match crate::body_ui::format_json(&r.body) {
                                Ok(body) => {
                                    r.body = body;
                                    self.mark_changed();
                                }
                                Err(e) => self.notice(e.to_string()),
                            }
                        }
                    }
                }
                "importCollection" => self.actions.push(Action::ImportPostman),
                "editEnvironment" => self.show_dialog(Dialog::Environments),
                "newRequest" => self.actions.push(Action::NewRequest(
                    self.active_collection_id(),
                    String::new(),
                )),
                "openPreferences" => self.show_dialog(Dialog::Settings),
                "zoomIn" => {
                    self.ws.preferences.appearance.zoom =
                        (self.ws.preferences.appearance.zoom + 10).min(150);
                    self.mark_changed();
                }
                "zoomOut" => {
                    self.ws.preferences.appearance.zoom = self
                        .ws
                        .preferences
                        .appearance
                        .zoom
                        .saturating_sub(10)
                        .max(50);
                    self.mark_changed();
                }
                "resetZoom" => {
                    self.ws.preferences.appearance.zoom = 100;
                    self.mark_changed();
                }
                "openTerminal" => {
                    let cid = self.active_collection_id();
                    let path = self
                        .ws
                        .collections
                        .iter()
                        .find(|c| c.id == cid)
                        .map(|c| c.path.clone())
                        .unwrap_or_else(|| self.root.clone());
                    self.open_terminal(path);
                }
                "closeBruno" => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                "pasteItem" => {
                    if let Some(copied) = self.tree_menu.copied.clone() {
                        let (collection, parent) = match self.tree_menu.selected.as_ref() {
                            Some(crate::tree_ops::Subject::Folder { collection, folder }) => {
                                (collection.clone(), folder.clone())
                            }
                            Some(crate::tree_ops::Subject::Request(id)) => self
                                .ws
                                .requests
                                .iter()
                                .find(|r| &r.id == id)
                                .map(|r| (r.collection.clone(), r.folder.clone()))
                                .unwrap_or_else(|| (self.active_collection_id(), String::new())),
                            None => (self.active_collection_id(), String::new()),
                        };
                        self.start_tree_operation(crate::tree_ops::Operation::Paste {
                            collection,
                            parent,
                            copied,
                        });
                    }
                }
                "cloneItem" | "renameItem" | "copyItem" => {
                    if let Some(target) = self.tree_menu.selected.clone() {
                        if !matches!(&target,crate::tree_ops::Subject::Folder {folder,..} if folder.is_empty())
                        {
                            let command = match action {
                                "cloneItem" => crate::tree_menu::Command::Clone,
                                "renameItem" => crate::tree_menu::Command::Rename,
                                _ => crate::tree_menu::Command::Copy,
                            };
                            self.actions.push(Action::TreeItem(target, command));
                            continue;
                        }
                    }
                    if !ctx.egui_wants_keyboard_input() {
                        if let Some(id) = self.ws.active.clone() {
                            if self.ws.requests.iter().any(|r| r.id == id) {
                                let command = match action {
                                    "cloneItem" => crate::tree_menu::Command::Clone,
                                    "renameItem" => crate::tree_menu::Command::Rename,
                                    _ => crate::tree_menu::Command::Copy,
                                };
                                self.actions.push(Action::TreeItem(
                                    crate::tree_ops::Subject::Request(id),
                                    command,
                                ));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        for (key, index) in [
            (Key::Num1, 0),
            (Key::Num2, 1),
            (Key::Num3, 2),
            (Key::Num4, 3),
            (Key::Num5, 4),
            (Key::Num6, 5),
            (Key::Num7, 6),
            (Key::Num8, 7),
        ] {
            if consume(ctx, Modifiers::CTRL, key) {
                if let Some(id) = self.ws.open_tabs.get(index).cloned() {
                    self.open(id);
                }
            }
        }
    }
}
