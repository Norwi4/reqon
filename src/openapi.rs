use crate::model::*;
use crate::storage;
use anyhow::{Result, bail};
use serde_json::Value;
use std::path::Path;

pub fn read(path: &Path) -> Result<Value> {
    use anyhow::{Context, ensure};
    use std::collections::{BTreeMap, VecDeque};
    let base = url::Url::from_file_path(std::fs::canonicalize(path)?)
        .map_err(|_| anyhow::anyhow!("Invalid OpenAPI file path"))?;
    fn load(url: &url::Url, budget: &mut usize) -> Result<Value> {
        use std::io::Read;
        let bytes = match url.scheme() {
            "file" => {
                let file = std::fs::File::open(
                    url.to_file_path()
                        .map_err(|_| anyhow::anyhow!("Invalid reference file path"))?,
                )?;
                let mut bytes = Vec::new();
                file.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
                bytes
            }
            "http" | "https" => {
                use std::io::Read;
                let mut response = reqwest::blocking::Client::builder()
                    .timeout(std::time::Duration::from_secs(30))
                    .build()?
                    .get(url.clone())
                    .send()?
                    .error_for_status()?;
                let mut bytes = Vec::new();
                response
                    .by_ref()
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                bytes
            }
            other => bail!("Unsupported OpenAPI reference scheme: {other}"),
        };
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "OpenAPI reference exceeds 16 MiB: {url}"
        );
        *budget = budget.saturating_add(bytes.len());
        ensure!(
            *budget <= 64 * 1024 * 1024,
            "OpenAPI reference graph exceeds 64 MiB"
        );
        Ok(serde_json::from_slice(&bytes)
            .or_else(|_| serde_yaml::from_slice(&bytes))
            .with_context(|| format!("Invalid OpenAPI document: {url}"))?)
    }
    fn rewrite(
        value: &mut Value,
        base: &url::Url,
        prefixes: &mut BTreeMap<String, String>,
        pending: &mut VecDeque<(url::Url, Value, String)>,
        budget: &mut usize,
    ) -> Result<()> {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str).map(str::to_owned)
                {
                    let mut target = base.join(&reference)?;
                    let pointer =
                        percent_encoding::percent_decode_str(target.fragment().unwrap_or(""))
                            .decode_utf8()?
                            .into_owned();
                    ensure!(
                        pointer.is_empty() || pointer.starts_with('/'),
                        "OpenAPI anchor references are not supported: {reference}"
                    );
                    target.set_fragment(None);
                    let prefix = if let Some(prefix) = prefixes.get(target.as_str()) {
                        prefix.clone()
                    } else {
                        ensure!(
                            prefixes.len() < 128,
                            "OpenAPI reference graph exceeds 128 documents"
                        );
                        let prefix = format!("/x-reqon-documents/document{}", prefixes.len());
                        let document = load(&target, budget)?;
                        prefixes.insert(target.to_string(), prefix.clone());
                        pending.push_back((target, document, prefix.clone()));
                        prefix
                    };
                    map.insert("$ref".into(), format!("#{prefix}{pointer}").into());
                }
                for value in map.values_mut() {
                    rewrite(value, base, prefixes, pending, budget)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    rewrite(value, base, prefixes, pending, budget)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    let mut budget = 0;
    let source = load(&base, &mut budget)?;
    let mut prefixes = BTreeMap::from([(base.to_string(), String::new())]);
    let mut pending = VecDeque::from([(base, source, String::new())]);
    let mut root = Value::Null;
    let mut external = serde_json::Map::new();
    while let Some((base, mut value, prefix)) = pending.pop_front() {
        rewrite(&mut value, &base, &mut prefixes, &mut pending, &mut budget)?;
        if prefix.is_empty() {
            root = value;
        } else {
            external.insert(prefix.rsplit('/').next().unwrap().into(), value);
        }
    }
    if !external.is_empty() {
        root["x-reqon-documents"] = external.into();
    }
    Ok(root)
}
fn resolve_ref<'a>(spec: &'a Value, mut value: &'a Value) -> &'a Value {
    for _ in 0..24 {
        let Some(reference) = value.get("$ref").and_then(Value::as_str) else {
            break;
        };
        let Some(pointer) = reference.strip_prefix('#') else {
            break;
        };
        let Some(next) = spec.pointer(pointer) else {
            break;
        };
        if std::ptr::eq(value, next) {
            break;
        }
        value = next;
    }
    value
}
fn sample(spec: &Value, schema: &Value, depth: usize) -> Value {
    if depth > 8 {
        return Value::Null;
    }
    let schema = resolve_ref(spec, schema);
    for key in ["example", "default", "const"] {
        if let Some(value) = schema.get(key) {
            return value.clone();
        }
    }
    if let Some(value) = schema["enum"].as_array().and_then(|a| a.first()) {
        return value.clone();
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(value) = schema[key].as_array().and_then(|a| a.first()) {
            return sample(spec, value, depth + 1);
        }
    }
    if let Some(all) = schema["allOf"].as_array() {
        let mut object = serde_json::Map::new();
        for part in all {
            if let Value::Object(fields) = sample(spec, part, depth + 1) {
                object.extend(fields);
            }
        }
        return object.into();
    }
    match schema["type"]
        .as_str()
        .unwrap_or(if schema.get("properties").is_some() {
            "object"
        } else {
            "string"
        }) {
        "object" => schema["properties"]
            .as_object()
            .map(|properties| {
                properties
                    .iter()
                    .filter(|(_, v)| v["readOnly"] != true)
                    .map(|(key, value)| (key.clone(), sample(spec, value, depth + 1)))
                    .collect::<serde_json::Map<_, _>>()
            })
            .unwrap_or_default()
            .into(),
        "array" => Value::Array(vec![sample(spec, &schema["items"], depth + 1)]),
        "integer" | "number" => schema.get("minimum").cloned().unwrap_or(0.into()),
        "boolean" => false.into(),
        "null" => Value::Null,
        _ => match schema["format"].as_str() {
            Some("date-time") => "2026-01-01T00:00:00Z",
            Some("date") => "2026-01-01",
            Some("email") => "user@example.com",
            Some("uuid") => "00000000-0000-4000-8000-000000000000",
            _ => "string",
        }
        .into(),
    }
}
fn string(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| value.to_string())
}
fn security(spec: &Value, op: &Value) -> Auth {
    let requirements = op.get("security").unwrap_or(&spec["security"]);
    let Some(name) = requirements
        .as_array()
        .and_then(|a| a.first())
        .and_then(Value::as_object)
        .and_then(|o| o.keys().next())
    else {
        return Auth::default();
    };
    let schemas = if spec.get("swagger").is_some() {
        &spec["securityDefinitions"]
    } else {
        &spec["components"]["securitySchemes"]
    };
    let schema = resolve_ref(spec, &schemas[name]);
    let mut auth = Auth::default();
    match schema["type"].as_str() {
        Some("apiKey") => {
            auth.kind = "apikey".into();
            auth.key = schema["name"].as_str().unwrap_or("X-API-Key").into();
            auth.value = "{{apiKey}}".into();
            auth.in_query = schema["in"] == "query";
        }
        Some("basic") => {
            auth.kind = "basic".into();
            auth.username = "{{username}}".into();
            auth.password = "{{password}}".into();
        }
        Some("http") => {
            auth.kind = schema["scheme"].as_str().unwrap_or("bearer").to_lowercase();
            auth.token = "{{token}}".into();
            if auth.kind == "basic" {
                auth.username = "{{username}}".into();
                auth.password = "{{password}}".into();
            }
        }
        Some("oauth2") => {
            auth.kind = "oauth2".into();
            let flow = schema["flows"]
                .get("clientCredentials")
                .or_else(|| schema["flows"].get("authorizationCode"))
                .unwrap_or(schema);
            auth.fields.insert(
                "accessTokenUrl".into(),
                flow["tokenUrl"].as_str().unwrap_or("").into(),
            );
            auth.fields.insert(
                "authorizeUrl".into(),
                flow["authorizationUrl"].as_str().unwrap_or("").into(),
            );
            auth.fields.insert(
                "grantType".into(),
                if flow.get("authorizationUrl").is_some() {
                    "authorization_code"
                } else {
                    "client_credentials"
                }
                .into(),
            );
            auth.fields.insert("clientId".into(), "{{clientId}}".into());
            auth.fields
                .insert("clientSecret".into(), "{{clientSecret}}".into());
        }
        _ => {}
    }
    auth
}

pub fn import(spec: &Value, path: &Path) -> Result<(Collection, Vec<Request>)> {
    fn validate_refs(spec: &Value, value: &Value) -> Result<()> {
        match value {
            Value::Object(map) => {
                if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                    let Some(pointer) = reference.strip_prefix('#') else {
                        bail!(
                            "External OpenAPI reference '{reference}' must be bundled into the document before importing"
                        );
                    };
                    anyhow::ensure!(
                        spec.pointer(pointer).is_some(),
                        "OpenAPI reference '{reference}' does not exist"
                    );
                }
                for value in map.values() {
                    validate_refs(spec, value)?;
                }
            }
            Value::Array(values) => {
                for value in values {
                    validate_refs(spec, value)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    validate_refs(spec, spec)?;
    if spec["swagger"] != "2.0"
        && spec["openapi"]
            .as_str()
            .is_none_or(|v| !v.starts_with("3."))
    {
        bail!("Expected an OpenAPI 3 or Swagger 2 document (JSON or YAML)");
    }
    let mut base = spec["servers"][0]["url"]
        .as_str()
        .unwrap_or("http://localhost")
        .to_owned();
    if spec["swagger"] == "2.0" {
        base = format!(
            "{}://{}{}",
            spec["schemes"][0].as_str().unwrap_or("https"),
            spec["host"].as_str().unwrap_or("localhost"),
            spec["basePath"].as_str().unwrap_or("")
        );
    }
    if let Some(variables) = spec["servers"][0]["variables"].as_object() {
        for (key, value) in variables {
            base = base.replace(&format!("{{{key}}}"), &string(&value["default"]));
        }
    }
    let mut c = Collection {
        id: id(),
        name: spec["info"]["title"].as_str().unwrap_or("OpenAPI").into(),
        path: path.to_path_buf(),
        environments: vec![Environment {
            name: "Default".into(),
            vars: vec![Pair::new("baseUrl", &base)],
        }],
        vars: vec![Pair::new("baseUrl", &base)],
        folders: vec![],
    };
    let mut requests = vec![];
    let Some(paths) = spec["paths"].as_object() else {
        bail!("OpenAPI document has no paths");
    };
    for (path, item) in paths {
        let item = resolve_ref(spec, item);
        let Some(ops) = item.as_object() else {
            continue;
        };
        for (method, op) in ops {
            let op = resolve_ref(spec, op);
            if ![
                "get", "post", "put", "patch", "delete", "head", "options", "trace",
            ]
            .contains(&method.as_str())
            {
                continue;
            }
            let name = op["summary"]
                .as_str()
                .or_else(|| op["operationId"].as_str())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{} {path}", method.to_uppercase()));
            let folder = safe_name(op["tags"][0].as_str().unwrap_or("Requests"));
            if !c.folders.contains(&folder) {
                c.folders.push(folder.clone());
            }
            let mut r = Request {
                name,
                method: method.to_uppercase(),
                url: format!("{{{{baseUrl}}}}{path}"),
                folder,
                collection: c.id.clone(),
                docs: op["description"].as_str().unwrap_or("").into(),
                auth: security(spec, op),
                ..Default::default()
            };
            for list in [&item["parameters"], &op["parameters"]] {
                if let Some(params) = list.as_array() {
                    for param in params {
                        let param = resolve_ref(spec, param);
                        let name = param["name"].as_str().unwrap_or("");
                        let v = param
                            .get("example")
                            .or_else(|| param["schema"].get("default"));
                        let val = v
                            .map(|v| {
                                v.as_str()
                                    .map(str::to_string)
                                    .unwrap_or_else(|| v.to_string())
                            })
                            .unwrap_or_default();
                        match param["in"].as_str() {
                            Some("query") => r.params.push(Pair::new(name, val)),
                            Some("header") => r.headers.push(Pair::new(name, val)),
                            Some("path") => {
                                r.url = r
                                    .url
                                    .replace(&format!("{{{name}}}"), &format!("{{{{{name}}}}}"));
                                r.vars.push(Pair::new(name, val));
                            }
                            Some("body") => {
                                r.body_kind = BodyKind::Json;
                                r.body = serde_json::to_string_pretty(&sample(
                                    spec,
                                    &param["schema"],
                                    0,
                                ))?;
                            }
                            Some("formData") => {
                                r.body_kind = if op["consumes"]
                                    .as_array()
                                    .is_some_and(|a| a.iter().any(|v| v == "multipart/form-data"))
                                {
                                    BodyKind::Multipart
                                } else {
                                    BodyKind::Form
                                };
                                r.form.push(Pair::new(name, val));
                            }
                            _ => {}
                        }
                    }
                }
            }
            let body = resolve_ref(spec, &op["requestBody"]);
            if let Some(contents) = body["content"].as_object() {
                if let Some((mime, content)) = contents
                    .iter()
                    .find(|(k, _)| k.as_str() == "application/json")
                    .or_else(|| contents.iter().next())
                {
                    let example = content
                        .get("example")
                        .cloned()
                        .or_else(|| {
                            content["examples"]
                                .as_object()
                                .and_then(|o| o.values().next())
                                .map(|v| resolve_ref(spec, v)["value"].clone())
                        })
                        .unwrap_or_else(|| sample(spec, &content["schema"], 0));
                    match mime.as_str() {
                        "application/x-www-form-urlencoded" | "multipart/form-data" => {
                            r.body_kind = if mime == "multipart/form-data" {
                                BodyKind::Multipart
                            } else {
                                BodyKind::Form
                            };
                            if let Some(fields) = example.as_object() {
                                r.form = fields
                                    .iter()
                                    .map(|(k, v)| Pair::new(k, string(v)))
                                    .collect();
                            }
                        }
                        _ if mime.contains("json") => {
                            r.body_kind = BodyKind::Json;
                            r.body = serde_json::to_string_pretty(&example)?;
                        }
                        _ if mime.contains("xml") => {
                            r.body_kind = BodyKind::Xml;
                            r.body = example.as_str().unwrap_or("<root/>").into();
                        }
                        _ => {
                            r.body_kind = BodyKind::Text;
                            r.body = string(&example);
                        }
                    }
                    if r.body_kind != BodyKind::Multipart {
                        r.headers.push(Pair::new("Content-Type", mime));
                    }
                }
            }
            r.seq = (requests.len() + 1) as f64;
            let mut file = c
                .path
                .join(&r.folder)
                .join(format!("{}.req", safe_name(&r.name)));
            let mut n = 2;
            while requests
                .iter()
                .any(|r: &Request| r.path.as_ref() == Some(&file))
            {
                file = c
                    .path
                    .join(&r.folder)
                    .join(format!("{} {n}.req", safe_name(&r.name)));
                n += 1;
            }
            r.path = Some(file);
            requests.push(r);
        }
    }
    storage::create_collection(&c)?;
    for r in &requests {
        storage::save_request(r)?;
    }
    Ok((c, requests))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_yaml_external_schema_and_security_references() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("schema.yml"),"User:\n  type: object\n  properties:\n    name:\n      type: string\n      example: Ada\n    count:\n      type: integer\n      minimum: 4\n").unwrap();
        std::fs::write(d.path().join("api.yml"),"openapi: 3.0.3\ninfo:\n  title: External\nservers:\n  - url: https://api.example.com\nsecurity:\n  - bearer: []\ncomponents:\n  securitySchemes:\n    bearer:\n      type: http\n      scheme: bearer\npaths:\n  /users:\n    post:\n      requestBody:\n        content:\n          application/json:\n            schema:\n              $ref: './schema.yml#/User'\n      responses: {}\n").unwrap();
        let spec = read(&d.path().join("api.yml")).unwrap();
        let (_, requests) = import(&spec, &d.path().join("imported")).unwrap();
        assert_eq!(requests.len(), 1);
        let body: Value = serde_json::from_str(&requests[0].body).unwrap();
        assert_eq!(body["name"], "Ada");
        assert_eq!(body["count"], 4);
        assert_eq!(requests[0].auth.kind, "bearer");
    }
    #[test]
    fn path_and_query_parameters() {
        let d = tempfile::tempdir().unwrap();
        let(_,rs)=import(&serde_json::json!({"openapi":"3.0.3","info":{"title":"Test"},"paths":{"/users/{id}":{"get":{"parameters":[{"name":"id","in":"path","example":42},{"name":"limit","in":"query","example":10}]}}}}),d.path()).unwrap();
        assert_eq!(rs[0].url, "{{baseUrl}}/users/{{id}}");
        assert_eq!(rs[0].params[0].value, "10");
        assert_eq!(rs[0].vars[0].value, "42");
    }
}
