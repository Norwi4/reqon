use anyhow::{Context, Result, ensure};
use base64::Engine as _;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
pub fn pem(label: &str, der: &[u8]) -> Vec<u8> {
    let b64 = base64::prelude::BASE64_STANDARD.encode(der);
    let mut s = format!("-----BEGIN {label}-----\n");
    for line in b64.as_bytes().chunks(64) {
        s.push_str(std::str::from_utf8(line).unwrap());
        s.push('\n');
    }
    s.push_str(&format!("-----END {label}-----\n"));
    s.into_bytes()
}
pub fn decrypt_pem(input: &[u8], password: &str) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(input).context("Private key must be PEM encoded")?;
    if text.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        let encrypted = pkcs8::SecretDocument::from_pem(text)?.1;
        let encrypted = pkcs8::EncryptedPrivateKeyInfo::try_from(encrypted.as_bytes())?;
        let plain = encrypted
            .decrypt(password)
            .context("Incorrect private key passphrase")?;
        return Ok(pem("PRIVATE KEY", plain.as_bytes()));
    }
    if let Some(line) = text.lines().find(|s| s.starts_with("DEK-Info:")) {
        ensure!(!password.is_empty(), "Private key requires a passphrase");
        let (algorithm, iv) = line
            .trim_start_matches("DEK-Info:")
            .trim()
            .split_once(',')
            .context("Invalid encrypted PEM header")?;
        let iv = (0..iv.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(iv.get(i..i + 2).unwrap_or(""), 16))
            .collect::<Result<Vec<_>, _>>()?;
        let key_len = match algorithm {
            "AES-128-CBC" => 16,
            "AES-192-CBC" => 24,
            "AES-256-CBC" => 32,
            "DES-EDE3-CBC" => 24,
            "DES-CBC" => 8,
            _ => anyhow::bail!("Unsupported PEM encryption: {algorithm}"),
        };
        ensure!(iv.len() >= 8, "Invalid private-key IV");
        let mut key = vec![];
        let mut last = vec![];
        use md5::{Digest, Md5};
        while key.len() < key_len {
            let mut hash = Md5::new();
            hash.update(&last);
            hash.update(password.as_bytes());
            hash.update(&iv[..8]);
            last = hash.finalize().to_vec();
            key.extend(&last);
        }
        key.truncate(key_len);
        let b64 = text
            .lines()
            .filter(|s| !s.starts_with('-') && !s.contains(':'))
            .collect::<String>();
        let cipher = base64::prelude::BASE64_STANDARD.decode(b64)?;
        macro_rules! decrypt {
            ($cipher:ty) => {
                cbc::Decryptor::<$cipher>::new_from_slices(&key, &iv)
                    .map_err(|_| anyhow::anyhow!("Invalid private-key encryption parameters"))?
                    .decrypt_padded_vec_mut::<Pkcs7>(&cipher)
                    .map_err(|_| anyhow::anyhow!("Incorrect private key passphrase"))?
            };
        }
        let plain = match algorithm {
            "AES-128-CBC" => decrypt!(aes::Aes128),
            "AES-192-CBC" => decrypt!(aes::Aes192),
            "AES-256-CBC" => decrypt!(aes::Aes256),
            "DES-EDE3-CBC" => decrypt!(des::TdesEde3),
            _ => decrypt!(des::Des),
        };
        let label = text
            .lines()
            .find_map(|s| {
                s.strip_prefix("-----BEGIN ")
                    .and_then(|s| s.strip_suffix("-----"))
            })
            .context("Missing private-key PEM label")?;
        return Ok(pem(label, &plain));
    }
    ensure!(
        rustls_pemfile::private_key(&mut input.as_ref())?.is_some(),
        "PEM file contains no private key"
    );
    Ok(input.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn openssl_encrypted_pkcs8_and_traditional_keys_accept_correct_passphrase() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        for fixture in [
            include_bytes!("../tests/fixtures/preferences/encrypted-pkcs8.pem").as_slice(),
            include_bytes!("../tests/fixtures/preferences/encrypted-traditional.pem").as_slice(),
        ] {
            assert!(decrypt_pem(fixture, "wrong").is_err());
            let pem = decrypt_pem(fixture, "fixture-pass").unwrap();
            let key = rustls_pemfile::private_key(&mut pem.as_slice())
                .unwrap()
                .unwrap();
            assert!(rustls::crypto::ring::sign::any_supported_type(&key).is_ok());
        }
    }
}
