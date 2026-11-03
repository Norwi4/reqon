//! Public configuration and private per-case execution contexts.
use crate::{input, package::Package, secrets::OutputPolicy, spec};
use anyhow::{Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const FORMAT: &str = "reqon-environment/v1";
pub const MAX_VARIABLES: usize = 256;
pub const MAX_CONTEXT_BYTES: usize = 1024 * 1024;
pub type Variables = BTreeMap<String, Value>;

#[derive(Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub env: String,
}

#[derive(Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub schema_version: String,
    pub id: String,
    #[serde(default)]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub variables: Variables,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
    #[serde(default)]
    pub secrets: BTreeMap<String, SecretRef>,
    #[serde(default)]
    pub transport: crate::transport::TransportConfig,
    #[serde(default)]
    pub regression: crate::compatibility::Profile,
    #[serde(default)]
    pub kafka: crate::kafka::Config,
}

pub fn valid_name(name: &str) -> bool {
    (1..=128).contains(&name.len())
        && (name.as_bytes()[0].is_ascii_alphabetic() || name.starts_with('_'))
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !matches!(name, "__proto__" | "prototype" | "constructor")
}

pub fn validate_variables(values: &Variables) -> Result<()> {
    if values.len() > MAX_VARIABLES || values.keys().any(|k| !valid_name(k)) {
        bail!("variables require unique safe identifier names and at most 256 entries");
    }
    if serde_json::to_vec(values)?.len() > MAX_CONTEXT_BYTES {
        bail!("variables exceed 1 MiB");
    }
    Ok(())
}

impl Environment {
    pub fn load(path: &Path) -> Result<Self> {
        // Shape/JSON errors may contain arbitrary input values. Configuration
        // diagnostics deliberately do not echo those values or file paths.
        let env: Self = input::read(path)
            .and_then(|b| input::json_value(&b))
            .and_then(input::decode)
            .map_err(|_| anyhow::anyhow!("cannot read environment: expected bounded UTF-8 JSON matching environment schema, with unique keys"))?;
        env.validate()?;
        Ok(env)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != FORMAT || !crate::package::valid_id(&self.id) {
            bail!("unsupported environment version or invalid environment ID");
        }
        validate_variables(&self.variables)?;
        self.transport.validate()?;
        self.regression.validate()?;
        self.kafka.validate()?;
        if let Some(password) = &self.kafka.password {
            let name = password
                .strip_prefix("${")
                .and_then(|s| s.strip_suffix('}'));
            if !name.is_some_and(|name| self.secrets.contains_key(name)) {
                bail!("Kafka password must reference a declared secret variable");
            }
        }
        if self.secrets.len() > 64
            || self
                .secrets
                .iter()
                .any(|(k, v)| !valid_name(k) || !valid_name(&v.env))
        {
            bail!("secret references require safe variable/process-environment names; limit 64");
        }
        if self.secrets.keys().any(|k| self.variables.contains_key(k)) {
            bail!("secret names must not overlap public variables");
        }
        if self.metadata.len() > 64 || serde_json::to_vec(&self.metadata)?.len() > 64 * 1024 {
            bail!("metadata exceeds 64 entries or 64 KiB");
        }
        for (key, value) in &self.metadata {
            if key
                .parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()
                .is_err()
            {
                bail!("metadata requires valid ASCII gRPC keys");
            }
            if value.contains("${") {
                if !value
                    .strip_prefix("${")
                    .and_then(|s| s.strip_suffix('}'))
                    .is_some_and(valid_name)
                {
                    bail!("metadata substitution must be one whole ${{name}} value");
                }
            } else if value
                .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                .is_err()
            {
                bail!("metadata requires ASCII literal values or whole variable references");
            }
        }
        if let Some(endpoint) = &self.endpoint {
            endpoint_check(endpoint)?;
        }
        Ok(())
    }
}

fn endpoint_check(endpoint: &str) -> Result<()> {
    crate::transport::url_allowed(endpoint, true).map_err(|_| {
        anyhow::anyhow!(
            "environment endpoint requires HTTP(S) without embedded credentials/fragment"
        )
    })?;
    Ok(())
}

pub struct Configuration {
    pub environment: Environment,
    pub source_paths: Vec<PathBuf>,
    transport: crate::transport::Transport,
}

impl Configuration {
    pub fn load(path: Option<&Path>, overrides: &[String], secrets: &[String]) -> Result<Self> {
        let mut environment = if let Some(path) = path {
            Environment::load(path)?
        } else {
            Environment {
                schema_version: FORMAT.into(),
                id: "default".into(),
                ..Environment::default()
            }
        };
        let mut seen = BTreeSet::new();
        for argument in overrides {
            let (name, raw) = argument
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("--var requires NAME=JSON"))?;
            if !valid_name(name) || !seen.insert(name) {
                bail!("invalid or duplicate --var name");
            }
            let value = input::json_value(raw.as_bytes())
                .map_err(|_| anyhow::anyhow!("--var value must be valid JSON with unique keys"))?;
            environment.variables.insert(name.into(), value);
        }
        seen.clear();
        for argument in secrets {
            let (name, env) = argument
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("--secret requires NAME=PROCESS_ENV_NAME"))?;
            if !valid_name(name) || !valid_name(env) || !seen.insert(name) {
                bail!("invalid or duplicate --secret reference");
            }
            environment
                .secrets
                .insert(name.into(), SecretRef { env: env.into() });
        }
        environment.validate()?;
        let mut source_paths: Vec<_> = path
            .map(|p| p.canonicalize())
            .transpose()?
            .into_iter()
            .collect();
        let root = path.and_then(Path::parent).unwrap_or(Path::new("."));
        let transport = environment.transport.load(root)?;
        source_paths.extend(transport.sources.clone());
        Ok(Self {
            environment,
            source_paths,
            transport,
        })
    }

    pub fn validate_package(&self, package: &Package) -> Result<()> {
        // Check ALL cases offline, including disabled and unselected cases.
        for case in &package.cases {
            let mut vars =
                self.public_values(&package.manifest.variables, &case.definition.variables)?;
            vars.extend(
                self.environment
                    .secrets
                    .keys()
                    .map(|k| (k.clone(), json!("secret-placeholder"))),
            );
            resolve_metadata(&self.environment.metadata, &vars)?;
        }
        Ok(())
    }

    fn public_values(&self, package: &Variables, case: &Variables) -> Result<Variables> {
        if self
            .environment
            .secrets
            .keys()
            .any(|k| package.contains_key(k) || case.contains_key(k))
        {
            bail!("secret names must not overlap package/case defaults");
        }
        let mut values = package.clone();
        values.extend(case.clone());
        values.extend(self.environment.variables.clone());
        validate_variables(&values)?;
        if values.len() + self.environment.secrets.len() > MAX_VARIABLES {
            bail!("combined context exceeds 256 variables");
        }
        Ok(values)
    }

    pub fn resolve(&self) -> Result<ExecutionContext> {
        let mut values = self.environment.variables.clone();
        let mut secret_values = Vec::new();
        for (name, reference) in &self.environment.secrets {
            let value = std::env::var(&reference.env).map_err(|_| {
                anyhow::anyhow!("required secret environment variable is missing or not Unicode")
            })?;
            if value.is_empty() || value.len() > 64 * 1024 {
                bail!("secret must be nonempty and no larger than 64 KiB");
            }
            secret_values.push(value.clone());
            values.insert(name.clone(), Value::String(value));
        }
        validate_variables(&values)?;
        let mut transport = self.transport.clone();
        if let Some(auth) = &transport.config.proxy_authorization {
            let value = spec::resolve_text(auth, &values)?;
            value
                .parse::<http::HeaderValue>()
                .map_err(|_| anyhow::anyhow!("invalid proxy authorization header"))?;
            secret_values.push(value.clone());
            transport.config.proxy_authorization = Some(value);
        }
        if let Some(identity) = &transport.identity {
            secret_values.push(String::from_utf8_lossy(&identity.1).into_owned());
        }
        Ok(ExecutionContext {
            values,
            environment_id: (!self.source_paths.is_empty()).then(|| self.environment.id.clone()),
            secret_names: self.environment.secrets.keys().cloned().collect(),
            metadata: self.environment.metadata.clone(),
            policy: OutputPolicy::new(secret_values),
            transport: std::sync::Arc::new(transport),
            session: crate::transport::Session::default(),
            regression: self.environment.regression.clone(),
            kafka: self.environment.kafka.clone(),
        })
    }
}

#[derive(Clone, Default)]
pub struct ExecutionContext {
    values: Variables,
    secret_names: BTreeSet<String>,
    metadata: BTreeMap<String, String>,
    pub environment_id: Option<String>,
    pub policy: OutputPolicy,
    pub transport: std::sync::Arc<crate::transport::Transport>,
    pub session: crate::transport::Session,
    pub regression: crate::compatibility::Profile,
    pub kafka: crate::kafka::Config,
}

impl ExecutionContext {
    pub fn with_data(mut self, row: Option<&Variables>, iteration: usize) -> Result<Self> {
        if let Some(row) = row {
            if self.values.contains_key("data") {
                bail!("data is reserved when a dataset is selected");
            }
            self.values
                .insert("data".into(), serde_json::to_value(row)?);
        }
        if self.values.contains_key("iteration") {
            bail!("iteration is reserved by the runner");
        }
        self.values.insert("iteration".into(), json!(iteration));
        validate_variables(&self.values)?;
        Ok(self)
    }
    pub fn initial_values(&self) -> Variables {
        self.values.clone()
    }

    pub fn for_case(&self, package: &Variables, case: &Variables) -> Result<Self> {
        if self
            .secret_names
            .iter()
            .any(|k| package.contains_key(k) || case.contains_key(k))
        {
            bail!("secret names must not overlap package/case defaults");
        }
        let mut values = package.clone();
        values.extend(case.clone());
        values.extend(self.values.clone());
        validate_variables(&values)?;
        Ok(Self {
            values,
            ..self.clone()
        })
    }

    pub fn resolved_metadata(&self) -> Result<BTreeMap<String, String>> {
        resolve_metadata(&self.metadata, &self.values)
    }

    pub fn update(&self, vars: &mut Variables, values: Variables) -> Result<()> {
        if values.keys().any(|k| self.secret_names.contains(k)) {
            bail!("runtime values must not overwrite secret bindings");
        }
        let mut next = vars.clone();
        next.extend(values);
        validate_variables(&next)?;
        *vars = next;
        Ok(())
    }
}

fn resolve_metadata(
    metadata: &BTreeMap<String, String>,
    vars: &Variables,
) -> Result<BTreeMap<String, String>> {
    metadata
        .iter()
        .map(|(key, template)| {
            let value = spec::resolve(&Value::String(template.clone()), vars)
                .map_err(|_| anyhow::anyhow!("metadata references an undefined variable"))?;
            let value = value
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("metadata variable must be a string"))?;
            if key
                .parse::<tonic::metadata::MetadataKey<tonic::metadata::Ascii>>()
                .is_err()
                || value
                    .parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>()
                    .is_err()
            {
                bail!("metadata requires valid ASCII gRPC keys/values");
            }
            Ok((key.clone(), value.to_owned()))
        })
        .collect()
}

pub fn schema() -> Value {
    let mut schema =
        serde_json::to_value(schemars::schema_for!(Environment)).expect("schema serializes");
    schema["$id"] = json!("urn:reqon:environment:v1");
    schema["properties"]["schema_version"]["const"] = json!(FORMAT);
    schema["description"] = json!(
        "Public environment values and process-environment secret references. Use validate for semantic checks; never store secret values here."
    );
    schema
}
