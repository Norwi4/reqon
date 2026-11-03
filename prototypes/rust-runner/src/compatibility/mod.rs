//! Explicit compatibility profile for the Java regression scenario format.
mod execute;
use crate::{CheckResult, package, runtime::Variables, spec::Step};
use anyhow::{Context, Result, bail};
pub(crate) use execute::call;
pub(crate) use execute::save;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

fn yes() -> bool {
    true
}
fn positive() -> String {
    "positive".into()
}
fn interval() -> String {
    "5s".into()
}
fn poll_timeout() -> String {
    "60s".into()
}
fn kafka_timeout() -> String {
    "30s".into()
}
fn fail() -> String {
    "fail".into()
}
fn chain() -> String {
    "chain-start".into()
}
fn json_format() -> String {
    "json".into()
}
fn call_timeout() -> u64 {
    60_000
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct Expectation {
    pub success: bool,
    pub error_code: Option<String>,
    pub equal_to: BTreeMap<String, Value>,
    pub not_null: Vec<String>,
    pub not_empty: Vec<String>,
}
impl Default for Expectation {
    fn default() -> Self {
        Self {
            success: yes(),
            error_code: None,
            equal_to: BTreeMap::new(),
            not_null: vec![],
            not_empty: vec![],
        }
    }
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Poll {
    #[serde(default = "interval")]
    pub interval: String,
    #[serde(default = "poll_timeout")]
    pub timeout: String,
    #[serde(default = "fail")]
    pub on_timeout: String,
    #[serde(default)]
    pub until: Option<Expectation>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KafkaWait {
    pub topic: String,
    #[serde(default = "kafka_timeout")]
    pub timeout: String,
    #[serde(default = "chain")]
    pub from: String,
    #[serde(default)]
    pub absent: bool,
    #[serde(default = "json_format")]
    pub format: String,
    #[serde(default, rename = "match")]
    pub matches: BTreeMap<String, Value>,
    #[serde(default)]
    pub expect: Expectation,
    #[serde(default)]
    pub save: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub service: String,
    pub method: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub request: BTreeMap<String, Value>,
    #[serde(default)]
    pub expect: Expectation,
    #[serde(default)]
    pub save: BTreeMap<String, String>,
    #[serde(default)]
    pub poll: Option<Poll>,
}
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub targets: BTreeMap<String, String>,
    pub addresses: BTreeMap<String, String>,
    #[serde(default = "yes")]
    pub use_default_endpoint: bool,
    #[serde(default = "call_timeout")]
    pub call_timeout_ms: u64,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            targets: BTreeMap::new(),
            addresses: BTreeMap::new(),
            use_default_endpoint: true,
            call_timeout_ms: call_timeout(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.call_timeout_ms > 3_600_000 || self.targets.len() + self.addresses.len() > 256 {
            bail!("invalid regression profile limits");
        }
        for endpoint in self.targets.values().chain(self.addresses.values()) {
            if !endpoint.trim().is_empty() {
                crate::transport::url_allowed(endpoint, true)?;
            }
        }
        Ok(())
    }
    pub fn timeout(&self) -> u64 {
        if self.call_timeout_ms == 0 {
            call_timeout()
        } else {
            self.call_timeout_ms
        }
    }
    pub fn endpoint<'a>(&'a self, call: &Call, fallback: &'a str) -> Result<&'a str> {
        if let Some(address) = self
            .addresses
            .get(&call.service)
            .filter(|v| !v.trim().is_empty())
        {
            return Ok(address.trim());
        }
        if let Some(target) = call.target.as_deref().filter(|v| !v.trim().is_empty())
            && let Some((_, address)) = self
                .targets
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(target.trim()))
        {
            if address.trim().is_empty() {
                bail!("INVOKER_NOT_CONFIGURED");
            }
            return Ok(address.trim());
        }
        if !self.use_default_endpoint {
            bail!("INVOKER_NOT_CONFIGURED");
        }
        Ok(fallback)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyScenario {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    bf: Option<String>,
    #[serde(default)]
    domain: Option<String>,
    #[serde(default = "positive")]
    kind: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    zephyr: Option<String>,
    #[serde(default)]
    disabled: bool,
    steps: Vec<LegacyStep>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LegacyStep {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    service: Option<String>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    request: BTreeMap<String, Value>,
    #[serde(default)]
    expect: Expectation,
    #[serde(default)]
    save: BTreeMap<String, String>,
    #[serde(default)]
    disabled: Vec<String>,
    #[serde(default)]
    poll: Option<Poll>,
    #[serde(default)]
    kafka: Option<KafkaWait>,
    #[serde(default)]
    pause: Option<String>,
}

pub fn duration(value: &str) -> Result<u64> {
    let value = value.trim().to_ascii_uppercase();
    let nanos = if let Some(iso) = value.strip_prefix('P') {
        let mut total = 0u64;
        let mut digits = String::new();
        let mut time = false;
        let mut terms = 0;
        let mut order = 0;
        for c in iso.chars() {
            if c == 'T' && !time && digits.is_empty() {
                time = true;
                continue;
            }
            if c.is_ascii_digit() || c == '.' || c == ',' {
                digits.push(if c == ',' { '.' } else { c });
                continue;
            }
            let (unit, rank) = match (time, c) {
                (false, 'D') => (86_400_000_000_000u64, 1),
                (true, 'H') => (3_600_000_000_000, 2),
                (true, 'M') => (60_000_000_000, 3),
                (true, 'S') => (1_000_000_000, 4),
                _ => bail!("invalid ISO duration"),
            };
            if rank <= order || digits.is_empty() {
                bail!("invalid ISO duration order");
            }
            order = rank;
            terms += 1;
            let amount = if c == 'S' && digits.contains('.') {
                let (whole, fraction) = digits.split_once('.').unwrap();
                if fraction.len() > 9 || fraction.is_empty() {
                    bail!("invalid fractional seconds");
                }
                whole
                    .parse::<u64>()?
                    .checked_mul(unit)
                    .and_then(|v| {
                        v.checked_add(
                            fraction
                                .parse::<u64>()
                                .ok()?
                                .checked_mul(10u64.pow(9 - fraction.len() as u32))?,
                        )
                    })
                    .context("duration overflow")?
            } else {
                digits
                    .parse::<u64>()?
                    .checked_mul(unit)
                    .context("duration overflow")?
            };
            total = total.checked_add(amount).context("duration overflow")?;
            digits.clear();
        }
        if terms == 0 || !digits.is_empty() || iso.ends_with('T') {
            bail!("invalid ISO duration");
        }
        total
    } else {
        let units = [
            ("NS", 1u64),
            ("US", 1_000),
            ("MS", 1_000_000),
            ("S", 1_000_000_000),
            ("M", 60_000_000_000),
            ("H", 3_600_000_000_000),
            ("D", 86_400_000_000_000),
        ];
        let (number, multiplier) = units
            .iter()
            .find_map(|(suffix, m)| value.strip_suffix(suffix).map(|n| (n, *m)))
            .unwrap_or((&value, 1_000_000));
        number
            .trim()
            .parse::<u64>()?
            .checked_mul(multiplier)
            .context("duration overflow")?
    };
    let result = nanos / 1_000_000;
    if result > 3_600_000 {
        bail!("duration exceeds the one-hour runner budget");
    }
    Ok(result)
}

pub fn find<'a>(value: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = value;
    if path.trim().is_empty() {
        return Some(current);
    }
    for part in path.trim().split('.') {
        let name = part.split('[').next()?;
        if !name.is_empty() {
            current = current.get(name)?;
        }
        let mut tail = &part[name.len()..];
        while !tail.is_empty() {
            let rest = tail.strip_prefix('[')?;
            let (index, remaining) = rest.split_once(']')?;
            current = current
                .as_array()?
                .get(index.trim().parse::<usize>().ok()?)?;
            tail = remaining;
        }
    }
    Some(current)
}
fn scalar(value: &Value) -> String {
    match value {
        Value::String(v) => v.clone(),
        _ => value.to_string(),
    }
}
pub fn equal(expected: &Value, actual: &Value) -> bool {
    if expected.is_number() && actual.is_number() {
        return decimal_key(&expected.to_string()) == decimal_key(&actual.to_string());
    }
    if !expected.is_container()
        && !actual.is_container()
        && expected.is_string() != actual.is_string()
    {
        return scalar(expected) == scalar(actual);
    }
    expected == actual
}
// Canonical decimal representation avoids rounding distinct 64-bit integers through f64.
fn decimal_key(number: &str) -> (bool, String, i32) {
    let negative = number.starts_with('-');
    let number = number.trim_start_matches('-');
    let (mantissa, exponent) = number
        .split_once(['e', 'E'])
        .map(|(m, e)| (m, e.parse::<i32>().unwrap_or(0)))
        .unwrap_or((number, 0));
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len() as i32);
    let mut digits = mantissa.replace('.', "").trim_start_matches('0').to_owned();
    if digits.is_empty() {
        return (false, "0".into(), 0);
    }
    let mut power = exponent - fraction;
    while digits.ends_with('0') {
        digits.pop();
        power += 1;
    }
    (negative, digits, power)
}
trait Container {
    fn is_container(&self) -> bool;
}
impl Container for Value {
    fn is_container(&self) -> bool {
        self.is_array() || self.is_object()
    }
}

pub fn substitute(value: &Value, vars: &Variables) -> Result<Value> {
    let result = match value {
        Value::String(v)
            if v.starts_with("${") && v.ends_with('}') && !v[2..v.len() - 1].contains("${") =>
        {
            vars.get(&v[2..v.len() - 1])
                .cloned()
                .context("undefined compatibility variable")?
        }
        Value::String(v) => {
            let mut rest = v.as_str();
            let mut output = String::new();
            while let Some((before, after)) = rest.split_once("${") {
                output.push_str(before);
                if let Some((name, tail)) = after.split_once('}') {
                    output.push_str(&scalar(
                        vars.get(name).context("undefined compatibility variable")?,
                    ));
                    rest = tail;
                } else {
                    output.push_str("${");
                    rest = after;
                    break;
                }
                if output.len() > 8 * 1024 * 1024 {
                    bail!("expanded compatibility input exceeds limit");
                }
            }
            output.push_str(rest);
            Value::String(output)
        }
        Value::Array(v) => Value::Array(
            v.iter()
                .map(|v| substitute(v, vars))
                .collect::<Result<_>>()?,
        ),
        Value::Object(v) => Value::Object(
            v.iter()
                .map(|(k, v)| Ok((k.clone(), substitute(v, vars)?)))
                .collect::<Result<_>>()?,
        ),
        _ => value.clone(),
    };
    if !crate::capture::fits(&result, 8 * 1024 * 1024) {
        bail!("expanded compatibility input exceeds limit");
    }
    Ok(result)
}

pub fn evaluate(
    expect: &Expectation,
    body: &Value,
    error: Option<&str>,
    vars: &Variables,
) -> Result<Vec<CheckResult>> {
    let mut checks = vec![];
    let mut add =
        |path: &str, expected: Value, actual: Option<Value>, passed: bool, operation: &str| {
            checks.push(CheckResult {
                path: path.into(),
                expected,
                actual,
                passed,
                operation: operation.into(),
                target: "body".into(),
            })
        };
    add(
        "$success",
        json!(expect.success),
        Some(json!(error.is_none())),
        expect.success == error.is_none(),
        "java_success",
    );
    if expect.success != error.is_none() {
        return Ok(checks);
    }
    if let Some(code) = error {
        if let Some(expected) = &expect.error_code {
            add(
                "$errorCode",
                json!(expected),
                Some(json!(code)),
                expected == code,
                "equals",
            );
        }
        return Ok(checks);
    }
    for (path, template) in &expect.equal_to {
        let expected = substitute(template, vars)?;
        let actual = find(body, path);
        let passed = actual.is_some_and(|actual| equal(&expected, actual));
        add(path, expected, actual.cloned(), passed, "java_equal");
    }
    for path in &expect.not_null {
        let actual = find(body, path);
        add(
            path,
            json!("not null"),
            actual.cloned(),
            actual.is_some_and(|v| !v.is_null()),
            "not_null",
        );
    }
    for path in &expect.not_empty {
        let actual = find(body, path);
        let passed = actual.is_some_and(|v| match v {
            Value::Null => false,
            Value::String(v) => !v.is_empty(),
            Value::Array(v) => !v.is_empty(),
            Value::Object(v) => !v.is_empty(),
            _ => true,
        });
        add(
            path,
            json!("not empty"),
            actual.cloned(),
            passed,
            "not_empty",
        );
    }
    Ok(checks)
}

pub fn validate_expectation(expect: &Expectation) -> Result<()> {
    if expect.equal_to.len() + expect.not_null.len() + expect.not_empty.len() > 256 {
        bail!("at most 256 compatibility assertions");
    }
    Ok(())
}
pub fn validate_call(call: &Call) -> Result<()> {
    if call.service.trim().is_empty()
        || call.method.trim().is_empty()
        || call.service.contains('/')
        || call.method.contains('/')
    {
        bail!("CALL requires service and method");
    }
    validate_expectation(&call.expect)?;
    validate_save(&call.save)?;
    if let Some(poll) = &call.poll {
        if duration(&poll.interval)? == 0 || duration(&poll.timeout)? == 0 {
            bail!("poll durations must be at least 1 ms");
        }
        if !["fail", "continue"].contains(&poll.on_timeout.as_str()) {
            bail!("unsupported poll.onTimeout");
        }
        if let Some(expect) = &poll.until {
            validate_expectation(expect)?;
        }
    }
    Ok(())
}
pub fn validate_save(save: &BTreeMap<String, String>) -> Result<()> {
    if save.len() > 256 || save.keys().any(|k| !crate::runtime::valid_name(k)) {
        bail!("invalid compatibility save variables");
    }
    Ok(())
}
pub fn validate_kafka(wait: &KafkaWait) -> Result<()> {
    if wait.topic.is_empty()
        || wait.topic.len() > 249
        || !wait
            .topic
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    {
        bail!("invalid Kafka topic");
    }
    duration(&wait.timeout)?;
    if !["chain-start", "step"].contains(&wait.from.as_str())
        || !["json", "string"].contains(&wait.format.as_str())
    {
        bail!("unsupported Kafka from/format");
    }
    validate_expectation(&wait.expect)?;
    validate_save(&wait.save)?;
    Ok(())
}

pub fn load(path: &Path, bytes: &[u8]) -> Result<package::Package> {
    let yaml: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(bytes).context("invalid or duplicate-key Java YAML")?;
    let value = serde_json::to_value(yaml)?;
    if !crate::capture::fits(&value, 8 * 1024 * 1024) {
        bail!("expanded YAML exceeds 8 MiB");
    }
    let source: LegacyScenario = serde_json::from_value(value)?;
    if !package::valid_id(&source.id) || source.steps.is_empty() || source.steps.len() > 1000 {
        bail!("invalid Java scenario ID or step count");
    }
    let annotation = serde_json::to_value(&source)?;
    let mut steps = Vec::new();
    for (index, step) in source.steps.into_iter().enumerate() {
        let id = format!("step-{}", index + 1);
        let call_present = step.service.as_ref().is_some_and(|s| !s.trim().is_empty())
            || step.method.as_ref().is_some_and(|s| !s.trim().is_empty());
        if usize::from(call_present)
            + usize::from(step.kafka.is_some())
            + usize::from(step.pause.is_some())
            != 1
        {
            bail!("a Java step must have exactly one of CALL, KAFKA, PAUSE");
        }
        if !call_present && step.poll.is_some() {
            bail!("poll is only supported on CALL");
        }
        if let Some(wait) = step.kafka {
            validate_kafka(&wait)?;
            steps.push(Step::Kafka {
                id,
                wait: Box::new(wait),
            });
        } else if let Some(pause) = step.pause {
            steps.push(Step::Pause {
                id,
                duration_ms: duration(&pause)?,
            });
        } else {
            let call = Call {
                service: step.service.context("CALL service missing")?,
                method: step.method.context("CALL method missing")?,
                target: step
                    .target
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| source.domain.clone()),
                request: step.request,
                expect: step.expect,
                save: step.save,
                poll: step.poll,
            };
            validate_call(&call)?;
            steps.push(Step::RegressionCall {
                id,
                call: Box::new(call),
            });
        }
    }
    let case = package::Case {
        schema_version: package::CASE_FORMAT.into(),
        id: source.id.clone(),
        name: source.name.unwrap_or_else(|| source.id.clone()),
        description: String::new(),
        tags: source.tags,
        disabled: source.disabled,
        annotations: BTreeMap::from([("vziskaniya".into(), annotation)]),
        variables: BTreeMap::new(),
        timeout_ms: 3_600_000,
        max_response_bytes: 4 * 1024 * 1024,
        steps,
    };
    let errors = case.scenario("http://127.0.0.1:1").diagnostics();
    if !errors.is_empty() {
        bail!(
            "invalid converted Java scenario: {}",
            serde_json::to_string(&errors)?
        );
    }
    let name = path
        .file_name()
        .context("scenario file name missing")?
        .to_string_lossy()
        .into_owned();
    let sha = package::hash_bytes(bytes);
    Ok(package::Package {
        manifest: package::Manifest {
            schema_version: package::FORMAT.into(),
            id: source.id.clone(),
            name: case.name.clone(),
            requires: package::Requirements {
                engine: "reqon".into(),
                version: format!("={}", env!("CARGO_PKG_VERSION")),
                capabilities: vec!["regression.java".into(), "kafka".into(), "pause".into()],
            },
            variables: BTreeMap::new(),
            cases: vec![name.clone()],
            suites: vec![],
            default_suite: None,
            timeout_ms: 3_600_000,
        },
        cases: vec![package::LoadedCase {
            sha256: package::hash_bytes(&serde_json::to_vec(&case)?),
            definition: case,
            file: name.clone(),
        }],
        suites: vec![],
        files: vec![package::SourceFile {
            path: name,
            sha256: sha.clone(),
        }],
        package_sha256: sha,
        source_paths: vec![path.canonicalize()?],
        assets: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_comparison_does_not_round_large_integers() {
        assert!(!equal(
            &json!(9007199254740992u64),
            &json!(9007199254740993u64)
        ));
        assert!(equal(&json!(1), &json!(1.0)));
        assert!(equal(&json!(1000), &json!(1e3)));
        assert!(equal(&json!(-0.0), &json!(0)));
        assert!(equal(&json!("3"), &json!(3)));
    }
    #[test]
    fn java_duration_forms_are_bounded_and_exact() {
        for (text, ms) in [
            ("0ms", 0),
            ("2", 2),
            ("1h", 3_600_000),
            ("PT1M2.003S", 62003),
            ("P0DT1S", 1000),
            ("1500us", 1),
        ] {
            assert_eq!(duration(text).unwrap(), ms, "{text}");
        }
        for text in ["-1s", "2h", "PT", "PT1S2M", "PT1.1234567891S", "1.5s"] {
            assert!(duration(text).is_err(), "{text}");
        }
    }
}
