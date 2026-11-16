use crate::{
    model::Preferences,
    preference_model::*,
    preferences::*,
    theme::Theme,
    widgets::{self as w, Icon},
};
use eframe::egui::{self, Stroke, TextEdit, vec2};
fn card(ui: &mut egui::Ui, t: Theme, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .stroke(Stroke::new(1., t.border))
        .corner_radius(8)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            body(ui);
        });
}
fn switch_card(ui: &mut egui::Ui, name: &str, desc: &str, on: &mut bool, t: Theme) -> bool {
    let mut changed = false;
    card(ui, t, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.strong(name);
                hint(ui, desc, t);
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                changed = toggle(ui, on, t);
            });
        });
    });
    changed
}
fn available_models(p: &Preferences) -> Vec<(String, String)> {
    p.ai.providers
        .iter()
        .filter(|x| x.enabled)
        .flat_map(|provider| {
            provider
                .models
                .iter()
                .filter(|m| m.enabled)
                .map(move |m| (format!("{}:{}", provider.id, m.id), m.label.clone()))
        })
        .collect()
}
fn model_select(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    models: &[(String, String)],
    auto: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(label)
            .selected_text(
                models
                    .iter()
                    .find(|(id, _)| id == value)
                    .map(|(_, name)| name.as_str())
                    .unwrap_or(auto),
            )
            .width(250.)
            .show_ui(ui, |ui| {
                changed |= ui.selectable_value(value, String::new(), auto).changed();
                for (id, name) in models {
                    changed |= ui.selectable_value(value, id.clone(), name).changed();
                }
            });
    });
    changed
}
pub fn ai(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    let tabs = ui.allocate_space(vec2(ui.available_width(), 26.)).1;
    w::line(ui, tabs.left_bottom(), tabs.right_bottom(), t.border);
    let mut x = tabs.left();
    for (index, (name, icon)) in [
        ("Configuration", Icon::Settings),
        ("Autocomplete", Icon::TerminalBox),
        ("Security", Icon::Shield),
    ]
    .into_iter()
    .enumerate()
    {
        let width = w::text_width(ui, name, 12.) + 32.;
        let r = w::rect(x, tabs.top(), width, 26.);
        w::icon(ui, egui::pos2(x + 8., r.top() + 10.), icon, t.muted, 14.);
        w::label(
            ui,
            egui::pos2(x + 22., r.top() + 10.),
            name,
            12.,
            if state.ai_tab == index {
                t.text
            } else {
                t.muted
            },
        );
        if state.ai_tab == index {
            ui.painter()
                .hline(r.x_range(), r.bottom(), egui::Stroke::new(1., t.accent));
        }
        if ui
            .interact(
                r,
                ui.scope_id().with(("ai-tab", index)),
                egui::Sense::click(),
            )
            .clicked()
        {
            state.ai_tab = index;
        }
        x += width + 16.;
    }
    ui.add_space(6.);
    match state.ai_tab {
        0 => {
            let r = ui.allocate_space(vec2(ui.available_width(), 46.)).1;
            ui.painter()
                .rect_stroke(r, 8, Stroke::new(1., t.border), egui::StrokeKind::Inside);
            w::icon(
                ui,
                egui::pos2(r.left() + 22., r.center().y),
                Icon::Sparkles,
                t.muted,
                16.,
            );
            w::semibold(
                ui,
                egui::pos2(r.left() + 40., r.top() + 15.),
                "AI Features",
                13.,
                t.text,
            );
            w::label(
                ui,
                egui::pos2(r.left() + 40., r.top() + 32.),
                "Turn on to configure providers and models. Your keys stay local.",
                11.,
                t.muted,
            );
            let mut toggle_ui = w::child(
                ui,
                w::rect(r.right() - 54., r.top() + 13., 40., 20.),
                "ai-master-toggle",
            );
            changed |= toggle(&mut toggle_ui, &mut p.ai.enabled, t);
            ui.add_space(8.);
            card(ui, t, |ui| {
                hint(
                    ui,
                    "Bring your own API key. Reqon talks to providers directly. Keys are stored locally and sent only to the selected provider.",
                    t,
                )
            });
            if p.ai.enabled {
                ui.add_space(8.);
                let mut remove = None;
                for provider in &mut p.ai.providers {
                    ui.push_id(provider.id.clone(), |ui| {
                        card(ui, t, |ui| {
                            let id = ui.scope_id().with("expanded");
                            let mut open = ui
                                .ctx()
                                .data_mut(|d| d.get_temp::<bool>(id))
                                .unwrap_or(provider.enabled);
                            ui.horizontal(|ui| {
                                if ui.selectable_label(open, &provider.name).clicked() {
                                    open = !open;
                                }
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if toggle(ui, &mut provider.enabled, t) {
                                            open = provider.enabled;
                                            changed = true;
                                        }
                                        hint(
                                            ui,
                                            if p.credentials
                                                .get(&format!("ai:{}", provider.id))
                                                .is_some_and(|k| !k.is_empty())
                                            {
                                                "Configured"
                                            } else {
                                                "Not configured"
                                            },
                                            t,
                                        );
                                    },
                                );
                            });
                            ui.ctx().data_mut(|d| d.insert_temp(id, open));
                            if open {
                                let custom =
                                    !matches!(provider.id.as_str(), "openai" | "anthropic");
                                if custom {
                                    changed |= input(ui, "Name", &mut provider.name, 300., false);
                                    changed |=
                                        input(ui, "Base URL", &mut provider.base_url, 440., false);
                                }
                                let key = p
                                    .credentials
                                    .entry(format!("ai:{}", provider.id))
                                    .or_default();
                                changed |= secret_input(ui, "API Key", key, 420., t);
                                ui.horizontal(|ui| {
                                    if primary(ui, "Save", t).clicked() {
                                        changed = true;
                                        state
                                            .status
                                            .insert(provider.id.clone(), "API key saved".into());
                                    }
                                    if ui.button("Test Connection").clicked() {
                                        let prov = provider.clone();
                                        let key = key.clone();
                                        state
                                            .status
                                            .insert(prov.id.clone(), "Testing connection…".into());
                                        state.job(ui.ctx(), move || {
                                            let id = prov.id.clone();
                                            JobResult::Models(
                                                id,
                                                crate::integrations::ai_models(&prov, &key)
                                                    .map_err(|e| format!("{e:#}")),
                                            )
                                        });
                                    }
                                    if ui.button("Remove Key").clicked() {
                                        key.clear();
                                        changed = true;
                                    }
                                    if custom && ui.button("Delete Endpoint").clicked() {
                                        remove = Some(provider.id.clone());
                                    }
                                });
                                if let Some(text) = state.status.get(&provider.id) {
                                    hint(ui, text, t);
                                }
                                separator(ui);
                                ui.label("Models");
                                let mut remove_model = None;
                                for (index, model) in provider.models.iter_mut().enumerate() {
                                    ui.push_id(index, |ui| {
                                        ui.horizontal(|ui| {
                                            changed |= w::checkbox(
                                                ui,
                                                &mut model.enabled,
                                                &model.label,
                                                t,
                                            );
                                            if custom && ui.small_button("×").clicked() {
                                                remove_model = Some(index);
                                            }
                                        });
                                    });
                                }
                                if let Some(index) = remove_model {
                                    provider.models.remove(index);
                                    changed = true;
                                }
                                if custom {
                                    let draft_id = ui.scope_id().with("model-draft");
                                    let mut draft = ui
                                        .ctx()
                                        .data_mut(|d| d.get_temp::<String>(draft_id))
                                        .unwrap_or_default();
                                    ui.horizontal(|ui| {
                                        ui.add(
                                            TextEdit::singleline(&mut draft).hint_text("Model ID"),
                                        );
                                        if ui
                                            .add_enabled(
                                                !draft.trim().is_empty(),
                                                egui::Button::new("+ Add Model"),
                                            )
                                            .clicked()
                                        {
                                            if !provider.models.iter().any(|m| m.id == draft.trim())
                                            {
                                                provider.models.push(AiModel {
                                                    id: draft.trim().into(),
                                                    label: draft.trim().into(),
                                                    enabled: true,
                                                });
                                                changed = true;
                                            }
                                            draft.clear();
                                        }
                                    });
                                    ui.ctx().data_mut(|d| d.insert_temp(draft_id, draft));
                                }
                            }
                        });
                        ui.add_space(8.);
                    });
                }
                if let Some(id) = remove {
                    p.ai.providers.retain(|p| p.id != id);
                    p.credentials.remove(&format!("ai:{id}"));
                    changed = true;
                }
                if ui.button("+ Add OpenAI-compatible endpoint").clicked() {
                    p.ai.providers.push(AiProvider {
                        name: "OpenAI-compatible".into(),
                        enabled: true,
                        ..Default::default()
                    });
                    changed = true;
                }
                separator(ui);
                let models = available_models(p);
                changed |= model_select(
                    ui,
                    "Default Model",
                    &mut p.ai.default_model,
                    &models,
                    "Auto",
                );
            }
        }
        1 => {
            if !p.ai.enabled {
                card(ui, t, |ui| {
                    hint(
                        ui,
                        "Turn on AI in the Configuration tab to configure autocomplete.",
                        t,
                    )
                });
                return changed;
            }
            changed |= switch_card(
                ui,
                "Inline Autocomplete",
                "Ghost-text suggestions in Pre-Request, Post-Response, and Tests scripts",
                &mut p.ai.autocomplete,
                t,
            );
            ui.add_space(8.);
            let models = available_models(p);
            card(ui, t, |ui| {
                if models.is_empty() {
                    hint(
                        ui,
                        "Add a provider API key and enable a model in the Configuration tab to enable autocomplete.",
                        t,
                    );
                }
                ui.add_enabled_ui(p.ai.autocomplete && !models.is_empty(), |ui| {
                    changed |= model_select(
                        ui,
                        "Model",
                        &mut p.ai.autocomplete_model,
                        &models,
                        "Auto (fastest available)",
                    );
                    ui.horizontal(|ui| {
                        ui.label("Trigger");
                        for (id, label) in [
                            ("aggressive", "Aggressive"),
                            ("debounced", "Debounced"),
                            ("manual", "Manual"),
                        ] {
                            changed |= ui
                                .selectable_value(&mut p.ai.trigger, id.into(), label)
                                .changed();
                        }
                    });
                });
            });
            ui.add_space(10.);
            for (key, label) in [
                ("Tab", "Accept suggestion"),
                ("Ctrl + Right", "Accept next word"),
                ("Esc", "Dismiss suggestion"),
                (r"Ctrl + \", "Trigger suggestion"),
            ] {
                row_text(ui, key, label, t);
            }
        }
        _ => {
            if !p.ai.enabled {
                card(ui, t, |ui| {
                    hint(
                        ui,
                        "Turn on AI in the Configuration tab to configure redaction.",
                        t,
                    )
                });
                return changed;
            }
            card(ui, t, |ui| {
                hint(
                    ui,
                    "Sensitive data is automatically redacted before context is sent to AI providers. Turn off protections if needed, or add custom headers and variables to redact.",
                    t,
                )
            });
            for (name, desc, value) in [
                (
                    "Redact sensitive header values",
                    "Masks Authorization, cookies, API keys and other credential-bearing headers.",
                    &mut p.ai.redact_headers,
                ),
                (
                    "Redact sensitive body keys",
                    "Masks passwords, tokens and secrets in JSON and GraphQL variables.",
                    &mut p.ai.redact_body,
                ),
                (
                    "Redact response values",
                    "Masks sensitive values in the response body and headers.",
                    &mut p.ai.redact_response,
                ),
                (
                    "Redact sensitive variables",
                    "Masks credentials in environment, collection, and runtime variables.",
                    &mut p.ai.redact_variables,
                ),
            ] {
                ui.add_space(4.);
                changed |= switch_card(ui, name, desc, value, t);
            }
            separator(ui);
            changed |= chips(ui, "Custom redacted headers", &mut p.ai.headers, t);
            separator(ui);
            changed |= chips(ui, "Custom redacted variables", &mut p.ai.variables, t);
            hint(ui, "Variables marked as secret are always redacted.", t);
        }
    }
    changed
}
fn chips(ui: &mut egui::Ui, label: &str, values: &mut Vec<String>, t: Theme) -> bool {
    let mut changed = false;
    title(ui, label, t);
    let id = ui.scope_id().with(label);
    let mut draft = ui
        .ctx()
        .data_mut(|d| d.get_temp::<String>(id))
        .unwrap_or_default();
    ui.horizontal(|ui| {
        ui.add_sized(
            vec2(320., 32.),
            TextEdit::singleline(&mut draft).char_limit(200),
        );
        if ui
            .add_enabled(
                !draft.trim().is_empty() && values.len() < 200,
                egui::Button::new("+ Add"),
            )
            .clicked()
        {
            if !values.iter().any(|v| v.eq_ignore_ascii_case(draft.trim())) {
                values.push(draft.trim().into());
                changed = true;
            }
            draft.clear();
        }
    });
    let mut remove = None;
    ui.horizontal_wrapped(|ui| {
        for (index, value) in values.iter().enumerate() {
            if ui.button(format!("{value}  ×")).clicked() {
                remove = Some(index);
            }
        }
    });
    if let Some(index) = remove {
        values.remove(index);
        changed = true;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, draft));
    changed
}
pub fn github(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    title(ui, "Git Providers", t);
    ui.add_space(8.);
    card(ui, t, |ui| {
        ui.horizontal(|ui| {
            let r = ui.allocate_space(vec2(44., 44.)).1;
            w::icon(ui, r.center(), Icon::Github, t.text, 30.);
            ui.vertical(|ui| {
                ui.strong("GitHub");
                hint(ui, "Connect GitHub to sync repositories with Reqon.", t);
            });
        });
        if p.github.connected {
            ui.label(format!("Connected as {}", p.github.username));
            if ui.button("Disconnect").clicked() {
                p.github.connected = false;
                p.github.username.clear();
                p.credentials.remove("github");
                changed = true;
            }
        } else {
            let key = p.credentials.entry("github".into()).or_default();
            changed |= secret_input(ui, "Access Token", key, 400., t);
            if primary(ui, "Connect", t).clicked() {
                let key = key.clone();
                let endpoint = p.github.endpoint.clone();
                state.status.insert("github".into(), "Connecting…".into());
                state.job(ui.ctx(), move || {
                    JobResult::Github(
                        crate::integrations::github_user(&endpoint, &key)
                            .map_err(|e| format!("{e:#}")),
                    )
                });
            }
        }
        if let Some(status) = state.status.get("github") {
            hint(ui, status, t);
        }
        ui.hyperlink_to(
            "GitHub token settings ↗",
            "https://github.com/settings/tokens",
        );
    });
    changed
}
pub const SECRET_KINDS: [(&str, &str); 5] = [
    ("aws-secrets-manager", "AWS Secrets Manager"),
    ("azure-key-vault", "Azure Key Vault"),
    ("hashicorp-vault-server", "HashiCorp Vault Server"),
    ("hashicorp-vault-cloud", "HashiCorp Vault Cloud"),
    ("gcp-secrets-manager", "Google Cloud Secret Manager"),
];
pub fn secrets(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    hint(
        ui,
        "Connect a secrets manager to use secrets in your requests.",
        t,
    );
    ui.add_space(12.);
    let mut remove = None;
    for provider in &mut p.secret_providers {
        ui.push_id(provider.id.clone(), |ui| {
            card(ui, t, |ui| {
                ui.horizontal(|ui| {
                    ui.strong(&provider.name);
                    hint(
                        ui,
                        SECRET_KINDS
                            .iter()
                            .find(|(k, _)| *k == provider.kind)
                            .map(|(_, n)| *n)
                            .unwrap_or(&provider.kind),
                        t,
                    );
                    changed |= toggle(ui, &mut provider.enabled, t);
                    if ui.button("Edit").clicked() {
                        state.secret_edit = Some(provider.clone());
                        state.secret_draft = p
                            .credentials
                            .iter()
                            .filter(|(k, _)| k.starts_with(&format!("secret:{}:", provider.id)))
                            .map(|(k, v)| (k.clone(), v.clone()))
                            .collect();
                        state.error.clear();
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(provider.id.clone());
                    }
                });
            });
        });
    }
    if let Some(id) = remove {
        p.secret_providers.retain(|x| x.id != id);
        p.credentials
            .retain(|key, _| !key.starts_with(&format!("secret:{id}:")));
        changed = true;
    }
    if primary(ui, "+ Add Provider", t).clicked() {
        state.secret_draft.clear();
        state.error.clear();
        state.secret_edit = Some(SecretProvider {
            id: crate::model::id(),
            kind: SECRET_KINDS[0].0.into(),
            enabled: true,
            ..Default::default()
        });
    }
    if let Some(mut provider) = state.secret_edit.take() {
        let mut close = false;
        let mut save = false;
        let (_, dismiss) = w::modal(ui.ctx(), "Secrets Manager", 570., t, |ui| {
            input(ui, "Name", &mut provider.name, 360., false);
            ui.horizontal(|ui| {
                ui.add_sized(vec2(100., 28.), egui::Label::new("Provider"));
                egui::ComboBox::from_id_salt("secret-kind")
                    .selected_text(
                        SECRET_KINDS
                            .iter()
                            .find(|(id, _)| *id == provider.kind)
                            .map(|(_, name)| *name)
                            .unwrap_or("Provider"),
                    )
                    .show_ui(ui, |ui| {
                        for (id, name) in SECRET_KINDS {
                            ui.selectable_value(&mut provider.kind, id.into(), name);
                        }
                    });
            });
            let fields: Vec<(&str, &str, bool)> = match provider.kind.as_str() {
                "aws-secrets-manager" => vec![
                    ("region", "Region", false),
                    ("access_key", "Access Key ID", true),
                    ("secret_key", "Secret Access Key", true),
                    ("session_token", "Session Token", true),
                    ("profile", "AWS CLI Profile", false),
                ],
                "azure-key-vault" => vec![
                    ("tenant", "Tenant ID", false),
                    ("client_id", "Client ID", false),
                    ("client_secret", "Client Secret", true),
                    ("vault", "Vault URL", false),
                ],
                "hashicorp-vault-cloud" => vec![
                    ("token_endpoint", "Token Endpoint", false),
                    ("secrets_endpoint", "Secrets Endpoint", false),
                    ("client_id", "Client ID", false),
                    ("client_secret", "Client Secret", true),
                    ("organization", "Organization ID", false),
                    ("project", "Project ID", false),
                    ("app", "App Name", false),
                ],
                "gcp-secrets-manager" => vec![
                    ("project", "Project ID", false),
                    ("client_email", "Client Email", false),
                    ("private_key", "Private Key", true),
                ],
                _ => {
                    ui.horizontal(|ui| {
                        ui.label("Authentication");
                        let method = provider
                            .fields
                            .entry("auth".into())
                            .or_insert("token".into());
                        for (id, label) in [
                            ("token", "Token"),
                            ("app_role", "AppRole"),
                            ("ldap", "LDAP"),
                        ] {
                            ui.selectable_value(method, id.into(), label);
                        }
                    });
                    let mut fields = vec![
                        ("url", "Server URL", false),
                        ("namespace", "Namespace", false),
                    ];
                    match provider
                        .fields
                        .get("auth")
                        .map(String::as_str)
                        .unwrap_or("token")
                    {
                        "app_role" => fields.extend([
                            ("mount", "Mount Path", false),
                            ("role_id", "Role ID", true),
                            ("secret_id", "Secret ID", true),
                        ]),
                        "ldap" => fields.extend([
                            ("username", "Username", false),
                            ("password", "Password", true),
                        ]),
                        _ => fields.push(("token", "Token", true)),
                    };
                    fields
                }
            };
            for (key, label, secret) in fields {
                if secret {
                    secret_input(
                        ui,
                        label,
                        state
                            .secret_draft
                            .entry(format!("secret:{}:{key}", provider.id))
                            .or_default(),
                        350.,
                        t,
                    );
                } else {
                    input(
                        ui,
                        label,
                        provider.fields.entry(key.into()).or_default(),
                        350.,
                        false,
                    );
                }
            }
            if !state.error.is_empty() {
                ui.colored_label(t.red, &state.error);
            }
            ui.add_space(12.);
            ui.horizontal(|ui| {
                if primary(ui, "Save", t).clicked() {
                    if provider.name.trim().is_empty() || provider.name.contains('.') {
                        state.error = "Enter a provider name without dots".into();
                    } else if p
                        .secret_providers
                        .iter()
                        .any(|x| x.id != provider.id && x.name == provider.name)
                    {
                        state.error = "A provider with this name already exists".into();
                    } else {
                        save = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        if save {
            p.credentials
                .extend(std::mem::take(&mut state.secret_draft));
            if let Some(old) = p.secret_providers.iter_mut().find(|x| x.id == provider.id) {
                *old = provider;
            } else {
                p.secret_providers.push(provider);
            }
            changed = true;
        } else if !dismiss && !close {
            state.secret_edit = Some(provider);
        }
    }
    ui.add_space(16.);
    hint(
        ui,
        "Reference a secret as {{secrets.provider-name.secret-path}}. Retrieved values stay in memory for the request.",
        t,
    );
    changed
}
