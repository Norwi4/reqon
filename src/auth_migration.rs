//! Explicit collection migration. Preview is read-only; apply publishes an
//! encrypted recovery journal before changing any source file.
use crate::{auth_vault, model, storage};
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
};

const MAX_FILE: u64 = 16 * 1024 * 1024;
const MAX_TOTAL: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 10_000;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub files_scanned: usize,
    pub files_to_change: usize,
    pub fields_to_protect: usize,
    pub yaml_files: usize,
    pub environment_fields: usize,
    pub paths: Vec<PathBuf>,
    pub backup_id: Option<String>,
    pub status: String,
}
pub struct Plan {
    root: PathBuf,
    files: Vec<(PathBuf, String)>,
    pub report: Report,
}
#[derive(Serialize, Deserialize)]
struct BackupFile {
    relative: PathBuf,
    before: String,
    after_hash: String,
}
#[derive(Serialize, Deserialize)]
struct Backup {
    version: u32,
    root: PathBuf,
    files: Vec<BackupFile>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn sensitive(kind: &str, key: &str) -> bool {
    let api_key = matches!(kind, "apikey" | "api-key");
    (auth_vault::secret_field(key) && !(api_key && key == "key")) || (api_key && key == "value")
}

/// Same top-level block convention as the collection reader. Unlike serializing
/// a whole request, these ranges leave scripts, bodies and unknown blocks intact.
pub(crate) fn blocks(source: &str) -> Result<Vec<(String, usize, usize)>> {
    let mut result = Vec::new();
    let mut current: Option<(String, usize, char)> = None;
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let text = line.trim_end_matches(['\r', '\n']);
        if let Some((name, start, close)) = &current {
            if text == close.to_string() {
                result.push((name.clone(), *start, offset));
                current = None;
            }
        } else {
            let text = text.trim().trim_start_matches('\u{feff}');
            if let Some(name) = text.strip_suffix('{').or_else(|| text.strip_suffix('[')) {
                current = Some((
                    name.trim().into(),
                    offset + line.len(),
                    if text.ends_with('{') { '}' } else { ']' },
                ));
            }
        }
        offset += line.len();
    }
    ensure!(current.is_none(), "Unclosed collection block");
    Ok(result)
}

fn transform_bru(
    source: &str,
    seal: &mut impl FnMut(&str) -> Result<String>,
) -> Result<(String, usize)> {
    transform_bru_impl(source, seal, false)
}
fn transform_bru_impl(
    source: &str,
    seal: &mut impl FnMut(&str) -> Result<String>,
    environment: bool,
) -> Result<(String, usize)> {
    let sections = blocks(source)?;
    let secret_names = storage::secret_names(source);
    let mut edits = Vec::new();
    let mut replacements = BTreeMap::new();
    let mut count = 0;
    for (name, start, end) in &sections {
        let kind = name.strip_prefix("auth:").unwrap_or("");
        if if environment {
            name != "vars"
        } else {
            kind.is_empty()
        } {
            continue;
        }
        let body = &source[*start..*end];
        let mut offset = 0;
        let mut consumed = 0;
        for line in body.split_inclusive('\n') {
            let position = offset;
            offset += line.len();
            if position < consumed {
                continue;
            }
            let Some((_, raw)) = crate::bru::split_pair(line.trim()) else {
                continue;
            };
            let begin = raw.as_ptr() as usize - body.as_ptr() as usize;
            let finish = if raw.starts_with("'''") {
                begin
                    + 3
                    + body[begin + 3..]
                        .find("'''")
                        .context("Unclosed multiline Auth value")?
                    + 3
            } else {
                begin + raw.len()
            };
            consumed = finish;
            let Some(pair) = crate::bru::pairs(&body[position..finish])
                .into_iter()
                .next()
            else {
                continue;
            };
            if if environment {
                !secret_names.contains(&pair.key)
            } else {
                !sensitive(kind, &pair.key)
            } {
                continue;
            }
            let value = pair.value;
            if environment && crate::environment_vault::is_reference(&value) {
                ensure!(
                    auth_vault::reference_id(&value).is_some(),
                    "Invalid secret variable reference"
                );
            }
            if if environment {
                value.is_empty() || crate::environment_vault::is_reference(&value)
            } else {
                !auth_vault::literal(&value)
            } {
                continue;
            }
            let reference = seal(&value)?;
            replacements.insert(value, reference.clone());
            edits.push((start + begin, start + finish, reference));
            count += 1;
        }
    }
    if count == 0 {
        return Ok((source.into(), 0));
    }
    for (name, start, end) in &sections {
        if name != "reqon:portable" {
            continue;
        }
        let mut value: serde_json::Value = serde_json::from_str(&source[*start..*end])
            .map_err(|_| anyhow::anyhow!("Invalid portable metadata"))?;
        auth_vault::replace_values(&mut value, &replacements);
        let newline = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let text = serde_json::to_string_pretty(&value)?
            .lines()
            .map(|s| format!("  {s}{newline}"))
            .collect::<String>();
        edits.push((*start, *end, text));
    }
    edits.sort_by_key(|(start, _, _)| *start);
    let mut result = source.to_owned();
    for (start, end, value) in edits.into_iter().rev() {
        result.replace_range(start..end, &value);
    }
    Ok((result, count))
}

fn transform_yaml(
    source: &str,
    seal: &mut impl FnMut(&str) -> Result<String>,
) -> Result<(String, usize)> {
    use serde_json::Value;
    let mut value: Value =
        serde_yaml::from_str(source).map_err(|_| anyhow::anyhow!("Invalid YAML document"))?;
    ensure!(value.is_object(), "Expected a YAML document");
    // Arbitrary YAML assets and environment files are outside Auth migration.
    if value.get("opencollection").is_none() && value.pointer("/info/type").is_none() {
        return Ok((source.into(), 0));
    }
    fn visit(
        value: &mut Value,
        kind: &str,
        replacements: &mut BTreeMap<String, String>,
        seal: &mut impl FnMut(&str) -> Result<String>,
    ) -> Result<usize> {
        let mut count = 0;
        if let Some(fields) = value.as_object_mut() {
            for (key, value) in fields {
                if sensitive(kind, key) && !value.is_null() {
                    ensure!(
                        !value.is_object() && !value.is_array(),
                        "Unsupported structured Auth secret"
                    );
                    let plain = value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string());
                    if auth_vault::literal(&plain) {
                        let reference = seal(&plain)?;
                        replacements.insert(plain, reference.clone());
                        *value = reference.into();
                        count += 1;
                    }
                } else {
                    count += visit(value, kind, replacements, seal)?;
                }
            }
        }
        Ok(count)
    }
    let mut count = 0;
    let mut replacements = BTreeMap::new();
    for protocol in ["http", "graphql", "grpc", "websocket", "request"] {
        if let Some(auth) = value.get_mut(protocol).and_then(|q| q.get_mut("auth")) {
            let kind = auth["type"].as_str().unwrap_or("").to_owned();
            count += visit(auth, &kind, &mut replacements, seal)?;
        }
    }
    if count == 0 {
        return Ok((source.into(), 0));
    }
    if let Some(extensions) = value.get_mut("extensions") {
        auth_vault::replace_values(extensions, &replacements);
    }
    if let Some(portable) = value.pointer_mut("/settings/reqonPortable") {
        auth_vault::replace_values(portable, &replacements);
    }
    Ok((serde_yaml::to_string(&value)?, count))
}
fn transform(
    path: &Path,
    source: &str,
    seal: &mut impl FnMut(&str) -> Result<String>,
) -> Result<(String, usize)> {
    if is_environment(path) {
        return if crate::yaml::is_yaml(path) {
            crate::environment_vault::migrate_yaml(source, seal)
        } else {
            transform_bru_impl(source, seal, true)
        };
    }
    if crate::yaml::is_yaml(path) {
        transform_yaml(source, seal)
    } else {
        transform_bru(source, seal)
    }
}
fn is_environment(path: &Path) -> bool {
    path.components()
        .next()
        .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case("environments"))
}
fn safe_file(root: &Path, relative: &Path) -> Result<PathBuf> {
    ensure!(
        !relative.as_os_str().is_empty()
            && relative
                .components()
                .all(|c| matches!(c, Component::Normal(_))),
        "Invalid migration file path"
    );
    let path = root.join(relative);
    crate::tree_move::within_roots(&path, &[root.to_owned()])?;
    for ancestor in path.ancestors().take_while(|p| *p != root) {
        let metadata = fs::symlink_metadata(ancestor)?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "Migration does not follow symbolic links"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "Migration does not follow reparse points"
            );
        }
    }
    ensure!(path.is_file(), "Migration source is not a file");
    Ok(path)
}
pub fn scan(root: &Path) -> Result<Plan> {
    scan_with_environments(root, false)
}
pub fn scan_with_environments(root: &Path, environments: bool) -> Result<Plan> {
    let root = root
        .canonicalize()
        .context("Collection directory is unavailable")?;
    ensure!(
        crate::native_format::is_collection(&root),
        "Choose a collection directory"
    );
    fn walk(
        root: &Path,
        dir: &Path,
        depth: usize,
        files: &mut Vec<PathBuf>,
        environments: bool,
    ) -> Result<()> {
        ensure!(depth <= 32, "Collection nesting exceeds 32 levels");
        for item in fs::read_dir(dir)? {
            let item = item?;
            let name = item.file_name().to_string_lossy().to_ascii_lowercase();
            if [
                ".git",
                ".reqon",
                ".trash",
                "node_modules",
                ".reqon-migration-backup",
            ]
            .contains(&name.as_str())
                || (!environments && name == "environments")
            {
                continue;
            }
            let metadata = fs::symlink_metadata(item.path())?;
            if metadata.file_type().is_symlink() {
                continue;
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if metadata.is_dir() {
                if !crate::native_format::is_collection(&item.path()) {
                    walk(root, &item.path(), depth + 1, files, environments)?;
                }
            } else if crate::native_format::is_source(&item.path())
                || crate::yaml::is_yaml(&item.path())
            {
                files.push(item.path().strip_prefix(root)?.into());
                ensure!(files.len() <= MAX_FILES, "Too many files to migrate");
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(&root, &root, 0, &mut files, environments)?;
    files.sort();
    let mut plan = Plan {
        root,
        files: vec![],
        report: Report {
            status: "preview".into(),
            ..Default::default()
        },
    };
    let mut total = 0;
    for relative in files {
        let path = safe_file(&plan.root, &relative)?;
        let size = fs::metadata(&path)?.len();
        total += size;
        ensure!(
            size <= MAX_FILE && total <= MAX_TOTAL,
            "Collection exceeds migration size limits"
        );
        let source = fs::read_to_string(&path).context("Cannot read migration source as UTF-8")?;
        let (_, count) = transform(&relative, &source, &mut |_| Ok("{{secret}}".into()))
            .with_context(|| format!("Cannot inspect {}", relative.display()))?;
        plan.report.files_scanned += 1;
        if count > 0 {
            plan.report.fields_to_protect += count;
            if is_environment(&relative) {
                plan.report.environment_fields += count;
            }
            plan.report.yaml_files += usize::from(crate::yaml::is_yaml(&relative));
            plan.report.paths.push(relative.clone());
            plan.files.push((relative, source));
        }
    }
    plan.report.files_to_change = plan.files.len();
    Ok(plan)
}
fn verify(root: &Path, relative: &Path, expected: &str) -> Result<PathBuf> {
    let path = safe_file(root, relative)?;
    ensure!(
        fs::read(&path)? == expected.as_bytes(),
        "File changed since preview: {}. Scan again.",
        relative.display()
    );
    Ok(path)
}
fn backup_path(vault: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        uuid::Uuid::parse_str(id).is_ok() && !id.contains(['/', '\\', ':']),
        "Invalid backup ID"
    );
    Ok(vault.join("auth-migrations").join(format!("{id}.bin")))
}
pub fn apply(plan: Plan) -> Result<Report> {
    apply_at(plan, &auth_vault::root()?, None)
}
fn apply_at(mut plan: Plan, vault: &Path, fail_after: Option<usize>) -> Result<Report> {
    if plan.files.is_empty() {
        plan.report.status = "unchanged".into();
        return Ok(plan.report);
    }
    for (relative, before) in &plan.files {
        verify(&plan.root, relative, before)?;
    }
    let mut writes = Vec::new();
    let mut backup = Backup {
        version: 1,
        root: plan.root.clone(),
        files: vec![],
    };
    for (relative, before) in &plan.files {
        let (after, _) = transform(relative, before, &mut |value| {
            auth_vault::store_at(vault, value)
        })?;
        backup.files.push(BackupFile {
            relative: relative.clone(),
            before: before.clone(),
            after_hash: hash(after.as_bytes()),
        });
        writes.push(after);
    }
    let id = model::id();
    let path = backup_path(vault, &id)?;
    let encrypted = crate::secrets::protect(&serde_json::to_vec(&backup)?)?;
    storage::atomic_write(&path, &encrypted)?;
    // FlushFileBuffers on Windows requires a handle opened for writing.
    fs::OpenOptions::new().write(true).open(&path)?.sync_all()?;
    // Confirm the durable recovery data can actually be decrypted before write 1.
    let restored: Backup = serde_json::from_slice(&crate::secrets::unprotect(&fs::read(&path)?)?)?;
    ensure!(
        restored.files.len() == backup.files.len(),
        "Cannot verify encrypted migration backup"
    );
    let result = (|| -> Result<()> {
        for (index, (file, after)) in backup.files.iter().zip(&writes).enumerate() {
            if fail_after == Some(index) {
                bail!("Injected migration write failure");
            }
            let path = verify(&plan.root, &file.relative, &file.before)?;
            storage::atomic_write(&path, after)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        return match restore_backup(&backup) {
            Ok(_) => Err(error.context(format!("Migration rolled back; encrypted backup {id}"))),
            Err(rollback) => Err(anyhow::anyhow!(
                "Migration failed: {error}; recovery stopped: {rollback}. Encrypted backup {id}"
            )),
        };
    }
    plan.report.status = "protected".into();
    plan.report.backup_id = Some(id);
    Ok(plan.report)
}
fn restore_backup(backup: &Backup) -> Result<usize> {
    ensure!(backup.version == 1, "Unsupported migration backup version");
    ensure!(
        backup.root.canonicalize()? == backup.root,
        "Collection location changed"
    );
    let mut pending = Vec::new();
    for file in &backup.files {
        let path = safe_file(&backup.root, &file.relative)?;
        let current = fs::read(&path)?;
        if current == file.before.as_bytes() {
            continue;
        }
        ensure!(
            hash(&current) == file.after_hash,
            "File was edited after migration: {}. Recovery will not overwrite it.",
            file.relative.display()
        );
        pending.push((path, file, current));
    }
    for (path, file, current) in &pending {
        ensure!(fs::read(path)? == *current, "File changed during recovery");
        storage::atomic_write(path, &file.before)?;
    }
    Ok(pending.len())
}
pub fn restore(root: &Path, id: &str) -> Result<Report> {
    restore_at(root, id, &auth_vault::root()?)
}

#[derive(Serialize)]
pub struct BackupInfo {
    pub id: String,
    pub files: usize,
    pub created_at: u64,
}
pub fn backups(root: &Path) -> Result<Vec<BackupInfo>> {
    backups_at(root, &auth_vault::root()?)
}
fn backups_at(root: &Path, vault: &Path) -> Result<Vec<BackupInfo>> {
    let root = root.canonicalize()?;
    let directory = vault.join("auth-migrations");
    if !directory.exists() {
        return Ok(vec![]);
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let path = entry.path();
        let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if backup_path(vault, id).is_err() {
            continue;
        }
        // An unrelated damaged backup must not hide this collection's backups.
        let Ok(bytes) = fs::read(&path)
            .and_then(|bytes| crate::secrets::unprotect(&bytes).map_err(std::io::Error::other))
        else {
            continue;
        };
        let Ok(backup) = serde_json::from_slice::<Backup>(&bytes) else {
            continue;
        };
        if backup.version != 1 || backup.root != root {
            continue;
        }
        result.push(BackupInfo {
            id: id.into(),
            files: backup.files.len(),
            created_at: entry
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
    }
    result.sort_by_key(|b| std::cmp::Reverse(b.created_at));
    Ok(result)
}

pub enum Operation {
    Scan(PathBuf, bool),
    Apply(Plan),
}
pub enum Outcome {
    Preview(Plan),
    Applied(Report),
}
pub struct Job {
    pub operation: Operation,
    pub reply: std::sync::mpsc::Sender<Result<Outcome, String>>,
}
pub fn execute(operation: Operation) -> Result<Outcome> {
    match operation {
        Operation::Scan(root, environments) => {
            scan_with_environments(&root, environments).map(Outcome::Preview)
        }
        Operation::Apply(plan) => apply(plan).map(Outcome::Applied),
    }
}
fn restore_at(root: &Path, id: &str, vault: &Path) -> Result<Report> {
    let bytes = fs::read(backup_path(vault, id)?).context("Encrypted backup is unavailable")?;
    let backup: Backup = serde_json::from_slice(&crate::secrets::unprotect(&bytes)?)?;
    ensure!(
        root.canonicalize()? == backup.root,
        "Backup belongs to a different collection"
    );
    let count = restore_backup(&backup)?;
    Ok(Report {
        status: "restored".into(),
        files_to_change: count,
        backup_id: Some(id.into()),
        paths: backup.files.into_iter().map(|f| f.relative).collect(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("collection");
        let vault = dir.path().join("vault");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("reqon.json"), "{\"version\":\"1\"}").unwrap();
        (dir, root, vault)
    }
    fn request(token: &str) -> String {
        format!(
            "meta {{\n  name: Echo\n  type: http\n}}\nget {{\n  url: http://localhost/echo\n  auth: bearer\n}}\nauth:bearer {{\n  token: {token}\n}}\n"
        )
    }
    #[test]
    fn bru_migration_only_changes_auth_and_matching_portable_metadata() {
        let source = "\u{feff}# leading comment\r\nauth:basic {\r\n  username: user\r\n  # keep this\r\n  password: abc-secret\r\n}\r\nauth:oauth2 {\r\n  clientSecret: '''\r\n    multi\r\n    line\r\n  '''\r\n  accessToken: {{TOKEN}}\r\n}\r\nbody:text {\r\n  abc-secret\r\n}\r\ncustom:vendor [\r\n  keep\r\n]\r\nreqon:portable {\r\n  {\"auth_value\":\"abc-secret\",\"keep\":42}\r\n}\r\n";
        let mut values = vec![];
        let (protected, count) = transform_bru(source, &mut |v| {
            values.push(v.to_owned());
            Ok("{{reference}}".into())
        })
        .unwrap();
        assert_eq!(count, 2);
        assert_eq!(values, ["abc-secret", "multi\nline"]);
        assert!(protected.starts_with("\u{feff}# leading comment\r\n"));
        assert!(protected.contains("# keep this\r\n  password: {{reference}}"));
        assert!(protected.contains("accessToken: {{TOKEN}}"));
        assert!(
            protected
                .contains("body:text {\r\n  abc-secret\r\n}\r\ncustom:vendor [\r\n  keep\r\n]")
        );
        let metadata = storage::blocks(&protected)
            .into_iter()
            .find(|(n, _)| n == "reqon:portable")
            .unwrap()
            .1;
        let v: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        assert_eq!(v["auth_value"], "{{reference}}");
        assert_eq!(v["keep"], 42);
    }
    #[test]
    fn preview_is_read_only_apply_is_idempotent_and_backup_restores_exact_bytes() {
        let (_dir, root, vault) = fixture();
        let first = request("migration-private-12345").replace('\n', "\r\n");
        fs::write(root.join("a.req"), &first).unwrap();
        fs::write(root.join("folder.bru"), "auth {\n mode: apikey\n}\nauth:apikey {\n key: X-API-Key\n value: folder-private-6789\n}\n").unwrap();
        let yaml = "info:\n  name: Yaml\n  type: http\nhttp:\n  method: GET\n  url: http://localhost\n  auth:\n    type: oauth2\n    credentials:\n      clientId: public-id\n      clientSecret: yaml-private-7890\n  body:\n    type: text\n    data: unchanged\nsettings:\n  reqonPortable:\n    auth_value: yaml-private-7890\nextensions:\n  custom:\n    data: [1, 2]\n";
        fs::write(root.join("b.yml"), yaml).unwrap();
        fs::create_dir(root.join("environments")).unwrap();
        fs::write(
            root.join("environments/Local.req"),
            "vars {\n token: env-not-migrated\n}\n",
        )
        .unwrap();
        fs::write(root.join("template.req"), request("{{TOKEN}}")).unwrap();
        let plan = scan(&root).unwrap();
        assert_eq!(plan.report.files_to_change, 3);
        assert_eq!(plan.report.fields_to_protect, 3);
        assert!(!vault.exists());
        assert_eq!(fs::read_to_string(root.join("a.req")).unwrap(), first);
        let report = apply_at(plan, &vault, None).unwrap();
        let id = report.backup_id.unwrap();
        let migrated = fs::read_to_string(root.join("a.req")).unwrap();
        assert!(!migrated.contains("migration-private-12345"));
        let mut r = storage::parse_bru(&migrated).unwrap();
        auth_vault::resolve_at(&vault, &mut r, true).unwrap();
        assert_eq!(r.auth.token, "migration-private-12345");
        let parent = fs::read_to_string(root.join("folder.bru")).unwrap();
        assert!(parent.contains("key: X-API-Key"));
        assert!(!parent.contains("folder-private-6789"));
        let output = fs::read_to_string(root.join("b.yml")).unwrap();
        assert!(!output.contains("yaml-private-7890"));
        let y: serde_json::Value = serde_yaml::from_str(&output).unwrap();
        assert_eq!(y["http"]["body"]["data"], "unchanged");
        assert_eq!(y["extensions"]["custom"]["data"], serde_json::json!([1, 2]));
        assert!(
            !String::from_utf8_lossy(&fs::read(backup_path(&vault, &id).unwrap()).unwrap())
                .contains("migration-private")
        );
        assert_eq!(
            apply_at(scan(&root).unwrap(), &vault, None).unwrap().status,
            "unchanged"
        );
        assert_eq!(
            fs::read_dir(vault.join("auth-migrations")).unwrap().count(),
            1
        );
        assert_eq!(restore_at(&root, &id, &vault).unwrap().files_to_change, 3);
        assert_eq!(fs::read_to_string(root.join("a.req")).unwrap(), first);
        assert_eq!(fs::read_to_string(root.join("b.yml")).unwrap(), yaml);
        assert_eq!(restore_at(&root, &id, &vault).unwrap().files_to_change, 0);
        // OpenCollection also accepts the hyphenated API-key type.
        let (protected, count) = transform_yaml(
            "info:\n  type: http\nhttp:\n  auth:\n    type: api-key\n    key: X-Key\n    value: private-api-key\n",
            &mut |_| Ok("{{reference}}".into()),
        ).unwrap();
        assert_eq!(count, 1);
        assert!(protected.contains("key: X-Key"));
        assert!(!protected.contains("private-api-key"));
    }
    #[test]
    fn preflight_rejects_stale_files_and_broken_vault_without_touching_sources() {
        let (_dir, root, vault) = fixture();
        let original = request("stale-private-123");
        fs::write(root.join("a.req"), &original).unwrap();
        let plan = scan(&root).unwrap();
        fs::write(root.join("a.req"), request("newer-private-456")).unwrap();
        assert!(
            apply_at(plan, &vault, None)
                .unwrap_err()
                .to_string()
                .contains("changed since preview")
        );
        assert!(!vault.exists());
        fs::write(root.join("a.req"), &original).unwrap();
        fs::create_dir(&vault).unwrap();
        fs::write(vault.join("key.bin"), "corrupt").unwrap();
        assert!(apply_at(scan(&root).unwrap(), &vault, None).is_err());
        assert_eq!(fs::read_to_string(root.join("a.req")).unwrap(), original);
    }
    #[test]
    fn mid_write_failure_restores_every_written_file_from_encrypted_backup() {
        let (_dir, root, vault) = fixture();
        let original = request("rollback-private-123");
        for name in ["a.req", "b.req"] {
            fs::write(root.join(name), &original).unwrap();
        }
        let error = apply_at(scan(&root).unwrap(), &vault, Some(1)).unwrap_err();
        assert!(error.to_string().contains("rolled back"));
        for name in ["a.req", "b.req"] {
            assert_eq!(fs::read_to_string(root.join(name)).unwrap(), original);
        }
        assert_eq!(
            fs::read_dir(vault.join("auth-migrations")).unwrap().count(),
            1
        );
        let backups = backups_at(&root, &vault).unwrap();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            restore_at(&root, &backups[0].id, &vault)
                .unwrap()
                .files_to_change,
            0
        );
    }
    #[test]
    fn recovery_refuses_newer_edits_and_wrong_collection() {
        let (_dir, root, vault) = fixture();
        for name in ["a.req", "b.req"] {
            fs::write(root.join(name), request("recover-private-123")).unwrap();
        }
        let id = apply_at(scan(&root).unwrap(), &vault, None)
            .unwrap()
            .backup_id
            .unwrap();
        let migrated = fs::read(root.join("a.req")).unwrap();
        fs::write(root.join("b.req"), "user-edited-content").unwrap();
        assert!(
            restore_at(&root, &id, &vault)
                .unwrap_err()
                .to_string()
                .contains("edited after migration")
        );
        assert_eq!(fs::read(root.join("a.req")).unwrap(), migrated);
        assert_eq!(
            fs::read_to_string(root.join("b.req")).unwrap(),
            "user-edited-content"
        );
        assert!(
            restore_at(&vault, &id, &vault)
                .unwrap_err()
                .to_string()
                .contains("different collection")
        );
        assert!(safe_file(&root, Path::new("../outside.req")).is_err());
        assert!(backup_path(&vault, "../../outside").is_err());
    }
    #[test]
    fn recovery_handles_partial_migration_and_rejects_corrupt_backup() {
        let (_dir, root, vault) = fixture();
        let original = request("partial-private-123");
        for name in ["a.req", "b.req"] {
            fs::write(root.join(name), &original).unwrap();
        }
        let id = apply_at(scan(&root).unwrap(), &vault, None)
            .unwrap()
            .backup_id
            .unwrap();
        // Simulate a process stopping with one file still in its original state.
        fs::write(root.join("b.req"), &original).unwrap();
        let path = backup_path(&vault, &id).unwrap();
        let encrypted = fs::read(&path).unwrap();
        fs::write(&path, b"broken backup").unwrap();
        let migrated = fs::read(root.join("a.req")).unwrap();
        assert!(restore_at(&root, &id, &vault).is_err());
        assert_eq!(fs::read(root.join("a.req")).unwrap(), migrated);
        fs::write(&path, encrypted).unwrap();
        assert_eq!(restore_at(&root, &id, &vault).unwrap().files_to_change, 1);
        for name in ["a.req", "b.req"] {
            assert_eq!(fs::read_to_string(root.join(name)).unwrap(), original);
        }
    }
    #[test]
    fn malformed_late_file_prevents_any_source_changes() {
        let (_dir, root, vault) = fixture();
        let original = request("preflight-private-123");
        fs::write(root.join("a.req"), &original).unwrap();
        fs::write(root.join("z.req"), "auth:bearer {\n token: broken\n").unwrap();
        assert!(scan(&root).is_err());
        assert_eq!(fs::read_to_string(root.join("a.req")).unwrap(), original);
        assert!(!vault.exists());
    }
    #[test]
    fn environment_migration_is_opt_in_preserves_bru_and_uses_recovery_journal() {
        let (_dir, root, vault) = fixture();
        fs::create_dir(root.join("environments")).unwrap();
        let bru = "# keep\r\nvars {\r\n  TOKEN: env-migrate-private-23948\r\n  ~DISABLED: disabled-private-2345\r\n  PUBLIC: unchanged\r\n}\r\nvars:secret [\r\n TOKEN\r\n DISABLED\r\n]\r\n";
        let yaml = "name: Other\nvendor: keep\nvariables:\n- name: TEMPLATE\n  secret: true\n  value: prefix-{{OTHER}}\n- name: PUBLIC\n  value: unchanged\n";
        fs::write(root.join("environments/Local.req"), bru).unwrap();
        fs::write(root.join("environments/Other.yml"), yaml).unwrap();
        assert_eq!(scan(&root).unwrap().report.files_to_change, 0);
        let plan = scan_with_environments(&root, true).unwrap();
        assert_eq!(plan.report.environment_fields, 3);
        assert!(!vault.exists());
        let id = apply_at(plan, &vault, None).unwrap().backup_id.unwrap();
        let protected = fs::read_to_string(root.join("environments/Local.req")).unwrap();
        assert!(protected.starts_with("# keep\r\nvars {\r\n"));
        assert!(!protected.contains("private"));
        assert!(protected.contains("~DISABLED:") && protected.contains("PUBLIC: unchanged"));
        assert_eq!(
            scan_with_environments(&root, true)
                .unwrap()
                .report
                .files_to_change,
            0
        );
        assert_eq!(restore_at(&root, &id, &vault).unwrap().files_to_change, 2);
        assert_eq!(
            fs::read_to_string(root.join("environments/Local.req")).unwrap(),
            bru
        );
        assert_eq!(
            fs::read_to_string(root.join("environments/Other.yml")).unwrap(),
            yaml
        );
    }
    #[cfg(feature = "desktop")]
    #[test]
    fn desktop_writer_applies_queued_saves_before_protection() {
        let dir = tempfile::tempdir().unwrap();
        let ws = storage::load(dir.path()).unwrap();
        let mut request = ws.requests[0].clone();
        request.auth.kind = "bearer".into();
        request.auth.token = "writer-migration-private-123".into();
        fs::write(request.path.as_ref().unwrap(), storage::to_bru(&request)).unwrap();
        let plan = scan(&ws.collections[0].path).unwrap();
        let writer =
            crate::persistence::Writer::new(dir.path().into(), eframe::egui::Context::default());
        request.url = "http://localhost/new-edit".into();
        writer.save_request(request.clone());
        let response = writer.auth_migration(Operation::Apply(plan));
        let error = response
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap()
            .err()
            .unwrap();
        assert!(error.contains("changed since preview"));
        let disk = crate::yaml::read_request(request.path.as_ref().unwrap()).unwrap();
        assert_eq!(disk.url, request.url);
        assert_eq!(disk.auth.token, request.auth.token);
    }
}
