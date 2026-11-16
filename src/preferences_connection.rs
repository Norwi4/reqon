use crate::{
    model::Preferences,
    preference_model::Certificate,
    preferences::*,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Stroke, StrokeKind, pos2, vec2};
pub fn proxy(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    let origin = ui.cursor().min;
    ui.add_space(19.);
    ui.horizontal(|ui| {
        field_label(ui, "Mode", 92., 24.);
        for (id, name) in [
            ("off", "Off"),
            ("manual", "On"),
            ("system", "System Proxy"),
            ("pac", "PAC"),
        ] {
            let selected =
                p.proxy_settings.mode == id || p.proxy_settings.mode == "legacy" && id == "system";
            let row = ui
                .allocate_space(vec2(w::text_width(ui, name, 13.) + 26., 24.))
                .1;
            let center = pos2(row.left() + 6., row.center().y);
            ui.painter().circle(
                center,
                6.,
                if selected {
                    t.bg.blend(t.accent.gamma_multiply(0.25))
                } else {
                    egui::Color32::from_gray(248)
                },
                Stroke::new(
                    1.,
                    if selected {
                        t.accent
                    } else {
                        egui::Color32::from_gray(145)
                    },
                ),
            );
            if selected {
                ui.painter().circle_filled(center, 3.2, t.accent);
            }
            w::label(
                ui,
                pos2(row.left() + 17., row.center().y),
                name,
                13.,
                t.text,
            );
            if ui
                .interact(
                    row,
                    ui.scope_id().with(("proxy-mode", id)),
                    egui::Sense::click(),
                )
                .clicked()
            {
                p.proxy_settings.mode = id.into();
                p.proxy.clear();
                changed = true;
            }
        }
    });
    ui.add_space(14.);
    match p.proxy_settings.mode.as_str() {
        "off" => {}
        "manual" => {
            ui.horizontal(|ui| {
                field_label(ui, "Protocol", 100., 24.);
                for protocol in ["http", "https", "socks4", "socks5"] {
                    changed |= ui
                        .radio_value(&mut p.proxy_settings.protocol, protocol.into(), protocol)
                        .changed();
                }
            });
            changed |= input(ui, "Hostname", &mut p.proxy_settings.host, 300., false);
            changed |= input(ui, "Port", &mut p.proxy_settings.port, 120., false);
            changed |= w::checkbox(ui, &mut p.proxy_settings.auth, "Authentication", t);
            ui.add_enabled_ui(p.proxy_settings.auth, |ui| {
                changed |= input(ui, "Username", &mut p.proxy_settings.username, 300., false);
                changed |= secret_input(
                    ui,
                    "Password",
                    p.credentials.entry("proxy".into()).or_default(),
                    300.,
                    t,
                );
            });
            changed |= input(
                ui,
                "Proxy Bypass",
                &mut p.proxy_settings.bypass,
                400.,
                false,
            );
            hint(
                ui,
                "Comma-separated hostnames that should bypass the proxy",
                t,
            );
            if !p.proxy_settings.port.is_empty()
                && p.proxy_settings
                    .port
                    .parse::<u16>()
                    .ok()
                    .filter(|x| *x > 0)
                    .is_none()
            {
                ui.colored_label(t.red, "Port must be between 1 and 65535");
            }
        }
        "pac" => {
            title(ui, "Proxy Auto-Configuration", t);
            ui.horizontal(|ui| {
                changed |= ui
                    .radio_value(&mut p.proxy_settings.pac_file, false, "URL")
                    .changed();
                changed |= ui
                    .radio_value(&mut p.proxy_settings.pac_file, true, "File")
                    .changed();
            });
            if p.proxy_settings.pac_file {
                changed |= file_input(ui, "PAC File", &mut p.proxy_settings.pac_source, t);
            } else {
                changed |= input(ui, "PAC URL", &mut p.proxy_settings.pac_source, 400., false);
            }
            if ui.link("Refresh PAC cache").clicked() {
                crate::preference_network::clear_pac();
                state
                    .status
                    .insert("pac".into(), "PAC cache cleared".into());
            }
            if let Some(s) = state.status.get("pac") {
                hint(ui, s, t);
            }
        }
        _ => {
            w::label(ui, origin + vec2(0., 74.), "System Proxy", 13., t.text);
            w::label(
                ui,
                origin + vec2(0., 93.),
                "Below values are sourced from your system proxy settings.",
                11.,
                t.muted,
            );
            w::label(
                ui,
                origin + vec2(0., 124.),
                &format!("Proxy source:  {}", state.system_proxy.source),
                12.,
                t.text,
            );
            w::label(
                ui,
                origin + vec2(0., 143.),
                "These values cannot be directly updated in Reqon. Please refer to your OS documentation to update these.",
                11.,
                t.muted,
            );
            for (i, (label, value)) in [
                ("http_proxy", &state.system_proxy.http),
                ("https_proxy", &state.system_proxy.https),
                ("no_proxy", &state.system_proxy.bypass),
                ("pac_url", &state.system_proxy.pac),
            ]
            .into_iter()
            .enumerate()
            {
                let y = origin.y + 186. + i as f32 * 23.5;
                w::label(ui, pos2(origin.x, y), label, 13., t.accent);
                let value = if value.is_empty() {
                    "-".into()
                } else {
                    redact_proxy(value)
                };
                w::label(ui, pos2(origin.x + 100., y), &value, 12., t.pink);
            }
            let r = rect(origin.x, origin.y + 285., 90., 22.);
            w::icon(
                ui,
                pos2(r.left() + 7., r.center().y),
                Icon::Refresh,
                t.blue,
                14.,
            );
            w::label(
                ui,
                pos2(r.left() + 18., r.center().y),
                "Refresh",
                13.,
                t.blue,
            );
            if ui
                .interact(r, ui.scope_id().with("refresh-proxy"), egui::Sense::click())
                .clicked()
            {
                state.system_proxy = crate::preference_network::system_proxy();
            }
            ui.allocate_rect(
                rect(origin.x, origin.y + 315., 1., 1.),
                egui::Sense::hover(),
            );
        }
    }
    changed
}
fn redact_proxy(value: &str) -> String {
    if let Ok(mut u) = url::Url::parse(value) {
        if u.password().is_some() {
            let _ = u.set_password(Some("********"));
            return u.to_string();
        }
    }
    value.into()
}
pub fn certificates(ui: &mut egui::Ui, p: &mut Preferences, t: Theme, state: &mut State) -> bool {
    let mut changed = false;
    title(ui, "Client Certificates", t);
    hint(
        ui,
        "Add client certificates to be used for specific domains.",
        t,
    );
    ui.add_space(16.);
    if p.certificates.is_empty() {
        let r = ui.allocate_space(vec2(ui.available_width(), 215.)).1;
        ui.painter()
            .rect_stroke(r, 8, Stroke::new(1., t.border), StrokeKind::Inside);
        w::icon(
            ui,
            pos2(r.center().x, r.top() + 44.),
            Icon::Certificate,
            t.muted,
            25.,
        );
        centered(
            ui,
            pos2(r.center().x, r.top() + 73.),
            "No client certificates",
            13.,
            t.text,
        );
        centered(
            ui,
            pos2(r.center().x, r.top() + 99.),
            "Client certificates authenticate you to servers that require them.",
            12.,
            t.muted,
        );
        centered(
            ui,
            pos2(r.center().x, r.top() + 117.),
            "Add a certificate to get started.",
            12.,
            t.muted,
        );
        if w::button(
            ui,
            rect(r.center().x - 66., r.top() + 154., 132., 28.),
            "+ Add Certificate",
            true,
            t,
        )
        .clicked()
        {
            state.certificate = Some((Certificate::default(), String::new()));
            state.error.clear();
        }
    } else {
        let mut remove = None;
        for c in &mut p.certificates {
            ui.push_id(&c.id, |ui| {
                egui::Frame::new()
                    .stroke(Stroke::new(1., t.border))
                    .corner_radius(8)
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&c.domain).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let r = ui.allocate_space(vec2(24., 24.)).1;
                                    if w::icon_button(ui, r, Icon::Trash, "Remove certificate", t)
                                        .clicked()
                                    {
                                        remove = Some(c.id.clone());
                                    }
                                    changed |= toggle(ui, &mut c.enabled, t);
                                },
                            );
                        });
                        row_text(
                            ui,
                            "Type",
                            if c.kind == "pfx" { "PFX" } else { "Cert & Key" },
                            t,
                        );
                        if c.kind == "pfx" {
                            row_text(ui, "PFX File", &basename(&c.pfx), t);
                        } else {
                            row_text(ui, "Cert File", &basename(&c.cert), t);
                            row_text(ui, "Key File", &basename(&c.key), t);
                        }
                        if let Some(password) = p
                            .credentials
                            .get(&format!("cert:{}", c.id))
                            .filter(|s| !s.is_empty())
                        {
                            let mut mask = password.clone();
                            if secret_input(ui, "Passphrase", &mut mask, 300., t) {
                                p.credentials.insert(format!("cert:{}", c.id), mask);
                                changed = true;
                            }
                        }
                    });
            });
            ui.add_space(8.);
        }
        if let Some(id) = remove {
            p.certificates.retain(|c| c.id != id);
            p.credentials.remove(&format!("cert:{id}"));
            changed = true;
        }
        if primary(ui, "+ Add Certificate", t).clicked() {
            state.certificate = Some((Certificate::default(), String::new()));
            state.error.clear();
        }
    }
    changed
}
fn basename(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}
pub fn certificate_dialog(
    ctx: &egui::Context,
    _p: &mut Preferences,
    t: Theme,
    state: &mut State,
) -> bool {
    let Some((mut c, mut password)) = state.certificate.take() else {
        return false;
    };
    let mut close = false;
    let (_, dismiss) = w::modal(ctx, "Add Client Certificate", 500., t, |ui| {
        hint(
            ui,
            "The certificate and key files are stored as absolute paths and apply to every collection.",
            t,
        );
        ui.add_space(10.);
        input(ui, "Domain", &mut c.domain, 300., false);
        ui.horizontal(|ui| {
            ui.add_sized(vec2(100., 28.), egui::Label::new("Type"));
            ui.selectable_value(&mut c.kind, "cert".into(), "Cert & Key");
            ui.selectable_value(&mut c.kind, "pfx".into(), "PFX");
        });
        if c.kind == "pfx" {
            file_input(ui, "PFX file", &mut c.pfx, t);
        } else {
            file_input(ui, "Cert file", &mut c.cert, t);
            file_input(ui, "Key file", &mut c.key, t);
        }
        secret_input(ui, "Passphrase", &mut password, 300., t);
        if !state.error.is_empty() {
            ui.colored_label(t.red, &state.error);
        }
        ui.add_space(16.);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !state.certificate_busy,
                    egui::Button::new(if state.certificate_busy {
                        "Checking…"
                    } else {
                        "Add"
                    })
                    .fill(t.accent),
                )
                .clicked()
            {
                if c.domain.trim().is_empty() {
                    state.error = "Domain is required".into();
                } else if c.kind == "pfx" && c.pfx.is_empty()
                    || c.kind != "pfx" && (c.cert.is_empty() || c.key.is_empty())
                {
                    state.error = "Select the certificate and private key files".into();
                } else {
                    let candidate = c.clone();
                    let password = password.clone();
                    let id = c.id.clone();
                    state.certificate_busy = true;
                    state.job(ctx, move || {
                        JobResult::Certificate(
                            id,
                            crate::preference_network::certificate_pem(&candidate, &password)
                                .map(|_| ())
                                .map_err(|e| format!("{e:#}")),
                        )
                    });
                }
            }
            if ui.button("Cancel").clicked() {
                close = true;
            }
        });
    });
    if !close && !dismiss {
        state.certificate = Some((c, password));
    }
    false
}
