//! Shared preparation for interactive WebSocket and gRPC transports.
use crate::model::*;

pub(crate) type ProtocolInputs = (
    Vec<Pair>,
    Vec<Pair>,
    std::collections::BTreeMap<String, String>,
);
#[cfg(test)]
pub(crate) fn prepare_protocol(
    request: Request,
    preferences: Preferences,
    inputs: ProtocolInputs,
) -> anyhow::Result<(Request, std::collections::BTreeMap<String, String>)> {
    prepare_protocol_cancellable(
        request,
        preferences,
        inputs,
        &crate::cancellation::CancellationToken::new(),
    )
}
pub(crate) fn prepare_protocol_cancellable(
    request: Request,
    preferences: Preferences,
    (globals, mut env, runtime): ProtocolInputs,
    token: &crate::cancellation::CancellationToken,
) -> anyhow::Result<(Request, std::collections::BTreeMap<String, String>)> {
    crate::cancellation::check(token)?;
    crate::environment_vault::resolve_rows(&mut env, true)?;
    let mut request = crate::inheritance::prepare_scoped(request, &globals, &env, &runtime)?;
    preferences.apply(&mut request);
    let mut vars = variables(
        &globals,
        &request.inherited_collection_vars,
        &env,
        &request.vars,
    );
    vars.extend(runtime);
    crate::environment_vault::resolve_variables(&mut vars, &mut vec![])?;
    crate::auth_vault::resolve_with_variables(&mut request, &vars)?;
    if !preferences.secret_providers.is_empty() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(crate::cancellation::run(
                token,
                crate::integrations::resolve_secrets_async(&preferences, &request, &mut vars),
            ))?;
    }
    crate::cancellation::check(token)?;
    request.script_scope = Some(ScriptScope {
        environment: typed_variables(&env),
        globals: typed_variables(&globals),
        collection: typed_variables(&request.inherited_collection_vars),
    });
    Ok((request, vars))
}
