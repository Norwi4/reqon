use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

fn text(v: &Value) -> String {
    if v.is_null() {
        String::new()
    } else {
        v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
    }
}
pub fn rows(value: &Value) -> Vec<Pair> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .map(|v| {
                    let typed = v["value"].is_object() && v["value"].get("type").is_some();
                    Pair {
                        key: text(v.get("name").unwrap_or(&v["key"])),
                        value: text(if typed {
                            &v["value"]["data"]
                        } else {
                            &v["value"]
                        }),
                        enabled: v["disabled"] != true,
                        secret: v["secret"] == true,
                        kind: if typed {
                            text(&v["value"]["type"])
                        } else {
                            text(&v["type"])
                        },
                        description: text(&v["description"]),
                        ..Default::default()
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}
pub fn row_values(rows: &[Pair], typed: bool) -> Value {
    rows.iter()
        .filter(|p| !p.key.is_empty())
        .map(|p| {
            let mut v = json!({"name":p.key});
            if p.secret {
                v["secret"] = true.into();
                if !p.kind.is_empty() {
                    v["type"] = p.kind.clone().into();
                }
            } else {
                v["value"] = if typed && !matches!(p.kind.as_str(), "" | "string" | "text") {
                    json!({"type":p.kind,"data":p.value})
                } else {
                    p.value.clone().into()
                };
            }
            if !p.enabled {
                v["disabled"] = true.into();
            }
            if !p.description.is_empty() {
                v["description"] = p.description.clone().into();
            }
            v
        })
        .collect::<Vec<_>>()
        .into()
}
fn auth(value: &Value) -> Auth {
    let mut a = Auth::default();
    if let Some(kind) = value.as_str() {
        a.kind = kind.into();
        return a;
    }
    a.kind = text(&value["type"]);
    if a.kind == "api-key" {
        a.kind = "apikey".into();
    }
    a.username = text(&value["username"]);
    a.password = text(&value["password"]);
    a.token = text(value.get("token").unwrap_or(&value["accessToken"]));
    a.key = text(&value["key"]);
    a.value = text(&value["value"]);
    a.in_query = matches!(value["placement"].as_str(), Some("queryparams" | "query"));
    if let Some(map) = value.as_object() {
        for (k, v) in map {
            if v.is_string() || v.is_boolean() || v.is_number() {
                a.fields.insert(k.clone(), text(v));
            }
        }
    }
    if a.kind == "oauth2" {
        for (to, path) in [
            ("grantType", "/flow"),
            ("authorizeUrl", "/authorizationUrl"),
            ("clientId", "/credentials/clientId"),
            ("clientSecret", "/credentials/clientSecret"),
            ("credentialsPlacement", "/credentials/placement"),
        ] {
            if let Some(v) = value.pointer(path) {
                a.fields.insert(to.into(), text(v));
            }
        }
        if let Some(v) = value.pointer("/pkce/disabled") {
            a.fields
                .insert("pkce".into(), (!v.as_bool().unwrap_or(true)).to_string());
        }
    }
    a
}
fn auth_value(auth: &Auth, original: &Value) -> Value {
    if matches!(auth.kind.as_str(), "" | "none" | "inherit") {
        return if auth.kind == "inherit" {
            "inherit"
        } else {
            "none"
        }
        .into();
    }
    let mut v = if original.is_object() {
        original.clone()
    } else {
        json!({})
    };
    v["type"] = auth.kind.clone().into();
    for (k, val) in &auth.fields {
        if !matches!(
            k.as_str(),
            "type"
                | "grantType"
                | "authorizeUrl"
                | "clientId"
                | "clientSecret"
                | "credentialsPlacement"
                | "pkce"
        ) {
            v[k] = val.clone().into();
        }
    }
    match auth.kind.as_str() {
        "basic" | "digest" | "ntlm" | "wsse" => {
            v["username"] = auth.username.clone().into();
            v["password"] = auth.password.clone().into();
        }
        "bearer" => v["token"] = auth.token.clone().into(),
        "apikey" => {
            v["key"] = auth.key.clone().into();
            v["value"] = auth.value.clone().into();
            v["placement"] = if auth.in_query {
                "queryparams"
            } else {
                "header"
            }
            .into();
        }
        "oauth2" => {
            for (key, path) in [("grantType", "flow"), ("authorizeUrl", "authorizationUrl")] {
                if let Some(value) = auth.fields.get(key) {
                    v[path] = value.clone().into();
                }
            }
            if !v["credentials"].is_object() {
                v["credentials"] = json!({});
            }
            for (key, target) in [
                ("clientId", "clientId"),
                ("clientSecret", "clientSecret"),
                ("credentialsPlacement", "placement"),
            ] {
                if let Some(value) = auth.fields.get(key) {
                    v["credentials"][target] = value.clone().into();
                }
            }
        }
        _ => {}
    }
    v
}
pub fn parse(source: &str) -> Result<Request> {
    let v: Value = serde_yaml::from_str(source).context("Invalid OpenCollection YAML")?;
    ensure!(v.is_object(), "Expected an OpenCollection YAML object");
    let mut r = Request {
        original: source.into(),
        file_format: "yml".into(),
        name: text(&v["info"]["name"]),
        seq: v["info"]["seq"].as_f64().unwrap_or(1.),
        ..Default::default()
    };
    r.protocol = if v.get("opencollection").is_some() {
        "collection".into()
    } else {
        match v["info"]["type"].as_str().unwrap_or("http") {
            "websocket" => "ws",
            kind => kind,
        }
        .into()
    };
    r.tags = v["info"]["tags"]
        .as_array()
        .map(|a| a.iter().map(text).collect())
        .unwrap_or_default();
    let parent = matches!(r.protocol.as_str(), "collection" | "folder");
    let protocol = if parent {
        "request"
    } else if r.protocol == "ws" {
        "websocket"
    } else {
        &r.protocol
    };
    let q = &v[protocol];
    r.url = text(&q["url"]);
    r.method = q["method"]
        .as_str()
        .unwrap_or(if r.protocol == "graphql" {
            "POST"
        } else {
            "GET"
        })
        .into();
    r.auth = auth(&q["auth"]);
    r.headers = rows(q.get("headers").unwrap_or(&q["metadata"]));
    for mut p in rows(&q["params"]) {
        if p.kind == "path" {
            p.kind.clear();
            r.path_params.push(p);
        } else {
            p.kind.clear();
            r.params.push(p);
        }
    }
    let body = &q["body"];
    r.body_kind = match body["type"].as_str().unwrap_or("none") {
        "json" => BodyKind::Json,
        "xml" => BodyKind::Xml,
        "sparql" => BodyKind::Sparql,
        "text" => BodyKind::Text,
        "form-urlencoded" => BodyKind::Form,
        "multipart-form" => BodyKind::Multipart,
        "graphql" => BodyKind::Graphql,
        "file" => BodyKind::Binary,
        _ => BodyKind::None,
    };
    match r.body_kind {
        BodyKind::Form | BodyKind::Multipart => {
            r.form = rows(&body["data"]);
            if r.body_kind == BodyKind::Multipart {
                r.form = body["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|value| {
                        let row = rows(&json!([value])).into_iter().next().unwrap_or_default();
                        if let Some(files) = value["value"].as_array() {
                            files
                                .iter()
                                .map(|file| Pair {
                                    value: text(file),
                                    ..row.clone()
                                })
                                .collect::<Vec<_>>()
                        } else {
                            vec![row]
                        }
                    })
                    .collect();
            }
            for p in &mut r.form {
                if p.kind == "file" && !p.value.starts_with('@') {
                    p.value = format!("@{}", p.value);
                }
            }
        }
        BodyKind::Graphql => {
            r.body = text(body["data"].get("query").unwrap_or(&body["data"]));
            r.graphql_variables = text(&body["data"]["variables"]);
        }
        BodyKind::Binary => {
            let file = body["data"]
                .as_array()
                .and_then(|a| a.iter().find(|f| f["selected"] != false))
                .unwrap_or(&body["data"]);
            r.body = text(file.get("filePath").unwrap_or(file));
        }
        _ => r.body = text(&body["data"]),
    }
    if r.protocol == "graphql" {
        r.body_kind = BodyKind::Graphql;
        r.body = text(&body["query"]);
        r.graphql_variables = text(&body["variables"]);
    }
    if matches!(r.protocol.as_str(), "grpc" | "ws") {
        r.grpc_method = text(&q["method"]).trim_start_matches('/').into();
        if let Some(path) = q["protoFilePath"].as_str() {
            r.proto_files.push(path.into());
        }
        if let Some(messages) = q["message"].as_array() {
            for (i, m) in messages.iter().enumerate() {
                let value = m["message"].get("data").unwrap_or(&m["message"]);
                let mut p = Pair::new(m["title"].as_str().unwrap_or("Message"), text(value));
                p.kind = m["message"]["type"].as_str().unwrap_or("json").into();
                p.enabled = i == 0;
                r.protocol_messages.push(p);
            }
        }
        r.body = r
            .protocol_messages
            .first()
            .map(|m| m.value.clone())
            .unwrap_or_default();
        r.body_kind = BodyKind::Json;
    }
    let runtime = if parent { q } else { &v["runtime"] };
    for mut p in rows(&runtime["variables"]) {
        if matches!(p.kind.as_str(), "after-response" | "response") {
            p.kind.clear();
            r.response_vars.push(p);
        } else {
            r.vars.push(p);
        }
    }
    if let Some(actions) = runtime["actions"].as_array() {
        for action in actions {
            if action["type"] == "set-variable" && action["phase"] == "after-response" {
                r.response_vars.push(Pair {
                    key: text(&action["variable"]["name"]),
                    value: text(&action["selector"]["expression"]),
                    enabled: action["disabled"] != true,
                    description: text(&action["description"]),
                    ..Default::default()
                });
            }
        }
    }
    if let Some(scripts) = runtime["scripts"].as_array() {
        for script in scripts {
            let target = match script["type"].as_str() {
                Some("before-request") => &mut r.pre_script,
                Some("after-response") => &mut r.post_script,
                Some("tests") => &mut r.tests,
                _ => continue,
            };
            if !target.is_empty() {
                target.push('\n');
            }
            target.push_str(&text(&script["code"]));
        }
    }
    if let Some(assertions) = runtime["assertions"].as_array() {
        r.assertions = assertions
            .iter()
            .map(|a| Pair {
                key: text(&a["expression"]),
                value: format!(
                    "{} {}",
                    a["operator"].as_str().unwrap_or("eq"),
                    text(&a["value"])
                )
                .trim_end()
                .into(),
                enabled: a["disabled"] != true,
                description: text(&a["description"]),
                ..Default::default()
            })
            .collect();
    }
    let docs = v
        .get("docs")
        .or_else(|| v.get("documentation"))
        .unwrap_or(&Value::Null);
    r.docs = text(docs.get("content").unwrap_or(docs));
    let s = &v["settings"];
    if parent {
        r.proto_files = v["config"]["protobuf"]["protoFiles"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| p["disabled"] != true)
            .filter_map(|p| p["path"].as_str())
            .map(str::to_owned)
            .collect();
    }
    r.timeout_inherit = s.get("timeout").is_none() || s["timeout"] == "inherit";
    r.timeout = s["timeout"].as_u64().unwrap_or(0);
    r.encode_url = s["encodeUrl"] == true;
    r.follow_redirects = s["followRedirects"] != false;
    r.forward_auth = s["forwardAuthorizationHeader"] != false;
    r.max_redirects = s["maxRedirects"].as_u64().unwrap_or(5) as usize;
    r.verify_tls = s["verifyTLS"] != false;
    r.grpc_tls = s["grpcTLS"].as_bool();
    r.portable = s["reqonPortable"].clone();
    if let Some(files) = s["reqonProtoFiles"].as_array() {
        r.proto_files = files
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    if let Some(paths) = s["reqonProtoImportPaths"].as_array() {
        r.proto_import_paths = paths
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    r.proxy = text(&s["proxy"]);
    r.client_certificate = text(&s["clientCertificate"]);
    r.client_key = text(&s["clientKey"]);
    r.ca_certificate = text(&s["caCertificate"]);
    Ok(r)
}
pub fn serialize(r: &Request) -> Result<String> {
    let public = crate::auth_vault::protect(r)?;
    let r = &public;
    let mut v: Value = serde_yaml::from_str(&r.original)
        .ok()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if !v["info"].is_object() {
        v["info"] = json!({});
    }
    v["info"]["name"] = r.name.clone().into();
    let parent = matches!(r.protocol.as_str(), "collection" | "folder");
    if r.protocol == "collection" {
        v["opencollection"] = "1.0.0".into();
    } else {
        v["info"]["type"] = if r.protocol == "ws" {
            "websocket"
        } else {
            &r.protocol
        }
        .into();
        v["info"]["seq"] = r.seq.into();
    }
    if !r.tags.is_empty() {
        v["info"]["tags"] = json!(r.tags);
    } else if let Some(info) = v["info"].as_object_mut() {
        info.remove("tags");
    }
    let protocol = if parent {
        "request"
    } else if r.protocol == "ws" {
        "websocket"
    } else {
        &r.protocol
    };
    if !parent {
        // A converted request has one active transport. Other protocol drafts
        // belong in the local workspace, not in the exported collection file.
        for other in ["http", "graphql", "grpc", "websocket"] {
            if other != protocol {
                v.as_object_mut().unwrap().remove(other);
            }
        }
    }
    if !v[protocol].is_object() {
        v[protocol] = json!({});
    }
    let q = &mut v[protocol];
    let binary_files = if r.body_kind == BodyKind::Binary {
        let mut files = q["body"]["data"].as_array().cloned().unwrap_or_default();
        if files.is_empty() {
            files.push(json!({"selected":true,"filePath":r.body}));
        } else {
            let selected = files
                .iter()
                .position(|f| f["selected"] != false)
                .unwrap_or(0);
            files[selected]["filePath"] = r.body.clone().into();
            files[selected]["selected"] = true.into();
        }
        Some(Value::Array(files))
    } else {
        None
    };
    if !parent {
        q["url"] = r.url.clone().into();
        if matches!(r.protocol.as_str(), "http" | "graphql") {
            q["method"] = r.method.clone().into();
        }
    }
    q["auth"] = auth_value(&r.auth, &q["auth"]);
    q[if r.protocol == "grpc" {
        "metadata"
    } else {
        "headers"
    }] = row_values(&r.headers, false);
    if !parent {
        let mut params = row_values(&r.params, false)
            .as_array()
            .cloned()
            .unwrap_or_default();
        for p in &mut params {
            p["type"] = "query".into();
        }
        let mut path = row_values(&r.path_params, false)
            .as_array()
            .cloned()
            .unwrap_or_default();
        for p in &mut path {
            p["type"] = "path".into();
        }
        params.extend(path);
        q["params"] = params.into();
        let (kind,data)=match r.body_kind{BodyKind::None=>("none",Value::Null),BodyKind::Json=>("json",r.body.clone().into()),BodyKind::Xml=>("xml",r.body.clone().into()),BodyKind::Sparql=>("sparql",r.body.clone().into()),BodyKind::Text=>("text",r.body.clone().into()),BodyKind::Binary=>("file",json!({"filePath":r.body})),BodyKind::Graphql=>("graphql",json!({"query":r.body,"variables":r.graphql_variables})),BodyKind::Form=>("form-urlencoded",row_values(&r.form,false)),BodyKind::Multipart=>("multipart-form",r.form.iter().filter(|p|!p.key.is_empty()).map(|p|json!({"name":p.key,"value":p.value.trim_start_matches('@'),"type":if p.value.starts_with('@'){"file"}else{"text"},"disabled":!p.enabled})).collect::<Vec<_>>().into())};
        if r.protocol == "graphql" {
            q["body"] = json!({"query":r.body,"variables":r.graphql_variables});
        } else if r.body_kind == BodyKind::None {
            q.as_object_mut().unwrap().remove("body");
        } else {
            q["body"] = json!({"type":kind,"data":data});
        }
    }
    if matches!(r.protocol.as_str(), "grpc" | "ws") {
        q.as_object_mut().unwrap().remove("body");
        q.as_object_mut().unwrap().remove("params");
        if r.protocol == "grpc" {
            q["method"] = format!("/{}", r.grpc_method.trim_start_matches('/')).into();
            if let Some(p) = r.proto_files.first() {
                q["protoFilePath"] = p.clone().into();
            }
        }
        let mut messages = r.protocol_messages.clone();
        if messages.is_empty() {
            messages.push(Pair::new("message 1", &r.body));
        }
        let selected = messages.iter().position(|m| m.enabled).unwrap_or(0);
        messages[selected].value = r.body.clone();
        q["message"]=messages.into_iter().map(|m|json!({"title":m.key,"message":if r.protocol=="grpc"{Value::String(m.value)}else{json!({"type":if m.kind.is_empty(){"json"}else{&m.kind},"data":m.value})}})).collect::<Vec<_>>().into();
    }
    if let Some(files) = binary_files {
        q["body"]["data"] = files;
    }
    let runtime = if parent {
        &mut v[protocol]
    } else {
        if !v["runtime"].is_object() {
            v["runtime"] = json!({});
        }
        &mut v["runtime"]
    };
    runtime["variables"] = row_values(&r.vars, true);
    let mut actions = runtime["actions"].as_array().cloned().unwrap_or_default();
    actions.retain(|a| a["type"] != "set-variable" || a["phase"] != "after-response");
    actions.extend(r.response_vars.iter().filter(|p|!p.key.is_empty()).map(|p|json!({"type":"set-variable","phase":"after-response","selector":{"expression":p.value,"method":"jsonq"},"variable":{"name":p.key,"scope":"runtime"},"disabled":!p.enabled,"description":p.description})));
    runtime["actions"] = actions.into();
    runtime["scripts"] = [
        ("before-request", &r.pre_script),
        ("after-response", &r.post_script),
        ("tests", &r.tests),
    ]
    .into_iter()
    .filter(|(_, s)| !s.trim().is_empty())
    .map(|(kind, code)| json!({"type":kind,"code":code}))
    .collect::<Vec<_>>()
    .into();
    runtime["assertions"]=r.assertions.iter().filter(|p|!p.key.is_empty()).map(|p|{let (op,value)=p.value.split_once(' ').unwrap_or((&p.value,""));json!({"expression":p.key,"operator":op,"value":value,"disabled":!p.enabled,"description":p.description})}).collect::<Vec<_>>().into();
    if !v["settings"].is_object() {
        v["settings"] = json!({});
    }
    let settings = &mut v["settings"];
    settings["timeout"] = if r.timeout_inherit {
        "inherit".into()
    } else {
        r.timeout.into()
    };
    settings["encodeUrl"] = r.encode_url.into();
    settings["followRedirects"] = r.follow_redirects.into();
    settings["maxRedirects"] = r.max_redirects.into();
    settings["forwardAuthorizationHeader"] = r.forward_auth.into();
    settings["verifyTLS"] = r.verify_tls.into();
    settings["reqonProtoFiles"] = json!(r.proto_files);
    settings["reqonProtoImportPaths"] = json!(r.proto_import_paths);
    if !r.portable.is_null() {
        settings["reqonPortable"] = r.portable.clone();
    } else {
        settings.as_object_mut().unwrap().remove("reqonPortable");
    }
    if let Some(tls) = r.grpc_tls.filter(|_| r.protocol == "grpc") {
        settings["grpcTLS"] = tls.into();
    } else {
        settings.as_object_mut().unwrap().remove("grpcTLS");
    }
    for (key, value) in [
        ("proxy", &r.proxy),
        ("clientCertificate", &r.client_certificate),
        ("clientKey", &r.client_key),
        ("caCertificate", &r.ca_certificate),
    ] {
        if !value.is_empty() {
            settings[key] = value.clone().into();
        } else {
            settings.as_object_mut().unwrap().remove(key);
        }
    }
    if !r.docs.is_empty() {
        v["docs"] = r.docs.clone().into();
    } else {
        v.as_object_mut().unwrap().remove("docs");
    }
    if parent && !r.proto_files.is_empty() {
        if !v["config"].is_object() {
            v["config"] = json!({});
        }
        if !v["config"]["protobuf"].is_object() {
            v["config"]["protobuf"] = json!({});
        }
        v["config"]["protobuf"]["protoFiles"] = r
            .proto_files
            .iter()
            .map(|path| json!({"type":"file","path":path}))
            .collect::<Vec<_>>()
            .into();
    }
    serde_yaml::to_string(&v).map_err(Into::into)
}
pub fn environment(source: &str) -> Result<Environment> {
    let v: Value = serde_yaml::from_str(source)?;
    Ok(Environment {
        name: text(&v["name"]),
        vars: rows(&v["variables"]),
    })
}
pub fn environment_source(env: &Environment) -> Result<String> {
    Ok(serde_yaml::to_string(
        &json!({"name":env.name,"variables":row_values(&env.vars,true)}),
    )?)
}
pub fn is_yaml(path: &Path) -> bool {
    path.extension().is_some_and(|e| e == "yml" || e == "yaml")
}
pub fn read_request(path: &Path) -> Result<Request> {
    let resolved = crate::native_format::source_path(path);
    let path = resolved.as_path();
    let source = std::fs::read_to_string(path)?;
    let mut r = if is_yaml(path) {
        parse(&source)?
    } else {
        storage::parse_bru(&source)?
    };
    r.path = Some(path.to_owned());
    if !is_yaml(path) {
        r.file_format = if crate::native_format::is_legacy(path) {
            "bru"
        } else {
            "req"
        }
        .into();
    }
    crate::auth_vault::resolve(&mut r, false)?;
    Ok(r)
}

pub type Migration = Vec<(std::path::PathBuf, std::path::PathBuf, String, String)>;
pub fn migrate(collection: &Collection, requests: &[Request]) -> Result<Migration> {
    ensure!(
        !collection.path.join("opencollection.yml").exists(),
        "Collection already uses YAML"
    );
    let mut files: Vec<(std::path::PathBuf, std::path::PathBuf, String)> = vec![];
    for request in requests {
        let Some(path) = &request.path else { continue };
        ensure!(
            path.starts_with(&collection.path),
            "Request is outside this collection"
        );
        let next = path.with_extension("yml");
        ensure!(!next.exists(), "Cannot overwrite {}", next.display());
        files.push((path.clone(), next, serialize(request)?));
    }
    for folder in &collection.folders {
        let old = collection.path.join(folder).join("folder.req");
        if let Some(mut request) = crate::inheritance::parent(&old)? {
            request.protocol = "folder".into();
            request.name = folder.rsplit('/').next().unwrap_or(folder).into();
            let next = old.with_extension("yml");
            ensure!(!next.exists(), "Cannot overwrite {}", next.display());
            files.push((old, next, serialize(&request)?));
        }
    }
    for env in &collection.environments {
        let old = collection
            .path
            .join("environments")
            .join(format!("{}.req", safe_name(&env.name)));
        let next = old.with_extension("yml");
        ensure!(!next.exists(), "Cannot overwrite {}", next.display());
        files.push((old, next, environment_source(env)?));
    }
    let old = collection.path.join("collection.req");
    let mut parent = crate::inheritance::parent(&old)?.unwrap_or_default();
    parent.protocol = "collection".into();
    parent.name = collection.name.clone();
    parent.vars = collection.vars.clone();
    let mut manifest: Value = serde_yaml::from_str(&serialize(&parent)?)?;
    if crate::native_format::manifest(&collection.path).exists() {
        let config: Value = serde_json::from_slice(&std::fs::read(
            crate::native_format::manifest(&collection.path),
        )?)?;
        if !manifest["config"].is_object() {
            manifest["config"] = json!({});
        }
        for key in ["protobuf", "proxy"] {
            if config.get(key).is_some() {
                manifest["config"][key] = config[key].clone();
            }
        }
        if let Some(files) = manifest["config"]["protobuf"]["protoFiles"].as_array_mut() {
            for file in files {
                if file.is_object() {
                    file["type"] = "file".into();
                }
            }
        }
        if let Some(certs) = config["clientCertificates"]["certs"].as_array() {
            manifest["config"]["clientCertificates"]=certs.iter().map(|c| {
                let mut out=json!({"domain":c["domain"],"disabled":c["disabled"]==true||config["clientCertificates"]["enabled"]==false});
                if c["type"]=="pfx" {out["type"]="pkcs12".into();out["pkcs12FilePath"]=c["pfxFilePath"].clone();}
                else {out["type"]="pem".into();out["certificateFilePath"]=c["certFilePath"].clone();out["privateKeyFilePath"]=c["keyFilePath"].clone();}
                if c.get("passphrase").is_some(){out["passphrase"]=c["passphrase"].clone();}
                out
            }).collect::<Vec<_>>().into();
        }
        if config.get("version").is_some() {
            manifest["info"]["version"] = config["version"].clone();
        }
        let mut extension = serde_json::Map::new();
        for key in ["scripts", "presets", "openapi", "ignore"] {
            if config.get(key).is_some() {
                extension.insert(key.into(), config[key].clone());
            }
        }
        if !extension.is_empty() {
            manifest["extensions"] = json!({"bruno":extension});
        }
    }
    // The manifest is written last so existing BRU collections remain readable throughout conversion.
    files.push((
        old,
        collection.path.join("opencollection.yml"),
        serde_yaml::to_string(&manifest)?,
    ));
    let backup = collection.path.join(".reqon/req-backup");
    let mut result = vec![];
    for (old, next, source) in files {
        if old.exists() {
            let dest = backup.join(old.strip_prefix(&collection.path)?);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(&old, &dest)?;
        }
        storage::atomic_write(&next, &source)?;
        let canonical = parse(&source)
            .map(|r| storage::to_bru(&r))
            .unwrap_or_default();
        result.push((old, next, canonical, source));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_variants_preserve_selection_and_metadata() {
        let mut r=parse("info:\n  name: Upload\n  type: http\nhttp:\n  method: POST\n  url: http://localhost\n  body:\n    type: file\n    data:\n      - filePath: first.bin\n        selected: false\n      - filePath: second.bin\n        contentType: application/octet-stream\n        selected: true\n").unwrap();
        assert_eq!(r.body, "second.bin");
        r.body = "third.bin".into();
        let out = serialize(&r).unwrap();
        let value: Value = serde_yaml::from_str(&out).unwrap();
        assert_eq!(value["http"]["body"]["data"][0]["filePath"], "first.bin");
        assert_eq!(
            value["http"]["body"]["data"][1]["contentType"],
            "application/octet-stream"
        );
        assert_eq!(parse(&out).unwrap().body, "third.bin");
    }
    #[test]
    fn graphql_document_preserves_query_variables_and_http_method() {
        let r=parse("info:\n  name: Query\n  type: graphql\ngraphql:\n  url: http://localhost/graphql\n  body:\n    query: 'query { users { id } }'\n    variables: '{\"id\":2}'\n").unwrap();
        assert_eq!(r.method, "POST");
        assert_eq!(r.body_kind, BodyKind::Graphql);
        let next = parse(&serialize(&r).unwrap()).unwrap();
        assert_eq!(next.body, r.body);
        assert_eq!(next.graphql_variables, r.graphql_variables);
    }
    #[test]
    fn migration_reopens_yaml_without_duplicates_and_keeps_bru_backups() {
        let dir = tempfile::tempdir().unwrap();
        let collection = Collection {
            id: id(),
            name: "Migration".into(),
            path: dir.path().join("collection"),
            environments: vec![Environment {
                name: "Local".into(),
                vars: vec![Pair::new("base", "http://localhost")],
            }],
            vars: vec![],
            folders: vec![],
        };
        storage::create_collection(&collection).unwrap();
        let request = Request {
            name: "Saved request".into(),
            collection: collection.id.clone(),
            url: "{{base}}/echo".into(),
            path: Some(collection.path.join("Saved request.req")),
            body_kind: BodyKind::Json,
            body: "{\"answer\":42}".into(),
            ..Default::default()
        };
        storage::save_request(&request).unwrap();
        migrate(&collection, &[request.clone()]).unwrap();
        assert!(
            collection
                .path
                .join(".reqon/req-backup/Saved request.req")
                .is_file()
        );
        let (loaded, requests, errors) = storage::read_collection(&collection.path).unwrap();
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].body, request.body);
        assert_eq!(requests[0].file_format, "yml");
        assert_eq!(loaded.environments[0].vars[0].value, "http://localhost");
        assert!(migrate(&loaded, &requests).is_err());
    }
    #[test]
    fn typed_vars_auth_body_scripts_and_unknown_keys_roundtrip() {
        let input = "info:\n  name: Sample\n  type: http\n  seq: 4\nhttp:\n  method: POST\n  url: '{{base}}/users'\n  auth:\n    type: bearer\n    token: '{{token}}'\n  body:\n    type: json\n    data: '{\"x\":1}'\nruntime:\n  variables:\n    - name: count\n      value:\n        type: number\n        data: '42'\n  scripts:\n    - type: tests\n      code: test('ok',()=>expect(res.status).eq(200));\nextensions:\n  future: keep\n";
        let r = parse(input).unwrap();
        assert_eq!(r.body, "{\"x\":1}");
        assert_eq!(r.vars[0].kind, "number");
        assert_eq!(r.auth.token, "{{token}}");
        let text = serialize(&r).unwrap();
        assert!(text.contains("future: keep"));
        let again = parse(&text).unwrap();
        assert_eq!(again.vars, r.vars);
        assert_eq!(again.tests, r.tests);
        assert_eq!(again.body, r.body);
    }
}
