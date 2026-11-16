use crate::widgets::{self as w, rect};
use eframe::egui::{self, Rect, Sense, TextEdit, pos2, vec2};
pub fn general(
    ui: &mut egui::Ui,
    origin: egui::Pos2,
    p: &mut crate::model::Preferences,
    t: crate::theme::Theme,
) -> bool {
    let mut changed = false;
    let section_at = |ui: &egui::Ui, text: &str, y: f32| {
        w::semibold(ui, pos2(origin.x, origin.y + y), text, 13., t.text)
    };
    let line_at = |ui: &egui::Ui, y: f32| {
        w::line(
            ui,
            pos2(origin.x, origin.y + y),
            pos2(origin.x + 640_f32.min(ui.available_width()), origin.y + y),
            t.border,
        )
    };
    section_at(ui, "Certificates", 57.75);
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 79.5, 640., 19.5),
        &mut p.verify_tls,
        "SSL/TLS Certificate Verification",
        t,
    );
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 107., 640., 19.5),
        &mut p.custom_ca,
        "Use Custom CA Certificate",
        t,
    );
    ui.add_enabled_ui(p.custom_ca, |ui| {
        if !p.ca_file.is_empty() {
            let name = std::path::Path::new(&p.ca_file)
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            w::label(
                ui,
                pos2(origin.x + 135., origin.y + 145.),
                &name,
                12.,
                t.muted,
            );
            if w::icon_button(
                ui,
                rect(origin.x + 400., origin.y + 132., 24., 26.),
                w::Icon::Trash,
                "Remove certificate",
                t,
            )
            .clicked()
            {
                p.ca_file.clear();
                changed = true;
            }
        }
        if w::button(
            ui,
            rect(origin.x + 24., origin.y + 131.5, 100., 27.5),
            "Select File",
            false,
            t,
        )
        .clicked()
        {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("PEM certificates", &["pem", "crt", "cer"])
                .pick_file()
            {
                p.ca_file = path.to_string_lossy().into_owned();
                changed = true;
            }
        }
        ui.add_enabled_ui(!p.ca_file.is_empty(), |ui| {
            changed |= w::checkbox_at(
                ui,
                rect(origin.x + 24., origin.y + 165.55, 616., 19.5),
                &mut p.keep_default_ca,
                "Keep Default CA Certificates",
                t,
            );
        });
    });
    line_at(ui, 201.55);
    section_at(ui, "Cookies & Authorization", 227.8);
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 249.55, 640., 19.5),
        &mut p.store_cookies,
        "Store Cookies automatically",
        t,
    );
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 277.05, 640., 19.5),
        &mut p.send_cookies,
        "Send Cookies automatically",
        t,
    );
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 304.55, 640., 19.5),
        &mut p.system_browser_oauth,
        "Use System Browser for OAuth2 Authorization",
        t,
    );
    line_at(ui, 340.55);
    section_at(ui, "Auto Save", 366.8);
    changed |= w::checkbox_at(
        ui,
        rect(origin.x, origin.y + 388.55, 640., 19.5),
        &mut p.autosave,
        "Enable Auto Save",
        t,
    );
    w::label(
        ui,
        pos2(origin.x, origin.y + 425.8),
        "Auto Save Delay (ms)",
        13.,
        t.muted,
    );
    ui.add_enabled_ui(p.autosave, |ui| {
        changed |= number(
            ui,
            rect(origin.x, origin.y + 441.55, 224., 35.875),
            "autosave-delay",
            &mut p.autosave_delay,
        );
    });
    line_at(ui, 493.42);
    section_at(ui, "Requests", 519.67);
    w::label(
        ui,
        pos2(origin.x, origin.y + 551.67),
        "Request Timeout (ms)",
        13.,
        t.text,
    );
    changed |= number(
        ui,
        rect(origin.x, origin.y + 567.42, 224., 35.875),
        "request-timeout",
        &mut p.timeout,
    );
    line_at(ui, 619.8);
    section_at(ui, "Default Location", 646.05);
    w::label(
        ui,
        pos2(origin.x, origin.y + 668.8),
        "Used as the default location for new workspaces and collections",
        11.,
        t.muted,
    );
    changed |= ui
        .put(
            rect(
                origin.x,
                origin.y + 694.3,
                640_f32.min(ui.available_width()),
                35.875,
            ),
            TextEdit::singleline(&mut p.default_location).margin(vec2(7., 7.)),
        )
        .changed();
    let browse = rect(origin.x, origin.y + 735., 70., 22.);
    w::label(
        ui,
        pos2(browse.left(), browse.center().y),
        "Browse",
        13.,
        t.blue,
    );
    if ui
        .interact(
            browse,
            ui.scope_id().with("default-location"),
            Sense::click(),
        )
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            p.default_location = path.to_string_lossy().into_owned();
            changed = true;
        }
    }
    ui.allocate_rect(rect(origin.x, origin.y + 760., 1., 1.), Sense::hover());
    changed
}
fn number(ui: &mut egui::Ui, r: Rect, key: &str, value: &mut u64) -> bool {
    let id = ui.scope_id().with(key);
    let mut cached = ui
        .ctx()
        .data_mut(|d| d.get_temp::<(u64, String)>(id))
        .unwrap_or((*value, value.to_string()));
    if cached.0 != *value {
        cached = (*value, value.to_string());
    }
    let response = ui.put(
        r,
        TextEdit::singleline(&mut cached.1)
            .id(id.with("input"))
            .margin(vec2(7., 7.)),
    );
    let mut changed = false;
    if response.changed() {
        if let Ok(next) = cached.1.parse::<u64>() {
            if key != "autosave-delay" || next >= 500 {
                *value = next;
                cached.0 = next;
                changed = true;
            }
        }
    }
    if response.lost_focus() {
        cached.1 = value.to_string();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, cached));
    changed
}
