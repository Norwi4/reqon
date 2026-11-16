//! Application preferences. Credentials are held in memory and persisted only by secrets.rs.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Appearance {
    pub mode: String,
    pub light: String,
    pub dark: String,
    pub font: String,
    pub zoom: u16,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            mode: "legacy".into(),
            light: "light".into(),
            dark: "dark".into(),
            font: "default".into(),
            zoom: 100,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Proxy {
    pub mode: String,
    pub protocol: String,
    pub host: String,
    pub port: String,
    pub auth: bool,
    pub username: String,
    pub bypass: String,
    pub pac_source: String,
    pub pac_file: bool,
}
impl Default for Proxy {
    fn default() -> Self {
        Self {
            mode: "legacy".into(),
            protocol: "http".into(),
            host: String::new(),
            port: String::new(),
            auth: false,
            username: String::new(),
            bypass: String::new(),
            pac_source: String::new(),
            pac_file: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Certificate {
    pub id: String,
    pub domain: String,
    pub kind: String,
    pub cert: String,
    pub key: String,
    pub pfx: String,
    pub enabled: bool,
}
impl Default for Certificate {
    fn default() -> Self {
        Self {
            id: crate::model::id(),
            domain: String::new(),
            kind: "cert".into(),
            cert: String::new(),
            key: String::new(),
            pfx: String::new(),
            enabled: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Features {
    pub api_spec: bool,
    pub git: bool,
    pub file_explorer: bool,
    #[serde(alias = "bruno_json")]
    pub reqon_json: bool,
    pub mock_server: bool,
}
impl Default for Features {
    fn default() -> Self {
        Self {
            api_spec: true,
            git: true,
            file_explorer: true,
            reqon_json: false,
            mock_server: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Keybindings {
    pub enabled: bool,
    /// An empty override explicitly disables an individual binding.
    pub bindings: BTreeMap<String, String>,
}
impl Default for Keybindings {
    fn default() -> Self {
        Self {
            enabled: true,
            bindings: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Cache {
    pub file: bool,
    pub ssl: bool,
}
impl Default for Cache {
    fn default() -> Self {
        Self {
            file: false,
            ssl: true,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SecretProvider {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub fields: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiProvider {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub enabled: bool,
    pub models: Vec<AiModel>,
}
impl Default for AiProvider {
    fn default() -> Self {
        Self {
            id: crate::model::id(),
            name: String::new(),
            base_url: String::new(),
            enabled: false,
            models: vec![],
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AiModel {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Ai {
    pub enabled: bool,
    pub providers: Vec<AiProvider>,
    pub default_model: String,
    pub autocomplete: bool,
    pub autocomplete_model: String,
    pub trigger: String,
    pub redact_headers: bool,
    pub redact_body: bool,
    pub redact_variables: bool,
    pub redact_response: bool,
    pub headers: Vec<String>,
    pub variables: Vec<String>,
}
impl Default for Ai {
    fn default() -> Self {
        Self {
            enabled: false,
            providers: vec![
                AiProvider {
                    id: "openai".into(),
                    name: "OpenAI".into(),
                    base_url: "https://api.openai.com/v1".into(),
                    models: builtin_models("openai"),
                    ..Default::default()
                },
                AiProvider {
                    id: "anthropic".into(),
                    name: "Anthropic".into(),
                    base_url: "https://api.anthropic.com/v1".into(),
                    models: builtin_models("anthropic"),
                    ..Default::default()
                },
            ],
            default_model: String::new(),
            autocomplete: true,
            autocomplete_model: String::new(),
            trigger: "debounced".into(),
            redact_headers: true,
            redact_body: true,
            redact_variables: true,
            redact_response: true,
            headers: vec![],
            variables: vec![],
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Github {
    pub connected: bool,
    pub username: String,
    pub endpoint: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct About {
    pub automatic_updates: bool,
    pub telemetry: bool,
}

fn builtin_models(provider: &str) -> Vec<AiModel> {
    let models: &[(&str, &str)] = if provider == "openai" {
        &[
            ("gpt-4o-mini", "GPT-4o Mini"),
            ("gpt-4o", "GPT-4o"),
            ("gpt-5", "GPT-5"),
            ("gpt-5-mini", "GPT-5 Mini"),
        ]
    } else {
        &[
            ("claude-opus-4-7", "Claude Opus 4.7"),
            ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
            ("claude-haiku-4-5-20251001", "Claude Haiku 4.5"),
        ]
    };
    models
        .iter()
        .map(|(id, label)| AiModel {
            id: (*id).into(),
            label: (*label).into(),
            enabled: true,
        })
        .collect()
}
