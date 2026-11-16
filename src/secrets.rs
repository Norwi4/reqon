use crate::model::{Pair, Workspace};
use anyhow::{Context, Result};
use std::{collections::BTreeMap, path::Path};

fn visit(ws: &Workspace, mut f: impl FnMut(String, &Pair)) {
    for p in &ws.globals {
        f(format!("global:{}", p.key), p);
    }
    for e in &ws.environments {
        for p in &e.vars {
            f(format!("workspace-env:{}:{}", e.name, p.key), p);
        }
    }
    for c in &ws.collections {
        for p in &c.vars {
            f(format!("collection:{}:{}", c.path.display(), p.key), p);
        }
        for e in &c.environments {
            for p in &e.vars {
                f(format!("env:{}:{}:{}", c.path.display(), e.name, p.key), p);
            }
        }
    }
    for r in ws.requests.iter().chain(ws.collection_drafts.values()) {
        for p in &r.vars {
            f(format!("request:{}:{}", r.id, p.key), p);
        }
    }
}
pub fn save(root: &Path, ws: &Workspace) -> Result<()> {
    let mut values = BTreeMap::<String, String>::new();
    visit(ws, |key, p| {
        if p.secret {
            values.insert(key, p.value.clone());
        }
    });
    for (key, value) in &ws.preferences.credentials {
        values.insert(format!("preferences:{key}"), value.clone());
    }
    let path = root.join("secrets.bin");
    if values.is_empty() && !path.exists() {
        return Ok(());
    }
    let plaintext = serde_json::to_vec(&values)?;
    let encrypted = protect(&plaintext)?;
    crate::storage::atomic_write(&path, encrypted)
}
pub fn restore(root: &Path, ws: &mut Workspace) -> Result<()> {
    let path = root.join("secrets.bin");
    if !path.exists() {
        return Ok(());
    }
    let data = unprotect(&std::fs::read(path)?)?;
    let values: BTreeMap<String, String> =
        serde_json::from_slice(&data).context("Cannot read local secret values")?;
    for (key, value) in &values {
        if let Some(key) = key.strip_prefix("preferences:") {
            ws.preferences
                .credentials
                .insert(key.to_owned(), value.clone());
        }
    }
    let restore = |key: String, p: &mut Pair| {
        if p.secret {
            if let Some(value) = values.get(&key) {
                p.value = value.clone();
            }
        }
    };
    for p in &mut ws.globals {
        restore(format!("global:{}", p.key), p);
    }
    for e in &mut ws.environments {
        for p in &mut e.vars {
            restore(format!("workspace-env:{}:{}", e.name, p.key), p);
        }
    }
    for c in &mut ws.collections {
        for p in &mut c.vars {
            restore(format!("collection:{}:{}", c.path.display(), p.key), p);
        }
        for e in &mut c.environments {
            for p in &mut e.vars {
                restore(format!("env:{}:{}:{}", c.path.display(), e.name, p.key), p);
            }
        }
    }
    for r in ws
        .requests
        .iter_mut()
        .chain(ws.collection_drafts.values_mut())
    {
        for p in &mut r.vars {
            restore(format!("request:{}:{}", r.id, p.key), p);
        }
    }
    Ok(())
}
#[cfg(target_os = "windows")]
fn crypt(bytes: &[u8], encrypt: bool) -> Result<Vec<u8>> {
    #[repr(C)]
    struct Blob {
        len: u32,
        data: *mut u8,
    }
    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut std::ffi::c_void,
            prompt: *const std::ffi::c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(memory: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
    }
    let input = Blob {
        len: bytes.len().try_into()?,
        data: bytes.as_ptr().cast_mut(),
    };
    let mut output = Blob {
        len: 0,
        data: std::ptr::null_mut(),
    };
    let success = unsafe {
        if encrypt {
            CryptProtectData(
                &input,
                std::ptr::null(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                1,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null(),
                1,
                &mut output,
            )
        }
    };
    if success == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Windows could not protect/unprotect local secret values");
    }
    let result = unsafe { std::slice::from_raw_parts(output.data, output.len as usize).to_vec() };
    unsafe {
        LocalFree(output.data.cast());
    }
    Ok(result)
}
#[cfg(target_os = "windows")]
pub(crate) fn protect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, true)
}
#[cfg(target_os = "windows")]
pub(crate) fn unprotect(data: &[u8]) -> Result<Vec<u8>> {
    crypt(data, false)
}
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{protect, unprotect};
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(crate) fn protect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("Secret storage requires the Windows credential protection service")
}
#[cfg(not(any(target_os = "windows", target_os = "linux")))]
pub(crate) fn unprotect(_: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("Secret storage requires the Windows credential protection service")
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::*;
    #[test]
    fn secrets_are_encrypted_and_restored_for_current_user() {
        let d = tempfile::tempdir().unwrap();
        let mut ws = crate::storage::load(d.path()).unwrap();
        let mut p = Pair::new("token", "fixture-private-token-42");
        p.secret = true;
        ws.collections[0].environments[0].vars.push(p);
        crate::storage::save(d.path(), &ws).unwrap();
        let plain = std::fs::read_to_string(d.path().join("workspace.json")).unwrap();
        assert!(!plain.contains("fixture-private-token-42"));
        let encrypted = std::fs::read(d.path().join("secrets.bin")).unwrap();
        assert!(!String::from_utf8_lossy(&encrypted).contains("fixture-private-token-42"));
        let loaded = crate::storage::load(d.path()).unwrap();
        assert_eq!(
            loaded.collections[0].environments[0]
                .vars
                .last()
                .unwrap()
                .value,
            "fixture-private-token-42"
        );
    }
}
