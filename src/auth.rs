use crate::model::{Auth, resolve};
use anyhow::{Context, Result, bail, ensure};
use base64::Engine as _;
use hmac::{Hmac, Mac};
use reqwest::{Client, Request, Response, header};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
pub struct TokenCache(tokio::sync::Mutex<BTreeMap<String, Token>>);
struct Token {
    value: String,
    refresh: String,
    expires: Instant,
}
pub fn values(auth: &Auth, vars: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    auth.fields
        .iter()
        .map(|(k, v)| (k.clone(), resolve(v, vars)))
        .collect()
}
fn get<'a>(fields: &'a BTreeMap<String, String>, key: &str) -> &'a str {
    fields.get(key).map(String::as_str).unwrap_or("")
}
pub fn sign_edgegrid(
    request: &mut Request,
    auth: &Auth,
    vars: &BTreeMap<String, String>,
) -> Result<()> {
    let fields = values(auth, vars);
    for key in ["accessToken", "clientToken", "clientSecret"] {
        ensure!(
            !get(&fields, key).trim().is_empty(),
            "EdgeGrid: {key} is required"
        );
    }
    let timestamp = if get(&fields, "timestamp").is_empty() {
        time::OffsetDateTime::now_utc().format(time::macros::format_description!(
            "[year][month][day]T[hour]:[minute]:[second]+0000"
        ))?
    } else {
        get(&fields, "timestamp").into()
    };
    let nonce = if get(&fields, "nonce").is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        get(&fields, "nonce").into()
    };
    let mut url = request.url().clone();
    if !get(&fields, "baseURL").trim().is_empty() {
        let raw = get(&fields, "baseURL").trim();
        let base = url::Url::parse(&if raw.contains("://") {
            raw.to_owned()
        } else {
            format!("{}://{raw}", url.scheme())
        })?;
        url.set_scheme(base.scheme())
            .map_err(|_| anyhow::anyhow!("Invalid EdgeGrid URL scheme"))?;
        url.set_host(base.host_str())?;
        url.set_port(base.port())
            .map_err(|_| anyhow::anyhow!("Invalid EdgeGrid URL port"))?;
    }
    let header = format!(
        "EG1-HMAC-SHA256 client_token={};access_token={};timestamp={timestamp};nonce={nonce};",
        get(&fields, "clientToken"),
        get(&fields, "accessToken")
    );
    let headers = get(&fields, "headersToSign")
        .split(',')
        .map(|n| n.trim().to_lowercase())
        .filter_map(|n| {
            let v = request.headers().get(&n)?.to_str().ok()?;
            Some(format!(
                "{n}:{}",
                v.split_whitespace().collect::<Vec<_>>().join(" ")
            ))
        })
        .collect::<Vec<_>>()
        .join("\t");
    let b64 = base64::engine::general_purpose::STANDARD;
    let body = request
        .body()
        .and_then(|b| b.as_bytes())
        .unwrap_or_default();
    ensure!(
        request.body().is_none() || request.body().is_some_and(|b| b.as_bytes().is_some()),
        "EdgeGrid signing requires a buffered request body"
    );
    let limit = get(&fields, "maxBodySize")
        .parse::<usize>()
        .unwrap_or(131072);
    let hash = if request.method() == reqwest::Method::POST && !body.is_empty() {
        b64.encode(Sha256::digest(&body[..body.len().min(limit)]))
    } else {
        String::new()
    };
    let path = format!(
        "{}{}",
        url.path(),
        url.query().map(|q| format!("?{q}")).unwrap_or_default()
    );
    let host = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let data = format!(
        "{}\t{}\t{host}\t{path}\t{headers}\t{hash}\t{header}",
        request.method(),
        url.scheme()
    );
    let hmac = |key: &[u8], data: &[u8]| {
        let mut h = Hmac::<Sha256>::new_from_slice(key).unwrap();
        h.update(data);
        b64.encode(h.finalize().into_bytes())
    };
    let signing_key = hmac(
        get(&fields, "clientSecret").as_bytes(),
        timestamp.as_bytes(),
    );
    let signature = hmac(signing_key.as_bytes(), data.as_bytes());
    request.headers_mut().insert(
        header::AUTHORIZATION,
        format!("{header}signature={signature}").parse()?,
    );
    Ok(())
}
impl TokenCache {
    pub async fn token(
        &self,
        auth: &Auth,
        vars: &BTreeMap<String, String>,
        client: &Client,
    ) -> Result<String> {
        let fields = values(auth, vars);
        let supplied = get(&fields, "accessToken");
        if !supplied.is_empty() {
            return Ok(supplied.to_owned());
        }
        if !auth.token.is_empty() {
            return Ok(resolve(&auth.token, vars));
        }
        let key = format!("{:x}", Sha256::digest(serde_json::to_vec(&fields)?));
        let cache = self.0.lock().await;
        if let Some(token) = cache.get(&key) {
            if token.expires > Instant::now() {
                return Ok(token.value.clone());
            }
        }
        let refresh = cache
            .get(&key)
            .map(|t| t.refresh.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(get(&fields, "refreshToken"))
            .to_owned();
        drop(cache);
        let grant = if !refresh.is_empty() {
            "refresh_token"
        } else {
            match get(&fields, "grantType") {
                "" => "client_credentials",
                other => other,
            }
        };
        let token_url = if grant == "refresh_token" && !get(&fields, "refreshTokenUrl").is_empty() {
            get(&fields, "refreshTokenUrl")
        } else {
            get(&fields, "accessTokenUrl")
        };
        ensure!(
            !token_url.is_empty(),
            "OAuth2 requires an Access Token URL or an access token"
        );
        let mut form = vec![("grant_type".to_owned(), grant.to_owned())];
        match grant {
            "client_credentials" => {}
            "password" => {
                form.push(("username".into(), resolve(&auth.username, vars)));
                form.push(("password".into(), resolve(&auth.password, vars)));
            }
            "refresh_token" => form.push(("refresh_token".into(), refresh.clone())),
            "authorization_code" => {
                let (code, callback, verifier) = if get(&fields, "code").is_empty() {
                    crate::oauth_browser::authorize(&fields).await?
                } else {
                    (
                        get(&fields, "code").to_owned(),
                        get(&fields, "callbackUrl").to_owned(),
                        get(&fields, "codeVerifier").to_owned(),
                    )
                };
                form.push(("code".into(), code));
                form.push(("redirect_uri".into(), callback));
                if !verifier.is_empty() {
                    form.push(("code_verifier".into(), verifier));
                }
            }
            other => bail!(
                "OAuth2 grant '{other}' requires an access token from the authorization server"
            ),
        }
        if !get(&fields, "scope").is_empty() {
            form.push(("scope".into(), get(&fields, "scope").into()));
        }
        let mut req = client.post(token_url).timeout(Duration::from_secs(30));
        let client_id = get(&fields, "clientId");
        let secret = get(&fields, "clientSecret");
        if matches!(
            get(&fields, "credentialsPlacement"),
            "body" | "request_body"
        ) {
            form.push(("client_id".into(), client_id.into()));
            form.push(("client_secret".into(), secret.into()));
        } else {
            let encode =
                |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
            req = req.basic_auth(encode(client_id), Some(encode(secret)));
        }
        let mut response = req
            .form(&form)
            .send()
            .await
            .context("OAuth2 token request failed")?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            ensure!(
                bytes.len() + chunk.len() <= 1024 * 1024,
                "OAuth2 token response exceeds 1 MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let data: serde_json::Value =
            serde_json::from_slice(&bytes).context("OAuth2 server returned invalid JSON")?;
        ensure!(
            status.is_success(),
            "OAuth2 token request returned {status}: {}",
            data["error_description"]
                .as_str()
                .or(data["error"].as_str())
                .unwrap_or("token request rejected")
        );
        let token = data["access_token"]
            .as_str()
            .filter(|s| !s.is_empty())
            .context("OAuth2 response has no access_token")?
            .to_owned();
        let seconds = data["expires_in"]
            .as_u64()
            .or_else(|| data["expires_in"].as_str().and_then(|s| s.parse().ok()))
            .unwrap_or(300)
            .min(604800);
        let lifetime = Duration::from_secs(seconds);
        let rotated = data["refresh_token"]
            .as_str()
            .unwrap_or(&refresh)
            .to_owned();
        let mut cache = self.0.lock().await;
        if cache.len() > 64 {
            cache.retain(|_, t| t.expires > Instant::now());
        }
        cache.insert(
            key,
            Token {
                value: token.clone(),
                refresh: rotated,
                expires: Instant::now() + lifetime.saturating_sub(lifetime / 10),
            },
        );
        Ok(token)
    }
}

fn challenge_fields(challenge: &str) -> BTreeMap<String, String> {
    let input = challenge
        .strip_prefix("Digest ")
        .or_else(|| challenge.strip_prefix("digest "))
        .unwrap_or(challenge);
    let mut result = BTreeMap::new();
    let mut rest = input.trim();
    while let Some((key, value)) = rest.split_once('=') {
        let key = key.trim().trim_start_matches(',').trim().to_lowercase();
        let value = value.trim_start();
        if let Some(quoted) = value.strip_prefix('"') {
            let mut out = String::new();
            let mut escaped = false;
            let mut end = None;
            for (i, c) in quoted.char_indices() {
                if escaped {
                    out.push(c);
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    end = Some(i);
                    break;
                } else {
                    out.push(c);
                }
            }
            let Some(end) = end else { break };
            result.insert(key, out);
            rest = quoted[end + 1..].trim_start_matches(',').trim_start();
        } else {
            let (value, next) = value.split_once(',').unwrap_or((value, ""));
            result.insert(key, value.trim().into());
            rest = next.trim();
        }
        if rest.is_empty() {
            break;
        }
    }
    result
}
pub fn digest_header(
    challenge: &str,
    method: &str,
    uri: &str,
    body: &[u8],
    username: &str,
    password: &str,
    cnonce: &str,
) -> Result<String> {
    let p = challenge_fields(challenge);
    let realm = get(&p, "realm");
    let nonce = get(&p, "nonce");
    ensure!(!nonce.is_empty(), "Digest challenge has no nonce");
    let algorithm = if get(&p, "algorithm").is_empty() {
        "MD5"
    } else {
        get(&p, "algorithm")
    };
    let hash = |value: &[u8]| -> Result<String> {
        Ok(
            match algorithm.to_ascii_uppercase().trim_end_matches("-SESS") {
                "MD5" => format!("{:x}", md5::Md5::digest(value)),
                "SHA-256" => format!("{:x}", Sha256::digest(value)),
                "SHA-512-256" => format!("{:x}", sha2::Sha512_256::digest(value)),
                other => bail!("Digest algorithm {other} is not supported"),
            },
        )
    };
    let mut ha1 = hash(format!("{username}:{realm}:{password}").as_bytes())?;
    if algorithm.to_ascii_uppercase().ends_with("-SESS") {
        ha1 = hash(format!("{ha1}:{nonce}:{cnonce}").as_bytes())?;
    }
    let choices = get(&p, "qop").split(',').map(str::trim).collect::<Vec<_>>();
    let qop = if choices.contains(&"auth") {
        "auth"
    } else if choices.contains(&"auth-int") {
        "auth-int"
    } else if get(&p, "qop").is_empty() {
        ""
    } else {
        bail!("Unsupported Digest quality of protection")
    };
    let ha2 = if qop == "auth-int" {
        hash(format!("{method}:{uri}:{}", hash(body)?).as_bytes())?
    } else {
        hash(format!("{method}:{uri}").as_bytes())?
    };
    let response = hash(
        if qop.is_empty() {
            format!("{ha1}:{nonce}:{ha2}")
        } else {
            format!("{ha1}:{nonce}:00000001:{cnonce}:{qop}:{ha2}")
        }
        .as_bytes(),
    )?;
    let quote = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
    let mut out = format!(
        "Digest username=\"{}\", realm=\"{}\", nonce=\"{}\", uri=\"{}\", response=\"{response}\", algorithm={algorithm}",
        quote(username),
        quote(realm),
        quote(nonce),
        quote(uri)
    );
    if !qop.is_empty() {
        out.push_str(&format!(", qop={qop}, nc=00000001, cnonce=\"{cnonce}\""));
    }
    if !get(&p, "opaque").is_empty() {
        out.push_str(&format!(", opaque=\"{}\"", quote(get(&p, "opaque"))));
    }
    Ok(out)
}
pub async fn digest_send(
    client: &Client,
    request: Request,
    auth: &Auth,
    vars: &BTreeMap<String, String>,
) -> Result<Response> {
    let retry = request
        .try_clone()
        .context("Digest authentication requires a replayable request body")?;
    let response = client.execute(request).await?;
    if response.status() != reqwest::StatusCode::UNAUTHORIZED {
        return Ok(response);
    }
    let Some(challenge) = response
        .headers()
        .get_all(header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .find(|v| v.to_lowercase().starts_with("digest "))
    else {
        return Ok(response);
    };
    let mut request = retry;
    let uri = match request.url().query() {
        Some(q) => format!("{}?{q}", request.url().path()),
        None => request.url().path().to_owned(),
    };
    let value = digest_header(
        challenge,
        request.method().as_str(),
        &uri,
        request.body().and_then(|b| b.as_bytes()).unwrap_or(&[]),
        &resolve(&auth.username, vars),
        &resolve(&auth.password, vars),
        &uuid::Uuid::new_v4().simple().to_string(),
    )?;
    request
        .headers_mut()
        .insert(header::AUTHORIZATION, value.parse()?);
    drop(response);
    Ok(client.execute(request).await?)
}

pub fn sign_aws(request: &mut Request, auth: &Auth, vars: &BTreeMap<String, String>) -> Result<()> {
    for key in [
        "authorization",
        "x-amz-date",
        "x-amz-content-sha256",
        "x-amz-security-token",
    ] {
        request.headers_mut().remove(key);
    }
    use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings};
    let p = values(auth, vars);
    let region = get(&p, "region");
    let service = get(&p, "service");
    ensure!(
        !get(&p, "accessKeyId").is_empty() && !get(&p, "secretAccessKey").is_empty(),
        "AWS Signature requires an Access Key ID and Secret Access Key"
    );
    ensure!(
        !region.is_empty() && !service.is_empty(),
        "AWS Signature requires a Region and Service"
    );
    let identity = aws_credential_types::Credentials::new(
        get(&p, "accessKeyId"),
        get(&p, "secretAccessKey"),
        p.get("sessionToken").filter(|s| !s.is_empty()).cloned(),
        None,
        "reqon",
    )
    .into();
    let mut settings = SigningSettings::default();
    settings.payload_checksum_kind = aws_sigv4::http_request::PayloadChecksumKind::XAmzSha256;
    if service == "s3" {
        settings.percent_encoding_mode = aws_sigv4::http_request::PercentEncodingMode::Single;
        settings.uri_path_normalization_mode =
            aws_sigv4::http_request::UriPathNormalizationMode::Disabled;
    }
    let params = aws_sigv4::sign::v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name(service)
        .time(SystemTime::now())
        .settings(settings)
        .build()?
        .into();
    let bytes = match request.body() {
        None => &[][..],
        Some(b) => b
            .as_bytes()
            .context("AWS Signature requires a replayable request body")?,
    };
    let headers = request
        .headers()
        .iter()
        .map(|(k, v)| Ok((k.as_str(), v.to_str()?)))
        .collect::<Result<Vec<_>>>()?;
    let signable = SignableRequest::new(
        request.method().as_str(),
        request.url().as_str(),
        headers.into_iter(),
        SignableBody::Bytes(bytes),
    )?;
    let (instructions, _) = aws_sigv4::http_request::sign(signable, &params)?.into_parts();
    for (name, value) in instructions.headers() {
        request.headers_mut().insert(
            header::HeaderName::from_bytes(name.as_bytes())?,
            value.parse()?,
        );
    }
    Ok(())
}
fn pct(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char)
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
pub fn sign_oauth1(
    request: &mut Request,
    auth: &Auth,
    vars: &BTreeMap<String, String>,
) -> Result<()> {
    let p = values(auth, vars);
    let mut oauth = BTreeMap::new();
    oauth.insert(
        "oauth_consumer_key".to_owned(),
        get(&p, "consumerKey").to_owned(),
    );
    if !get(&p, "accessToken").is_empty() {
        oauth.insert("oauth_token".into(), get(&p, "accessToken").into());
    }
    let method = if get(&p, "signatureMethod").is_empty() {
        "HMAC-SHA1"
    } else {
        get(&p, "signatureMethod")
    };
    oauth.insert("oauth_signature_method".into(), method.into());
    oauth.insert(
        "oauth_timestamp".into(),
        p.get("timestamp")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
                    .to_string()
            }),
    );
    oauth.insert(
        "oauth_nonce".into(),
        p.get("nonce")
            .filter(|s| !s.is_empty())
            .cloned()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
    );
    oauth.insert("oauth_version".into(), "1.0".into());
    for (field, key) in [
        ("callbackUrl", "oauth_callback"),
        ("verifier", "oauth_verifier"),
    ] {
        if !get(&p, field).is_empty() {
            oauth.insert(key.into(), get(&p, field).into());
        }
    }
    let mut params = request
        .url()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect::<Vec<_>>();
    if request
        .headers()
        .get(header::CONTENT_TYPE)
        .is_some_and(|v| {
            v.to_str()
                .unwrap_or("")
                .starts_with("application/x-www-form-urlencoded")
        })
    {
        params.extend(
            url::form_urlencoded::parse(request.body().and_then(|b| b.as_bytes()).unwrap_or(&[]))
                .map(|(k, v)| (k.into_owned(), v.into_owned())),
        );
    }
    params.extend(oauth.clone());
    let mut encoded = params
        .iter()
        .map(|(k, v)| (pct(k), pct(v)))
        .collect::<Vec<_>>();
    encoded.sort();
    let normalized = encoded
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");
    let mut base_url = request.url().clone();
    base_url.set_query(None);
    base_url.set_fragment(None);
    let base = format!(
        "{}&{}&{}",
        request.method(),
        pct(base_url.as_str()),
        pct(&normalized)
    );
    let key = format!(
        "{}&{}",
        pct(get(&p, "consumerSecret")),
        pct(get(&p, "accessTokenSecret"))
    );
    let signature = match method {
        "HMAC-SHA1" => {
            let mut m = Hmac::<sha1::Sha1>::new_from_slice(key.as_bytes())?;
            m.update(base.as_bytes());
            base64::prelude::BASE64_STANDARD.encode(m.finalize().into_bytes())
        }
        "HMAC-SHA256" => {
            let mut m = Hmac::<Sha256>::new_from_slice(key.as_bytes())?;
            m.update(base.as_bytes());
            base64::prelude::BASE64_STANDARD.encode(m.finalize().into_bytes())
        }
        "PLAINTEXT" => key,
        other => bail!("OAuth1 signature method '{other}' is not supported"),
    };
    oauth.insert("oauth_signature".into(), signature);
    match get(&p, "placement") {
        "query" | "queryparams" => {
            for (k, v) in oauth {
                request.url_mut().query_pairs_mut().append_pair(&k, &v);
            }
        }
        "body" => {
            let mut pairs = params
                .into_iter()
                .filter(|(k, _)| !k.starts_with("oauth_"))
                .collect::<Vec<_>>();
            pairs.extend(oauth);
            let text = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(pairs)
                .finish();
            *request.body_mut() = Some(text.into());
            request.headers_mut().insert(
                header::CONTENT_TYPE,
                header::HeaderValue::from_static("application/x-www-form-urlencoded"),
            );
        }
        _ => {
            let value = format!(
                "OAuth {}",
                oauth
                    .iter()
                    .map(|(k, v)| format!("{}=\"{}\"", pct(k), pct(v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            request
                .headers_mut()
                .insert(header::AUTHORIZATION, value.parse()?);
        }
    }
    Ok(())
}
pub fn sign_wsse(
    request: &mut Request,
    auth: &Auth,
    vars: &BTreeMap<String, String>,
) -> Result<()> {
    let nonce = uuid::Uuid::new_v4();
    let created =
        time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339)?;
    let mut bytes = nonce.as_bytes().to_vec();
    bytes.extend_from_slice(created.as_bytes());
    bytes.extend_from_slice(resolve(&auth.password, vars).as_bytes());
    let digest = base64::prelude::BASE64_STANDARD.encode(sha1::Sha1::digest(bytes));
    let username = resolve(&auth.username, vars).replace('"', "\\\"");
    let value = format!(
        "UsernameToken Username=\"{username}\", PasswordDigest=\"{digest}\", Nonce=\"{}\", Created=\"{created}\"",
        base64::prelude::BASE64_STANDARD.encode(nonce.as_bytes())
    );
    request.headers_mut().insert("x-wsse", value.parse()?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edgegrid_matches_published_akamai_signature_vectors() {
        let auth = Auth {
            kind: "edgegrid".into(),
            fields: BTreeMap::from([
                (
                    "clientToken".into(),
                    "akab-client-token-xxx-xxxxxxxxxxxxxxxx".into(),
                ),
                (
                    "accessToken".into(),
                    "akab-access-token-xxx-xxxxxxxxxxxxxxxx".into(),
                ),
                (
                    "clientSecret".into(),
                    "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx=".into(),
                ),
                (
                    "nonce".into(),
                    "nonce-xx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".into(),
                ),
                ("timestamp".into(), "20140321T19:34:21+0000".into()),
            ]),
            ..Default::default()
        };
        for (method, path, body, signature) in [
            (
                "GET",
                "/",
                "",
                "tL+y4hxyHxgWVD30X3pWnGKHcPzmrIF+LThiAOhMxYU=",
            ),
            (
                "GET",
                "/testapi/v1/t1?p1=1&p2=2",
                "",
                "hKDH1UlnQySSHjvIcZpDMbQHihTQ0XyVAKZaApabdeA=",
            ),
            (
                "POST",
                "/testapi/v1/t3",
                "datadatadatadatadatadatadatadata",
                "hXm4iCxtpN22m4cbZb4lVLW5rhX8Ca82vCFqXzSTPe4=",
            ),
        ] {
            let mut request = Client::new()
                .request(
                    method.parse().unwrap(),
                    format!(
                        "https://akaa-baseurl-xxxxxxxxxxx-xxxxxxxxxxxxx.luna.akamaiapis.net{path}"
                    ),
                )
                .body(body)
                .build()
                .unwrap();
            sign_edgegrid(&mut request, &auth, &BTreeMap::new()).unwrap();
            assert!(
                request.headers()[header::AUTHORIZATION]
                    .to_str()
                    .unwrap()
                    .ends_with(&format!("signature={signature}"))
            );
        }
    }
    #[tokio::test]
    async fn oauth2_fetches_and_reuses_access_token() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut token_requests = 0;
            for _ in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 8192];
                let n = socket.read(&mut buf).await.unwrap();
                let raw = String::from_utf8_lossy(&buf[..n]);
                let body = if raw.starts_with("POST /token ") {
                    token_requests += 1;
                    assert!(
                        raw.to_lowercase()
                            .contains("authorization: basic y2xpzw50onnly3jlda==")
                    );
                    r#"{"access_token":"cached-token","token_type":"Bearer","expires_in":300}"#
                } else {
                    assert!(
                        raw.to_lowercase()
                            .contains("authorization: bearer cached-token")
                    );
                    "OK"
                };
                socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
            }
            token_requests
        });
        let engine = crate::engine::Engine::default();
        let request = crate::model::Request {
            url: format!("http://{address}/resource"),
            auth: Auth {
                kind: "oauth2".into(),
                fields: BTreeMap::from([
                    ("accessTokenUrl".into(), format!("http://{address}/token")),
                    ("clientId".into(), "client".into()),
                    ("clientSecret".into(), "secret".into()),
                ]),
                ..Default::default()
            },
            ..Default::default()
        };
        for _ in 0..2 {
            let reply = engine.execute(request.clone(), BTreeMap::new()).await;
            assert_eq!(reply.error, "");
            assert_eq!(reply.status, 200);
        }
        assert_eq!(server.await.unwrap(), 1);
    }
    #[tokio::test]
    async fn digest_retries_401_with_valid_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut first, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 4096];
            first.read(&mut buf).await.unwrap();
            first.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Digest realm=\"qa\", nonce=\"test-nonce\", qop=\"auth\", algorithm=SHA-256\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap();
            drop(first);
            let (mut second, _) = listener.accept().await.unwrap();
            let n = second.read(&mut buf).await.unwrap();
            let raw = String::from_utf8_lossy(&buf[..n]);
            let value = raw
                .lines()
                .find_map(|l| l.strip_prefix("authorization: "))
                .unwrap();
            let fields = challenge_fields(value);
            let expected = digest_header(
                "Digest realm=\"qa\", nonce=\"test-nonce\", qop=\"auth\", algorithm=SHA-256",
                "GET",
                "/protected?q=1",
                b"",
                "alice",
                "secret",
                &fields["cnonce"],
            )
            .unwrap();
            assert_eq!(fields["response"], challenge_fields(&expected)["response"]);
            second
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let request = crate::model::Request {
            url: format!("http://{address}/protected?q=1"),
            auth: Auth {
                kind: "digest".into(),
                username: "alice".into(),
                password: "secret".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let reply = crate::engine::Engine::default()
            .execute(request, BTreeMap::new())
            .await;
        assert_eq!(reply.error, "");
        assert_eq!(reply.status, 200);
        server.await.unwrap();
    }
    #[test]
    fn rfc_digest_vector() {
        let header=digest_header("Digest realm=\"testrealm@host.com\", qop=\"auth,auth-int\", nonce=\"dcd98b7102dd2f0e8b11d0f600bfb0c093\", opaque=\"5ccc069c403ebaf9f0171e9517f40e41\"","GET","/dir/index.html",b"","Mufasa","Circle Of Life","0a4f113b").unwrap();
        assert!(
            header.contains("response=\"6629fae49393a05397450978507c4ef1\""),
            "{header}"
        );
    }
    #[test]
    fn oauth1_signature_vector() {
        let _engine = crate::engine::Engine::default();
        let mut request = Client::new()
            .get("http://photos.example.net/photos?file=vacation.jpg&size=original")
            .build()
            .unwrap();
        let auth = Auth {
            kind: "oauth1".into(),
            fields: BTreeMap::from([
                ("consumerKey".into(), "dpf43f3p2l4k3l03".into()),
                ("consumerSecret".into(), "kd94hf93k423kf44".into()),
                ("accessToken".into(), "nnch734d00sl2jdk".into()),
                ("accessTokenSecret".into(), "pfkkdhi9sl3r4s00".into()),
                ("timestamp".into(), "1191242096".into()),
                ("nonce".into(), "kllo9940pd9333jh".into()),
            ]),
            ..Default::default()
        };
        sign_oauth1(&mut request, &auth, &BTreeMap::new()).unwrap();
        assert!(
            request.headers()[header::AUTHORIZATION]
                .to_str()
                .unwrap()
                .contains("tR3%2BTy81lMeYAr%2FFid0kMTYa%2FWM%3D")
        );
    }
}
