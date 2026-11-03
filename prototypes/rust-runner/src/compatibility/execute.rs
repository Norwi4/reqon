use super::{Call, duration, evaluate, find, substitute};
use crate::{ExecutionState, Problem, RunOptions, StepReport, TransportReport, spec::Scenario};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};
use tokio::time::{sleep, timeout};

pub(crate) fn save(
    body: &Value,
    save: &BTreeMap<String, String>,
    vars: &mut crate::runtime::Variables,
    options: &RunOptions,
) -> Result<(), Problem> {
    for (name, path) in save {
        let value = find(body, path)
            .filter(|v| !v.is_null())
            .ok_or_else(|| Problem::failed("compatibility save path is missing or null"))?;
        options
            .context
            .update(vars, BTreeMap::from([(name.clone(), value.clone())]))?;
    }
    Ok(())
}

pub(crate) async fn call(
    spec: &Scenario,
    call: &Call,
    result: &mut StepReport,
    state: &mut ExecutionState,
    options: &RunOptions,
    emit: &mut impl FnMut(Value),
) -> Result<(), Problem> {
    let request = substitute(
        &serde_json::to_value(&call.request).map_err(anyhow::Error::from)?,
        &state.vars,
    )?;
    result.request = Some(crate::capture::value(&request, 64 * 1024));
    let profile = &options.context.regression;
    let configured = profile.endpoint(call, &spec.endpoint);
    let endpoint = configured
        .as_ref()
        .copied()
        .unwrap_or(&spec.endpoint)
        .to_owned();
    crate::transport::url_allowed(
        &endpoint,
        options.context.transport.config.allow_plaintext_remote,
    )?;
    let call_ms = profile.timeout();
    let deadline = call
        .poll
        .as_ref()
        .map(|p| duration(&p.timeout).map(|ms| Instant::now() + Duration::from_millis(ms)))
        .transpose()?;
    let expected = call
        .poll
        .as_ref()
        .and_then(|p| p.until.as_ref())
        .unwrap_or(&call.expect);
    loop {
        result.attempts += 1;
        emit(json!({"type":"attempt_started","step_id":result.id,"attempt":result.attempts}));
        let operation = async {
            if configured.is_err() {
                anyhow::bail!("INVOKER_NOT_CONFIGURED");
            }
            if !state.regression_clients.contains_key(&endpoint) {
                let client = crate::grpc::DynamicClient::connect(
                    &endpoint,
                    &spec.metadata,
                    spec.max_response_bytes,
                    &options.context.transport,
                    &options.context.session,
                )
                .await?;
                state.regression_clients.insert(endpoint.clone(), client);
            }
            let client = state.regression_clients.get_mut(&endpoint).unwrap();
            let descriptor = client
                .compatibility_method(&call.service, &call.method)
                .await?;
            if descriptor.is_client_streaming() || descriptor.is_server_streaming() {
                return Err(anyhow::anyhow!("UNSUPPORTED_METHOD"));
            }
            prost_reflect::DynamicMessage::deserialize(descriptor.input(), &request)
                .map_err(|_| anyhow::anyhow!("REQUEST_FORMAT_ERROR"))?;
            let response = client.unary(&descriptor, &request, call_ms).await?;
            let mut business_error = None;
            for field in descriptor.output().fields() {
                if field.is_list() || field.is_map() {
                    continue;
                }
                if let prost_reflect::Kind::Message(message) = field.kind()
                    && message.name() == "ServerCallInfo"
                    && let Some(info) = response.body.get(field.json_name())
                    && info
                        .get("result")
                        .and_then(Value::as_str)
                        .is_some_and(|v| v.eq_ignore_ascii_case("ERROR"))
                {
                    business_error = Some(
                        info.get("resultCode")
                            .and_then(Value::as_str)
                            .filter(|v| !v.is_empty())
                            .unwrap_or("ERROR")
                            .to_owned(),
                    );
                }
            }
            Ok::<_, anyhow::Error>((response, business_error))
        };
        let (body, error) = match timeout(Duration::from_millis(call_ms), operation).await {
            Ok(Ok((response, error))) => {
                result.grpc_code = Some("OK".into());
                result.transport = Some(TransportReport {
                    protocol: "grpc".into(),
                    headers: response.headers,
                    trailers: response.trailers,
                    response_bytes: response.bytes,
                    ..Default::default()
                });
                (
                    if error.is_some() {
                        Value::Null
                    } else {
                        response.body
                    },
                    error,
                )
            }
            Ok(Err(error)) => {
                let text = error.to_string();
                let local = if [
                    "REQUEST_FORMAT_ERROR",
                    "UNSUPPORTED_METHOD",
                    "INVOKER_NOT_CONFIGURED",
                ]
                .contains(&text.as_str())
                {
                    text.as_str()
                } else {
                    "INVOCATION_FAILED"
                };
                let code = error
                    .downcast_ref::<tonic::Status>()
                    .map(|s| code_name(s.code()))
                    .unwrap_or(local)
                    .to_string();
                result.grpc_code = Some(code.clone());
                (Value::Null, Some(code))
            }
            Err(_) => {
                result.grpc_code = Some("DEADLINE_EXCEEDED".into());
                (Value::Null, Some("DEADLINE_EXCEEDED".into()))
            }
        };
        result.response = Some(crate::capture::value(&body, 64 * 1024));
        state.last_response = body.clone();
        result.checks = evaluate(expected, &body, error.as_deref(), &state.vars)?;
        emit(
            json!({"type":"attempt_finished","step_id":result.id,"attempt":result.attempts,"grpc_code":result.grpc_code}),
        );
        if result.checks.iter().all(|c| c.passed) {
            if call.poll.as_ref().is_some_and(|p| p.until.is_some()) {
                result.checks = evaluate(&call.expect, &body, error.as_deref(), &state.vars)?;
            }
            if result.checks.iter().any(|c| !c.passed) {
                return Err(Problem::failed("compatibility response assertion failed"));
            }
            return save(&body, &call.save, &mut state.vars, options);
        }
        let Some(poll) = &call.poll else {
            return Err(Problem::failed("compatibility response assertion failed"));
        };
        let interval = Duration::from_millis(duration(&poll.interval)?);
        if Instant::now() + interval > deadline.unwrap() {
            if poll.on_timeout == "continue" {
                if error.is_none() {
                    let _ = save(&body, &call.save, &mut state.vars, options);
                }
                result.logs.push(
                    "Poll timed out; continuing as explicitly requested by the Java scenario"
                        .into(),
                );
                result.checks.clear();
                return Ok(());
            }
            return Err(Problem::failed("compatibility poll condition timed out"));
        }
        sleep(interval).await;
    }
}

fn code_name(code: tonic::Code) -> &'static str {
    match code {
        tonic::Code::Ok => "OK",
        tonic::Code::Cancelled => "CANCELLED",
        tonic::Code::Unknown => "UNKNOWN",
        tonic::Code::InvalidArgument => "INVALID_ARGUMENT",
        tonic::Code::DeadlineExceeded => "DEADLINE_EXCEEDED",
        tonic::Code::NotFound => "NOT_FOUND",
        tonic::Code::AlreadyExists => "ALREADY_EXISTS",
        tonic::Code::PermissionDenied => "PERMISSION_DENIED",
        tonic::Code::ResourceExhausted => "RESOURCE_EXHAUSTED",
        tonic::Code::FailedPrecondition => "FAILED_PRECONDITION",
        tonic::Code::Aborted => "ABORTED",
        tonic::Code::OutOfRange => "OUT_OF_RANGE",
        tonic::Code::Unimplemented => "UNIMPLEMENTED",
        tonic::Code::Internal => "INTERNAL",
        tonic::Code::Unavailable => "UNAVAILABLE",
        tonic::Code::DataLoss => "DATA_LOSS",
        tonic::Code::Unauthenticated => "UNAUTHENTICATED",
    }
}
