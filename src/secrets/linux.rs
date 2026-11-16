//! Linux Secret Service stores the random master key; profile files contain only
//! authenticated ciphertext. There is deliberately no plaintext fallback.
use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    aead,
    rand::{SecureRandom, SystemRandom},
};
use std::{
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    process::{Command, Output, Stdio},
    sync::Mutex,
    time::{Duration, Instant},
};

const MAGIC: &[u8; 8] = b"RQSEC001";
static KEY: Mutex<Option<[u8; 32]>> = Mutex::new(None);

fn secret_tool(operation: &str, input: Option<&[u8]>) -> Result<Output> {
    let mut command = Command::new("secret-tool");
    command.arg(operation);
    if operation == "store" {
        command.arg("--label=Reqon profile encryption");
    }
    command
        .args(["application", "reqon", "purpose", "profile-encryption-v1"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().context(
        "Install libsecret-tools and enable your desktop Secret Service (GNOME Keyring or KWallet)",
    )?;
    if let Some(mut stdin) = child.stdin.take() {
        if let Some(input) = input {
            stdin.write_all(input)?;
        }
    }
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(child.wait_with_output()?);
        }
        if started.elapsed() > Duration::from_secs(60) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Desktop keyring timed out. Unlock your keyring and retry saving.");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn lookup() -> Result<Option<[u8; 32]>> {
    let output = secret_tool("lookup", None)?;
    if output.status.success() {
        let text =
            std::str::from_utf8(&output.stdout).context("Invalid Reqon key in desktop keyring")?;
        let bytes = STANDARD
            .decode(text.trim())
            .context("Invalid Reqon key in desktop keyring")?;
        return Ok(Some(bytes.try_into().map_err(|_| {
            anyhow!("Invalid Reqon key length in desktop keyring")
        })?));
    }
    if output.status.code() == Some(1) && output.stderr.is_empty() {
        return Ok(None);
    }
    bail!(
        "Desktop Secret Service is unavailable or locked. Unlock GNOME Keyring / KWallet and retry."
    )
}
fn master_key(create: bool) -> Result<[u8; 32]> {
    let mut cached = KEY
        .lock()
        .map_err(|_| anyhow!("Desktop keyring lock failed"))?;
    if let Some(key) = *cached {
        return Ok(key);
    }
    // Serialize first-time creation across Reqon processes so a second process
    // cannot replace a key that has already encrypted another process's data.
    let directory = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
        .context("HOME or XDG_DATA_HOME is required for Linux credential storage")?
        .join("Reqon");
    std::fs::create_dir_all(&directory)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(directory.join(".keyring-init.lock"))?;
    lock.lock()?;
    let key = if let Some(key) = lookup()? {
        key
    } else {
        ensure!(
            create,
            "Reqon encryption key is missing from the desktop keyring; existing encrypted data was left intact"
        );
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| anyhow!("Cannot generate a secure encryption key"))?;
        let output = secret_tool("store", Some(STANDARD.encode(key).as_bytes()))?;
        ensure!(
            output.status.success(),
            "Cannot save the Reqon encryption key. Unlock your desktop keyring and retry."
        );
        ensure!(
            lookup()? == Some(key),
            "Desktop keyring did not retain the Reqon encryption key"
        );
        key
    };
    *cached = Some(key);
    Ok(key)
}
fn cipher(create: bool) -> Result<aead::LessSafeKey> {
    let key = aead::UnboundKey::new(&aead::AES_256_GCM, &master_key(create)?)
        .map_err(|_| anyhow!("Cannot initialize profile encryption"))?;
    Ok(aead::LessSafeKey::new(key))
}
pub(crate) fn protect(data: &[u8]) -> Result<Vec<u8>> {
    let key = cipher(true)?;
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| anyhow!("Cannot generate a secure nonce"))?;
    let mut ciphertext = data.to_vec();
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::from(MAGIC),
        &mut ciphertext,
    )
    .map_err(|_| anyhow!("Cannot encrypt local profile data"))?;
    let mut result = Vec::with_capacity(20 + ciphertext.len());
    result.extend_from_slice(MAGIC);
    result.extend_from_slice(&nonce);
    result.extend_from_slice(&ciphertext);
    Ok(result)
}
pub(crate) fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        data.len() >= 36 && data.starts_with(MAGIC),
        "Unsupported or damaged Linux encrypted profile data"
    );
    let key = cipher(false)?;
    let nonce: [u8; 12] = data[8..20].try_into()?;
    let mut ciphertext = data[20..].to_vec();
    let plaintext = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(MAGIC),
            &mut ciphertext,
        )
        .map_err(|_| anyhow!("Cannot decrypt local profile data: key mismatch or damaged file"))?;
    Ok(plaintext.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_encryption_rejects_tampering_and_uses_fresh_nonces() {
        let first = protect(b"linux-fixture-secret").unwrap();
        let second = protect(b"linux-fixture-secret").unwrap();
        assert_ne!(first, second);
        assert_eq!(unprotect(&first).unwrap(), b"linux-fixture-secret");
        let mut tampered = first.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert!(unprotect(&tampered).is_err());
        assert!(unprotect(&first[..20]).is_err());
        assert!(unprotect(b"plaintext must never be accepted").is_err());
    }
    #[test]
    fn keyring_child_process() {
        if let Some(file) = std::env::var_os("REQON_TEST_SECRET_FILE") {
            assert_eq!(
                unprotect(&std::fs::read(file).unwrap()).unwrap(),
                b"cross-process-fixture"
            );
        }
    }
    #[test]
    fn keyring_key_survives_a_new_process() {
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("ciphertext.bin");
        std::fs::write(&file, protect(b"cross-process-fixture").unwrap()).unwrap();
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "secrets::linux::tests::keyring_child_process"])
            .env("REQON_TEST_SECRET_FILE", file)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
