//! Private, versioned GUI IPC. No reports, request files or response truncation.
//! One owner per process; closing stdin or terminating it releases its connections.
use anyhow::{Context, Result, bail};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::time::{Duration, Instant};

#[derive(Clone)]
struct GuiConnection {
    channel: tonic::transport::Channel,
    session: crate::transport::Session,
    created: Instant,
    used: Instant,
}

#[derive(Default)]
struct GuiCache(HashMap<String, GuiConnection>);

impl GuiCache {
    fn retain_fresh(&mut self) {
        self.0
            .retain(|_, value| value.created.elapsed() < Duration::from_secs(60));
        let bytes: usize = self
            .0
            .values()
            .map(|entry| {
                entry
                    .session
                    .descriptors
                    .lock()
                    .unwrap()
                    .values()
                    .map(|pool| {
                        pool.file_descriptor_protos()
                            .map(prost::Message::encoded_len)
                            .sum::<usize>()
                    })
                    .sum::<usize>()
            })
            .sum();
        if bytes > 8 * 1024 * 1024 {
            self.0.clear();
        }
    }
    fn insert(&mut self, key: String, value: GuiConnection) {
        if self.0.len() >= 8 {
            if let Some(oldest) = self
                .0
                .iter()
                .min_by_key(|(_, v)| v.used)
                .map(|(k, _)| k.clone())
            {
                self.0.remove(&oldest);
            }
        }
        self.0.insert(key, value);
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    protocol: u32,
    kind: String,
    url: String,
    #[serde(default)]
    method: String,
    #[serde(default)]
    headers: Vec<(String, String)>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    payload: Value,
    #[serde(default)]
    timeout_ms: u64,
    #[serde(default)]
    limit: usize,
    #[serde(default)]
    http_version: String,
    #[serde(default)]
    verify: bool,
    #[serde(default)]
    strict: bool,
    #[serde(default)]
    skip_redirect_body: bool,
    #[serde(default)]
    transport: crate::transport::TransportConfig,
    #[serde(default)]
    origin: Option<String>,
    #[serde(default)]
    tls_server_name: Option<String>,
    #[serde(default)]
    raw_headers: bool,
    #[serde(default)]
    grpc_cache: bool,
}

fn emit(value: Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &value)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}

fn chunks(bytes: &[u8]) -> Result<()> {
    for part in bytes.chunks(48 * 1024) {
        emit(json!({"type":"chunk", "data": STANDARD.encode(part)}))?;
    }
    Ok(())
}

pub async fn serve() -> Result<()> {
    emit(json!({"type":"hello", "protocol":1, "version":env!("CARGO_PKG_VERSION")}))?;
    let disconnected = tokio_util::sync::CancellationToken::new();
    let reader_closed = disconnected.clone();
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            let mut line = Vec::new();
            // One pending bounded frame. EOF also cancels an active unlimited RPC.
            if std::io::Read::take(&mut reader, 512 * 1024 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .is_err()
                || line.is_empty()
            {
                break;
            }
            if line.len() > 512 * 1024 * 1024 {
                let _ = sender.blocking_send(Err(anyhow::anyhow!(
                    "GUI request exceeds 512 MiB IPC limit"
                )));
                return;
            }
            if sender.blocking_send(Ok(line)).is_err() {
                return;
            }
        }
        reader_closed.cancel();
    });
    let mut cache = GuiCache::default();
    while let Some(line) = receiver.recv().await {
        let line = line?;
        let operation = async {
            let input: Input = serde_json::from_slice(&line)?;
            if input.protocol != 1 {
                bail!("unsupported GUI request protocol");
            }
            let deadline = input.timeout_ms;
            let grpc = input.kind == "grpc";
            let operation = execute(input, &mut cache);
            if deadline == 0 {
                operation.await
            } else {
                match tokio::time::timeout(Duration::from_millis(deadline), operation).await {
                    Ok(result) => result,
                    Err(_) if grpc => {
                        Err(tonic::Status::deadline_exceeded("Connection Timeout exceeded.").into())
                    }
                    Err(_) => bail!("Request timeout exceeded"),
                }
            }
        };
        let result = tokio::select! {
            _ = disconnected.cancelled() => return Ok(()),
            result = operation => result,
        };
        if let Err(error) = result {
            // A failed/deadline-expired exchange must not retain stale reflection
            // after a restart. Never retry a possibly side-effecting RPC here.
            cache.0.clear();
            let gui = error.downcast_ref::<crate::grpc::GuiError>();
            let status = gui
                .map(|e| &e.status)
                .or_else(|| error.downcast_ref::<tonic::Status>());
            let code = status.map(|s| format!("{:?}", s.code()));
            let message = status
                .map(|s| s.message().to_owned())
                .unwrap_or_else(|| format!("{error:#}"));
            emit(json!({"type":"error", "message":message, "code":code,
                "headers":gui.map(|e| crate::grpc::gui_metadata(&e.headers)).unwrap_or_default(),
                "trailers":status.map(|s| crate::grpc::gui_metadata(s.metadata())).unwrap_or_default()}))?;
        }
    }
    Ok(())
}

async fn execute(input: Input, cache: &mut GuiCache) -> Result<()> {
    match input.kind.as_str() {
        "http" => http(input).await,
        "grpc" => grpc(input, cache).await,
        _ => bail!("unknown GUI request kind"),
    }
}

async fn http(input: Input) -> Result<()> {
    if input.raw_headers {
        return raw_http(input).await;
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let transport = input.transport.load(std::path::Path::new("."))?;
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .referer(false)
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .http1_title_case_headers()
        .tls_danger_accept_invalid_certs(!input.verify)
        .http1_allow_obsolete_multiline_headers_in_responses(!input.strict)
        .http1_allow_spaces_after_header_name_in_responses(!input.strict);
    if input.http_version == "HTTP/1.x" {
        builder = builder.http1_only();
    }
    if input.http_version == "HTTP/2" && input.url.starts_with("http://") {
        builder = builder.http2_prior_knowledge();
    }
    if let Some(ca) = &transport.ca {
        for cert in reqwest::Certificate::from_pem_bundle(ca)? {
            builder = builder.add_root_certificate(cert);
        }
    }
    if let Some(identity) = &transport.identity {
        let mut pem = identity.0.clone();
        pem.extend_from_slice(b"\n");
        pem.extend_from_slice(&identity.1);
        builder = builder.identity(reqwest::Identity::from_pem(&pem)?);
    }
    if let Some(url) = &transport.config.proxy {
        let mut proxy = reqwest::Proxy::all(url)?;
        if let Some(auth) = &transport.config.proxy_authorization {
            proxy = proxy.custom_http_auth(auth.parse()?);
        }
        builder = builder.proxy(proxy);
    }
    let mut request = builder.build()?.request(input.method.parse()?, &input.url);
    for (name, value) in &input.headers {
        request = request.header(name, value);
    }
    if let Some(body) = &input.body {
        request = request.body(STANDARD.decode(body)?);
    }
    let mut response = request.send().await.map_err(|e| {
        if input.strict && format!("{e:#}").contains("invalid") {
            anyhow::anyhow!("Strict HTTP parser: {}", anyhow::Error::from(e))
        } else {
            e.into()
        }
    })?;
    let status = response.status();
    let version = format!("{:?}", response.version());
    if input.http_version == "HTTP/2" && response.version() != http::Version::HTTP_2 {
        bail!("The server did not negotiate HTTP/2. Select Auto or HTTP/1.x in General.");
    }
    let headers: Vec<_> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.as_bytes()
                    .iter()
                    .map(|b| char::from(*b))
                    .collect::<String>(),
            )
        })
        .collect();
    emit(
        json!({"type":"headers", "status":status.as_u16(), "reason":status.canonical_reason().unwrap_or(""), "headers":headers, "http_version":version}),
    )?;
    let skip = input.skip_redirect_body
        && matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
        && response.headers().contains_key("location");
    let mut size = 0usize;
    let eof_framed = matches!(
        response.version(),
        http::Version::HTTP_10 | http::Version::HTTP_11
    ) && !response.headers().contains_key("content-length")
        && !response.headers().contains_key("transfer-encoding");
    if !skip {
        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(error) => {
                    let error = anyhow::Error::from(error);
                    // Preserve the GUI's HTTP/1 EOF-delimited body semantics.
                    // Never suppress truncation of Content-Length, chunked or H2 bodies.
                    if eof_framed
                        && format!("{error:#}").contains("without sending TLS close_notify")
                    {
                        break;
                    }
                    return Err(error);
                }
            };
            size = size
                .checked_add(chunk.len())
                .context("response size overflow")?;
            if input.limit > 0 && size > input.limit {
                bail!("Max response size exceeded. Adjust General > Max response size.");
            }
            chunks(&chunk)?;
        }
    }
    emit(json!({"type":"complete", "size":size}))
}

async fn grpc(input: Input, cache: &mut GuiCache) -> Result<()> {
    let transport = input.transport.load(std::path::Path::new("."))?;
    // Hash actual certificate/descriptor bytes as well as configuration: files
    // may be replaced at the same path. Metadata stays isolated, including
    // duplicate and binary entries. Keys are never written to logs or IPC.
    let key = crate::package::hash_bytes(&serde_json::to_vec(&json!({
        "url": input.url, "origin": input.origin, "server_name": input.tls_server_name,
        "headers": input.headers, "config": transport.config,
        "ca": transport.ca.as_deref(), "identity": transport.identity.as_deref(),
        "descriptors": transport.descriptors.as_ref().map(|p| p.encode_to_vec()),
    }))?);
    cache.retain_fresh();
    let connection = if input.grpc_cache {
        cache.0.get_mut(&key).map(|entry| {
            entry.used = Instant::now();
            entry.clone()
        })
    } else {
        None
    };
    let connection = match connection {
        Some(connection) => connection,
        None => {
            let connection = GuiConnection {
                channel: transport
                    .grpc_channel_named(&input.url, input.tls_server_name.as_deref())
                    .await
                    .map_err(|e| tonic::Status::unavailable(format!("{e:#}")))?,
                session: crate::transport::Session::default(),
                created: Instant::now(),
                used: Instant::now(),
            };
            if input.grpc_cache {
                cache.insert(key, connection.clone());
            }
            connection
        }
    };
    let mut client = crate::grpc::DynamicClient::connect_gui(
        connection.channel,
        &input.headers,
        if input.limit == 0 {
            usize::MAX
        } else {
            input.limit
        },
        &transport,
        &connection.session,
        input.origin,
    )?;
    let method = tokio::time::timeout(Duration::from_secs(10), client.method_any(&input.method))
        .await
        .map_err(|_| {
            tonic::Status::deadline_exceeded(
                "Server reflection did not complete within 10 seconds.",
            )
        })??;
    let reply = client
        .call_gui(&method, &input.payload, input.timeout_ms)
        .await?;
    let body = serde_json::to_vec(&reply.body)?;
    let flatten = |values: crate::http_client::Headers| {
        values
            .into_iter()
            .filter(|(key, _)| {
                !matches!(
                    key.as_str(),
                    "content-type"
                        | "date"
                        | "grpc-status"
                        | "grpc-message"
                        | "grpc-encoding"
                        | "grpc-accept-encoding"
                )
            })
            .flat_map(|(key, values)| {
                values.into_iter().map(move |value| {
                    (
                        key.clone(),
                        if key.ends_with("-bin") {
                            value.strip_prefix("base64:").unwrap_or(&value).to_owned()
                        } else {
                            value
                        },
                    )
                })
            })
            .collect::<Vec<_>>()
    };
    emit(
        json!({"type":"headers", "status":0, "reason":"OK", "headers":flatten(reply.headers), "trailers":flatten(reply.trailers)}),
    )?;
    chunks(&body)?;
    emit(json!({"type":"complete", "size":body.len()}))
}

/// Hyper's wire API has no implicit Accept header. TLS/proxy on this uncommon
/// route is supplied by the GUI's existing scoped OpenSSL/SOCKS relay.
async fn raw_http(input: Input) -> Result<()> {
    use http_body_util::{BodyExt, Full};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    let url = url::Url::parse(&input.url)?;
    if url.scheme() != "http" {
        bail!("raw HTTP route requires a local/plain transport");
    }
    let socket = tokio::net::TcpStream::connect((
        url.host_str().context("missing host")?,
        url.port_or_known_default().unwrap(),
    ))
    .await?;
    let io = TokioIo::new(socket);
    let h2 = input.http_version == "HTTP/2";
    let uri = if h2 {
        let origin = input.origin.as_deref().unwrap_or(&input.url);
        let origin = url::Url::parse(origin)?;
        let authority = input
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("host"))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| {
                origin[url::Position::BeforeHost..url::Position::AfterPort].to_owned()
            });
        format!(
            "{}://{}{}{}",
            origin.scheme(),
            authority,
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    } else {
        format!(
            "{}{}",
            url.path(),
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    };
    let mut request = http::Request::builder()
        .method(input.method.as_str())
        .uri(uri);
    for (key, value) in &input.headers {
        if h2
            && matches!(
                key.to_ascii_lowercase().as_str(),
                "host"
                    | "connection"
                    | "keep-alive"
                    | "proxy-connection"
                    | "upgrade"
                    | "transfer-encoding"
            )
        {
            continue;
        }
        request = request.header(key, value);
    }
    let body = input
        .body
        .as_deref()
        .map(|s| STANDARD.decode(s))
        .transpose()?
        .unwrap_or_default();
    let request = request.body(Full::new(bytes::Bytes::from(body)))?;
    let response = if h2 {
        let (mut sender, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), io).await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        sender.send_request(request).await?
    } else {
        let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
            .title_case_headers(true)
            .allow_obsolete_multiline_headers_in_responses(!input.strict)
            .allow_spaces_after_header_name_in_responses(!input.strict)
            .handshake(io)
            .await?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        sender
            .send_request(request)
            .await
            .map_err(|e| anyhow::anyhow!("HTTP parser: {e:#}"))?
    };
    let status = response.status();
    let headers: Vec<_> = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                v.as_bytes()
                    .iter()
                    .map(|b| char::from(*b))
                    .collect::<String>(),
            )
        })
        .collect();
    emit(
        json!({"type":"headers", "status":status.as_u16(), "reason":status.canonical_reason().unwrap_or(""),
        "headers":headers,"http_version":format!("{:?}",response.version())}),
    )?;
    let skip = input.skip_redirect_body
        && matches!(status.as_u16(), 301 | 302 | 303 | 307 | 308)
        && response.headers().contains_key("location");
    let mut body = response.into_body();
    let mut size = 0usize;
    if !skip {
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame?.into_data() {
                size = size
                    .checked_add(data.len())
                    .context("response size overflow")?;
                if input.limit > 0 && size > input.limit {
                    bail!("Max response size exceeded. Adjust General > Max response size.");
                }
                chunks(&data)?;
            }
        }
    }
    emit(json!({"type":"complete","size":size}))
}
