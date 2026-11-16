use crate::model::*;
use anyhow::Result;

pub fn command(source: &str, shell: &str) -> Result<Request> {
    let imported = crate::command_import::parse(source, shell)?;
    let mut r = Request {
        name: imported.data["name"]
            .as_str()
            .unwrap_or("Imported request")
            .into(),
        ..Default::default()
    };
    crate::interchange::apply_getman(&imported.data, &mut r, std::path::Path::new("."))?;
    if !imported.notes.is_empty() {
        r.portable["__command_notes"] = serde_json::json!(imported.notes);
    }
    // Imported query text is already encoded. Keep duplicate keys, flags, pluses
    // and percent escapes exactly as pasted instead of re-encoding them.
    if r.protocol != "grpc" {
        r.forward_auth = imported.data["http_follow_auth"].as_bool().unwrap_or(false);
        r.encode_url = false;
        if let Some(query) = imported.data["imported_query"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            r.url.push('?');
            r.url.push_str(query);
        }
        crate::query::update_rows(&mut r);
        if r.body_kind == BodyKind::Text && crate::json_comments::value(&r.body).is_ok() {
            r.body_kind = BodyKind::Json;
        }
    }
    Ok(r)
}
pub fn curl(source: &str) -> Result<Request> {
    command(source, "auto")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curl_json_auth_quotes() {
        let r=curl("curl --request POST 'http://localhost/x' -H 'X-Test: value:with:colons' --data-raw '{\"name\":\"hello world\"}' -u 'me:pass'").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.auth.username, "me");
        assert_eq!(r.headers[0].value, "value:with:colons");
        assert_eq!(r.body, "{\"name\":\"hello world\"}");
        assert_eq!(r.body_kind, BodyKind::Json);
    }
    #[test]
    fn imports_generated_form_and_network_settings() {
        let request = Request {
            url: "https://example.com/items".into(),
            method: "POST".into(),
            body_kind: BodyKind::Form,
            form: vec![Pair::new("name", "a b&c")],
            timeout: 1250,
            timeout_inherit: false,
            verify_tls: false,
            max_redirects: 3,
            proxy: "http://localhost:8888".into(),
            auth: Auth {
                kind: "digest".into(),
                username: "user".into(),
                password: "p'a\\ss".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let source = crate::engine::curl(&request, &Default::default());
        let imported = curl(&source).unwrap();
        assert_eq!(imported.body_kind, BodyKind::Form);
        assert_eq!(imported.form[0].value, "a b&c");
        assert_eq!(imported.timeout, 1250);
        assert_eq!(imported.auth, request.auth);
        assert!(!imported.verify_tls);
        assert_eq!(imported.proxy, request.proxy);
    }
}
