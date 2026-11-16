use crate::{bru, model::*, storage};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, fs, path::Path};

fn merge(rows: &mut Vec<Pair>, more: &[Pair], case_insensitive: bool) {
    for p in more.iter().filter(|p| p.enabled && !p.key.is_empty()) {
        rows.retain(|v| {
            if case_insensitive {
                !v.key.eq_ignore_ascii_case(&p.key)
            } else {
                v.key != p.key
            }
        });
        rows.push(p.clone());
    }
}
pub fn parent(path: &Path) -> Result<Option<Request>> {
    let alternate = if path
        .file_name()
        .is_some_and(|s| s == "collection.req" || s == "collection.bru")
    {
        path.with_file_name("opencollection.yml")
    } else {
        path.with_extension("yml")
    };
    if alternate.exists() {
        return Ok(Some(crate::yaml::read_request(&alternate)?));
    }
    if crate::yaml::is_yaml(path) && path.exists() {
        return Ok(Some(crate::yaml::read_request(path)?));
    }
    let resolved = crate::native_format::source_path(path);
    let path = resolved.as_path();
    if !path.exists() {
        return Ok(None);
    }
    let source =
        fs::read_to_string(path).with_context(|| format!("Cannot read {}", path.display()))?;
    let blocks: BTreeMap<_, _> = storage::blocks(&source).into_iter().collect();
    let auth = blocks
        .get("auth")
        .map(|s| bru::pairs(s))
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.key == "mode")
        .map(|p| p.value)
        .unwrap_or_else(|| "inherit".into());
    let mut r = storage::parse_bru(&format!(
        "get {{\n  url: http://localhost\n  auth: {auth}\n}}\n\n{source}"
    ))?;
    r.path = Some(path.to_owned());
    r.protocol = if matches!(
        path.file_name().and_then(|n| n.to_str()),
        Some("collection.req" | "collection.bru")
    ) {
        "collection"
    } else {
        "folder"
    }
    .into();
    crate::auth_vault::resolve(&mut r, false)?;
    Ok(Some(r))
}
fn combine(parts: impl Iterator<Item = String>) -> String {
    parts
        .filter(|s| !s.trim().is_empty())
        .map(|s| format!("await (async () => {{\n{s}\n}})();"))
        .collect::<Vec<_>>()
        .join("\n\n")
}
/// Resolve the closest collection and each enclosing folder. This runs on a worker.
#[cfg(test)]
pub fn prepare(request: Request) -> Result<Request> {
    prepare_scoped(request, &[], &[], &BTreeMap::new())
}
pub fn prepare_scoped(
    request: Request,
    globals: &[Pair],
    env: &[Pair],
    runtime: &BTreeMap<String, String>,
) -> Result<Request> {
    prepare_with_collection(request, globals, env, runtime, &BTreeMap::new())
}
pub fn prepare_with_collection(
    mut request: Request,
    globals: &[Pair],
    env: &[Pair],
    runtime: &BTreeMap<String, String>,
    collection: &BTreeMap<String, serde_json::Value>,
) -> Result<Request> {
    crate::collection_runner::apply_values(&mut request.inherited_collection_vars, collection, &[]);
    let request_path = request.path.clone();
    let Some(dir) = request_path.as_deref().and_then(Path::parent) else {
        return Ok(request);
    };
    let Some(root) = dir
        .ancestors()
        .take(32)
        .find(|p| crate::native_format::is_collection(p))
    else {
        return Ok(request);
    };
    let mut chain = vec![];
    if let Some(mut c) = parent(&root.join("collection.req"))? {
        crate::collection_runner::apply_values(&mut c.vars, collection, &[]);
        request.inherited_collection_vars = std::mem::take(&mut c.vars);
        chain.push(c);
    }
    let mut dirs = dir
        .ancestors()
        .take_while(|p| *p != root)
        .collect::<Vec<_>>();
    dirs.reverse();
    for dir in dirs {
        if let Some(p) = parent(&dir.join("folder.req"))? {
            chain.push(p);
        }
    }
    let sequential = manifest(root)
        .ok()
        .and_then(|v| {
            v.pointer("/scripts/flow")
                .and_then(|s| s.as_str())
                .map(str::to_owned)
        })
        .unwrap_or_else(|| "sequential".into())
        == "sequential";
    let mut headers = vec![];
    let mut vars = vec![];
    let mut response_vars = vec![];
    let mut assertions = vec![];
    let mut auth = Auth::default();
    let inherit_timeout = request.timeout_inherit;
    for p in &chain {
        if request.proxy.is_empty() {
            request.proxy = p.proxy.clone();
        }
        if request.client_certificate.is_empty() {
            request.client_certificate = parent_path(p, &p.client_certificate);
            request.client_key = parent_path(p, &p.client_key);
        }
        if request.ca_certificate.is_empty() {
            request.ca_certificate = parent_path(p, &p.ca_certificate);
        }
        if request.proto_files.is_empty() {
            request.proto_files = p
                .proto_files
                .iter()
                .map(|value| parent_path(p, value))
                .collect();
        }
        if !p.verify_tls {
            request.verify_tls = false;
        }
        if inherit_timeout && !p.timeout_inherit {
            request.timeout = p.timeout;
            request.timeout_inherit = false;
        }
        merge(&mut headers, &p.headers, true);
        merge(&mut vars, &p.vars, false);
        merge(&mut response_vars, &p.response_vars, false);
        merge(&mut assertions, &p.assertions, false);
        if p.auth.kind != "inherit" {
            auth = p.auth.clone();
        }
    }
    merge(&mut headers, &request.headers, true);
    merge(&mut vars, &request.vars, false);
    merge(&mut response_vars, &request.response_vars, false);
    merge(&mut assertions, &request.assertions, false);
    if request.auth.kind == "inherit" {
        request.auth = auth;
    }
    request.headers = headers;
    request.vars = vars;
    request.response_vars = response_vars;
    request.assertions = assertions;
    request.pre_script = combine(
        chain
            .iter()
            .map(|p| p.pre_script.clone())
            .chain([request.pre_script]),
    );
    let mut post = chain
        .iter()
        .map(|p| p.post_script.clone())
        .chain([request.post_script])
        .collect::<Vec<_>>();
    let mut tests = chain
        .iter()
        .map(|p| p.tests.clone())
        .chain([request.tests])
        .collect::<Vec<_>>();
    if !sequential {
        post.reverse();
        tests.reverse();
    }
    request.post_script = combine(post.into_iter());
    request.tests = combine(tests.into_iter());
    let mut vars = variables(
        globals,
        &request.inherited_collection_vars,
        env,
        &request.vars,
    );
    vars.extend(runtime.clone());
    apply_manifest(&mut request, root, &vars)?;
    Ok(request)
}
fn parent_path(parent: &Request, value: &str) -> String {
    if value.is_empty() || value.contains("{{") || Path::new(value).is_absolute() {
        return value.into();
    }
    parent
        .path
        .as_deref()
        .and_then(Path::parent)
        .map(|dir| dir.join(value).to_string_lossy().into_owned())
        .unwrap_or_else(|| value.into())
}

fn manifest(root: &Path) -> Result<serde_json::Value> {
    let path = root.join("opencollection.yml");
    if path.exists() {
        let source: serde_json::Value = serde_yaml::from_slice(&fs::read(path)?)?;
        let mut value = source["config"].clone();
        if !value.is_object() {
            value = serde_json::json!({});
        }
        value["scripts"] = source["extensions"]["bruno"]["scripts"].clone();
        if let Some(certs) = source["config"]["clientCertificates"].as_array() {
            value["clientCertificates"] = serde_json::json!({"certs":certs.iter().map(|c|serde_json::json!({
                "domain":c["domain"],"disabled":c["disabled"],"type":if c["type"]=="pkcs12"{"pfx"}else{"cert"},
                "certFilePath":c["certificateFilePath"],"keyFilePath":c["privateKeyFilePath"],"pfxFilePath":c["pkcs12FilePath"],"passphrase":c["passphrase"]
            })).collect::<Vec<_>>()});
        }
        return Ok(value);
    }
    let path = crate::native_format::manifest(root);
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn apply_manifest(
    request: &mut Request,
    root: &Path,
    vars: &BTreeMap<String, String>,
) -> Result<()> {
    let manifest = manifest(root)?;
    if request.proxy.is_empty() {
        let proxy = &manifest["proxy"];
        let cfg = &proxy["config"];
        if proxy["disabled"] == true && proxy["inherit"] != true {
            request.proxy_disabled = true;
        }
        if proxy["disabled"] != true && proxy["inherit"] != true {
            if let Some(host) = cfg["hostname"].as_str().filter(|s| !s.is_empty()) {
                let bypass = cfg["bypassProxy"].as_str().unwrap_or("");
                let target = url::Url::parse(&resolve(&request.url, vars)).ok();
                let skip = target.as_ref().and_then(|u| u.host_str()).is_some_and(|h| {
                    bypass.split(',').map(str::trim).any(|s| {
                        s == "*" || s == h || s.strip_prefix('*').is_some_and(|s| h.ends_with(s))
                    })
                });
                if skip {
                    request.proxy_disabled = true;
                }
                if !skip {
                    let scheme = cfg["protocol"].as_str().unwrap_or("http");
                    if let Ok(mut uri) = url::Url::parse(&format!("{scheme}://{host}")) {
                        let port = cfg["port"]
                            .as_u64()
                            .map(|p| p.to_string())
                            .or_else(|| cfg["port"].as_str().map(str::to_owned))
                            .unwrap_or_default();
                        if let Ok(port) = port.parse() {
                            let _ = uri.set_port(Some(port));
                        }
                        if let Some(user) = cfg["auth"]["username"].as_str() {
                            let _ = uri.set_username(user);
                        }
                        if let Some(pass) = cfg["auth"]["password"].as_str() {
                            let _ = uri.set_password(Some(pass));
                        }
                        request.proxy = uri.into();
                    }
                }
            }
        }
    }
    if request.proto_files.is_empty() {
        if let Some(files) = manifest["protobuf"]["protoFiles"].as_array() {
            request.proto_files = files
                .iter()
                .filter(|f| f["disabled"] != true && f["enabled"] != false)
                .filter_map(|f| f.as_str().or_else(|| f["path"].as_str()))
                .map(|p| root.join(p).to_string_lossy().into_owned())
                .collect();
        }
    }
    if let Some(paths) = manifest["protobuf"]["importPaths"].as_array() {
        request.proto_import_paths = paths
            .iter()
            .filter(|p| p["disabled"] != true && p["enabled"] != false)
            .filter_map(|p| p.as_str().or_else(|| p["path"].as_str()))
            .map(|p| root.join(p).to_string_lossy().into_owned())
            .collect();
    }
    let certs = &manifest["clientCertificates"];
    if request.client_certificate.is_empty() && certs["enabled"] != false {
        if let Some(certs) = certs["certs"].as_array() {
            let host = url::Url::parse(&resolve(&request.url, vars))
                .ok()
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_default();
            if let Some(cert) = certs.iter().find(|c| {
                c["disabled"] != true
                    && c["domain"].as_str().is_some_and(|d| {
                        let domain = d
                            .split("://")
                            .last()
                            .unwrap_or(d)
                            .split('/')
                            .next()
                            .unwrap_or(d)
                            .split(':')
                            .next()
                            .unwrap_or(d);
                        domain == host
                            || domain == "*"
                            || domain
                                .strip_prefix("*.")
                                .is_some_and(|d| host.ends_with(&format!(".{d}")))
                    })
            }) {
                if cert["type"] == "pfx" {
                    anyhow::bail!(
                        "This collection uses a PFX client certificate. Convert it to PEM certificate and key files in Client Certificates."
                    );
                }
                request.client_certificate = cert["certFilePath"]
                    .as_str()
                    .map(|p| root.join(p).to_string_lossy().into_owned())
                    .unwrap_or_default();
                request.client_key = cert["keyFilePath"]
                    .as_str()
                    .map(|p| root.join(p).to_string_lossy().into_owned())
                    .unwrap_or_default();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inherited_timeout_and_variable_certificate_domain_respect_scope() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("folder")).unwrap();
        fs::write(dir.path().join("reqon.json"),r#"{"proxy":{"config":{"hostname":"proxy.invalid","port":9999,"bypassProxy":"localhost"}},"clientCertificates":{"enabled":true,"certs":[{"domain":"localhost","type":"cert","certFilePath":"certs/client.pem","keyFilePath":"certs/key.pem"}]}}"#).unwrap();
        fs::write(
            dir.path().join("collection.req"),
            "settings {\n  timeout: 900\n}\n",
        )
        .unwrap();
        let request = Request {
            url: "{{base}}/test".into(),
            path: Some(dir.path().join("folder/request.req")),
            ..Default::default()
        };
        let mut request = prepare_scoped(
            request,
            &[],
            &[Pair::new("base", "https://localhost")],
            &BTreeMap::new(),
        )
        .unwrap();
        Preferences {
            timeout: 1000,
            proxy: "http://global-proxy.invalid:8080".into(),
            ..Default::default()
        }
        .apply(&mut request);
        assert_eq!(request.timeout, 900);
        assert!(request.proxy.is_empty());
        assert_eq!(
            request.client_certificate,
            dir.path().join("certs/client.pem").to_string_lossy()
        );
        assert_eq!(
            request.client_key,
            dir.path().join("certs/key.pem").to_string_lossy()
        );
    }
    #[test]
    fn yaml_manifest_resolves_only_enabled_proto_files_and_import_directories() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("requests")).unwrap();
        fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\ninfo:\n  name: Proto\nconfig:\n  protobuf:\n    protoFiles:\n      - path: schema/echo.proto\n      - path: ignored.proto\n        disabled: true\n    importPaths:\n      - path: includes\n        enabled: true\n      - path: ignored\n        enabled: false\n").unwrap();
        let request = prepare(Request {
            path: Some(dir.path().join("requests/echo.yml")),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(
            request.proto_files,
            [dir.path().join("schema/echo.proto").to_string_lossy()]
        );
        assert_eq!(
            request.proto_import_paths,
            [dir.path().join("includes").to_string_lossy()]
        );
    }
    #[test]
    fn nested_headers_auth_vars_and_scripts() {
        let d = tempfile::tempdir().unwrap();
        fs::write(
            d.path().join("reqon.json"),
            r#"{"scripts":{"flow":"sandwich"}}"#,
        )
        .unwrap();
        fs::create_dir(d.path().join("a")).unwrap();
        fs::write(d.path().join("collection.req"),"auth {\n  mode: bearer\n}\nauth:bearer {\n  token: parent\n}\nheaders {\n  X-Test: collection\n}\nscript:pre-request {\n  const local=1; bru.setVar('c',local);\n}\nscript:post-response {\n  console.log('collection');\n}\n").unwrap();
        fs::write(d.path().join("a/folder.req"),"headers {\n  X-Test: folder\n}\nvars:pre-request {\n  host: localhost\n}\nscript:post-response {\n  console.log('folder');\n}\n").unwrap();
        let r = Request {
            path: Some(d.path().join("a/request.req")),
            auth: Auth {
                kind: "inherit".into(),
                ..Default::default()
            },
            pre_script: "const local=2; bru.setVar('r',local);".into(),
            post_script: "console.log('request');".into(),
            ..Default::default()
        };
        let r = prepare(r).unwrap();
        assert_eq!(r.auth.token, "parent");
        assert_eq!(r.headers[0].value, "folder");
        assert_eq!(r.vars[0].value, "localhost");
        let out =
            crate::scripts::run(&r.pre_script, &r, &Reply::default(), &BTreeMap::new()).unwrap();
        assert_eq!(out.vars["c"], "1");
        assert_eq!(out.vars["r"], "2");
        let out =
            crate::scripts::run(&r.post_script, &r, &Reply::default(), &BTreeMap::new()).unwrap();
        assert_eq!(out.logs, ["request", "folder", "collection"]);
    }
}
