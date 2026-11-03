use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use reqon_runner::{
    RunOptions, artifacts,
    input::{self, Diagnostic},
    javascript, output, package, report, runtime,
    secrets::OutputPolicy,
    spec,
};
use serde_json::{Value, json};
use std::{
    io::BufRead,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(version, about = "Reqon native regression runner prototype")]
struct Args {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Args, Default)]
struct SelectionArgs {
    /// Select a suite by its stable ID (packages only).
    #[arg(long, conflicts_with = "cases")]
    suite: Option<String>,
    /// Select case IDs in this order; repeat for multiple cases (packages only).
    #[arg(long = "case")]
    cases: Vec<String>,
    /// Include cases matching any supplied tag, keeping declared order.
    #[arg(long = "tag")]
    tags: Vec<String>,
    #[arg(long = "exclude-tag")]
    exclude_tags: Vec<String>,
    #[arg(long)]
    iterations: Option<usize>,
    /// JSON array of objects or CSV with unique column names.
    #[arg(long)]
    data: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,
    /// Stop subsequent independent cases after the first failure/error.
    #[arg(long)]
    bail: bool,
}

#[derive(clap::Args, Default)]
struct RuntimeArgs {
    /// Public environment JSON with optional process-environment secret references.
    #[arg(long = "env")]
    environment: Option<PathBuf>,
    /// Public override NAME=JSON; repeat for distinct names. Never pass secrets here.
    #[arg(long = "var")]
    overrides: Vec<String>,
    /// Bind NAME=PROCESS_ENV_NAME; never put the secret value on the command line.
    #[arg(long = "secret")]
    secrets: Vec<String>,
}

impl RuntimeArgs {
    fn explicit(&self) -> bool {
        self.environment.is_some() || !self.overrides.is_empty() || !self.secrets.is_empty()
    }
    fn configuration(&self) -> Result<runtime::Configuration> {
        runtime::Configuration::load(self.environment.as_deref(), &self.overrides, &self.secrets)
    }
}
impl SelectionArgs {
    fn explicit(&self) -> bool {
        self.suite.is_some()
            || !self.cases.is_empty()
            || !self.tags.is_empty()
            || !self.exclude_tags.is_empty()
            || self.iterations.is_some()
            || self.data.is_some()
            || self.delay_ms > 0
            || self.bail
    }
    fn selection(&self) -> Result<package::Selection> {
        Ok(package::Selection {
            suite: self.suite.clone(),
            cases: self.cases.clone(),
            tags: self.tags.clone(),
            exclude_tags: self.exclude_tags.clone(),
            iterations: self.iterations.unwrap_or(1),
            dataset: self
                .data
                .as_deref()
                .map(reqon_runner::dataset::Dataset::load)
                .transpose()?,
            delay_ms: self.delay_ms,
            bail: self.bail,
        })
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaKind {
    Scenario,
    Package,
    Case,
    Suite,
    Environment,
}

#[derive(Subcommand)]
enum Command {
    /// Check a scenario or a complete package offline, without executing any code.
    Validate {
        input: PathBuf,
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        selection: SelectionArgs,
        #[command(flatten)]
        context: RuntimeArgs,
    },
    /// Print a deterministic execution plan for a package as JSON; never executes it.
    Plan {
        input: PathBuf,
        #[command(flatten)]
        selection: SelectionArgs,
    },
    /// Print a JSON Schema (scenario is the legacy single-file format).
    Schema {
        #[arg(long, value_enum, default_value = "scenario")]
        kind: SchemaKind,
    },
    /// Print supported features and limitations as JSON.
    Capabilities,
    /// GUI transport: versioned JSON lines on stdin/stdout, full response chunks.
    Request,
    /// Recover durable partial records after a killed/crashed run; exits 2.
    Recover {
        journal: PathBuf,
        #[arg(long)]
        json: PathBuf,
        #[arg(long)]
        junit: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
    },
    Run {
        input: PathBuf,
        #[arg(long)]
        json: PathBuf,
        #[arg(long)]
        junit: PathBuf,
        #[arg(long)]
        html: Option<PathBuf>,
        #[arg(long)]
        journal: Option<PathBuf>,
        /// Required for packages; never stored in the package's case definitions.
        #[arg(long)]
        endpoint: Option<String>,
        #[command(flatten)]
        selection: SelectionArgs,
        /// Read {"type":"cancel"} commands from stdin; stdout is NDJSON events.
        #[arg(long)]
        control_stdin: bool,
        #[command(flatten)]
        context: RuntimeArgs,
    },
    #[command(hide = true)]
    JsWorker,
}

enum Document {
    Scenario(Box<spec::Scenario>, PathBuf),
    Package(Box<package::Package>),
    Environment(Box<runtime::Environment>),
}

fn load(path: &Path) -> Result<Document, Vec<Diagnostic>> {
    let path = if path.is_dir() {
        path.join("catalog.json")
    } else {
        path.to_path_buf()
    };
    let bytes = input::read(&path)?;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("yaml") || s.eq_ignore_ascii_case("yml"))
    {
        return reqon_runner::compatibility::load(&path, &bytes)
            .map(|p| Document::Package(Box::new(p)))
            .map_err(context_error);
    }
    let value = input::json_value(&bytes)?;
    if value.get("schema_version").is_none()
        && value.get("steps").is_some()
        && value.get("id").is_some()
    {
        return reqon_runner::compatibility::load(&path, &bytes)
            .map(|p| Document::Package(Box::new(p)))
            .map_err(context_error);
    }
    let version = value
        .get("schema_version")
        .and_then(Value::as_str)
        .unwrap_or("");
    if version.starts_with("reqon-environment/") {
        runtime::Environment::load(&path)
            .map(|env| Document::Environment(Box::new(env)))
            .map_err(context_error)
    } else if version.starts_with("reqon-package/") {
        package::load(&path, &bytes).map(|p| Document::Package(Box::new(p)))
    } else if version.starts_with("reqon-case/") || version.starts_with("reqon-suite/") {
        Err(vec![Diagnostic::new(
            "input.format",
            "$.schema_version",
            "load catalog.json to validate/run case or suite files",
        )])
    } else {
        let scenario = input::parse(&bytes)?;
        if scenario.steps.iter().any(|s| !s.file_paths().is_empty()) {
            return Err(vec![Diagnostic::new(
                "input.assets",
                "$.steps",
                "HTTP file bodies require a package snapshot; use catalog.json",
            )]);
        }
        Ok(Document::Scenario(Box::new(scenario), path))
    }
}

fn diagnostic_error(errors: Vec<Diagnostic>) -> anyhow::Error {
    anyhow::anyhow!(
        "{}",
        errors
            .iter()
            .map(|e| format!(
                "{}:{} [{}]: {}",
                e.file.as_deref().unwrap_or("input"),
                e.path,
                e.code,
                e.message
            ))
            .collect::<Vec<_>>()
            .join("; ")
    )
}

fn context_error(error: anyhow::Error) -> Vec<Diagnostic> {
    vec![Diagnostic::new(
        "environment.configuration",
        "$",
        error.to_string(),
    )]
}

fn validate(
    path: &Path,
    selection: &SelectionArgs,
    context: &RuntimeArgs,
) -> Result<Value, Vec<Diagnostic>> {
    match load(path)? {
        Document::Environment(environment) => {
            if selection.explicit() || context.explicit() {
                return Err(vec![Diagnostic::new(
                    "input.selection",
                    "$",
                    "environment validation accepts no package/runtime flags",
                )]);
            }
            Ok(
                json!({"schema_version":"reqon-prototype/validation-v1","type":"validation_result","valid":true,"input_kind":"environment","environment_id":environment.id,"variable_count":environment.variables.len(),"secret_reference_count":environment.secrets.len(),"diagnostics":[]}),
            )
        }
        Document::Scenario(spec, _) => {
            if selection.explicit() || context.explicit() {
                return Err(vec![Diagnostic::new(
                    "input.selection",
                    "$",
                    "selection and runtime context flags require a package",
                )]);
            }
            Ok(
                json!({"schema_version":"reqon-prototype/validation-v1","type":"validation_result","valid":true,"scenario_id":spec.id,"step_count":spec.steps.len(),"diagnostics":[]}),
            )
        }
        Document::Package(package) => {
            context
                .configuration()
                .and_then(|c| c.validate_package(&package))
                .map_err(context_error)?;
            let selected = if selection.explicit() {
                Some(
                    package
                        .plan(&selection.selection().map_err(context_error)?)?
                        .cases
                        .len(),
                )
            } else {
                None
            };
            Ok(
                json!({"schema_version":"reqon-prototype/validation-v1","type":"validation_result","valid":true,"input_kind":"package","package_id":package.manifest.id,"package_sha256":package.package_sha256,"case_count":package.cases.len(),"suite_count":package.suites.len(),"selected_case_count":selected,"diagnostics":[]}),
            )
        }
    }
}

fn cancellation(control_stdin: bool) -> CancellationToken {
    let token = CancellationToken::new();
    if control_stdin {
        let reader_token = token.clone();
        std::thread::spawn(move || {
            let mut stdin = std::io::stdin().lock();
            loop {
                let mut line = Vec::new();
                if std::io::Read::take(&mut stdin, 1025)
                    .read_until(b'\n', &mut line)
                    .is_err()
                    || line.is_empty()
                    || line.len() > 1024
                {
                    break;
                }
                if serde_json::from_slice::<Value>(&line)
                    .ok()
                    .is_some_and(|v| v["type"] == "cancel")
                {
                    reader_token.cancel();
                    break;
                }
            }
        });
    }
    token
}

fn main() {
    let mut policy = OutputPolicy::default();
    let code = match execute(&mut policy) {
        Ok(code) => code,
        Err(error) => {
            eprintln!(
                "{}",
                json!({"type":"fatal_error","message":if policy.sensitive() { "run failed; details withheld in secret mode".to_owned() } else { format!("{error:#}") }})
            );
            2
        }
    };
    std::process::exit(code);
}

fn execute(policy: &mut OutputPolicy) -> Result<i32> {
    match Args::parse().command {
        Command::Schema { kind } => {
            let name = match kind {
                SchemaKind::Scenario => "scenario",
                SchemaKind::Package => "package",
                SchemaKind::Case => "case",
                SchemaKind::Suite => "suite",
                SchemaKind::Environment => "environment",
            };
            println!("{}", serde_json::to_string_pretty(&package::schema(name))?);
            Ok(0)
        }
        Command::Capabilities => {
            println!("{}", serde_json::to_string_pretty(&input::capabilities())?);
            Ok(0)
        }
        Command::Recover {
            journal,
            json,
            junit,
            html,
        } => {
            let mut paths = vec![json.clone(), junit, json.with_extension("manifest.json")];
            if let Some(html) = html {
                paths.push(html);
            }
            let paths = output::prepare_many(&paths, &[journal.canonicalize()?])?;
            let dest = artifacts::Destinations {
                json: paths[0].clone(),
                junit: paths[1].clone(),
                manifest: paths[2].clone(),
                html: paths.get(3).cloned(),
            };
            artifacts::recover(&journal, &dest)?;
            Ok(2)
        }
        Command::Validate {
            input,
            json,
            selection,
            context,
        } => {
            let result = validate(&input, &selection, &context);
            let valid = result.is_ok();
            let value = result.unwrap_or_else(|errors| json!({"schema_version":"reqon-prototype/validation-v1","type":"validation_result","valid":false,"diagnostics":errors}));
            if json {
                println!("{value}");
            } else if valid {
                println!(
                    "Valid: {}. Offline validation only.",
                    value
                        .get("package_id")
                        .or_else(|| value.get("scenario_id"))
                        .unwrap_or(&Value::Null)
                );
            } else {
                eprintln!("{}", serde_json::to_string_pretty(&value["diagnostics"])?);
            }
            Ok(if valid { 0 } else { 2 })
        }
        Command::Plan { input, selection } => {
            let Document::Package(package) = load(&input).map_err(diagnostic_error)? else {
                bail!("plan requires a package manifest or directory");
            };
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &package
                        .plan(&selection.selection()?)
                        .map_err(diagnostic_error)?
                )?
            );
            Ok(0)
        }
        Command::JsWorker => {
            javascript::worker_main()?;
            Ok(0)
        }
        Command::Request => {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?
                .block_on(reqon_runner::request::serve())?;
            Ok(0)
        }
        Command::Run {
            input,
            json,
            junit,
            html,
            journal,
            endpoint,
            selection,
            control_stdin,
            context,
        } => {
            let configuration = context.configuration()?;
            let execution_context = configuration.resolve()?;
            *policy = execution_context.policy.clone();
            let document = load(&input).map_err(diagnostic_error)?;
            let selected = selection.selection()?;
            let mut inputs = match &document {
                Document::Environment(_) => {
                    bail!("run requires a scenario or package, not an environment file")
                }
                Document::Scenario(_, path) => {
                    if selection.explicit() || endpoint.is_some() || context.explicit() {
                        bail!(
                            "selection, endpoint and runtime context flags require a package; legacy scenarios contain their own endpoint"
                        );
                    }
                    vec![path.canonicalize()?]
                }
                Document::Package(package) => {
                    if endpoint.is_none() && configuration.environment.endpoint.is_none() {
                        bail!(
                            "--endpoint or an environment endpoint is required for package execution"
                        );
                    }
                    configuration.validate_package(package)?;
                    package.plan(&selected).map_err(diagnostic_error)?;
                    package.input_paths().to_vec()
                }
            };
            inputs.extend(configuration.source_paths);
            if let Some(data) = &selected.dataset {
                inputs.push(data.path.clone());
            }
            let endpoint = endpoint.or(configuration.environment.endpoint);
            let run_id = uuid::Uuid::new_v4().to_string();
            let journal_path =
                journal.unwrap_or_else(|| json.with_extension(format!("{run_id}.journal.ndjson")));
            let mut paths = vec![
                json.clone(),
                junit,
                json.with_extension("manifest.json"),
                journal_path,
            ];
            if let Some(html) = html {
                paths.push(html);
            }
            let paths = output::prepare_many(&paths, &inputs)?;
            let dest = artifacts::Destinations {
                json: paths[0].clone(),
                junit: paths[1].clone(),
                manifest: paths[2].clone(),
                html: paths.get(4).cloned(),
            };
            let journal = artifacts::Journal::create(&paths[3], run_id.clone())?;
            artifacts::atomic_json(
                &dest.manifest,
                &json!({"schema_version":"reqon-artifacts/v1","run_id":run_id,"state":"running","journal":paths[3]}),
            )?;
            let cancellation = cancellation(control_stdin);
            let events = reqon_runner::events::Events::new(run_id);
            let emit = |event| events.emit(event);
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()?;
            let (mut metadata, xml, exit_code) = runtime.block_on(async {
                let token = cancellation.clone();
                tokio::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        token.cancel();
                    }
                });
                #[cfg(unix)]
                {
                    let token = cancellation.clone();
                    tokio::spawn(async move {
                        if let Ok(mut signal) = tokio::signal::unix::signal(
                            tokio::signal::unix::SignalKind::terminate(),
                        ) {
                            signal.recv().await;
                            token.cancel();
                        }
                    });
                }
                let options = RunOptions {
                    cancellation,
                    javascript_executable: std::env::current_exe()?,
                    deadline: None,
                    context: execution_context,
                    assets: Default::default(),
                    journal: Some(journal.clone()),
                    instance_id: String::new(),
                };
                match document {
                    Document::Environment(_) => unreachable!("rejected before execution"),
                    Document::Scenario(spec, _) => {
                        let options = RunOptions {
                            instance_id: spec.id.clone(),
                            ..options
                        };
                        let result = reqon_runner::run(&spec, options, emit).await?;
                        Ok::<_, anyhow::Error>((
                            serde_json::to_value(&result)?,
                            Some(report::junit(&result)),
                            result.exit_code(),
                        ))
                    }
                    Document::Package(package) => {
                        let result = package::run(
                            &package,
                            &selected,
                            endpoint.as_deref().unwrap(),
                            options,
                            emit,
                        )
                        .await?;
                        Ok((serde_json::to_value(&result)?, None, result.exit_code()))
                    }
                }
            })?;
            metadata["events_dropped"] = json!(events.dropped());
            artifacts::publish(&journal, metadata, xml, &dest, exit_code)?;
            events.finish();
            Ok(exit_code)
        }
    }
}
