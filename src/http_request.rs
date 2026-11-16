use crate::model::{BodyKind, Request, resolve};
use std::collections::BTreeMap;

impl Request {
    pub fn http_method(&self) -> String {
        let method = self.method.trim();
        let upper = method.to_ascii_uppercase();
        match upper.as_str() {
            "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "TRACE"
            | "CONNECT" => upper,
            _ => method.into(), // Extension method tokens are case-sensitive.
        }
    }

    pub fn http_body_allowed(&self) -> bool {
        !["GET", "HEAD", "TRACE"]
            .iter()
            .any(|method| self.method.trim().eq_ignore_ascii_case(method))
    }

    /// Determine what goes on the wire without destroying the editor's body draft.
    /// Resolve variables first: an empty JSON variable is also an absent body.
    pub fn http_body_kind(&self, vars: &BTreeMap<String, String>) -> BodyKind {
        if !self.http_body_allowed() {
            return BodyKind::None;
        }
        let empty = match self.body_kind {
            BodyKind::None => true,
            BodyKind::Json | BodyKind::Binary => resolve(&self.body, vars).trim().is_empty(),
            BodyKind::Text | BodyKind::Xml | BodyKind::Sparql => {
                resolve(&self.body, vars).is_empty()
            }
            BodyKind::Form | BodyKind::Multipart => {
                !self.form.iter().any(|p| p.enabled && !p.key.is_empty())
            }
            BodyKind::Graphql => false,
        };
        if empty {
            BodyKind::None
        } else {
            self.body_kind.clone()
        }
    }

    pub fn http_header_is_sent(&self, name: &str) -> bool {
        // Framing is calculated from the actual payload, including resolved
        // variables and multipart boundaries. Stale lengths can hang a server.
        !name.eq_ignore_ascii_case("content-length")
            && !name.eq_ignore_ascii_case("transfer-encoding")
            && (self.http_body_allowed()
                || (!name.eq_ignore_ascii_case("content-type")
                    && !name.eq_ignore_ascii_case("content-encoding")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{Engine, curl},
        model::{Auth, Pair},
    };
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    #[derive(Debug)]
    struct WireRequest {
        method: String,
        path: String,
        headers: BTreeMap<String, String>,
        body: Vec<u8>,
    }

    #[tokio::test]
    async fn generated_headers_can_be_disabled_edited_and_reset_on_the_wire() {
        use crate::generated_headers::{Override, set_override};
        let names = [
            "Host",
            "Cache-Control",
            "User-Agent",
            "Accept",
            "Accept-Encoding",
            "Connection",
            "Content-Type",
        ];
        let (url, server) = server(vec![response(200, "", "ok"); 4]).await;
        let engine = Engine::default();
        let mut request = Request {
            url,
            method: "POST".into(),
            body_kind: BodyKind::Json,
            body: "{}".into(),
            ..Default::default()
        };
        for name in names {
            set_override(
                &mut request,
                name,
                Override {
                    disabled: true,
                    value: None,
                },
            );
        }
        let first = engine.execute(request.clone(), BTreeMap::new()).await;
        assert!(first.error.is_empty(), "{}", first.error);
        // Manual values win even over a disabled generated row, including variable names.
        request.headers = vec![
            Pair::new("{{header}}", "manual"),
            Pair::new("ACCEPT", "text/plain"),
        ];
        let second = engine
            .execute(
                request.clone(),
                BTreeMap::from([("header".into(), "User-Agent".into())]),
            )
            .await;
        assert!(second.error.is_empty(), "{}", second.error);
        request.headers.clear();
        for name in names {
            set_override(&mut request, name, Override::default());
        }
        set_override(
            &mut request,
            "User-Agent",
            Override {
                value: Some("{{agent}}".into()),
                ..Default::default()
            },
        );
        set_override(
            &mut request,
            "Host",
            Override {
                value: Some("override.test".into()),
                ..Default::default()
            },
        );
        set_override(
            &mut request,
            "Content-Type",
            Override {
                value: Some("application/custom".into()),
                ..Default::default()
            },
        );
        let third = engine
            .execute(
                request.clone(),
                BTreeMap::from([("agent".into(), "edited".into())]),
            )
            .await;
        assert!(third.error.is_empty(), "{}", third.error);
        for name in names {
            set_override(&mut request, name, Override::default());
        }
        let fourth = engine.execute(request, BTreeMap::new()).await;
        assert!(fourth.error.is_empty(), "{}", fourth.error);
        let wire = server.await.unwrap();
        for name in names {
            assert!(
                !wire[0].headers.contains_key(&name.to_ascii_lowercase()),
                "{name}: {:?}",
                wire[0]
            );
        }
        assert_eq!(wire[0].body, b"{}");
        assert_eq!(wire[0].headers["content-length"], "2");
        assert_eq!(wire[1].headers["user-agent"], "manual");
        assert_eq!(wire[1].headers["accept"], "text/plain");
        assert!(!wire[1].headers.contains_key("accept-encoding"));
        assert_eq!(wire[2].headers["user-agent"], "edited");
        assert_eq!(wire[2].headers["host"], "override.test");
        assert_eq!(wire[2].headers["content-type"], "application/custom");
        assert_eq!(
            wire[3].headers["user-agent"],
            crate::generated_headers::USER_AGENT
        );
        assert_eq!(wire[3].headers["accept"], "*/*");
        assert_eq!(wire[3].headers["content-type"], "application/json");
        assert!(wire[3].headers["host"].starts_with("127.0.0.1:"));
    }

    #[tokio::test]
    async fn generated_headers_reach_the_server_and_custom_values_win() {
        let (url, server) = server(vec![response(200, "", "ok"), response(200, "", "ok")]).await;
        let engine = Engine::default();
        let mut request = Request {
            url,
            method: "POST".into(),
            body_kind: BodyKind::Json,
            body: "{\"a\":1}".into(),
            ..Default::default()
        };
        let first = engine.execute(request.clone(), BTreeMap::new()).await;
        assert!(first.error.is_empty(), "{}", first.error);
        request.method = "GET".into();
        request.headers = vec![
            Pair::new("user-agent", "Custom client"),
            Pair::new("Accept", "text/plain"),
            Pair::new("cache-control", "max-age=60"),
        ];
        let second = engine.execute(request, BTreeMap::new()).await;
        assert!(second.error.is_empty(), "{}", second.error);
        let sent = server.await.unwrap();
        assert_eq!(
            sent[0].headers["user-agent"],
            crate::generated_headers::USER_AGENT
        );
        assert_eq!(sent[0].headers["accept"], "*/*");
        assert_eq!(sent[0].headers["accept-encoding"], "gzip, deflate");
        assert_eq!(sent[0].headers["cache-control"], "no-cache");
        assert_eq!(sent[0].headers["connection"], "keep-alive");
        assert_eq!(sent[0].headers["content-length"], "7");
        assert_eq!(sent[1].headers["user-agent"], "Custom client");
        assert_eq!(sent[1].headers["accept"], "text/plain");
        assert_eq!(sent[1].headers["cache-control"], "max-age=60");
        assert!(sent[1].body.is_empty());
        assert!(!sent[1].headers.contains_key("content-type"));
    }

    fn response(status: u16, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status} Test\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    async fn server(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<WireRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            let mut received = Vec::new();
            for response in responses {
                let (mut stream, _) =
                    tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 4096];
                let (end, headers) = loop {
                    let n = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        stream.read(&mut chunk),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    assert!(n > 0, "Connection closed before request headers");
                    bytes.extend_from_slice(&chunk[..n]);
                    if let Some(end) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                        let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
                        let headers: BTreeMap<String, String> = head
                            .lines()
                            .skip(1)
                            .filter_map(|l| l.split_once(':'))
                            .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().into()))
                            .collect();
                        break (end + 4, headers);
                    }
                };
                assert!(
                    !headers.contains_key("transfer-encoding"),
                    "Buffered bodies must have deterministic framing"
                );
                let length = headers
                    .get("content-length")
                    .map(|s| s.parse::<usize>().unwrap())
                    .unwrap_or(0);
                while bytes.len() < end + length {
                    let n = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        stream.read(&mut chunk),
                    )
                    .await
                    .unwrap()
                    .unwrap();
                    assert!(n > 0, "Connection closed before body");
                    bytes.extend_from_slice(&chunk[..n]);
                }
                let head = String::from_utf8(bytes[..end].to_vec()).unwrap();
                let mut line = head.lines().next().unwrap().split_whitespace();
                received.push(WireRequest {
                    method: line.next().unwrap().into(),
                    path: line.next().unwrap().into(),
                    headers,
                    body: bytes[end..].to_vec(),
                });
                stream.write_all(response.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            }
            received
        });
        (url, task)
    }

    fn request(method: &str, url: &str) -> Request {
        Request {
            method: method.into(),
            url: url.into(),
            proxy_disabled: true,
            timeout: 3000,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn json_comments_are_preserved_in_draft_but_absent_on_wire_and_in_curl() {
        let engine = Engine::default();
        let (url, received) = server(vec![response(200, "", "{}")]).await;
        let mut r = request("POST", &url);
        r.body_kind = BodyKind::Json;
        r.body = "// payload\n{\n\"url\":\"https://x/*keep*/\",\n//\"disabled\":true,\n\"n\":123456789012345678901234567890,/* tail */\n}".into();
        let draft = r.body.clone();
        let clean = crate::json_comments::normalize(&draft)
            .unwrap()
            .into_owned();
        assert!(curl(&r, &BTreeMap::new()).contains(&clean));
        let _ = engine.execute(r.clone(), BTreeMap::new()).await;
        let wire = received.await.unwrap();
        assert_eq!(String::from_utf8(wire[0].body.clone()).unwrap(), clean);
        assert_eq!(r.body, draft);
        let parsed: serde_json::Value = serde_json::from_slice(&wire[0].body).unwrap();
        assert!(parsed.get("disabled").is_none());
    }

    #[tokio::test]
    async fn get_head_trace_ignore_body_drafts_and_stale_payload_headers() {
        let engine = Engine::default();
        for method in ["GET", "HEAD", "TRACE", " get "] {
            for kind in BodyKind::ALL {
                let (url, received) = server(vec![response(200, "", "")]).await;
                let mut r = request(method, &format!("{url}/items?q=one"));
                r.body_kind = kind;
                r.body = "not JSON or an existing file".into();
                r.form = vec![Pair::new("upload", "@not-an-existing-file")];
                r.headers = vec![
                    Pair::new("Content-Type", "application/json"),
                    Pair::new("Content-Length", "999"),
                    Pair::new("Transfer-Encoding", "chunked"),
                    Pair::new("Content-Encoding", "gzip"),
                    Pair::new("X-Keep", "yes"),
                ];
                r.auth = Auth {
                    kind: "bearer".into(),
                    token: "test-token".into(),
                    ..Default::default()
                };
                let draft = r.clone();
                let reply = engine.execute(r.clone(), BTreeMap::new()).await;
                assert!(
                    reply.error.is_empty(),
                    "{method}/{:?}: {}",
                    r.body_kind,
                    reply.error
                );
                assert_eq!(reply.status, 200);
                let wire = received.await.unwrap().remove(0);
                assert_eq!(wire.method, r.http_method());
                assert_eq!(wire.path, "/items?q=one");
                assert!(wire.body.is_empty());
                assert!(!wire.headers.contains_key("content-type"));
                assert!(!wire.headers.contains_key("content-encoding"));
                assert_eq!(
                    wire.headers
                        .get("content-length")
                        .map(String::as_str)
                        .unwrap_or("0"),
                    "0"
                );
                assert_eq!(wire.headers["x-keep"], "yes");
                assert_eq!(wire.headers["authorization"], "Bearer test-token");
                let code = curl(&r, &BTreeMap::new());
                assert!(
                    !code.contains("--data")
                        && !code.contains("--form")
                        && !code.to_ascii_lowercase().contains("content-type")
                );
                assert_eq!(
                    r, draft,
                    "Sending and generating code must preserve body drafts"
                );
            }
        }
    }

    #[tokio::test]
    async fn empty_bodies_are_optional_for_post_put_patch_delete_and_options() {
        let engine = Engine::default();
        for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
            for kind in [
                BodyKind::None,
                BodyKind::Json,
                BodyKind::Text,
                BodyKind::Xml,
                BodyKind::Form,
                BodyKind::Multipart,
                BodyKind::Binary,
            ] {
                let (url, received) = server(vec![response(204, "", "")]).await;
                let mut r = request(method, &url);
                r.body_kind = kind;
                r.form = vec![Pair {
                    enabled: false,
                    ..Pair::new("ignored", "@missing")
                }];
                if r.body_kind == BodyKind::Json {
                    r.body = " {{empty}} \n".into();
                }
                let vars = BTreeMap::from([("empty".into(), String::new())]);
                let reply = engine.execute(r.clone(), vars.clone()).await;
                assert!(
                    reply.error.is_empty(),
                    "{method}/{:?}: {}",
                    r.body_kind,
                    reply.error
                );
                assert_eq!(reply.status, 204);
                assert!(reply.body.is_empty());
                let wire = received.await.unwrap().remove(0);
                assert!(wire.body.is_empty());
                assert!(!wire.headers.contains_key("content-type"));
                assert_eq!(wire.method, method);
                let code = curl(&r, &vars);
                assert!(!code.contains("--data") && !code.contains("--form"));
            }
        }
    }

    #[tokio::test]
    async fn write_methods_send_resolved_json_and_recalculate_length() {
        let engine = Engine::default();
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            let status = if method == "POST" { 201 } else { 200 };
            let (url, received) = server(vec![response(
                status,
                "Content-Type: application/json\r\n",
                "{\"ok\":true}",
            )])
            .await;
            let mut r = request(method, &format!("{url}/items/:id?q=old"));
            r.body_kind = BodyKind::Json;
            r.body = r#"{"name":"{{name}}"}"#.into();
            r.params = vec![Pair::new("q", "one two")];
            r.path_params = vec![Pair::new("id", "42")];
            r.headers = vec![
                Pair::new("Content-Length", "1"),
                Pair::new("Transfer-Encoding", "chunked"),
            ];
            let vars = BTreeMap::from([("name".into(), "Привет".into())]);
            let reply = engine.execute(r.clone(), vars.clone()).await;
            assert!(reply.error.is_empty(), "{}", reply.error);
            assert_eq!(reply.status, status);
            let wire = received.await.unwrap().remove(0);
            assert_eq!(wire.method, method);
            assert_eq!(wire.path, "/items/42?q=one%20two");
            assert_eq!(
                serde_json::from_slice::<serde_json::Value>(&wire.body).unwrap()["name"],
                "Привет"
            );
            assert_eq!(wire.headers["content-type"], "application/json");
            assert_eq!(
                wire.headers["content-length"].parse::<usize>().unwrap(),
                wire.body.len()
            );
            assert!(curl(&r, &vars).contains("--data-raw"));
        }
    }

    #[tokio::test]
    async fn write_methods_support_text_xml_forms_multipart_and_binary() {
        let engine = Engine::default();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("payload.bin");
        let data = [0, 1, 2, 255, 10];
        std::fs::write(&file, data).unwrap();
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            for kind in [
                BodyKind::Text,
                BodyKind::Xml,
                BodyKind::Form,
                BodyKind::Multipart,
                BodyKind::Binary,
            ] {
                let (url, received) = server(vec![response(200, "", "OK")]).await;
                let mut r = request(method, &url);
                r.body_kind = kind.clone();
                r.body = match kind {
                    BodyKind::Text => "hello {{name}}".into(),
                    BodyKind::Xml => "<name>{{name}}</name>".into(),
                    BodyKind::Binary => file.to_string_lossy().into_owned(),
                    _ => String::new(),
                };
                r.form = vec![
                    Pair::new("name", "{{name}}"),
                    Pair::new("name", "a&b"),
                    Pair {
                        enabled: false,
                        ..Pair::new("ignored", "@missing")
                    },
                ];
                if kind == BodyKind::Multipart {
                    r.form
                        .push(Pair::new("file", &format!("@{}", file.display())));
                }
                if kind == BodyKind::Text {
                    r.headers.push(Pair::new("Content-Type", "text/custom"));
                }
                let reply = engine
                    .execute(r, BTreeMap::from([("name".into(), "one two".into())]))
                    .await;
                assert!(reply.error.is_empty(), "{method}/{kind:?}: {}", reply.error);
                let wire = received.await.unwrap().remove(0);
                assert_eq!(wire.method, method);
                match kind {
                    BodyKind::Text => {
                        assert_eq!(wire.body, b"hello one two");
                        assert_eq!(wire.headers["content-type"], "text/custom");
                    }
                    BodyKind::Xml => {
                        assert_eq!(wire.body, b"<name>one two</name>");
                        assert_eq!(wire.headers["content-type"], "application/xml");
                    }
                    BodyKind::Form => {
                        assert_eq!(wire.body, b"name=one+two&name=a%26b");
                        assert_eq!(
                            wire.headers["content-type"],
                            "application/x-www-form-urlencoded"
                        );
                    }
                    BodyKind::Multipart => {
                        let boundary = wire.headers["content-type"]
                            .strip_prefix("multipart/form-data; boundary=")
                            .unwrap();
                        let text = String::from_utf8_lossy(&wire.body);
                        assert!(text.starts_with(&format!("--{boundary}\r\n")));
                        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
                        assert!(text.contains("name=\"name\"\r\n\r\none two"));
                        assert!(text.contains("filename=\"payload.bin\""));
                        assert!(!text.contains("ignored"));
                        assert!(wire.body.windows(data.len()).any(|w| w == data));
                    }
                    BodyKind::Binary => assert_eq!(wire.body, data),
                    _ => unreachable!(),
                }
                assert_eq!(
                    wire.headers["content-length"].parse::<usize>().unwrap(),
                    wire.body.len()
                );
            }
        }
    }

    #[tokio::test]
    async fn invalid_nonempty_json_is_reported_before_any_network_request() {
        let engine = Engine::default();
        for method in ["POST", "PUT", "PATCH", "DELETE"] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut r = request(
                method,
                &format!("http://{}", listener.local_addr().unwrap()),
            );
            r.body_kind = BodyKind::Json;
            r.body = "{broken".into();
            let reply = engine.execute(r, BTreeMap::new()).await;
            assert!(reply.error.contains("Invalid JSON request body"));
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(30), listener.accept())
                    .await
                    .is_err()
            );
        }
    }

    #[tokio::test]
    async fn redirects_preserve_or_drop_method_and_body_together() {
        let engine = Engine::default();
        for method in ["POST", "PUT", "DELETE"] {
            for status in [301, 302, 303, 307, 308] {
                let (url, received) = server(vec![
                    response(status, "Location: /target\r\n", ""),
                    response(204, "", ""),
                ])
                .await;
                let mut r = request(method, &format!("{url}/start"));
                r.body_kind = BodyKind::Json;
                r.body = "{\"id\":1}".into();
                let reply = engine.execute(r, BTreeMap::new()).await;
                assert!(reply.error.is_empty(), "{method}/{status}: {}", reply.error);
                assert_eq!(reply.status, 204);
                let wire = received.await.unwrap();
                assert_eq!(wire[0].method, method);
                assert_eq!(wire[1].path, "/target");
                let becomes_get =
                    status == 303 || ([301, 302].contains(&status) && method == "POST");
                assert_eq!(wire[1].method, if becomes_get { "GET" } else { method });
                if becomes_get {
                    assert!(wire[1].body.is_empty());
                    assert!(!wire[1].headers.contains_key("content-type"));
                } else {
                    assert_eq!(wire[1].body, wire[0].body);
                }
            }
        }
    }

    #[tokio::test]
    async fn pre_request_script_method_changes_use_the_same_body_policy() {
        let engine = Engine::default();
        let (url, received) = server(vec![response(200, "", ""), response(201, "", "")]).await;
        let mut r = request("POST", &url);
        r.body_kind = BodyKind::Json;
        r.body = "invalid draft".into();
        r.pre_script = "req.setMethod('GET');".into();
        assert!(
            engine
                .execute(r.clone(), BTreeMap::new())
                .await
                .error
                .is_empty()
        );
        r.method = "GET".into();
        r.pre_script = "req.setMethod('POST'); req.setBody({id:42});".into();
        assert!(engine.execute(r, BTreeMap::new()).await.error.is_empty());
        let wire = received.await.unwrap();
        assert_eq!(wire[0].method, "GET");
        assert!(wire[0].body.is_empty());
        assert_eq!(wire[1].method, "POST");
        assert_eq!(wire[1].body, b"{\"id\":42}");
    }
}
