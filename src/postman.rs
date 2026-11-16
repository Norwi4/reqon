use crate::model::{Auth, Request};
use serde_json::{Value, json};

pub fn value(v: &Value) -> String {
    v.as_str().map(str::to_owned).unwrap_or_else(|| {
        if v.is_null() {
            String::new()
        } else {
            v.to_string()
        }
    })
}
pub fn auth(source: &Value) -> Auth {
    let kind = source["type"].as_str().unwrap_or("inherit");
    let mut auth = Auth {
        kind: if kind == "noauth" { "none" } else { kind }.into(),
        ..Default::default()
    };
    if let Some(fields) = source[kind].as_array() {
        for item in fields {
            let key = item["key"].as_str().unwrap_or("");
            let v = value(&item["value"]);
            match key {
                "token" => auth.token = v,
                "username" => auth.username = v,
                "password" => auth.password = v,
                "key" => auth.key = v,
                "value" => auth.value = v,
                "in" => auth.in_query = v == "query",
                _ => {
                    auth.fields.insert(
                        match key {
                            "accessKey" => "accessKeyId",
                            "secretKey" => "secretAccessKey",
                            "sessionToken" => "sessionToken",
                            _ => key,
                        }
                        .into(),
                        v,
                    );
                }
            }
        }
    }
    auth
}
pub fn metadata(source: &Value, request: &mut Request) {
    if let Some(events) = source["event"].as_array() {
        for event in events
            .iter()
            .filter(|e| e["disabled"] != true && e["script"]["disabled"] != true)
        {
            let script = if let Some(lines) = event["script"]["exec"].as_array() {
                lines
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                event["script"]["exec"].as_str().unwrap_or("").into()
            };
            let destination = match event["listen"].as_str() {
                Some("prerequest") => &mut request.pre_script,
                Some("test") => &mut request.tests,
                _ => continue,
            };
            if !destination.is_empty() {
                destination.push('\n');
            }
            destination.push_str(&script);
        }
    }
}
pub fn export_auth(auth: &Auth) -> Value {
    let kind = auth.kind.as_str();
    if kind == "inherit" {
        return Value::Null;
    }
    if kind.is_empty() || kind == "none" {
        return json!({"type":"noauth"});
    }
    let mut fields = auth.fields.clone();
    for (key, val) in [
        ("token", &auth.token),
        ("username", &auth.username),
        ("password", &auth.password),
        ("key", &auth.key),
        ("value", &auth.value),
    ] {
        if !val.is_empty() {
            fields.insert(key.into(), val.clone());
        }
    }
    if kind == "apikey" {
        fields.insert(
            "in".into(),
            if auth.in_query { "query" } else { "header" }.into(),
        );
    }
    let values = fields.into_iter().map(|(key,value)| json!({"key":match key.as_str(){"accessKeyId"=>"accessKey","secretAccessKey"=>"secretKey",_=>key.as_str()},"value":value,"type":"string"})).collect::<Vec<_>>();
    let mut output = json!({"type":kind});
    output[kind] = values.into();
    output
}
pub fn events(request: &Request) -> Value {
    let mut events = vec![];
    for (listen, script) in [
        ("prerequest", request.pre_script.clone()),
        (
            "test",
            [request.post_script.as_str(), request.tests.as_str()]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
        ),
    ] {
        if !script.is_empty() {
            events.push(json!({"listen":listen,"script":{"type":"text/javascript","exec":script.lines().collect::<Vec<_>>()}}));
        }
    }
    events.into()
}
