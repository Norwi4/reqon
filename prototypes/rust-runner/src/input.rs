//! Shared offline input validation for CLI, future Qt adapter and MR checks.
use crate::spec::{FORMAT, Scenario};
use serde::{
    Deserialize, Serialize,
    de::DeserializeOwned,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value, json};
use std::{fmt, fs::File, io::Read, path::Path};

pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Serialize)]
pub struct Diagnostic {
    pub code: &'static str,
    pub path: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

impl Diagnostic {
    pub fn new(code: &'static str, path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code,
            path: path.into(),
            message: message.into(),
            file: None,
        }
    }
}

/// serde_json::Value normally accepts duplicate map keys. Reject them recursively
/// before typed deserialization, so validate/run cannot interpret an MR differently.
struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct JsonVisitor;
        impl<'de> Visitor<'de> for JsonVisitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("JSON with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|v| UniqueJson(Value::Number(v)))
                    .ok_or_else(|| E::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueJson(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(de::Error::custom("duplicate object key"));
                    }
                    let UniqueJson(value) = map.next_value()?;
                    values.insert(key, value);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(JsonVisitor)
    }
}

pub fn load(path: &Path) -> Result<Scenario, Vec<Diagnostic>> {
    parse(&read(path)?)
}

pub fn read(path: &Path) -> Result<Vec<u8>, Vec<Diagnostic>> {
    let read = || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_INPUT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    let bytes = read().map_err(|e| vec![Diagnostic::new("input.io", "$", e.to_string())])?;
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(vec![Diagnostic::new(
            "input.size",
            "$",
            "scenario exceeds 8 MiB",
        )]);
    }
    Ok(bytes)
}

pub fn parse(bytes: &[u8]) -> Result<Scenario, Vec<Diagnostic>> {
    let scenario: Scenario = decode(json_value(bytes)?)?;
    let diagnostics = scenario.diagnostics();
    if diagnostics.is_empty() {
        Ok(scenario)
    } else {
        Err(diagnostics)
    }
}

pub fn json_value(bytes: &[u8]) -> Result<Value, Vec<Diagnostic>> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(vec![Diagnostic::new(
            "input.size",
            "$",
            "scenario exceeds 8 MiB",
        )]);
    }
    // PowerShell/Windows editors may write a UTF-8 BOM. It is encoding metadata,
    // not part of the JSON document; accept exactly one leading BOM.
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let UniqueJson(value) = serde_json::from_slice(bytes)
        .map_err(|e| vec![Diagnostic::new("input.json", "$", e.to_string())])?;
    Ok(value)
}

pub fn decode<T: DeserializeOwned>(value: Value) -> Result<T, Vec<Diagnostic>> {
    serde_path_to_error::deserialize(value).map_err(|e| {
        let path = e.path().to_string();
        let path = if path == "." {
            "$".into()
        } else {
            format!("$.{path}")
        };
        vec![Diagnostic::new("input.shape", path, e.inner().to_string())]
    })
}

pub fn schema() -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(Scenario)).expect("generated schema serializes");
    schema["$id"] = json!("urn:reqon:scenario:prototype:v1");
    schema["description"] = json!(
        "Experimental Reqon scenario structure. Run `validate` for semantic checks; this schema does not verify remote contracts or JavaScript behavior."
    );
    schema["properties"]["schema_version"]["const"] = json!(FORMAT);
    schema
}

pub fn capabilities() -> Value {
    json!({
        "schema_version": "reqon-prototype/capabilities-v1",
        "engine_version": env!("CARGO_PKG_VERSION"),
        "scenario_formats": [FORMAT],
        "input_formats": ["json", "java-yaml"],
        "commands": ["run", "validate", "plan", "schema", "capabilities", "recover", "request"],
        "gui_request": {"protocol":1,"transport":"stdio-json-lines","full_response_chunks":true,"grpc":["unary","server-streaming","client-streaming","bidi-streaming"],"http":true,"cancellation":"terminate-owned-process","regression_reports":false},
        "package_formats": [crate::package::FORMAT],
        "case_formats": [crate::package::CASE_FORMAT],
        "suite_formats": [crate::package::SUITE_FORMAT],
        "environment_formats": [crate::runtime::FORMAT],
        "variables": {"package_defaults":true,"case_defaults":true,"environment":true,"cli_json_overrides":true,"runtime_case_isolation":true,"recursive_interpolation":false,"max_entries":crate::runtime::MAX_VARIABLES,"max_bytes":crate::runtime::MAX_CONTEXT_BYTES},
        "secrets": {"process_environment_refs":true,"cli_literal_values":false,"immutable_bindings":true,"report_mode":"secret-context/v1","withhold_dynamic_values":true,"external_providers":false},
        "package_capabilities": crate::package::FEATURES,
        "packages": {"stable_ids":true,"explicit_suites":true,"deterministic_plan":true,"case_selection":true,"tag_filter":true,"yaml":false,"max_input_bytes":crate::package::MAX_PACKAGE_BYTES,"max_files":crate::package::MAX_FILES},
        "step_types": ["grpc", "javascript", "http", "pause", "regression_call", "kafka"],
        "http": {"https":true,"methods":true,"query":true,"json":true,"text":true,"form":true,"multipart":true,"binary":true,"cookies":"per-case","auth":["basic","bearer"],"redirects":"explicit-boundaries","proxy":"explicit-http-connect","compression":["gzip","br","deflate"]},
        "grpc": {"unary": true, "streaming": false, "reflection": ["v1", "v1alpha"], "local_descriptors": true,"local_proto":true, "tls": true,"mtls":true,"proxy":true,"metadata":true,"trailers":true,"emit_defaults":"configurable", "endpoint_scope": "https-or-explicit-remote-plaintext"},
        "assertions": {"targets":["body","status","headers","trailers","elapsed_ms","grpc_code"],"operations":["equals","contains","exists","schema","less_than","greater_than"],"schema_refs":"local-fragments-only","regex":"linear-time","missing_distinct_from_null":true},
        "runner": {"sequential": true, "poll": true, "typed_variables": true, "stop_on_failure": true,"bail":true,"default_case_policy":"continue","pause":true,"delay":true, "parallel": false, "iteration_data": ["json","csv"],"max_instances":10000,"max_planned_steps":50000},
        "reports": ["json", "junit", "html", "recovery-journal"],
        "progress": "ndjson",
        "cancellation": ["stdin", "ctrl_c", "sigterm-unix"],
        "javascript": {"runtime": "quickjs", "separate_process": true,"process_reuse":"per-case","vm_context":"fresh-per-script","hooks":["pre","post","tests"],"reqon_api":["vars.get","vars.set","assert","test","log","send"],"send_policy":"explicit-origin-and-call-budget", "async_api": false, "host_io": false, "os_sandbox": false, "heap_bytes": 16*1024*1024},
        "limits": {"input_bytes": MAX_INPUT_BYTES, "steps": 1000, "run_timeout_ms": 3600000},
        "exit_codes": {"passed": 0, "failed": 1, "error": 2, "cancelled": 130},
        "unsupported": ["graphql_profile", "grpc_streaming", "sql", "native_package_yaml", "git_catalog", "database_history", "oauth2_refresh", "ws", "sse", "parallel_cases"],
        "java_compatibility": {"formats":["yaml", "json"],"steps":["CALL","KAFKA","PAUSE"],"kafka_offset_commits":false,"kafka_partition_snapshot":"chain-start"},
        "production_ready": false
    })
}
