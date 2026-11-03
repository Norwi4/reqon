use crate::{
    CheckResult,
    http_client::Headers,
    runtime::Variables,
    spec::{self, Check, CheckTarget},
};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Cache {
    schemas: HashMap<String, jsonschema::Validator>,
}

fn compile(schema: &Value) -> Result<jsonschema::Validator> {
    jsonschema::options()
        .with_pattern_options(jsonschema::PatternOptions::regex())
        .build(schema)
        .map_err(|_| {
            anyhow::anyhow!(
                "invalid assertion JSON Schema (patterns must use linear-time regular expressions)"
            )
        })
}

pub struct ResponseView<'a> {
    pub body: &'a Value,
    pub status: Option<u16>,
    pub headers: &'a Headers,
    pub trailers: &'a Headers,
    pub elapsed_ms: f64,
    pub grpc_code: Option<&'a str>,
}

fn offline(schema: &Value) -> Result<()> {
    match schema {
        Value::Object(obj) => {
            for (k, v) in obj {
                if matches!(k.as_str(), "$ref" | "$dynamicRef" | "$recursiveRef")
                    && !v.as_str().is_some_and(|s| s.starts_with('#'))
                {
                    bail!("JSON Schema permits local fragment references only");
                }
                offline(v)?;
            }
        }
        Value::Array(items) => {
            for v in items {
                offline(v)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn validate(check: &Check) -> Result<()> {
    spec::validate_pointer(&check.path)?;
    let count = [
        check.equals.is_some(),
        check.contains.is_some(),
        check.exists.is_some(),
        check.schema.is_some(),
        check.less_than.is_some(),
        check.greater_than.is_some(),
    ]
    .into_iter()
    .filter(|x| *x)
    .count();
    if count != 1 {
        bail!("check requires exactly one comparison operation");
    }
    if let Some(schema) = &check.schema {
        if serde_json::to_vec(schema)?.len() > 256 * 1024 {
            bail!("assertion schema exceeds 256 KiB");
        }
        offline(schema)?;
        compile(schema)?;
    }
    Ok(())
}

impl Cache {
    pub fn evaluate(
        &mut self,
        response: &ResponseView<'_>,
        checks: &[Check],
        vars: &Variables,
    ) -> Result<Vec<CheckResult>> {
        let mut out = Vec::with_capacity(checks.len());
        for check in checks {
            let root = match check.target {
                CheckTarget::Body => None,
                CheckTarget::Status => {
                    Some(response.status.map(|s| json!(s)).unwrap_or(Value::Null))
                }
                CheckTarget::Headers => Some(serde_json::to_value(response.headers)?),
                CheckTarget::Trailers => Some(serde_json::to_value(response.trailers)?),
                CheckTarget::ElapsedMs => Some(json!(response.elapsed_ms)),
                CheckTarget::GrpcCode => {
                    Some(response.grpc_code.map(|s| json!(s)).unwrap_or(Value::Null))
                }
            };
            let actual = root.as_ref().unwrap_or(response.body).pointer(&check.path);
            let (expected, passed, operation) = if let Some(expected) = &check.equals {
                let expected = spec::resolve(expected, vars)?;
                let passed = actual == Some(&expected);
                (expected, passed, "equals")
            } else if let Some(expected) = &check.contains {
                let expected = spec::resolve(expected, vars)?;
                let passed = match (actual, &expected) {
                    (Some(Value::String(s)), Value::String(part)) => s.contains(part),
                    (Some(Value::Array(items)), value) => items.contains(value),
                    (Some(Value::Object(obj)), Value::Object(subset)) => {
                        subset.iter().all(|(k, v)| obj.get(k) == Some(v))
                    }
                    _ => false,
                };
                (expected, passed, "contains")
            } else if let Some(exists) = check.exists {
                (json!(exists), actual.is_some() == exists, "exists")
            } else if let Some(schema) = &check.schema {
                let key = serde_json::to_string(schema)?;
                if !self.schemas.contains_key(&key) {
                    offline(schema)?;
                    if self.schemas.len() >= 32 {
                        self.schemas.clear();
                    }
                    self.schemas.insert(key.clone(), compile(schema)?);
                }
                (
                    schema.clone(),
                    actual.is_some_and(|value| self.schemas[&key].is_valid(value)),
                    "schema",
                )
            } else if let Some(bound) = check.less_than {
                (
                    json!(bound),
                    actual.and_then(Value::as_f64).is_some_and(|n| n < bound),
                    "less_than",
                )
            } else if let Some(bound) = check.greater_than {
                (
                    json!(bound),
                    actual.and_then(Value::as_f64).is_some_and(|n| n > bound),
                    "greater_than",
                )
            } else {
                bail!("check has no comparison");
            };
            out.push(CheckResult {
                path: check.path.clone(),
                expected: crate::capture::value(&expected, 4096),
                actual: actual.map(|v| crate::capture::value(v, 4096)),
                passed,
                operation: operation.into(),
                target: serde_json::to_value(&check.target)?
                    .as_str()
                    .unwrap()
                    .into(),
            });
        }
        Ok(out)
    }
}
