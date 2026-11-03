use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};

pub const FORMAT: &str = "reqon-prototype/v1";
pub(crate) fn run_timeout() -> u64 {
    10_000
}
pub(crate) fn call_timeout() -> u64 {
    2_000
}
pub(crate) fn response_limit() -> usize {
    4 * 1024 * 1024
}
fn empty_object() -> Value {
    serde_json::json!({})
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub schema_version: String,
    pub id: String,
    pub endpoint: String,
    #[serde(default = "run_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "response_limit")]
    pub max_response_bytes: usize,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    RegressionCall {
        id: String,
        call: Box<crate::compatibility::Call>,
    },
    Kafka {
        id: String,
        wait: Box<crate::compatibility::KafkaWait>,
    },
    Grpc {
        id: String,
        method: String,
        #[serde(default = "empty_object")]
        request: Value,
        #[serde(default)]
        expect: Vec<Check>,
        #[serde(default)]
        save: BTreeMap<String, String>,
        #[serde(default)]
        poll: Option<Poll>,
        #[serde(default = "call_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        expect_code: Option<String>,
        #[serde(default)]
        hooks: Hooks,
    },
    Http {
        id: String,
        request: Box<crate::http_client::RequestSpec>,
        #[serde(default)]
        expect: Vec<Check>,
        #[serde(default)]
        save: BTreeMap<String, String>,
        #[serde(default)]
        poll: Option<Poll>,
        #[serde(default = "call_timeout")]
        timeout_ms: u64,
        #[serde(default)]
        hooks: Hooks,
    },
    Pause {
        id: String,
        duration_ms: u64,
    },
    Javascript {
        id: String,
        script: String,
        #[serde(default = "call_timeout")]
        timeout_ms: u64,
    },
}

impl Step {
    pub fn id(&self) -> &str {
        match self {
            Self::Grpc { id, .. }
            | Self::RegressionCall { id, .. }
            | Self::Kafka { id, .. }
            | Self::Http { id, .. }
            | Self::Pause { id, .. }
            | Self::Javascript { id, .. } => id,
        }
    }
    pub fn file_paths(&self) -> Vec<&str> {
        if let Self::Http { request, .. } = self {
            request.body.paths()
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Hooks {
    pub pre: Vec<ScriptHook>,
    pub post: Vec<ScriptHook>,
    pub tests: Vec<ScriptHook>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScriptHook {
    pub script: String,
    #[serde(default = "call_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckTarget {
    #[default]
    Body,
    Status,
    Headers,
    Trailers,
    ElapsedMs,
    GrpcCode,
}

fn present_value<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Value>, D::Error> {
    Value::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    #[serde(default)]
    pub target: CheckTarget,
    #[serde(default)]
    pub path: String,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub equals: Option<Value>,
    #[serde(
        default,
        deserialize_with = "present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub contains: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exists: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub less_than: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub greater_than: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Poll {
    pub interval_ms: u64,
    pub timeout_ms: u64,
    /// Explicit opt-in; only idempotent/read-only calls may be polled by this PoC.
    pub read_only: bool,
    pub until: Vec<Check>,
}

impl Scenario {
    pub fn validate(&self) -> Result<()> {
        let errors = self.diagnostics();
        if !errors.is_empty() {
            bail!(
                "{}",
                errors
                    .iter()
                    .map(|e| format!("{}: {}", e.path, e.message))
                    .collect::<Vec<_>>()
                    .join("; ")
            );
        }
        Ok(())
    }

    pub fn diagnostics(&self) -> Vec<crate::input::Diagnostic> {
        let mut errors = Vec::new();
        let mut add = |code, path: String, message: &str| {
            errors.push(crate::input::Diagnostic::new(code, path, message))
        };
        if self.schema_version != FORMAT {
            add(
                "scenario.version",
                "$.schema_version".into(),
                "unsupported schema_version",
            );
        }
        if self.id.trim().is_empty() {
            add(
                "scenario.id",
                "$.id".into(),
                "scenario id must not be empty",
            );
        }
        if self.steps.is_empty() {
            add(
                "scenario.steps",
                "$.steps".into(),
                "steps must not be empty",
            );
        }
        if self.steps.len() > 1000 {
            add(
                "scenario.steps",
                "$.steps".into(),
                "prototype limit: 1000 steps",
            );
        }
        if self.timeout_ms == 0 || self.timeout_ms > 3_600_000 {
            add(
                "scenario.timeout",
                "$.timeout_ms".into(),
                "run timeout must be 1..3600000 ms",
            );
        }
        if !(1024..=64 * 1024 * 1024).contains(&self.max_response_bytes) {
            add(
                "scenario.response_limit",
                "$.max_response_bytes".into(),
                "response limit must be 1 KiB..64 MiB",
            );
        }
        // The experiment must not accidentally send credentials to a remote cleartext service.
        let endpoint_ok = crate::transport::url_allowed(&self.endpoint, true).is_ok_and(|uri| {
            !self.steps.iter().any(|s| matches!(s, Step::Grpc { .. }))
                || (uri.path() == "/" && uri.query().is_none())
        });
        if !endpoint_ok {
            add(
                "scenario.endpoint",
                "$.endpoint".into(),
                "endpoint requires HTTP(S), no credentials/fragment, and no path/query for gRPC",
            );
        }
        for (key, value) in &self.metadata {
            if key
                .parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()
                .is_err()
                || value
                    .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                    .is_err()
            {
                add(
                    "scenario.metadata",
                    "$.metadata".into(),
                    "metadata must contain valid ASCII gRPC keys and values",
                );
            }
        }
        let mut ids = HashSet::new();
        for (index, step) in self.steps.iter().enumerate() {
            let path = format!("$.steps[{index}]");
            if step.id().trim().is_empty() || !ids.insert(step.id()) {
                add(
                    "step.id",
                    format!("{path}.id"),
                    "step ids must be nonempty and unique",
                );
            }
            let timeout = match step {
                Step::RegressionCall { call, .. } => {
                    if let Err(error) = crate::compatibility::validate_call(call) {
                        add("regression.call", path.clone(), &error.to_string());
                    }
                    self.timeout_ms
                }
                Step::Kafka { wait, .. } => {
                    if let Err(error) = crate::compatibility::validate_kafka(wait) {
                        add("kafka.wait", path.clone(), &error.to_string());
                    }
                    self.timeout_ms
                }
                Step::Grpc {
                    method,
                    expect,
                    save,
                    poll,
                    timeout_ms,
                    expect_code,
                    ..
                } => {
                    if expect.len() > 256 || poll.as_ref().is_some_and(|p| p.until.len() > 256) {
                        add("check.limit", path.clone(), "at most 256 checks per phase");
                    }
                    if expect_code.as_deref().is_some_and(|c| {
                        ![
                            "OK",
                            "Cancelled",
                            "Unknown",
                            "InvalidArgument",
                            "DeadlineExceeded",
                            "NotFound",
                            "AlreadyExists",
                            "PermissionDenied",
                            "ResourceExhausted",
                            "FailedPrecondition",
                            "Aborted",
                            "OutOfRange",
                            "Unimplemented",
                            "Internal",
                            "Unavailable",
                            "DataLoss",
                            "Unauthenticated",
                        ]
                        .contains(&c)
                    }) {
                        add("grpc.code", path.clone(), "unknown expected gRPC code");
                    }
                    if method.split('/').count() != 2 || method.split('/').any(str::is_empty) {
                        add(
                            "grpc.method",
                            format!("{path}.method"),
                            "method must be package.Service/Method",
                        );
                    }
                    for (i, check) in expect.iter().enumerate() {
                        if crate::assertions::validate(check).is_err() {
                            add(
                                "check.pointer",
                                format!("{path}.expect[{i}].path"),
                                "invalid check: exactly one operation, valid JSON Pointer and offline schema required",
                            );
                        }
                    }
                    for pointer in save.values() {
                        if validate_pointer(pointer).is_err() {
                            add(
                                "save.pointer",
                                format!("{path}.save"),
                                "invalid JSON Pointer; use /field and ~0/~1 escapes",
                            );
                        }
                    }
                    if save.keys().any(|name| !crate::runtime::valid_name(name)) {
                        add(
                            "save.variable",
                            format!("{path}.save"),
                            "save targets require safe variable identifiers",
                        );
                    }
                    if let Some(poll) = poll {
                        if !poll.read_only
                            || poll.until.is_empty()
                            || poll.interval_ms == 0
                            || poll.timeout_ms == 0
                            || poll.timeout_ms > self.timeout_ms
                        {
                            add(
                                "poll.configuration",
                                format!("{path}.poll"),
                                "poll requires read_only=true, conditions, positive interval and bounded timeout",
                            );
                        }
                        for (i, check) in poll.until.iter().enumerate() {
                            if crate::assertions::validate(check).is_err() {
                                add(
                                    "check.pointer",
                                    format!("{path}.poll.until[{i}].path"),
                                    "invalid JSON Pointer; use /field and ~0/~1 escapes",
                                );
                            }
                        }
                    }
                    *timeout_ms
                }
                Step::Javascript {
                    script, timeout_ms, ..
                } => {
                    if script.len() > 256 * 1024 {
                        add(
                            "script.size",
                            format!("{path}.script"),
                            "script exceeds 256 KiB",
                        );
                    }
                    if *timeout_ms > 60_000 {
                        add(
                            "script.timeout",
                            format!("{path}.timeout_ms"),
                            "script timeout must not exceed 60000 ms",
                        );
                    }
                    *timeout_ms
                }
                Step::Http {
                    request,
                    expect,
                    save,
                    poll,
                    timeout_ms,
                    ..
                } => {
                    if expect.len() > 256 || poll.as_ref().is_some_and(|p| p.until.len() > 256) {
                        add("check.limit", path.clone(), "at most 256 checks per phase");
                    }
                    if let Err(e) = request.validate() {
                        add("http.request", format!("{path}.request"), &e.to_string());
                    }
                    for (i, check) in expect.iter().enumerate() {
                        if let Err(e) = crate::assertions::validate(check) {
                            add(
                                "check.configuration",
                                format!("{path}.expect[{i}]"),
                                &e.to_string(),
                            );
                        }
                    }
                    if save.keys().any(|s| !crate::runtime::valid_name(s))
                        || save.values().any(|s| validate_pointer(s).is_err())
                    {
                        add(
                            "save.configuration",
                            format!("{path}.save"),
                            "invalid save variable or JSON pointer",
                        );
                    }
                    if let Some(poll) = poll
                        && (!poll.read_only
                            || poll.until.is_empty()
                            || poll.interval_ms == 0
                            || poll.timeout_ms == 0
                            || poll.timeout_ms > self.timeout_ms
                            || poll
                                .until
                                .iter()
                                .any(|c| crate::assertions::validate(c).is_err()))
                    {
                        add(
                            "poll.configuration",
                            format!("{path}.poll"),
                            "invalid bounded read-only polling configuration",
                        );
                    }
                    *timeout_ms
                }
                Step::Pause { duration_ms, .. } => *duration_ms,
            };
            let hooks = match step {
                Step::Grpc { hooks, .. } | Step::Http { hooks, .. } => Some(hooks),
                _ => None,
            };
            if let Some(hooks) = hooks {
                let all: Vec<_> = hooks
                    .pre
                    .iter()
                    .chain(&hooks.post)
                    .chain(&hooks.tests)
                    .collect();
                if all.len() > 32
                    || all.iter().any(|h| {
                        h.script.len() > 256 * 1024
                            || h.timeout_ms == 0
                            || h.timeout_ms > self.timeout_ms.min(60_000)
                    })
                {
                    add(
                        "hooks.configuration",
                        format!("{path}.hooks"),
                        "hooks exceed count/script/timeout limits",
                    );
                }
            }
            if (timeout == 0 && !matches!(step, Step::Pause { .. })) || timeout > self.timeout_ms {
                add(
                    "step.timeout",
                    format!("{path}.timeout_ms"),
                    "step timeout must be positive and no greater than run timeout",
                );
            }
        }
        errors
    }
}

pub(crate) fn validate_pointer(path: &str) -> Result<()> {
    if !path.is_empty() && !path.starts_with('/') {
        bail!("checks and save use JSON Pointers, e.g. /serverCallInfo/success");
    }
    let mut bytes = path.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
            bail!("invalid JSON Pointer escape");
        }
    }
    Ok(())
}

/// String templates are explicit; JSON whole-value substitution remains typed.
pub fn resolve_text(input: &str, vars: &BTreeMap<String, Value>) -> Result<String> {
    let mut result = String::new();
    let mut rest = input;
    while let Some(start) = rest.find("${") {
        if result.len() + start > 8 * 1024 * 1024 {
            bail!("resolved text exceeds 8 MiB");
        }
        result.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| anyhow::anyhow!("unterminated variable reference"))?;
        let name = &tail[..end];
        let value = vars
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("unknown variable: {name}"))?;
        match value {
            Value::String(s) => {
                if result.len() + s.len() > 8 * 1024 * 1024 {
                    bail!("resolved text exceeds 8 MiB");
                }
                result.push_str(s)
            }
            Value::Number(_) | Value::Bool(_) => result.push_str(&value.to_string()),
            _ => bail!("string templates require string/number/boolean variables"),
        }
        rest = &tail[end + 1..];
    }
    if result.len() + rest.len() > 8 * 1024 * 1024 {
        bail!("resolved text exceeds 8 MiB");
    }
    result.push_str(rest);
    Ok(result)
}

/// A whole-value placeholder preserves types. No arbitrary string interpolation.
pub fn resolve(value: &Value, vars: &BTreeMap<String, Value>) -> Result<Value> {
    resolve_bounded(value, vars, &mut (8 * 1024 * 1024))
}
fn resolve_bounded(
    value: &Value,
    vars: &BTreeMap<String, Value>,
    budget: &mut usize,
) -> Result<Value> {
    let charge = |budget: &mut usize, n: usize| -> Result<()> {
        if n > *budget {
            bail!("resolved JSON exceeds 8 MiB");
        }
        *budget -= n;
        Ok(())
    };
    Ok(match value {
        Value::String(s) if s.starts_with("${") && s.ends_with('}') => {
            let value = vars
                .get(&s[2..s.len() - 1])
                .ok_or_else(|| anyhow::anyhow!("unknown variable: {}", &s[2..s.len() - 1]))?;
            let bytes = crate::capture::size_within(value, *budget)
                .ok_or_else(|| anyhow::anyhow!("resolved JSON exceeds 8 MiB"))?;
            charge(budget, bytes)?;
            value.clone()
        }
        Value::Array(items) => {
            charge(budget, items.len() + 2)?;
            Value::Array(
                items
                    .iter()
                    .map(|v| resolve_bounded(v, vars, budget))
                    .collect::<Result<_>>()?,
            )
        }
        Value::Object(items) => {
            charge(budget, items.len() + 2)?;
            Value::Object(
                items
                    .iter()
                    .map(|(k, v)| {
                        let bytes = crate::capture::size_within(k, *budget)
                            .ok_or_else(|| anyhow::anyhow!("resolved JSON exceeds 8 MiB"))?;
                        charge(budget, bytes + 1)?;
                        Ok((k.clone(), resolve_bounded(v, vars, budget)?))
                    })
                    .collect::<Result<_>>()?,
            )
        }
        _ => {
            let bytes = crate::capture::size_within(value, *budget)
                .ok_or_else(|| anyhow::anyhow!("resolved JSON exceeds 8 MiB"))?;
            charge(budget, bytes)?;
            value.clone()
        }
    })
}
