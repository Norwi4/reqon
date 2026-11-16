//! Per-request command drafts. Parsing never executes a command or sends a request.
use crate::{
    app::App,
    model::*,
    widgets::{self as w, Icon, rect},
};
use eframe::egui::{self, Rect, Ui, pos2, vec2};
use std::{
    collections::{BTreeMap, HashMap},
    sync::mpsc,
    time::{Duration, Instant},
};

pub struct Draft {
    pub source: String,
    pub shell: String,
    pub message: String,
    pub dirty: bool,
    pub error: bool,
    edited: Instant,
    revision: u64,
    parsed: Option<u64>,
    pending: Option<mpsc::Receiver<(u64, Result<Request, String>)>>,
    expected: Option<String>,
}
impl Default for Draft {
    fn default() -> Self {
        Self {
            source: String::new(),
            shell: "auto".into(),
            message: String::new(),
            dirty: false,
            error: false,
            edited: Instant::now(),
            revision: 0,
            parsed: None,
            pending: None,
            expected: None,
        }
    }
}
#[derive(Default)]
pub struct State {
    pub open: bool,
    pub drafts: HashMap<String, Draft>,
}
pub fn looks_like_command(source: &str) -> bool {
    let first = source
        .trim_start()
        .split_whitespace()
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(
        first.as_str(),
        "curl" | "curl.exe" | "grpcurl" | "grpcurl.exe"
    )
}

fn retain_details(new: &mut Vec<Pair>, old: &[Pair]) {
    let mut used = vec![false; old.len()];
    for row in new.iter_mut() {
        if let Some((i, source)) = old
            .iter()
            .enumerate()
            .find(|(i, p)| !used[*i] && p.enabled && p.key == row.key)
        {
            used[i] = true;
            row.description = source.description.clone();
            row.annotations = source.annotations.clone();
        }
    }
    new.extend(old.iter().filter(|p| !p.enabled).cloned());
}
/// Only replace fields represented by a command; identity and local metadata survive.
pub fn apply(request: &mut Request, mut parsed: Request) {
    retain_details(&mut parsed.headers, &request.headers);
    retain_details(&mut parsed.params, &request.params);
    retain_details(&mut parsed.form, &request.form);
    request.method = parsed.method;
    request.protocol = parsed.protocol;
    request.url = parsed.url;
    request.params = parsed.params;
    request.headers = parsed.headers;
    // GET/HEAD commands deliberately omit the dormant editor body.
    if parsed.body_kind != BodyKind::None || !matches!(request.method.as_str(), "GET" | "HEAD") {
        request.body_kind = parsed.body_kind;
        request.body = parsed.body;
        request.form = parsed.form;
        request.graphql_variables = parsed.graphql_variables;
    }
    request.auth = parsed.auth;
    request.encode_url = parsed.encode_url;
    request.timeout = parsed.timeout;
    request.timeout_inherit = parsed.timeout_inherit;
    request.verify_tls = parsed.verify_tls;
    request.follow_redirects = parsed.follow_redirects;
    request.max_redirects = parsed.max_redirects;
    request.forward_auth = parsed.forward_auth;
    request.proxy = parsed.proxy;
    request.ca_certificate = parsed.ca_certificate;
    request.client_certificate = parsed.client_certificate;
    request.client_key = parsed.client_key;
    request.grpc_method = parsed.grpc_method;
    request.grpc_tls = parsed.grpc_tls;
    request.proto_files = parsed.proto_files;
    request.proto_import_paths = parsed.proto_import_paths;
    if request.protocol == "grpc" {
        request.protocol_messages = vec![Pair::new("Message 1", &request.body)];
    }
    if !request.portable.is_object() {
        request.portable = serde_json::json!({});
    }
    // A removed command option must stop affecting the next execution.
    // Keep unrelated import metadata, examples and extension fields.
    for key in [
        "generated_headers",
        "http_version",
        "http_follow_original_method",
        "http_auth_scheme",
        "http_proxy",
        "http_max_redirects",
        "http_follow_auth",
        "tls_server_name",
        "grpc_emit_defaults",
        "grpc_max_response_mb",
        "grpc_proto_files",
        "grpc_include_paths",
        "descriptor_file",
        "descriptor_base64",
        "__command_notes",
        "__legacy_query_url",
    ] {
        request.portable.as_object_mut().unwrap().remove(key);
    }
    if let Some(values) = parsed.portable.as_object() {
        for (key, value) in values {
            request.portable[key] = value.clone();
        }
    }
}
impl Draft {
    pub fn regenerate(&mut self, request: &Request) {
        self.revision += 1;
        self.source = crate::engine::curl(request, &BTreeMap::new());
        self.expected = Some(self.source.clone());
        self.dirty = false;
        self.error = false;
        self.pending = None;
        self.parsed = None;
        self.shell = "auto".into();
        self.message = "Edit the command to update this request automatically.".into();
    }
    pub fn edit(&mut self) {
        self.revision += 1;
        self.dirty = true;
        self.error = false;
        self.parsed = None;
        self.pending = None;
        self.edited = Instant::now();
        self.message = "Editing…".into();
    }
    pub fn tick(&mut self, request: &mut Request, ctx: &egui::Context) -> bool {
        let mut changed = false;
        if let Some((revision, result)) = self.pending.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.pending = None;
            if revision == self.revision {
                self.parsed = Some(revision);
                if !self.matches(request) {
                    self.conflict();
                    return false;
                }
                match result {
                    Ok(parsed) => {
                        let notes = parsed.portable["__command_notes"]
                            .as_array()
                            .map(|a| {
                                a.iter()
                                    .filter_map(serde_json::Value::as_str)
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            })
                            .unwrap_or_default();
                        apply(request, parsed);
                        self.expected = Some(crate::engine::curl(request, &BTreeMap::new()));
                        self.message = if notes.is_empty() {
                            "Changes applied".into()
                        } else {
                            format!("Changes applied · {notes}")
                        };
                        self.dirty = false;
                        self.error = false;
                        changed = true;
                    }
                    Err(error) => {
                        self.message = error;
                        self.error = true;
                    }
                }
            }
        }
        if self.dirty && self.parsed != Some(self.revision) && self.pending.is_none() {
            let delay = Duration::from_millis(650);
            if self.edited.elapsed() >= delay {
                let (tx, rx) = mpsc::channel();
                let source = self.source.clone();
                let shell = self.shell.clone();
                let revision = self.revision;
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result =
                        crate::import::command(&source, &shell).map_err(|e| format!("{e:#}"));
                    let _ = tx.send((revision, result));
                    ctx.request_repaint();
                });
                self.pending = Some(rx);
                self.message = "Checking command…".into();
            } else {
                ctx.request_repaint_after(delay.saturating_sub(self.edited.elapsed()));
            }
        }
        changed
    }
    fn matches(&self, request: &Request) -> bool {
        self.expected
            .as_ref()
            .is_none_or(|expected| *expected == crate::engine::curl(request, &BTreeMap::new()))
    }
    fn conflict(&mut self) {
        self.revision += 1;
        self.parsed = Some(self.revision);
        self.pending = None;
        self.error = true;
        self.message =
            "Request changed. Regenerate, or edit this command again to apply it.".into();
    }
}
impl App {
    pub fn curl_tick(&mut self, ctx: &egui::Context) {
        for request in &mut self.ws.requests {
            if self.curl.open && self.ws.active.as_deref() == Some(&request.id) {
                continue;
            }
            if let Some(draft) = self.curl.drafts.get_mut(&request.id) {
                if draft.tick(request, ctx) {
                    self.tree.stale = true;
                    self.dirty_cache.borrow_mut().remove(&request.id);
                    self.modified_requests.insert(request.id.clone());
                    self.dirty_at = Some(Instant::now());
                }
            }
        }
    }
    pub fn curl_flush(&mut self) {
        for request in &mut self.ws.requests {
            if let Some(draft) = self.curl.drafts.get_mut(&request.id) {
                if draft.dirty && !draft.error && draft.matches(request) {
                    if let Ok(parsed) = crate::import::command(&draft.source, &draft.shell) {
                        apply(request, parsed);
                        self.modified_requests.insert(request.id.clone());
                    }
                }
            }
        }
    }
    pub fn curl_panel(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        request: &mut Request,
        external_changed: bool,
    ) -> bool {
        let t = self.theme;
        w::line(ui, r.left_top(), r.left_bottom(), t.border);
        w::medium(
            ui,
            pos2(r.left() + 16., r.top() + 20.),
            "Code snippet",
            13.,
            t.text,
        );
        if w::icon_button(
            ui,
            rect(r.right() - 34., r.top() + 6., 26., 26.),
            Icon::Close,
            "Close code snippet",
            t,
        )
        .clicked()
        {
            self.curl.open = false;
        }
        let draft = self.curl.drafts.entry(request.id.clone()).or_default();
        if draft.source.is_empty() && !draft.dirty || external_changed && !draft.dirty {
            draft.regenerate(request);
        }
        if external_changed && draft.dirty {
            // Don't let an old delayed parse overwrite a newer edit in the request UI.
            draft.conflict();
        }
        let mut area = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("curl-panel", &request.id))
                .max_rect(r.shrink2(vec2(14., 44.)))
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        area.set_clip_rect(r.intersect(ui.clip_rect()));
        area.horizontal(|ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("shell")
                .selected_text(match draft.shell.as_str() {
                    "bash" => "Bash",
                    "cmd" => "CMD",
                    "powershell" => "PowerShell",
                    _ => "Auto-detect shell",
                })
                .show_ui(ui, |ui| {
                    for (key, label) in [
                        ("auto", "Auto-detect shell"),
                        ("bash", "Bash"),
                        ("cmd", "CMD"),
                        ("powershell", "PowerShell"),
                    ] {
                        changed |= ui
                            .selectable_value(&mut draft.shell, key.into(), label)
                            .changed();
                    }
                });
            if changed {
                draft.expected = Some(crate::engine::curl(request, &BTreeMap::new()));
                draft.edit();
            }
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(draft.source.clone());
            }
            if ui.small_button("Regenerate").clicked() {
                draft.regenerate(request);
            }
        });
        egui::ScrollArea::both()
            .id_salt("command-source")
            .max_height((r.height() - 160.).max(100.))
            .show(&mut area, |ui| {
                if ui
                    .add(
                        egui::TextEdit::multiline(&mut draft.source)
                            .id_salt(("command-text", &request.id))
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(12)
                            .hint_text("Paste cURL or gRPCurl"),
                    )
                    .changed()
                {
                    draft.expected = Some(crate::engine::curl(request, &BTreeMap::new()));
                    draft.edit();
                }
            });
        let changed = draft.tick(request, ui.ctx());
        area.add_space(10.);
        area.colored_label(if draft.error { t.red } else { t.muted }, &draft.message);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_edit_preserves_identity_metadata_and_rejects_incomplete_input() {
        let mut r = Request {
            name: "My request".into(),
            url: "https://example.test/old".into(),
            collection: "c".into(),
            folder: "f".into(),
            path: Some("x.req".into()),
            seq: 7.,
            docs: "docs".into(),
            pre_script: "// script".into(),
            headers: vec![Pair {
                enabled: false,
                ..Pair::new("disabled", "kept")
            }],
            ..Default::default()
        };
        let id = r.id.clone();
        let parsed =
            crate::import::curl("curl -XPOST https://example.test/new --json '{\"v\":2}'").unwrap();
        apply(&mut r, parsed);
        assert_eq!(r.id, id);
        assert_eq!(r.name, "My request");
        assert_eq!(r.seq, 7.);
        assert_eq!(r.folder, "f");
        assert_eq!(r.docs, "docs");
        assert_eq!(r.pre_script, "// script");
        assert_eq!(r.body, "{\"v\":2}");
        assert!(r.headers.iter().any(|p| p.key == "disabled" && !p.enabled));
        let original = r.clone();
        let mut draft = Draft::default();
        draft.source = "curl --url 'unfinished".into();
        draft.edit();
        draft.edited = Instant::now() - Duration::from_secs(1);
        let ctx = egui::Context::default();
        assert!(!draft.tick(&mut r, &ctx));
        for _ in 0..100 {
            if draft.pending.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            assert!(!draft.tick(&mut r, &ctx));
        }
        assert!(draft.error);
        assert!(draft.dirty);
        assert_eq!(r, original);
        draft.source = "curl https://example.test/complete".into();
        draft.edit();
        draft.edited = Instant::now() - Duration::from_secs(1);
        draft.tick(&mut r, &ctx);
        for _ in 0..100 {
            if draft.pending.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            draft.tick(&mut r, &ctx);
        }
        assert_eq!(r.url, "https://example.test/complete");
        assert!(!draft.dirty);
    }
    #[test]
    fn grpc_and_raw_query_commands_roundtrip() {
        let r=crate::import::curl("grpcurl -plaintext -H 'x-id: one' -H 'x-id: two' -import-path 'C:/schemas' -proto api.proto -d '{\"v\":1}' localhost:9000 demo.API/Call").unwrap();
        assert_eq!(r.badge(), "gRPC");
        assert_eq!(r.headers.len(), 2);
        assert!(!r.grpc_tls_enabled(&BTreeMap::new()));
        let after = crate::import::curl(&crate::engine::curl(&r, &BTreeMap::new())).unwrap();
        assert_eq!(after.grpc_method, r.grpc_method);
        assert_eq!(after.proto_files, r.proto_files);
        assert_eq!(after.proto_import_paths, r.proto_import_paths);
        assert_eq!(after.body, r.body);
        let r=crate::import::curl("curl 'https://example.test/items?q=a%20b&q=c%2Bd&flag&empty=' --data-raw '@literal' -XPOST").unwrap();
        assert_eq!(
            crate::engine::request_url(&r, &BTreeMap::new())
                .unwrap()
                .query(),
            Some("q=a%20b&q=c%2Bd&flag&empty=")
        );
        let after = crate::import::curl(&crate::engine::curl(&r, &BTreeMap::new())).unwrap();
        assert_eq!(after.url, r.url);
        assert_eq!(after.body, "@literal");
    }
    #[test]
    fn removing_flags_clears_transport_options_but_keeps_import_metadata() {
        let mut r = crate::import::curl(
            "curl --http2-prior-knowledge --post301 --digest -u user:pass https://example.test",
        )
        .unwrap();
        r.portable["saved_example"] = serde_json::json!({"body":"keep"});
        apply(
            &mut r,
            crate::import::curl("curl https://example.test").unwrap(),
        );
        assert!(r.portable["http_version"].is_null());
        assert!(r.portable["http_follow_original_method"].is_null());
        assert!(r.portable["http_auth_scheme"].is_null());
        assert_eq!(r.portable["saved_example"]["body"], "keep");
        assert_eq!(r.auth.kind, "none");
    }
    #[test]
    fn delayed_command_cannot_overwrite_a_newer_request_edit() {
        let mut r = crate::import::curl("curl https://example.test/initial").unwrap();
        let mut draft = Draft::default();
        draft.regenerate(&r);
        draft.source = "curl https://example.test/command".into();
        draft.edit();
        draft.edited = Instant::now() - Duration::from_secs(1);
        let ctx = egui::Context::default();
        draft.tick(&mut r, &ctx);
        r.url = "https://example.test/newer-ui-edit".into();
        for _ in 0..100 {
            if draft.pending.is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
            assert!(!draft.tick(&mut r, &ctx));
        }
        assert!(draft.error);
        assert_eq!(r.url, "https://example.test/newer-ui-edit");
    }
}
