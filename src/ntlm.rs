//! NTLMv2 over HTTP; wire layout follows the client used by Bruno.
use crate::model::{Auth, resolve};
use anyhow::{Context, Result, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hmac::{Hmac, Mac};
use md4::{Digest, Md4};
use md5::Md5;
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
const FLAGS: u32 = 0xa2888207;
fn unicode(value: &str) -> Vec<u8> {
    value.encode_utf16().flat_map(u16::to_le_bytes).collect()
}
fn hmac(key: &[u8], data: &[u8]) -> [u8; 16] {
    let mut h = Hmac::<Md5>::new_from_slice(key).unwrap();
    h.update(data);
    h.finalize().into_bytes().into()
}
fn u16_at(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .context("Truncated NTLM challenge")?
            .try_into()?,
    ))
}
fn u32_at(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("Truncated NTLM challenge")?
            .try_into()?,
    ))
}
fn field(bytes: &[u8], at: usize) -> Result<&[u8]> {
    let len = u16_at(bytes, at)? as usize;
    let start = u32_at(bytes, at + 4)? as usize;
    bytes
        .get(start..start.checked_add(len).context("Invalid NTLM field")?)
        .context("Invalid NTLM security buffer")
}
fn append(message: &mut Vec<u8>, at: usize, data: &[u8]) -> Result<()> {
    let len = u16::try_from(data.len()).context("NTLM credential field too long")?;
    message[at..at + 2].copy_from_slice(&len.to_le_bytes());
    message[at + 2..at + 4].copy_from_slice(&len.to_le_bytes());
    let offset = message.len() as u32;
    message[at + 4..at + 8].copy_from_slice(&offset.to_le_bytes());
    message.extend_from_slice(data);
    Ok(())
}
fn type1() -> Vec<u8> {
    let mut b = vec![0; 40];
    b[..8].copy_from_slice(b"NTLMSSP\0");
    b[8..12].copy_from_slice(&1u32.to_le_bytes());
    b[12..16].copy_from_slice(&FLAGS.to_le_bytes());
    b[32..40].copy_from_slice(&[10, 0, 0, 0, 0, 0, 0, 15]);
    b
}
fn type3(
    challenge: &[u8],
    negotiate: &[u8],
    username: &str,
    password: &str,
    domain: &str,
    workstation: &str,
) -> Result<Vec<u8>> {
    ensure!(
        challenge.len() >= 48
            && challenge.get(..8) == Some(b"NTLMSSP\0")
            && u32_at(challenge, 8)? == 2,
        "Invalid NTLM challenge message"
    );
    let server_flags = u32_at(challenge, 20)?;
    ensure!(
        server_flags & 0x00080000 != 0,
        "This server does not support NTLMv2 extended session security"
    );
    let server_nonce = &challenge[24..32];
    let target_info = if server_flags & 0x00800000 != 0 {
        field(challenge, 40)?
    } else {
        &[0, 0, 0, 0]
    };
    let mut timestamp = None;
    let mut mic = false;
    let mut offset = 0;
    while offset + 4 <= target_info.len() {
        let kind = u16_at(target_info, offset)?;
        let len = u16_at(target_info, offset + 2)? as usize;
        offset += 4;
        if kind == 0 {
            break;
        }
        let value = target_info
            .get(offset..offset + len)
            .context("Invalid NTLM target information")?;
        if kind == 7 && len == 8 {
            timestamp = Some(value.to_vec());
        }
        if kind == 6 && len == 4 {
            mic = u32_at(value, 0)? & 2 != 0;
        }
        offset += len;
    }
    let time = timestamp.clone().unwrap_or_else(|| {
        ((SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            / 100) as u64
            + 116444736000000000)
            .to_le_bytes()
            .to_vec()
    });
    let nonce = uuid::Uuid::new_v4();
    let nonce = &nonce.as_bytes()[..8];
    let nthash = Md4::digest(unicode(password));
    let key = hmac(
        &nthash,
        &unicode(&format!("{}{domain}", username.to_uppercase())),
    );
    let mut blob = vec![1, 1, 0, 0, 0, 0, 0, 0];
    blob.extend_from_slice(&time);
    blob.extend_from_slice(nonce);
    blob.extend_from_slice(&[0; 4]);
    blob.extend_from_slice(target_info);
    blob.extend_from_slice(&[0; 4]);
    let mut proof_input = server_nonce.to_vec();
    proof_input.extend_from_slice(&blob);
    let proof = hmac(&key, &proof_input);
    let mut nt = proof.to_vec();
    nt.extend_from_slice(&blob);
    let lm = if timestamp.is_some() {
        vec![0; 24]
    } else {
        let mut input = server_nonce.to_vec();
        input.extend_from_slice(nonce);
        let mut lm = hmac(&key, &input).to_vec();
        lm.extend_from_slice(nonce);
        lm
    };
    let flags = (server_flags & FLAGS) | 0x200;
    let has_version = flags & 0x02000000 != 0;
    let mut message = vec![
        0;
        if mic {
            88
        } else if has_version {
            72
        } else {
            64
        }
    ];
    message[..8].copy_from_slice(b"NTLMSSP\0");
    message[8..12].copy_from_slice(&3u32.to_le_bytes());
    message[60..64].copy_from_slice(&flags.to_le_bytes());
    if has_version {
        message[64..72].copy_from_slice(&[10, 0, 0, 0, 0, 0, 0, 15]);
    }
    let encode = |text: &str| {
        if flags & 1 != 0 {
            unicode(text)
        } else {
            text.as_bytes().to_vec()
        }
    };
    append(&mut message, 12, &lm)?;
    append(&mut message, 20, &nt)?;
    append(&mut message, 28, &encode(domain))?;
    append(&mut message, 36, &encode(username))?;
    append(&mut message, 44, &encode(workstation))?;
    append(&mut message, 52, &[])?;
    if mic {
        let session_key = hmac(&key, &proof);
        let mut all = negotiate.to_vec();
        all.extend_from_slice(challenge);
        all.extend_from_slice(&message);
        message[72..88].copy_from_slice(&hmac(&session_key, &all));
    }
    Ok(message)
}
pub async fn send(
    client: &reqwest::Client,
    mut request: reqwest::Request,
    auth: &Auth,
    vars: &BTreeMap<String, String>,
) -> Result<reqwest::Response> {
    let mut retry = request
        .try_clone()
        .context("NTLM requires a replayable request body")?;
    let negotiate = type1();
    request.headers_mut().insert(
        reqwest::header::AUTHORIZATION,
        format!("NTLM {}", STANDARD.encode(&negotiate)).parse()?,
    );
    let mut response = client.execute(request).await?;
    if response.status() != 401 {
        return Ok(response);
    }
    let challenge = response
        .headers()
        .get_all(reqwest::header::WWW_AUTHENTICATE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .find_map(|v| {
            let (scheme, value) = v.trim().split_once(' ')?;
            scheme
                .eq_ignore_ascii_case("NTLM")
                .then(|| value.trim().to_owned())
        });
    let Some(challenge) = challenge else {
        return Ok(response);
    };
    let challenge = STANDARD
        .decode(challenge)
        .context("Invalid NTLM challenge encoding")?;
    let username = resolve(&auth.username, vars);
    let password = resolve(&auth.password, vars);
    let fields = crate::auth::values(auth, vars);
    let configured_domain = fields.get("domain").map(String::as_str).unwrap_or("");
    let (domain, user) = username
        .split_once('\\')
        .unwrap_or((configured_domain, &username));
    let workstation = fields
        .get("workstation")
        .cloned()
        .unwrap_or_else(|| std::env::var("COMPUTERNAME").unwrap_or_default());
    let authenticate = type3(
        &challenge,
        &negotiate,
        user,
        &password,
        domain,
        &workstation,
    )?;
    // Read the whole challenge response so reqwest returns this same socket to its pool.
    let mut consumed = 0usize;
    while let Some(chunk) = response.chunk().await? {
        consumed += chunk.len();
        ensure!(
            consumed <= 1024 * 1024,
            "NTLM challenge response exceeds 1 MiB"
        );
    }
    retry.headers_mut().insert(
        reqwest::header::AUTHORIZATION,
        format!("NTLM {}", STANDARD.encode(authenticate)).parse()?,
    );
    Ok(client.execute(retry).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn authenticates_ntlmv2_on_same_connection_with_verified_password_proof() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 16384];
            let n = socket.read(&mut buf).await.unwrap();
            let first = String::from_utf8_lossy(&buf[..n]);
            assert!(first.to_lowercase().contains("authorization: ntlm "));
            let mut challenge = vec![0; 52];
            challenge[..8].copy_from_slice(b"NTLMSSP\0");
            challenge[8..12].copy_from_slice(&2u32.to_le_bytes());
            challenge[20..24].copy_from_slice(&0x00880001u32.to_le_bytes());
            challenge[24..32].copy_from_slice(b"12345678");
            challenge[40..42].copy_from_slice(&4u16.to_le_bytes());
            challenge[42..44].copy_from_slice(&4u16.to_le_bytes());
            challenge[44..48].copy_from_slice(&48u32.to_le_bytes());
            socket.write_all(format!("HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM {}\r\nContent-Length: 0\r\nConnection: keep-alive\r\n\r\n",STANDARD.encode(challenge)).as_bytes()).await.unwrap();
            let n = socket.read(&mut buf).await.unwrap();
            let second = String::from_utf8_lossy(&buf[..n]);
            let header = second
                .lines()
                .find_map(|l| {
                    let (k, v) = l.split_once(':')?;
                    k.eq_ignore_ascii_case("authorization").then(|| v.trim())
                })
                .unwrap();
            let message = STANDARD
                .decode(header.strip_prefix("NTLM ").unwrap())
                .unwrap();
            assert_eq!(u32_at(&message, 8).unwrap(), 3);
            let nt = field(&message, 20).unwrap();
            let nthash = Md4::digest(unicode("Password"));
            assert_eq!(format!("{nthash:x}"), "a4f49c406510bdcab6824ee7c30fd852");
            let key = hmac(&nthash, &unicode("USERDomain"));
            let mut proof = b"12345678".to_vec();
            proof.extend_from_slice(&nt[16..]);
            assert_eq!(&nt[..16], &hmac(&key, &proof));
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK")
                .await
                .unwrap();
        });
        let reply = crate::engine::Engine::default()
            .execute(
                crate::model::Request {
                    url: format!("http://{address}/"),
                    auth: Auth {
                        kind: "ntlm".into(),
                        username: "Domain\\User".into(),
                        password: "Password".into(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                BTreeMap::new(),
            )
            .await;
        assert_eq!(reply.error, "");
        assert_eq!(reply.status, 200);
        server.await.unwrap();
    }
}
