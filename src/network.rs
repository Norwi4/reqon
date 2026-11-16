use crate::model::{Request, resolve};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use rustls::{
    DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug)]
pub struct Unverified;
impl ServerCertVerifier for Unverified {
    fn verify_server_cert(
        &self,
        _: &CertificateDer<'_>,
        _: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::ring::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        rustls::crypto::ring::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
pub fn tls_config(
    request: &Request,
    vars: &BTreeMap<String, String>,
) -> Result<Arc<rustls::ClientConfig>> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut roots = rustls::RootCertStore::empty();
    if request.ca_certificate.is_empty() || request.default_ca.unwrap_or(true) {
        roots.add_parsable_certificates(rustls_native_certs::load_native_certs().certs);
    }
    if !request.ca_certificate.is_empty() {
        let pem = std::fs::read(crate::engine::file_path(
            request,
            &resolve(&request.ca_certificate, vars),
        ))?;
        for cert in rustls_pemfile::certs(&mut pem.as_slice()) {
            roots.add(cert?)?;
        }
    }
    let builder = rustls::ClientConfig::builder().with_root_certificates(roots);
    let mut config = if let Some(pem) = &request.client_identity {
        builder.with_client_auth_cert(
            rustls_pemfile::certs(&mut pem.as_slice()).collect::<Result<Vec<_>, _>>()?,
            rustls_pemfile::private_key(&mut pem.as_slice())?
                .context("Certificate contains no private key")?,
        )?
    } else if !request.client_certificate.is_empty() {
        let cert = std::fs::read(crate::engine::file_path(
            request,
            &resolve(&request.client_certificate, vars),
        ))?;
        let key = if request.client_key.is_empty() {
            cert.clone()
        } else {
            std::fs::read(crate::engine::file_path(
                request,
                &resolve(&request.client_key, vars),
            ))?
        };
        builder.with_client_auth_cert(
            rustls_pemfile::certs(&mut cert.as_slice()).collect::<Result<Vec<_>, _>>()?,
            rustls_pemfile::private_key(&mut key.as_slice())?
                .context("PEM file contains no private key")?,
        )?
    } else {
        builder.with_no_client_auth()
    };
    if !request.verify_tls {
        config
            .dangerous()
            .set_certificate_verifier(Arc::new(Unverified));
    }
    Ok(Arc::new(config))
}
fn decoded(value: &str) -> String {
    percent_encoding::percent_decode_str(value)
        .decode_utf8_lossy()
        .into_owned()
}
pub trait Transport: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Transport for T {}
pub type Stream = Box<dyn Transport>;
#[cfg(test)]
pub async fn tcp(host: &str, port: u16, proxy: &str) -> Result<Stream> {
    tcp_with_tls(host, port, proxy, None).await
}
pub fn proxy_tls_config(
    request: &Request,
    vars: &BTreeMap<String, String>,
) -> Result<Option<Arc<rustls::ClientConfig>>> {
    if !request.proxy.starts_with("https://") {
        return Ok(None);
    }
    let mut policy = request.clone();
    policy.client_certificate.clear();
    policy.client_key.clear();
    policy.client_identity = None;
    Ok(Some(tls_config(&policy, vars)?))
}
pub async fn tcp_with_tls(
    host: &str,
    port: u16,
    proxy: &str,
    tls: Option<Arc<rustls::ClientConfig>>,
) -> Result<Stream> {
    if proxy.trim().is_empty() {
        let stream = tokio::net::TcpStream::connect((host, port)).await?;
        stream.set_nodelay(true)?;
        return Ok(Box::new(stream));
    }
    let proxy = url::Url::parse(proxy).context("Invalid proxy URL")?;
    let proxy_port = proxy
        .port()
        .unwrap_or(if proxy.scheme().starts_with("socks") {
            1080
        } else if proxy.scheme() == "https" {
            443
        } else {
            8080
        });
    let stream = tokio::net::TcpStream::connect((
        proxy.host_str().context("Proxy has no host")?,
        proxy_port,
    ))
    .await?;
    stream.set_nodelay(true)?;
    let mut stream: Stream = if proxy.scheme() == "https" {
        let config = match tls {
            Some(config) => config,
            None => tls_config(&Request::default(), &BTreeMap::new())?,
        };
        let name = ServerName::try_from(proxy.host_str().context("Proxy has no host")?.to_owned())?;
        Box::new(
            tokio_rustls::TlsConnector::from(config)
                .connect(name, stream)
                .await
                .context("TLS connection to proxy failed")?,
        )
    } else {
        Box::new(stream)
    };
    match proxy.scheme() {
        "http" | "https" => {
            let target = if host.contains(':') {
                format!("[{host}]:{port}")
            } else {
                format!("{host}:{port}")
            };
            let auth = if proxy.username().is_empty() {
                String::new()
            } else {
                format!(
                    "Proxy-Authorization: Basic {}\r\n",
                    base64::prelude::BASE64_STANDARD.encode(format!(
                        "{}:{}",
                        decoded(proxy.username()),
                        decoded(proxy.password().unwrap_or(""))
                    ))
                )
            };
            stream
                .write_all(
                    format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n{auth}\r\n").as_bytes(),
                )
                .await?;
            let mut response = Vec::new();
            while !response.ends_with(b"\r\n\r\n") {
                ensure!(response.len() < 16384, "Proxy response header too large");
                response.push(stream.read_u8().await?);
            }
            let status = String::from_utf8_lossy(&response)
                .lines()
                .next()
                .unwrap_or("")
                .to_owned();
            ensure!(
                status.split_whitespace().nth(1) == Some("200"),
                "Proxy tunnel failed: {status}"
            );
        }
        "socks4" | "socks4a" => {
            let user = decoded(proxy.username());
            ensure!(
                !user.contains('\0') && !host.contains('\0'),
                "Invalid SOCKS4 address or user"
            );
            let ip = if proxy.scheme() == "socks4" {
                tokio::net::lookup_host((host, port))
                    .await?
                    .find_map(|a| match a.ip() {
                        std::net::IpAddr::V4(v) => Some(v.octets()),
                        _ => None,
                    })
                    .context("SOCKS4 requires an IPv4 destination")?
            } else {
                [0, 0, 0, 1]
            };
            let mut bytes = vec![4, 1];
            bytes.extend(port.to_be_bytes());
            bytes.extend(ip);
            bytes.extend(user.as_bytes());
            bytes.push(0);
            if proxy.scheme() == "socks4a" {
                bytes.extend(host.as_bytes());
                bytes.push(0);
            }
            stream.write_all(&bytes).await?;
            let mut answer = [0; 8];
            stream.read_exact(&mut answer).await?;
            ensure!(
                answer[1] == 90,
                "SOCKS4 connection rejected (code {})",
                answer[1]
            );
        }
        "socks5" | "socks5h" => {
            let auth = !proxy.username().is_empty();
            stream
                .write_all(if auth { &[5, 2, 0, 2] } else { &[5, 1, 0] })
                .await?;
            let mut answer = [0; 2];
            stream.read_exact(&mut answer).await?;
            ensure!(
                answer[0] == 5 && answer[1] != 255,
                "SOCKS5 proxy rejected authentication methods"
            );
            if answer[1] == 2 {
                let user = decoded(proxy.username());
                let pass = decoded(proxy.password().unwrap_or(""));
                ensure!(
                    user.len() < 256 && pass.len() < 256,
                    "SOCKS credentials too long"
                );
                let mut bytes = vec![1, user.len() as u8];
                bytes.extend(user.as_bytes());
                bytes.push(pass.len() as u8);
                bytes.extend(pass.as_bytes());
                stream.write_all(&bytes).await?;
                stream.read_exact(&mut answer).await?;
                ensure!(answer[1] == 0, "SOCKS5 authentication failed");
            } else {
                ensure!(answer[1] == 0, "Unsupported SOCKS5 authentication method");
            }
            ensure!(host.len() < 256, "SOCKS5 host name too long");
            let mut bytes = vec![5, 1, 0, 3, host.len() as u8];
            bytes.extend(host.as_bytes());
            bytes.extend(port.to_be_bytes());
            stream.write_all(&bytes).await?;
            let mut header = [0; 4];
            stream.read_exact(&mut header).await?;
            ensure!(
                header[1] == 0,
                "SOCKS5 connection failed with code {}",
                header[1]
            );
            let size = match header[3] {
                1 => 4,
                4 => 16,
                3 => stream.read_u8().await? as usize,
                _ => bail!("Invalid SOCKS5 address type"),
            };
            let mut tail = vec![0; size + 2];
            stream.read_exact(&mut tail).await?;
        }
        scheme => bail!(
            "Proxy scheme '{scheme}' is not available for this protocol; use HTTP CONNECT or SOCKS5"
        ),
    }
    Ok(stream)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn http_proxy_connect_authenticates_and_preserves_tunnel_bytes() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let proxy = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(stream.read_u8().await.unwrap());
            }
            let text = String::from_utf8(request).unwrap();
            assert!(text.starts_with("CONNECT example.test:443 HTTP/1.1"));
            assert!(text.contains("Proxy-Authorization: Basic dXNlcjpwYXNz"));
            stream
                .write_all(b"HTTP/1.1 200 Connection established\r\n\r\nPONG")
                .await
                .unwrap();
        });
        let mut tunnel = tcp("example.test", 443, &format!("http://user:pass@{address}"))
            .await
            .unwrap();
        let mut output = [0; 4];
        tunnel.read_exact(&mut output).await.unwrap();
        assert_eq!(&output, b"PONG");
        proxy.await.unwrap();
    }
    #[tokio::test]
    async fn tls_policy_rejects_untrusted_and_accepts_custom_ca_or_explicit_override() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let private = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.cert.der().clone()], private.into())
            .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                let (stream, _) = listener.accept().await.unwrap();
                if let Ok(mut tls) = acceptor.accept(stream).await {
                    let mut data = [0; 4];
                    tls.read_exact(&mut data).await.unwrap();
                    tls.write_all(&data).await.unwrap();
                }
            }
        });
        let dir = tempfile::tempdir().unwrap();
        let ca = dir.path().join("ca.pem");
        std::fs::write(&ca, cert.cert.pem()).unwrap();
        for (verify, custom, success) in [
            (true, false, false),
            (true, true, true),
            (false, false, true),
        ] {
            let request = Request {
                verify_tls: verify,
                ca_certificate: if custom {
                    ca.to_string_lossy().into_owned()
                } else {
                    String::new()
                },
                ..Default::default()
            };
            let connector =
                tokio_rustls::TlsConnector::from(tls_config(&request, &BTreeMap::new()).unwrap());
            let stream = tokio::net::TcpStream::connect(address).await.unwrap();
            let result = connector
                .connect(ServerName::try_from("localhost").unwrap(), stream)
                .await;
            assert_eq!(result.is_ok(), success);
            if let Ok(mut stream) = result {
                stream.write_all(b"PING").await.unwrap();
                let mut echo = [0; 4];
                stream.read_exact(&mut echo).await.unwrap();
                assert_eq!(&echo, b"PING");
            }
        }
        server.await.unwrap();
    }
}
