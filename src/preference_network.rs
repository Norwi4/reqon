use crate::{
    model::{Preferences, Request, resolve},
    preference_model::Certificate,
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub fn host_matches(pattern: &str, url: &url::Url) -> bool {
    let pattern = pattern
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("grpcs://")
        .trim_start_matches("wss://")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let (host, port) = pattern
        .rsplit_once(':')
        .filter(|(_, p)| p.parse::<u16>().is_ok())
        .map_or((pattern.as_str(), None), |(h, p)| {
            (h, p.parse::<u16>().ok())
        });
    if port.is_some() && port != url.port_or_known_default() {
        return false;
    }
    let actual = url.host_str().unwrap_or("").to_ascii_lowercase();
    host == actual
        || host == "*"
        || host
            .strip_prefix("*.")
            .is_some_and(|suffix| actual.ends_with(&format!(".{suffix}")))
}
pub fn bypasses(list: &str, url: &url::Url) -> bool {
    list.split([',', ';'])
        .filter(|x| !x.trim().is_empty())
        .any(|entry| {
            let entry = entry.trim();
            if entry == "<local>" {
                return !url.host_str().unwrap_or("").contains('.');
            }
            if let Some((address, bits)) = entry.split_once('/') {
                if let (Ok(network), Ok(host), Ok(bits)) = (
                    address.parse::<std::net::Ipv4Addr>(),
                    url.host_str().unwrap_or("").parse::<std::net::Ipv4Addr>(),
                    bits.parse::<u32>(),
                ) {
                    if bits <= 32 {
                        let mask = if bits == 0 {
                            0
                        } else {
                            u32::MAX << (32 - bits)
                        };
                        return u32::from(network) & mask == u32::from(host) & mask;
                    }
                }
            }
            host_matches(entry, url)
                || host_matches(&format!("*.{}", entry.trim_start_matches('.')), url)
        })
}

#[cfg(windows)]
fn registry(name: &str) -> Option<Vec<u8>> {
    #[link(name = "advapi32")]
    unsafe extern "system" {
        fn RegGetValueW(
            hkey: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut std::ffi::c_void,
            size: *mut u32,
        ) -> i32;
    }
    let sub: Vec<u16> = "Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings\0"
        .encode_utf16()
        .collect();
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let mut len = 0;
    unsafe {
        if RegGetValueW(
            0x80000001u32 as i32 as isize,
            sub.as_ptr(),
            name.as_ptr(),
            0xffff,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut len,
        ) != 0
        {
            return None;
        }
        let mut bytes = vec![0u8; len as usize];
        if RegGetValueW(
            0x80000001u32 as i32 as isize,
            sub.as_ptr(),
            name.as_ptr(),
            0xffff,
            std::ptr::null_mut(),
            bytes.as_mut_ptr().cast(),
            &mut len,
        ) != 0
        {
            return None;
        }
        Some(bytes)
    }
}
#[cfg(not(windows))]
fn registry(_: &str) -> Option<Vec<u8>> {
    None
}
fn registry_text(name: &str) -> String {
    registry(name)
        .map(|b| {
            String::from_utf16_lossy(
                &b.chunks_exact(2)
                    .map(|x| u16::from_le_bytes([x[0], x[1]]))
                    .take_while(|x| *x != 0)
                    .collect::<Vec<_>>(),
            )
        })
        .unwrap_or_default()
}
fn env_proxy(name: &str) -> String {
    std::env::var(name)
        .or_else(|_| std::env::var(name.to_uppercase()))
        .unwrap_or_default()
}
#[derive(Clone, Default)]
pub struct SystemProxy {
    pub source: String,
    pub http: String,
    pub https: String,
    pub bypass: String,
    pub pac: String,
}
pub fn system_proxy() -> SystemProxy {
    let mut result = SystemProxy {
        source: "environment".into(),
        http: env_proxy("http_proxy"),
        https: env_proxy("https_proxy"),
        bypass: env_proxy("no_proxy"),
        pac: env_proxy("pac_url"),
    };
    let all = env_proxy("all_proxy");
    if result.http.is_empty() {
        result.http = all.clone();
    }
    if result.https.is_empty() {
        result.https = all;
    }
    if result.http.is_empty() && result.https.is_empty() && result.pac.is_empty() {
        result.source = "system".into();
        result.pac = registry_text("AutoConfigURL");
        let enabled = registry("ProxyEnable").is_some_and(|b| b.first() == Some(&1));
        if enabled {
            let value = registry_text("ProxyServer");
            for item in value.split(';') {
                if let Some((protocol, address)) = item.split_once('=') {
                    match protocol {
                        "http" => result.http = address.into(),
                        "https" => result.https = address.into(),
                        _ => {}
                    }
                } else {
                    result.http = item.into();
                    result.https = item.into();
                }
            }
            result.bypass = registry_text("ProxyOverride");
        }
    }
    for value in [&mut result.http, &mut result.https] {
        if !value.is_empty() && !value.contains("://") {
            *value = format!("http://{value}");
        }
    }
    result
}

type PacCache = BTreeMap<String, (Instant, String)>;
fn pac_cache() -> &'static Mutex<PacCache> {
    static C: OnceLock<Mutex<PacCache>> = OnceLock::new();
    C.get_or_init(Default::default)
}
pub fn clear_pac() {
    pac_cache().lock().unwrap().clear();
}
pub fn pac(script: &str, url: &url::Url) -> Result<String> {
    use rquickjs::{Context as JsContext, Function, Runtime};
    let runtime = Runtime::new()?;
    runtime.set_memory_limit(8 * 1024 * 1024);
    let end = Instant::now() + Duration::from_secs(3);
    runtime.set_interrupt_handler(Some(Box::new(move || Instant::now() > end)));
    let ctx = JsContext::full(&runtime)?;
    ctx.with(|ctx| -> Result<String> {
        ctx.globals().set(
            "dnsResolve",
            Function::new(ctx.clone(), |host: String| -> Option<String> {
                use std::net::ToSocketAddrs;
                if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
                    return Some(ip.to_string());
                }
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let answer = (host.as_str(), 0)
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut a| a.find(|a| a.is_ipv4()))
                        .map(|a| a.ip().to_string());
                    let _ = tx.send(answer);
                });
                rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
            })?,
        )?;
        ctx.globals().set(
            "myIpAddress",
            Function::new(ctx.clone(), || -> String {
                std::net::UdpSocket::bind("0.0.0.0:0")
                    .ok()
                    .and_then(|socket| {
                        socket.connect("192.0.2.1:9").ok()?;
                        socket.local_addr().ok().map(|a| a.ip().to_string())
                    })
                    .unwrap_or_else(|| "127.0.0.1".into())
            })?,
        )?;
        ctx.eval::<(), _>(include_str!("../assets/pac.js"))?;
        ctx.eval::<(), _>(script).context("Invalid PAC script")?;
        let f: Function = ctx
            .globals()
            .get("FindProxyForURL")
            .context("PAC file has no FindProxyForURL")?;
        let result: String = f
            .call((url.as_str(), url.host_str().unwrap_or("")))
            .context("PAC script failed")?;
        for item in result.split(';') {
            let part: Vec<_> = item.split_whitespace().collect();
            match part.as_slice() {
                ["DIRECT"] => return Ok(String::new()),
                [kind, host] => {
                    let scheme = match *kind {
                        "PROXY" | "HTTP" => "http",
                        "HTTPS" => "https",
                        "SOCKS" | "SOCKS4" => "socks4",
                        "SOCKS5" => "socks5h",
                        _ => continue,
                    };
                    return Ok(format!("{scheme}://{host}"));
                }
                _ => {}
            }
        }
        anyhow::bail!("PAC did not return a supported proxy or DIRECT")
    })
}
async fn resolve_pac(source: &str, url: &url::Url) -> Result<String> {
    ensure!(
        !source.trim().is_empty(),
        "Select a PAC file or enter its URL"
    );
    let cached = pac_cache()
        .lock()
        .unwrap()
        .get(source)
        .filter(|(at, _)| at.elapsed() < Duration::from_secs(300))
        .map(|(_, s)| s.clone());
    let script = if let Some(s) = cached {
        s
    } else {
        let s = if source.starts_with("https://") || source.starts_with("http://") {
            let response = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(15))
                .build()?
                .get(source)
                .send()
                .await?
                .error_for_status()?;
            ensure!(
                response.content_length().unwrap_or(0) <= 2 * 1024 * 1024,
                "PAC file is too large"
            );
            response.text().await?
        } else {
            let path = source.strip_prefix("file:///").unwrap_or(source);
            std::fs::read_to_string(path).context("Cannot read PAC file")?
        };
        ensure!(s.len() <= 2 * 1024 * 1024, "PAC file is too large");
        pac_cache()
            .lock()
            .unwrap()
            .insert(source.into(), (Instant::now(), s.clone()));
        s
    };
    let url = url.clone();
    tokio::task::spawn_blocking(move || pac(&script, &url)).await?
}

pub fn certificate_pem(c: &Certificate, password: &str) -> Result<Vec<u8>> {
    let mut pem = vec![];
    if c.kind == "pfx" {
        let data = std::fs::read(&c.pfx).context("Cannot read PFX file")?;
        let store = p12_keystore::KeyStore::from_pkcs12(
            &data,
            password,
            p12_keystore::Pkcs12ImportPolicy::Strict,
        )
        .context("Cannot unlock PFX certificate; check its passphrase")?;
        let (_, chain) = store
            .private_key_chain()
            .context("PFX contains no private key and certificate chain")?;
        for cert in chain.certs() {
            pem.extend(crate::certificate_keys::pem("CERTIFICATE", cert.as_der()));
        }
        pem.extend(crate::certificate_keys::pem(
            "PRIVATE KEY",
            chain.key().as_der(),
        ));
    } else {
        pem = std::fs::read(&c.cert).context("Cannot read certificate file")?;
        let key = if c.key.is_empty() {
            pem.clone()
        } else {
            std::fs::read(&c.key).context("Cannot read private key file")?
        };
        let key = crate::certificate_keys::decrypt_pem(&key, password)?;
        pem.push(b'\n');
        pem.extend(key);
    }
    reqwest::Identity::from_pem(&pem).context("Certificate and private key are invalid")?;
    Ok(pem)
}

pub async fn prepare(
    p: &Preferences,
    r: &mut Request,
    vars: &BTreeMap<String, String>,
) -> Result<()> {
    // Local proto discovery needs neither a host nor any network preferences.
    if r.url.trim().is_empty() && r.protocol == "grpc" {
        return Ok(());
    }
    let raw = if r.protocol == "grpc" {
        r.grpc_endpoint(vars)?
    } else {
        resolve(&r.url, vars)
    };
    let raw = if raw.contains("://") {
        raw
    } else {
        format!("http://{raw}")
    };
    let url = url::Url::parse(&raw).context("Invalid request URL")?;
    if !r.proxy_disabled && r.proxy.trim().is_empty() {
        let config = &p.proxy_settings;
        let proxy = match config.mode.as_str() {
            "off" => String::new(),
            "manual" => {
                if bypasses(&config.bypass, &url) {
                    String::new()
                } else {
                    ensure!(!config.host.trim().is_empty(), "Proxy hostname is required");
                    let mut u =
                        url::Url::parse(&format!("{}://{}", config.protocol, config.host.trim()))
                            .context("Invalid proxy hostname")?;
                    if !config.port.is_empty() {
                        u.set_port(Some(
                            config
                                .port
                                .parse::<u16>()
                                .context("Proxy port must be between 1 and 65535")?,
                        ))
                        .map_err(|_| anyhow::anyhow!("Invalid proxy port"))?;
                        ensure!(
                            u.port() != Some(0),
                            "Proxy port must be between 1 and 65535"
                        );
                    }
                    if config.auth {
                        let _ = u.set_username(&config.username);
                        let _ = u.set_password(Some(
                            p.credentials.get("proxy").map(String::as_str).unwrap_or(""),
                        ));
                    }
                    u.to_string()
                }
            }
            "pac" => resolve_pac(&config.pac_source, &url).await?,
            _ => {
                let system = system_proxy();
                if bypasses(&system.bypass, &url) {
                    String::new()
                } else if !system.pac.is_empty() {
                    resolve_pac(&system.pac, &url).await?
                } else if matches!(url.scheme(), "https" | "wss") {
                    system.https
                } else {
                    system.http
                }
            }
        };
        r.proxy = proxy;
        r.proxy_disabled = r.proxy.is_empty();
    }
    r.default_ca = Some(p.keep_default_ca);
    if r.client_certificate.is_empty() {
        if let Some(cert) = p
            .certificates
            .iter()
            .find(|c| c.enabled && host_matches(&c.domain, &url))
        {
            let cert = cert.clone();
            let password = p
                .credentials
                .get(&format!("cert:{}", cert.id))
                .cloned()
                .unwrap_or_default();
            r.client_identity = Some(
                tokio::task::spawn_blocking(move || certificate_pem(&cert, &password)).await??,
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn domain_and_proxy_exclusions_respect_boundaries_and_ports() {
        let u = url::Url::parse("https://api.example.com:8443/x").unwrap();
        assert!(host_matches("*.example.com:8443", &u));
        assert!(!host_matches("example.com", &u));
        assert!(!host_matches("*.example.com:443", &u));
        assert!(bypasses("localhost,example.com", &u));
        assert!(!bypasses("ample.com", &u));
        assert!(bypasses(
            "10.0.0.0/8",
            &url::Url::parse("http://10.8.9.1").unwrap()
        ));
    }
    #[test]
    fn pac_selects_proxy_without_exposing_host_functions() {
        let u = url::Url::parse("https://api.example.com/x").unwrap();
        assert_eq!(pac("function FindProxyForURL(url,host){return dnsDomainIs(host,'.example.com')?'PROXY localhost:8181':'DIRECT';}",&u).unwrap(),"http://localhost:8181");
        assert!(pac("function FindProxyForURL(){while(true){}}", &u).is_err());
    }
}
