//! One-time, transactional upgrade of the 0.19 SQLite workspace.
//! The source is opened read-only; SQLite backup includes committed WAL pages.
use crate::{model::*, storage};
use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn default_root(exe: &Path, explicit: Option<PathBuf>) -> PathBuf {
    if let Some(root) = explicit {
        return if !root.join("workspace.json").exists() && source_in(&root).is_some() {
            root.join("native")
        } else {
            root
        };
    }
    let portable = exe.parent().unwrap_or(Path::new(".")).join("data");
    if portable.join("workspace.json").exists() || exe.with_file_name("portable.flag").exists() {
        return portable;
    }
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|s| PathBuf::from(s).join(".local/share")));
    base.unwrap_or_else(|| exe.parent().unwrap_or(Path::new(".")).into())
        .join("Reqon/native")
}
pub fn source_in(root: &Path) -> Option<PathBuf> {
    ["reqon.sqlite3", "getman.sqlite3"]
        .iter()
        .map(|n| root.join(n))
        .find(|p| p.is_file())
}
pub fn auto_source(root: &Path, isolated: bool) -> Option<PathBuf> {
    if root.join("workspace.json").exists() {
        return None;
    }
    if let Some(source) = source_in(root).or_else(|| {
        root.parent()
            .filter(|_| root.file_name().is_some_and(|n| n == "native"))
            .and_then(source_in)
    }) {
        return Some(source);
    }
    if isolated {
        return None;
    }
    #[cfg(windows)]
    if let Some(base) = std::env::var_os("APPDATA").map(PathBuf::from) {
        return [
            base.join("Reqon"),
            base.join("Getman/Getman"),
            base.join("Getman"),
        ]
        .iter()
        .find_map(|p| source_in(p));
    }
    None
}
fn text(value: &Value) -> String {
    value.as_str().map(str::to_owned).unwrap_or_default()
}
fn key(row: &Value) -> i64 {
    row["id"].as_i64().unwrap_or(0)
}
fn payload(row: &Value, field: &str) -> Result<Value> {
    match row.get(field) {
        Some(Value::String(s)) if !s.is_empty() => serde_json::from_str(s)
            .with_context(|| format!("Invalid {field} in legacy record {}", key(row))),
        Some(v) if !v.is_null() => Ok(v.clone()),
        _ => Ok(json!({})),
    }
}
fn table(db: &Connection, name: &str) -> Result<Vec<Value>> {
    let exists: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
        [name],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(vec![]);
    }
    let mut statement = db.prepare(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")))?;
    let columns = statement
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    Ok(statement
        .query_map([], |row| {
            let mut object = serde_json::Map::new();
            for (i, column) in columns.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => json!(n),
                    ValueRef::Real(n) => json!(n),
                    ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                    ValueRef::Blob(s) => {
                        use base64::Engine;
                        json!({"base64":base64::prelude::BASE64_STANDARD.encode(s)})
                    }
                };
                object.insert(column.clone(), value);
            }
            Ok(Value::Object(object))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}
fn ordered(mut rows: Vec<Value>) -> Vec<Value> {
    rows.sort_by_key(|r| (r["sort_order"].as_i64().unwrap_or(0), key(r)));
    rows
}
fn vars(value: &Value) -> Vec<Pair> {
    if let Some(rows) = value.as_array() {
        return rows
            .iter()
            .filter_map(|row| {
                if let Some(a) = row.as_array() {
                    return Some(Pair {
                        key: a.first().map(text).unwrap_or_default(),
                        value: a.get(1).map(text).unwrap_or_default(),
                        description: a.get(2).map(text).unwrap_or_default(),
                        enabled: a.get(3).and_then(Value::as_bool).unwrap_or(true),
                        ..Default::default()
                    });
                }
                Some(Pair {
                    key: row["key"].as_str()?.into(),
                    value: row["value"]
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| row["value"].to_string()),
                    enabled: row["enabled"].as_bool().unwrap_or(true),
                    secret: row["type"] == "secret",
                    description: text(&row["description"]),
                    ..Default::default()
                })
            })
            .collect();
    }
    value
        .as_object()
        .into_iter()
        .flatten()
        .map(|(k, v)| {
            Pair::new(
                k,
                v.as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| v.to_string()),
            )
        })
        .collect()
}
fn uid(row: &Value, prefix: &str) -> String {
    format!(
        "{prefix}-{}",
        row["uid"]
            .as_str()
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| key(row).to_string())
    )
}
fn parent_metadata(
    row: &Value,
    path: PathBuf,
    protocol: &str,
    cid: &str,
    folder: &str,
) -> Result<Request> {
    let metadata = payload(row, "import_metadata")?;
    let mut request = Request {
        name: text(&row["name"]),
        protocol: protocol.into(),
        collection: cid.into(),
        folder: folder.into(),
        path: Some(path),
        docs: text(&row["description"]),
        portable: json!({"legacy_metadata":row}),
        ..Default::default()
    };
    request.auth = crate::postman::auth(&metadata["auth"]);
    crate::postman::metadata(&metadata, &mut request);
    Ok(request)
}
fn attachment(source: &str, legacy: &Path, stage: &Path, target: &Path) -> Result<String> {
    if source.is_empty() {
        return Ok(String::new());
    }
    let path = Path::new(source);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        legacy.join(path)
    };
    // Copy referenced managed resources; external files keep an absolute reference.
    if path.is_file() && path.starts_with(legacy) {
        use sha2::{Digest, Sha256};
        let bytes = fs::read(&path)?;
        let name = format!(
            "{:x}.{}",
            Sha256::digest(&bytes),
            path.extension().and_then(|s| s.to_str()).unwrap_or("bin")
        );
        let relative = PathBuf::from("resources").join(name);
        storage::atomic_write(&stage.join(&relative), bytes)?;
        return Ok(target.join(relative).to_string_lossy().into_owned());
    }
    Ok(path.to_string_lossy().into_owned())
}
fn request(data: &Value, legacy: &Path, stage: &Path, target: &Path) -> Result<Request> {
    let mut r = Request::default();
    crate::interchange::apply_getman(data, &mut r, stage)?;
    r.name = data["name"].as_str().unwrap_or("Untitled").into();
    if r.protocol != "grpc" && !data["_reqon"].is_object() {
        r.portable["__legacy_query_url"] = json!(r.url);
    }
    for value in [
        &mut r.ca_certificate,
        &mut r.client_certificate,
        &mut r.client_key,
    ] {
        *value = attachment(value, legacy, stage, target)?;
    }
    for path in &mut r.proto_files {
        if let Ok(relative) = Path::new(path).strip_prefix(stage) {
            *path = target.join(relative).to_string_lossy().into_owned();
        } else {
            *path = attachment(path, legacy, stage, target)?;
        }
    }
    for path in &mut r.proto_import_paths {
        if Path::new(path).is_relative() {
            *path = legacy.join(&*path).to_string_lossy().into_owned();
        }
    }
    if r.body_kind == BodyKind::Binary {
        r.body = attachment(&r.body, legacy, stage, target)?;
    }
    for row in &mut r.form {
        if row.kind == "file" {
            row.value = format!(
                "@{}",
                attachment(row.value.trim_start_matches('@'), legacy, stage, target)?
            );
        }
    }
    r.docs = data["description"]
        .as_str()
        .or_else(|| data["docs"].as_str())
        .unwrap_or("")
        .into();
    if r.protocol == "grpc" {
        r.protocol_messages = vec![Pair::new("Message 1", &r.body)];
    }
    Ok(r)
}
fn defaults(r: &mut Request, data: &Value, general: &Value) {
    if data["general_overrides"] != true {
        if let Some(v) = general["verify_ssl"].as_bool() {
            r.verify_tls = v;
        }
        if let Some(v) = general["timeout_ms"].as_u64() {
            r.timeout = v;
            r.timeout_inherit = false;
        }
        if let Some(v) = general["follow_redirects"].as_bool() {
            r.follow_redirects = v;
        }
    }
}

pub fn migrate(source: &Path, target: &Path) -> Result<Value> {
    if target.join("workspace.json").is_file() {
        return Ok(json!({"already_migrated":true}));
    }
    ensure!(source.is_file(), "Previous workspace database is missing");
    let parent = target.parent().context("Workspace path needs a parent")?;
    fs::create_dir_all(parent)?;
    // SQLite owns the cross-process lock, and automatically releases it on crash.
    let lock = Connection::open(parent.join(".reqon-upgrade-lock.sqlite3"))?;
    lock.busy_timeout(Duration::from_secs(15))?;
    lock.execute_batch("BEGIN EXCLUSIVE")?;
    if target.join("workspace.json").is_file() {
        return Ok(json!({"already_migrated":true}));
    }
    ensure!(
        !target.exists() || fs::read_dir(target)?.next().is_none(),
        "The target workspace is not empty. Existing files were left intact."
    );
    let run = id();
    let backup = parent
        .join("upgrade-backups")
        .join(format!("before-0.20.0-{run}"));
    fs::create_dir_all(&backup)?;
    let source_db = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    source_db.busy_timeout(Duration::from_secs(10))?;
    let mut saved = Connection::open(backup.join("workspace-0.19.sqlite3"))?;
    let start = std::time::Instant::now();
    {
        let copy = rusqlite::backup::Backup::new(&source_db, &mut saved)?;
        loop {
            match copy.step(256)? {
                rusqlite::backup::StepResult::Done => break,
                _ => {
                    ensure!(
                        start.elapsed() < Duration::from_secs(20),
                        "Previous workspace is busy. Close the previous application and retry; its data is unchanged."
                    );
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
        }
    }
    let integrity: String = saved.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    ensure!(
        integrity == "ok",
        "Previous workspace failed its integrity check; the source was not changed"
    );
    drop(source_db);
    let mut all = BTreeMap::new();
    let tables = saved
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for name in tables {
        all.insert(name.clone(), table(&saved, &name)?);
    }
    // Preserve every column and unknown extension, including future/optional metadata.
    storage::atomic_write(
        &backup.join("records.json"),
        serde_json::to_vec_pretty(&all)?,
    )?;
    let stage = parent.join(format!(".reqon-upgrade-{run}"));
    fs::create_dir(&stage)?;
    let legacy = source.parent().unwrap();
    let result = convert(&all, legacy, &stage, target, &backup);
    let (mut ws, mut report) = match result {
        Ok(v) => v,
        Err(error) => {
            return Err(error.context(format!(
                "Upgrade stopped. Source and backup are intact: {}",
                backup.display()
            )));
        }
    };
    for c in &mut ws.collections {
        c.path = target.join(c.path.strip_prefix(&stage)?);
    }
    for r in ws
        .requests
        .iter_mut()
        .chain(ws.collection_drafts.values_mut())
    {
        if let Some(path) = &r.path {
            if let Ok(relative) = path.strip_prefix(&stage) {
                r.path = Some(target.join(relative));
            }
        }
    }
    storage::save(&stage, &ws)?;
    report["backup"] = json!(backup);
    report["source"] = json!(source);
    report["version"] = json!(env!("CARGO_PKG_VERSION"));
    storage::atomic_write(
        &stage.join("upgrade-0.19-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    // workspace.json becomes visible only with the fully completed new profile.
    if target.exists() {
        fs::remove_dir(target)?;
    }
    fs::rename(&stage, target)
        .context("Could not publish the converted workspace; source and backup are intact")?;
    Ok(report)
}

fn convert(
    all: &BTreeMap<String, Vec<Value>>,
    legacy: &Path,
    stage: &Path,
    target: &Path,
    backup: &Path,
) -> Result<(Workspace, Value)> {
    let empty = vec![];
    let rows = |name: &str| all.get(name).unwrap_or(&empty);
    let state = rows("workspace_state")
        .iter()
        .map(|r| Ok((text(&r["key"]), payload(r, "data")?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut ws = Workspace::default();
    let mut collections = HashMap::new();
    let mut folder_paths = HashMap::<i64, (String, String)>::new();
    let mut tab_ids = HashMap::new();
    let general = state.get("general").cloned().unwrap_or(json!({}));
    if let Some(v) = general["autosave"].as_bool() {
        ws.preferences.autosave = v;
    }
    if let Some(v) = general["verify_ssl"].as_bool() {
        ws.preferences.verify_tls = v;
    }
    if let Some(v) = general["disable_cookies"].as_bool() {
        ws.preferences.store_cookies = !v;
        ws.preferences.send_cookies = !v;
    }
    if let Some(v) = general["timeout_ms"].as_u64() {
        ws.preferences.timeout = v;
    }
    if let Some(v) = general["font_size"].as_f64() {
        ws.preferences.editor_font_size = v as f32;
    }
    if let Some(v) = general["font_family"].as_str() {
        ws.preferences.appearance.font = v.into();
    }
    ws.side_by_side = general["two_pane"].as_bool().unwrap_or(false);
    if let Some(theme) = state.get("theme") {
        ws.dark = theme["manual"] != "light";
        ws.preferences.appearance.mode = if theme["mode"] == "system" {
            "system"
        } else if ws.dark {
            "dark"
        } else {
            "light"
        }
        .into();
    }
    if let Some(proxy) = state.get("proxy") {
        let p = &mut ws.preferences.proxy_settings;
        p.mode = if proxy["custom"] == true {
            "manual"
        } else if proxy["system"] == true || proxy["environment"] == true {
            "system"
        } else {
            "off"
        }
        .into();
        p.protocol = proxy["protocol"].as_str().unwrap_or("http").into();
        p.host = text(&proxy["host"]);
        p.port = text(&proxy["port"]);
        p.auth = proxy["auth"] == true;
        p.username = text(&proxy["username"]);
        p.bypass = text(&proxy["bypass"]);
        let password = text(&proxy["password"]);
        let password = if let Some(encoded) = password.strip_prefix("dpapi:") {
            use base64::Engine;
            String::from_utf8(crate::secrets::unprotect(
                &base64::prelude::BASE64_STANDARD.decode(encoded)?,
            )?)?
        } else {
            password
        };
        if !password.is_empty() {
            ws.preferences.credentials.insert("proxy".into(), password);
        }
    }
    ws.globals = vars(state.get("environment_globals").unwrap_or(&Value::Null));
    for row in ordered(rows("collections").clone()) {
        let cid = uid(&row, "collection");
        let c = Collection {
            id: cid.clone(),
            name: text(&row["name"]),
            path: stage.join("collections").join(format!(
                "{}-{}",
                safe_name(&text(&row["name"])),
                key(&row)
            )),
            environments: vec![],
            vars: vars(&payload(&row, "variables")?),
            folders: vec![],
        };
        storage::create_collection(&c)?;
        let mut parent =
            parent_metadata(&row, c.path.join("collection.req"), "collection", &cid, "")?;
        parent.vars = c.vars.clone();
        storage::save_request(&parent)?;
        tab_ids.insert(
            format!("collection:{}", text(&row["uid"])),
            format!("__collection__{cid}"),
        );
        collections.insert(key(&row), ws.collections.len());
        ws.collections.push(c);
    }
    let mut todo = ordered(rows("folders").clone());
    while !todo.is_empty() {
        let before = todo.len();
        let mut next = vec![];
        for row in todo {
            let parent = row["parent_id"].as_i64();
            if parent.is_some_and(|p| !folder_paths.contains_key(&p)) {
                next.push(row);
                continue;
            }
            let c = &mut ws.collections[*collections
                .get(&row["collection_id"].as_i64().unwrap_or(0))
                .context("Folder refers to a missing collection")?];
            let prefix = parent
                .and_then(|p| folder_paths.get(&p))
                .map(|(_, p)| format!("{p}/"))
                .unwrap_or_default();
            let name = safe_name(&text(&row["name"]));
            let mut folder = format!("{prefix}{name}");
            let mut suffix = 2;
            while c.folders.iter().any(|f| f.eq_ignore_ascii_case(&folder)) {
                folder = format!("{prefix}{name} {suffix}");
                suffix += 1;
            }
            let mut folder_request = parent_metadata(
                &row,
                c.path.join(&folder).join("folder.req"),
                "folder",
                &c.id,
                &folder,
            )?;
            folder_request.seq = row["sort_order"].as_f64().unwrap_or(0.);
            storage::save_request(&folder_request)?;
            tab_ids.insert(
                format!("folder:{}", text(&row["uid"])),
                format!("__folder__{}:{folder}", c.id),
            );
            folder_paths.insert(key(&row), (c.id.clone(), folder.clone()));
            c.folders.push(folder);
        }
        ensure!(
            next.len() < before,
            "Invalid cyclic or missing parent in legacy folder tree"
        );
        todo = next;
    }
    let mut request_ids = HashMap::new();
    for row in ordered(rows("requests").clone()) {
        let data = payload(&row, "data")?;
        let mut r = request(&data, legacy, stage, target)?;
        defaults(&mut r, &data, &general);
        r.id = uid(&row, "request");
        r.name = text(&row["name"]);
        r.seq = row["sort_order"].as_f64().unwrap_or(0.);
        let c = &ws.collections[*collections
            .get(&row["collection_id"].as_i64().unwrap_or(0))
            .context("Request refers to a missing collection")?];
        r.collection = c.id.clone();
        r.folder = row["folder_id"]
            .as_i64()
            .and_then(|id| folder_paths.get(&id))
            .map(|(_, p)| p.clone())
            .unwrap_or_default();
        let name = safe_name(&r.name);
        let mut path = c.path.join(&r.folder).join(format!("{name}.req"));
        let mut suffix = 2;
        while path.exists() {
            path = c.path.join(&r.folder).join(format!("{name} {suffix}.req"));
            suffix += 1;
        }
        r.path = Some(path);
        r.portable["legacy_row"] = row.clone();
        // Keep imported examples in the existing lossless export envelope.
        let examples = ordered(
            rows("examples")
                .iter()
                .filter(|e| e["request_id"].as_i64() == Some(key(&row)))
                .cloned()
                .collect(),
        );
        let mut responses = vec![];
        for e in examples {
            let snapshot = payload(&e, "data")?;
            let response = &snapshot["response"];
            responses.push(json!({"name":e["name"],"x-getman":snapshot,"code":response["status"],"status":response["reason"],"body":response["body"],"header":response["headers"]}));
        }
        if !responses.is_empty() {
            r.portable["__reqon_responses"] = json!(responses);
        }
        storage::save_request(&r)?;
        ws.saved_fingerprints.insert(
            r.id.clone(),
            crate::disk_sync::fingerprint(&storage::to_bru(&r)),
        );
        tab_ids.insert(format!("request:{}", text(&row["uid"])), r.id.clone());
        request_ids.insert(key(&row), r.id.clone());
        ws.requests.push(r);
    }
    for row in rows("local_drafts") {
        let data = payload(row, "data")?;
        let request_data = if row["kind"] == "example" {
            &data["request"]
        } else {
            &data
        };
        let mut r = request(request_data, legacy, stage, target)?;
        defaults(&mut r, request_data, &general);
        r.id = format!("draft-{}", text(&row["uid"]));
        r.name = text(&row["name"]);
        r.portable["legacy_draft"] = row.clone();
        tab_ids.insert(format!("draft:{}", text(&row["uid"])), r.id.clone());
        ws.requests.push(r);
    }
    let mut env_names = HashMap::new();
    let mut environments = rows("environments").clone();
    environments.sort_by_key(|e| {
        rows("environment_details")
            .iter()
            .find(|d| d["environment_id"].as_i64() == Some(key(e)))
            .and_then(|d| d["sort_order"].as_i64())
            .unwrap_or(key(e))
    });
    for row in environments {
        let detail = rows("environment_details")
            .iter()
            .find(|d| d["environment_id"].as_i64() == Some(key(&row)));
        let values = if let Some(detail) = detail {
            payload(detail, "rows")?
        } else {
            payload(&row, "variables")?
        };
        let mut name = text(&row["name"]);
        let base = name.clone();
        let mut suffix = 2;
        while ws.environments.iter().any(|e| e.name == name) {
            name = format!("{base} {suffix}");
            suffix += 1;
        }
        env_names.insert(key(&row), name.clone());
        ws.environments.push(Environment {
            name,
            vars: vars(&values),
        });
    }
    for row in rows("history") {
        let data = payload(row, "data")?;
        let request_data = data.get("request").unwrap_or(&data);
        let mut r = request(request_data, legacy, stage, target)?;
        if r.url.is_empty() {
            r.url = text(&row["url"]);
        }
        r.portable["legacy_history"] = row.clone();
        let response = data.get("response").unwrap_or(&data);
        let reply = Reply {
            status: response["status"]
                .as_u64()
                .or_else(|| row["status"].as_u64())
                .unwrap_or(0) as u16,
            reason: text(&response["reason"]),
            body: text(&response["body"]),
            elapsed_ms: response["duration"]
                .as_u64()
                .or_else(|| row["duration"].as_u64())
                .unwrap_or(0) as u128,
            headers: header_values(&response["headers"]),
            trailers: header_values(&response["trailers"]),
            url: r.url.clone(),
            size: response["size"].as_u64().unwrap_or(0) as usize,
            ..Default::default()
        };
        let date = text(&row["created_at"]);
        let at = time::PrimitiveDateTime::parse(
            &date,
            time::macros::format_description!("[year]-[month]-[day] [hour]:[minute]:[second]"),
        )
        .map(|d| d.assume_utc().unix_timestamp().max(0) as u64)
        .unwrap_or(0);
        ws.history.push(History {
            at,
            request: r,
            reply,
        });
    }
    if let Some(session) = state.get("session") {
        let mut active = None;
        for (i, entry) in session["tabs"].as_array().into_iter().flatten().enumerate() {
            if let Some(id) =
                tab_ids.get(&format!("{}:{}", text(&entry["kind"]), text(&entry["uid"])))
            {
                if !ws.open_tabs.contains(id) {
                    ws.open_tabs.push(id.clone());
                }
                if session["active"].as_u64() == Some(i as u64) {
                    active = Some(id.clone());
                }
            }
        }
        ws.active = active.or_else(|| ws.open_tabs.first().cloned());
        ws.workspace_environment = session["environment"]
            .as_i64()
            .and_then(|i| env_names.get(&i))
            .cloned()
            .unwrap_or_default();
    }
    // Drafts absent from the previous tab strip remain accessible as local tabs.
    for r in ws.requests.iter().filter(|r| r.id.starts_with("draft-")) {
        if !ws.open_tabs.contains(&r.id) {
            ws.open_tabs.push(r.id.clone());
        }
    }
    if ws.active.is_none() {
        ws.active = ws.open_tabs.first().cloned();
    }
    storage::atomic_write(
        &stage.join("legacy-metadata.json"),
        serde_json::to_vec_pretty(
            &json!({"workspace_state":state,"environments":rows("environments"),"environment_details":rows("environment_details"),"backup":backup}),
        )?,
    )?;
    if legacy.join("cookies.txt").is_file() {
        fs::copy(legacy.join("cookies.txt"), backup.join("cookies.txt"))?;
        crate::cookies::import_legacy(legacy, &stage)?;
    }
    let report = json!({"collections":ws.collections.len(),"requests":rows("requests").len(),"drafts":rows("local_drafts").len(),"folders":rows("folders").len(),"examples":rows("examples").len(),"environments":ws.environments.len(),"history":ws.history.len(),"open_tabs":ws.open_tabs.len(),"source_unchanged":true});
    Ok((ws, report))
}
fn header_values(value: &Value) -> Vec<(String, String)> {
    if let Some(object) = value.as_object() {
        return object.iter().map(|(k, v)| (k.clone(), text(v))).collect();
    }
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((p.get(0)?.as_str()?.into(), p.get(1)?.as_str()?.into())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;
    fn fixture(path: &Path) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL;
            CREATE TABLE collections(id INTEGER PRIMARY KEY,name TEXT,kind TEXT,uid TEXT,sort_order INTEGER,description TEXT,variables TEXT,import_metadata TEXT);
            CREATE TABLE folders(id INTEGER PRIMARY KEY,collection_id INTEGER,parent_id INTEGER,name TEXT,uid TEXT,sort_order INTEGER,import_metadata TEXT);
            CREATE TABLE requests(id INTEGER PRIMARY KEY,collection_id INTEGER,folder_id INTEGER,name TEXT,uid TEXT,sort_order INTEGER,data TEXT);
            CREATE TABLE local_drafts(uid TEXT PRIMARY KEY,kind TEXT,name TEXT,data TEXT);
            CREATE TABLE examples(id INTEGER PRIMARY KEY,request_id INTEGER,name TEXT,uid TEXT,sort_order INTEGER,data TEXT);
            CREATE TABLE environments(id INTEGER PRIMARY KEY,name TEXT,variables TEXT);
            CREATE TABLE environment_details(environment_id INTEGER,uid TEXT,rows TEXT,sort_order INTEGER,color TEXT);
            CREATE TABLE history(id INTEGER PRIMARY KEY,created_at TEXT,method TEXT,url TEXT,status INTEGER,duration INTEGER,data TEXT);
            CREATE TABLE workspace_state(key TEXT PRIMARY KEY,data TEXT);
            CREATE TABLE future_metadata(id INTEGER PRIMARY KEY,opaque TEXT);
            INSERT INTO future_metadata VALUES(1,'keep every unknown field');
            INSERT INTO collections VALUES(1,'My API','mixed','c1',0,'Collection docs','{\"base\":\"https://example.test\"}','{\"auth\":{\"type\":\"bearer\",\"bearer\":[{\"key\":\"token\",\"value\":\"inherited\"}]}}');
            INSERT INTO folders VALUES(2,1,NULL,'Folder','f2',0,'{}');
            INSERT INTO folders VALUES(3,1,2,'Nested','f3',0,'{}');
            INSERT INTO environments VALUES(7,'Local','{}');
            INSERT INTO environment_details VALUES(7,'env7','[{\"key\":\"token\",\"value\":\"secret value\",\"type\":\"secret\"},{\"key\":\"disabled\",\"value\":\"kept\",\"enabled\":false}]',0,'#123456');
            INSERT INTO workspace_state VALUES('session','{\"tabs\":[{\"kind\":\"request\",\"uid\":\"g4\"},{\"kind\":\"draft\",\"uid\":\"draft5\"},{\"kind\":\"request\",\"uid\":\"h6\"}],\"active\":0,\"environment\":7}');
            INSERT INTO workspace_state VALUES('general','{\"autosave\":true,\"font_size\":15,\"timeout_ms\":3100,\"verify_ssl\":true}');
            INSERT INTO workspace_state VALUES('environment_globals','[{\"key\":\"global\",\"value\":\"yes\"}]');
        ").unwrap();
        let grpc = json!({"kind":"grpc","name":"RPC","server":"localhost:9000","service":"demo.API","rpc_method":"Call","message":"{\"name\":\"value\"}","metadata_rows":[["x-id","one","desc",true],["x-id","two","",true],["off","saved","",false]],"tls":true,"tls_verify":false,"grpc_timeout_ms":1750,"general_overrides":true,"auth_type":"Bearer Token","auth_value":"token","unknown":{"preserve":true}});
        let http = json!({"kind":"http","name":"HTTP","url":"https://example.test","method":"POST","body_type":"raw","raw_language":"JSON","body":"{}","http_timeout":1.25,"general_overrides":true,"param_rows":[["q","a b","description",true]],"header_rows":[["X-Test","a","keep",false]],"auth_type":"Basic Auth","auth_value":"user:pass"});
        db.execute(
            "INSERT INTO requests VALUES(4,1,3,'RPC','g4',1,?)",
            [grpc.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO requests VALUES(6,1,NULL,'HTTP','h6',2,?)",
            [http.to_string()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO local_drafts VALUES('draft5','http','Draft',?)",
            [http.to_string()],
        )
        .unwrap();
        db.execute("INSERT INTO examples VALUES(8,6,'Example','e8',0,?)",[json!({"request":http,"response":{"status":200,"body":"example response","headers":[["X-Example","yes"]]}}).to_string()]).unwrap();
        db.execute("INSERT INTO history VALUES(9,'2026-11-14 10:20:30','POST','https://example.test',201,25,?)",[http.to_string()]).unwrap();
        db
    }
    #[test]
    fn upgrade_preserves_wal_rows_metadata_drafts_order_and_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("reqon.sqlite3");
        let db = fixture(&source);
        let target = d.path().join("native");
        let source_rows = table(&db, "requests").unwrap();
        let report = migrate(&source, &target).unwrap();
        assert_eq!(report["requests"], 2);
        assert_eq!(report["drafts"], 1);
        assert_eq!(report["examples"], 1);
        assert_eq!(table(&db, "requests").unwrap(), source_rows);
        let ws = storage::load(&target).unwrap();
        assert_eq!(
            ws.open_tabs,
            vec!["request-g4", "draft-draft5", "request-h6"]
        );
        assert_eq!(ws.active.as_deref(), Some("request-g4"));
        assert_eq!(ws.requests.len(), 3);
        assert_eq!(ws.collections[0].folders, vec!["Folder", "Folder/Nested"]);
        let g = ws.requests.iter().find(|r| r.id == "request-g4").unwrap();
        assert_eq!(g.grpc_method, "demo.API/Call");
        assert_eq!(g.headers.len(), 3);
        assert!(!g.headers[2].enabled);
        assert_eq!(g.headers[0].description, "desc");
        assert_eq!(g.timeout, 1750);
        assert_eq!(g.auth.token, "token");
        assert_eq!(g.portable["unknown"]["preserve"], true);
        let h = ws.requests.iter().find(|r| r.id == "request-h6").unwrap();
        assert_eq!(h.timeout, 1250);
        assert_eq!(h.body, "{}");
        assert_eq!(h.auth.password, "pass");
        assert_eq!(
            h.portable["__reqon_responses"][0]["body"],
            "example response"
        );
        assert_eq!(ws.environments[0].vars[0].value, "secret value");
        assert!(!ws.environments[0].vars[1].enabled);
        assert_eq!(ws.workspace_environment, "Local");
        assert_eq!(ws.globals[0].key, "global");
        assert_eq!(ws.history.len(), 1);
        let before = fs::read(target.join("workspace.json")).unwrap();
        assert_eq!(migrate(&source, &target).unwrap()["already_migrated"], true);
        assert_eq!(before, fs::read(target.join("workspace.json")).unwrap());
        let backup = Connection::open(
            Path::new(report["backup"].as_str().unwrap()).join("workspace-0.19.sqlite3"),
        )
        .unwrap();
        assert_eq!(
            table(&backup, "future_metadata").unwrap(),
            table(&db, "future_metadata").unwrap()
        );
    }
    #[test]
    fn corrupt_record_does_not_publish_partial_profile_or_modify_database() {
        let d = tempfile::tempdir().unwrap();
        let source = d.path().join("reqon.sqlite3");
        let db = fixture(&source);
        db.execute(
            "UPDATE requests SET data=? WHERE id=6",
            params!["invalid JSON"],
        )
        .unwrap();
        let target = d.path().join("native");
        assert!(migrate(&source, &target).is_err());
        assert!(!target.join("workspace.json").exists());
        assert_eq!(
            db.query_row("SELECT data FROM requests WHERE id=6", [], |r| r
                .get::<_, String>(0))
                .unwrap(),
            "invalid JSON"
        );
        db.execute("UPDATE requests SET data='{}' WHERE id=6", [])
            .unwrap();
        assert!(migrate(&source, &target).is_ok());
    }
    #[test]
    fn explicit_profiles_are_isolated_and_existing_native_data_wins() {
        let d = tempfile::tempdir().unwrap();
        let exe = d.path().join("Reqon.exe");
        let explicit = d.path().join("qa");
        assert_eq!(default_root(&exe, Some(explicit.clone())), explicit);
        assert!(auto_source(&explicit, true).is_none());
        fs::create_dir(&explicit).unwrap();
        fs::write(explicit.join("reqon.sqlite3"), b"old").unwrap();
        assert_eq!(
            default_root(&exe, Some(explicit.clone())),
            explicit.join("native")
        );
        fs::write(explicit.join("workspace.json"), b"{}").unwrap();
        assert_eq!(default_root(&exe, Some(explicit.clone())), explicit);
        assert!(auto_source(&explicit, true).is_none());
    }
}
