use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub fn id() -> String {
    uuid::Uuid::new_v4().to_string()
}
fn yes() -> bool {
    true
}
fn timeout() -> u64 {
    30_000
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Pair {
    #[serde(default = "yes")]
    pub enabled: bool,
    pub key: String,
    pub value: String,
    #[serde(default)]
    pub secret: bool,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub annotations: Vec<String>,
}
impl Serialize for Pair {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut s = serializer.serialize_struct("Pair", 7)?;
        s.serialize_field("enabled", &self.enabled)?;
        s.serialize_field("key", &self.key)?;
        // Opaque vault refs are portable metadata, never plaintext values.
        s.serialize_field(
            "value",
            if self.secret && crate::auth_vault::reference_id(&self.value).is_none() {
                ""
            } else {
                &self.value
            },
        )?;
        s.serialize_field("secret", &self.secret)?;
        s.serialize_field("description", &self.description)?;
        s.serialize_field("kind", &self.kind)?;
        s.serialize_field("annotations", &self.annotations)?;
        s.end()
    }
}
impl Default for Pair {
    fn default() -> Self {
        Self {
            enabled: true,
            key: String::new(),
            value: String::new(),
            secret: false,
            description: String::new(),
            kind: String::new(),
            annotations: vec![],
        }
    }
}
impl Pair {
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum BodyKind {
    #[default]
    None,
    Json,
    Text,
    Xml,
    Sparql,
    Form,
    Multipart,
    Graphql,
    Binary,
}
impl BodyKind {
    #[cfg(test)]
    pub const ALL: [Self; 9] = [
        Self::None,
        Self::Json,
        Self::Text,
        Self::Xml,
        Self::Sparql,
        Self::Form,
        Self::Multipart,
        Self::Graphql,
        Self::Binary,
    ];
    pub fn label(&self) -> &str {
        match self {
            Self::None => "No Body",
            Self::Json => "JSON",
            Self::Text => "TEXT",
            Self::Xml => "XML",
            Self::Sparql => "SPARQL",
            Self::Form => "Form URL Encoded",
            Self::Multipart => "Multipart Form",
            Self::Graphql => "GraphQL",
            Self::Binary => "File / Binary",
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Auth {
    pub kind: String,
    pub token: String,
    pub username: String,
    pub password: String,
    pub key: String,
    pub value: String,
    pub in_query: bool,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Request {
    pub id: String,
    pub name: String,
    pub method: String,
    pub seq: f64,
    pub tags: Vec<String>,
    pub protocol: String,
    pub url: String,
    pub params: Vec<Pair>,
    pub path_params: Vec<Pair>,
    pub headers: Vec<Pair>,
    pub body_kind: BodyKind,
    pub body: String,
    pub form: Vec<Pair>,
    pub graphql_variables: String,
    pub grpc_method: String,
    /// None follows the URL scheme; a lock click explicitly selects the transport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc_tls: Option<bool>,
    pub proto_files: Vec<String>,
    #[serde(default)]
    pub proto_import_paths: Vec<String>,
    pub protocol_messages: Vec<Pair>,
    /// Local editor state for other protocols; never written into .req/.yml files.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub protocol_drafts: BTreeMap<RequestKind, ProtocolDraft>,
    /// Opaque interchange fields from getman, retained across save/export.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub portable: serde_json::Value,
    pub auth: Auth,
    pub vars: Vec<Pair>,
    pub response_vars: Vec<Pair>,
    pub pre_script: String,
    pub post_script: String,
    pub tests: String,
    pub assertions: Vec<Pair>,
    pub docs: String,
    pub timeout: u64,
    pub timeout_inherit: bool,
    pub encode_url: bool,
    pub max_redirects: usize,
    pub forward_auth: bool,
    pub verify_tls: bool,
    pub follow_redirects: bool,
    pub proxy: String,
    #[serde(skip)]
    pub proxy_disabled: bool,
    pub client_certificate: String,
    pub client_key: String,
    pub ca_certificate: String,
    #[serde(skip)]
    pub client_identity: Option<Vec<u8>>,
    #[serde(skip)]
    pub default_ca: Option<bool>,
    pub path: Option<PathBuf>,
    pub folder: String,
    pub collection: String,
    pub original: String,
    pub file_format: String,
    #[serde(skip)]
    pub inherited_collection_vars: Vec<Pair>,
    #[serde(skip)]
    pub script_scope: Option<ScriptScope>,
    #[serde(skip)]
    pub script_depth: usize,
    /// Per-run input; never written into saved request files or history.
    #[serde(skip)]
    pub iteration_data: BTreeMap<String, serde_json::Value>,
    #[serde(skip)]
    pub run_iteration: u32,
    #[serde(skip)]
    pub run_iteration_count: u32,
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum RequestKind {
    #[default]
    Http,
    Graphql,
    Grpc,
    Ws,
}
impl RequestKind {
    pub const ALL: [Self; 4] = [Self::Http, Self::Graphql, Self::Grpc, Self::Ws];
    pub fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Graphql => "GraphQL",
            Self::Grpc => "gRPC",
            Self::Ws => "WebSocket",
        }
    }
    pub fn of(request: &Request) -> Self {
        match request.protocol.as_str() {
            "grpc" => Self::Grpc,
            "ws" => Self::Ws,
            "graphql" => Self::Graphql,
            _ if request.body_kind == BodyKind::Graphql => Self::Graphql,
            _ => Self::Http,
        }
    }
}
impl Request {
    /// A presentation label; `method` can contain an RPC path for gRPC.
    pub fn badge(&self) -> String {
        match RequestKind::of(self) {
            RequestKind::Grpc => "gRPC".into(),
            RequestKind::Graphql => "GQL".into(),
            RequestKind::Ws => "WS".into(),
            RequestKind::Http => self.method.to_uppercase(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ProtocolDraft {
    pub method: String,
    pub url: String,
    pub params: Vec<Pair>,
    pub path_params: Vec<Pair>,
    pub headers: Vec<Pair>,
    pub body_kind: BodyKind,
    pub body: String,
    pub form: Vec<Pair>,
    pub graphql_variables: String,
    pub grpc_method: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grpc_tls: Option<bool>,
    pub proto_files: Vec<String>,
    pub protocol_messages: Vec<Pair>,
}
impl Default for Request {
    fn default() -> Self {
        Self {
            id: id(),
            name: "Untitled".into(),
            method: "GET".into(),
            seq: 1.,
            tags: vec![],
            protocol: "http".into(),
            url: String::new(),
            params: vec![],
            path_params: vec![],
            headers: vec![],
            body_kind: BodyKind::None,
            body: String::new(),
            form: vec![],
            graphql_variables: "{}".into(),
            grpc_method: String::new(),
            grpc_tls: None,
            proto_files: vec![],
            proto_import_paths: vec![],
            protocol_messages: vec![],
            protocol_drafts: BTreeMap::new(),
            portable: serde_json::Value::Null,
            auth: Auth::default(),
            vars: vec![],
            response_vars: vec![],
            pre_script: String::new(),
            post_script: String::new(),
            tests: String::new(),
            assertions: vec![],
            docs: String::new(),
            timeout: timeout(),
            timeout_inherit: true,
            encode_url: false,
            max_redirects: 5,
            forward_auth: true,
            verify_tls: true,
            follow_redirects: true,
            proxy: String::new(),
            proxy_disabled: false,
            client_certificate: String::new(),
            client_key: String::new(),
            ca_certificate: String::new(),
            client_identity: None,
            default_ca: None,
            path: None,
            folder: String::new(),
            collection: String::new(),
            original: String::new(),
            file_format: "req".into(),
            inherited_collection_vars: vec![],
            script_scope: None,
            script_depth: 0,
            iteration_data: BTreeMap::new(),
            run_iteration: 0,
            run_iteration_count: 0,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Environment {
    pub name: String,
    pub vars: Vec<Pair>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub path: PathBuf,
    #[serde(default)]
    pub environments: Vec<Environment>,
    #[serde(default)]
    pub vars: Vec<Pair>,
    #[serde(default)]
    pub folders: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub error: String,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default)]
    pub cancelled: bool,
    pub status: u16,
    pub reason: String,
    pub elapsed_ms: u128,
    pub size: usize,
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub trailers: Vec<(String, String)>,
    pub body: String,
    #[serde(default)]
    pub bytes: Vec<u8>,
    pub url: String,
    pub error: String,
    pub timeline: Vec<String>,
    pub tests: Vec<TestResult>,
    pub logs: Vec<String>,
    pub runtime_vars: BTreeMap<String, String>,
    #[serde(default)]
    pub script_effects: ScriptEffects,
    #[serde(default)]
    pub deleted_vars: Vec<String>,
    /// Effective authentication values used by this execution, kept only in
    /// memory so history and CLI reports can redact inherited/template secrets.
    #[serde(skip)]
    pub redaction_values: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ScriptScope {
    pub environment: BTreeMap<String, serde_json::Value>,
    pub globals: BTreeMap<String, serde_json::Value>,
    pub collection: BTreeMap<String, serde_json::Value>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ScriptEffects {
    pub environment: BTreeMap<String, serde_json::Value>,
    pub globals: BTreeMap<String, serde_json::Value>,
    pub collection: BTreeMap<String, serde_json::Value>,
    pub deleted_environment: Vec<String>,
    pub deleted_globals: Vec<String>,
    pub persist_environment: bool,
    pub skip: bool,
    pub stop: bool,
    pub next: Option<String>,
}
impl ScriptEffects {
    pub fn merge(&mut self, other: Self) {
        for key in &other.deleted_environment {
            self.environment.remove(key);
        }
        for key in &other.deleted_globals {
            self.globals.remove(key);
        }
        self.deleted_environment
            .retain(|key| !other.environment.contains_key(key));
        self.deleted_globals
            .retain(|key| !other.globals.contains_key(key));
        self.deleted_environment.extend(other.deleted_environment);
        self.deleted_globals.extend(other.deleted_globals);
        self.environment.extend(other.environment);
        self.globals.extend(other.globals);
        self.collection.extend(other.collection);
        self.persist_environment |= other.persist_environment;
        self.skip |= other.skip;
        self.stop |= other.stop;
        if other.next.is_some() {
            self.next = other.next;
        }
    }
}
pub fn typed_variables(rows: &[Pair]) -> BTreeMap<String, serde_json::Value> {
    rows.iter()
        .filter(|p| p.enabled && !p.key.is_empty())
        .map(|p| {
            (
                p.key.clone(),
                if !p.kind.is_empty() && p.kind != "string" {
                    serde_json::from_str(&p.value)
                        .unwrap_or(serde_json::Value::String(p.value.clone()))
                } else {
                    serde_json::Value::String(p.value.clone())
                },
            )
        })
        .collect()
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct History {
    pub at: u64,
    pub request: Request,
    pub reply: Reply,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Workspace {
    pub schema_version: u32,
    pub name: String,
    pub collections: Vec<Collection>,
    pub requests: Vec<Request>,
    pub open_tabs: Vec<String>,
    pub active: Option<String>,
    pub dark: bool,
    pub sidebar_width: f32,
    pub split: f32,
    pub request_height: f32,
    pub request_width: f32,
    pub side_by_side: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub pane_layouts: BTreeMap<String, crate::ui_state::PaneLayout>,
    pub selected_environment: String,
    pub globals: Vec<Pair>,
    pub environments: Vec<Environment>,
    pub workspace_environment: String,
    pub git_drafts: BTreeMap<String, crate::ui_state::GitDraft>,
    pub history: Vec<History>,
    pub saved_fingerprints: BTreeMap<String, String>,
    pub collection_drafts: BTreeMap<String, Request>,
    pub last_request_kind: BTreeMap<String, RequestKind>,
    pub docs: String,
    pub preferences: Preferences,
}
impl Default for Workspace {
    fn default() -> Self {
        Self {
            schema_version: 3,
            name: "My Workspace".into(),
            collections: vec![],
            requests: vec![],
            open_tabs: vec![],
            active: None,
            dark: true,
            sidebar_width: 250.,
            split: 0.5,
            request_height: 380.,
            request_width: 0.,
            side_by_side: false,
            pane_layouts: BTreeMap::new(),
            selected_environment: String::new(),
            globals: vec![],
            environments: vec![],
            workspace_environment: String::new(),
            git_drafts: BTreeMap::new(),
            history: vec![],
            saved_fingerprints: BTreeMap::new(),
            collection_drafts: BTreeMap::new(),
            last_request_kind: BTreeMap::new(),
            docs: String::new(),
            preferences: Preferences::default(),
        }
    }
}
impl Workspace {
    pub fn next_request_seq(&self, collection: &str, folder: &str) -> f64 {
        self.requests
            .iter()
            .filter(|r| r.collection == collection && r.folder == folder && r.seq.is_finite())
            .map(|r| r.seq)
            .max_by(f64::total_cmp)
            .unwrap_or(0.)
            + 1.
    }

    pub fn metadata_snapshot(&self) -> Self {
        Self {
            schema_version: self.schema_version,
            name: self.name.clone(),
            collections: self.collections.clone(),
            requests: vec![],
            history: vec![],
            open_tabs: self.open_tabs.clone(),
            active: self.active.clone(),
            dark: self.dark,
            sidebar_width: self.sidebar_width,
            split: self.split,
            request_height: self.request_height,
            request_width: self.request_width,
            side_by_side: self.side_by_side,
            pane_layouts: self.pane_layouts.clone(),
            selected_environment: self.selected_environment.clone(),
            globals: self.globals.clone(),
            environments: self.environments.clone(),
            workspace_environment: self.workspace_environment.clone(),
            git_drafts: self.git_drafts.clone(),
            saved_fingerprints: self.saved_fingerprints.clone(),
            collection_drafts: self.collection_drafts.clone(),
            last_request_kind: self.last_request_kind.clone(),
            docs: self.docs.clone(),
            preferences: self.preferences.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub verify_tls: bool,
    pub custom_ca: bool,
    pub ca_file: String,
    pub keep_default_ca: bool,
    pub store_cookies: bool,
    pub send_cookies: bool,
    pub system_browser_oauth: bool,
    pub autosave: bool,
    pub autosave_delay: u64,
    pub timeout: u64,
    pub proxy: String,
    pub default_location: String,
    pub editor_font_size: f32,
    pub appearance: crate::preference_model::Appearance,
    pub proxy_settings: crate::preference_model::Proxy,
    pub certificates: Vec<crate::preference_model::Certificate>,
    pub features: crate::preference_model::Features,
    pub keybindings: crate::preference_model::Keybindings,
    pub cache: crate::preference_model::Cache,
    pub secret_providers: Vec<crate::preference_model::SecretProvider>,
    pub github: crate::preference_model::Github,
    pub ai: crate::preference_model::Ai,
    pub about: crate::preference_model::About,
    #[serde(skip)]
    pub credentials: BTreeMap<String, String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            verify_tls: true,
            custom_ca: false,
            ca_file: String::new(),
            keep_default_ca: true,
            store_cookies: true,
            send_cookies: true,
            system_browser_oauth: true,
            autosave: true,
            autosave_delay: 1000,
            timeout: 0,
            proxy: String::new(),
            default_location: String::new(),
            editor_font_size: 13.,
            appearance: Default::default(),
            proxy_settings: Default::default(),
            certificates: vec![],
            features: Default::default(),
            keybindings: Default::default(),
            cache: Default::default(),
            secret_providers: vec![],
            github: Default::default(),
            ai: Default::default(),
            about: Default::default(),
            credentials: BTreeMap::new(),
        }
    }
}
impl Preferences {
    pub fn apply(&self, r: &mut Request) {
        if !self.verify_tls {
            r.verify_tls = false;
        }
        if self.custom_ca && r.ca_certificate.is_empty() {
            r.ca_certificate = self.ca_file.clone();
        }
        if r.proxy.is_empty() && !r.proxy_disabled {
            r.proxy = self.proxy.clone();
        }
        if r.timeout_inherit {
            r.timeout = self.timeout;
        }
    }
}

pub fn variables(
    globals: &[Pair],
    collection: &[Pair],
    env: &[Pair],
    local: &[Pair],
) -> BTreeMap<String, String> {
    let mut result = BTreeMap::new();
    for rows in [globals, collection, env, local] {
        for p in rows.iter().filter(|p| p.enabled && !p.key.is_empty()) {
            let name = variable_name(&p.key);
            if !name.is_empty() {
                result.insert(name.into(), p.value.clone());
            }
        }
    }
    result
}
pub fn variable_name(name: &str) -> &str {
    let name = name.trim();
    name.strip_prefix("{{")
        .and_then(|v| v.strip_suffix("}}"))
        .unwrap_or(name)
        .trim()
}
pub fn resolve(input: &str, vars: &BTreeMap<String, String>) -> String {
    let mut result = input.to_owned();
    for _ in 0..10 {
        let mut out = String::new();
        let mut rest = result.as_str();
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            if let Some(end) = rest[start + 2..].find("}}") {
                let key = rest[start + 2..start + 2 + end].trim();
                if let Some(value) = vars.get(key) {
                    out.push_str(value);
                } else if let Some(name) = key.strip_prefix("process.env.") {
                    out.push_str(
                        &std::env::var(name)
                            .unwrap_or_else(|_| rest[start..start + end + 4].to_owned()),
                    );
                } else {
                    out.push_str(&rest[start..start + end + 4]);
                }
                rest = &rest[start + end + 4..];
            } else {
                out.push_str(&rest[start..]);
                rest = "";
                break;
            }
        }
        out.push_str(rest);
        if out == result {
            break;
        }
        result = out;
    }
    result
}
pub fn safe_name(input: &str) -> String {
    let s: String = input
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let s = s.trim().trim_matches('.');
    if s.is_empty() {
        "Untitled".into()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn variable_precedence_and_recursion() {
        let v = variables(
            &[Pair::new("host", "old")],
            &[],
            &[
                Pair::new("host", "localhost"),
                Pair::new("base", "http://{{host}}"),
            ],
            &[Pair::new("host", "127.0.0.1")],
        );
        assert_eq!(
            resolve("{{base}}/x/{{missing}}", &v),
            "http://127.0.0.1/x/{{missing}}"
        );
    }
    #[test]
    fn cycles_terminate() {
        let v = BTreeMap::from([("x".into(), "{{y}}".into()), ("y".into(), "{{x}}".into())]);
        assert!(resolve("{{x}}", &v).len() < 10);
    }
}
