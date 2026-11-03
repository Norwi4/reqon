use super::{Package, Plan, Selection};
use crate::{Outcome, RunOptions, RunReport, StepReport, report};
use anyhow::{Result, anyhow};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct CaseResult {
    pub case_id: String,
    pub instance_id: String,
    pub iteration: usize,
    pub data_row: Option<usize>,
    pub outcome: Outcome,
    pub skip_reason: Option<String>,
    pub report: Option<RunReport>,
}

#[derive(Debug, Serialize)]
pub struct PackageReport {
    pub schema_version: String,
    pub engine_version: String,
    pub plan: Plan,
    pub outcome: Outcome,
    pub duration_ms: f64,
    pub error: Option<String>,
    pub cases: Vec<CaseResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redaction: Option<crate::secrets::RedactionInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_id: Option<String>,
}

impl PackageReport {
    pub fn exit_code(&self) -> i32 {
        match self.outcome {
            Outcome::Passed => 0,
            Outcome::Failed => 1,
            Outcome::Cancelled => 130,
            _ => 2,
        }
    }

    pub fn junit(&self) -> String {
        let mut output = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n");
        for (case, planned) in self.cases.iter().zip(&self.plan.cases) {
            let skipped;
            let run = if let Some(run) = &case.report {
                run
            } else {
                skipped = RunReport {
                    schema_version: crate::spec::FORMAT.into(),
                    engine_version: self.engine_version.clone(),
                    scenario_id: case.case_id.clone(),
                    outcome: Outcome::Skipped,
                    duration_ms: 0.0,
                    reflection_requests: 0,
                    variables: BTreeMap::new(),
                    redaction: None,
                    steps: planned
                        .step_ids
                        .iter()
                        .map(|id| StepReport {
                            id: id.clone(),
                            outcome: Outcome::Skipped,
                            duration_ms: 0.0,
                            attempts: 0,
                            checks: Vec::new(),
                            error: case.skip_reason.clone(),
                            grpc_code: None,
                            response: None,
                            request: None,
                            transport: None,
                            script_checks: Vec::new(),
                            logs: Vec::new(),
                            check_summary: None,
                            capture_omitted: false,
                        })
                        .collect(),
                };
                &skipped
            };
            output.push_str(&report::junit_suite(
                run,
                &format!("{}/{}", self.plan.package_id, case.instance_id),
            ));
        }
        // Cancellation/deadline may occur between cases, before any step can
        // report it. Keep JUnit red even when every case was skipped or passed.
        if self.error.is_some()
            && !self
                .cases
                .iter()
                .filter_map(|c| c.report.as_ref())
                .any(|r| {
                    r.steps
                        .iter()
                        .any(|s| matches!(s.outcome, Outcome::Error | Outcome::Cancelled))
                })
        {
            let lifecycle = RunReport {
                schema_version: crate::spec::FORMAT.into(),
                engine_version: self.engine_version.clone(),
                scenario_id: self.plan.package_id.clone(),
                outcome: self.outcome,
                duration_ms: self.duration_ms,
                reflection_requests: 0,
                variables: BTreeMap::new(),
                redaction: None,
                steps: vec![StepReport {
                    id: "package-lifecycle".into(),
                    outcome: self.outcome,
                    error: self.error.clone(),
                    ..StepReport::new("package-lifecycle")
                }],
            };
            output.push_str(&report::junit_suite(&lifecycle, &self.plan.package_id));
        }
        output.push_str("</testsuites>\n");
        output
    }
}

fn merge(current: Outcome, next: Outcome) -> Outcome {
    let rank = |value| match value {
        Outcome::Cancelled => 4,
        Outcome::Error => 3,
        Outcome::Failed => 2,
        Outcome::Passed => 1,
        Outcome::Skipped => 0,
    };
    if rank(next) > rank(current) {
        next
    } else {
        current
    }
}

pub async fn run(
    package: &Package,
    selection: &Selection,
    endpoint: &str,
    mut options: RunOptions,
    mut emit: impl FnMut(Value),
) -> Result<PackageReport> {
    options.assets = package.assets.clone();
    let policy = options.context.policy.clone();
    let mut emit = |event| emit(policy.event(event));
    let plan = package
        .plan(selection)
        .map_err(|e| anyhow!("{}", e[0].message))?;
    // Validate all execution inputs before the first case. No partial run when a
    // later case has an incompatible configuration.
    // Validate all selected definitions and data rows, retain no duplicated
    // scenarios/contexts: iterations can share megabytes of immutable input.
    for planned in &plan.cases {
        let case = package
            .cases
            .iter()
            .find(|c| c.definition.id == planned.id)
            .unwrap();
        let context = options
            .context
            .for_case(&package.manifest.variables, &case.definition.variables)?
            .with_data(
                planned
                    .data_row
                    .map(|row| &selection.dataset.as_ref().unwrap().rows[row - 1]),
                planned.iteration,
            )?;
        let mut scenario = case.definition.scenario(endpoint);
        scenario.metadata = context.resolved_metadata()?;
        scenario.validate()?;
    }
    let started = Instant::now();
    let deadline = started + Duration::from_millis(plan.timeout_ms);
    let deadline = options
        .deadline
        .map(|outer| outer.min(deadline))
        .unwrap_or(deadline);
    options.deadline = Some(deadline);
    let mut outcome = Outcome::Passed;
    let mut cases = Vec::new();
    let mut stop = None;
    let mut error = None;
    emit(
        json!({"type":"package_started","event_schema_version":"reqon-package-events/v1","package_id":plan.package_id,"plan_sha256":plan.plan_sha256,"case_count":plan.cases.len()}),
    );
    for (index, planned) in plan.cases.iter().enumerate() {
        if index > 0 && stop.is_none() && selection.delay_ms > 0 {
            tokio::select! {
                _=options.cancellation.cancelled()=>{},
                _=tokio::time::sleep(Duration::from_millis(selection.delay_ms).min(deadline.saturating_duration_since(Instant::now())))=>{},
            }
        }
        if options.cancellation.is_cancelled() {
            outcome = Outcome::Cancelled;
            stop = Some("package cancelled");
            error = stop.map(str::to_owned);
        } else if Instant::now() >= deadline && outcome != Outcome::Cancelled {
            outcome = Outcome::Error;
            stop = Some("package deadline exceeded");
            error = stop.map(str::to_owned);
        }
        let skip_reason = if planned.disabled {
            Some("disabled")
        } else {
            stop
        };
        let mut result = if let Some(reason) = skip_reason {
            CaseResult {
                case_id: planned.id.clone(),
                instance_id: planned.instance_id.clone(),
                iteration: planned.iteration,
                data_row: planned.data_row,
                outcome: Outcome::Skipped,
                skip_reason: Some(reason.into()),
                report: None,
            }
        } else {
            let case = package
                .cases
                .iter()
                .find(|c| c.definition.id == planned.id)
                .unwrap();
            let context = options
                .context
                .for_case(&package.manifest.variables, &case.definition.variables)?
                .with_data(
                    planned
                        .data_row
                        .map(|row| &selection.dataset.as_ref().unwrap().rows[row - 1]),
                    planned.iteration,
                )?;
            let mut scenario = case.definition.scenario(endpoint);
            scenario.metadata = context.resolved_metadata()?;
            emit(
                json!({"type":"case_started","event_schema_version":"reqon-package-events/v1","package_id":plan.package_id,"case_id":planned.id,"instance_id":planned.instance_id,"iteration":planned.iteration,"data_row":planned.data_row}),
            );
            let case_options = RunOptions {
                context,
                instance_id: planned.instance_id.clone(),
                ..options.clone()
            };
            let report = crate::run(&scenario, case_options, |mut event| {
                event["package_id"] = json!(plan.package_id);
                event["case_id"] = json!(planned.id);
                event["instance_id"] = json!(planned.instance_id);
                event["event_schema_version"] = json!("reqon-package-events/v1");
                emit(event);
            })
            .await?;
            outcome = merge(outcome, report.outcome);
            if selection.bail && matches!(report.outcome, Outcome::Failed | Outcome::Error) {
                stop = Some("bail after case failure");
            }
            if report.outcome == Outcome::Cancelled {
                stop = Some("package cancelled");
                error = stop.map(str::to_owned);
            }
            if Instant::now() >= deadline && outcome != Outcome::Cancelled {
                outcome = Outcome::Error;
                stop = Some("package deadline exceeded");
                error = stop.map(str::to_owned);
            }
            CaseResult {
                case_id: planned.id.clone(),
                instance_id: planned.instance_id.clone(),
                iteration: planned.iteration,
                data_row: planned.data_row,
                outcome: report.outcome,
                skip_reason: None,
                report: Some(report),
            }
        };
        emit(
            json!({"type":"case_finished","event_schema_version":"reqon-package-events/v1","package_id":plan.package_id,"case_id":planned.id,"instance_id":planned.instance_id,"outcome":result.outcome,"skip_reason":result.skip_reason}),
        );
        result.case_id = policy.text(&result.case_id);
        result.instance_id = policy.text(&result.instance_id);
        if let Some(journal) = &options.journal {
            let skipped;
            let report = if let Some(report) = &result.report {
                report
            } else {
                skipped = RunReport {
                    schema_version: crate::spec::FORMAT.into(),
                    engine_version: env!("CARGO_PKG_VERSION").into(),
                    scenario_id: result.case_id.clone(),
                    outcome: Outcome::Skipped,
                    duration_ms: 0.0,
                    reflection_requests: 0,
                    variables: BTreeMap::new(),
                    redaction: policy.info(),
                    steps: planned
                        .step_ids
                        .iter()
                        .map(|id| StepReport {
                            outcome: Outcome::Skipped,
                            error: result.skip_reason.clone(),
                            ..StepReport::new(&policy.text(id))
                        })
                        .collect(),
                };
                &skipped
            };
            let xml = report::junit_suite(
                report,
                &format!("{}/{}", policy.text(&plan.package_id), result.instance_id),
            );
            let mut journal = journal.lock().unwrap();
            journal.record(&json!({"type":"case","result":result,"junit":xml}))?;
            journal.sync()?;
            result.report = None;
        }
        cases.push(result);
    }
    let mut result = PackageReport {
        schema_version: "reqon-package-result/v1".into(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        plan,
        outcome,
        duration_ms: started.elapsed().as_secs_f64() * 1000.0,
        error,
        cases,
        redaction: policy.info(),
        environment_id: options
            .context
            .environment_id
            .as_ref()
            .map(|s| policy.text(s)),
    };
    emit(
        json!({"type":"package_finished","event_schema_version":"reqon-package-events/v1","package_id":result.plan.package_id,"outcome":result.outcome,"exit_code":result.exit_code(),"duration_ms":result.duration_ms}),
    );
    if policy.sensitive() {
        result.plan.redact(&policy);
        for case in &mut result.cases {
            case.case_id = policy.text(&case.case_id);
            case.instance_id = policy.text(&case.instance_id);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tokio_util::sync::CancellationToken;

    async fn stopped_before_first_case(cancel: bool) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/package/catalog.json");
        let package = crate::package::load(&path, &std::fs::read(&path).unwrap()).unwrap();
        let cancellation = CancellationToken::new();
        if cancel {
            cancellation.cancel();
        }
        let options = RunOptions {
            cancellation,
            javascript_executable: PathBuf::from("must-not-launch"),
            deadline: (!cancel).then(Instant::now),
            context: crate::runtime::ExecutionContext::default(),
            assets: Default::default(),
            journal: None,
            instance_id: String::new(),
        };
        let mut events = Vec::new();
        let report = run(
            &package,
            &Selection::default(),
            "http://127.0.0.1:1",
            options,
            |e| events.push(e),
        )
        .await
        .unwrap();
        assert_eq!(report.exit_code(), if cancel { 130 } else { 2 });
        assert!(report.cases.iter().all(|c| c.report.is_none()));
        assert!(events.iter().all(|e| e["type"] != "case_started"));
        assert!(report.junit().contains("name=\"package-lifecycle\""));
        assert!(report.junit().contains("<error "));
    }

    #[tokio::test]
    async fn cancelled_before_first_case_is_visible_in_junit() {
        stopped_before_first_case(true).await;
    }

    #[tokio::test]
    async fn expired_before_first_case_is_visible_in_junit() {
        stopped_before_first_case(false).await;
    }
}
