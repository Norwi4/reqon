use crate::model::{Request, resolve};
use std::collections::BTreeMap;

fn url_tls(url: &str) -> Option<bool> {
    match url
        .trim()
        .split_once("://")?
        .0
        .to_ascii_lowercase()
        .as_str()
    {
        "https" | "grpcs" => Some(true),
        "http" | "grpc" => Some(false),
        _ => None,
    }
}

impl Request {
    pub fn grpc_tls_enabled(&self, vars: &BTreeMap<String, String>) -> bool {
        self.grpc_tls
            .unwrap_or_else(|| url_tls(&resolve(&self.url, vars)).unwrap_or(false))
    }

    pub fn set_grpc_tls(&mut self, enabled: bool) {
        self.grpc_tls = Some(enabled);
        if let Some((scheme, host)) = self.url.trim().split_once("://") {
            let scheme = match (scheme.to_ascii_lowercase().as_str(), enabled) {
                ("http" | "https", true) => "https",
                ("http" | "https", false) => "http",
                ("grpc" | "grpcs", true) => "grpcs",
                ("grpc" | "grpcs", false) => "grpc",
                _ => return,
            };
            self.url = format!("{scheme}://{host}");
        }
    }

    pub fn sync_grpc_tls_from_url(&mut self) {
        // Typing an explicit scheme takes precedence over the previous lock state.
        // Bare host:port and templates retain the selected transport.
        if url_tls(&self.url).is_some() {
            self.grpc_tls = None;
        }
    }

    pub fn grpc_endpoint(&self, vars: &BTreeMap<String, String>) -> anyhow::Result<String> {
        let resolved = resolve(&self.url, vars);
        let url = resolved.trim();
        let host = if let Some((scheme, host)) = url.split_once("://") {
            anyhow::ensure!(
                matches!(
                    scheme.to_ascii_lowercase().as_str(),
                    "http" | "https" | "grpc" | "grpcs"
                ),
                "Unsupported gRPC URL scheme: {scheme}"
            );
            host
        } else {
            url
        };
        anyhow::ensure!(!host.is_empty(), "Enter a gRPC server URL");
        Ok(format!(
            "{}://{host}",
            if self.grpc_tls_enabled(vars) {
                "https"
            } else {
                "http"
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RequestKind;

    #[test]
    fn lock_toggles_bare_hosts_without_changing_certificate_verification() {
        let vars = BTreeMap::new();
        let mut request = Request {
            url: "localhost:50051".into(),
            ..Default::default()
        };
        assert!(!request.grpc_tls_enabled(&vars));
        request.set_grpc_tls(true);
        assert_eq!(request.url, "localhost:50051");
        assert_eq!(
            request.grpc_endpoint(&vars).unwrap(),
            "https://localhost:50051"
        );
        assert!(request.verify_tls);
        request.set_grpc_tls(false);
        assert_eq!(
            request.grpc_endpoint(&vars).unwrap(),
            "http://localhost:50051"
        );
        assert!(request.verify_tls);
        request.url.clear();
        request.set_grpc_tls(true);
        request.url = "[::1]:50051".into();
        request.sync_grpc_tls_from_url();
        assert_eq!(request.grpc_endpoint(&vars).unwrap(), "https://[::1]:50051");
    }

    #[test]
    fn url_schemes_and_templates_match_the_lock_and_transport() {
        let vars = BTreeMap::from([("server".into(), "https://localhost:50051".into())]);
        for url in [
            "https://localhost:50051",
            "grpcs://localhost:50051",
            "HTTPS://localhost:50051",
            "{{server}}",
        ] {
            let mut request = Request {
                url: url.into(),
                ..Default::default()
            };
            assert!(request.grpc_tls_enabled(&vars));
            assert_eq!(
                request.grpc_endpoint(&vars).unwrap(),
                "https://localhost:50051"
            );
            request.set_grpc_tls(false);
            assert!(!request.grpc_tls_enabled(&vars));
            assert_eq!(
                request.grpc_endpoint(&vars).unwrap(),
                "http://localhost:50051"
            );
            if url == "{{server}}" {
                assert_eq!(request.url, url);
            }
        }
        let mut request = Request {
            url: "grpc://localhost:50051".into(),
            ..Default::default()
        };
        request.set_grpc_tls(true);
        assert_eq!(request.url, "grpcs://localhost:50051");
        request.url = "http://localhost:50052".into();
        request.sync_grpc_tls_from_url();
        assert!(!request.grpc_tls_enabled(&vars));
        assert_eq!(request.grpc_tls, None);
        request.url = "ftp://localhost".into();
        assert!(request.grpc_endpoint(&vars).is_err());
    }

    #[test]
    fn tls_choice_survives_workspace_collection_files_and_protocol_switches() {
        let mut request = Request {
            protocol: "grpc".into(),
            method: "GRPC".into(),
            url: "localhost:443".into(),
            ..Default::default()
        };
        for tls in [Some(true), Some(false), None] {
            request.grpc_tls = tls;
            let json = serde_json::to_string(&request).unwrap();
            let restored: Request = serde_json::from_str(&json).unwrap();
            assert_eq!(restored.grpc_tls, tls);
            let bru = crate::storage::to_bru(&request);
            assert_eq!(crate::storage::parse_bru(&bru).unwrap().grpc_tls, tls);
            let yaml = crate::yaml::serialize(&request).unwrap();
            assert_eq!(crate::yaml::parse(&yaml).unwrap().grpc_tls, tls);
            request.switch_kind(RequestKind::Http);
            assert_eq!(request.grpc_tls, None);
            request.switch_kind(RequestKind::Grpc);
            assert_eq!(request.grpc_tls, tls);
        }
        let old: Request =
            serde_json::from_str(r#"{"protocol":"grpc","url":"https://localhost:443"}"#).unwrap();
        assert!(old.grpc_tls_enabled(&BTreeMap::new()));
    }
}
