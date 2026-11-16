//! Generated HTTP headers shared by previews and the wire.
#[cfg(feature = "desktop")]
pub use crate::generated_headers_ui::draw;
use crate::model::*;
use std::collections::BTreeMap;
pub const USER_AGENT: &str = concat!("Reqon/", env!("CARGO_PKG_VERSION"));
const CALCULATED: &str = "Calculated when request is sent";

#[cfg(test)]
mod tests {
    use super::*;
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
