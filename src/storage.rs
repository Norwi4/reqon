use crate::model::*;
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};

pub fn atomic_write(path: &Path, data: impl AsRef<[u8]>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.tmp", id()));
    fs::write(&tmp, data)?;
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(error.into());
    }
    Ok(())
}
pub fn move_file(from: &Path, to: &Path) -> Result<()> {
    if from == to {
        return Ok(());
    }
    if to.exists() {
        bail!("Destination already exists: {}", to.display());
    }
    fs::rename(from, to).with_context(|| format!("Cannot rename {}", from.display()))
}
pub fn load(root: &Path) -> Result<Workspace> {
    let state = root.join("workspace.json");
    if state.exists() {
        let value: Value = serde_json::from_slice(&fs::read(&state)?)
            .context("Cannot read workspace.json; the existing file was left intact")?;
        let version = value
            .get("schema_version")
            .and_then(Value::as_u64)
            .unwrap_or(1);
        let mut ws: Workspace = serde_json::from_value(value)?;
        if version < 2 {
            for request in &mut ws.requests {
                request.timeout = request.timeout.saturating_mul(1000);
            }
            for h in &mut ws.history {
                h.request.timeout = h.request.timeout.saturating_mul(1000);
            }
            ws.schema_version = 2;
        }
        crate::disk_sync::restore(&mut ws);
        crate::secrets::restore(root, &mut ws)?;
        for request in ws
            .requests
            .iter_mut()
            .chain(ws.collection_drafts.values_mut())
        {
            crate::auth_vault::resolve(request, false)?;
        }
        for history in &mut ws.history {
            crate::auth_vault::resolve(&mut history.request, false)?;
        }
        if version < 3 {
            // Freeze the old visible order once. From schema 3 onwards, equal
            // sequence numbers use this persisted order, never editable names.
            ws.requests
                .sort_by(|a, b| a.seq.total_cmp(&b.seq).then_with(|| a.name.cmp(&b.name)));
            ws.schema_version = 3;
        }
        return Ok(ws);
    }
    let mut ws = Workspace::default();
    let collection = Collection {
        id: id(),
        name: "Sample API Collection".into(),
        path: root.join("collections/Sample API Collection"),
        environments: vec![
            Environment {
                name: "Local".into(),
                vars: vec![Pair::new("baseUrl", "http://127.0.0.1:9473")],
            },
            Environment {
                name: "Production".into(),
                vars: vec![Pair::new("baseUrl", "https://jsonplaceholder.typicode.com")],
            },
        ],
        vars: vec![],
        folders: vec!["Examples".into()],
    };
    create_collection(&collection)?;
    let mut users = Request {
        name: "Get Users".into(),
        url: "https://jsonplaceholder.typicode.com/users".into(),
        collection: collection.id.clone(),
        ..Default::default()
    };
    users.path = Some(collection.path.join("Get Users.req"));
    users.tests="test(\"Status is 200\", () => {\n  expect(res.getStatus()).to.equal(200);\n});\n\ntest(\"Response is an array\", () => {\n  expect(res.getBody()).to.be.an(\"array\");\n});".into();
    save_request(&users)?;
    let mut post=Request{name:"Create Post".into(),method:"POST".into(),url:"{{baseUrl}}/posts".into(),collection:collection.id.clone(),folder:"Examples".into(),body_kind:BodyKind::Json,body:"{\n  \"title\": \"Hello from Reqon\",\n  \"body\": \"A native Rust API client\",\n  \"userId\": 1\n}".into(),..Default::default()};
    post.path = Some(collection.path.join("Examples/Create Post.req"));
    save_request(&post)?;
    ws.active = Some(users.id.clone());
    ws.open_tabs.push(users.id.clone());
    ws.requests = vec![users, post];
    ws.collections.push(collection);
    save(root, &ws)?;
    Ok(ws)
}
pub fn save(root: &Path, ws: &Workspace) -> Result<()> {
    crate::secrets::save(root, ws)?;
    let mut public = ws.clone();
    for request in public
        .requests
        .iter_mut()
        .chain(public.collection_drafts.values_mut())
    {
        *request = public_request(request)?;
    }
    for history in &mut public.history {
        history.request = public_request(&history.request)?;
    }
    let mut public = serde_json::to_value(public)?;
    let secrets = crate::auth_vault::known_values(ws);
    if let Some(history) = public.get_mut("history") {
        crate::auth_vault::redact(history, &secrets);
    }
    atomic_write(
        &root.join("workspace.json"),
        serde_json::to_vec_pretty(&public)?,
    )?;
    crate::workspace_store::save_portable(root, ws)
}
fn public_request(request: &Request) -> Result<Request> {
    let mut public = crate::auth_vault::protect(request)?;
    if !public.original.is_empty() {
        public.original = source(&public)?;
    }
    Ok(public)
}
pub fn create_collection(c: &Collection) -> Result<()> {
    fs::create_dir_all(&c.path)?;
    atomic_write(
        &c.path.join("reqon.json"),
        serde_json::to_vec_pretty(
            &json!({"version":"1","name":c.name,"type":"collection","ignore":["node_modules",".git"]}),
        )?,
    )?;
    for folder in &c.folders {
        fs::create_dir_all(c.path.join(folder))?;
    }
    save_environments(c)
}
pub fn save_environments(c: &Collection) -> Result<()> {
    let yaml = c.path.join("opencollection.yml").is_file();
    let mut writes = vec![];
    // Prepare every environment before touching sources: vault failures must not
    // erase a previously saved value or partially publish this environment batch.
    for env in &c.environments {
        let mut path = c.path.join("environments").join(format!(
            "{}.{}",
            safe_name(&env.name),
            if yaml { "yml" } else { "req" }
        ));
        if yaml && !path.exists() && path.with_extension("yaml").exists() {
            path = path.with_extension("yaml");
        } else if !yaml {
            path = crate::native_format::source_path(&path);
        }
        let original = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(e.into()),
        };
        writes.push((
            path,
            crate::environment_vault::source(env, yaml, &original)?,
        ));
    }
    for (path, source) in writes {
        atomic_write(&path, source)?;
    }
    if yaml {
        let path = c.path.join("opencollection.yml");
        let mut parent = crate::yaml::read_request(&path)?;
        parent.vars = c.vars.clone();
        atomic_write(&path, crate::yaml::serialize(&parent)?)?;
        return Ok(());
    }
    let path = crate::native_format::source_path(&c.path.join("collection.req"));
    if !c.vars.is_empty() || path.exists() {
        let mut parent = crate::inheritance::parent(&path)?.unwrap_or_default();
        parent.protocol = "collection".into();
        parent.path = Some(path.clone());
        parent.vars = c.vars.clone();
        atomic_write(&path, source(&parent)?)?;
    }
    Ok(())
}
// Top-level closing braces in .req start at column zero. Keeping blocks verbatim
// also preserves sections that this version does not interpret.
pub fn blocks(source: &str) -> Vec<(String, String)> {
    let mut output = vec![];
    let mut name = String::new();
    let mut contents = vec![];
    for line in source.lines() {
        if name.is_empty() {
            if let Some(prefix) = line.trim().strip_suffix('{') {
                name = prefix.trim().to_string();
                contents.clear();
            }
        } else if line == "}" {
            output.push((std::mem::take(&mut name), contents.join("\n")));
            contents.clear();
        } else {
            contents.push(line.strip_prefix("  ").unwrap_or(line).to_string());
        }
    }
    output
}
fn pairs(block: &str) -> Vec<Pair> {
    crate::bru::pairs(block)
}
fn properties(block: &str) -> BTreeMap<String, String> {
    pairs(block).into_iter().map(|p| (p.key, p.value)).collect()
}
pub fn parse_bru(source: &str) -> Result<Request> {
    let bs: BTreeMap<String, String> = blocks(source).into_iter().collect();
    let meta = properties(bs.get("meta").map(String::as_str).unwrap_or(""));
    let mut r = Request {
        original: source.into(),
        ..Default::default()
    };
    if let Some(data) = bs.get("reqon:portable") {
        r.portable = serde_json::from_str(data).context("Invalid portable collection data")?;
    }
    r.name = meta
        .get("name")
        .cloned()
        .unwrap_or_else(|| "Untitled".into());
    r.seq = meta.get("seq").and_then(|v| v.parse().ok()).unwrap_or(1.);
    r.tags = crate::bru::list(bs.get("meta").map(String::as_str).unwrap_or(""), "tags");
    let methods = [
        "get", "post", "put", "patch", "delete", "head", "options", "trace", "connect", "grpc",
        "ws", "http",
    ];
    let method = methods
        .into_iter()
        .find(|m| bs.contains_key(*m))
        .context("No request block in .req file")?;
    r.method = method.to_uppercase();
    let http = properties(&bs[method]);
    if matches!(method, "grpc" | "ws") {
        r.protocol = method.into();
    }
    if method == "http" {
        r.method = http.get("method").cloned().unwrap_or("GET".into());
    }
    r.grpc_method = http.get("method").cloned().unwrap_or_default();
    if let Some(path) = http.get("protoPath").filter(|s| !s.is_empty()) {
        r.proto_files.push(path.clone());
    }
    r.url = http.get("url").cloned().unwrap_or_default();
    r.body_kind = match http.get("body").map(String::as_str).unwrap_or("none") {
        "json" => BodyKind::Json,
        "text" => BodyKind::Text,
        "xml" => BodyKind::Xml,
        "sparql" => BodyKind::Sparql,
        "form-urlencoded" => BodyKind::Form,
        "multipart-form" => BodyKind::Multipart,
        "graphql" => BodyKind::Graphql,
        "file" => BodyKind::Binary,
        _ => BodyKind::None,
    };
    for (name, target) in [
        ("params:query", &mut r.params),
        ("params:path", &mut r.path_params),
        ("headers", &mut r.headers),
        ("vars:pre-request", &mut r.vars),
        ("vars:post-response", &mut r.response_vars),
        ("assert", &mut r.assertions),
    ] {
        if let Some(b) = bs.get(name) {
            *target = pairs(b);
        }
    }
    if let Some(b) = bs.get("metadata") {
        r.headers = pairs(b);
    }
    if matches!(method, "grpc" | "ws") {
        r.body_kind = BodyKind::Json;
        for (key, body) in blocks(source) {
            if key == format!("body:{method}") {
                let fields = properties(&body);
                let mut message = Pair::new(
                    fields.get("name").cloned().unwrap_or("Message 1".into()),
                    fields.get("content").cloned().unwrap_or_default(),
                );
                message.kind = fields.get("type").cloned().unwrap_or("text".into());
                message.enabled = fields.get("selected").is_some_and(|s| s == "true");
                r.protocol_messages.push(message);
            }
        }
        if let Some(message) = r
            .protocol_messages
            .iter()
            .find(|m| m.enabled)
            .or_else(|| r.protocol_messages.first())
        {
            r.body = message.value.clone();
        }
    }
    for (name, target) in [
        ("script:pre-request", &mut r.pre_script),
        ("script:post-response", &mut r.post_script),
        ("tests", &mut r.tests),
        ("docs", &mut r.docs),
        ("body:graphql:vars", &mut r.graphql_variables),
    ] {
        if let Some(b) = bs.get(name) {
            *target = b.clone();
        }
    }
    for name in [
        "body:json",
        "body:text",
        "body:xml",
        "body:sparql",
        "body:graphql",
    ] {
        if let Some(b) = bs.get(name) {
            r.body = b.clone();
        }
    }
    for name in ["body:form-urlencoded", "body:multipart-form"] {
        if let Some(b) = bs.get(name) {
            r.form = pairs(b);
        }
    }
    if let Some(b) = bs.get("body:file") {
        if let Some(value) = properties(b).get("file") {
            if let Some(path) = value
                .strip_prefix("@file(")
                .and_then(|v| v.split_once(')').map(|(p, _)| p))
            {
                r.body = path.to_owned();
            }
            if let Some(content_type) = value
                .split("@contentType(")
                .nth(1)
                .and_then(|v| v.split_once(')').map(|(t, _)| t))
            {
                if !r
                    .headers
                    .iter()
                    .any(|p| p.key.eq_ignore_ascii_case("content-type"))
                {
                    r.headers.push(Pair::new("Content-Type", content_type));
                }
            }
        }
    }
    if r.body_kind == BodyKind::Multipart {
        for pair in &mut r.form {
            if let Some(path) = pair
                .value
                .strip_prefix("@file(")
                .and_then(|v| v.split_once(')').map(|(p, _)| p))
            {
                pair.value = format!("@{path}");
                pair.kind = "file".into();
            } else {
                pair.kind = "text".into();
            }
        }
    }
    r.auth.kind = http.get("auth").cloned().unwrap_or_default();
    if let Some(b) = bs.get(&format!("auth:{}", r.auth.kind)) {
        r.auth.fields = properties(b);
        r.auth.username = r.auth.fields.remove("username").unwrap_or_default();
        r.auth.password = r.auth.fields.remove("password").unwrap_or_default();
    }
    if let Some(b) = bs.get("auth:bearer") {
        r.auth.token = properties(b).get("token").cloned().unwrap_or_default();
    }
    if let Some(b) = bs.get("auth:basic") {
        let p = properties(b);
        r.auth.username = p.get("username").cloned().unwrap_or_default();
        r.auth.password = p.get("password").cloned().unwrap_or_default();
    }
    if let Some(b) = bs.get("auth:apikey") {
        let p = properties(b);
        r.auth.key = p.get("key").cloned().unwrap_or_default();
        r.auth.value = p.get("value").cloned().unwrap_or_default();
        r.auth.in_query = p.get("placement").is_some_and(|v| v == "queryparams");
    }
    if let Some(b) = bs.get("settings") {
        let p = properties(b);
        r.timeout_inherit = p.get("timeout").is_none_or(|v| v == "inherit");
        r.encode_url = p.get("encodeUrl").is_some_and(|v| v == "true");
        r.max_redirects = p
            .get("maxRedirects")
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);
        r.forward_auth = p
            .get("forwardAuthorizationHeader")
            .is_none_or(|v| v != "false");
        r.timeout = p
            .get("timeout")
            .and_then(|s| s.parse().ok())
            .unwrap_or(30_000);
        r.follow_redirects = p.get("followRedirects").is_none_or(|s| s != "false");
        r.verify_tls = p.get("verifyTLS").is_none_or(|s| s != "false");
        r.grpc_tls = p.get("grpcTLS").and_then(|s| s.parse().ok());
        r.proxy = p.get("proxy").cloned().unwrap_or_default();
        r.client_certificate = p.get("clientCertificate").cloned().unwrap_or_default();
        r.client_key = p.get("clientKey").cloned().unwrap_or_default();
        r.ca_certificate = p.get("caCertificate").cloned().unwrap_or_default();
        if let Some(files) = p
            .get("protoFiles")
            .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
        {
            r.proto_files = files;
        }
        if let Some(paths) = p
            .get("protoImportPaths")
            .and_then(|v| serde_json::from_str::<Vec<String>>(v).ok())
        {
            r.proto_import_paths = paths;
        }
    }
    Ok(r)
}
fn rows_text(rows: &[Pair]) -> String {
    crate::bru::rows_text(rows)
}
fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn to_bru(r: &Request) -> String {
    let mut result = vec![];
    let original_source = if r.file_format == "yml" {
        ""
    } else {
        &r.original
    };
    let original: BTreeMap<_, _> = blocks(original_source).into_iter().collect();
    let mut push = |name: &str, mut body: String| {
        if matches!(
            name,
            "meta"
                | "settings"
                | "get"
                | "post"
                | "put"
                | "patch"
                | "delete"
                | "head"
                | "options"
                | "trace"
                | "connect"
                | "http"
                | "grpc"
                | "ws"
        ) || name.starts_with("auth:")
        {
            if let Some(old) = original.get(name) {
                let mut keys = properties(&body);
                if name == "meta" {
                    keys.insert("tags".into(), String::new());
                }
                if name == "settings" {
                    keys.insert("protoFiles".into(), String::new());
                    keys.insert("grpcTLS".into(), String::new());
                }
                let extras = crate::bru::unknown_properties(old, &keys);
                if !extras.is_empty() {
                    body.push('\n');
                    body.push_str(&extras);
                }
            }
        }
        result.push(format!("{name} {{\n{body}\n}}"));
    };
    push(
        "meta",
        format!(
            "  name: {}\n  type: {}\n  seq: {}{}",
            r.name,
            if matches!(r.protocol.as_str(), "grpc" | "ws") {
                &r.protocol
            } else {
                "http"
            },
            r.seq,
            if r.tags.is_empty() {
                String::new()
            } else {
                format!(
                    "\n  tags: [\n{}\n  ]",
                    r.tags
                        .iter()
                        .map(|s| format!("    {s}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            }
        ),
    );
    let kind = match r.body_kind {
        BodyKind::None => "none",
        BodyKind::Json => "json",
        BodyKind::Text => "text",
        BodyKind::Xml => "xml",
        BodyKind::Sparql => "sparql",
        BodyKind::Form => "form-urlencoded",
        BodyKind::Multipart => "multipart-form",
        BodyKind::Graphql => "graphql",
        BodyKind::Binary => "file",
    };
    if matches!(r.protocol.as_str(), "grpc" | "ws") {
        let mut properties = vec![
            Pair::new("url", &r.url),
            Pair::new(
                "auth",
                if r.auth.kind.is_empty() {
                    "none"
                } else {
                    &r.auth.kind
                },
            ),
        ];
        if r.protocol == "grpc" {
            properties.push(Pair::new("method", &r.grpc_method));
            properties.push(Pair::new("body", "grpc"));
            if let Some(path) = r.proto_files.first() {
                properties.push(Pair::new("protoPath", path));
            }
        }
        push(&r.protocol, rows_text(&properties));
        let mut messages = r.protocol_messages.clone();
        if messages.is_empty() {
            messages.push(Pair::new("Message 1", &r.body));
        }
        let selected = messages.iter().position(|m| m.enabled).unwrap_or(0);
        messages[selected].value = r.body.clone();
        for (i, message) in messages.iter().enumerate() {
            let mut fields = vec![
                Pair::new("name", &message.key),
                Pair::new("content", &message.value),
            ];
            if r.protocol == "ws" {
                fields.push(Pair::new(
                    "type",
                    if message.kind.is_empty() {
                        "text"
                    } else {
                        &message.kind
                    },
                ));
                if i == selected {
                    fields.push(Pair::new("selected", "true"));
                }
            }
            push(&format!("body:{}", r.protocol), rows_text(&fields));
        }
    } else {
        let standard = [
            "GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS", "TRACE", "CONNECT",
        ]
        .contains(&r.method.as_str());
        let block = if standard {
            r.method.to_lowercase()
        } else {
            "http".into()
        };
        push(
            &block,
            format!(
                "{}  url: {}\n  body: {kind}\n  auth: {}",
                if standard {
                    String::new()
                } else {
                    format!("  method: {}\n", r.method)
                },
                r.url,
                if r.auth.kind.is_empty() {
                    "none"
                } else {
                    &r.auth.kind
                }
            ),
        );
    }
    for (name, rows) in [
        ("params:query", &r.params),
        ("params:path", &r.path_params),
        (
            if r.protocol == "grpc" {
                "metadata"
            } else {
                "headers"
            },
            &r.headers,
        ),
        ("vars:pre-request", &r.vars),
        ("vars:post-response", &r.response_vars),
        ("assert", &r.assertions),
    ] {
        if rows.iter().any(|p| !p.key.is_empty()) {
            push(name, rows_text(rows));
        }
    }
    if !matches!(r.protocol.as_str(), "grpc" | "ws") {
        match r.body_kind {
            BodyKind::Json
            | BodyKind::Text
            | BodyKind::Xml
            | BodyKind::Sparql
            | BodyKind::Graphql => push(&format!("body:{kind}"), indent(&r.body)),
            BodyKind::Form => push(&format!("body:{kind}"), rows_text(&r.form)),
            BodyKind::Multipart => {
                let mut rows = r.form.clone();
                for row in &mut rows {
                    if let Some(path) = row.value.strip_prefix('@').filter(|_| row.kind != "text") {
                        row.value = format!("@file({path})");
                    }
                    if matches!(row.kind.as_str(), "file" | "text") {
                        row.kind.clear();
                    }
                }
                push("body:multipart-form", rows_text(&rows));
            }
            BodyKind::Binary => {
                let mime = r
                    .headers
                    .iter()
                    .find(|p| p.enabled && p.key.eq_ignore_ascii_case("content-type"))
                    .map(|p| p.value.as_str())
                    .unwrap_or("application/octet-stream");
                push(
                    "body:file",
                    format!("  file: @file({}) @contentType({mime})", r.body),
                );
            }
            _ => {}
        }
    }
    if r.body_kind == BodyKind::Graphql {
        push("body:graphql:vars", indent(&r.graphql_variables));
    }
    match r.auth.kind.as_str() {
        "bearer" => push("auth:bearer", format!("  token: {}", r.auth.token)),
        "basic" => push(
            "auth:basic",
            format!(
                "  username: {}\n  password: {}",
                r.auth.username, r.auth.password
            ),
        ),
        "apikey" => push(
            "auth:apikey",
            format!(
                "  key: {}\n  value: {}\n  placement: {}",
                r.auth.key,
                r.auth.value,
                if r.auth.in_query {
                    "queryparams"
                } else {
                    "headers"
                }
            ),
        ),
        kind if !matches!(kind, "" | "none" | "inherit") => {
            let mut fields = r.auth.fields.clone();
            if !r.auth.username.is_empty() {
                fields.insert("username".into(), r.auth.username.clone());
            }
            if !r.auth.password.is_empty() {
                fields.insert("password".into(), r.auth.password.clone());
            }
            if !r.auth.token.is_empty() {
                fields.insert("accessToken".into(), r.auth.token.clone());
            }
            push(
                &format!("auth:{kind}"),
                rows_text(
                    &fields
                        .into_iter()
                        .map(|(k, v)| Pair::new(k, v))
                        .collect::<Vec<_>>(),
                ),
            );
        }
        _ => {}
    }
    for (name, s) in [
        ("script:pre-request", &r.pre_script),
        ("script:post-response", &r.post_script),
        ("tests", &r.tests),
        ("docs", &r.docs),
    ] {
        if !s.trim().is_empty() {
            push(name, indent(s));
        }
    }
    push(
        "settings",
        format!(
            "  timeout: {}\n  followRedirects: {}\n  verifyTLS: {}\n  proxy: {}\n  clientCertificate: {}\n  clientKey: {}\n  caCertificate: {}\n  encodeUrl: {}\n  maxRedirects: {}\n  forwardAuthorizationHeader: {}{}",
            if r.timeout_inherit {
                "inherit".to_owned()
            } else {
                r.timeout.to_string()
            },
            r.follow_redirects,
            r.verify_tls,
            r.proxy,
            r.client_certificate,
            r.client_key,
            r.ca_certificate,
            r.encode_url,
            r.max_redirects,
            r.forward_auth,
            r.grpc_tls
                .filter(|_| r.protocol == "grpc")
                .map(|tls| format!("\n  grpcTLS: {tls}"))
                .unwrap_or_default()
        ),
    );
    if !r.portable.is_null() {
        push(
            "reqon:portable",
            indent(&serde_json::to_string_pretty(&r.portable).unwrap()),
        );
    }
    let known = [
        "reqon:portable",
        "body:sparql",
        "meta",
        "get",
        "post",
        "put",
        "patch",
        "delete",
        "head",
        "options",
        "trace",
        "connect",
        "grpc",
        "ws",
        "http",
        "metadata",
        "body:grpc",
        "body:ws",
        "params:query",
        "params:path",
        "headers",
        "vars:pre-request",
        "vars:post-response",
        "assert",
        "body:json",
        "body:text",
        "body:xml",
        "body:form-urlencoded",
        "body:multipart-form",
        "body:graphql",
        "body:graphql:vars",
        "auth:basic",
        "auth:bearer",
        "auth:apikey",
        "script:pre-request",
        "script:post-response",
        "tests",
        "docs",
        "settings",
    ];
    drop(push);
    if !r.proto_files.is_empty() || !r.proto_import_paths.is_empty() {
        if let Some(settings) = result.iter_mut().find(|s| s.starts_with("settings {")) {
            let at = settings.rfind('}').unwrap();
            settings.insert_str(
                at,
                &format!(
                    "  protoFiles: {}\n  protoImportPaths: {}\n",
                    serde_json::to_string(&r.proto_files).unwrap(),
                    serde_json::to_string(&r.proto_import_paths).unwrap()
                ),
            );
        }
    }
    let mut known_sections = known.to_vec();
    known_sections.push("body:file");
    let auth_block = format!("auth:{}", r.auth.kind);
    known_sections.push(&auth_block);
    // Inactive auth blocks can contain obsolete credentials. The current Auth
    // model is authoritative; never carry old blocks through as vendor data.
    known_sections.extend(
        original
            .keys()
            .filter(|key| key.starts_with("auth:"))
            .map(String::as_str),
    );
    result.extend(
        crate::bru::unknown_sections(original_source, &known_sections)
            .into_iter()
            .map(|s| s.trim_end().to_owned()),
    );
    let result = result.join("\n\n") + "\n";
    if r.protocol == "collection" || r.protocol == "folder" {
        let mut out = format!(
            "auth {{\n  mode: {}\n}}\n\n",
            if r.auth.kind.is_empty() {
                "none"
            } else {
                &r.auth.kind
            }
        );
        for (name, body) in blocks(&result) {
            if name == "meta" && r.protocol == "folder" {
                let rows = body
                    .lines()
                    .filter(|line| !line.trim_start().starts_with("type:"))
                    .collect::<Vec<_>>()
                    .join("\n");
                out.push_str(&format!("meta {{\n{}\n}}\n\n", indent(&rows)));
            }
            if !matches!(
                name.as_str(),
                "meta"
                    | "auth"
                    | "get"
                    | "post"
                    | "put"
                    | "patch"
                    | "delete"
                    | "head"
                    | "options"
                    | "trace"
                    | "connect"
                    | "http"
                    | "params:query"
                    | "params:path"
            ) {
                out.push_str(&format!("{name} {{\n{}\n}}\n\n", indent(&body)));
            }
        }
        out
    } else {
        result
    }
}
pub fn save_request(r: &Request) -> Result<()> {
    atomic_write(
        r.path
            .as_deref()
            .context("Choose a collection before saving")?,
        source(r)?,
    )
}
pub fn source(r: &Request) -> Result<String> {
    let public = crate::auth_vault::protect(r)?;
    let r = &public;
    if r.path.as_deref().is_some_and(crate::yaml::is_yaml) || r.file_format == "yml" {
        crate::yaml::serialize(r)
    } else {
        Ok(to_bru(r))
    }
}

pub fn read_collection(path: &Path) -> Result<(Collection, Vec<Request>, Vec<String>)> {
    let (mut collection, requests, warnings) =
        crate::file_cache::read(path, || read_collection_uncached(path))?;
    for env in &mut collection.environments {
        crate::environment_vault::resolve_rows(&mut env.vars, false)?;
    }
    Ok((collection, requests, warnings))
}
pub fn open_collection(path: &Path) -> Result<(Collection, Vec<Request>, Vec<String>)> {
    anyhow::ensure!(
        crate::native_format::is_collection(path),
        "Choose a folder containing a collection manifest"
    );
    crate::native_format::migrate_collection(path)?;
    read_collection(path)
}
fn read_collection_uncached(path: &Path) -> Result<(Collection, Vec<Request>, Vec<String>)> {
    let yaml = path.join("opencollection.yml").is_file();
    let manifest: Value = if yaml {
        serde_yaml::from_slice(&fs::read(path.join("opencollection.yml"))?)?
    } else {
        serde_json::from_slice(
            &fs::read(crate::native_format::manifest(path))
                .context("Choose a folder containing a collection manifest")?,
        )?
    };
    let mut c = Collection {
        id: id(),
        name: if yaml {
            &manifest["info"]["name"]
        } else {
            &manifest["name"]
        }
        .as_str()
        .unwrap_or("Collection")
        .into(),
        path: path.to_path_buf(),
        environments: vec![],
        vars: vec![],
        folders: vec![],
    };
    let mut requests = vec![];
    let mut warnings = vec![];
    fn visit(
        dir: &Path,
        root: &Path,
        c: &mut Collection,
        out: &mut Vec<Request>,
        warnings: &mut Vec<String>,
        depth: usize,
        yaml: bool,
    ) -> Result<()> {
        if depth > 32 {
            bail!("Collection nesting exceeds 32 levels");
        }
        let mut entries = fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let file = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.file_type()?.is_symlink() {
                continue;
            }
            if file.is_dir() {
                if [".git", "node_modules", "environments", ".reqon"].contains(&name.as_str()) {
                    continue;
                }
                c.folders.push(
                    file.strip_prefix(root)?
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
                visit(&file, root, c, out, warnings, depth + 1, yaml)?;
            } else if (if yaml {
                crate::yaml::is_yaml(&file)
            } else {
                crate::native_format::readable_source(&file)
            }) && name != "collection.req"
                && name != "folder.req"
                && name != "collection.bru"
                && name != "folder.bru"
                && name != "folder.yml"
                && name != "opencollection.yml"
            {
                match crate::yaml::read_request(&file) {
                    Ok(mut r) => {
                        r.path = Some(file.clone());
                        r.collection = c.id.clone();
                        r.folder = file
                            .parent()
                            .unwrap()
                            .strip_prefix(root)?
                            .to_string_lossy()
                            .replace('\\', "/");
                        out.push(r);
                    }
                    Err(e) => warnings.push(format!("{}: {e}", file.display())),
                }
            }
        }
        Ok(())
    }
    visit(path, path, &mut c, &mut requests, &mut warnings, 0, yaml)?;
    if yaml {
        c.vars = crate::yaml::parse(&fs::read_to_string(path.join("opencollection.yml"))?)?.vars;
    }
    if let Ok(source) = fs::read_to_string(crate::native_format::source_path(
        &path.join("collection.req"),
    )) && !yaml
    {
        for (n, b) in blocks(&source) {
            if n == "vars:pre-request" {
                c.vars = pairs(&b);
            }
        }
    }
    if let Ok(entries) = fs::read_dir(path.join("environments")) {
        for e in entries.flatten() {
            let p = e.path();
            if yaml && crate::yaml::is_yaml(&p) {
                match crate::yaml::environment(&fs::read_to_string(&p)?) {
                    Ok(mut env) => {
                        if env.name.is_empty() {
                            env.name = p.file_stem().unwrap().to_string_lossy().into_owned();
                        }
                        c.environments.push(env);
                    }
                    Err(e) => warnings.push(format!("{}: {e}", p.display())),
                };
                continue;
            }
            if !yaml && crate::native_format::readable_source(&p) {
                let src = fs::read_to_string(&p)?;
                let env = crate::environment_vault::parse_bru(
                    p.file_stem().unwrap().to_string_lossy().into_owned(),
                    &src,
                );
                c.environments.push(env);
            }
        }
    }
    Ok((c, requests, warnings))
}
pub fn secret_names(source: &str) -> Vec<String> {
    let mut in_list = false;
    let mut names = vec![];
    for line in source.lines() {
        let line = line.trim();
        if line == "vars:secret [" {
            in_list = true;
            continue;
        }
        if in_list {
            if line == "]" {
                in_list = false;
            } else if !line.is_empty() && !line.starts_with('#') {
                let name = line.trim_end_matches(',');
                names.push(
                    serde_json::from_str::<String>(name)
                        .unwrap_or_else(|_| name.trim_matches(['\'', '"']).to_owned()),
                );
            }
        }
    }
    names
}

pub fn import_postman(value: &Value, path: &Path) -> Result<(Collection, Vec<Request>)> {
    if !value["item"].is_array() {
        bail!("Expected a collection with an item array");
    }
    let mut c = Collection {
        id: id(),
        name: value["info"]["name"]
            .as_str()
            .unwrap_or("Imported Collection")
            .into(),
        path: path.to_path_buf(),
        environments: vec![],
        vars: vec![],
        folders: vec![],
    };
    if let Some(vars) = value["variable"].as_array() {
        c.vars = vars
            .iter()
            .map(|v| {
                Pair::new(
                    v["key"].as_str().unwrap_or(""),
                    crate::postman::value(&v["value"]),
                )
            })
            .collect();
    }
    fn walk(
        items: &[Value],
        folder: &str,
        c: &mut Collection,
        requests: &mut Vec<Request>,
        parents: &mut Vec<Request>,
    ) -> Result<()> {
        for item in items {
            let name = item["name"].as_str().unwrap_or("Untitled");
            if let Some(children) = item["item"].as_array() {
                let f = if folder.is_empty() {
                    safe_name(name)
                } else {
                    format!("{folder}/{}", safe_name(name))
                };
                let base = f.clone();
                let mut f = f;
                let mut suffix = 2;
                while c.folders.contains(&f) {
                    f = format!("{base} {suffix}");
                    suffix += 1;
                }
                c.folders.push(f.clone());
                let mut parent = Request {
                    name: name.into(),
                    protocol: "folder".into(),
                    auth: crate::postman::auth(&item["auth"]),
                    path: Some(c.path.join(&f).join("folder.req")),
                    docs: item["description"].as_str().unwrap_or("").into(),
                    ..Default::default()
                };
                crate::postman::metadata(item, &mut parent);
                parents.push(parent);
                walk(children, &f, c, requests, parents)?;
                continue;
            }
            let q = &item["request"];
            let mut r = Request {
                name: name.into(),
                collection: c.id.clone(),
                folder: folder.into(),
                method: q["method"].as_str().unwrap_or("GET").into(),
                url: q["url"]
                    .as_str()
                    .or_else(|| q["url"]["raw"].as_str())
                    .unwrap_or("")
                    .into(),
                docs: q["description"].as_str().unwrap_or("").into(),
                ..Default::default()
            };
            crate::postman::metadata(item, &mut r);
            if let Some(headers) = q["header"].as_array() {
                r.headers = headers
                    .iter()
                    .map(|h| Pair {
                        key: h["key"].as_str().unwrap_or("").into(),
                        value: h["value"].as_str().unwrap_or("").into(),
                        enabled: !h["disabled"].as_bool().unwrap_or(false),
                        secret: false,
                        description: h["description"].as_str().unwrap_or("").into(),
                        ..Default::default()
                    })
                    .collect();
            }
            if let Some(params) = q["url"]["query"].as_array() {
                r.params = params
                    .iter()
                    .map(|v| Pair {
                        key: v["key"].as_str().unwrap_or("").into(),
                        value: v["value"].as_str().unwrap_or("").into(),
                        enabled: !v["disabled"].as_bool().unwrap_or(false),
                        description: v["description"].as_str().unwrap_or("").into(),
                        ..Default::default()
                    })
                    .collect();
            }
            match q["body"]["mode"].as_str() {
                Some("raw") => {
                    r.body = q["body"]["raw"].as_str().unwrap_or("").into();
                    r.body_kind = match q["body"]["options"]["raw"]["language"].as_str() {
                        Some("json") => BodyKind::Json,
                        Some("xml") => BodyKind::Xml,
                        Some("sparql") => BodyKind::Sparql,
                        _ => BodyKind::Text,
                    };
                }
                Some("urlencoded") => {
                    r.body_kind = BodyKind::Form;
                    if let Some(items) = q["body"]["urlencoded"].as_array() {
                        r.form = items
                            .iter()
                            .map(|v| Pair {
                                key: v["key"].as_str().unwrap_or("").into(),
                                value: crate::postman::value(&v["value"]),
                                enabled: v["disabled"] != true,
                                description: v["description"].as_str().unwrap_or("").into(),
                                ..Default::default()
                            })
                            .collect();
                    }
                }
                Some("formdata") => {
                    r.body_kind = BodyKind::Multipart;
                    if let Some(rows) = q["body"]["formdata"].as_array() {
                        r.form = rows
                            .iter()
                            .map(|v| Pair {
                                key: v["key"].as_str().unwrap_or("").into(),
                                value: if v["type"] == "file" {
                                    format!("@{}", v["src"].as_str().unwrap_or(""))
                                } else {
                                    v["value"].as_str().unwrap_or("").into()
                                },
                                enabled: !v["disabled"].as_bool().unwrap_or(false),
                                kind: if v["type"] == "file" { "file" } else { "text" }.into(),
                                description: v["description"].as_str().unwrap_or("").into(),
                                ..Default::default()
                            })
                            .collect();
                    }
                }
                Some("graphql") => {
                    r.body_kind = BodyKind::Graphql;
                    r.body = q["body"]["graphql"]["query"].as_str().unwrap_or("").into();
                    r.graphql_variables = q["body"]["graphql"]["variables"]
                        .as_str()
                        .unwrap_or("{}")
                        .into();
                }
                Some("file") => {
                    r.body_kind = BodyKind::Binary;
                    r.body = q["body"]["file"]["src"].as_str().unwrap_or("").into();
                }
                _ => {}
            }
            r.auth = crate::postman::auth(&q["auth"]);
            r.seq = requests.len() as f64 + 1.;
            crate::interchange::apply_getman(&item["x-getman"], &mut r, &c.path)?;
            if let Some(auth) = q.get("auth") {
                r.auth = crate::postman::auth(auth);
            }
            if item["response"].is_array() {
                if !r.portable.is_object() {
                    r.portable = json!({});
                }
                r.portable["__reqon_responses"] = item["response"].clone();
            }
            let mut p = c.path.join(folder).join(format!("{}.req", safe_name(name)));
            let mut suffix = 2;
            while requests.iter().any(|r| r.path.as_ref() == Some(&p)) {
                p = c
                    .path
                    .join(folder)
                    .join(format!("{} {suffix}.req", safe_name(name)));
                suffix += 1;
            }
            r.path = Some(p);
            requests.push(r);
        }
        Ok(())
    }
    let mut requests = vec![];
    let mut parents = vec![];
    walk(
        value["item"].as_array().unwrap(),
        "",
        &mut c,
        &mut requests,
        &mut parents,
    )?;
    create_collection(&c)?;
    let mut parent = Request {
        name: c.name.clone(),
        protocol: "collection".into(),
        auth: crate::postman::auth(&value["auth"]),
        vars: c.vars.clone(),
        path: Some(c.path.join("collection.req")),
        docs: value["info"]["description"].as_str().unwrap_or("").into(),
        ..Default::default()
    };
    crate::postman::metadata(value, &mut parent);
    parents.push(parent);
    for parent in &parents {
        save_request(parent)?;
    }
    for r in &mut requests {
        crate::auth_vault::resolve(r, false)?;
        save_request(r)?;
    }
    Ok((c, requests))
}
pub fn export_postman(c: &Collection, requests: &[Request]) -> Result<Value> {
    let protected = requests
        .iter()
        .map(crate::auth_vault::protect)
        .collect::<Result<Vec<_>>>()?;
    let requests = protected.as_slice();
    fn rows(rows: &[Pair]) -> Vec<Value> {
        rows.iter().filter(|p|!p.key.is_empty()).map(|p|json!({"key":p.key,"value":if p.secret { "" } else { &p.value },"disabled":!p.enabled,"description":p.description})).collect()
    }
    fn request(r: &Request) -> Value {
        let mut q = json!({"method":r.method,"url":{"raw":r.url,"query":rows(&r.params)},"header":rows(&r.headers),"description":r.docs});
        q["body"] = match r.body_kind {
            BodyKind::None => Value::Null,
            BodyKind::Form => json!({"mode":"urlencoded","urlencoded":rows(&r.form)}),
            BodyKind::Multipart => {
                json!({"mode":"formdata","formdata":r.form.iter().filter(|p|!p.key.is_empty()).map(|p|if let Some(path)=p.value.strip_prefix('@').filter(|_|p.kind!="text"){json!({"key":p.key,"src":path,"type":"file","disabled":!p.enabled,"description":p.description})}else{json!({"key":p.key,"value":p.value,"type":"text","disabled":!p.enabled,"description":p.description})}).collect::<Vec<_>>()})
            }
            BodyKind::Graphql => {
                json!({"mode":"graphql","graphql":{"query":r.body,"variables":r.graphql_variables}})
            }
            BodyKind::Binary => json!({"mode":"file","file":{"src":r.body}}),
            _ => {
                json!({"mode":"raw","raw":r.body,"options":{"raw":{"language":match r.body_kind{BodyKind::Json=>"json",BodyKind::Xml=>"xml",BodyKind::Sparql=>"sparql",_=>"text"}}}})
            }
        };
        if r.body_kind == BodyKind::None {
            q.as_object_mut().unwrap().remove("body");
        }
        q["auth"] = crate::postman::export_auth(&r.auth);
        let mut item = json!({"name":r.name,"request":q,"event":crate::postman::events(r),"x-getman":crate::interchange::getman_data(r)});
        if let Some(responses) = r.portable.get("__reqon_responses") {
            item["response"] = responses.clone();
        }
        item
    }
    fn tree(c: &Collection, folder: &str, requests: &[&Request]) -> Result<Vec<Value>> {
        let mut items: Vec<Value> = requests
            .iter()
            .filter(|r| r.folder == folder)
            .map(|r| request(r))
            .collect();
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        let mut children = std::collections::BTreeSet::new();
        for folder in &c.folders {
            if let Some(rest) = folder.strip_prefix(&prefix).filter(|s| !s.is_empty()) {
                children.insert(rest.split('/').next().unwrap().to_owned());
            }
        }
        for r in requests {
            if let Some(rest) = r.folder.strip_prefix(&prefix) {
                if !rest.is_empty() {
                    children.insert(rest.split('/').next().unwrap().to_owned());
                }
            }
        }
        for name in children {
            let path = format!("{prefix}{name}");
            let mut item = json!({"name":name,"item":tree(c,&path,requests)?});
            if let Ok(Some(parent)) = crate::inheritance::parent(&c.path.join(&path).join(
                if c.path.join("opencollection.yml").is_file() {
                    "folder.yml"
                } else {
                    "folder.req"
                },
            )) {
                let parent = crate::auth_vault::protect(&parent)?;
                item["auth"] = crate::postman::export_auth(&parent.auth);
                item["event"] = crate::postman::events(&parent);
                item["description"] = parent.docs.into();
            }
            items.push(item);
        }
        Ok(items)
    }
    let mut reqs = requests
        .iter()
        .filter(|r| r.collection == c.id)
        .collect::<Vec<_>>();
    reqs.sort_by(|a, b| a.seq.total_cmp(&b.seq));
    let mut output = json!({"info":{"name":c.name,"schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"variable":rows(&c.vars),"item":tree(c,"",&reqs)?});
    if let Ok(Some(parent)) = crate::inheritance::parent(&c.path.join(
        if c.path.join("opencollection.yml").is_file() {
            "opencollection.yml"
        } else {
            "collection.req"
        },
    )) {
        let parent = crate::auth_vault::protect(&parent)?;
        output["auth"] = crate::postman::export_auth(&parent.auth);
        output["event"] = crate::postman::events(&parent);
        output["info"]["description"] = parent.docs.into();
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn postman_inherits_auth_and_executes_collection_folder_request_scripts() {
        let dir = tempfile::tempdir().unwrap();
        let source = json!({"info":{"name":"Postman compatibility"},"auth":{"type":"bearer","bearer":[{"key":"token","value":"parent-token"}]},
            "event":[{"listen":"prerequest","script":{"exec":["pm.variables.set('order','collection');"]}}],
            "item":[{"name":"Folder","event":[{"listen":"prerequest","script":{"exec":["pm.variables.set('order',pm.variables.get('order')+',folder');"]}}],"item":[{"name":"Echo","event":[
                {"listen":"prerequest","script":{"exec":["pm.variables.set('order',pm.variables.get('order')+',request'); pm.request.headers.upsert({key:'X-Imported',value:'yes'});"]}},
                {"listen":"test","script":{"exec":["pm.test('Status',()=>pm.response.to.have.status(201)); pm.test('JSON',()=>pm.expect(pm.response.json().id).equal(7)); pm.environment.set('id',pm.response.json().id);"]}}
            ],"request":{"method":"GET","url":"http://localhost/echo"}}]}]});
        let (collection, requests) = import_postman(&source, &dir.path().join("one")).unwrap();
        let request = crate::inheritance::prepare(requests[0].clone()).unwrap();
        assert_eq!(request.auth.token, "parent-token");
        let result = crate::scripts::run(
            &request.pre_script,
            &request,
            &Reply::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(result.vars["order"], "collection,folder,request");
        assert_eq!(result.request.headers[0].value, "yes");
        let response = Reply {
            status: 201,
            body: r#"{"id":7}"#.into(),
            ..Default::default()
        };
        let result =
            crate::scripts::run(&request.tests, &request, &response, &Default::default()).unwrap();
        assert_eq!(result.tests.len(), 2);
        assert!(result.tests.iter().all(|t| t.passed));
        assert!(result.effects.persist_environment);
        assert_eq!(result.effects.environment["id"], json!(7));
        let exported = export_postman(&collection, &requests).unwrap();
        let (_, imported) = import_postman(&exported, &dir.path().join("two")).unwrap();
        let second = crate::inheritance::prepare(imported[0].clone()).unwrap();
        assert_eq!(second.pre_script, request.pre_script);
        assert_eq!(second.tests, request.tests);
        assert_eq!(second.auth, request.auth);
    }
    #[test]
    fn grpc_and_websocket_messages_round_trip() {
        for protocol in ["grpc", "ws"] {
            let source = format!(
                "meta {{\n  name: Stream\n  type: {protocol}\n  seq: 2\n}}\n{protocol} {{\n  url: http://localhost:5000\n  method: qa.Echo/Say\n  protoPath: echo.proto\n}}\nbody:{protocol} {{\n  name: First\n  content: '''\n    {{\"text\":\"Привет\"}}\n  '''\n}}\nbody:{protocol} {{\n  name: Second\n  content: second\n}}\n"
            );
            let mut request = parse_bru(&source).unwrap();
            assert_eq!(request.protocol_messages.len(), 2);
            assert_eq!(request.protocol, protocol);
            request.body = "changed".into();
            let loaded = parse_bru(&to_bru(&request)).unwrap();
            assert_eq!(loaded.body, "changed");
            assert_eq!(loaded.protocol_messages.len(), 2);
            assert_eq!(loaded.protocol_messages[1].value, "second");
        }
    }
    #[test]
    fn descriptions_binary_and_parent_blocks_survive_save() {
        let source = "meta {\n  name: Upload\n}\npost {\n  url: http://localhost\n  body: file\n}\nheaders {\n  @description('''Document type''')\n  Content-Type: application/json\n}\nbody:file {\n  file: @file(data.json) @contentType(application/json)\n}\n";
        let req = parse_bru(source).unwrap();
        let round = parse_bru(&to_bru(&req)).unwrap();
        assert_eq!(round.body, "data.json");
        assert_eq!(round.headers[0].description, "Document type");
        let dir = tempfile::tempdir().unwrap();
        let ws = load(dir.path()).unwrap();
        let collection = &ws.collections[0];
        fs::write(
            collection.path.join("collection.req"),
            "auth {\n  mode: bearer\n}\nauth:bearer {\n  token: preserved\n}\n",
        )
        .unwrap();
        save_environments(collection).unwrap();
        let out = fs::read_to_string(collection.path.join("collection.req")).unwrap();
        assert!(!out.contains("token: preserved"));
        assert_eq!(
            crate::inheritance::parent(&collection.path.join("collection.req"))
                .unwrap()
                .unwrap()
                .auth
                .token,
            "preserved"
        );
        let folder = Request {
            protocol: "folder".into(),
            name: "Folder".into(),
            tags: vec!["smoke".into()],
            headers: vec![Pair::new("X-Folder", "inherited")],
            ..Default::default()
        };
        let path = collection.path.join("folder.req");
        fs::write(&path, to_bru(&folder)).unwrap();
        let folder = crate::inheritance::parent(&path).unwrap().unwrap();
        assert_eq!(folder.tags, vec!["smoke"]);
        assert_eq!(folder.headers[0].value, "inherited");
    }
    #[test]
    fn multipart_text_at_sign_survives_bru_and_postman_export() {
        let d = tempfile::tempdir().unwrap();
        let source = json!({"info":{"name":"Multipart"},"item":[{"name":"Upload","request":{"method":"POST","url":"http://localhost","body":{"mode":"formdata","formdata":[{"key":"text","type":"text","value":"@someone","description":"Literal text"},{"key":"upload","type":"file","src":"photo.png"}]}}}]});
        let (collection, requests) = import_postman(&source, d.path()).unwrap();
        let loaded = parse_bru(&to_bru(&requests[0])).unwrap();
        assert_eq!(loaded.form[0].kind, "text");
        assert_eq!(loaded.form[0].value, "@someone");
        assert_eq!(loaded.form[1].kind, "file");
        assert_eq!(loaded.form[1].value, "@photo.png");
        let mut loaded = loaded;
        loaded.collection = collection.id.clone();
        let exported = export_postman(&collection, &[loaded]).unwrap();
        let body = &exported["item"][0]["request"]["body"]["formdata"];
        assert_eq!(body[0]["type"], "text");
        assert_eq!(body[0]["value"], "@someone");
        assert_eq!(body[0]["description"], "Literal text");
        assert_eq!(body[1]["type"], "file");
        assert_eq!(body[1]["src"], "photo.png");
    }
    #[test]
    fn postman_export_preserves_folders_auth_and_body() {
        let d = tempfile::tempdir().unwrap();
        let ws = load(d.path()).unwrap();
        let mut requests = ws.requests.clone();
        requests[1].auth = Auth {
            kind: "bearer".into(),
            token: "{{token}}".into(),
            ..Default::default()
        };
        let exported = export_postman(&ws.collections[0], &requests).unwrap();
        let target = d.path().join("imported");
        let (_, imported) = import_postman(&exported, &target).unwrap();
        let post = imported.iter().find(|r| r.method == "POST").unwrap();
        assert_eq!(post.folder, "Examples");
        assert_eq!(post.body_kind, BodyKind::Json);
        assert_eq!(post.auth.token, "{{token}}");
        assert_eq!(post.body, requests[1].body);
    }
    #[test]
    fn bru_roundtrip_preserves_body_disabled_rows_and_unknown_blocks() {
        let source = "meta {\n  name: Cyrillic Привет\n  type: http\n  seq: 9\n}\n\npost {\n  url: {{base}}/items\n  body: json\n  auth: bearer\n}\n\nbody:json {\n  {\n    \"a\": {\"b\": 2}\n  }\n}\n\nheaders {\n  ~X-Off: no\n  X-On: yes: still\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\ncustom:future {\n  keep: me\n}\n";
        let r = parse_bru(source).unwrap();
        let out = to_bru(&r);
        let b = parse_bru(&out).unwrap();
        assert_eq!(b.body, r.body);
        assert_eq!(b.headers, r.headers);
        assert_eq!(b.auth, r.auth);
        assert!(out.contains("custom:future"));
        assert!(out.contains("seq: 9"));
        assert_eq!(b.name, "Cyrillic Привет");
    }
    #[test]
    fn workspace_survives_restart_and_saves_files() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = load(dir.path()).unwrap();
        ws.name = "Пример".into();
        save(dir.path(), &ws).unwrap();
        assert_eq!(load(dir.path()).unwrap().name, "Пример");
        let (c, r, errors) = read_collection(&ws.collections[0].path).unwrap();
        assert_eq!(c.environments.len(), 2);
        assert_eq!(r.len(), 2);
        assert!(errors.is_empty());
    }
    #[test]
    fn imports_postman_folders() {
        let dir = tempfile::tempdir().unwrap();
        let(c,r)=import_postman(&json!({"info":{"name":"Demo"},"item":[{"name":"Folder","item":[{"name":"One","request":{"method":"POST","url":"http://localhost","body":{"mode":"raw","raw":"{}","options":{"raw":{"language":"json"}}}}}]}]}),dir.path()).unwrap();
        assert_eq!(r[0].folder, "Folder");
        assert_eq!(r[0].body_kind, BodyKind::Json);
        assert!(c.path.join("Folder/One.req").is_file());
    }
}
