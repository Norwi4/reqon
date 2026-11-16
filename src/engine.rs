use crate::cancellation::{self, CancellationToken};
use crate::{cookies::Jar, model::*, scripts};
use anyhow::{Context, Result, bail};
use base64::Engine as _;
use reqwest::{
    Client,
    header::{HeaderMap, HeaderName, HeaderValue},
    multipart,
};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub const MAX_RESPONSE_BYTES: usize = 32 * 1024 * 1024;
fn certificate_key(
    request: &Request,
    path: &str,
    vars: &BTreeMap<String, String>,
) -> (String, u128, u64) {
    if path.is_empty() {
        return (String::new(), 0, 0);
    }
    let path = file_path(request, &resolve(path, vars));
    let metadata = std::fs::metadata(&path).ok();
    (
        path.to_string_lossy().into_owned(),
        metadata
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0),
        metadata.map(|m| m.len()).unwrap_or(0),
    )
}
async fn send_redirects(
    client: &Client,
    mut next: reqwest::Request,
    settings: &Request,
    vars: &BTreeMap<String, String>,
    timeline: &mut Vec<String>,
) -> Result<reqwest::Response> {
    let deadline = if settings.timeout > 0 {
        Some(tokio::time::Instant::now() + Duration::from_millis(settings.timeout))
    } else {
        None
    };
    let mut visited = std::collections::HashSet::new();
    let mut allow_auth = true;
    for redirects in 0..=settings.max_redirects {
        if allow_auth {
            match settings.auth.kind.as_str() {
                "awsv4" => crate::auth::sign_aws(&mut next, &settings.auth, vars)?,
                "oauth1" => crate::auth::sign_oauth1(&mut next, &settings.auth, vars)?,
                "edgegrid" => crate::auth::sign_edgegrid(&mut next, &settings.auth, vars)?,
                _ => {}
            }
        }
        if let Some(deadline) = deadline {
            *next.timeout_mut() =
                Some(deadline.saturating_duration_since(tokio::time::Instant::now()));
        }
        let previous = next.try_clone();
        let response = if settings.auth.kind == "digest" && allow_auth {
            crate::auth::digest_send(client, next, &settings.auth, vars).await?
        } else if settings.auth.kind == "ntlm" && allow_auth {
            crate::ntlm::send(client, next, &settings.auth, vars).await?
        } else {
            client.execute(next).await.context("Request failed")?
        };
        if !settings.follow_redirects
            || settings.max_redirects == 0
            || !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308)
        {
            return Ok(response);
        }
        let Some(location) = response
            .headers()
            .get("location")
            .and_then(|v| v.to_str().ok())
        else {
            return Ok(response);
        };
        let destination = response
            .url()
            .join(location)
            .context("Invalid redirect URL")?;
        if !["http", "https"].contains(&destination.scheme()) {
            bail!("Redirect target must use HTTP or HTTPS");
        }
        if redirects >= settings.max_redirects {
            bail!("Maximum redirects ({}) exceeded", settings.max_redirects);
        }
        if !visited.insert(destination.clone()) {
            bail!("Redirect loop at {destination}");
        }
        let mut request = previous.context("Cannot replay streaming request body on redirect")?;
        let cross_origin = request.url().origin() != destination.origin();
        if settings.portable["http_follow_original_method"] != true
            && (response.status().as_u16() == 303 && request.method() != reqwest::Method::HEAD
                || matches!(response.status().as_u16(), 301 | 302)
                    && request.method() == reqwest::Method::POST)
        {
            *request.method_mut() = reqwest::Method::GET;
            *request.body_mut() = None;
            for key in ["content-length", "content-type", "transfer-encoding"] {
                request.headers_mut().remove(key);
            }
        }
        if cross_origin {
            request.headers_mut().remove("cookie");
            request.headers_mut().remove("host");
            if !settings.forward_auth {
                allow_auth = false;
                request.headers_mut().remove("authorization");
                request.headers_mut().remove("proxy-authorization");
                for key in [
                    "x-wsse",
                    "x-amz-security-token",
                    "x-amz-date",
                    "x-amz-content-sha256",
                ] {
                    request.headers_mut().remove(key);
                }
            }
        }
        timeline.push(format!(
            "{} redirect → {destination}",
            response.status().as_u16()
        ));
        *request.url_mut() = destination;
        next = request;
    }
    unreachable!()
}
pub async fn timed<F: std::future::Future>(milliseconds: u64, future: F) -> Result<F::Output> {
    if milliseconds == 0 {
        Ok(future.await)
    } else {
        Ok(tokio::time::timeout(Duration::from_millis(milliseconds), future).await?)
    }
}
#[derive(Clone)]
pub struct Engine {
    cancellation: CancellationToken,
    pub cookies: Arc<Jar>,
    pub tokens: Arc<crate::auth::TokenCache>,
    clients: Arc<std::sync::Mutex<BTreeMap<String, Client>>>,
    preferences: Arc<std::sync::Mutex<Preferences>>,
}
struct CookiePolicy {
    jar: Arc<Jar>,
    store: bool,
    send: bool,
}
impl reqwest::cookie::CookieStore for CookiePolicy {
    fn set_cookies(
        &self,
        headers: &mut dyn Iterator<Item = &reqwest::header::HeaderValue>,
        url: &url::Url,
    ) {
        if self.store {
            self.jar.set_cookies(headers, url);
        }
    }
    fn cookies(&self, url: &url::Url) -> Option<reqwest::header::HeaderValue> {
        if self.send {
            self.jar.cookies(url)
        } else {
            None
        }
    }
}
impl Default for Engine {
    fn default() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        Self {
            cancellation: CancellationToken::new(),
            cookies: Arc::new(Jar::default()),
            tokens: Arc::new(crate::auth::TokenCache::default()),
            clients: Arc::new(std::sync::Mutex::new(BTreeMap::new())),
            preferences: Arc::new(std::sync::Mutex::new(Preferences::default())),
        }
    }
}
impl Engine {
    /// Bind this clone to a caller-owned cancellation scope. Cached clients and
    /// cookies remain shared; cancelling a request never poisons sibling scopes.
    pub fn with_cancellation(mut self, token: CancellationToken) -> Self {
        self.cancellation = token;
        self
    }
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation.clone()
    }
    fn execution_scope(&self) -> Self {
        self.clone()
            .with_cancellation(self.cancellation.child_token())
    }

    pub fn configure(&self, preferences: Preferences) {
        if !preferences.cache.ssl {
            self.clear_tls_cache();
        }
        *self.preferences.lock().unwrap() = preferences;
    }
    pub fn clear_tls_cache(&self) {
        self.clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }
    pub async fn execute_scoped(
        &self,
        request: Request,
        globals: Vec<Pair>,
        env: Vec<Pair>,
        runtime: BTreeMap<String, String>,
    ) -> Reply {
        self.execute_with_collection(request, globals, env, runtime, BTreeMap::new())
            .await
    }
    pub async fn execute_with_collection(
        &self,
        request: Request,
        globals: Vec<Pair>,
        env: Vec<Pair>,
        runtime: BTreeMap<String, String>,
        collection: BTreeMap<String, serde_json::Value>,
    ) -> Reply {
        let engine = self.execution_scope();
        let _guard = engine.cancellation.clone().drop_guard();
        if engine.cancellation.is_cancelled() {
            return cancellation::reply();
        }
        engine
            .execute_collection_inner(request, globals, env, runtime, collection)
            .await
    }
    async fn execute_collection_inner(
        &self,
        request: Request,
        globals: Vec<Pair>,
        mut env: Vec<Pair>,
        runtime: BTreeMap<String, String>,
        collection: BTreeMap<String, serde_json::Value>,
    ) -> Reply {
        if let Err(error) = crate::environment_vault::resolve_rows(&mut env, true) {
            return Reply {
                error: error.to_string(),
                ..Default::default()
            };
        }
        let context_globals = globals.clone();
        let context_env = env.clone();
        let mut context_runtime = runtime.clone();
        context_runtime.extend(crate::runner_data::variables(&request.iteration_data));
        match tokio::task::spawn_blocking(move || {
            crate::inheritance::prepare_with_collection(
                request,
                &context_globals,
                &context_env,
                &context_runtime,
                &collection,
            )
        })
        .await
        {
            Ok(Ok(mut request)) => {
                self.preferences.lock().unwrap().apply(&mut request);
                let mut vars = variables(
                    &globals,
                    &request.inherited_collection_vars,
                    &env,
                    &request.vars,
                );
                request.script_scope = Some(ScriptScope {
                    environment: typed_variables(&env),
                    globals: typed_variables(&globals),
                    collection: typed_variables(&request.inherited_collection_vars),
                });
                vars.extend(runtime);
                vars.extend(crate::runner_data::variables(&request.iteration_data));
                let secret_names = globals
                    .iter()
                    .chain(&env)
                    .filter(|p| p.secret && p.enabled)
                    .map(|p| variable_name(&p.key).to_owned())
                    .collect();
                let mut reply = self.execute_sensitive(request, vars, secret_names).await;
                reply.redaction_values.extend(
                    globals
                        .iter()
                        .chain(&env)
                        .filter(|p| p.secret && !p.value.is_empty())
                        .map(|p| p.value.clone()),
                );
                for row in env.iter().filter(|p| p.secret) {
                    if let Some(value) = reply.script_effects.environment.get(&row.key) {
                        reply.redaction_values.push(
                            value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string()),
                        );
                    }
                    if let Some(value) = reply.runtime_vars.get(&row.key) {
                        reply.redaction_values.push(value.clone());
                    }
                }
                reply
            }
            result => Reply {
                error: format!("Cannot prepare request: {result:?}"),
                ..Default::default()
            },
        }
    }
    pub async fn execute(&self, request: Request, vars: BTreeMap<String, String>) -> Reply {
        let engine = self.execution_scope();
        let _guard = engine.cancellation.clone().drop_guard();
        engine.execute_sensitive(request, vars, vec![]).await
    }
    async fn execute_sensitive(
        &self,
        request: Request,
        vars: BTreeMap<String, String>,
        secret_names: Vec<String>,
    ) -> Reply {
        let start = Instant::now();
        let grpc = request.protocol == "grpc";
        let mut redaction_values = Vec::new();
        // This pipeline contains several large transport/provider futures. Keep
        // it off the Windows CLI main thread's small stack (also in debug builds).
        let mut reply =
            match Box::pin(self.execute_inner(request, vars, &mut redaction_values, &secret_names))
                .await
            {
                Ok(mut r) => {
                    r.elapsed_ms = start.elapsed().as_millis();
                    r
                }
                Err(e) if e.is::<cancellation::Cancelled>() || self.cancellation.is_cancelled() => {
                    let mut reply = cancellation::reply();
                    reply.elapsed_ms = start.elapsed().as_millis();
                    reply
                }
                Err(e) => Reply {
                    status: if grpc {
                        e.downcast_ref::<tonic::Status>()
                            .map(|s| s.code() as u16)
                            .unwrap_or(2)
                    } else {
                        0
                    },
                    reason: if grpc {
                        e.downcast_ref::<tonic::Status>()
                            .map(|s| format!("{:?}", s.code()).to_uppercase())
                            .unwrap_or_else(|| "UNKNOWN".into())
                    } else {
                        String::new()
                    },
                    error: format!("{e:#}"),
                    elapsed_ms: start.elapsed().as_millis(),
                    ..Default::default()
                },
            };
        reply.redaction_values = redaction_values;
        reply
    }
    async fn execute_inner(
        &self,
        mut request: Request,
        mut vars: BTreeMap<String, String>,
        redaction_values: &mut Vec<String>,
        secret_names: &[String],
    ) -> Result<Reply> {
        cancellation::check(&self.cancellation)?;
        crate::environment_vault::resolve_variables(&mut vars, redaction_values)?;
        crate::environment_vault::capture_values(secret_names, &vars, redaction_values);
        crate::auth_vault::resolve_with_variables(&mut request, &vars)?;
        redaction_values.extend(crate::auth_vault::execution_values(&request, &vars));
        let secret_preferences = self.preferences.lock().unwrap().clone();
        if !secret_preferences.secret_providers.is_empty() {
            cancellation::run(
                &self.cancellation,
                Box::pin(crate::integrations::resolve_secrets_async(
                    &secret_preferences,
                    &request,
                    &mut vars,
                )),
            )
            .await?;
        }
        let initial_vars = vars.clone();
        crate::environment_vault::capture_values(secret_names, &vars, redaction_values);
        let mut logs = vec![];
        let mut pre_tests = vec![];
        let mut effects = ScriptEffects::default();
        if !request.pre_script.trim().is_empty() {
            let r = request.clone();
            let v = vars.clone();
            let token = self.cancellation.clone();
            let out = tokio::task::spawn_blocking(move || {
                scripts::run_cancellable(&r.pre_script, &r, &Reply::default(), &v, &token)
            })
            .await??;
            request = out.request;
            vars = out.vars;
            logs.extend(out.logs);
            pre_tests.extend(out.tests);
            effects.merge(out.effects);
        }
        redaction_values.extend(crate::auth_vault::execution_values(&request, &vars));
        crate::environment_vault::capture_values(secret_names, &vars, redaction_values);
        let mut reply = if effects.skip {
            Reply {
                reason: "Skipped".into(),
                ..Default::default()
            }
        } else {
            let preferences = self.preferences.lock().unwrap().clone();
            cancellation::run(
                &self.cancellation,
                crate::preference_network::prepare(&preferences, &mut request, &vars),
            )
            .await?;
            let result = cancellation::run(&self.cancellation, async {
                match request.protocol.as_str() {
                    "grpc" => crate::grpc::execute(&request, &vars).await,
                    "ws" | "websocket" => crate::websocket::exchange(&request, &vars).await,
                    _ => self.http(&request, &vars).await,
                }
            })
            .await;
            cancellation::check(&self.cancellation)?;
            match result {
                Ok(reply) => reply,
                Err(error) => Reply {
                    error: format!("{error:#}"),
                    status: if request.protocol == "grpc" {
                        error
                            .downcast_ref::<tonic::Status>()
                            .map(|s| s.code() as u16)
                            .unwrap_or(2)
                    } else {
                        0
                    },
                    reason: if request.protocol == "grpc" {
                        error
                            .downcast_ref::<tonic::Status>()
                            .map(|s| format!("{:?}", s.code()).to_uppercase())
                            .unwrap_or("UNKNOWN".into())
                    } else {
                        String::new()
                    },
                    ..Default::default()
                },
            }
        };
        reply.logs.splice(0..0, logs);
        reply.tests.extend(pre_tests);
        if !effects.skip
            && reply.error.is_empty()
            && (!request.post_script.trim().is_empty()
                || !request.tests.trim().is_empty()
                || request
                    .assertions
                    .iter()
                    .any(|p| p.enabled && !p.key.is_empty())
                || !request.response_vars.is_empty())
        {
            let r = request.clone();
            let v = vars.clone();
            let mut rep = reply.clone();
            rep.bytes.clear();
            let token = self.cancellation.clone();
            match tokio::task::spawn_blocking(move || {
                let mut source = format!("{}\n", r.post_script);
                for p in r
                    .response_vars
                    .iter()
                    .filter(|p| p.enabled && !p.key.is_empty())
                {
                    source.push_str(&format!(
                        "bru.setVar({},eval({}));\n",
                        serde_json::to_string(&p.key)?,
                        serde_json::to_string(&resolve(&p.value, &v))?
                    ));
                }
                for p in r
                    .assertions
                    .iter()
                    .filter(|p| p.enabled && !p.key.is_empty())
                {
                    source.push_str(&format!(
                        "test({},()=>__assert({},{}));\n",
                        serde_json::to_string(&format!("{}: {}", p.key, p.value))?,
                        serde_json::to_string(&p.key)?,
                        serde_json::to_string(&resolve(&p.value, &v))?
                    ));
                }
                source.push_str("\nawait Promise.all(__pendingTests);\n");
                source.push_str(&r.tests);
                scripts::run_cancellable(&source, &r, &rep, &v, &token)
            })
            .await?
            {
                Ok(out) => {
                    reply.tests.extend(out.tests);
                    reply.logs.extend(out.logs);
                    vars = out.vars;
                    effects.merge(out.effects);
                }
                Err(e) => reply.tests.push(TestResult {
                    name: "Post-response script".into(),
                    passed: false,
                    error: format!("{e:#}"),
                }),
            }
        }
        crate::environment_vault::capture_values(secret_names, &vars, redaction_values);
        cancellation::check(&self.cancellation)?;
        reply.deleted_vars = initial_vars
            .keys()
            .filter(|key| !vars.contains_key(*key))
            .cloned()
            .collect();
        reply.script_effects = effects;
        reply.runtime_vars = vars
            .into_iter()
            .filter(|(k, v)| initial_vars.get(k) != Some(v))
            .collect();
        Ok(reply)
    }
    async fn http(&self, request: &Request, vars: &BTreeMap<String, String>) -> Result<Reply> {
        let start = Instant::now();
        let preferences = self.preferences.lock().unwrap().clone();
        let url = request_url(&request, &vars)?;
        let omitted_headers = crate::generated_headers::omitted(request, vars);
        let client_key = serde_json::to_string(&(
            request.portable["http_version"].as_str(),
            &omitted_headers,
            request.timeout,
            request.verify_tls,
            request.follow_redirects,
            request.max_redirects,
            resolve(&request.proxy, &vars),
            request.proxy_disabled,
            certificate_key(request, &request.client_certificate, vars),
            certificate_key(request, &request.client_key, vars),
            certificate_key(request, &request.ca_certificate, vars),
            preferences.store_cookies,
            preferences.send_cookies,
            preferences.keep_default_ca,
            request.client_identity.as_ref().map(|v| {
                crate::disk_sync::fingerprint(&base64::prelude::BASE64_STANDARD.encode(v))
            }),
        ))?;
        let cached = if request.auth.kind == "ntlm" || !preferences.cache.ssl {
            None
        } else {
            self.clients
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&client_key)
                .cloned()
        };
        let client = if let Some(client) = cached {
            client
        } else {
            let mut builder = Client::builder()
                .omit_headers(
                    omitted_headers
                        .iter()
                        .map(|name| HeaderName::from_bytes(name.as_bytes()))
                        .collect::<Result<Vec<_>, _>>()?,
                )
                .cookie_provider(Arc::new(CookiePolicy {
                    jar: self.cookies.clone(),
                    store: preferences.store_cookies,
                    send: preferences.send_cookies,
                }))
                .connect_timeout(Duration::from_secs(30))
                .danger_accept_invalid_certs(!request.verify_tls)
                .redirect(reqwest::redirect::Policy::none());
            match request.portable["http_version"].as_str() {
                Some("HTTP/1.x") => builder = builder.http1_only(),
                Some("HTTP/2") => builder = builder.http2_prior_knowledge(),
                _ => {}
            }
            if request.auth.kind == "ntlm" {
                builder = builder.http1_only().pool_max_idle_per_host(1);
            }
            if request.proxy_disabled {
                builder = builder.no_proxy();
            } else if !request.proxy.trim().is_empty() {
                builder = builder.proxy(reqwest::Proxy::all(resolve(&request.proxy, &vars))?);
            }
            if request.timeout > 0 {
                builder = builder.timeout(Duration::from_millis(request.timeout));
            }
            if let Some(pem) = &request.client_identity {
                builder = builder.identity(reqwest::Identity::from_pem(pem)?);
            } else if !request.client_certificate.trim().is_empty() {
                let mut pem = std::fs::read(file_path(
                    &request,
                    &resolve(&request.client_certificate, &vars),
                ))
                .context("Cannot read client certificate")?;
                if !request.client_key.trim().is_empty() {
                    pem.push(b'\n');
                    pem.extend(
                        std::fs::read(file_path(&request, &resolve(&request.client_key, &vars)))
                            .context("Cannot read client private key")?,
                    );
                }
                builder = builder.identity(
                    reqwest::Identity::from_pem(&pem)
                        .context("Invalid PEM client certificate/private key")?,
                );
            }
            if !request.ca_certificate.trim().is_empty() {
                let pem = std::fs::read(file_path(
                    &request,
                    &resolve(&request.ca_certificate, &vars),
                ))
                .context("Cannot read CA certificate")?;
                let certificates = reqwest::Certificate::from_pem_bundle(&pem)?;
                builder = if preferences.keep_default_ca {
                    builder.tls_certs_merge(certificates)
                } else {
                    builder.tls_certs_only(certificates)
                };
            }
            let client = builder.build()?;
            let mut cache = self.clients.lock().unwrap_or_else(|e| e.into_inner());
            if cache.len() >= 16 {
                cache.clear();
            }
            if request.auth.kind != "ntlm" && preferences.cache.ssl {
                cache.insert(client_key, client.clone());
            }
            client
        };
        let method = reqwest::Method::from_bytes(request.http_method().as_bytes())
            .context("Invalid HTTP method")?;
        let mut headers = HeaderMap::new();
        let generated_values = crate::generated_headers::custom_values(request, vars);
        for p in request
            .headers
            .iter()
            .chain(&generated_values)
            .filter(|p| p.enabled && !p.key.is_empty())
        {
            let name = resolve(&p.key, vars);
            if !request.http_header_is_sent(&name) {
                continue;
            }
            headers.append(
                HeaderName::from_bytes(name.as_bytes()).context("Invalid header name")?,
                HeaderValue::from_str(&resolve(&p.value, &vars)).context("Invalid header value")?,
            );
        }
        if request.auth.kind == "apikey" && !request.auth.in_query {
            headers.insert(
                HeaderName::from_bytes(resolve(&request.auth.key, &vars).as_bytes())?,
                HeaderValue::from_str(&resolve(&request.auth.value, &vars))?,
            );
        }
        let mut req = client.request(method, url.clone()).headers(headers.clone());
        match request.auth.kind.as_str() {
            "basic" => {
                req = req.basic_auth(
                    resolve(&request.auth.username, &vars),
                    Some(resolve(&request.auth.password, &vars)),
                )
            }
            "bearer" => req = req.bearer_auth(resolve(&request.auth.token, &vars)),
            "oauth2" => {
                let mut auth = request.auth.clone();
                auth.fields.insert(
                    "openBrowser".into(),
                    preferences.system_browser_oauth.to_string(),
                );
                let token = self.tokens.token(&auth, &vars, &client).await?;
                let fields = crate::auth::values(&request.auth, &vars);
                if fields
                    .get("tokenPlacement")
                    .is_some_and(|s| s == "queryparams" || s == "query")
                {
                    req = req.query(&[(
                        fields
                            .get("tokenQueryKey")
                            .map(String::as_str)
                            .unwrap_or("access_token"),
                        token,
                    )]);
                } else {
                    let prefix = fields
                        .get("tokenHeaderPrefix")
                        .map(String::as_str)
                        .unwrap_or("Bearer");
                    req = req.header("authorization", format!("{prefix} {token}").trim());
                }
            }
            "digest" | "awsv4" | "oauth1" | "wsse" | "edgegrid" | "ntlm" => {}
            "" | "none" | "apikey" | "inherit" => {}
            other => bail!("Authorization type '{other}' is not implemented in this version"),
        }
        let has_type = headers.contains_key("content-type");
        match request.http_body_kind(vars) {
            BodyKind::None => {}
            BodyKind::Json | BodyKind::Text | BodyKind::Xml | BodyKind::Sparql => {
                let body = resolve(&request.body, &vars);
                let body = if request.body_kind == BodyKind::Json {
                    crate::json_comments::validate(&body)
                        .context("Invalid JSON request body")?
                        .into_owned()
                } else {
                    body
                };
                if !has_type {
                    req = req.header(
                        "content-type",
                        match request.body_kind {
                            BodyKind::Json => "application/json",
                            BodyKind::Xml => "application/xml",
                            BodyKind::Sparql => "application/sparql-query",
                            _ => "text/plain",
                        },
                    );
                }
                req = req.body(body);
            }
            BodyKind::Form => {
                let data: Vec<_> = request
                    .form
                    .iter()
                    .filter(|p| p.enabled && !p.key.is_empty())
                    .map(|p| (resolve(&p.key, &vars), resolve(&p.value, &vars)))
                    .collect();
                req = req.form(&data);
            }
            BodyKind::Multipart => {
                let mut form = multipart::Form::new();
                for p in request
                    .form
                    .iter()
                    .filter(|p| p.enabled && !p.key.is_empty())
                {
                    let key = resolve(&p.key, &vars);
                    let value = resolve(&p.value, &vars);
                    if let Some(path) = value.strip_prefix('@').filter(|_| p.kind != "text") {
                        let path = file_path(&request, path);
                        let filename = path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                        let bytes = std::fs::read(&path)
                            .with_context(|| format!("Cannot read {}", path.display()))?;
                        form = form.part(key, multipart::Part::bytes(bytes).file_name(filename));
                    } else {
                        form = form.text(key, value);
                    }
                }
                req = req.multipart(form);
            }
            BodyKind::Graphql => {
                let raw = resolve(&request.graphql_variables, &vars);
                let variables: serde_json::Value =
                    crate::json_comments::value(if raw.trim().is_empty() { "{}" } else { &raw })
                        .context("Invalid GraphQL variables JSON")?;
                req=req.json(&serde_json::json!({"query":resolve(&request.body,&vars),"variables":variables}));
            }
            BodyKind::Binary => {
                let path = file_path(&request, &resolve(&request.body, &vars));
                if !has_type {
                    req = req.header("content-type", "application/octet-stream");
                }
                req = req.body(
                    std::fs::read(&path)
                        .with_context(|| format!("Cannot read binary file: {}", path.display()))?,
                );
            }
        }
        let mut timeline = vec![
            format!("{} {}", request.http_method(), url),
            format!("TLS certificate validation: {}", request.verify_tls),
            format!("Request prepared in {} ms", start.elapsed().as_millis()),
        ];
        let mut prepared = req.build()?;
        // Multipart builders may replace Content-Type; explicit values take priority.
        if let Some(value) = headers.get("content-type") {
            prepared.headers_mut().insert("content-type", value.clone());
        }
        for (key, value) in crate::generated_headers::defaults(request) {
            let key = HeaderName::from_bytes(key.as_bytes())?;
            if !prepared.headers().contains_key(&key)
                && !omitted_headers.iter().any(|name| name == key.as_str())
            {
                prepared
                    .headers_mut()
                    .insert(key, HeaderValue::from_static(value));
            }
        }
        if prepared.body().is_some_and(|b| b.as_bytes().is_none()) {
            use http_body_util::BodyExt;
            let body = prepared.body_mut().take().unwrap();
            let bytes = body.collect().await?.to_bytes();
            *prepared.body_mut() = Some(bytes.into());
        }
        // Auth-generated headers must follow the same payload policy as user headers.
        for name in [
            "content-length",
            "transfer-encoding",
            "content-type",
            "content-encoding",
        ] {
            if !request.http_header_is_sent(name) {
                prepared.headers_mut().remove(name);
            }
        }
        // Sign the same headers that will go on the wire. The transport repeats
        // suppression after middleware that may reintroduce library defaults.
        for name in &omitted_headers {
            prepared.headers_mut().remove(name);
        }
        match request.auth.kind.as_str() {
            "wsse" => crate::auth::sign_wsse(&mut prepared, &request.auth, &vars)?,
            _ => {}
        }
        let mut response = send_redirects(&client, prepared, request, vars, &mut timeline).await?;
        let status = response.status();
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("<binary>").to_string()))
            .collect();
        let response_url = response.url().to_string();
        timeline.push(format!(
            "Response headers received in {} ms · {:?}",
            start.elapsed().as_millis(),
            response.version()
        ));
        let mut bytes = vec![];
        while let Some(chunk) = response.chunk().await? {
            if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
                bail!("Response exceeds the 32 MiB limit");
            }
            bytes.extend_from_slice(&chunk);
        }
        timeline.push(format!(
            "Received {} bytes in {} ms",
            bytes.len(),
            start.elapsed().as_millis()
        ));
        let charset = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .and_then(|(_, v)| {
                v.split(';').skip(1).find_map(|part| {
                    let (key, value) = part.trim().split_once('=')?;
                    key.eq_ignore_ascii_case("charset")
                        .then(|| value.trim().trim_matches(['\'', '"']))
                })
            });
        let encoding = charset
            .and_then(|label| encoding_rs::Encoding::for_label(label.as_bytes()))
            .unwrap_or(encoding_rs::UTF_8);
        let (body, _, _) = encoding.decode(&bytes);
        let body = body.into_owned();
        Ok(Reply {
            status: status.as_u16(),
            reason: status.canonical_reason().unwrap_or("").into(),
            elapsed_ms: start.elapsed().as_millis(),
            size: bytes.len(),
            headers,
            body,
            bytes,
            url: response_url,
            timeline,
            logs: vec![],
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod preference_cache_tests {
    use super::*;
    #[tokio::test]
    async fn ssl_session_cache_toggle_and_clear_control_the_connection_pool() {
        let engine = Engine::default();
        let mut p = Preferences::default();
        p.proxy_settings.mode = "off".into();
        for enabled in [true, false, true] {
            p.cache.ssl = enabled;
            engine.configure(p.clone());
            let (url, server) =
                crate::preference_tests::http_fixture(serde_json::json!({"ok":true}));
            let reply = engine
                .execute_scoped(
                    Request {
                        url,
                        ..Default::default()
                    },
                    vec![],
                    vec![],
                    BTreeMap::new(),
                )
                .await;
            assert_eq!(reply.status, 200, "{}", reply.error);
            server.join().unwrap();
            assert_eq!(!engine.clients.lock().unwrap().is_empty(), enabled);
        }
        engine.clear_tls_cache();
        assert!(engine.clients.lock().unwrap().is_empty());
    }
}

pub fn request_url(request: &Request, vars: &BTreeMap<String, String>) -> Result<url::Url> {
    if let Some(url) = crate::query::legacy_url(request, vars) {
        return url;
    }
    let mut raw = resolve(&request.url, vars);
    if raw.contains("{{") {
        bail!(
            "The URL contains unresolved variables. Select an environment or define them in Vars."
        );
    }
    if !raw.contains("://") {
        raw = format!("http://{raw}");
    }
    let (base, query) = raw.split_once('?').unwrap_or((&raw, ""));
    let mut base = base.to_owned();
    for p in request
        .path_params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
    {
        let marker = format!("/:{}", p.key);
        let replacement = format!("/{}", crate::query::component(&resolve(&p.value, vars)));
        let mut offset = 0;
        while let Some(index) = base[offset..].find(&marker) {
            let start = offset + index;
            let end = start + marker.len();
            if end == base.len() || base[end..].starts_with('/') || base[end..].starts_with('#') {
                base.replace_range(start..end, &replacement);
                offset = start + replacement.len();
            } else {
                offset = end;
            }
        }
    }
    let (query, fragment) = if request.encode_url {
        (query, "")
    } else {
        query.split_once('#').unwrap_or((query, ""))
    };
    let keys = request
        .params
        .iter()
        .filter(|p| !p.key.is_empty())
        .map(|p| resolve(&p.key, vars))
        .collect::<Vec<_>>();
    let mut parts = query
        .split('&')
        .filter(|p| !p.is_empty())
        .filter(|p| {
            !keys
                .iter()
                .any(|key| key == p.split('=').next().unwrap_or(p))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for p in request
        .params
        .iter()
        .filter(|p| p.enabled && !p.key.is_empty())
    {
        let key = resolve(&p.key, vars);
        let value = resolve(&p.value, vars);
        if value.is_empty() && query.split('&').any(|part| part == key) {
            parts.push(key);
        } else {
            parts.push(format!("{key}={value}"));
        }
    }
    if request.auth.kind == "apikey" && request.auth.in_query {
        parts.push(format!(
            "{}={}",
            resolve(&request.auth.key, vars),
            resolve(&request.auth.value, vars)
        ));
    }
    if !parts.is_empty() {
        base.push('?');
        base.push_str(&parts.join("&"));
    }
    if !fragment.is_empty() {
        base.push('#');
        base.push_str(fragment);
    }
    let raw = if request.encode_url {
        crate::query::encode_url(&base)
    } else {
        base
    };
    let url = url::Url::parse(&raw).context("Enter a valid HTTP or HTTPS URL")?;
    if !["http", "https"].contains(&url.scheme()) {
        bail!(
            "This request uses an unsupported protocol: {}",
            url.scheme()
        );
    }
    Ok(url)
}

pub fn curl(r: &Request, vars: &BTreeMap<String, String>) -> String {
    fn quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
    if r.protocol == "grpc" {
        let url = resolve(&r.url, vars);
        let plain = !r.grpc_tls_enabled(vars);
        let endpoint = url
            .split_once("://")
            .map(|(_, s)| s)
            .unwrap_or(&url)
            .trim_end_matches('/');
        let mut out = format!("grpcurl{}", if plain { " -plaintext" } else { "" });
        if r.portable["grpc_emit_defaults"] == true {
            out.push_str(" -emit-defaults");
        }
        if let Some(n) = r.portable["grpc_max_response_mb"].as_u64() {
            out.push_str(&format!(
                " -max-msg-sz {}",
                if n == 0 { 2147483647 } else { n * 1048576 }
            ));
        }
        if let Some(authority) = r.portable["tls_server_name"]
            .as_str()
            .filter(|s| !s.is_empty())
        {
            out.push_str(&format!(" -authority {}", quote(authority)));
        }
        for file in &r.proto_files {
            let flag = if std::path::Path::new(file)
                .extension()
                .is_some_and(|x| x == "protoset" || x == "pb" || x == "desc")
            {
                "-protoset"
            } else {
                "-proto"
            };
            out.push_str(&format!(" {flag} {}", quote(file)));
        }
        for path in &r.proto_import_paths {
            out.push_str(&format!(" -import-path {}", quote(path)));
        }
        if !r.verify_tls {
            out.push_str(" -insecure");
        }
        if r.timeout > 0 && !r.timeout_inherit {
            out.push_str(&format!(" -max-time {}", r.timeout as f64 / 1000.));
        }
        for (flag, value) in [
            ("-cacert", &r.ca_certificate),
            ("-cert", &r.client_certificate),
            ("-key", &r.client_key),
        ] {
            if !value.is_empty() {
                out.push_str(&format!(" {flag} {}", quote(value)));
            }
        }
        if r.auth.kind == "bearer" {
            out.push_str(&format!(
                " -H {}",
                quote(&format!(
                    "authorization: Bearer {}",
                    resolve(&r.auth.token, vars)
                ))
            ));
        }
        for h in r.headers.iter().filter(|h| h.enabled && !h.key.is_empty()) {
            out.push_str(&format!(
                " -H {}",
                quote(&format!(
                    "{}: {}",
                    resolve(&h.key, vars),
                    resolve(&h.value, vars)
                ))
            ));
        }
        out.push_str(&format!(
            " -d {} {} {}",
            quote(
                &crate::json_comments::normalize(&resolve(&r.body, vars))
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| resolve(&r.body, vars))
            ),
            quote(endpoint),
            quote(&r.grpc_method)
        ));
        return out;
    }
    let mut out = format!(
        "curl --request {} {}",
        r.http_method(),
        quote(
            &request_url(r, vars)
                .map(|u| u.to_string())
                .unwrap_or_else(|_| resolve(&r.url, vars))
        )
    );
    let generated_values = crate::generated_headers::custom_values(r, vars);
    let omitted_headers = crate::generated_headers::omitted(r, vars);
    let effective_headers: Vec<_> = r
        .headers
        .iter()
        .chain(&generated_values)
        .filter(|h| h.enabled && !h.key.is_empty())
        .collect();
    for h in &effective_headers {
        if !r.http_header_is_sent(&resolve(&h.key, vars)) {
            continue;
        }
        let name = resolve(&h.key, vars);
        let value = resolve(&h.value, vars);
        let header = if value.is_empty() {
            format!("{name};")
        } else {
            format!("{name}: {value}")
        };
        out.push_str(&format!(" \\\n  --header {}", quote(&header)));
    }
    for (name, value) in crate::generated_headers::defaults(r) {
        if !effective_headers
            .iter()
            .any(|p| resolve(&p.key, vars).eq_ignore_ascii_case(name))
            && !omitted_headers
                .iter()
                .any(|key| key.eq_ignore_ascii_case(name))
        {
            out.push_str(&format!(" --header {}", quote(&format!("{name}: {value}"))));
        }
    }
    for name in &omitted_headers {
        // An empty -H value prevents curl from reintroducing its own defaults.
        out.push_str(&format!(" --header {}", quote(&format!("{name}:"))));
    }
    out.push_str(" --compressed");
    if r.auth.kind == "bearer" {
        out.push_str(&format!(
            " \\\n  --header {}",
            quote(&format!(
                "Authorization: Bearer {}",
                resolve(&r.auth.token, vars)
            ))
        ));
    }
    if matches!(r.auth.kind.as_str(), "basic" | "digest" | "ntlm") {
        if r.auth.kind == "digest" {
            out.push_str(" \\\n  --digest");
        }
        if r.auth.kind == "ntlm" {
            out.push_str(" \\\n  --ntlm");
        }
        out.push_str(&format!(
            " \\\n  --user {}",
            quote(&format!(
                "{}:{}",
                resolve(&r.auth.username, vars),
                resolve(&r.auth.password, vars)
            ))
        ));
    }
    if r.auth.kind == "apikey" && !r.auth.in_query {
        out.push_str(&format!(
            " \\\n  --header {}",
            quote(&format!(
                "{}: {}",
                resolve(&r.auth.key, vars),
                resolve(&r.auth.value, vars)
            ))
        ));
    }
    if r.follow_redirects {
        out.push_str(&format!(
            " \\\n  --location --max-redirs {}",
            r.max_redirects
        ));
    }
    if r.forward_auth && r.follow_redirects {
        out.push_str(" \\\n  --location-trusted");
    }
    if r.portable["http_follow_original_method"] == true {
        out.push_str(" \\\n  --post301 --post302 --post303");
    }
    match r.portable["http_version"].as_str() {
        Some("HTTP/1.x") => out.push_str(" \\\n  --http1.1"),
        Some("HTTP/2") => out.push_str(" \\\n  --http2-prior-knowledge"),
        _ => {}
    }
    if !r.verify_tls {
        out.push_str(" \\\n  --insecure");
    }
    if r.timeout > 0 && !r.timeout_inherit {
        out.push_str(&format!(" \\\n  --max-time {}", r.timeout as f64 / 1000.));
    }
    for (flag, value) in [
        ("--proxy", &r.proxy),
        ("--cert", &r.client_certificate),
        ("--key", &r.client_key),
        ("--cacert", &r.ca_certificate),
    ] {
        if !value.is_empty() {
            out.push_str(&format!(" \\\n  {flag} {}", quote(&resolve(value, vars))));
        }
    }
    let body_kind = r.http_body_kind(vars);
    let content_type = match body_kind {
        BodyKind::Json | BodyKind::Graphql => "application/json",
        BodyKind::Xml => "application/xml",
        BodyKind::Sparql => "application/sparql-query",
        BodyKind::Text => "text/plain",
        BodyKind::Form => "application/x-www-form-urlencoded",
        _ => "",
    };
    if !content_type.is_empty()
        && !effective_headers
            .iter()
            .any(|p| resolve(&p.key, vars).eq_ignore_ascii_case("content-type"))
        && !omitted_headers.iter().any(|key| key == "content-type")
    {
        out.push_str(&format!(
            " \\\n  --header {}",
            quote(&format!("Content-Type: {content_type}"))
        ));
    }
    match body_kind {
        BodyKind::None => {}
        BodyKind::Form => {
            for p in r.form.iter().filter(|p| p.enabled && !p.key.is_empty()) {
                out.push_str(&format!(
                    " \\\n  --data-urlencode {}",
                    quote(&format!(
                        "{}={}",
                        resolve(&p.key, vars),
                        resolve(&p.value, vars)
                    ))
                ));
            }
        }
        BodyKind::Multipart => {
            for p in r.form.iter().filter(|p| p.enabled && !p.key.is_empty()) {
                let value = resolve(&p.value, vars);
                out.push_str(&format!(
                    " \\\n  {} {}",
                    if value.starts_with('@') && p.kind != "text" {
                        "--form"
                    } else {
                        "--form-string"
                    },
                    quote(&format!("{}={value}", resolve(&p.key, vars)))
                ));
            }
        }
        BodyKind::Binary => out.push_str(&format!(
            " \\\n  --data-binary {}",
            quote(&format!("@{}", resolve(&r.body, vars)))
        )),
        BodyKind::Graphql => {
            let variables = crate::json_comments::value(&resolve(&r.graphql_variables, vars))
                .unwrap_or_else(|_| serde_json::json!({}));
            let body = serde_json::json!({"query":resolve(&r.body,vars),"variables":variables})
                .to_string();
            out.push_str(&format!(" \\\n  --data-raw {}", quote(&body)));
        }
        BodyKind::Json => out.push_str(&format!(
            " \\\n  --data-raw {}",
            quote(
                &crate::json_comments::normalize(&resolve(&r.body, vars))
                    .map(|s| s.into_owned())
                    .unwrap_or_else(|_| resolve(&r.body, vars))
            )
        )),
        _ => out.push_str(&format!(
            " \\\n  --data-raw {}",
            quote(&resolve(&r.body, vars))
        )),
    }

    out
}
pub fn file_path(r: &Request, path: &str) -> std::path::PathBuf {
    let p = std::path::PathBuf::from(path);
    if p.is_absolute() {
        p
    } else {
        r.path
            .as_ref()
            .and_then(|p| p.parent())
            .unwrap_or(std::path::Path::new("."))
            .join(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[test]
    fn disabled_query_is_removed_and_curl_uses_effective_url() {
        let mut off = Pair::new("skip", "value");
        off.enabled = false;
        let r = Request {
            url: "http://localhost/items?skip=value&q=old".into(),
            params: vec![off, Pair::new("q", "{{query}}")],
            ..Default::default()
        };
        let vars = BTreeMap::from([("query".into(), "hello world".into())]);
        let url = request_url(&r, &vars).unwrap();
        assert_eq!(url.query(), Some("q=hello%20world"));
        assert!(curl(&r, &vars).contains("q=hello%20world"));
        assert!(!curl(&r, &vars).contains("skip="));
    }
    #[tokio::test]
    async fn redirects_forward_session_cookies() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let mut buf = [0; 4096];
            first.read(&mut buf).await.unwrap();
            first.write_all(b"HTTP/1.1 302 Found\r\nLocation: /target\r\nSet-Cookie: session=works; Path=/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            drop(first);
            let (mut second, _) = listener.accept().await.unwrap();
            let n = second.read(&mut buf).await.unwrap();
            let raw = String::from_utf8_lossy(&buf[..n]).to_string();
            second
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
            raw
        });
        let reply = Engine::default()
            .execute(
                Request {
                    url: format!("http://{addr}/start"),
                    ..Default::default()
                },
                BTreeMap::new(),
            )
            .await;
        assert_eq!(reply.status, 200);
        assert!(reply.url.ends_with("/target"));
        assert!(
            server
                .await
                .unwrap()
                .to_lowercase()
                .contains("cookie: session=works")
        );
    }
    #[tokio::test]
    async fn sends_query_auth_json_and_runs_scripts() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0; 8192];
            let n = stream.read(&mut bytes).await.unwrap();
            let request = String::from_utf8_lossy(&bytes[..n]).into_owned();
            let body = r#"{"id":42}"#;
            stream.write_all(format!("HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nSet-Cookie: session=hello; Path=/\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            request
        });
        let r = Request {
            method: "POST".into(),
            url: format!("http://{address}/items?q=old"),
            params: vec![Pair::new("q", "hello world")],
            body_kind: BodyKind::Json,
            body: "{\"name\":\"{{name}}\"}".into(),
            auth: Auth {
                kind: "bearer".into(),
                token: "abc".into(),
                ..Default::default()
            },
            pre_script: "req.setHeader('X-Pre','yes');".into(),
            post_script: "bru.setVar('id',res.body.id)".into(),
            tests: "test('created',()=>expect(res.status).to.equal(201));".into(),
            response_vars: vec![Pair::new("extracted", "res('id')")],
            assertions: vec![
                Pair::new("res.status", "between 200, 299"),
                Pair::new("res('id')", "gte 40"),
                Pair::new("res.body", "isJson"),
            ],
            ..Default::default()
        };
        let reply = Engine::default()
            .execute(r, BTreeMap::from([("name".into(), "Rust".into())]))
            .await;
        assert_eq!(reply.error, "");
        assert_eq!(reply.status, 201);
        assert!(reply.tests[0].passed);
        assert_eq!(reply.runtime_vars["id"], "42");
        assert_eq!(reply.runtime_vars["extracted"], "42");
        assert_eq!(reply.tests.len(), 4);
        assert!(reply.tests.iter().all(|t| t.passed), "{:?}", reply.tests);
        let raw = server.await.unwrap().to_lowercase();
        assert!(raw.contains("q=hello%20world"));
        assert!(!raw.contains("q=old"));
        assert!(raw.contains("authorization: bearer abc"));
        assert!(raw.contains("x-pre: yes"));
        assert!(raw.contains("\"name\":\"rust\""));
    }
    #[tokio::test]
    async fn unsupported_auth_fails_without_sending() {
        let reply = Engine::default()
            .execute(
                Request {
                    url: "http://127.0.0.1:1".into(),
                    auth: Auth {
                        kind: "unknown-auth".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                BTreeMap::new(),
            )
            .await;
        assert!(reply.error.contains("not implemented"));
    }
}
