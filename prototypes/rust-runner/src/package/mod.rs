//! Versioned, local JSON packages. No Git or database side effects.
mod execution;
mod load;
pub use execution::{CaseResult, PackageReport, run};
pub use load::load;

use crate::{
    input::Diagnostic,
    spec::{self, Scenario, Step},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    path::PathBuf,
};

pub const FORMAT: &str = "reqon-package/v1";
pub const CASE_FORMAT: &str = "reqon-case/v1";
pub const SUITE_FORMAT: &str = "reqon-suite/v1";
pub const FEATURES: &[&str] = &[
    "grpc.unary",
    "javascript",
    "poll",
    "variables",
    "http",
    "pause",
    "hooks",
    "regression.java",
    "kafka",
];
pub const MAX_PACKAGE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_FILES: usize = 1024;
pub type Validation<T> = Result<T, Vec<Diagnostic>>;
fn package_timeout() -> u64 {
    60_000
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Requirements {
    pub engine: String,
    pub version: String,
    pub capabilities: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    pub requires: Requirements,
    #[serde(default)]
    pub variables: crate::runtime::Variables,
    pub cases: Vec<String>,
    #[serde(default)]
    pub suites: Vec<String>,
    #[serde(default)]
    pub default_suite: Option<String>,
    #[serde(default = "package_timeout")]
    pub timeout_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Case {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub disabled: bool,
    /// Non-executable, namespaced metadata for adapters (e.g. vziskaniya).
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    #[serde(default)]
    pub variables: crate::runtime::Variables,
    #[serde(default = "spec::run_timeout")]
    pub timeout_ms: u64,
    #[serde(default = "spec::response_limit")]
    pub max_response_bytes: usize,
    pub steps: Vec<Step>,
}

impl Case {
    pub fn scenario(&self, endpoint: &str) -> Scenario {
        Scenario {
            schema_version: spec::FORMAT.into(),
            id: self.id.clone(),
            endpoint: endpoint.into(),
            timeout_ms: self.timeout_ms,
            max_response_bytes: self.max_response_bytes,
            metadata: BTreeMap::new(),
            steps: self.steps.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Suite {
    pub schema_version: String,
    pub id: String,
    pub name: String,
    /// Explicit case IDs in execution order; not paths or recursive suite references.
    pub cases: Vec<String>,
}

pub struct LoadedCase {
    pub definition: Case,
    pub file: String,
    pub sha256: String,
}
pub struct LoadedSuite {
    pub definition: Suite,
    pub file: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceFile {
    pub path: String,
    pub sha256: String,
}

pub struct Package {
    pub manifest: Manifest,
    pub cases: Vec<LoadedCase>,
    pub suites: Vec<LoadedSuite>,
    pub files: Vec<SourceFile>,
    pub package_sha256: String,
    pub(crate) source_paths: Vec<PathBuf>,
    pub assets: crate::http_client::Assets,
}

pub struct Selection {
    pub suite: Option<String>,
    pub cases: Vec<String>,
    pub tags: Vec<String>,
    pub exclude_tags: Vec<String>,
    pub iterations: usize,
    pub dataset: Option<crate::dataset::Dataset>,
    pub delay_ms: u64,
    pub bail: bool,
}
impl Default for Selection {
    fn default() -> Self {
        Self {
            suite: None,
            cases: Vec::new(),
            tags: Vec::new(),
            exclude_tags: Vec::new(),
            iterations: 1,
            dataset: None,
            delay_ms: 0,
            bail: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedCase {
    pub id: String,
    pub name: String,
    pub file: String,
    /// Hash of normalized typed case JSON, independent of path and formatting.
    pub sha256: String,
    pub disabled: bool,
    pub tags: Vec<String>,
    pub annotations: BTreeMap<String, Value>,
    pub step_ids: Vec<String>,
    pub instance_id: String,
    pub iteration: usize,
    pub data_row: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Plan {
    pub schema_version: String,
    pub engine_version: String,
    pub package_id: String,
    pub package_sha256: String,
    pub plan_sha256: String,
    pub suite_id: Option<String>,
    pub timeout_ms: u64,
    pub files: Vec<SourceFile>,
    pub cases: Vec<PlannedCase>,
    pub data_sha256: Option<String>,
    pub delay_ms: u64,
    pub bail: bool,
}

impl Plan {
    pub(crate) fn redact(&mut self, policy: &crate::secrets::OutputPolicy) {
        self.package_id = policy.text(&self.package_id);
        self.suite_id = self.suite_id.as_ref().map(|s| policy.text(s));
        for file in &mut self.files {
            file.path = policy.text(&file.path);
        }
        for case in &mut self.cases {
            case.id = policy.text(&case.id);
            case.instance_id = policy.text(&case.instance_id);
            case.name = policy.text(&case.name);
            case.file = policy.text(&case.file);
            case.step_ids.iter_mut().for_each(|s| *s = policy.text(s));
            case.tags.iter_mut().for_each(|s| *s = policy.text(s));
            case.annotations = std::mem::take(&mut case.annotations)
                .into_iter()
                .map(|(k, mut v)| {
                    policy.value(&mut v);
                    (policy.text(&k), v)
                })
                .collect();
        }
    }
}

impl Package {
    pub fn input_paths(&self) -> &[PathBuf] {
        &self.source_paths
    }

    pub fn plan(&self, selection: &Selection) -> Validation<Plan> {
        let error = |message: &str| vec![Diagnostic::new("package.selection", "$", message)];
        if !(1..=1000).contains(&selection.iterations) || selection.delay_ms > 60_000 {
            return Err(error(
                "iterations must be 1..1000 and delay at most 60000 ms",
            ));
        }
        if selection
            .tags
            .iter()
            .chain(&selection.exclude_tags)
            .any(|t| t.is_empty() || t.len() > 128)
        {
            return Err(error("invalid tag filter"));
        }
        if selection.suite.is_some() && !selection.cases.is_empty() {
            return Err(error("--suite and --case cannot be combined"));
        }
        let suite_id = if selection.cases.is_empty() {
            selection
                .suite
                .as_ref()
                .or(self.manifest.default_suite.as_ref())
        } else {
            None
        };
        let ids = if let Some(id) = suite_id {
            self.suites
                .iter()
                .find(|s| &s.definition.id == id)
                .ok_or_else(|| error("unknown suite ID"))?
                .definition
                .cases
                .clone()
        } else if selection.cases.is_empty() {
            self.cases.iter().map(|c| c.definition.id.clone()).collect()
        } else {
            selection.cases.clone()
        };
        let mut seen = HashSet::new();
        let mut cases = Vec::new();
        for id in ids {
            if !seen.insert(id.clone()) {
                return Err(error("duplicate case ID in selection"));
            }
            let case = self
                .cases
                .iter()
                .find(|c| c.definition.id == id)
                .ok_or_else(|| error("unknown case ID"))?;
            let c = &case.definition;
            if (!selection.tags.is_empty()
                && !selection.tags.iter().any(|tag| c.tags.contains(tag)))
                || selection
                    .exclude_tags
                    .iter()
                    .any(|tag| c.tags.contains(tag))
            {
                continue;
            }
            cases.push(PlannedCase {
                id: c.id.clone(),
                name: c.name.clone(),
                file: case.file.clone(),
                sha256: case.sha256.clone(),
                disabled: c.disabled,
                tags: c.tags.clone(),
                annotations: c.annotations.clone(),
                step_ids: c.steps.iter().map(|s| s.id().to_owned()).collect(),
                instance_id: c.id.clone(),
                iteration: 1,
                data_row: None,
            });
        }
        if cases.is_empty() || cases.iter().all(|c| c.disabled) {
            return Err(error("selection has no enabled cases"));
        }
        let rows = selection.dataset.as_ref().map_or(1, |d| d.rows.len());
        let repeats = selection.iterations.saturating_mul(rows);
        if cases.len().saturating_mul(repeats) > 10_000
            || cases
                .iter()
                .map(|c| c.step_ids.len())
                .sum::<usize>()
                .saturating_mul(repeats)
                > 50_000
            || serde_json::to_vec(&cases)
                .unwrap()
                .len()
                .saturating_mul(repeats)
                > 16 * 1024 * 1024
        {
            return Err(error(
                "expanded plan exceeds 10000 instances, 50000 steps or 16 MiB",
            ));
        }
        if repeats > 1 || selection.dataset.is_some() {
            let base = cases;
            cases = Vec::with_capacity(base.len() * repeats);
            for iteration in 1..=selection.iterations {
                for row in 0..rows {
                    for template in &base {
                        let mut instance = template.clone();
                        instance.iteration = iteration;
                        instance.data_row = selection.dataset.as_ref().map(|_| row + 1);
                        instance.instance_id = format!(
                            "{}::r{}::d{}",
                            instance.id,
                            iteration,
                            instance.data_row.unwrap_or(0)
                        );
                        cases.push(instance);
                    }
                }
            }
        }
        let mut plan = Plan {
            schema_version: "reqon-plan/v1".into(),
            engine_version: env!("CARGO_PKG_VERSION").into(),
            package_id: self.manifest.id.clone(),
            package_sha256: self.package_sha256.clone(),
            plan_sha256: String::new(),
            suite_id: suite_id.cloned(),
            timeout_ms: self.manifest.timeout_ms,
            files: self.files.clone(),
            cases,
            data_sha256: selection.dataset.as_ref().map(|d| d.sha256.clone()),
            delay_ms: selection.delay_ms,
            bail: selection.bail,
        };
        let mut hash_input = serde_json::to_value(&plan).expect("plan serializes");
        hash_input.as_object_mut().unwrap().remove("plan_sha256");
        plan.plan_sha256 = hash_json(&hash_input);
        Ok(plan)
    }
}

pub(crate) fn valid_id(id: &str) -> bool {
    (1..=128).contains(&id.len())
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

pub(crate) fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Lexicographically sorted keys, compact UTF-8 serde JSON, arrays in declared order.
/// This is Reqon canonicalization v1, not a claim of RFC 8785/JCS compatibility.
pub(crate) fn hash_json(value: &Value) -> String {
    fn canonical(value: &Value, output: &mut Vec<u8>) {
        match value {
            Value::Object(map) => {
                output.push(b'{');
                let mut keys: Vec<_> = map.keys().collect();
                keys.sort();
                for (i, key) in keys.iter().enumerate() {
                    if i > 0 {
                        output.push(b',');
                    }
                    serde_json::to_writer(&mut *output, key).unwrap();
                    output.push(b':');
                    canonical(&map[*key], output);
                }
                output.push(b'}');
            }
            Value::Array(items) => {
                output.push(b'[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        output.push(b',');
                    }
                    canonical(item, output);
                }
                output.push(b']');
            }
            scalar => serde_json::to_writer(output, scalar).unwrap(),
        }
    }
    let mut bytes = Vec::new();
    canonical(value, &mut bytes);
    hash_bytes(&bytes)
}

pub fn schema(kind: &str) -> Value {
    if kind == "environment" {
        return crate::runtime::schema();
    }
    let (schema, format) = match kind {
        "package" => (schemars::schema_for!(Manifest), FORMAT),
        "case" => (schemars::schema_for!(Case), CASE_FORMAT),
        "suite" => (schemars::schema_for!(Suite), SUITE_FORMAT),
        _ => return crate::input::schema(),
    };
    let mut value = serde_json::to_value(schema).expect("schema serializes");
    value["$id"] = json!(format!("urn:reqon:{kind}:v1"));
    value["properties"]["schema_version"]["const"] = json!(format);
    value["description"] = json!(
        "Reqon JSON package v1. Run validate for IDs, references, paths, engine capabilities and other semantic checks. YAML/legacy Java execution is not supported by this schema."
    );
    value
}
