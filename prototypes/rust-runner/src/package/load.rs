use super::*;
use crate::input;
use serde::de::DeserializeOwned;
use std::{fs, path::Path};

fn errors_at(mut errors: Vec<Diagnostic>, file: &str) -> Vec<Diagnostic> {
    for error in &mut errors {
        error.file = Some(file.to_owned());
    }
    errors
}

fn error(code: &'static str, file: &str, path: &str, message: &str) -> Diagnostic {
    let mut value = Diagnostic::new(code, path, message);
    value.file = Some(file.into());
    value
}

fn portable_path(path: &str) -> bool {
    path.len() <= 512
        && path.split('/').all(|part| {
            let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with(['.', ' '])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
                && !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && matches!(base.as_bytes()[3], b'1'..=b'9'))
        })
}

fn resolve(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if !portable_path(relative) {
        return Err("use a portable relative .json path; no absolute paths, parent traversal or Windows device names".into());
    }
    let mut path = root.to_path_buf();
    for part in relative.split('/') {
        path.push(part);
        let metadata = fs::symlink_metadata(&path)
            .map_err(|_| "referenced file or directory does not exist")?;
        let mut linked = metadata.file_type().is_symlink();
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            linked |= metadata.file_attributes() & 0x400 != 0;
        }
        if linked {
            return Err(
                "linked files/directories and Windows reparse points are not supported in packages"
                    .into(),
            );
        }
    }
    let resolved = path
        .canonicalize()
        .map_err(|_| "cannot resolve referenced file")?;
    if !resolved.starts_with(root) || !resolved.is_file() {
        return Err("reference must be a regular file inside the package".into());
    }
    Ok(resolved)
}

struct Reader {
    root: PathBuf,
    bytes: usize,
    used: HashSet<String>,
    files: Vec<SourceFile>,
    paths: Vec<PathBuf>,
}

impl Reader {
    fn read<T: DeserializeOwned>(&mut self, relative: &str) -> Validation<T> {
        if !relative.ends_with(".json") {
            return Err(vec![error(
                "package.path",
                relative,
                "$",
                "JSON input requires a .json extension",
            )]);
        }
        if !self.used.insert(relative.to_lowercase()) {
            return Err(vec![error(
                "package.path_duplicate",
                relative,
                "$",
                "file references must be unique, including letter case",
            )]);
        }
        let path = resolve(&self.root, relative)
            .map_err(|message| vec![error("package.path", relative, "$", &message)])?;
        let bytes = input::read(&path).map_err(|e| errors_at(e, relative))?;
        self.bytes += bytes.len();
        if self.bytes > MAX_PACKAGE_BYTES {
            return Err(vec![error(
                "package.size",
                relative,
                "$",
                "package inputs exceed 32 MiB",
            )]);
        }
        self.files.push(SourceFile {
            path: relative.into(),
            sha256: hash_bytes(&bytes),
        });
        self.paths.push(path);
        input::decode(input::json_value(&bytes).map_err(|e| errors_at(e, relative))?)
            .map_err(|e| errors_at(e, relative))
    }
}

/// The caller passes the already-read manifest bytes. All execution uses this
/// loaded snapshot; no case is re-read after validation or selection.
pub fn load(path: &Path, bytes: &[u8]) -> Validation<Package> {
    let file = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("catalog.json");
    let manifest: Manifest =
        input::decode(input::json_value(bytes).map_err(|e| errors_at(e, file))?)
            .map_err(|e| errors_at(e, file))?;
    let mut errors = Vec::new();
    if manifest.schema_version != FORMAT {
        errors.push(error(
            "package.version",
            file,
            "$.schema_version",
            "unsupported package format",
        ));
    }
    if !valid_id(&manifest.id) {
        errors.push(error("package.id", file, "$.id", "ID must be 1..128 ASCII letters/digits/dot/underscore/hyphen, starting with a letter or digit"));
    }
    if manifest.name.trim().is_empty() {
        errors.push(error(
            "package.name",
            file,
            "$.name",
            "name must not be empty",
        ));
    }
    if manifest.cases.is_empty()
        || manifest.cases.len() > 1000
        || manifest.cases.len() + manifest.suites.len() + 1 > MAX_FILES
    {
        errors.push(error(
            "package.files",
            file,
            "$.cases",
            "package requires 1..1000 cases and at most 1024 total input files",
        ));
    }
    if !(1..=3_600_000).contains(&manifest.timeout_ms) {
        errors.push(error(
            "package.timeout",
            file,
            "$.timeout_ms",
            "package timeout must be 1..3600000 ms",
        ));
    }
    let engine =
        semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo version is SemVer");
    if manifest.requires.engine != "reqon"
        || !semver::VersionReq::parse(&manifest.requires.version)
            .is_ok_and(|requirement| requirement.matches(&engine))
    {
        errors.push(error(
            "package.engine",
            file,
            "$.requires",
            "engine or version requirement is incompatible with this runner",
        ));
    }
    let mut features = HashSet::new();
    for feature in &manifest.requires.capabilities {
        if !FEATURES.contains(&feature.as_str()) || !features.insert(feature.as_str()) {
            errors.push(error(
                "package.capability",
                file,
                "$.requires.capabilities",
                "capabilities must be supported and unique",
            ));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    if let Err(e) = crate::runtime::validate_variables(&manifest.variables) {
        return Err(vec![error(
            "package.variables",
            file,
            "$.variables",
            &e.to_string(),
        )]);
    }
    if !manifest.variables.is_empty() && !features.contains("variables") {
        return Err(vec![error(
            "package.capability",
            file,
            "$.requires.capabilities",
            "declare variables capability for package defaults",
        )]);
    }
    let manifest_path = path
        .canonicalize()
        .map_err(|e| vec![error("input.io", file, "$", &e.to_string())])?;
    let root = manifest_path.parent().unwrap().to_path_buf();
    let mut reader = Reader {
        root,
        bytes: bytes.len(),
        used: HashSet::from([file.to_lowercase()]),
        files: vec![SourceFile {
            path: file.into(),
            sha256: hash_bytes(bytes),
        }],
        paths: vec![manifest_path],
    };
    let mut cases = Vec::new();
    let mut assets = BTreeMap::new();
    let mut case_ids = HashSet::new();
    for relative in &manifest.cases {
        let case: Case = match reader.read(relative) {
            Ok(case) => case,
            Err(e) => {
                errors.extend(e);
                if reader.bytes > MAX_PACKAGE_BYTES {
                    break;
                }
                continue;
            }
        };
        if case.schema_version != CASE_FORMAT {
            errors.push(error(
                "case.version",
                relative,
                "$.schema_version",
                "unsupported case format",
            ));
        }
        if !valid_id(&case.id) || !case_ids.insert(case.id.clone()) {
            errors.push(error(
                "case.id",
                relative,
                "$.id",
                "case IDs must be portable and unique",
            ));
        }
        if case.name.trim().is_empty() {
            errors.push(error(
                "case.name",
                relative,
                "$.name",
                "name must not be empty",
            ));
        }
        let mut tags = HashSet::new();
        if let Err(e) = crate::runtime::validate_variables(&case.variables) {
            errors.push(error(
                "case.variables",
                relative,
                "$.variables",
                &e.to_string(),
            ));
        }
        let mut defaults = manifest.variables.clone();
        defaults.extend(case.variables.clone());
        if let Err(e) = crate::runtime::validate_variables(&defaults) {
            errors.push(error(
                "case.variables",
                relative,
                "$.variables",
                &e.to_string(),
            ));
        }
        if !case.variables.is_empty() && !features.contains("variables") {
            errors.push(error(
                "case.capability",
                relative,
                "$.variables",
                "declare variables capability for case defaults",
            ));
        }
        if case
            .tags
            .iter()
            .any(|tag| tag.trim().is_empty() || !tags.insert(tag))
        {
            errors.push(error(
                "case.tags",
                relative,
                "$.tags",
                "tags must be nonempty and unique",
            ));
        }
        errors.extend(errors_at(
            case.scenario("http://127.0.0.1:1").diagnostics(),
            relative,
        ));
        for (i, step) in case.steps.iter().enumerate() {
            let mut needed = Vec::new();
            match step {
                Step::Grpc { poll, .. } => {
                    needed.push("grpc.unary");
                    if poll.is_some() {
                        needed.push("poll");
                    }
                }
                Step::Javascript { .. } => needed.push("javascript"),
                Step::RegressionCall { .. } => needed.push("regression.java"),
                Step::Kafka { .. } => needed.push("kafka"),
                Step::Http { poll, .. } => {
                    needed.push("http");
                    if poll.is_some() {
                        needed.push("poll");
                    }
                }
                Step::Pause { .. } => needed.push("pause"),
            }
            let hooks = match step {
                Step::Grpc { hooks, .. } | Step::Http { hooks, .. } => Some(hooks),
                _ => None,
            };
            if hooks.is_some_and(|h| !h.pre.is_empty() || !h.post.is_empty() || !h.tests.is_empty())
            {
                needed.extend(["javascript", "hooks"]);
            }
            if needed.iter().any(|feature| !features.contains(feature)) {
                errors.push(error(
                    "case.capability",
                    relative,
                    &format!("$.steps[{i}]"),
                    "step uses a capability not declared in manifest.requires.capabilities",
                ));
            }
            for asset in step.file_paths() {
                if assets.contains_key(asset) {
                    continue;
                }
                if !reader.used.insert(asset.to_lowercase()) {
                    errors.push(error(
                        "package.asset",
                        asset,
                        "$",
                        "asset conflicts with an existing input path",
                    ));
                    continue;
                }
                let loaded = resolve(&reader.root, asset)
                    .map_err(|e| vec![error("package.asset", asset, "$", &e)])
                    .and_then(|path| {
                        let bytes = input::read(&path)?;
                        reader.bytes += bytes.len();
                        if reader.bytes > MAX_PACKAGE_BYTES || reader.files.len() >= MAX_FILES {
                            return Err(vec![error(
                                "package.size",
                                asset,
                                "$",
                                "package assets exceed aggregate limits",
                            )]);
                        }
                        reader.files.push(SourceFile {
                            path: asset.into(),
                            sha256: hash_bytes(&bytes),
                        });
                        reader.paths.push(path);
                        Ok(bytes)
                    });
                match loaded {
                    Ok(bytes) => {
                        assets.insert(asset.to_owned(), bytes::Bytes::from(bytes));
                    }
                    Err(e) => errors.extend(e),
                }
            }
        }
        let sha256 = hash_json(&serde_json::to_value(&case).expect("case serializes"));
        cases.push(LoadedCase {
            definition: case,
            file: relative.clone(),
            sha256,
        });
    }
    if reader.bytes > MAX_PACKAGE_BYTES {
        return Err(errors);
    }
    let mut suites = Vec::new();
    let mut suite_ids = HashSet::new();
    for relative in &manifest.suites {
        let suite: Suite = match reader.read(relative) {
            Ok(suite) => suite,
            Err(e) => {
                errors.extend(e);
                if reader.bytes > MAX_PACKAGE_BYTES {
                    break;
                }
                continue;
            }
        };
        if suite.schema_version != SUITE_FORMAT {
            errors.push(error(
                "suite.version",
                relative,
                "$.schema_version",
                "unsupported suite format",
            ));
        }
        if !valid_id(&suite.id) || !suite_ids.insert(suite.id.clone()) {
            errors.push(error(
                "suite.id",
                relative,
                "$.id",
                "suite IDs must be portable and unique",
            ));
        }
        if suite.name.trim().is_empty() || suite.cases.is_empty() || suite.cases.len() > 1000 {
            errors.push(error(
                "suite.empty",
                relative,
                "$",
                "suite requires a name and 1..1000 case IDs",
            ));
            continue;
        }
        let mut seen = HashSet::new();
        for (i, id) in suite.cases.iter().enumerate() {
            if !case_ids.contains(id) || !seen.insert(id) {
                errors.push(error(
                    "suite.reference",
                    relative,
                    &format!("$.cases[{i}]"),
                    "suite references must be known, unique case IDs",
                ));
            }
        }
        suites.push(LoadedSuite {
            definition: suite,
            file: relative.clone(),
        });
    }
    if manifest
        .default_suite
        .as_ref()
        .is_some_and(|id| !suite_ids.contains(id))
    {
        errors.push(error(
            "package.default_suite",
            file,
            "$.default_suite",
            "default_suite must reference a declared suite ID",
        ));
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    reader.files.sort_by(|a, b| a.path.cmp(&b.path));
    let package_sha256 = hash_json(&json!({"schema_version":FORMAT,"files":reader.files}));
    Ok(Package {
        manifest,
        cases,
        suites,
        files: reader.files,
        package_sha256,
        source_paths: reader.paths,
        assets: std::sync::Arc::new(assets),
    })
}
