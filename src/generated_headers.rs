//! Generated HTTP headers shared by previews and the wire.
#[cfg(feature = "desktop")]
pub use crate::generated_headers_ui::draw;
use crate::model::*;
use std::collections::BTreeMap;
pub const USER_AGENT: &str = concat!("Reqon/", env!("CARGO_PKG_VERSION"));
const CALCULATED: &str = "Calculated when request is sent";

const SETTINGS: &str = "generated_headers";

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(default)]
pub struct Override {
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HeaderRow {
    pub name: String,
    pub automatic: String,
    pub setting: Override,
}

impl HeaderRow {
    pub fn editable(&self) -> bool {
        configurable(&self.name)
    }
}

pub(crate) fn configurable(name: &str) -> bool {
    [
        "host",
        "cache-control",
        "user-agent",
        "accept",
        "accept-encoding",
        "connection",
        "content-type",
    ]
    .iter()
    .any(|key| name.eq_ignore_ascii_case(key))
}

fn overrides(request: &Request) -> BTreeMap<String, Override> {
    serde_json::from_value(request.portable[SETTINGS].clone()).unwrap_or_default()
}

pub fn rows(request: &Request, vars: &BTreeMap<String, String>) -> Vec<HeaderRow> {
    let settings = overrides(request);
    preview(request, vars)
        .into_iter()
        .map(|(name, automatic)| {
            let setting = settings
                .get(&name.to_ascii_lowercase())
                .cloned()
                .unwrap_or_default();
            HeaderRow {
                name,
                automatic,
                setting,
            }
        })
        .collect()
}

pub fn set_override(request: &mut Request, name: &str, setting: Override) {
    if !configurable(name) {
        return;
    }
    let mut settings = overrides(request);
    let key = name.to_ascii_lowercase();
    if setting == Override::default() {
        settings.remove(&key);
    } else {
        settings.insert(key, setting);
    }
    if settings.is_empty() {
        if let Some(object) = request.portable.as_object_mut() {
            object.remove(SETTINGS);
            if object.is_empty() {
                request.portable = serde_json::Value::Null;
            }
        }
    } else {
        if !request.portable.is_object() {
            request.portable = serde_json::json!({});
        }
        request.portable[SETTINGS] = serde_json::to_value(settings).unwrap();
    }
}

/// Only suppress automatic values. An enabled explicit header always wins.
pub fn omitted(request: &Request, vars: &BTreeMap<String, String>) -> Vec<String> {
    overrides(request)
        .into_iter()
        .filter_map(|(name, setting)| {
            (setting.disabled && configurable(&name) && !manual(request, &name, vars))
                .then_some(name)
        })
        .collect()
}

fn manual(request: &Request, name: &str, vars: &BTreeMap<String, String>) -> bool {
    request
        .headers
        .iter()
        .any(|p| p.enabled && resolve(&p.key, vars).eq_ignore_ascii_case(name))
}

pub fn custom_values(request: &Request, vars: &BTreeMap<String, String>) -> Vec<Pair> {
    rows(request, vars)
        .into_iter()
        .filter_map(|row| {
            if !row.editable() || row.setting.disabled {
                return None;
            }
            row.setting.value.map(|value| Pair::new(row.name, value))
        })
        .collect()
}

pub fn enabled_count(request: &Request, vars: &BTreeMap<String, String>) -> usize {
    rows(request, vars)
        .iter()
        .filter(|row| !row.setting.disabled)
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overrides_round_trip_and_reset_without_freezing_automatic_values() {
        let mut request = Request {
            url: "https://{{host}}/".into(),
            ..Default::default()
        };
        set_override(
            &mut request,
            "User-Agent",
            Override {
                disabled: true,
                value: Some("{{agent}}".into()),
            },
        );
        set_override(
            &mut request,
            "Accept",
            Override {
                disabled: true,
                value: None,
            },
        );
        for restored in [
            crate::storage::parse_bru(&crate::storage::to_bru(&request)).unwrap(),
            crate::yaml::parse(&crate::yaml::serialize(&request).unwrap()).unwrap(),
            serde_json::from_str::<Request>(&serde_json::to_string(&request).unwrap()).unwrap(),
        ] {
            assert_eq!(restored.portable, request.portable);
            let vars = BTreeMap::from([("host".into(), "one.test".into())]);
            let preview = rows(&restored, &vars);
            assert!(
                preview
                    .iter()
                    .any(|r| r.name == "Host" && r.automatic == "one.test")
            );
            assert!(preview.iter().any(|r| r.name == "User-Agent"
                && r.setting.disabled
                && r.setting.value.as_deref() == Some("{{agent}}")));
        }
        request
            .headers
            .push(Pair::new("aCcEpT", "application/json"));
        assert_eq!(omitted(&request, &BTreeMap::new()), vec!["user-agent"]);
        set_override(&mut request, "Accept", Override::default());
        set_override(&mut request, "USER-AGENT", Override::default());
        assert!(request.portable.is_null());
        assert!(custom_values(&request, &BTreeMap::new()).is_empty());
    }

    #[test]
    fn curl_uses_overrides_and_suppresses_its_own_defaults() {
        let mut request = Request {
            url: "https://example.test/".into(),
            method: "POST".into(),
            body_kind: BodyKind::Json,
            body: "{}".into(),
            ..Default::default()
        };
        for name in ["Accept", "Accept-Encoding", "Content-Type", "Host"] {
            set_override(
                &mut request,
                name,
                Override {
                    disabled: true,
                    value: None,
                },
            );
        }
        set_override(
            &mut request,
            "User-Agent",
            Override {
                value: Some("{{agent}}".into()),
                ..Default::default()
            },
        );
        let output = crate::engine::curl(
            &request,
            &BTreeMap::from([("agent".into(), "Custom client".into())]),
        );
        for name in ["accept", "accept-encoding", "content-type", "host"] {
            assert!(output.contains(&format!("'{name}:'")), "{output}");
        }
        assert!(output.contains("User-Agent: Custom client"));
        assert!(!output.contains("Content-Type: application/json"));
        assert!(!output.contains(USER_AGENT));
        let imported = crate::import::curl(&output).unwrap();
        assert_eq!(
            omitted(&imported, &BTreeMap::new()),
            vec!["accept", "accept-encoding", "content-type", "host"]
        );
        let imported = crate::import::curl("curl https://example.test -H 'Authorization: Bearer demo' -H 'Accept:' -H 'User-Agent;' --compressed").unwrap();
        assert_eq!(omitted(&imported, &BTreeMap::new()), vec!["accept"]);
        assert!(
            imported
                .headers
                .iter()
                .any(|p| p.key == "User-Agent" && p.value.is_empty())
        );
    }
    #[test]
    fn previews_follow_body_method_and_manual_header_overrides() {
        let mut r = Request {
            url: "https://example.test:8443/items".into(),
            body_kind: BodyKind::Json,
            body: "{}".into(),
            ..Default::default()
        };
        let vars = BTreeMap::new();
        let rows = preview(&r, &vars);
        assert!(
            rows.iter()
                .any(|(k, v)| k == "Host" && v == "example.test:8443")
        );
        assert!(
            !rows
                .iter()
                .any(|(k, _)| k == "Content-Type" || k == "Content-Length")
        );
        r.method = "POST".into();
        r.headers.push(Pair::new("user-agent", "Custom"));
        let rows = preview(&r, &vars);
        assert!(
            !rows
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("user-agent"))
        );
        assert!(
            rows.iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
        r.portable = serde_json::json!({"http_version":"HTTP/2"});
        assert!(!preview(&r, &vars).iter().any(|(k, _)| k == "Connection"));
    }
}
pub fn defaults(request: &Request) -> Vec<(&'static str, &'static str)> {
    let mut rows = vec![
        ("Cache-Control", "no-cache"),
        ("User-Agent", USER_AGENT),
        ("Accept", "*/*"),
        ("Accept-Encoding", "gzip, deflate"),
    ];
    if request.portable["http_version"] != "HTTP/2" {
        rows.push(("Connection", "keep-alive"));
    }
    rows
}
pub fn preview(request: &Request, vars: &BTreeMap<String, String>) -> Vec<(String, String)> {
    let mut rows = vec![];
    let mut add = |name: &str, value: String| {
        if !request
            .headers
            .iter()
            .any(|p| p.enabled && resolve(&p.key, vars).eq_ignore_ascii_case(name))
        {
            rows.push((name.into(), value));
        }
    };
    let host = crate::engine::request_url(request, vars)
        .ok()
        .map(|u| u[url::Position::BeforeHost..url::Position::AfterPort].to_owned());
    add("Host", host.unwrap_or_else(|| CALCULATED.into()));
    for (key, value) in defaults(request) {
        add(key, value.into());
    }
    let kind = request.http_body_kind(vars);
    if kind != BodyKind::None {
        let media = match kind {
            BodyKind::Json | BodyKind::Graphql => "application/json",
            BodyKind::Xml => "application/xml",
            BodyKind::Sparql => "application/sparql-query",
            BodyKind::Form => "application/x-www-form-urlencoded",
            BodyKind::Multipart => "multipart/form-data; boundary=<calculated>",
            BodyKind::Binary => "application/octet-stream",
            _ => "text/plain",
        };
        add("Content-Type", media.into());
        add("Content-Length", CALCULATED.into());
    } else if matches!(request.http_method().as_str(), "POST" | "PUT" | "PATCH") {
        add("Content-Length", "0".into());
    }
    if !matches!(
        request.auth.kind.as_str(),
        "" | "none" | "inherit" | "apikey"
    ) {
        add("Authorization", CALCULATED.into());
    }
    if request.auth.kind == "apikey" && !request.auth.in_query && !request.auth.key.is_empty() {
        add(&resolve(&request.auth.key, vars), CALCULATED.into());
    }
    rows
}
