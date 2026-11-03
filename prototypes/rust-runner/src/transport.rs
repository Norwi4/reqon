//! Explicit transport configuration; no implicit system proxy or insecure TLS switch.
use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone, Default, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TransportConfig {
    pub ca_cert: Option<PathBuf>,
    pub client_cert: Option<PathBuf>,
    pub client_key: Option<PathBuf>,
    pub proxy: Option<String>,
    pub proxy_authorization: Option<String>,
    pub allow_plaintext_remote: bool,
    pub grpc_descriptor_set: Option<PathBuf>,
    pub grpc_proto_files: Vec<PathBuf>,
    pub grpc_include_paths: Vec<PathBuf>,
    pub grpc_emit_defaults: bool,
    pub script_http_origins: Vec<String>,
    pub script_http_max_calls: usize,
}

#[derive(Clone, Default)]
pub struct Transport {
    pub config: TransportConfig,
    pub ca: Option<Arc<Vec<u8>>>,
    pub identity: Option<Arc<(Vec<u8>, Vec<u8>)>>,
    pub descriptors: Option<prost_reflect::DescriptorPool>,
    pub sources: Vec<PathBuf>,
}

#[derive(Clone, Default)]
pub struct Session {
    pub http_origins: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    pub http: Arc<std::sync::OnceLock<reqwest::Client>>,
    pub channels:
        Arc<tokio::sync::Mutex<std::collections::HashMap<String, tonic::transport::Channel>>>,
    pub descriptors:
        Arc<std::sync::Mutex<std::collections::HashMap<String, prost_reflect::DescriptorPool>>>,
}

impl Session {
    pub fn http(&self, transport: &Transport) -> Result<reqwest::Client> {
        if let Some(client) = self.http.get() {
            return Ok(client.clone());
        }
        let client = transport.http_client()?;
        let _ = self.http.set(client.clone());
        Ok(client)
    }
    pub async fn channel(
        &self,
        transport: &Transport,
        endpoint: &str,
    ) -> Result<tonic::transport::Channel> {
        if let Some(channel) = self.channels.lock().await.get(endpoint) {
            return Ok(channel.clone());
        }
        if self.channels.lock().await.len() >= 64 {
            bail!("gRPC connection-pool origin limit exceeded");
        }
        let channel = transport.grpc_channel(endpoint).await?;
        self.channels
            .lock()
            .await
            .insert(endpoint.into(), channel.clone());
        Ok(channel)
    }
}

impl TransportConfig {
    pub fn validate(&self) -> Result<()> {
        if self.script_http_origins.len() > 16 || self.script_http_max_calls > 16 {
            bail!("script HTTP policy exceeds origin/call limits");
        }
        for origin in &self.script_http_origins {
            let url = url_allowed(origin, self.allow_plaintext_remote)?;
            if url.origin().ascii_serialization() != *origin {
                bail!(
                    "script HTTP allowlist requires exact origins without path or trailing slash"
                );
            }
        }
        if self.client_cert.is_some() != self.client_key.is_some() {
            bail!("client_cert and client_key must be configured together");
        }
        if self.grpc_descriptor_set.is_some() && !self.grpc_proto_files.is_empty() {
            bail!("choose grpc_descriptor_set or grpc_proto_files, not both");
        }
        if self.grpc_proto_files.len() > 128 || self.grpc_include_paths.len() > 16 {
            bail!("too many protobuf source/include paths");
        }
        if self.proxy_authorization.is_some() && self.proxy.is_none() {
            bail!("proxy_authorization requires an explicit proxy");
        }
        if let Some(proxy) = &self.proxy {
            let url = url::Url::parse(proxy).context("invalid proxy URL")?;
            if url.scheme() != "http"
                || url.host_str().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
            {
                bail!(
                    "proxy must be an HTTP URL without credentials/path/query/fragment; use a secret reference for proxy_authorization"
                );
            }
        }
        Ok(())
    }

    pub fn load(&self, root: &Path) -> Result<Transport> {
        self.validate()?;
        let mut out = Transport {
            config: self.clone(),
            ..Transport::default()
        };
        let mut read = |path: &Path, limit: usize| -> Result<Vec<u8>> {
            let path = root
                .join(path)
                .canonicalize()
                .context("cannot resolve transport asset")?;
            let file = std::fs::File::open(&path).context("cannot read transport asset")?;
            let mut bytes = Vec::new();
            std::io::Read::take(file, limit as u64 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > limit {
                bail!("transport asset exceeds size limit");
            }
            out.sources.push(path);
            Ok(bytes)
        };
        if let Some(path) = &self.ca_cert {
            out.ca = Some(Arc::new(read(path, 1024 * 1024)?));
        }
        if let (Some(cert), Some(key)) = (&self.client_cert, &self.client_key) {
            out.identity = Some(Arc::new((
                read(cert, 1024 * 1024)?,
                read(key, 1024 * 1024)?,
            )));
        }
        if let Some(path) = &self.grpc_descriptor_set {
            let bytes = read(path, 8 * 1024 * 1024)?;
            out.descriptors = Some(
                prost_reflect::DescriptorPool::decode(bytes.as_slice())
                    .context("invalid descriptor set")?,
            );
        } else if !self.grpc_proto_files.is_empty() {
            let files: Vec<_> = self.grpc_proto_files.iter().map(|p| root.join(p)).collect();
            let includes: Vec<_> = if self.grpc_include_paths.is_empty() {
                vec![root.to_owned()]
            } else {
                self.grpc_include_paths
                    .iter()
                    .map(|p| root.join(p))
                    .collect()
            };
            for file in &files {
                if std::fs::metadata(file)?.len() > 1024 * 1024 {
                    bail!("proto source exceeds 1 MiB");
                }
            }
            let (fds, sources) = crate::proto_source::compile(&files, &includes)
                .context("cannot compile protobuf sources")?;
            out.sources.extend(sources);
            if fds.file.len() > 512 {
                bail!("protobuf dependency limit exceeded");
            }
            out.descriptors = Some(prost_reflect::DescriptorPool::from_file_descriptor_set(
                fds,
            )?);
        }
        Ok(out)
    }
}

pub fn url_allowed(raw: &str, allow_plaintext_remote: bool) -> Result<url::Url> {
    let url = url::Url::parse(raw).context("invalid request URL")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("request URL must be HTTP(S) without embedded credentials or fragment");
    }
    let local = matches!(
        url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if url.scheme() == "http" && !local && !allow_plaintext_remote {
        bail!("remote plaintext HTTP requires explicit allow_plaintext_remote; prefer HTTPS");
    }
    Ok(url)
}

impl Transport {
    pub fn http_client(&self) -> Result<reqwest::Client> {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .referer(false)
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(8)
            .pool_idle_timeout(Duration::from_secs(60))
            .tcp_nodelay(true)
            .user_agent(concat!("ReqonRunner/", env!("CARGO_PKG_VERSION")));
        if let Some(ca) = &self.ca {
            for cert in reqwest::Certificate::from_pem_bundle(ca)? {
                client = client.add_root_certificate(cert);
            }
        }
        if let Some(identity) = &self.identity {
            let mut pem = identity.0.clone();
            pem.extend_from_slice(b"\n");
            pem.extend_from_slice(&identity.1);
            client = client.identity(reqwest::Identity::from_pem(&pem)?);
        }
        if let Some(url) = &self.config.proxy {
            let mut proxy = reqwest::Proxy::all(url)?;
            if let Some(auth) = &self.config.proxy_authorization {
                proxy = proxy.custom_http_auth(auth.parse()?);
            }
            client = client.proxy(proxy);
        }
        client.build().context("cannot configure HTTP transport")
    }

    pub async fn grpc_channel(&self, endpoint: &str) -> Result<tonic::transport::Channel> {
        self.grpc_channel_named(endpoint, None).await
    }

    pub async fn grpc_channel_named(
        &self,
        endpoint: &str,
        server_name: Option<&str>,
    ) -> Result<tonic::transport::Channel> {
        let url = url_allowed(endpoint, self.config.allow_plaintext_remote)?;
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut target = tonic::transport::Endpoint::from_shared(endpoint.to_owned())?
            .http2_max_header_list_size(64 * 1024)
            .connect_timeout(Duration::from_secs(10))
            .tcp_nodelay(true);
        if url.scheme() == "https" {
            let mut tls = tonic::transport::ClientTlsConfig::new().with_native_roots();
            if let Some(name) = server_name {
                tls = tls.domain_name(name);
            }
            if let Some(ca) = &self.ca {
                tls = tls.ca_certificate(tonic::transport::Certificate::from_pem(ca.as_slice()));
            }
            if let Some(identity) = &self.identity {
                tls = tls.identity(tonic::transport::Identity::from_pem(
                    &identity.0,
                    &identity.1,
                ));
            }
            target = target.tls_config(tls)?;
        }
        if let Some(proxy) = &self.config.proxy {
            let proxy = url::Url::parse(proxy)?;
            let host = proxy.host_str().context("proxy host missing")?.to_owned();
            let port = proxy
                .port_or_known_default()
                .context("proxy port missing")?;
            let auth = self.config.proxy_authorization.clone();
            let authority = format!(
                "{}:{}",
                url.host_str().unwrap(),
                url.port_or_known_default().unwrap()
            );
            let connector = tower::service_fn(move |_uri: http::Uri| {
                let (host, authority, auth) = (host.clone(), authority.clone(), auth.clone());
                async move {
                    let mut stream = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
                    stream.set_nodelay(true)?;
                    let mut request =
                        format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
                    if let Some(auth) = auth {
                        request.push_str(&format!("Proxy-Authorization: {auth}\r\n"));
                    }
                    request.push_str("\r\n");
                    stream.write_all(request.as_bytes()).await?;
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        if head.len() >= 16 * 1024 {
                            return Err(std::io::Error::other(
                                "proxy response headers exceed limit",
                            ));
                        }
                        head.push(stream.read_u8().await?);
                    }
                    let status = String::from_utf8_lossy(&head);
                    if !status
                        .lines()
                        .next()
                        .is_some_and(|s| s.split_whitespace().nth(1) == Some("200"))
                    {
                        return Err(std::io::Error::other("proxy CONNECT failed"));
                    }
                    Ok::<_, std::io::Error>(hyper_util::rt::TokioIo::new(stream))
                }
            });
            Ok(target.connect_with_connector(connector).await?)
        } else {
            Ok(target.connect().await?)
        }
    }
}
