//! Machine-local, encrypted authentication values. References are immutable and
//! independent of a request's name/path, so moving or copying files is safe.
use crate::model::{Auth, Request};
use anyhow::{Context, Result, bail, ensure};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

const PREFIX: &str = "{{reqonSecret:";
static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn root() -> Result<PathBuf> {
    #[cfg(test)]
    {
        static ROOT: std::sync::OnceLock<tempfile::TempDir> = std::sync::OnceLock::new();
        return Ok(ROOT
            .get_or_init(|| tempfile::tempdir().expect("test vault"))
            .path()
            .into());
    }
    #[cfg(not(test))]
    {
        if let Some(path) = std::env::var_os("REQON_VAULT_DIR") {
            ensure!(!path.is_empty(), "REQON_VAULT_DIR must not be empty");
            return Ok(PathBuf::from(path));
        }
        #[cfg(target_os = "windows")]
        let base = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(not(target_os = "windows"))]
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")));
        Ok(base
            .context("Cannot locate the local authentication vault")?
            .join("Reqon/auth-vault"))
    }
}

pub(crate) fn reference_id(value: &str) -> Option<&str> {
    let id = value.strip_prefix(PREFIX)?.strip_suffix("}}")?;
    (id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit())).then_some(id)
}

pub(crate) fn secret_field(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace(['_', '-'], "");
    matches!(
        normalized.as_str(),
        "password"
            | "clientsecret"
            | "clienttoken"
            | "key"
            | "secret"
            | "secretkey"
            | "secretaccesskey"
            | "accesskeyid"
            | "accesstoken"
            | "refreshtoken"
            | "token"
            | "tokensecret"
            | "consumersecret"
            | "sessiontoken"
            | "privatekey"
            | "codeverifier"
            | "clientkeypassword"
    )
}
fn fields(auth: &mut Auth, mut visit: impl FnMut(&str, &mut String) -> Result<()>) -> Result<()> {
    visit("token", &mut auth.token)?;
    visit("password", &mut auth.password)?;
    visit("API key", &mut auth.value)?;
    for (name, value) in &mut auth.fields {
        if secret_field(name) {
            visit(name, value)?;
        }
    }
    Ok(())
}

pub(crate) fn literal(value: &str) -> bool {
    !value.is_empty() && !(value.contains("{{") && value.contains("}}"))
}

/// Install a complete encrypted file without replacing an existing one. The
/// hard link publishes fully written bytes atomically, also across processes.
fn install(path: &Path, data: &[u8]) -> Result<()> {
    let parent = path.parent().context("Invalid vault path")?;
    fs::create_dir_all(parent)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    }
    let temporary = parent.join(format!(".{}.tmp", crate::model::id()));
    let result = (|| -> Result<()> {
        use std::io::Write;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => Err(e.into()),
        }
    })();
    let _ = fs::remove_file(temporary);
    result
}

pub(crate) fn store_at(root: &Path, value: &str) -> Result<String> {
    let _lock = WRITE_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Authentication vault lock failed"))?;
    let key_path = root.join("key.bin");
    if !key_path.exists() {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut key = [0u8; 32];
        SystemRandom::new()
            .fill(&mut key)
            .map_err(|_| anyhow::anyhow!("Cannot create vault key"))?;
        install(&key_path, &crate::secrets::protect(&key)?)?;
    }
    let key = crate::secrets::unprotect(&fs::read(&key_path)?)?;
    ensure!(key.len() == 32, "Invalid authentication vault key");
    let mut mac = Hmac::<Sha256>::new_from_slice(&key).expect("HMAC key length");
    mac.update(value.as_bytes());
    let id: String = mac
        .finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let path = root.join(format!("{id}.bin"));
    if !path.exists() {
        install(&path, &crate::secrets::protect(value.as_bytes())?)?;
    }
    // A missing/corrupt ciphertext must never be replaced by a dangling reference.
    ensure!(
        crate::secrets::unprotect(&fs::read(path)?)? == value.as_bytes(),
        "Authentication vault verification failed"
    );
    Ok(format!("{PREFIX}{id}}}}}"))
}

pub fn protect_at(root: &Path, request: &Request) -> Result<Request> {
    let mut result = request.clone();
    let mut replacements = BTreeMap::new();
    fields(&mut result.auth, |_, value| {
        if literal(value) {
            let reference = store_at(root, value)?;
            replacements.insert(value.clone(), reference.clone());
            *value = reference;
        }
        Ok(())
    })?;
    // Interchange metadata can contain a second copy of the original auth.
    replace_values(&mut result.portable, &replacements);
    if result.file_format == "yml" {
        if let Ok(mut original) = serde_yaml::from_str::<serde_json::Value>(&result.original) {
            replace_values(&mut original, &replacements);
            result.original = serde_yaml::to_string(&original)?;
        }
    }
    Ok(result)
}

pub fn protect(request: &Request) -> Result<Request> {
    let mut auth = request.auth.clone();
    let mut needed = false;
    fields(&mut auth, |_, value| {
        needed |= literal(value);
        Ok(())
    })?;
    if !needed {
        return Ok(request.clone());
    }
    protect_at(&root()?, request)
}

/// Source preview must never unlock a keyring or write files on the UI thread.
pub fn preview(request: &Request) -> Request {
    let mut result = request.clone();
    let mut replacements = BTreeMap::new();
    let _ = fields(&mut result.auth, |_, value| {
        if literal(value) {
            replacements.insert(value.clone(), "{{secret}}".into());
            *value = "{{secret}}".into();
        }
        Ok(())
    });
    replace_values(&mut result.portable, &replacements);
    if result.file_format == "yml" {
        if let Ok(mut original) = serde_yaml::from_str::<serde_json::Value>(&result.original) {
            replace_values(&mut original, &replacements);
            result.original = serde_yaml::to_string(&original).unwrap_or_default();
        }
    }
    result
}

pub fn resolve_at(root: &Path, request: &mut Request, required: bool) -> Result<()> {
    let mut replacements = BTreeMap::new();
    fields(&mut request.auth, |name, value| {
        if let Some(id) = reference_id(value) {
            let path = root.join(format!("{id}.bin"));
            if !path.exists() {
                if required {
                    bail!(
                        "Missing local secret for Auth {name}. Enter it in Auth on this computer."
                    );
                }
                return Ok(());
            }
            let plaintext = String::from_utf8(crate::secrets::unprotect(&fs::read(path)?)?)
                .context("Invalid authentication vault value")?;
            replacements.insert(value.clone(), plaintext.clone());
            *value = plaintext;
        } else if value.starts_with(PREFIX) && required {
            bail!("Invalid local secret reference in Auth {name}");
        }
        Ok(())
    })?;
    replace_values(&mut request.portable, &replacements);
    Ok(())
}

pub fn resolve(request: &mut Request, required: bool) -> Result<()> {
    let mut auth = request.auth.clone();
    let mut needed = false;
    fields(&mut auth, |_, value| {
        needed |= value.starts_with(PREFIX);
        Ok(())
    })?;
    if needed {
        resolve_at(&root()?, request, required)?;
    }
    Ok(())
}

pub fn resolve_with_variables(
    request: &mut Request,
    variables: &BTreeMap<String, String>,
) -> Result<()> {
    fields(&mut request.auth, |_, value| {
        if let Some(id) = reference_id(value) {
            if let Some(secret) = variables.get(&format!("reqonSecret:{id}")) {
                *value = secret.clone();
            }
        }
        Ok(())
    })?;
    resolve(request, true)
}

pub fn replace_values(value: &mut serde_json::Value, replacements: &BTreeMap<String, String>) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(next) = replacements.get(s) {
                *s = next.clone();
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                replace_values(value, replacements);
            }
        }
        serde_json::Value::Object(values) => {
            for value in values.values_mut() {
                replace_values(value, replacements);
            }
        }
        _ => {}
    }
}

pub fn request_values(request: &Request) -> Vec<String> {
    let mut values = Vec::new();
    let mut auth = request.auth.clone();
    let _ = fields(&mut auth, |_, value| {
        if literal(value) {
            values.push(value.clone());
        }
        Ok(())
    });
    for row in request
        .vars
        .iter()
        .chain(&request.headers)
        .chain(&request.params)
        .chain(&request.form)
        .chain(&request.inherited_collection_vars)
    {
        if row.secret && !row.value.is_empty() {
            values.push(row.value.clone());
        }
    }
    if auth.kind == "basic" && !auth.password.is_empty() {
        use base64::Engine;
        values.push(
            base64::prelude::BASE64_STANDARD.encode(format!("{}:{}", auth.username, auth.password)),
        );
    }
    values
}

pub fn execution_values(request: &Request, variables: &BTreeMap<String, String>) -> Vec<String> {
    let mut resolved = request.clone();
    let _ = fields(&mut resolved.auth, |_, value| {
        *value = crate::model::resolve(value, variables);
        Ok(())
    });
    resolved.auth.username = crate::model::resolve(&resolved.auth.username, variables);
    request_values(&resolved)
}

pub fn known_values(workspace: &crate::model::Workspace) -> Vec<String> {
    let mut values: Vec<_> = workspace
        .requests
        .iter()
        .chain(workspace.collection_drafts.values())
        .chain(workspace.history.iter().map(|h| &h.request))
        .flat_map(request_values)
        .collect();
    values.extend(
        workspace
            .history
            .iter()
            .flat_map(|h| h.reply.redaction_values.iter().cloned()),
    );
    for row in workspace
        .globals
        .iter()
        .chain(workspace.environments.iter().flat_map(|e| &e.vars))
        .chain(workspace.collections.iter().flat_map(|c| {
            c.vars
                .iter()
                .chain(c.environments.iter().flat_map(|e| &e.vars))
        }))
    {
        if row.secret && !row.value.is_empty() {
            values.push(row.value.clone());
        }
    }
    values.sort_by_key(|s| std::cmp::Reverse(s.len()));
    values.dedup();
    values
}

pub fn redact(value: &mut serde_json::Value, secrets: &[String]) {
    match value {
        serde_json::Value::String(s) => {
            *s = redact_text(s, secrets);
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact(value, secrets);
            }
        }
        serde_json::Value::Object(values) => {
            // Script mutations retain JSON types. Mask whole secret bindings
            // without changing status codes, passed flags or runner controls.
            if let Some(effects) = values.get_mut("script_effects") {
                for scope in ["environment", "globals", "collection"] {
                    if let Some(bindings) = effects.get_mut(scope).and_then(|v| v.as_object_mut()) {
                        for binding in bindings.values_mut() {
                            if !binding.is_string() && secrets.contains(&binding.to_string()) {
                                *binding = "[secret]".into();
                            }
                        }
                    }
                }
            }
            for value in values.values_mut() {
                redact(value, secrets);
            }
        }
        _ => {}
    }
}

fn redact_text(text: &str, secrets: &[String]) -> String {
    let mut output = String::new();
    let mut tail = text;
    while !tail.is_empty() {
        // Opaque references may contain short secrets by coincidence. Preserve
        // complete references even when embedded in serialized request source.
        let reference = tail.match_indices(PREFIX).find_map(|(start, rest)| {
            let end = tail[start + rest.len()..].find("}}")? + start + rest.len() + 2;
            reference_id(&tail[start..end]).map(|_| (start, end))
        });
        let end = reference.map(|(start, _)| start).unwrap_or(tail.len());
        let mut plain = &tail[..end];
        while let Some((index, secret)) = secrets
            .iter()
            .filter(|s| !s.is_empty())
            .filter_map(|secret| plain.find(secret.as_str()).map(|i| (i, secret)))
            .min_by(|(a, x), (b, y)| a.cmp(b).then(y.len().cmp(&x.len())))
        {
            output.push_str(&plain[..index]);
            output.push_str("[secret]");
            plain = &plain[index + secret.len()..];
        }
        output.push_str(plain);
        if let Some((start, finish)) = reference {
            output.push_str(&tail[start..finish]);
            tail = &tail[finish..];
        } else {
            break;
        }
    }
    output
}

#[cfg(all(test, any(target_os = "windows", target_os = "linux")))]
mod tests {
    use super::*;
    #[test]
    fn typed_secret_script_values_are_masked_without_changing_runner_controls() {
        let mut report = serde_json::json!({"passed":true,"response":{"status":200,
            "script_effects":{"environment":{"NUMBER":123456789,"FLAG":false,"JSON":{"key":"private"}},
            "skip":false,"stop":false}}});
        redact(
            &mut report,
            &[
                "123456789".into(),
                "false".into(),
                "{\"key\":\"private\"}".into(),
            ],
        );
        let effects = &report["response"]["script_effects"];
        assert_eq!(effects["environment"]["NUMBER"], "[secret]");
        assert_eq!(effects["environment"]["FLAG"], "[secret]");
        assert_eq!(effects["environment"]["JSON"], "[secret]");
        assert_eq!(effects["skip"], false);
        assert_eq!(effects["stop"], false);
        assert_eq!(report["passed"], true);
        assert_eq!(report["response"]["status"], 200);
    }
    #[test]
    fn redaction_preserves_embedded_references_and_masks_short_secrets() {
        let reference = format!("{PREFIX}{}}}}}", "abcdef12".repeat(8));
        let text = format!("password=ab; token={reference}; echo=abcdef12");
        let output = redact_text(&text, &["ab".into(), "abcdef12".into()]);
        assert_eq!(
            output,
            format!("password=[secret]; token={reference}; echo=[secret]")
        );
    }
    #[test]
    fn immutable_references_survive_moves_copies_and_token_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = Request::default();
        request.auth.kind = "bearer".into();
        request.auth.token = "private-auth-fixture-42".into();
        let first = protect_at(dir.path(), &request).unwrap();
        assert!(reference_id(&first.auth.token).is_some());
        assert_eq!(
            first.auth.token,
            protect_at(dir.path(), &request).unwrap().auth.token
        );
        request.auth.token = "rotated-fixture-43".into();
        let second = protect_at(dir.path(), &request).unwrap();
        assert_ne!(first.auth.token, second.auth.token);
        let mut moved = first.clone();
        moved.id = crate::model::id();
        moved.path = Some("different/folder.req".into());
        resolve_at(dir.path(), &mut moved, true).unwrap();
        assert_eq!(moved.auth.token, "private-auth-fixture-42");
        for entry in fs::read_dir(dir.path()).unwrap() {
            assert!(
                !String::from_utf8_lossy(&fs::read(entry.unwrap().path()).unwrap())
                    .contains("private-auth-fixture-42")
            );
        }
        let other = tempfile::tempdir().unwrap();
        assert!(
            resolve_at(other.path(), &mut first.clone(), true)
                .unwrap_err()
                .to_string()
                .contains("Missing local secret")
        );
    }
    #[test]
    fn templates_remain_portable_and_bad_vaults_do_not_erase_literals() {
        let dir = tempfile::tempdir().unwrap();
        let mut request = Request::default();
        request.auth.token = "{{TOKEN}}".into();
        assert_eq!(
            protect_at(dir.path(), &request).unwrap().auth.token,
            "{{TOKEN}}"
        );
        fs::write(dir.path().join("key.bin"), "invalid ciphertext").unwrap();
        request.auth.token = "must-survive".into();
        assert!(protect_at(dir.path(), &request).is_err());
        assert_eq!(request.auth.token, "must-survive");
    }

    #[test]
    fn disk_profile_export_and_history_hide_auth_but_execution_restores_it() {
        use crate::{
            model::{History, Reply},
            storage,
        };
        let dir = tempfile::tempdir().unwrap();
        let mut workspace = storage::load(dir.path()).unwrap();
        let token = "auth-storage-sentinel-9876";
        let request = &mut workspace.requests[0];
        request.auth.kind = "bearer".into();
        request.auth.token = token.into();
        request.original = storage::to_bru(request);
        request.portable = serde_json::json!({"_postman_auth":{"auth":{"token":token}}});
        storage::save_request(request).unwrap();
        let file = request.path.clone().unwrap();
        assert!(!fs::read_to_string(&file).unwrap().contains(token));
        assert_eq!(crate::yaml::read_request(&file).unwrap().auth.token, token);
        workspace.history.push(History {
            at: 1,
            request: request.clone(),
            reply: Reply {
                body: token.into(),
                logs: vec![format!("token={token}")],
                ..Default::default()
            },
        });
        storage::save(dir.path(), &workspace).unwrap();
        assert!(
            !fs::read_to_string(dir.path().join("workspace.json"))
                .unwrap()
                .contains(token)
        );
        let loaded = storage::load(dir.path()).unwrap();
        assert_eq!(loaded.requests[0].auth.token, token);
        assert_eq!(loaded.history[0].reply.body, "[secret]");
        let exported =
            storage::export_postman(&workspace.collections[0], &workspace.requests).unwrap();
        assert!(!exported.to_string().contains(token));
        let archive = dir.path().join("workspace.zip");
        crate::workspace_zip::export(&workspace, &archive).unwrap();
        let mut zip = zip::ZipArchive::new(fs::File::open(archive).unwrap()).unwrap();
        use std::io::Read;
        for i in 0..zip.len() {
            let mut bytes = Vec::new();
            zip.by_index(i).unwrap().read_to_end(&mut bytes).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains(token));
        }
    }
}
