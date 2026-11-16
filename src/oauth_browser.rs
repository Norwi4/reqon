use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Authorization {
    callback: url::Url,
    url: url::Url,
    state: String,
    verifier: String,
}
fn prepare(fields: &BTreeMap<String, String>) -> Result<Authorization> {
    let value = |key: &str| fields.get(key).map(String::as_str).unwrap_or("");
    let mut url = url::Url::parse(value("authorizeUrl"))
        .context("OAuth2 requires a valid Authorization URL")?;
    ensure!(
        matches!(url.scheme(), "http" | "https"),
        "OAuth2 Authorization URL must use HTTP or HTTPS"
    );
    ensure!(!value("clientId").is_empty(), "OAuth2 requires a Client ID");
    let callback = url::Url::parse(if value("callbackUrl").is_empty() {
        "http://127.0.0.1:53682/callback"
    } else {
        value("callbackUrl")
    })?;
    ensure!(
        callback.scheme() == "http"
            && matches!(
                callback.host_str(),
                Some("localhost" | "127.0.0.1" | "[::1]")
            ),
        "Automatic OAuth2 authorization needs an http://localhost or loopback callback URL. For other callbacks, enter the authorization code manually."
    );
    let state = uuid::Uuid::new_v4().to_string();
    let verifier = if !value("codeVerifier").is_empty() {
        value("codeVerifier").into()
    } else if value("pkce") == "true" {
        format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        )
    } else {
        String::new()
    };
    {
        let mut query = url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", value("clientId"))
            .append_pair("redirect_uri", callback.as_str())
            .append_pair("state", &state);
        if !value("scope").is_empty() {
            query.append_pair("scope", value("scope"));
        }
        if !value("audience").is_empty() {
            query.append_pair("audience", value("audience"));
        }
        if !verifier.is_empty() {
            query
                .append_pair("code_challenge_method", "S256")
                .append_pair(
                    "code_challenge",
                    &base64::engine::general_purpose::URL_SAFE_NO_PAD
                        .encode(Sha256::digest(verifier.as_bytes())),
                );
        }
    }
    Ok(Authorization {
        callback,
        url,
        state,
        verifier,
    })
}
pub async fn authorize(fields: &BTreeMap<String, String>) -> Result<(String, String, String)> {
    ensure!(
        fields.get("openBrowser").is_none_or(|s| s != "false"),
        "Enter an authorization code, or enable Use System Browser for OAuth2 Authorization in Preferences"
    );
    let auth = prepare(fields)?;
    let host = if auth.callback.host_str() == Some("[::1]") {
        "::1"
    } else {
        "127.0.0.1"
    };
    let listener = TcpListener::bind((host, auth.callback.port_or_known_default().unwrap_or(80)))
        .await
        .context("Cannot listen on the OAuth2 callback port")?;
    open_browser(auth.url.as_str())?;
    let code = tokio::time::timeout(
        Duration::from_secs(180),
        wait_callback(listener, &auth.callback, &auth.state),
    )
    .await
    .context("OAuth2 authorization timed out after 3 minutes")??;
    Ok((code, auth.callback.to_string(), auth.verifier))
}
async fn wait_callback(listener: TcpListener, callback: &url::Url, state: &str) -> Result<String> {
    loop {
        let (mut stream, _) = listener.accept().await?;
        let mut bytes = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(10), async {
            let mut buf = [0u8; 2048];
            loop {
                let n = stream.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
                if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
                ensure!(bytes.len() < 32768, "OAuth2 callback headers too large");
            }
            Ok::<_, anyhow::Error>(())
        })
        .await;
        if !matches!(read, Ok(Ok(()))) {
            continue;
        }
        let request = String::from_utf8_lossy(&bytes);
        let mut first = request.lines().next().unwrap_or("").split_whitespace();
        if first.next() != Some("GET") {
            continue;
        }
        let Some(target) = first.next() else {
            continue;
        };
        let Ok(url) = callback.join(target) else {
            continue;
        };
        let query = url.query_pairs().into_owned().collect::<BTreeMap<_, _>>();
        let valid = url.path() == callback.path() && query.get("state").is_some_and(|s| s == state);
        let code = query.get("code").filter(|s| !s.is_empty());
        let success = valid && code.is_some();
        let body = if success {
            "Authorization received. You can close this window and return to Reqon."
        } else {
            "Authorization callback rejected. Return to the authorization page and try again."
        };
        let status = if success { "200 OK" } else { "400 Bad Request" };
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes()).await;
        let _ = stream.shutdown().await;
        if valid {
            if let Some(error) = query.get("error") {
                bail!("OAuth2 authorization failed: {error}");
            }
            if let Some(code) = code {
                return Ok(code.clone());
            }
        }
    }
}
#[cfg(windows)]
fn open_browser(url: &str) -> Result<()> {
    #[link(name = "shell32")]
    unsafe extern "system" {
        fn ShellExecuteW(
            window: *mut std::ffi::c_void,
            operation: *const u16,
            file: *const u16,
            parameters: *const u16,
            directory: *const u16,
            show: i32,
        ) -> isize;
    }
    let url = url.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let operation = "open".encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            url.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    ensure!(
        result > 32,
        "Windows could not open the OAuth2 authorization page ({result})"
    );
    Ok(())
}
#[cfg(not(windows))]
fn open_browser(_: &str) -> Result<()> {
    bail!("Automatic OAuth2 browser authorization is available on Windows")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn callback_rejects_wrong_state_then_accepts_valid_code() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let callback = url::Url::parse(&format!(
            "http://{}/callback",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let fields = BTreeMap::from([
            ("authorizeUrl".into(), "http://localhost/authorize".into()),
            ("clientId".into(), "fixture-client".into()),
            ("callbackUrl".into(), callback.to_string()),
            ("pkce".into(), "true".into()),
        ]);
        let auth = prepare(&fields).unwrap();
        let pairs = auth.url.query_pairs().collect::<BTreeMap<_, _>>();
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert!(auth.verifier.len() >= 43);
        let url = callback.to_string();
        let state = auth.state.clone();
        let server =
            tokio::spawn(async move { wait_callback(listener, &callback, &state).await.unwrap() });
        let client = reqwest::Client::new();
        assert_eq!(
            client
                .get(format!("{url}?code=bad&state=wrong"))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        assert_eq!(
            client
                .get(format!("{url}?code=good&state={}", auth.state))
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(server.await.unwrap(), "good");
    }
}
