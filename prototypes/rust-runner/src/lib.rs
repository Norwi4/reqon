pub mod artifacts;
pub mod assertions;
pub mod capture;
pub mod compatibility;
pub mod dataset;
pub mod events;
pub mod grpc;
pub mod http_client;
pub mod input;
pub mod javascript;
pub mod kafka;
pub mod output;
pub mod package;
mod proto_source;
pub mod report;
pub mod request;
pub mod runtime;
pub mod secrets;
pub mod spec;
pub mod transport;

use grpc::DynamicClient;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use spec::{Scenario, Step, resolve};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Passed,
    Failed,
    Error,
    Cancelled,
    Skipped,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckResult {
    pub path: String,
    pub expected: Value,
    pub actual: Option<Value>,
    pub passed: bool,
    #[serde(default)]
    pub operation: String,
    #[serde(default)]
    pub target: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct TransportReport {
    pub protocol: String,
    pub status: Option<u16>,
    pub headers: http_client::Headers,
    pub trailers: http_client::Headers,
    pub url: Option<String>,
    pub response_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckSummary {
    pub passed: usize,
    pub failed: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StepReport {
    pub id: String,
    pub outcome: Outcome,
    pub duration_ms: f64,
    pub attempts: usize,
    pub checks: Vec<CheckResult>,
    pub error: Option<String>,
    pub grpc_code: Option<String>,
    pub response: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<TransportReport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_checks: Vec<javascript::ScriptCheck>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub logs: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub check_summary: Option<CheckSummary>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub capture_omitted: bool,
}

impl StepReport {
    fn new(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            outcome: Outcome::Passed,
            duration_ms: 0.0,
            attempts: 0,
            checks: Vec::new(),
            error: None,
            grpc_code: None,
            response: None,
            request: None,
            transport: None,
            script_checks: Vec::new(),
            logs: Vec::new(),
            check_summary: None,
            capture_omitted: false,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RunReport {
    pub schema_version: String,
    pub engine_version: String,
    pub scenario_id: String,
    pub outcome: Outcome,
    pub duration_ms: f64,
    pub reflection_requests: usize,
    pub variables: BTreeMap<String, Value>,
    pub steps: Vec<StepReport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redaction: Option<secrets::RedactionInfo>,
}

impl RunReport {
    pub fn exit_code(&self) -> i32 {
        match self.outcome {
            Outcome::Passed => 0,
            Outcome::Failed => 1,
            Outcome::Cancelled => 130,
            _ => 2,
        }
    }
}

#[derive(Clone)]
pub struct RunOptions {
    pub cancellation: CancellationToken,
    pub javascript_executable: PathBuf,
    /// A package deadline may be earlier than this scenario's own budget.
    pub deadline: Option<Instant>,
    pub context: runtime::ExecutionContext,
    pub assets: http_client::Assets,
    pub journal: Option<crate::artifacts::SharedJournal>,
    pub instance_id: String,
}

struct Problem {
    outcome: Outcome,
    message: String,
}
impl Problem {
    fn error(message: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Error,
            message: message.into(),
        }
    }
    fn failed(message: impl Into<String>) -> Self {
        Self {
            outcome: Outcome::Failed,
            message: message.into(),
        }
    }
}
impl From<anyhow::Error> for Problem {
    fn from(error: anyhow::Error) -> Self {
        Self::error(format!("{error:#}"))
    }
}

#[derive(Default)]
struct ExecutionState {
    client: Option<DynamicClient>,
    vars: BTreeMap<String, Value>,
    last_response: Value,
    http: Option<http_client::HttpClient>,
    checks: assertions::Cache,
    worker: Option<javascript::Worker>,
    regression_clients: BTreeMap<String, DynamicClient>,
    kafka: Option<kafka::Subscription>,
}

#[allow(clippy::too_many_arguments)]
async fn script(
    code: &str,
    timeout_ms: u64,
    hook: bool,
    spec: &Scenario,
    step_id: &str,
    result: &mut StepReport,
    worker: &mut Option<javascript::Worker>,
    http: &mut Option<http_client::HttpClient>,
    vars: &mut runtime::Variables,
    response: &Value,
    info: &Value,
    options: &RunOptions,
    emit: &mut impl FnMut(Value),
) -> Result<(), Problem> {
    if worker.is_none() {
        *worker = Some(javascript::Worker::start(
            &options.javascript_executable,
            |pid| emit(json!({"type":"script_worker_started","step_id":step_id,"pid":pid})),
        )?);
    }
    if http.is_none() {
        *http = Some(http_client::HttpClient::new(
            &options.context.transport,
            &options.context.session,
        )?);
    }
    let input = javascript::ScriptInput {
        script: code.into(),
        vars: vars.clone(),
        response: response.clone(),
        timeout_ms,
        hook,
        response_info: info.clone(),
    };
    let network_vars = &input.vars;
    let client = http.as_ref().unwrap();
    let policy = &options.context.transport.config;
    let calls = std::cell::Cell::new(0usize);
    let started = Instant::now();
    let output=worker.as_mut().unwrap().run(&input,|request| {
        let calls=&calls;
        async move {
            request.validate()?;
            calls.set(calls.get()+1);
            if calls.get()>policy.script_http_max_calls {anyhow::bail!("script HTTP calls are disabled or their limit was exceeded");}
            let url=spec::resolve_text(&request.url,network_vars)?;
            let target=url::Url::parse(&spec.endpoint)?.join(&url)?;
            if !policy.script_http_origins.contains(&target.origin().ascii_serialization()) {anyhow::bail!("script HTTP origin is not in the environment allowlist");}
            if request.redirects.allow_cross_origin {anyhow::bail!("script HTTP cannot enable cross-origin redirects");}
            let remaining=Duration::from_millis(timeout_ms).saturating_sub(started.elapsed());
            if remaining.is_zero() {anyhow::bail!("script HTTP deadline exceeded");}
            let response=client.send(&spec.endpoint,&request.method,&request.url,&request.query,&request.headers,&request.body,request.auth.as_ref(),&request.redirects,network_vars,&options.assets,remaining.as_millis().max(1) as u64,256*1024,256*1024,request.cookies).await?;
            Ok(json!({"body":response.body,"status":response.status,"headers":response.headers,"elapsed_ms":response.duration_ms}))
        }
    }).await?;
    result.script_checks.extend(output.checks);
    result.logs.extend(output.logs);
    if let Some(message) = output.error {
        return Err(if output.kind.as_deref() == Some("assertion") {
            Problem::failed(message)
        } else {
            Problem::error(message)
        });
    }
    options.context.update(
        vars,
        output
            .values
            .ok_or_else(|| Problem::error("script produced no variables"))?,
    )?;
    Ok(())
}

async fn execute_step(
    spec: &Scenario,
    step: &Step,
    result: &mut StepReport,
    state: &mut ExecutionState,
    options: &RunOptions,
    emit: &mut impl FnMut(Value),
) -> Result<(), Problem> {
    if state.kafka.is_none() {
        let topics = spec
            .steps
            .iter()
            .filter_map(|s| {
                if let Step::Kafka { wait, .. } = s {
                    Some(wait.topic.clone())
                } else {
                    None
                }
            })
            .collect::<std::collections::BTreeSet<_>>();
        if !topics.is_empty() {
            state.kafka = Some(kafka::Subscription::start(topics, options).await?);
            emit(json!({"type":"kafka_ready","scenario_id":spec.id}));
        }
    }
    if let Step::RegressionCall { call, .. } = step {
        return compatibility::call(spec, call, result, state, options, emit).await;
    }
    if let Step::Kafka { wait, .. } = step {
        return state
            .kafka
            .as_ref()
            .unwrap()
            .wait(wait, result, &mut state.vars, options)
            .await;
    }
    let ExecutionState {
        client,
        vars,
        last_response,
        http,
        checks,
        worker,
        ..
    } = state;
    let hooks = match step {
        Step::Http { hooks, .. } | Step::Grpc { hooks, .. } => Some(hooks),
        _ => None,
    };
    if let Some(hooks) = hooks {
        for h in &hooks.pre {
            script(
                &h.script,
                h.timeout_ms,
                true,
                spec,
                step.id(),
                result,
                worker,
                http,
                vars,
                last_response,
                &Value::Null,
                options,
                emit,
            )
            .await?;
        }
    }
    match step {
        Step::RegressionCall { .. } | Step::Kafka { .. } => {
            unreachable!("handled by compatibility profile")
        }
        Step::Pause { duration_ms, .. } => sleep(Duration::from_millis(*duration_ms)).await,
        Step::Http {
            request,
            expect,
            save,
            poll,
            timeout_ms,
            ..
        } => {
            if http.is_none() {
                *http = Some(http_client::HttpClient::new(
                    &options.context.transport,
                    &options.context.session,
                )?);
            }
            let poll_deadline = poll
                .as_ref()
                .map(|p| Instant::now() + Duration::from_millis(p.timeout_ms));
            loop {
                let budget = poll_deadline
                    .map(|d| {
                        d.saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(*timeout_ms))
                    })
                    .unwrap_or(Duration::from_millis(*timeout_ms));
                if budget.is_zero() {
                    return Err(Problem::failed("poll condition timed out"));
                }
                result.attempts += 1;
                emit(
                    json!({"type":"attempt_started","step_id":step.id(),"attempt":result.attempts}),
                );
                let response = timeout(
                    budget,
                    http.as_ref().unwrap().send(
                        &spec.endpoint,
                        &request.method,
                        &request.url,
                        &request.query,
                        &request.headers,
                        &request.body,
                        request.auth.as_ref(),
                        &request.redirects,
                        vars,
                        &options.assets,
                        budget.as_millis().max(1) as u64,
                        spec.max_response_bytes,
                        request.max_request_bytes,
                        request.cookies,
                    ),
                )
                .await
                .map_err(|_| Problem::error("HTTP deadline exceeded"))??;
                result.transport = Some(TransportReport {
                    protocol: "http".into(),
                    status: Some(response.status),
                    headers: response.headers.clone(),
                    url: Some(response.url.clone()),
                    response_bytes: response.bytes,
                    ..TransportReport::default()
                });
                let info = json!({"body":response.body,"status":response.status,"headers":response.headers,"elapsed_ms":response.duration_ms});
                for h in &hooks.unwrap().post {
                    script(
                        &h.script,
                        h.timeout_ms,
                        true,
                        spec,
                        step.id(),
                        result,
                        worker,
                        http,
                        vars,
                        &response.body,
                        &info,
                        options,
                        emit,
                    )
                    .await?;
                }
                let view = assertions::ResponseView {
                    body: &response.body,
                    status: Some(response.status),
                    headers: &response.headers,
                    trailers: &BTreeMap::new(),
                    elapsed_ms: response.duration_ms,
                    grpc_code: None,
                };
                result.checks = checks.evaluate(&view, expect, vars)?;
                result.response = Some(capture::value(&response.body, 64 * 1024));
                *last_response = response.body.clone();
                emit(
                    json!({"type":"attempt_finished","step_id":step.id(),"attempt":result.attempts,"status":response.status}),
                );
                let ready = if let Some(poll) = poll {
                    let conditions = checks.evaluate(&view, &poll.until, vars)?;
                    let ready = conditions.iter().all(|c| c.passed);
                    if result.checks.iter().all(|c| c.passed) {
                        result.checks.extend(conditions);
                        ready
                    } else {
                        return Err(Problem::failed("response assertion failed"));
                    }
                } else {
                    true
                };
                result.transport = Some(TransportReport {
                    protocol: "http".into(),
                    status: Some(response.status),
                    headers: response.headers,
                    url: Some(response.url),
                    response_bytes: response.bytes,
                    ..TransportReport::default()
                });
                if !ready {
                    let remaining = poll_deadline
                        .unwrap()
                        .saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(Problem::failed("poll condition timed out"));
                    }
                    sleep(remaining.min(Duration::from_millis(poll.as_ref().unwrap().interval_ms)))
                        .await;
                    continue;
                }
                if result.checks.iter().any(|c| !c.passed) {
                    return Err(Problem::failed("response assertion failed"));
                }
                let values = save
                    .iter()
                    .map(|(key, path)| {
                        response
                            .body
                            .pointer(path)
                            .cloned()
                            .map(|v| (key.clone(), v))
                            .ok_or_else(|| {
                                Problem::error(format!("save path does not exist: {path}"))
                            })
                    })
                    .collect::<Result<BTreeMap<_, _>, _>>()?;
                options.context.update(vars, values)?;
                break;
            }
        }
        Step::Grpc {
            method,
            request,
            expect,
            save,
            poll,
            timeout_ms,
            expect_code,
            ..
        } => {
            if client.is_none() {
                *client = Some(
                    timeout(
                        Duration::from_millis(*timeout_ms),
                        DynamicClient::connect(
                            &spec.endpoint,
                            &spec.metadata,
                            spec.max_response_bytes,
                            &options.context.transport,
                            &options.context.session,
                        ),
                    )
                    .await
                    .map_err(|_| Problem::error("connection timeout"))??,
                );
            }
            let client = client.as_mut().unwrap();
            let descriptor = timeout(Duration::from_millis(*timeout_ms), client.method(method))
                .await
                .map_err(|_| Problem::error("reflection timeout"))??;
            let payload = resolve(request, vars)?;
            let poll_deadline = poll
                .as_ref()
                .map(|p| Instant::now() + Duration::from_millis(p.timeout_ms));
            loop {
                let budget = poll_deadline
                    .map(|d| {
                        d.saturating_duration_since(Instant::now())
                            .min(Duration::from_millis(*timeout_ms))
                    })
                    .unwrap_or(Duration::from_millis(*timeout_ms));
                if budget.is_zero() {
                    return Err(Problem::failed("poll condition timed out"));
                }
                result.attempts += 1;
                emit(
                    json!({"type":"attempt_started","step_id":step.id(),"attempt":result.attempts}),
                );
                let call_started = Instant::now();
                let call = timeout(
                    budget,
                    client.unary(&descriptor, &payload, budget.as_millis().max(1) as u64),
                )
                .await;
                let response = match call {
                    Ok(Ok(value)) => {
                        result.grpc_code = Some("OK".into());
                        value
                    }
                    Ok(Err(error)) => {
                        result.grpc_code = error
                            .downcast_ref::<tonic::Status>()
                            .map(|s| format!("{:?}", s.code()));
                        let trailers = error
                            .downcast_ref::<tonic::Status>()
                            .map(|s| grpc::metadata(s.metadata()))
                            .unwrap_or_default();
                        if expect_code
                            .as_ref()
                            .is_some_and(|expected| result.grpc_code.as_ref() == Some(expected))
                        {
                            grpc::Reply {
                                body: Value::Null,
                                headers: BTreeMap::new(),
                                trailers,
                                bytes: 0,
                            }
                        } else {
                            result.transport = Some(TransportReport {
                                protocol: "grpc".into(),
                                trailers,
                                ..TransportReport::default()
                            });
                            return Err(error.into());
                        }
                    }
                    Err(_) => {
                        result.grpc_code = Some("DeadlineExceeded".into());
                        return Err(Problem::error("RPC deadline exceeded"));
                    }
                };
                result.transport = Some(TransportReport {
                    protocol: "grpc".into(),
                    headers: response.headers.clone(),
                    trailers: response.trailers.clone(),
                    response_bytes: response.bytes,
                    ..TransportReport::default()
                });
                let info = json!({"body":response.body,"grpc_code":result.grpc_code,"headers":response.headers,"trailers":response.trailers});
                for h in &hooks.unwrap().post {
                    script(
                        &h.script,
                        h.timeout_ms,
                        true,
                        spec,
                        step.id(),
                        result,
                        worker,
                        http,
                        vars,
                        &response.body,
                        &info,
                        options,
                        emit,
                    )
                    .await?;
                }
                let view = assertions::ResponseView {
                    body: &response.body,
                    status: None,
                    headers: &response.headers,
                    trailers: &response.trailers,
                    elapsed_ms: call_started.elapsed().as_secs_f64() * 1000.0,
                    grpc_code: result.grpc_code.as_deref(),
                };
                result.checks = checks.evaluate(&view, expect, vars)?;
                if let Some(code) = expect_code {
                    result.checks.push(CheckResult {
                        path: String::new(),
                        expected: json!(code),
                        actual: result.grpc_code.as_ref().map(|c| json!(c)),
                        passed: result.grpc_code.as_ref() == Some(code),
                        target: "grpc_code".into(),
                        operation: "equals".into(),
                    });
                }
                result.response = Some(capture::value(&response.body, 64 * 1024));
                *last_response = response.body.clone();
                emit(
                    json!({"type":"attempt_finished","step_id":step.id(),"attempt":result.attempts,"grpc_code":result.grpc_code}),
                );
                if result.checks.iter().any(|c| !c.passed) {
                    return Err(Problem::failed(
                        "response assertion failed (transport succeeded)",
                    ));
                }
                if let Some(poll) = poll {
                    let conditions = checks.evaluate(&view, &poll.until, vars)?;
                    let ready = conditions.iter().all(|c| c.passed);
                    result.checks.extend(conditions);
                    if !ready {
                        let remaining = poll_deadline
                            .unwrap()
                            .saturating_duration_since(Instant::now());
                        if remaining.is_zero() {
                            return Err(Problem::failed("poll condition timed out"));
                        }
                        sleep(remaining.min(Duration::from_millis(poll.interval_ms))).await;
                        continue;
                    }
                }
                // Validate all extracted paths before committing any variables.
                let values = save
                    .iter()
                    .map(|(key, path)| {
                        response
                            .body
                            .pointer(path)
                            .cloned()
                            .map(|v| (key.clone(), v))
                            .ok_or_else(|| {
                                Problem::error(format!("save path does not exist: {path}"))
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                options.context.update(vars, values.into_iter().collect())?;
                result.transport = Some(TransportReport {
                    protocol: "grpc".into(),
                    headers: response.headers,
                    trailers: response.trailers,
                    response_bytes: response.bytes,
                    ..TransportReport::default()
                });
                break;
            }
        }
        Step::Javascript {
            script, timeout_ms, ..
        } => {
            self::script(
                script,
                *timeout_ms,
                false,
                spec,
                step.id(),
                result,
                worker,
                http,
                vars,
                last_response,
                &Value::Null,
                options,
                emit,
            )
            .await?;
        }
    }
    if let Some(hooks) = hooks {
        let info = json!({"body":last_response,"status":result.transport.as_ref().and_then(|t|t.status),"grpc_code":result.grpc_code,"headers":result.transport.as_ref().map(|t|&t.headers),"trailers":result.transport.as_ref().map(|t|&t.trailers)});
        for h in &hooks.tests {
            script(
                &h.script,
                h.timeout_ms,
                true,
                spec,
                step.id(),
                result,
                worker,
                http,
                vars,
                last_response,
                &info,
                options,
                emit,
            )
            .await?;
        }
    }
    Ok(())
}

/// Core API shared by future GUI and CLI adapters. No Qt imports or GUI event loop.
pub async fn run(
    spec: &Scenario,
    options: RunOptions,
    mut emit: impl FnMut(Value),
) -> anyhow::Result<RunReport> {
    spec.validate().map_err(|error| {
        if options.context.policy.sensitive() {
            anyhow::anyhow!("invalid scenario; details withheld in secret mode")
        } else {
            error
        }
    })?;
    let policy = options.context.policy.clone();
    transport::url_allowed(
        &spec.endpoint,
        options.context.transport.config.allow_plaintext_remote,
    )?;
    let mut emit = |event| emit(policy.event(event));
    let started = Instant::now();
    let mut report = RunReport {
        schema_version: spec::FORMAT.into(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        scenario_id: spec.id.clone(),
        outcome: Outcome::Passed,
        duration_ms: 0.0,
        reflection_requests: 0,
        variables: BTreeMap::new(),
        steps: Vec::new(),
        redaction: None,
    };
    let mut state = ExecutionState {
        vars: options.context.initial_values(),
        ..ExecutionState::default()
    };
    emit(json!({"type":"run_started","scenario_id":spec.id,"schema_version":spec::FORMAT}));
    let mut capture_budget = 1024 * 1024;
    for step in &spec.steps {
        let mut result = StepReport::new(step.id());
        if report.outcome != Outcome::Passed {
            result.outcome = Outcome::Skipped;
            report.steps.push(result);
            continue;
        }
        let step_started = Instant::now();
        emit(json!({"type":"step_started","step_id":step.id()}));
        let mut remaining =
            Duration::from_millis(spec.timeout_ms).saturating_sub(started.elapsed());
        if let Some(deadline) = options.deadline {
            remaining = remaining.min(deadline.saturating_duration_since(Instant::now()));
        }
        let work = execute_step(spec, step, &mut result, &mut state, &options, &mut emit);
        let execution = tokio::select! {
            biased;
            _ = options.cancellation.cancelled() => Err(Problem { outcome: Outcome::Cancelled, message: "run cancelled".into() }),
            _ = sleep(remaining) => Err(Problem::error("run deadline exceeded")),
            value = work => value,
        };
        if let Err(problem) = execution {
            result.outcome = problem.outcome;
            result.error = Some(problem.message);
            report.outcome = result.outcome;
        }
        result.duration_ms = step_started.elapsed().as_secs_f64() * 1000.0;
        capture::step(&mut result, &mut capture_budget);
        policy.step(&mut result);
        if let Some(journal) = &options.journal {
            journal
                .lock()
                .unwrap()
                .record(&json!({"type":"step","instance_id":options.instance_id,"step":result}))?;
        }
        emit(
            json!({"type":"step_finished","step_id":step.id(),"outcome":result.outcome,"duration_ms":result.duration_ms}),
        );
        report.steps.push(result);
    }
    report.reflection_requests = state.client.map(|c| c.reflection_requests).unwrap_or(0)
        + state
            .regression_clients
            .values()
            .map(|c| c.reflection_requests)
            .sum::<usize>();
    report.variables = state.vars;
    if !capture::fits(&report.variables, 64 * 1024) {
        report.variables.clear();
    }
    report.duration_ms = started.elapsed().as_secs_f64() * 1000.0;
    emit(
        json!({"type":"run_finished","scenario_id":spec.id,"outcome":report.outcome,"duration_ms":report.duration_ms,"exit_code":report.exit_code()}),
    );
    policy.report(&mut report);
    Ok(report)
}
