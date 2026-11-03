use crate::{runtime::Variables, spec, transport::Transport};
use anyhow::{Context, Result, bail};
use base64::Engine;
use bytes::Bytes;
use reqwest::cookie::CookieStore;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

pub type Headers = BTreeMap<String, Vec<String>>;
pub type Assets = Arc<BTreeMap<String, Bytes>>;

fn get_method() -> String {
    "GET".into()
}
fn request_limit() -> usize {
    8 * 1024 * 1024
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestSpec {
    #[serde(default = "get_method")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub query: Vec<Field>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub body: Body,
    #[serde(default)]
    pub auth: Option<Auth>,
    #[serde(default)]
    pub redirects: Redirects,
    #[serde(default = "yes")]
    pub cookies: bool,
    #[serde(default = "request_limit")]
    pub max_request_bytes: usize,
}

impl RequestSpec {
    pub fn validate(&self) -> Result<()> {
        self.method
            .parse::<reqwest::Method>()
            .context("invalid HTTP method")?;
        if self.url.is_empty()
            || self.url.len() > 16 * 1024
            || self.redirects.max > 20
            || !(1024..=64 * 1024 * 1024).contains(&self.max_request_bytes)
            || self.query.len() > 256
            || self.headers.len() > 128
        {
            bail!("HTTP request exceeds configuration limits");
        }
        for (key, value) in &self.headers {
            let key = reqwest::header::HeaderName::from_bytes(key.as_bytes())
                .context("invalid HTTP header name")?;
            if matches!(
                key.as_str(),
                "host" | "content-length" | "transfer-encoding" | "proxy-authorization"
            ) {
                bail!("transport-managed HTTP header cannot be overridden");
            }
            if !value.contains("${") {
                value
                    .parse::<reqwest::header::HeaderValue>()
                    .context("invalid HTTP header value")?;
            }
        }
        if matches!(&self.body,Body::Form {fields} if fields.len()>256)
            || matches!(&self.body,Body::Multipart {parts} if parts.len()>128)
        {
            bail!("HTTP form/multipart field limit exceeded");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Field {
    pub name: String,
    pub value: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Auth {
    Basic { username: String, password: String },
    Bearer { token: String },
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Part {
    Text {
        name: String,
        value: String,
    },
    File {
        name: String,
        path: String,
        #[serde(default)]
        filename: Option<String>,
        #[serde(default)]
        content_type: Option<String>,
    },
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Body {
    #[default]
    Empty,
    Json {
        value: Value,
    },
    Text {
        value: String,
        #[serde(default)]
        content_type: Option<String>,
    },
    Form {
        fields: Vec<Field>,
    },
    Binary {
        path: String,
        #[serde(default)]
        content_type: Option<String>,
    },
    Multipart {
        parts: Vec<Part>,
    },
}

impl Body {
    pub fn paths(&self) -> Vec<&str> {
        match self {
            Self::Binary { path, .. } => vec![path],
            Self::Multipart { parts } => parts
                .iter()
                .filter_map(|p| {
                    if let Part::File { path, .. } = p {
                        Some(path.as_str())
                    } else {
                        None
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Redirects {
    pub max: usize,
    pub allow_cross_origin: bool,
}
impl Default for Redirects {
    fn default() -> Self {
        Self {
            max: 10,
            allow_cross_origin: false,
        }
    }
}

pub struct Reply {
    pub body: Value,
    pub status: u16,
    pub headers: Headers,
    pub duration_ms: f64,
    pub bytes: usize,
    pub url: String,
}

pub struct HttpClient {
    pub client: reqwest::Client,
    cookies: reqwest::cookie::Jar,
    cookie_budget: std::sync::atomic::AtomicUsize,
    allow_plaintext_remote: bool,
    origins: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
}

impl HttpClient {
    pub fn new(transport: &Transport, session: &crate::transport::Session) -> Result<Self> {
        Ok(Self {
            client: session.http(transport)?,
            origins: session.http_origins.clone(),
            cookies: reqwest::cookie::Jar::default(),
            cookie_budget: std::sync::atomic::AtomicUsize::new(0),
            allow_plaintext_remote: transport.config.allow_plaintext_remote,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &self,
        endpoint: &str,
        method: &str,
        url: &str,
        query: &[Field],
        headers: &BTreeMap<String, String>,
        body: &Body,
        auth: Option<&Auth>,
        redirects: &Redirects,
        vars: &Variables,
        assets: &Assets,
        timeout_ms: u64,
        response_limit: usize,
        request_limit: usize,
        use_cookies: bool,
    ) -> Result<Reply> {
        let started = Instant::now();
        let target = spec::resolve_text(url, vars)?;
        if target.len() > 16 * 1024 {
            bail!("resolved HTTP URL exceeds 16 KiB");
        }
        let base = url::Url::parse(endpoint).context("invalid HTTP base URL")?;
        let mut target = base.join(&target).context("invalid HTTP URL")?;
        crate::transport::url_allowed(target.as_str(), self.allow_plaintext_remote)?;
        if !query.is_empty() {
            for field in query {
                let value = spec::resolve(&field.value, vars)?;
                let value = scalar(&value)?;
                if field.name.len() + value.len() > 16 * 1024 {
                    bail!("HTTP query field exceeds 16 KiB");
                }
                target.query_pairs_mut().append_pair(&field.name, &value);
                if target.as_str().len() > 16 * 1024 {
                    bail!("HTTP URL with query exceeds 16 KiB");
                }
            }
        }
        let mut method: reqwest::Method = method.parse().context("invalid HTTP method")?;
        let mut base_headers = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let key = reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
            if matches!(
                key.as_str(),
                "host" | "content-length" | "transfer-encoding" | "proxy-authorization"
            ) {
                bail!("transport-managed HTTP header cannot be overridden");
            }
            base_headers.insert(key, spec::resolve_text(value, vars)?.parse()?);
        }
        if let Some(auth) = auth {
            if base_headers.contains_key(reqwest::header::AUTHORIZATION) {
                bail!("choose auth or an authorization header");
            }
            let value = match auth {
                Auth::Basic { username, password } => {
                    let pair = format!(
                        "{}:{}",
                        spec::resolve_text(username, vars)?,
                        spec::resolve_text(password, vars)?
                    );
                    format!(
                        "Basic {}",
                        base64::engine::general_purpose::STANDARD.encode(pair)
                    )
                }
                Auth::Bearer { token } => format!("Bearer {}", spec::resolve_text(token, vars)?),
            };
            base_headers.insert(reqwest::header::AUTHORIZATION, value.parse()?);
        }
        let mut include_body = true;
        if base_headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
            .sum::<usize>()
            > 64 * 1024
        {
            bail!("HTTP request headers exceed 64 KiB");
        }
        // Redirects are explicit so credentials and custom headers cannot cross
        // origins accidentally, and multipart/file bodies can be replayed safely.
        for hop in 0..=redirects.max {
            {
                let mut origins = self.origins.lock().unwrap();
                let origin = target.origin().ascii_serialization();
                if !origins.contains(&origin) && origins.len() >= 64 {
                    bail!("HTTP connection-pool origin limit exceeded");
                }
                origins.insert(origin);
            }
            let mut request = self
                .client
                .request(method.clone(), target.clone())
                .headers(base_headers.clone())
                .timeout(Duration::from_millis(timeout_ms).saturating_sub(started.elapsed()));
            if use_cookies
                && !base_headers.contains_key(reqwest::header::COOKIE)
                && let Some(cookie) = self.cookies.cookies(&target)
            {
                request = request.header(reqwest::header::COOKIE, cookie);
            }
            if include_body {
                request = encode_body(request, body, vars, assets, request_limit)?;
            }
            let mut response = request.send().await.context("HTTP request failed")?;
            let status = response.status();
            if response
                .headers()
                .iter()
                .map(|(k, v)| k.as_str().len() + v.as_bytes().len())
                .sum::<usize>()
                > 64 * 1024
            {
                bail!("HTTP response headers exceed 64 KiB");
            }
            if use_cookies {
                let charge = response
                    .headers()
                    .get_all(reqwest::header::SET_COOKIE)
                    .iter()
                    .map(|v| v.as_bytes().len() + 256)
                    .sum::<usize>();
                if self
                    .cookie_budget
                    .fetch_add(charge, std::sync::atomic::Ordering::Relaxed)
                    .saturating_add(charge)
                    > 256 * 1024
                {
                    bail!("case cookie budget exceeds 256 KiB");
                }
                let mut cookies = response
                    .headers()
                    .get_all(reqwest::header::SET_COOKIE)
                    .iter();
                self.cookies.set_cookies(&mut cookies, &target);
            }
            if redirects.max > 0
                && matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
                && let Some(location) = response.headers().get(reqwest::header::LOCATION)
            {
                if hop == redirects.max {
                    bail!("HTTP redirect limit exceeded");
                }
                let next = target
                    .join(location.to_str()?)
                    .context("invalid redirect location")?;
                crate::transport::url_allowed(next.as_str(), self.allow_plaintext_remote)?;
                if target.scheme() == "https" && next.scheme() != "https" {
                    bail!("HTTPS downgrade redirect refused");
                }
                if target.origin() != next.origin() {
                    if !redirects.allow_cross_origin {
                        bail!("cross-origin redirect requires explicit opt-in");
                    }
                    // Strip ALL user headers, not only standard authorization.
                    base_headers.clear();
                    if include_body
                        && !matches!(method, reqwest::Method::GET | reqwest::Method::HEAD)
                    {
                        bail!("cross-origin redirect with request body refused");
                    }
                }
                if status.as_u16() == 303 && method != reqwest::Method::HEAD
                    || matches!(status.as_u16(), 301 | 302) && method == reqwest::Method::POST
                {
                    method = reqwest::Method::GET;
                    include_body = false;
                    base_headers.remove(reqwest::header::CONTENT_TYPE);
                }
                target = next;
                continue;
            }
            if response
                .content_length()
                .is_some_and(|n| n > response_limit as u64)
            {
                bail!("HTTP response exceeds size limit");
            }
            let response_headers = collect_headers(response.headers());
            let mut bytes = Vec::with_capacity(
                response
                    .content_length()
                    .unwrap_or(0)
                    .min(response_limit as u64)
                    .min(64 * 1024) as usize,
            );
            while let Some(chunk) = response.chunk().await.context("HTTP body read failed")? {
                if chunk.len() > response_limit.saturating_sub(bytes.len()) {
                    bail!("HTTP response exceeds size limit");
                }
                bytes.extend_from_slice(&chunk);
            }
            let size = bytes.len();
            let body = if bytes.is_empty() {
                Value::Null
            } else if let Ok(value) = serde_json::from_slice::<Value>(&bytes) {
                value
            } else if let Ok(text) = std::str::from_utf8(&bytes) {
                Value::String(text.into())
            } else {
                json!({"encoding":"base64","data":base64::engine::general_purpose::STANDARD.encode(&bytes)})
            };
            return Ok(Reply {
                body,
                status: status.as_u16(),
                headers: response_headers,
                duration_ms: started.elapsed().as_secs_f64() * 1000.0,
                bytes: size,
                url: target.to_string(),
            });
        }
        unreachable!("redirect limit returns above")
    }
}

pub fn collect_headers(headers: &reqwest::header::HeaderMap) -> Headers {
    let mut out = Headers::new();
    for (key, value) in headers {
        out.entry(key.as_str().into()).or_default().push(
            value.to_str().map(str::to_owned).unwrap_or_else(|_| {
                format!(
                    "base64:{}",
                    base64::engine::general_purpose::STANDARD.encode(value.as_bytes())
                )
            }),
        );
    }
    out
}

fn scalar(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(_) | Value::Bool(_) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        _ => bail!("query/form values must be JSON scalars"),
    }
}

fn encode_body(
    mut request: reqwest::RequestBuilder,
    body: &Body,
    vars: &Variables,
    assets: &Assets,
    limit: usize,
) -> Result<reqwest::RequestBuilder> {
    let mut bytes_count = 0;
    let mut charge = |size: usize| -> Result<()> {
        bytes_count += size;
        if bytes_count > limit {
            bail!("HTTP request body exceeds size limit");
        }
        Ok(())
    };
    let file = |path: &str| {
        assets
            .get(path)
            .cloned()
            .context("request file is not in the validated package snapshot")
    };
    match body {
        Body::Empty => {}
        Body::Json { value } => {
            let value = spec::resolve(value, vars)?;
            let bytes = serde_json::to_vec(&value)?;
            charge(bytes.len())?;
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes);
        }
        Body::Text {
            value,
            content_type,
        } => {
            let text = spec::resolve_text(value, vars)?;
            charge(text.len())?;
            request = request
                .header(
                    reqwest::header::CONTENT_TYPE,
                    content_type
                        .as_deref()
                        .unwrap_or("text/plain; charset=utf-8"),
                )
                .body(text);
        }
        Body::Form { fields } => {
            let mut bytes = String::new();
            for field in fields {
                let encoded = url::form_urlencoded::Serializer::new(String::new())
                    .append_pair(&field.name, &scalar(&spec::resolve(&field.value, vars)?)?)
                    .finish();
                charge(encoded.len() + usize::from(!bytes.is_empty()))?;
                if !bytes.is_empty() {
                    bytes.push('&');
                }
                bytes.push_str(&encoded);
            }
            request = request
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(bytes);
        }
        Body::Binary { path, content_type } => {
            let bytes = file(path)?;
            charge(bytes.len())?;
            request = request
                .header(
                    reqwest::header::CONTENT_TYPE,
                    content_type
                        .as_deref()
                        .unwrap_or("application/octet-stream"),
                )
                .body(bytes);
        }
        Body::Multipart { parts } => {
            let mut form = reqwest::multipart::Form::new();
            for part in parts {
                match part {
                    Part::Text { name, value } => {
                        let text = spec::resolve_text(value, vars)?;
                        charge(text.len() + name.len() * 3 + 256)?;
                        form = form.text(name.clone(), text);
                    }
                    Part::File {
                        name,
                        path,
                        filename,
                        content_type,
                    } => {
                        let bytes = file(path)?;
                        charge(
                            bytes.len()
                                + name.len() * 3
                                + filename.as_ref().map_or(path.len(), String::len) * 3
                                + content_type.as_ref().map_or(0, String::len)
                                + 1024,
                        )?;
                        let filename = filename
                            .clone()
                            .unwrap_or_else(|| path.rsplit('/').next().unwrap_or("file").into());
                        let mut part = reqwest::multipart::Part::stream_with_length(
                            reqwest::Body::from(bytes.clone()),
                            bytes.len() as u64,
                        )
                        .file_name(filename);
                        if let Some(mime) = content_type {
                            part = part.mime_str(mime)?;
                        }
                        form = form.part(name.clone(), part);
                    }
                }
            }
            request = request.multipart(form);
        }
    }
    Ok(request)
}
