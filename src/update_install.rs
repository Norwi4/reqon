//! A separate copy of Reqon waits for the UI process to exit, then runs Inno
//! Setup silently. No shell command is constructed and no process is killed.
use crate::updates::{Download, Version};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const REGISTRATION: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{D860FEA6-4908-4C53-BCA8-51662A1D759B}_is1";
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Installation {
    pub executable: PathBuf,
    pub directory: PathBuf,
    pub version: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Pending {
    pub download: Download,
    pub installation: Installation,
    pub data_root: PathBuf,
    pub parent_id: u32,
}
pub type Handoff = std::sync::Arc<std::sync::Mutex<Option<Pending>>>;
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Outcome {
    pub success: bool,
    pub message: String,
}
pub fn take_outcome(root: &Path) -> Option<Outcome> {
    let path = root.join("update-result.json");
    let bytes = fs::read(&path).ok()?;
    let result = serde_json::from_slice(&bytes).ok();
    let _ = fs::remove_file(path);
    result
}
fn record(pending: &Pending, outcome: &Outcome) -> Result<()> {
    crate::storage::atomic_write(
        &pending.data_root.join("update-result.json"),
        serde_json::to_vec(outcome)?,
    )
}
pub fn cache_root() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("Reqon/updates"))
        .context("The Windows update cache is unavailable")
}
pub fn cleanup_cache() {
    let Ok(root) = cache_root() else { return };
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name
            .to_str()
            .and_then(|s| s.strip_prefix("reqon-update-"))
            .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok())
        {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() || kind.is_symlink() {
            continue;
        }
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| at.elapsed().ok())
            .is_some_and(|age| age > std::time::Duration::from_secs(7 * 24 * 60 * 60));
        if old {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}
pub fn installed() -> Result<Installation> {
    #[cfg(windows)]
    {
        windows::installed_at(&std::env::current_exe()?, REGISTRATION)
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!(
            "In-app installation is available for the Windows installer edition. Download this release to update your installation."
        )
    }
}
pub fn launch(pending: Pending) {
    #[cfg(windows)]
    if let Err(error) = windows::spawn_helper(&pending) {
        let recorded = record(
            &pending,
            &Outcome {
                success: false,
                message: format!("Could not start the update: {error:#}"),
            },
        )
        .is_ok();
        let restarted = windows::restart(&pending);
        if !recorded || restarted.is_err() {
            rfd::MessageDialog::new()
                .set_title("Reqon update")
                .set_description(format!(
                    "Could not start the update: {error:#}\n\n{}",
                    restarted
                        .err()
                        .map(|e| format!("Please reopen Reqon. {e:#}"))
                        .unwrap_or_else(|| "Reqon has been reopened. Please try again.".into())
                ))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
    #[cfg(not(windows))]
    let _ = pending;
}
/// Called before the application creates any window or opens a workspace.
pub fn run_helper(args: &[String]) -> bool {
    if args.get(1).map(String::as_str) != Some("--apply-update") {
        return false;
    }
    #[cfg(windows)]
    {
        let result = (|| -> Result<()> {
            ensure!(args.len() == 3, "Invalid update helper arguments");
            let plan_path = fs::canonicalize(&args[2])?;
            let cache = fs::canonicalize(cache_root()?)?;
            ensure!(
                plan_path.starts_with(&cache)
                    && plan_path
                        .file_name()
                        .is_some_and(|n| n == "update-plan.json"),
                "Invalid update plan location"
            );
            let directory = plan_path.parent().context("Invalid update directory")?;
            ensure!(
                same_path(
                    &std::env::current_exe()?,
                    &directory.join("Reqon-update-helper.exe")
                )?,
                "Invalid update helper location"
            );
            let bytes = fs::read(&plan_path)?;
            ensure!(bytes.len() < 64 * 1024, "Update plan is too large");
            let pending: Pending = serde_json::from_slice(&bytes)?;
            ensure!(
                same_path(
                    pending
                        .download
                        .installer
                        .parent()
                        .context("Missing installer directory")?,
                    directory
                )?,
                "Invalid installer location"
            );
            windows::finish(&pending, REGISTRATION)
        })();
        if let Err(error) = result {
            rfd::MessageDialog::new()
                .set_title("Reqon update")
                .set_description(format!(
                    "The update could not finish. Please reopen Reqon.\n\n{error:#}"
                ))
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
    }
    true
}
fn same_path(a: &Path, b: &Path) -> Result<bool> {
    Ok(fs::canonicalize(a)?
        .to_string_lossy()
        .eq_ignore_ascii_case(&fs::canonicalize(b)?.to_string_lossy()))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        ffi::OsStr,
        os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, process::CommandExt},
        process::{Command, Stdio},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, ERROR_INVALID_PARAMETER, ERROR_SUCCESS, WAIT_OBJECT_0},
        System::{
            Registry::{
                HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY, RegGetValueW,
                RegSetKeyValueW,
            },
            Threading::{
                CREATE_NO_WINDOW, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
                PROCESS_SYNCHRONIZE, QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
    };
    fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }
    pub(super) fn registry(key: &str, name: &str) -> Result<String> {
        let key = wide(key);
        let name = wide(name);
        let mut bytes = 0;
        let flags = RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY;
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        ensure!(
            result == ERROR_SUCCESS && bytes > 0 && bytes < 65536,
            "Use the Windows-installed copy of Reqon to update in the app."
        );
        let mut buffer = vec![0u16; bytes as usize / 2];
        let result = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut bytes,
            )
        };
        ensure!(
            result == ERROR_SUCCESS,
            "Could not read the Reqon installation"
        );
        Ok(String::from_utf16_lossy(
            &buffer[..buffer.iter().position(|v| *v == 0).unwrap_or(buffer.len())],
        ))
    }
    pub(super) fn installed_at(executable: &Path, key: &str) -> Result<Installation> {
        let directory = PathBuf::from(registry(key, "Inno Setup: App Path")?);
        ensure!(directory.is_absolute(), "Invalid installation directory");
        ensure!(
            same_path(executable, &directory.join("Reqon.exe"))?,
            "This is a portable or development copy. Open the installed Reqon app to update automatically."
        );
        ensure!(
            directory.join("unins000.exe").is_file() && !directory.join("portable.flag").exists(),
            "This copy does not support automatic installation"
        );
        let version = registry(key, "DisplayVersion")?;
        Version::parse(&version)?;
        Ok(Installation {
            executable: directory.join("Reqon.exe"),
            directory,
            version,
        })
    }
    fn command(executable: &Path) -> Command {
        let mut command = Command::new(executable);
        command
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    pub(super) fn spawn_helper(pending: &Pending) -> Result<()> {
        let directory = pending
            .download
            .installer
            .parent()
            .context("Missing update directory")?;
        let installed = installed_at(&pending.installation.executable, REGISTRATION)?;
        ensure!(
            same_path(&installed.directory, &pending.installation.directory)?,
            "The installation location changed"
        );
        let helper = directory.join("Reqon-update-helper.exe");
        fs::copy(&pending.installation.executable, &helper)
            .context("Could not prepare the updater")?;
        let plan = directory.join("update-plan.json");
        crate::storage::atomic_write(&plan, serde_json::to_vec(pending)?)?;
        command(&helper)
            .arg("--apply-update")
            .arg(plan)
            .spawn()
            .context("Could not launch the updater")?;
        Ok(())
    }
    fn wait_for_parent(pending: &Pending) -> Result<()> {
        ensure!(
            pending.parent_id != std::process::id() && pending.parent_id > 0,
            "Invalid parent process"
        );
        let handle = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                pending.parent_id,
            )
        };
        if handle.is_null() {
            let error = std::io::Error::last_os_error();
            ensure!(
                error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32),
                "Could not wait for Reqon to close: {error}"
            );
            return Ok(());
        }
        let result = (|| {
            let mut name = vec![0u16; 32768];
            let mut length = name.len() as u32;
            ensure!(
                unsafe { QueryFullProcessImageNameW(handle, 0, name.as_mut_ptr(), &mut length) }
                    != 0,
                "Could not identify the closing app"
            );
            let path = PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
            // A reused PID is not our old process. Never wait on or stop it.
            if !same_path(&path, &pending.installation.executable)? {
                return Ok(());
            }
            ensure!(
                unsafe { WaitForSingleObject(handle, 120_000) } == WAIT_OBJECT_0,
                "Reqon did not finish closing. Please retry the update."
            );
            Ok(())
        })();
        unsafe {
            CloseHandle(handle);
        }
        result
    }
    fn installer_command(pending: &Pending) -> Command {
        let mut result = command(&pending.download.installer);
        result
            .args([
                "/VERYSILENT",
                "/SUPPRESSMSGBOXES",
                "/NORESTART",
                "/NOCLOSEAPPLICATIONS",
                "/NOFORCECLOSEAPPLICATIONS",
                "/NORESTARTAPPLICATIONS",
                "/RESTARTEXITCODE=3010",
            ])
            .arg(format!("/DIR={}", pending.installation.directory.display()))
            .arg(format!(
                "/LOG={}",
                pending
                    .download
                    .installer
                    .with_file_name("install.log")
                    .display()
            ));
        result
    }
    fn apply(pending: &Pending, key: &str) -> Result<()> {
        let installation = installed_at(&pending.installation.executable, key)?;
        ensure!(
            same_path(&installation.directory, &pending.installation.directory)?,
            "The installation location changed"
        );
        ensure!(
            Version::parse(&pending.download.release.version)?
                > Version::parse(&installation.version)?,
            "The offered update is not newer than the installed version"
        );
        crate::updates::verify_file(
            &pending.download.installer,
            pending.download.release.installer.size,
            &pending.download.release.installer.sha256,
        )?;
        let backup = pending
            .download
            .installer
            .with_file_name("previous-binaries");
        fs::create_dir_all(&backup)?;
        for name in ["Reqon.exe", "reqon-cli.exe"] {
            let file = installation.directory.join(name);
            // Refuse to proceed if another Reqon/CLI process is using a binary.
            let unlocked = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .share_mode(0)
                .open(&file)
                .context(
                    "Close other Reqon windows and running CLI commands, then retry the update",
                )?;
            drop(unlocked);
            fs::copy(&file, backup.join(name))?;
        }
        let result = (|| {
            let status = installer_command(pending)
                .status()
                .context("Could not start the installer")?;
            ensure!(
                status.success(),
                "The installer did not complete (code {}). See {}",
                status.code().unwrap_or(-1),
                pending
                    .download
                    .installer
                    .with_file_name("install.log")
                    .display()
            );
            ensure!(
                registry(key, "DisplayVersion")? == pending.download.release.version,
                "The installer did not activate the expected version"
            );
            Ok(())
        })();
        if result.is_err() {
            // Keep the old executable pair usable even after a partial installer failure.
            // Inno owns the other installation files; user workspaces are never copied.
            for name in ["Reqon.exe", "reqon-cli.exe"] {
                fs::copy(backup.join(name), installation.directory.join(name)).context("Could not restore the previous application. The backup is in the update cache.")?;
            }
            let key = wide(key);
            let value = wide("DisplayVersion");
            let version = wide(&installation.version);
            let restored = unsafe {
                RegSetKeyValueW(
                    HKEY_CURRENT_USER,
                    key.as_ptr(),
                    value.as_ptr(),
                    REG_SZ,
                    version.as_ptr().cast(),
                    (version.len() * 2) as u32,
                )
            };
            ensure!(
                restored == ERROR_SUCCESS,
                "Could not restore the previous version registration"
            );
        } else {
            let _ = fs::remove_dir_all(&backup);
        }
        result
    }
    pub(super) fn restart(pending: &Pending) -> Result<()> {
        command(&pending.installation.executable)
            .arg("--data-dir")
            .arg(&pending.data_root)
            .current_dir(&pending.installation.directory)
            .spawn()
            .context("Could not reopen Reqon")?;
        Ok(())
    }
    pub(super) fn finish(pending: &Pending, key: &str) -> Result<()> {
        wait_for_parent(pending)?;
        let result = apply(pending, key);
        let outcome = Outcome {
            success: result.is_ok(),
            message: match result {
                Ok(()) => format!(
                    "Reqon {} was installed successfully.",
                    pending.download.release.version
                ),
                Err(error) => format!("Update could not finish: {error:#}"),
            },
        };
        let recorded = record(pending, &outcome);
        restart(pending)?;
        recorded.context("Reqon reopened, but the update result could not be saved")?;
        if outcome.success {
            let _ = fs::remove_file(&pending.download.installer);
        }
        Ok(())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn installer_arguments_are_separate_and_never_force_close_or_reboot() {
            let pending = Pending {
                download: Download {
                    release: crate::updates::Release {
                        version: "0.24.0".into(),
                        notes_url: String::new(),
                        installer: crate::updates::Asset {
                            name: String::new(),
                            url: String::new(),
                            size: 1,
                            sha256: "a".repeat(64),
                        },
                        checksums: crate::updates::Asset {
                            name: String::new(),
                            url: String::new(),
                            size: 1,
                            sha256: "a".repeat(64),
                        },
                    },
                    installer: PathBuf::from(r"C:\Update cache & files\setup.exe"),
                },
                installation: Installation {
                    directory: PathBuf::from(r"C:\My Apps & Stuff\Reqon"),
                    executable: PathBuf::from(r"C:\My Apps & Stuff\Reqon\Reqon.exe"),
                    version: "0.23.0".into(),
                },
                data_root: PathBuf::from(r"C:\Profiles & drafts"),
                parent_id: 1,
            };
            let cmd = installer_command(&pending);
            let args: Vec<_> = cmd
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            assert!(args.contains(&"/DIR=C:\\My Apps & Stuff\\Reqon".into()));
            assert!(args.contains(&"/NORESTART".into()));
            assert!(args.contains(&"/NOCLOSEAPPLICATIONS".into()));
            assert!(
                !args
                    .iter()
                    .any(|s| s == "/FORCECLOSEAPPLICATIONS" || s == "/CLOSEAPPLICATIONS")
            );
        }
        #[test]
        #[ignore = "requires isolated installers prepared by the local QA harness"]
        fn isolated_silent_upgrade_and_failure_recovery() {
            use sha2::{Digest, Sha256};
            let root =
                PathBuf::from(std::env::var_os("REQON_UPDATER_QA_DIR").expect("QA directory"));
            let key = std::env::var("REQON_UPDATER_QA_KEY").expect("isolated QA registry key");
            assert!(key.contains("ReqonUpdaterQA"));
            let executable = root.join("installed/Reqon.exe");
            let install = installed_at(&executable, &key).unwrap();
            assert_eq!(install.version, "0.22.0");
            let plan = |name: &str, version: &str| {
                let installer = root.join(name);
                let bytes = fs::read(&installer).unwrap();
                let asset = crate::updates::Asset {
                    name: name.into(),
                    url: String::new(),
                    size: bytes.len() as u64,
                    sha256: format!("{:x}", Sha256::digest(&bytes)),
                };
                Pending {
                    download: Download {
                        release: crate::updates::Release {
                            version: version.into(),
                            notes_url: String::new(),
                            installer: asset.clone(),
                            checksums: asset,
                        },
                        installer,
                    },
                    installation: install.clone(),
                    data_root: root.join("profile"),
                    parent_id: 1,
                }
            };
            let sentinel = root.join("installed/user-data.req");
            fs::write(&sentinel, "unsaved user collection").unwrap();
            apply(&plan("upgrade.exe", "0.23.0"), &key).unwrap();
            assert_eq!(registry(&key, "DisplayVersion").unwrap(), "0.23.0");
            assert_eq!(
                fs::read_to_string(&sentinel).unwrap(),
                "unsaved user collection"
            );
            let before = fs::read(&executable).unwrap();
            let cli = install.directory.join("reqon-cli.exe");
            let locked = fs::OpenOptions::new()
                .read(true)
                .share_mode(0)
                .open(&cli)
                .unwrap();
            let error = apply(&plan("abort.exe", "0.24.0"), &key).unwrap_err();
            assert!(format!("{error:#}").contains("Close other Reqon"));
            drop(locked);
            assert!(apply(&plan("abort.exe", "0.24.0"), &key).is_err());
            assert_eq!(fs::read(&executable).unwrap(), before);
            assert_eq!(registry(&key, "DisplayVersion").unwrap(), "0.23.0");
            assert_eq!(
                fs::read_to_string(&sentinel).unwrap(),
                "unsaved user collection"
            );
        }
    }
}
