use crate::{
    app::App,
    preference_model::*,
    theme::Theme,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Sense, Stroke, TextEdit, pos2, vec2};
use std::collections::HashMap;

pub const TAB_ID: &str = "__preferences__";
pub const PANES: [(&str, Icon); 14] = [
    ("General", Icon::Settings),
    ("Themes", Icon::Palette),
    ("Display", Icon::Browser),
    ("Proxy", Icon::UserCircle),
    ("Client Certificates", Icon::Certificate),
    ("Features", Icon::Star),
    ("Secrets Manager", Icon::Lock),
    ("Git Providers", Icon::GitBranch),
    ("Keybindings", Icon::Keyboard),
    ("AI", Icon::Sparkles),
    ("Cache", Icon::Database),
    ("Support", Icon::Support),
    ("Beta", Icon::Beta),
    ("About", Icon::Info),
];
pub struct State {
    pub certificate: Option<(Certificate, String)>,
    pub certificate_busy: bool,
    pub error: String,
    pub ai_tab: usize,
    pub key_capture: Option<String>,
    pub key_draft: String,
    pub closed_tabs: Vec<String>,
    pub closing_tabs: std::collections::VecDeque<String>,
    pub status: HashMap<String, String>,
    pub system_proxy: crate::preference_network::SystemProxy,
    pub secret_edit: Option<SecretProvider>,
    pub secret_draft: std::collections::BTreeMap<String, String>,
    pub file_size: u64,
    pub jobs: Vec<std::sync::mpsc::Receiver<JobResult>>,
    pub applied: String,
    pub font: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            certificate: None,
            certificate_busy: false,
            error: String::new(),
            ai_tab: 0,
            key_capture: None,
            key_draft: String::new(),
            closed_tabs: vec![],
            closing_tabs: Default::default(),
            status: HashMap::new(),
            system_proxy: crate::preference_network::system_proxy(),
            secret_edit: None,
            secret_draft: Default::default(),
            file_size: 0,
            jobs: vec![],
            applied: String::new(),
            font: String::new(),
        }
    }
}
pub enum JobResult {
    Certificate(String, Result<(), String>),
    Models(String, Result<Vec<AiModel>, String>),
    Github(Result<String, String>),
    FileSize(u64),
}
impl State {
    pub fn job(&mut self, ctx: &egui::Context, job: impl FnOnce() -> JobResult + Send + 'static) {
        let (tx, rx) = std::sync::mpsc::channel();
        let ctx = ctx.clone();
        self.jobs.push(rx);
        std::thread::spawn(move || {
            let result = job();
            let _ = tx.send(result);
            ctx.request_repaint();
        });
    }
}
impl App {
    pub fn preferences_tick(&mut self, ctx: &egui::Context) {
        if self.dialog == crate::app::Dialog::None && self.pending_close.is_empty() {
            while let Some(id) = self.preference_ui.closing_tabs.pop_front() {
                if self.ws.open_tabs.contains(&id) {
                    self.actions.push(crate::app::Action::Close(id));
                    break;
                }
            }
        }
        let mut results = vec![];
        self.preference_ui.jobs.retain(|rx| match rx.try_recv() {
            Ok(r) => {
                results.push(r);
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(_) => false,
        });
        for result in results {
            match result {
                JobResult::Certificate(id, result) => {
                    self.preference_ui.certificate_busy = false;
                    if self
                        .preference_ui
                        .certificate
                        .as_ref()
                        .is_some_and(|(c, _)| c.id == id)
                    {
                        match result {
                            Ok(()) => {
                                let (certificate, password) =
                                    self.preference_ui.certificate.take().unwrap();
                                self.ws
                                    .preferences
                                    .credentials
                                    .insert(format!("cert:{id}"), password);
                                self.ws.preferences.certificates.push(certificate);
                                self.engine.configure(self.ws.preferences.clone());
                                self.mark_workspace_changed();
                            }
                            Err(error) => self.preference_ui.error = error,
                        }
                    }
                }
                JobResult::Models(id, result) => match result {
                    Ok(models) => {
                        if let Some(p) = self
                            .ws
                            .preferences
                            .ai
                            .providers
                            .iter_mut()
                            .find(|p| p.id == id)
                        {
                            for model in models {
                                if !p.models.iter().any(|m| m.id == model.id) {
                                    p.models.push(model);
                                }
                            }
                            self.mark_changed();
                        }
                        self.preference_ui
                            .status
                            .insert(id, "Connection successful".into());
                    }
                    Err(e) => {
                        self.preference_ui.status.insert(id, e);
                    }
                },
                JobResult::Github(result) => match result {
                    Ok(user) => {
                        self.ws.preferences.github.connected = true;
                        self.ws.preferences.github.username = user;
                        crate::git::configure_provider(&self.ws.preferences);
                        self.mark_changed();
                        self.preference_ui
                            .status
                            .insert("github".into(), "Connected".into());
                    }
                    Err(e) => {
                        self.preference_ui.status.insert("github".into(), e);
                    }
                },
                JobResult::FileSize(size) => self.preference_ui.file_size = size,
            }
        }
        let p = &self.ws.preferences;
        let key = format!(
            "{}:{}:{}:{}:{:?}",
            p.appearance.mode,
            p.appearance.light,
            p.appearance.dark,
            p.appearance.zoom,
            ctx.system_theme()
        );
        if key != self.preference_ui.applied {
            self.theme = crate::theme_registry::current(p, self.ws.dark, ctx);
            self.ws.dark = self.theme.dark;
            self.theme.apply(ctx);
            ctx.set_zoom_factor(f32::from(p.appearance.zoom.clamp(50, 150)) / 100.);
            self.preference_ui.applied = key;
            invalidate_editor_style(ctx);
        }
        if self.preference_ui.font != p.appearance.font && !ctx.egui_wants_keyboard_input() {
            crate::theme::editor_fonts(ctx, &p.appearance.font);
            self.preference_ui.font = p.appearance.font.clone();
            invalidate_editor_style(ctx);
        }
    }

    pub fn preferences_workspace(&mut self, ui: &mut egui::Ui, r: Rect) {
        let t = self.theme;
        self.preferences_tab = self.preferences_tab.min(PANES.len() - 1);
        let mut nav = w::child(
            ui,
            rect(r.left() + 12., r.top() + 12., 156., r.height() - 24.),
            "preferences-nav",
        );
        egui::ScrollArea::vertical()
            .id_salt("preferences-nav-scroll")
            .show(&mut nav, |ui| {
                ui.spacing_mut().item_spacing.y = 0.;
                for (i, (name, icon)) in PANES.iter().enumerate() {
                    let row = ui.allocate_space(vec2(156., 31.5)).1;
                    let response = ui.interact(row, ui.scope_id().with(i), Sense::click());
                    if i == self.preferences_tab || response.hovered() {
                        ui.painter().rect_filled(row, 4, t.surface);
                    }
                    w::icon(
                        ui,
                        pos2(row.left() + 17., row.center().y),
                        *icon,
                        t.muted,
                        16.,
                    );
                    w::label(
                        ui,
                        pos2(row.left() + 34., row.center().y),
                        name,
                        13.,
                        t.text,
                    );
                    if response.clicked() {
                        self.preferences_tab = i;
                        self.preference_ui.error.clear();
                        if i == 10 {
                            self.preference_ui
                                .job(ui.ctx(), || JobResult::FileSize(crate::file_cache::size()));
                        }
                    }
                }
            });
        let max = match self.preferences_tab {
            0 | 2 | 3 | 5 | 11 | 13 => 640.,
            4 => 800.,
            8 => 840.,
            _ => f32::INFINITY,
        };
        let content = rect(
            r.left() + 200.,
            r.top() + 12.,
            (r.width() - 212.).min(max).max(80.),
            r.height() - 24.,
        );
        let mut body = w::child(ui, content, "preferences-content");
        body.spacing_mut().item_spacing.y = 8.;
        let mut p = self.ws.preferences.clone();
        let mut changed = false;
        egui::ScrollArea::vertical().id_salt(("preferences-pane",self.preferences_tab)).show(&mut body,|ui|{
            ui.set_min_width(ui.available_width());let origin=ui.cursor().min;
            if !matches!(self.preferences_tab,4|7){
                let title=match self.preferences_tab{0=>"GENERAL SETTINGS",1=>"APPEARANCE",3=>"PROXY SETTINGS",12=>"BETA FEATURES",_=>PANES[self.preferences_tab].0};
                let row=ui.allocate_space(vec2(ui.available_width(),34.)).1;
                w::tracked_label(ui,pos2(row.left(),row.top()+14.),&title.to_uppercase(),12.,t.muted,0.6);if self.preferences_tab==9{let badge=rect(row.left()+26.,row.top()+5.,31.,18.);ui.painter().rect_filled(badge,4,t.blue.gamma_multiply(0.15));centered(ui,badge.center(),"Beta",11.,t.blue);}ui.add_space(-8.);
            }
            match self.preferences_tab{
                0=>changed|=crate::preferences_general::general(ui,origin,&mut p,t),
                1=>changed|=crate::preferences_appearance::themes(ui,&mut p,t,self.ws.dark),
                2=>changed|=crate::preferences_appearance::display(ui,&mut p,t,&mut self.preference_ui),
                3=>changed|=crate::preferences_connection::proxy(ui,&mut p,t,&mut self.preference_ui),
                4=>changed|=crate::preferences_connection::certificates(ui,&mut p,t,&mut self.preference_ui),
                5=>{ui.add_space(15.);ui.label("Turn on/off additional features.");ui.add_space(8.);for (label,value)in[("API Spec",&mut p.features.api_spec),("Git",&mut p.features.git),("File Explorer",&mut p.features.file_explorer),("Show reqon.json",&mut p.features.reqon_json)]{changed|=w::checkbox(ui,value,label,t);}},
                6=>changed|=crate::preferences_integrations::secrets(ui,&mut p,t,&mut self.preference_ui),
                7=>changed|=crate::preferences_integrations::github(ui,&mut p,t,&mut self.preference_ui),
                8=>changed|=crate::keybindings::settings(ui,&mut p.keybindings,t,&mut self.preference_ui),
                9=>changed|=crate::preferences_integrations::ai(ui,&mut p,t,&mut self.preference_ui),
                10=>{
                    ui.add_space(-4.);
                    let before=p.cache.file;
                    let ctx=ui.ctx().clone();
                    changed|=cache_card(ui,"File cache",Some("Beta"),"Opens collections faster by caching their parsed contents. Reqon refreshes the cache when your collection changes. Clearing it won't affect your original files.",&mut p.cache.file,Some(self.preference_ui.file_size),t,||{self.preference_ui.job(&ctx,||{crate::file_cache::clear();JobResult::FileSize(crate::file_cache::size())});});
                    if before!=p.cache.file{crate::file_cache::configure(&self.root,p.cache.file);}
                    ui.add_space(8.);
                    changed|=cache_card(ui,"SSL session cache",None,"Reuses TLS sessions and connections across requests for faster handshakes. Disable to create a fresh connection for every request.",&mut p.cache.ssl,None,t,||self.engine.clear_tls_cache());
                },
                11=>crate::preferences_appearance::support(ui,t),
                12=>changed|=crate::preferences_appearance::beta(ui,&mut p,t,&mut self.preferences_tab),
                13=>{if crate::preferences_appearance::about(ui,&mut p,t,&self.root){self.open_updates();}},_=>{}
            }
        });
        changed |= crate::preferences_connection::certificate_dialog(
            ui.ctx(),
            &mut p,
            t,
            &mut self.preference_ui,
        );
        if changed {
            self.ws.preferences = p;
            crate::git::configure_provider(&self.ws.preferences);
            self.engine.configure(self.ws.preferences.clone());
            self.tree.stale = true;
            self.mark_changed();
        }
    }
}

pub fn hint(ui: &mut egui::Ui, text: &str, t: Theme) {
    ui.label(egui::RichText::new(text).size(12.).color(t.muted));
}
pub fn title(ui: &mut egui::Ui, text: &str, t: Theme) {
    let r = ui.allocate_space(vec2(ui.available_width(), 24.)).1;
    w::semibold(ui, pos2(r.left(), r.top() + 10.), text, 13., t.text);
}
pub fn separator(ui: &mut egui::Ui) {
    ui.add_space(8.);
    ui.separator();
    ui.add_space(8.);
}
pub fn row_text(ui: &mut egui::Ui, label: &str, text: &str, _t: Theme) {
    ui.horizontal(|ui| {
        field_label(ui, label, 115., 24.);
        ui.label(text);
    });
}
pub fn input(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    width: f32,
    password: bool,
) -> bool {
    ui.horizontal(|ui| {
        field_label(ui, label, 100., 30.);
        ui.add_sized(
            vec2(width.min((ui.available_width() - 24.).max(80.)), 34.),
            TextEdit::singleline(value)
                .password(password)
                .margin(vec2(7., 7.)),
        )
        .changed()
    })
    .inner
}
pub fn secret_input(
    ui: &mut egui::Ui,
    label: &str,
    value: &mut String,
    width: f32,
    t: Theme,
) -> bool {
    let id = ui.scope_id().with(("secret-visible", label));
    let mut show = ui
        .ctx()
        .data_mut(|d| d.get_temp::<bool>(id))
        .unwrap_or(false);
    let mut changed = false;
    ui.horizontal(|ui| {
        field_label(ui, label, 100., 30.);
        changed = ui
            .add_sized(
                vec2(width.min((ui.available_width() - 32.).max(80.)), 34.),
                TextEdit::singleline(value)
                    .password(!show)
                    .margin(vec2(7., 7.)),
            )
            .changed();
        let r = ui.allocate_space(vec2(24., 28.)).1;
        if w::icon_button(ui, r, Icon::Eye, "Show / hide", t).clicked() {
            show = !show;
        }
    });
    ui.ctx().data_mut(|d| d.insert_temp(id, show));
    changed
}
pub fn toggle(ui: &mut egui::Ui, value: &mut bool, t: Theme) -> bool {
    let (r, response) = ui.allocate_exact_size(vec2(32., 18.), Sense::click());
    let mut changed = false;
    if response.clicked() {
        *value = !*value;
        changed = true;
    }
    ui.painter().rect_filled(
        r,
        9,
        if *value {
            t.accent
        } else {
            egui::Color32::from_gray(120)
        },
    );
    ui.painter().circle_filled(
        pos2(
            if *value {
                r.right() - 8.
            } else {
                r.left() + 8.
            },
            r.center().y,
        ),
        6.,
        egui::Color32::WHITE,
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Switch,
            ui.is_enabled(),
            *value,
            "Toggle setting",
        )
    });
    changed
}
pub fn toggle_row(ui: &mut egui::Ui, value: &mut bool, text: &str, t: Theme) -> bool {
    ui.horizontal(|ui| {
        let changed = toggle(ui, value, t);
        ui.label(text);
        changed
    })
    .inner
}
pub fn primary(ui: &mut egui::Ui, text: &str, t: Theme) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(egui::Color32::BLACK))
            .fill(t.accent)
            .stroke(Stroke::NONE),
    )
}
pub fn file_input(ui: &mut egui::Ui, label: &str, path: &mut String, t: Theme) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        field_label(ui, label, 100., 30.);
        if path.is_empty() {
            if ui.button("Select File").clicked() {
                if let Some(p) = rfd::FileDialog::new().pick_file() {
                    *path = p.display().to_string();
                    changed = true;
                }
            }
        } else {
            ui.label(
                std::path::Path::new(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy(),
            )
            .on_hover_text(path.as_str());
            let r = ui.allocate_space(vec2(24., 24.)).1;
            if w::icon_button(ui, r, Icon::Trash, "Remove file", t).clicked() {
                path.clear();
                changed = true;
            }
        }
    });
    changed
}
pub fn centered(ui: &egui::Ui, at: egui::Pos2, text: &str, size: f32, color: egui::Color32) {
    w::label(
        ui,
        pos2(at.x - w::text_width(ui, text, size) / 2., at.y),
        text,
        size,
        color,
    );
}
fn cache_card(
    ui: &mut egui::Ui,
    name: &str,
    badge: Option<&str>,
    description: &str,
    enabled: &mut bool,
    size: Option<u64>,
    t: Theme,
    clear: impl FnOnce(),
) -> bool {
    let width = ui.available_width();
    let mut job = egui::text::LayoutJob::simple(
        description.to_owned(),
        egui::FontId::proportional(13.),
        t.muted,
        (width - 76.).max(80.),
    );
    for section in &mut job.sections {
        section.format.line_height = Some(19.5);
    }
    let galley = ui.painter().layout_job(job);
    let height = 47. + 28. + galley.size().y + if size.is_some() { 28. } else { 0. };
    let r = ui
        .allocate_space(vec2(
            width,
            height.max(if size.is_some() { 143. } else { 100. }),
        ))
        .1;
    let mut card = w::child(ui, r, ("cache-card", name));
    card.painter().rect_filled(r, 8, t.bg);
    card.painter().rect_filled(
        rect(r.left(), r.top(), width, 47.),
        egui::CornerRadius {
            nw: 8,
            ne: 8,
            sw: 0,
            se: 0,
        },
        t.surface,
    );
    card.painter()
        .rect_stroke(r, 8, Stroke::new(1., t.border), egui::StrokeKind::Inside);
    w::line(
        &card,
        pos2(r.left(), r.top() + 47.),
        pos2(r.right(), r.top() + 47.),
        t.border,
    );
    w::semibold(
        &card,
        pos2(r.left() + 16., r.top() + 23.5),
        name,
        14.,
        t.text,
    );
    if let Some(badge) = badge {
        let x = r.left() + 16. + w::text_width(&card, name, 14.) + 12.;
        let badge_rect = rect(x, r.top() + 14., 37., 19.);
        card.painter()
            .rect_filled(badge_rect, 4, t.blue.gamma_multiply(0.15));
        centered(&card, badge_rect.center(), badge, 11., t.blue);
    }
    let mut toggle_ui = w::child(
        &mut card,
        rect(r.right() - 48., r.top() + 15., 32., 18.),
        "toggle",
    );
    let changed = toggle(&mut toggle_ui, enabled, t);
    card.painter()
        .galley(pos2(r.left() + 16., r.top() + 61.), galley, t.muted);
    if let Some(bytes) = size {
        w::label(
            &card,
            pos2(r.left() + 16., r.bottom() - 24.),
            "Cache size",
            13.,
            t.muted,
        );
        let value = if bytes >= 1024 * 1024 {
            format!("{:.1} MB", bytes as f64 / 1048576.)
        } else if bytes >= 1024 {
            format!("{:.1} KB", bytes as f64 / 1024.)
        } else {
            format!("{bytes} B")
        };
        w::semibold(
            &card,
            pos2(
                r.left() + 20. + w::text_width(&card, "Cache size", 13.),
                r.bottom() - 24.,
            ),
            &value,
            13.,
            t.text,
        );
    }
    let clear_rect = rect(r.right() - 42., r.bottom() - 38., 24., 24.);
    if size != Some(0) {
        if w::icon_button(&mut card, clear_rect, Icon::Eraser, "Clear cache", t).clicked() {
            clear();
        }
    } else {
        w::icon(
            &card,
            clear_rect.center(),
            Icon::Eraser,
            t.faint.gamma_multiply(0.5),
            16.,
        );
    }
    changed
}

fn invalidate_editor_style(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        let key = egui::Id::unique("editor-style-generation");
        let next = d.get_temp::<u64>(key).unwrap_or(0).wrapping_add(1);
        d.insert_temp(key, next);
    });
}

pub fn field_label(ui: &mut egui::Ui, text: &str, width: f32, height: f32) {
    let r = ui.allocate_space(vec2(width, height)).1;
    w::label(ui, r.left_center(), text, 13., ui.visuals().text_color());
}
