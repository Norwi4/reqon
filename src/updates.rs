//! Release discovery and verified downloads. This client never inherits request
//! credentials, cookies, or the API client's option to disable TLS validation.
use anyhow::{Context, Result, bail, ensure};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub const RELEASES: &str = "https://github.com/Norwi4/reqon/releases";
const LATEST: &str = "https://api.github.com/repos/Norwi4/reqon/releases/latest";
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version(pub [u64; 3]);
impl Version {
    pub fn parse(text: &str) -> Result<Self> {
        let parts: Vec<_> = text.split('.').collect();
        ensure!(parts.len() == 3, "Invalid release version");
        let mut version = [0; 3];
        for (index, part) in parts.iter().enumerate() {
            ensure!(
                !part.is_empty()
                    && part.bytes().all(|c| c.is_ascii_digit())
                    && (part.len() == 1 || !part.starts_with('0')),
                "Invalid release version"
            );
            version[index] = part.parse().context("Invalid release version")?;
        }
        Ok(Self(version))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Release {
    pub version: String,
    pub notes_url: String,
    pub installer: Asset,
    pub checksums: Asset,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Download {
    pub release: Release,
    pub installer: PathBuf,
}
#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ApiAsset>,
}
#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    size: u64,
    digest: Option<String>,
    state: String,
}
fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn asset(api: &ApiRelease, version: &str, name: String, max: u64) -> Result<Asset> {
    let matches: Vec<_> = api.assets.iter().filter(|a| a.name == name).collect();
    ensure!(
        matches.len() == 1,
        "The release does not contain one verified {name}"
    );
    let found = matches[0];
    let url = format!("{RELEASES}/download/v{version}/{name}");
    ensure!(
        found.browser_download_url == url,
        "Unexpected release download address"
    );
    ensure!(
        found.state == "uploaded" && found.size > 0 && found.size <= max,
        "Invalid release asset size or state"
    );
    let hash = found
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .filter(|s| valid_hash(s))
        .context("The release has no valid SHA-256 checksum")?;
    Ok(Asset {
        name,
        url,
        size: found.size,
        sha256: hash.to_ascii_lowercase(),
    })
}
fn parse_release(bytes: &[u8], current: &str) -> Result<Option<Release>> {
    let api: ApiRelease = serde_json::from_slice(bytes).context("Invalid release response")?;
    ensure!(
        !api.draft && !api.prerelease,
        "The server returned a preview release"
    );
    let version = api
        .tag_name
        .strip_prefix('v')
        .context("Invalid release tag")?;
    if Version::parse(version)? <= Version::parse(current)? {
        return Ok(None);
    }
    Ok(Some(Release {
        version: version.into(),
        notes_url: format!("{RELEASES}/tag/v{version}"),
        installer: asset(
            &api,
            version,
            format!("Reqon-Setup-{version}.exe"),
            MAX_INSTALLER,
        )?,
        checksums: asset(
            &api,
            version,
            format!("Reqon-{version}-SHA256SUMS.txt"),
            64 * 1024,
        )?,
    }))
}
fn client() -> Result<Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(Client::builder()
        .user_agent(concat!("Reqon/", env!("CARGO_PKG_VERSION"), " updater"))
        .connect_timeout(Duration::from_secs(10))
        // The blocking client applies this to each read, so slow downloads may
        // continue while a stalled connection (including cancellation) is bounded.
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            if attempt.previous().len() >= 5
                || url.scheme() != "https"
                || !matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.error("Untrusted update redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}
pub fn check(current: &str) -> Result<Option<Release>> {
    let response = client()?
        .get(LATEST)
        .timeout(Duration::from_secs(25))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2026-03-10")
        .send()
        .context("Could not reach the update server")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    if matches!(response.status().as_u16(), 403 | 429) {
        bail!("The update server is busy. Please try again later.");
    }
    let mut bytes = Vec::new();
    response
        .error_for_status()?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "Release response is too large");
    parse_release(&bytes, current)
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Download cancelled");
    Ok(())
}
fn read_checked(
    mut reader: impl Read,
    mut writer: impl Write,
    asset: &Asset,
    cancel: &AtomicBool,
    mut progress: impl FnMut(u64),
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut received = 0;
    let mut buffer = [0; 64 * 1024];
    loop {
        cancelled(cancel)?;
        let count = reader
            .read(&mut buffer)
            .context("Update download was interrupted")?;
        if count == 0 {
            break;
        }
        received += count as u64;
        ensure!(
            received <= asset.size,
            "Update download exceeded the expected size"
        );
        hash.update(&buffer[..count]);
        writer.write_all(&buffer[..count])?;
        progress(received);
    }
    cancelled(cancel)?;
    ensure!(received == asset.size, "Update download is incomplete");
    ensure!(
        format!("{:x}", hash.finalize()) == asset.sha256,
        "Update checksum mismatch. The file will not be run."
    );
    Ok(())
}
fn verify_manifest(bytes: &[u8], installer: &Asset) -> Result<()> {
    let text = std::str::from_utf8(bytes).context("Invalid checksum manifest")?;
    let matches: Vec<_> = text
        .lines()
        .filter_map(|line| line.split_once("  "))
        .filter(|(_, name)| *name == installer.name)
        .collect();
    ensure!(
        matches.len() == 1 && matches[0].0 == installer.sha256,
        "Installer checksum does not match the release manifest"
    );
    Ok(())
}
pub fn download(
    release: Release,
    cache: &Path,
    cancel: &AtomicBool,
    progress: impl FnMut(u64),
) -> Result<Download> {
    Version::parse(&release.version)?;
    let client = client()?;
    let mut manifest = Vec::new();
    let response = client
        .get(&release.checksums.url)
        .timeout(Duration::from_secs(30))
        .send()?
        .error_for_status()?;
    read_checked(response, &mut manifest, &release.checksums, cancel, |_| {})?;
    verify_manifest(&manifest, &release.installer)?;
    cancelled(cancel)?;
    fs::create_dir_all(cache)?;
    let directory = cache.join(format!("reqon-update-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory)?;
    let partial = directory.join("installer.part");
    let installer = directory.join(&release.installer.name);
    let result = (|| {
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        let response = client
            .get(&release.installer.url)
            .send()?
            .error_for_status()?;
        read_checked(response, &mut output, &release.installer, cancel, progress)?;
        output.sync_all()?;
        drop(output);
        let mut magic = [0; 2];
        fs::File::open(&partial)?.read_exact(&mut magic)?;
        ensure!(magic == *b"MZ", "The update is not a Windows installer");
        fs::rename(&partial, &installer)?;
        Ok(Download { release, installer })
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&directory);
    }
    result
}
pub fn verify_file(path: &Path, size: u64, sha256: &str) -> Result<()> {
    ensure!(
        valid_hash(sha256) && size > 0 && size <= MAX_INSTALLER,
        "Invalid installer checksum or size"
    );
    let input = fs::File::open(path)?;
    read_checked(
        input,
        std::io::sink(),
        &Asset {
            name: String::new(),
            url: String::new(),
            size,
            sha256: sha256.into(),
        },
        &AtomicBool::new(false),
        |_| {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> serde_json::Value {
        serde_json::json!({"tag_name":"v0.23.0","draft":false,"prerelease":false,"assets":[
            {"name":"Reqon-Setup-0.23.0.exe","browser_download_url":format!("{RELEASES}/download/v0.23.0/Reqon-Setup-0.23.0.exe"),"size":42,"digest":format!("sha256:{}", "a".repeat(64)),"state":"uploaded"},
            {"name":"Reqon-0.23.0-SHA256SUMS.txt","browser_download_url":format!("{RELEASES}/download/v0.23.0/Reqon-0.23.0-SHA256SUMS.txt"),"size":100,"digest":format!("sha256:{}", "b".repeat(64)),"state":"uploaded"}
        ]})
    }
    #[test]
    fn versions_are_numeric_and_reject_paths_and_previews() {
        assert!(Version::parse("0.10.0").unwrap() > Version::parse("0.9.9").unwrap());
        for value in [
            "1.2",
            "1.2.3.4",
            "01.2.3",
            "1.2.3-beta",
            "../1.2.3",
            "1.2.3 /SILENT",
            "-1.2.3",
        ] {
            assert!(Version::parse(value).is_err(), "{value}");
        }
    }
    #[test]
    fn stable_release_requires_exact_assets_and_checksums() {
        let encode = |v: &serde_json::Value| serde_json::to_vec(v).unwrap();
        assert!(
            parse_release(&encode(&fixture()), "0.22.0")
                .unwrap()
                .is_some()
        );
        assert!(
            parse_release(&encode(&fixture()), "0.23.0")
                .unwrap()
                .is_none()
        );
        assert!(
            parse_release(&encode(&fixture()), "0.24.0")
                .unwrap()
                .is_none()
        );
        for key in ["draft", "prerelease"] {
            let mut v = fixture();
            v[key] = true.into();
            assert!(parse_release(&encode(&v), "0.22.0").is_err());
        }
        for field in ["browser_download_url", "digest", "state"] {
            let mut v = fixture();
            v["assets"][0][field] = "invalid".into();
            assert!(parse_release(&encode(&v), "0.22.0").is_err());
        }
        let mut v = fixture();
        let duplicate = v["assets"][0].clone();
        v["assets"].as_array_mut().unwrap().push(duplicate);
        assert!(parse_release(&encode(&v), "0.22.0").is_err());
    }
    #[test]
    fn transfer_rejects_truncation_extra_bytes_tampering_and_cancellation() {
        let data = b"MZverified installer fixture";
        let asset = Asset {
            name: "test.exe".into(),
            url: String::new(),
            size: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
        };
        assert!(
            read_checked(
                &data[..],
                std::io::sink(),
                &asset,
                &AtomicBool::new(false),
                |_| {}
            )
            .is_ok()
        );
        for bytes in [
            &data[..data.len() - 1],
            b"MZtampered installer fixture".as_slice(),
            b"MZverified installer fixture extra".as_slice(),
        ] {
            assert!(
                read_checked(
                    bytes,
                    std::io::sink(),
                    &asset,
                    &AtomicBool::new(false),
                    |_| {}
                )
                .is_err()
            );
        }
        assert!(
            read_checked(
                &data[..],
                std::io::sink(),
                &asset,
                &AtomicBool::new(true),
                |_| {}
            )
            .is_err()
        );
        let manifest = format!("{}  test.exe\n", asset.sha256);
        assert!(verify_manifest(manifest.as_bytes(), &asset).is_ok());
        assert!(verify_manifest(format!("{manifest}{manifest}").as_bytes(), &asset).is_err());
        assert!(verify_manifest(b"incorrect  test.exe\n", &asset).is_err());
    }
    #[test]
    #[ignore = "downloads the current public installer without executing it"]
    fn live_release_download_is_verified() {
        let release = check("0.0.0").unwrap().expect("published stable release");
        let directory = tempfile::tempdir().unwrap();
        let download =
            download(release, directory.path(), &AtomicBool::new(false), |_| {}).unwrap();
        verify_file(
            &download.installer,
            download.release.installer.size,
            &download.release.installer.sha256,
        )
        .unwrap();
    }
}
