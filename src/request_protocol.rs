//! Protocol switching preserves independent request drafts without UI state.
use crate::model::{BodyKind, ProtocolDraft, Request, RequestKind};

impl ProtocolDraft {
    fn take(request: &mut Request) -> Self {
        use std::mem::take;
        Self {
            method: take(&mut request.method),
            url: take(&mut request.url),
            params: take(&mut request.params),
            path_params: take(&mut request.path_params),
            headers: take(&mut request.headers),
            body_kind: take(&mut request.body_kind),
            body: take(&mut request.body),
            form: take(&mut request.form),
            graphql_variables: take(&mut request.graphql_variables),
            grpc_method: take(&mut request.grpc_method),
            grpc_tls: take(&mut request.grpc_tls),
            proto_files: take(&mut request.proto_files),
            protocol_messages: take(&mut request.protocol_messages),
        }
    }

    fn apply(self, request: &mut Request) {
        request.method = self.method;
        request.url = self.url;
        request.params = self.params;
        request.path_params = self.path_params;
        request.headers = self.headers;
        request.body_kind = self.body_kind;
        request.body = self.body;
        request.form = self.form;
        request.graphql_variables = self.graphql_variables;
        request.grpc_method = self.grpc_method;
        request.grpc_tls = self.grpc_tls;
        request.proto_files = self.proto_files;
        request.protocol_messages = self.protocol_messages;
    }
}

impl Request {
    pub fn switch_kind(&mut self, kind: RequestKind) -> bool {
        let previous = RequestKind::of(self);
        if kind == previous {
            return false;
        }
        let next = self.protocol_drafts.remove(&kind).unwrap_or_else(|| {
            // Seed a new protocol with the endpoint and headers/metadata. Shared
            // auth, scripts, variables, TLS and timeout settings stay on Request.
            let mut draft = Request {
                url: protocol_url(&self.url, kind),
                headers: self.headers.clone(),
                ..Default::default()
            };
            match kind {
                RequestKind::Http => {}
                RequestKind::Graphql => {
                    draft.method = "POST".into();
                    draft.body_kind = BodyKind::Graphql;
                }
                RequestKind::Grpc => {
                    draft.method = "GRPC".into();
                    draft.body_kind = BodyKind::Json;
                    draft.body = if self.body_kind == BodyKind::Json {
                        self.body.clone()
                    } else {
                        "{}".into()
                    };
                }
                RequestKind::Ws => {
                    draft.method = "WS".into();
                    draft.body_kind = BodyKind::Text;
                }
            }
            crate::query::update_rows(&mut draft);
            ProtocolDraft::take(&mut draft)
        });
        // Move large bodies between editors instead of cloning on every switch.
        let old = ProtocolDraft::take(self);
        self.protocol_drafts.insert(previous, old);
        next.apply(self);
        self.protocol = match kind {
            RequestKind::Http => "http",
            RequestKind::Graphql => "graphql",
            RequestKind::Grpc => "grpc",
            RequestKind::Ws => "ws",
        }
        .into();
        self.proto_import_paths.clear();
        true
    }
}

fn protocol_url(url: &str, kind: RequestKind) -> String {
    // Preserve variables and paths. Only replace an explicitly supplied scheme.
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.into();
    };
    let scheme = match (kind, scheme) {
        (RequestKind::Ws, "http" | "grpc") => "ws",
        (RequestKind::Ws, "https" | "grpcs") => "wss",
        (_, "ws" | "grpc") => "http",
        (_, "wss" | "grpcs") => "https",
        _ => scheme,
    };
    format!("{scheme}://{rest}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::*, storage};

    #[test]
    fn protocol_switch_preserves_identity_shared_settings_and_independent_editors() {
        let mut request = Request {
            name: "Orders — заказы".into(),
            method: "PATCH".into(),
            url: "https://localhost:8443/orders/:id?limit=3".into(),
            body_kind: BodyKind::Multipart,
            body: "original HTTP body".into(),
            form: vec![Pair::new("attachment", "@report.txt")],
            headers: vec![Pair::new("X-Client", "reqon")],
            path_params: vec![Pair::new("id", "42")],
            auth: Auth {
                kind: "bearer".into(),
                token: "test-token".into(),
                ..Default::default()
            },
            collection: "collection".into(),
            folder: "Folder".into(),
            path: Some("Folder/Orders.req".into()),
            verify_tls: false,
            pre_script: "console.log('shared');".into(),
            ..Default::default()
        };
        crate::query::update_rows(&mut request);
        let original = request.clone();
        assert!(request.switch_kind(RequestKind::Grpc));
        assert_eq!(request.id, original.id);
        assert_eq!(request.name, original.name);
        assert_eq!(request.path, original.path);
        assert_eq!(request.collection, original.collection);
        assert_eq!(request.folder, original.folder);
        assert_eq!(request.auth, original.auth);
        assert_eq!(request.pre_script, original.pre_script);
        assert!(!request.verify_tls);
        assert_eq!(request.method, "GRPC");
        assert_eq!(request.body_kind, BodyKind::Json);
        assert_eq!(request.body, "{}");
        assert_eq!(request.headers, original.headers);
        request.url = "http://localhost:50051".into();
        request.body = r#"{"orderId":42}"#.into();
        request.grpc_method = "orders.Orders/Get".into();
        request.proto_files = vec!["orders.proto".into()];
        request.headers.push(Pair::new("grpc-header", "value"));
        let grpc = request.clone();
        assert!(request.switch_kind(RequestKind::Http));
        let mut restored = request.clone();
        restored.protocol_drafts.clear();
        assert_eq!(restored, original);
        assert!(request.switch_kind(RequestKind::Grpc));
        assert_eq!(request.body, grpc.body);
        assert_eq!(request.url, grpc.url);
        assert_eq!(request.grpc_method, grpc.grpc_method);
        assert_eq!(request.proto_files, grpc.proto_files);
        assert_eq!(request.headers, grpc.headers);
        assert!(!request.switch_kind(RequestKind::Grpc));
    }

    #[test]
    fn protocol_drafts_survive_save_reload_and_external_file_refresh() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = Request {
            path: Some(dir.path().join("Request.req")),
            collection: "c".into(),
            method: "POST".into(),
            body_kind: BodyKind::Json,
            body: r#"{"http":true}"#.into(),
            ..Default::default()
        };
        request.switch_kind(RequestKind::Grpc);
        request.body = r#"{"grpc":true}"#.into();
        storage::save_request(&request).unwrap();
        let source = storage::to_bru(&request);
        assert!(!source.contains("protocol_drafts"));
        assert!(!source.contains("http\":true"));
        let id = request.id.clone();
        let mut ws = Workspace::default();
        ws.saved_fingerprints
            .insert(id.clone(), crate::disk_sync::fingerprint(&source));
        ws.requests.push(request);
        storage::save(dir.path(), &ws).unwrap();
        let mut restored = storage::load(dir.path()).unwrap();
        assert_eq!(restored.requests[0].body, r#"{"grpc":true}"#);
        assert!(
            restored.requests[0]
                .protocol_drafts
                .contains_key(&RequestKind::Http)
        );
        let mut disk = crate::yaml::read_request(ws.requests[0].path.as_ref().unwrap()).unwrap();
        disk.collection = "c".into();
        disk.name = "Renamed externally".into();
        let mut saved = std::collections::HashMap::from([(id, source)]);
        assert_eq!(
            crate::disk_sync::merge_requests(&mut restored, "c", vec![disk], &mut saved),
            (1, 0)
        );
        restored.requests[0].switch_kind(RequestKind::Http);
        assert_eq!(restored.requests[0].body, r#"{"http":true}"#);
        assert_eq!(restored.requests[0].method, "POST");
        assert_eq!(restored.requests[0].name, "Renamed externally");
    }

    #[test]
    fn all_protocols_keep_separate_bodies_and_use_compatible_url_schemes() {
        let mut request = Request {
            url: "https://{{host}}/api".into(),
            ..Default::default()
        };
        for kind in RequestKind::ALL {
            request.switch_kind(kind);
            assert_eq!(RequestKind::of(&request), kind);
            request.body = format!("{} body", kind.label());
        }
        assert_eq!(request.url, "wss://{{host}}/api");
        for kind in RequestKind::ALL {
            request.switch_kind(kind);
            assert_eq!(request.body, format!("{} body", kind.label()));
            assert_eq!(RequestKind::of(&request), kind);
        }
        assert_eq!(
            protocol_url("{{baseUrl}}/orders", RequestKind::Grpc),
            "{{baseUrl}}/orders"
        );
        assert_eq!(
            protocol_url("grpcs://localhost:443", RequestKind::Http),
            "https://localhost:443"
        );
    }

    #[test]
    fn converted_yaml_and_bru_files_reopen_with_only_the_selected_protocol() {
        let mut request = Request {
            method: "POST".into(),
            url: "http://localhost:50051".into(),
            body_kind: BodyKind::Json,
            body: r#"{"http":true}"#.into(),
            ..Default::default()
        };
        request.file_format = "yml".into();
        request.original = crate::yaml::serialize(&request).unwrap();
        request.switch_kind(RequestKind::Grpc);
        request.grpc_method = "example.Echo/Call".into();
        request.body = r#"{"message":"gRPC"}"#.into();
        let yaml = crate::yaml::serialize(&request).unwrap();
        let value: serde_json::Value = serde_yaml::from_str(&yaml).unwrap();
        assert!(value.get("http").is_none());
        assert!(value.get("protocol_drafts").is_none());
        let loaded = crate::yaml::parse(&yaml).unwrap();
        assert_eq!(RequestKind::of(&loaded), RequestKind::Grpc);
        assert_eq!(loaded.body, request.body);
        assert_eq!(
            loaded.grpc_method.trim_start_matches('/'),
            "example.Echo/Call"
        );
        request.file_format = "req".into();
        request.original = storage::to_bru(&request);
        request.switch_kind(RequestKind::Http);
        let loaded = storage::parse_bru(&storage::to_bru(&request)).unwrap();
        assert_eq!(RequestKind::of(&loaded), RequestKind::Http);
        assert_eq!(loaded.method, "POST");
        assert_eq!(loaded.body, r#"{"http":true}"#);
    }
}
