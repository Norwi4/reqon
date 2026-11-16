//! Postman v2.1 plus getman's native/multiple-collection envelopes.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use prost::Message;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

fn text(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_default()
}
fn pairs(v: &Value) -> Vec<Pair> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            let row = row.as_array()?;
            Some(Pair {
                key: text(row.first()?),
                value: row.get(1).map(text).unwrap_or_default(),
                description: row.get(2).map(text).unwrap_or_default(),
                enabled: row.get(3).and_then(Value::as_bool).unwrap_or(true),
                kind: if row.get(4).and_then(Value::as_str) == Some("File") {
                    "file"
                } else {
                    "text"
                }
                .into(),
                ..Default::default()
            })
        })
        .collect()
}
fn rows(pairs: &[Pair]) -> Value {
    pairs
        .iter()
        .map(|p| json!([p.key, p.value, p.description, p.enabled]))
        .collect()
}

pub fn apply_getman(data: &Value, r: &mut Request, root: &Path) -> Result<()> {
    if !data.is_object() {
        return Ok(());
    }
    // Restore Reqon-only fields, then overlay values currently edited in getman.
    if data["_reqon"].is_object() {
        let old = r.clone();
        *r = serde_json::from_value(data["_reqon"].clone())
            .context("Invalid Reqon interchange metadata")?;
        r.id = old.id;
        r.name = old.name;
        r.collection = old.collection;
        r.folder = old.folder;
        r.seq = old.seq;
        r.path = old.path;
    }
    r.portable = data.clone();
    r.portable.as_object_mut().unwrap().remove("_reqon");
    let grpc = data["kind"] == "grpc";
    if grpc {
        r.protocol = "grpc".into();
        r.method = "GRPC".into();
        r.url = text(&data["server"]);
        r.grpc_method = format!("{}/{}", text(&data["service"]), text(&data["rpc_method"]))
            .trim_matches('/')
            .into();
        r.body_kind = BodyKind::Json;
        r.body = text(&data["message"]);
        r.grpc_tls = data["tls"].as_bool();
        r.headers = pairs(data.get("metadata_rows").unwrap_or(&data["metadata"]));
        if let Some(value) = data["tls_verify"].as_bool() {
            r.verify_tls = value;
        }
        r.ca_certificate = text(&data["tls_ca_file"]);
        r.client_certificate = text(&data["tls_cert_file"]);
        r.client_key = text(&data["tls_key_file"]);
        if let Some(timeout) = data["grpc_timeout_ms"].as_u64() {
            r.timeout = timeout;
            r.timeout_inherit = false;
        }
        if let Some(encoded) = data["descriptor_base64"].as_str().filter(|v| !v.is_empty()) {
            let bytes = base64::prelude::BASE64_STANDARD
                .decode(encoded)
                .context("Invalid embedded gRPC descriptor")?;
            prost_types::FileDescriptorSet::decode(bytes.as_slice())
                .context("Invalid embedded gRPC descriptor set")?;
            use sha2::{Digest, Sha256};
            let path = root
                .join(".reqon")
                .join("descriptors")
                .join(format!("{:x}.protoset", Sha256::digest(&bytes)));
            storage::atomic_write(&path, bytes)?;
            r.proto_files = vec![path.to_string_lossy().into_owned()];
        } else if let Some(path) = data["descriptor_file"].as_str().filter(|v| !v.is_empty()) {
            r.proto_files = vec![path.into()];
        }
    } else {
        r.method = data["method"].as_str().unwrap_or("GET").into();
        r.url = text(&data["url"]);
        r.headers = pairs(data.get("header_rows").unwrap_or(&data["headers"]));
        r.params = pairs(data.get("param_rows").unwrap_or(&data["params"]));
        r.body_kind = match data["body_type"].as_str().unwrap_or("none") {
            "raw" => match data["raw_language"]
                .as_str()
                .unwrap_or("Text")
                .to_ascii_lowercase()
                .as_str()
            {
                "json" => BodyKind::Json,
                "xml" => BodyKind::Xml,
                "sparql" => BodyKind::Sparql,
                _ => BodyKind::Text,
            },
            "x-www-form-urlencoded" => BodyKind::Form,
            "form-data" => BodyKind::Multipart,
            "file" | "binary" => BodyKind::Binary,
            "GraphQL" => BodyKind::Graphql,
            _ => BodyKind::None,
        };
        r.protocol = if r.body_kind == BodyKind::Graphql {
            "graphql"
        } else {
            "http"
        }
        .into();
        r.body = match r.body_kind {
            BodyKind::Binary => text(&data["body_file"]),
            BodyKind::Graphql => text(&data["graphql_query"]),
            _ => text(&data["body"]),
        };
        r.graphql_variables = data["graphql_variables"].as_str().unwrap_or("{}").into();
        r.form = pairs(if r.body_kind == BodyKind::Multipart {
            &data["multipart_rows"]
        } else {
            data.get("form_rows").unwrap_or(&data["form"])
        });
        for p in &mut r.form {
            if p.kind == "file" {
                p.value = format!("@{}", p.value);
            }
        }
        if let Some(value) = data["http_verify"].as_bool() {
            r.verify_tls = value;
        }
        if let Some(value) = data["http_follow_redirects"].as_bool() {
            r.follow_redirects = value;
        }
        if let Some(timeout) = data["http_timeout"].as_f64() {
            r.timeout = (timeout.max(0.) * 1000.).round() as u64;
            r.timeout_inherit = false;
        }
        for (key, target) in [
            ("http_ca_file", &mut r.ca_certificate),
            ("http_cert_file", &mut r.client_certificate),
            ("http_key_file", &mut r.client_key),
        ] {
            if data[key].is_string() {
                *target = text(&data[key]);
            }
        }
    }
    if let Some(kind) = data["auth_type"].as_str() {
        let value = text(&data["auth_value"]);
        match kind {
            "No Auth" => {
                r.auth = Auth {
                    kind: "none".into(),
                    ..Default::default()
                }
            }
            "Bearer Token" => {
                r.auth = Auth {
                    kind: "bearer".into(),
                    token: value,
                    ..Default::default()
                }
            }
            "Basic Auth" | "API Key" => {
                let (key, value) = value.split_once(':').unwrap_or((&value, ""));
                r.auth = if kind == "Basic Auth" {
                    Auth {
                        kind: "basic".into(),
                        username: key.into(),
                        password: value.into(),
                        ..Default::default()
                    }
                } else {
                    Auth {
                        kind: "apikey".into(),
                        key: key.into(),
                        value: value.into(),
                        in_query: data["api_key_location"] == "Query Params",
                        ..Default::default()
                    }
                };
            }
            _ => {}
        }
    }
    if let Some(scheme) = data["http_auth_scheme"]
        .as_str()
        .filter(|s| matches!(*s, "digest" | "ntlm"))
    {
        r.auth.kind = scheme.into();
    }
    if let Some(proxy) = data["http_proxy"].as_str() {
        r.proxy = proxy.into();
    }
    if let Some(n) = data["http_max_redirects"].as_u64() {
        r.max_redirects = n as usize;
    }
    if let Some(v) = data["http_follow_auth"].as_bool() {
        r.forward_auth = v;
    }
    if let Some(files) = data["grpc_proto_files"].as_array() {
        r.proto_files = files
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(paths) = data["grpc_include_paths"].as_array() {
        r.proto_import_paths = paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    // Only retain imported auth while its editable getman fields still match.
    let signature = json!({"auth_type":data["auth_type"],"auth_value":data["auth_value"],
        "api_key_location":data.get("api_key_location").unwrap_or(&json!("Header"))});
    if data["_postman_auth"]["resolved"] == signature {
        r.auth = crate::postman::auth(&data["_postman_auth"]["auth"]);
    }
    Ok(())
}

pub fn getman_data(r: &Request) -> Value {
    let mut data = if r.portable.is_object() {
        r.portable.clone()
    } else {
        json!({})
    };
    let mut reqon = serde_json::to_value(r).unwrap();
    for key in [
        "id",
        "path",
        "collection",
        "folder",
        "original",
        "portable",
        "protocol_drafts",
    ] {
        reqon.as_object_mut().unwrap().remove(key);
    }
    data["_reqon"] = reqon;
    data["name"] = r.name.clone().into();
    let (kind, value) = match r.auth.kind.as_str() {
        "bearer" => ("Bearer Token", r.auth.token.clone()),
        "basic" => (
            "Basic Auth",
            format!("{}:{}", r.auth.username, r.auth.password),
        ),
        "apikey" => ("API Key", format!("{}:{}", r.auth.key, r.auth.value)),
        "" | "none" | "inherit" => ("No Auth", String::new()),
        _ => ("Unsupported: Reqon", String::new()),
    };
    data["auth_type"] = kind.into();
    data["auth_value"] = value.into();
    data["api_key_location"] = if r.auth.in_query {
        "Query Params"
    } else {
        "Header"
    }
    .into();
    let auth = crate::postman::export_auth(&r.auth);
    // Preserve standard inherited/advanced auth when read by getman's importer.
    let effective = if auth.is_null() {
        crate::inheritance::prepare_scoped(r.clone(), &[], &[], &BTreeMap::new())
            .and_then(|r| crate::auth_vault::protect(&r))
            .map(|r| crate::postman::export_auth(&r.auth))
            .unwrap_or(json!({"type":"noauth"}))
    } else {
        auth.clone()
    };
    data["_postman_auth"] = json!({"auth":auth,"effective":effective,
        "resolved":{"auth_type":data["auth_type"],"auth_value":data["auth_value"],"api_key_location":data["api_key_location"]}});
    data.as_object_mut().unwrap().remove("__reqon_responses");
    if r.protocol == "grpc" {
        data["kind"] = "grpc".into();
        data["server"] = r.url.clone().into();
        data["tls"] = r.grpc_tls_enabled(&BTreeMap::new()).into();
        data["tls_verify"] = r.verify_tls.into();
        let (service, method) = r
            .grpc_method
            .trim_start_matches('/')
            .rsplit_once('/')
            .unwrap_or(("", &r.grpc_method));
        data["service"] = service.into();
        data["rpc_method"] = method.into();
        data["message"] = r.body.clone().into();
        data["metadata_rows"] = rows(&r.headers);
        data["metadata"] = rows(
            &r.headers
                .iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect::<Vec<_>>(),
        );
        for (key, value) in [
            ("tls_ca_file", &r.ca_certificate),
            ("tls_cert_file", &r.client_certificate),
            ("tls_key_file", &r.client_key),
        ] {
            data[key] = value.clone().into();
        }
        data["grpc_timeout_ms"] = r.timeout.into();
        data.as_object_mut().unwrap().remove("descriptor_base64");
        data["descriptor_file"] = "".into();
        if let Ok(bytes) = descriptor_bytes(r) {
            if !bytes.is_empty() {
                data["descriptor_base64"] = base64::prelude::BASE64_STANDARD.encode(bytes).into();
                data["descriptor_file"] = "".into();
            }
        }
    } else {
        data["kind"] = "http".into();
        data["method"] = r.method.clone().into();
        data["url"] = r.url.clone().into();
        data["header_rows"] = rows(&r.headers);
        data["headers"] = rows(
            &r.headers
                .iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect::<Vec<_>>(),
        );
        data["param_rows"] = rows(&r.params);
        data["params"] = rows(
            &r.params
                .iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect::<Vec<_>>(),
        );
        data["body_type"] = match r.body_kind {
            BodyKind::None => "none",
            BodyKind::Form => "x-www-form-urlencoded",
            BodyKind::Multipart => "form-data",
            BodyKind::Binary => "binary",
            BodyKind::Graphql => "GraphQL",
            _ => "raw",
        }
        .into();
        data["raw_language"] = match r.body_kind {
            BodyKind::Json => "JSON",
            BodyKind::Xml => "XML",
            BodyKind::Sparql => "SPARQL",
            _ => {
                if matches!(data["raw_language"].as_str(), Some("HTML" | "JavaScript")) {
                    data["raw_language"].as_str().unwrap()
                } else {
                    "Text"
                }
            }
        }
        .into();
        data["body"] = r.body.clone().into();
        data["body_file"] = if r.body_kind == BodyKind::Binary {
            r.body.clone()
        } else {
            String::new()
        }
        .into();
        data["form_rows"] = rows(&r.form);
        data["form"] = rows(
            &r.form
                .iter()
                .filter(|p| p.enabled)
                .cloned()
                .collect::<Vec<_>>(),
        );
        data["multipart_rows"] = r
            .form
            .iter()
            .map(|p| {
                let file = p.kind == "file" || (p.kind != "text" && p.value.starts_with('@'));
                json!([
                    p.key,
                    if file {
                        p.value.strip_prefix('@').unwrap_or(&p.value)
                    } else {
                        &p.value
                    },
                    p.description,
                    p.enabled,
                    if file { "File" } else { "Text" }
                ])
            })
            .collect();
        data["graphql_query"] = r.body.clone().into();
        data["graphql_variables"] = r.graphql_variables.clone().into();
        data["http_verify"] = r.verify_tls.into();
        data["http_follow_redirects"] = r.follow_redirects.into();
        data["http_timeout"] = json!(r.timeout as f64 / 1000.);
        for (key, value) in [
            ("http_ca_file", &r.ca_certificate),
            ("http_cert_file", &r.client_certificate),
            ("http_key_file", &r.client_key),
        ] {
            data[key] = value.clone().into();
        }
    }
    data
}

pub fn descriptor_bytes(r: &Request) -> Result<Vec<u8>> {
    let paths = r
        .proto_files
        .iter()
        .map(|p| crate::engine::file_path(r, p))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let mut set = prost_types::FileDescriptorSet::default();
    let mut protos = vec![];
    for path in &paths {
        if path
            .extension()
            .is_some_and(|e| e == "protoset" || e == "pb" || e == "desc")
        {
            let bytes = std::fs::read(path)?;
            set.file
                .extend(prost_types::FileDescriptorSet::decode(bytes.as_slice())?.file);
        } else {
            protos.push(path.clone());
        }
    }
    if !protos.is_empty() {
        let includes = paths
            .iter()
            .filter_map(|p| p.parent().map(Path::to_path_buf))
            .chain(
                r.proto_import_paths
                    .iter()
                    .map(|p| crate::engine::file_path(r, p)),
            )
            .collect::<Vec<_>>();
        set.file.extend(protox::compile(protos, includes)?.file);
    }
    let mut names = std::collections::HashSet::new();
    set.file.retain(|f| names.insert(f.name.clone()));
    Ok(set.encode_to_vec())
}

fn normalize(value: &Value) -> Result<Value> {
    if value["format"] != "getman-collection-v1" {
        ensure!(
            value["item"].is_array(),
            "Expected a collection JSON document"
        );
        return Ok(value.clone());
    }
    fn branch(items: &Value) -> Result<Vec<Value>> {
        let mut output = vec![];
        for item in items
            .as_array()
            .context("Collection items must be an array")?
        {
            if item["type"] == "folder" {
                let mut folder = json!({"name":item["name"],"item":branch(&item["items"])?});
                if let Some(auth) = item.get("auth") {
                    folder["auth"] = auth.clone();
                }
                output.push(folder);
            } else {
                ensure!(item["data"].is_object(), "Missing request data");
                ensure!(
                    matches!(item["data"]["kind"].as_str(), None | Some("http" | "grpc")),
                    "Unsupported getman request protocol"
                );
                let responses = item["examples"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|example| {
                        let response = &example["data"]["response"];
                        json!({"name":example["name"],"x-getman":example["data"],
                        "code":response.get("status").unwrap_or(&json!(200)),
                        "status":response.get("reason").unwrap_or(&json!("OK")),
                        "body":response.get("body").unwrap_or(&json!(""))})
                    })
                    .collect::<Vec<_>>();
                output.push(json!({"name":item["data"]["name"],"request":{},"x-getman":item["data"],"response":responses}));
            }
        }
        Ok(output)
    }
    let vars = value["variables"]
        .as_object()
        .map(|v| {
            v.iter()
                .map(|(k, v)| json!({"key":k,"value":v}))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut result = json!({"info":{"name":value["name"],"description":value["description"]},"variable":vars,"item":branch(&value["items"])?});
    if let Some(auth) = value.get("auth") {
        result["auth"] = auth.clone();
    }
    Ok(result)
}

pub fn import_documents(value: &Value, root: &Path) -> Result<Vec<(Collection, Vec<Request>)>> {
    let documents = if value["format"] == "reqon-collections-v1" {
        value["collections"]
            .as_array()
            .context("Missing collections array")?
            .clone()
    } else {
        vec![value.clone()]
    };
    ensure!(!documents.is_empty(), "The collections file is empty");
    let documents = documents
        .iter()
        .map(normalize)
        .collect::<Result<Vec<_>>>()?;
    documents
        .iter()
        .enumerate()
        .map(|(i, v)| storage::import_postman(v, &root.join(format!("collection-{}", i + 1))))
        .collect()
}

/// Validate portable resources before opening the output file, so failed exports
/// never replace an existing collection with an incomplete document.
pub fn export_document(c: &Collection, requests: &[Request]) -> Result<Value> {
    for r in requests
        .iter()
        .filter(|r| r.collection == c.id && r.protocol == "grpc")
    {
        descriptor_bytes(r).with_context(|| format!("Cannot export descriptors for {}", r.name))?;
    }
    storage::export_postman(c, requests)
}

/// Getman's portable envelope: complete collections, in workspace order.
pub fn export_documents(
    collections: &[Collection],
    requests: &[Request],
    selected: &[String],
) -> Result<Value> {
    let selected: std::collections::HashSet<_> = selected.iter().collect();
    ensure!(!selected.is_empty(), "Select at least one collection");
    let collections: Vec<_> = collections
        .iter()
        .filter(|c| selected.contains(&c.id))
        .collect();
    ensure!(
        collections.len() == selected.len(),
        "A selected collection is no longer open"
    );
    let documents = collections
        .into_iter()
        .map(|c| export_document(c, requests))
        .collect::<Result<Vec<_>>>()?;
    Ok(json!({"format":"reqon-collections-v1", "collections":documents}))
}

/// Accept multiple JSON files and bundles, validating every document before writing.
pub fn import_files(
    paths: &[std::path::PathBuf],
    root: &Path,
) -> Result<Vec<(Collection, Vec<Request>)>> {
    let mut documents = Vec::new();
    for path in paths {
        let bytes =
            std::fs::read(path).with_context(|| format!("Cannot read {}", path.display()))?;
        let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes);
        let value: Value = serde_json::from_slice(bytes)
            .with_context(|| format!("Invalid JSON in {}", path.display()))?;
        if value["format"] == "reqon-collections-v1" {
            let entries = value["collections"]
                .as_array()
                .context("Missing collections array")?;
            ensure!(
                !entries.is_empty(),
                "The collections file is empty: {}",
                path.display()
            );
            documents.extend(entries.iter().cloned());
        } else {
            documents.push(value);
        }
    }
    import_documents(
        &json!({"format":"reqon-collections-v1", "collections":documents}),
        root,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, process::Command};

    fn bridge(root: &Path, source: &Value, stage: &str) -> Value {
        let getman = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("getman");
        let input = root.join(format!("{stage}-in.json"));
        let output = root.join(format!("{stage}-out.json"));
        fs::write(&input, serde_json::to_vec_pretty(source).unwrap()).unwrap();
        let result = Command::new("python")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("scripts/getman-bridge.py"))
            .arg(getman)
            .arg(input)
            .arg(&output)
            .arg(root.join(format!("{stage}-store")))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "getman bridge: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice(&fs::read(output).unwrap()).unwrap()
    }

    #[test]
    fn actual_getman_store_roundtrip_http_grpc_folders_examples_and_descriptors() {
        // This integration test uses the sibling checkout, never the user's DB.
        if !Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("getman/main.py")
            .is_file()
        {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let proto = dir.path().join("echo.proto");
        fs::write(&proto, "syntax = \"proto3\"; package qa; message Input { string text = 1; } service Echo { rpc Say(Input) returns (Input); }").unwrap();
        let bytes = protox::compile([proto], [dir.path()])
            .unwrap()
            .encode_to_vec();
        let source = json!({"format":"reqon-collections-v1","collections":[
            {"info":{"name":"HTTP"},"auth":{"type":"bearer","bearer":[{"key":"token","value":"parent-token"}]},
            "item":[{"name":"Empty folder","item":[]},{"name":"Nested","item":[
                {"name":"Get draft","request":{"method":"GET","url":"http://localhost/test","header":[{"key":"disabled","value":"retained","disabled":true,"description":"Header note"}],
                 "body":{"mode":"raw","raw":"{\"draft\":true}","options":{"raw":{"language":"json"}}}},
                 "response":[{"name":"Success","code":200,"status":"OK","header":[],"body":"{\"ok\":true}"}]},
                {"name":"No body","request":{"method":"DELETE","url":"http://localhost/test"}},
                {"name":"Form","request":{"method":"POST","url":"http://localhost/form","body":{"mode":"formdata","formdata":[{"key":"file","src":"example.txt","type":"file","disabled":true},{"key":"literal","value":"@text","type":"text"}]}}}
            ]}]},
            {"format":"getman-collection-v1","kind":"grpc","name":"RPC","description":"RPC docs","variables":{"host":"localhost"},"items":[
                {"type":"folder","name":"Service","items":[{"type":"request","data":{
                    "kind":"grpc","name":"Say","server":"{{host}}:50051","tls":true,"tls_verify":true,
                    "service":"qa.Echo","rpc_method":"Say","message":"{\"text\":\"Привет\"}",
                    "metadata_rows":[["disabled","kept","note",false],["active","yes","",true]],
                    "auth_type":"Bearer Token","auth_value":"rpc-token","grpc_emit_defaults":true,
                    "descriptor_base64":base64::prelude::BASE64_STANDARD.encode(bytes)},
                    "examples":[{"name":"RPC example","data":{"request":{"kind":"grpc","name":"Say"},"response":{"status":0,"body":"{\"text\":\"example\"}"}}}]}]}]}
        ]});
        let from_getman = bridge(dir.path(), &source, "getman-source");
        let collections = import_documents(&from_getman, &dir.path().join("reqon-one")).unwrap();
        assert_eq!(collections.len(), 2);
        for (c, requests) in &collections {
            let (_, disk, warnings) = storage::read_collection(&c.path).unwrap();
            assert!(warnings.is_empty(), "{warnings:?}");
            assert_eq!(requests.len(), disk.len());
            for request in requests {
                let reloaded = disk.iter().find(|r| r.name == request.name).unwrap();
                assert_eq!(request.portable, reloaded.portable);
            }
        }
        let collection_rows = collections
            .iter()
            .map(|(c, _)| c.clone())
            .collect::<Vec<_>>();
        let request_rows = collections
            .iter()
            .flat_map(|(_, r)| r.clone())
            .collect::<Vec<_>>();
        let selected = collection_rows
            .iter()
            .map(|c| c.id.clone())
            .collect::<Vec<_>>();
        let exported = export_documents(&collection_rows, &request_rows, &selected).unwrap();
        let back = bridge(dir.path(), &exported, "reqon-to-getman");
        let roundtrip = import_documents(&back, &dir.path().join("reqon-two")).unwrap();
        let http = &roundtrip[0];
        assert!(http.0.folders.contains(&"Empty folder".into()));
        let get = http.1.iter().find(|r| r.name == "Get draft").unwrap();
        assert_eq!(get.body, "{\"draft\":true}");
        assert!(!get.headers[0].enabled);
        assert_eq!(get.headers[0].description, "Header note");
        assert_eq!(
            crate::inheritance::prepare(get.clone()).unwrap().auth.token,
            "parent-token"
        );
        assert_eq!(
            get.portable["__reqon_responses"][0]["body"],
            "{\"ok\":true}"
        );
        let form = http.1.iter().find(|r| r.name == "Form").unwrap();
        assert_eq!(form.form[0].kind, "file");
        assert!(!form.form[0].enabled);
        assert_eq!(form.form[1].value, "@text");
        assert_eq!(form.form[1].kind, "text");
        let rpc = &roundtrip[1].1[0];
        assert_eq!(rpc.protocol, "grpc");
        assert_eq!(rpc.grpc_tls, Some(true));
        assert!(rpc.verify_tls);
        assert_eq!(rpc.grpc_method, "qa.Echo/Say");
        assert_eq!(rpc.auth.token, "rpc-token");
        assert_eq!(rpc.portable["grpc_emit_defaults"], true);
        assert!(!rpc.headers[0].enabled);
        let descriptor = descriptor_bytes(rpc).unwrap();
        let pool = prost_reflect::DescriptorPool::decode(descriptor.as_slice()).unwrap();
        assert!(pool.get_service_by_name("qa.Echo").is_some());
        assert_eq!(rpc.portable["__reqon_responses"][0]["name"], "RPC example");
    }

    #[test]
    fn selected_bundle_excludes_other_collections_and_keeps_empty_folders_and_live_drafts() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({"format":"reqon-collections-v1","collections":[
            {"info":{"name":"Same name"},"item":[{"name":"Empty","item":[]},{"name":"First","request":{"method":"GET","url":"http://localhost/one"}}]},
            {"info":{"name":"Excluded"},"item":[{"name":"Private","request":{"method":"POST","url":"http://localhost/private"}}]},
            {"info":{"name":"Same name"},"item":[{"name":"Third","request":{"method":"POST","url":"http://localhost/three"}}]}
        ]});
        let imported = import_documents(&source, &dir.path().join("initial")).unwrap();
        let collections = imported.iter().map(|(c, _)| c.clone()).collect::<Vec<_>>();
        let mut requests = imported
            .into_iter()
            .flat_map(|(_, r)| r)
            .collect::<Vec<_>>();
        requests[0].body_kind = BodyKind::Json;
        requests[0].body = "{\"unsaved\":true}".into();
        let selected = vec![collections[2].id.clone(), collections[0].id.clone()];
        let bundle = export_documents(&collections, &requests, &selected).unwrap();
        let roundtrip = import_documents(&bundle, &dir.path().join("roundtrip")).unwrap();
        assert_eq!(roundtrip.len(), 2);
        assert_eq!(roundtrip[0].1[0].name, "First");
        assert_eq!(roundtrip[1].1[0].name, "Third");
        assert!(roundtrip[0].0.folders.contains(&"Empty".to_owned()));
        assert_eq!(roundtrip[0].1[0].body, "{\"unsaved\":true}");
        assert!(export_documents(&collections, &requests, &[]).is_err());
        assert!(export_documents(&collections, &requests, &["missing".into()]).is_err());
    }

    #[test]
    fn multiple_files_support_bundles_and_bom_and_validate_all_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first.json");
        let bundle = dir.path().join("bundle.json");
        let invalid = dir.path().join("invalid.json");
        fs::write(
            &first,
            b"\xef\xbb\xbf{\"info\":{\"name\":\"First\"},\"item\":[]}",
        )
        .unwrap();
        fs::write(
            &bundle,
            serde_json::to_vec(&json!({"format":"reqon-collections-v1","collections":[
                {"info":{"name":"Second"},"item":[]}, {"info":{"name":"Third"},"item":[]}
            ]}))
            .unwrap(),
        )
        .unwrap();
        fs::write(&invalid, b"{}").unwrap();
        let failed_root = dir.path().join("failed");
        assert!(import_files(&[first.clone(), invalid], &failed_root).is_err());
        assert!(!failed_root.exists());
        let imported = import_files(&[first, bundle], &dir.path().join("valid")).unwrap();
        assert_eq!(
            imported
                .iter()
                .map(|(c, _)| c.name.as_str())
                .collect::<Vec<_>>(),
            ["First", "Second", "Third"]
        );
        assert!(import_files(&[], &dir.path().join("none")).is_err());
    }

    #[test]
    fn sparql_and_portable_metadata_survive_bru_yaml_and_changed_auth() {
        let r = Request {
            body_kind: BodyKind::Sparql,
            body: "SELECT * WHERE { ?s ?p ?o }".into(),
            portable: json!({"custom":{"braces":"{}","array":[1,true]}}),
            ..Default::default()
        };
        for loaded in [
            storage::parse_bru(&storage::to_bru(&r)).unwrap(),
            crate::yaml::parse(&crate::yaml::serialize(&r).unwrap()).unwrap(),
        ] {
            assert_eq!(loaded.body_kind, r.body_kind);
            assert_eq!(loaded.body, r.body);
            assert_eq!(loaded.portable, r.portable);
        }
        let mut data = getman_data(&Request {
            auth: Auth {
                kind: "bearer".into(),
                token: "old".into(),
                ..Default::default()
            },
            ..Default::default()
        });
        data["auth_value"] = "edited in getman".into();
        let mut request = Request::default();
        apply_getman(&data, &mut request, Path::new(".")).unwrap();
        assert_eq!(request.auth.token, "edited in getman");
    }
}
