//! Collection environment secrets use the same immutable local vault as Auth.
//! Public exports still omit values; saved collection files carry opaque refs.
use crate::{
    auth_vault,
    model::{Environment, Pair},
};
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, path::Path};

pub fn is_reference(value: &str) -> bool {
    value.starts_with("{{reqonSecret:")
}
pub fn capture_values(
    names: &[String],
    vars: &BTreeMap<String, String>,
    secrets: &mut Vec<String>,
) {
    for name in names {
        if let Some(value) = vars.get(name) {
            let value = crate::model::resolve(value, vars);
            if !value.is_empty() {
                secrets.push(value);
            }
        }
    }
}
fn protect_at(root: &Path, rows: &[Pair]) -> Result<Vec<Pair>> {
    let mut rows = rows.to_vec();
    for row in &mut rows {
        if is_reference(&row.value) {
            ensure!(
                auth_vault::reference_id(&row.value).is_some(),
                "Invalid secret reference for variable '{}'",
                row.key
            );
            row.secret = true;
        } else if row.secret && !row.value.is_empty() {
            row.value = auth_vault::store_at(root, &row.value)?;
        }
    }
    Ok(rows)
}
fn read_value(root: &Path, value: &str) -> Result<String> {
    let id = auth_vault::reference_id(value).context("Invalid local secret reference")?;
    String::from_utf8(crate::secrets::unprotect(&fs::read(
        root.join(format!("{id}.bin")),
    )?)?)
    .context("Invalid encrypted variable value")
}
pub fn resolve_rows(rows: &mut [Pair], required: bool) -> Result<()> {
    resolve_rows_at(&auth_vault::root()?, rows, required)
}
fn resolve_rows_at(root: &Path, rows: &mut [Pair], required: bool) -> Result<()> {
    for row in rows {
        if is_reference(&row.value) {
            row.secret = true;
            match read_value(root, &row.value) {
                Ok(value) => row.value = value,
                Err(_) if !required || !row.enabled => continue,
                Err(_) => anyhow::bail!(
                    "Secret variable '{}' is unavailable on this computer. Enter its value in the environment or use --secret NAME=ENV_NAME.",
                    row.key
                ),
            }
        }
        ensure!(
            !required || !row.enabled || !row.secret || !row.value.is_empty(),
            "Secret variable '{}' has no local value. Enter it in the environment or use --secret NAME=ENV_NAME.",
            row.key
        );
    }
    Ok(())
}
/// Resolve references supplied directly to Engine, before any scripts/network.
pub fn resolve_variables(
    vars: &mut BTreeMap<String, String>,
    secrets: &mut Vec<String>,
) -> Result<()> {
    for (key, value) in vars {
        if is_reference(value) {
            *value = read_value(&auth_vault::root()?, value).map_err(|_| {
                anyhow::anyhow!("Secret variable '{key}' is unavailable on this computer")
            })?;
            secrets.push(value.clone());
        }
    }
    Ok(())
}
pub fn mark_secret_names(rows: &mut Vec<Pair>, names: &[String]) {
    for name in names {
        if rows.iter().any(|p| &p.key == name) {
            for row in rows.iter_mut().filter(|p| &p.key == name) {
                row.secret = true;
            }
        } else {
            let mut row = Pair::new(name, "");
            row.secret = true;
            rows.push(row);
        }
    }
}
pub fn parse_bru(name: String, source: &str) -> Environment {
    let mut vars = crate::storage::blocks(source)
        .into_iter()
        .filter(|(name, _)| name == "vars")
        .flat_map(|(_, body)| crate::bru::pairs(&body))
        .collect();
    mark_secret_names(&mut vars, &crate::storage::secret_names(source));
    Environment { name, vars }
}

/// Only called for disk saves, never by public exports or previews.
pub fn source(env: &Environment, yaml: bool, original: &str) -> Result<String> {
    source_at(&auth_vault::root()?, env, yaml, original)
}
fn source_at(root: &Path, env: &Environment, yaml: bool, original: &str) -> Result<String> {
    let rows = protect_at(root, &env.vars)?;
    if yaml {
        let mut value: serde_json::Value = if original.trim().is_empty() {
            serde_json::json!({})
        } else {
            serde_yaml::from_str(original)
                .map_err(|_| anyhow::anyhow!("Invalid environment YAML"))?
        };
        ensure!(value.is_object(), "Expected environment YAML object");
        value["name"] = env.name.clone().into();
        let mut serialized = crate::yaml::row_values(&rows, true);
        for (out, row) in serialized
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip(rows.iter().filter(|p| !p.key.is_empty()))
        {
            if row.secret && !row.value.is_empty() {
                out["value"] = row.value.clone().into();
            }
            // Preserve vendor metadata while replacing all known variable fields.
            if let Some(old) = value["variables"]
                .as_array()
                .and_then(|a| a.iter().find(|v| v["name"] == row.key))
            {
                if let Some(old) = old.as_object() {
                    for (key, value) in old {
                        if ![
                            "name",
                            "key",
                            "value",
                            "secret",
                            "type",
                            "disabled",
                            "description",
                        ]
                        .contains(&key.as_str())
                        {
                            out[key] = value.clone();
                        }
                    }
                }
            }
        }
        value["variables"] = serialized;
        return Ok(serde_yaml::to_string(&value)?);
    }
    let names = rows
        .iter()
        .filter(|p| p.secret && !p.key.is_empty())
        .map(|p| format!("  {}", serde_json::to_string(&p.key).unwrap()))
        .collect::<Vec<_>>()
        .join("\n");
    let mut visible = rows;
    for row in &mut visible {
        row.secret = false;
    }
    let body = crate::bru::rows_text(&visible);
    let mut edits = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for (name, start, end) in crate::auth_migration::blocks(original)? {
        if name == "vars" || name == "vars:secret" {
            let text = if seen.insert(name.clone()) {
                if name == "vars" { &body } else { &names }
            } else {
                ""
            };
            edits.push((start, end, format!("{text}\n")));
        }
    }
    let mut output = original.to_owned();
    for (start, end, text) in edits.into_iter().rev() {
        output.replace_range(start..end, &text);
    }
    if !output.is_empty() && !output.ends_with('\n') {
        output.push('\n');
    }
    if !seen.contains("vars") {
        output.push_str(&format!("vars {{\n{body}\n}}\n"));
    }
    if !seen.contains("vars:secret") && !names.is_empty() {
        output.push_str(&format!("vars:secret [\n{names}\n]\n"));
    }
    Ok(output)
}

pub fn migrate_yaml(
    source: &str,
    seal: &mut impl FnMut(&str) -> Result<String>,
) -> Result<(String, usize)> {
    let mut value: serde_json::Value =
        serde_yaml::from_str(source).map_err(|_| anyhow::anyhow!("Invalid environment YAML"))?;
    ensure!(value.is_object(), "Expected environment YAML object");
    ensure!(
        value["variables"].is_null() || value["variables"].is_array(),
        "Expected environment variables list"
    );
    let mut count = 0;
    if let Some(rows) = value["variables"].as_array_mut() {
        for row in rows {
            if row["secret"] != true {
                continue;
            }
            let value = if row["value"].is_object() && row["value"].get("type").is_some() {
                &mut row["value"]["data"]
            } else {
                &mut row["value"]
            };
            if value.is_null() {
                continue;
            }
            let plain = value
                .as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| value.to_string());
            ensure!(
                !is_reference(&plain) || auth_vault::reference_id(&plain).is_some(),
                "Invalid secret variable reference"
            );
            if !plain.is_empty() && !is_reference(&plain) {
                *value = seal(&plain)?.into();
                count += 1;
            }
        }
    }
    Ok((
        if count == 0 {
            source.to_owned()
        } else {
            serde_yaml::to_string(&value)?
        },
        count,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::*, storage};
    fn secret(name: &str, value: &str) -> Pair {
        Pair {
            secret: true,
            ..Pair::new(name, value)
        }
    }
    #[test]
    fn protected_sources_preserve_metadata_and_survive_copy_rename_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        let mut hidden = secret("weird \" key", "789012345");
        hidden.enabled = false;
        hidden.kind = "number".into();
        hidden.description = "Keep this description".into();
        let mut env = Environment {
            name: "Local".into(),
            vars: vec![
                secret("TOKEN", "environment-private-old-294857"),
                hidden,
                Pair::new("region", "local"),
            ],
        };
        let original = "# keep comment\r\ncustom {\r\n  vendor: unchanged\r\n}\r\nvars {\r\n}\r\nvars:secret [\r\n TOKEN\r\n TOKEN\r\n]\r\n";
        let output = source_at(&vault, &env, false, original).unwrap();
        assert!(!output.contains("environment-private-old-294857"));
        assert!(output.starts_with("# keep comment\r\ncustom {\r\n  vendor: unchanged\r\n}"));
        let mut loaded = parse_bru("Copy".into(), &output);
        assert_eq!(loaded.vars.len(), 3);
        let reference = loaded.vars[0].value.clone();
        assert_eq!(
            serde_json::to_value(&loaded.vars[0]).unwrap()["value"],
            reference
        );
        resolve_rows_at(&vault, &mut loaded.vars, true).unwrap();
        assert_eq!(loaded.vars, env.vars);
        assert_eq!(source_at(&vault, &env, false, &output).unwrap(), output);
        env.name = "Renamed".into();
        env.vars[0].value = "environment-private-new-384920".into();
        let rotated = source_at(&vault, &env, false, &output).unwrap();
        assert!(!rotated.contains(&reference));
        let mut copy = parse_bru("Copy".into(), &output);
        resolve_rows_at(&vault, &mut copy.vars, true).unwrap();
        assert_eq!(copy.vars[0].value, "environment-private-old-294857");
        assert!(
            !crate::environments::bru_environment_source(&loaded).contains("environment-private")
        );
        assert!(
            !crate::yaml::environment_source(&loaded)
                .unwrap()
                .contains("environment-private")
        );
        assert!(
            !serde_json::to_string(&loaded)
                .unwrap()
                .contains("environment-private")
        );
    }
    #[test]
    fn yaml_protection_preserves_typed_secrets_unknown_fields_and_disabled_rows() {
        assert!(migrate_yaml("scalar", &mut |_| panic!("must not seal malformed data")).is_err());
        assert!(
            migrate_yaml("variables: bad", &mut |_| panic!(
                "must not seal malformed data"
            ))
            .is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let original = "name: Local\nvendor: keep\nvariables:\n- name: TOKEN\n  secret: true\n  value: yaml-private-185934\n  vendorRow: keep\n- name: NUMBER\n  secret: true\n  disabled: true\n  value:\n    type: number\n    data: 123456789\n";
        let env = crate::yaml::environment(original).unwrap();
        let output = source_at(dir.path(), &env, true, original).unwrap();
        assert!(!output.contains("yaml-private") && !output.contains("123456789"));
        let value: serde_json::Value = serde_yaml::from_str(&output).unwrap();
        assert_eq!(value["vendor"], "keep");
        assert_eq!(value["variables"][0]["vendorRow"], "keep");
        let mut loaded = crate::yaml::environment(&output).unwrap();
        resolve_rows_at(dir.path(), &mut loaded.vars, true).unwrap();
        assert_eq!(loaded.vars, env.vars);
    }
    #[test]
    fn missing_corrupt_and_empty_values_fail_closed_but_disabled_rows_do_not_block() {
        let dir = tempfile::tempdir().unwrap();
        let row = secret("TOKEN", "private-failure-183920");
        let protected = protect_at(dir.path(), &[row]).unwrap();
        let reference = protected[0].value.clone();
        let id = auth_vault::reference_id(&reference).unwrap();
        fs::write(dir.path().join(format!("{id}.bin")), b"damaged").unwrap();
        let mut rows = protected;
        resolve_rows_at(dir.path(), &mut rows, false).unwrap();
        assert_eq!(rows[0].value, reference);
        assert!(
            resolve_rows_at(dir.path(), &mut rows, true)
                .unwrap_err()
                .to_string()
                .contains("TOKEN")
        );
        rows[0].enabled = false;
        resolve_rows_at(dir.path(), &mut rows, true).unwrap();
        assert!(resolve_rows_at(dir.path(), &mut [secret("EMPTY", "")], true).is_err());
        fs::write(dir.path().join("key.bin"), b"damaged").unwrap();
        assert!(protect_at(dir.path(), &[secret("NEW", "new-private-value")]).is_err());
        assert!(protect_at(dir.path(), &[secret("BAD", "{{reqonSecret:invalid}}")]).is_err());
    }
    #[test]
    fn interactive_protocols_resolve_secrets_and_reject_missing_values_before_connection() {
        let reference =
            auth_vault::store_at(&auth_vault::root().unwrap(), "protocol-env-private-421948")
                .unwrap();
        let (_, vars) = crate::protocol_prepare::prepare_protocol(
            Request::default(),
            Preferences::default(),
            (vec![], vec![secret("TOKEN", &reference)], BTreeMap::new()),
        )
        .unwrap();
        assert_eq!(vars["TOKEN"], "protocol-env-private-421948");
        assert!(
            crate::protocol_prepare::prepare_protocol(
                Request::default(),
                Preferences::default(),
                (vec![], vec![secret("TOKEN", "")], BTreeMap::new())
            )
            .is_err()
        );
    }
    #[test]
    fn legacy_profile_secrets_become_cli_readable_after_environment_save() {
        let dir = tempfile::tempdir().unwrap();
        let mut ws = storage::load(dir.path()).unwrap();
        ws.collections[0].environments[0]
            .vars
            .push(secret("TOKEN", "legacy-profile-private-592843"));
        storage::save(dir.path(), &ws).unwrap();
        let restored = storage::load(dir.path()).unwrap();
        storage::save_environments(&restored.collections[0]).unwrap();
        let (collection, _, _) = storage::read_collection(&restored.collections[0].path).unwrap();
        assert_eq!(
            collection
                .environments
                .iter()
                .find(|e| e.name == "Local")
                .unwrap()
                .vars
                .last()
                .unwrap()
                .value,
            "legacy-profile-private-592843"
        );
        let source = fs::read_to_string(collection.path.join("environments/Local.req")).unwrap();
        assert!(source.contains("reqonSecret:") && !source.contains("legacy-profile-private"));
        assert!(
            !fs::read_to_string(dir.path().join("workspace.json"))
                .unwrap()
                .contains("legacy-profile-private")
        );
    }
    #[tokio::test]
    async fn runner_resolves_environment_scripts_and_redacts_rotated_secret_values() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let mut seen = vec![];
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut bytes = vec![];
                let mut buffer = [0; 2048];
                while !bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buffer).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                }
                let request = String::from_utf8(bytes).unwrap();
                let value = request
                    .lines()
                    .find(|l| l.starts_with("x-token:"))
                    .unwrap()
                    .trim_start_matches("x-token:")
                    .trim()
                    .to_owned();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    value.len(),
                    value
                );
                socket.write_all(response.as_bytes()).unwrap();
                seen.push(value);
            }
            seen
        });
        let original = "runner-env-private-first-183920";
        let rotated = "runner-env-private-next-294851";
        let mut env = vec![secret("TOKEN", "{{RAW}}"), Pair::new("RAW", original)];
        env = protect_at(&auth_vault::root().unwrap(), &env).unwrap();
        let request = Request { url: format!("http://{address}/"), headers: vec![Pair::new("X-Token", "{{TOKEN}}")],
            pre_script: "pm.test('environment is decrypted',()=>pm.expect(pm.environment.get('TOKEN').includes('reqonSecret:')).equal(false));".into(),
            post_script: format!("pm.environment.set('TOKEN','{rotated}'); console.log(pm.environment.get('TOKEN'));"),
            ..Default::default() };
        let mut plan = crate::collection_runner::Plan::new(
            vec![request],
            crate::collection_runner::Options {
                iterations: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let mut session =
            crate::collection_runner::Session::new(crate::engine::Engine::default(), vec![], env);
        let report = crate::collection_runner::run(&mut plan, &mut session).await;
        assert!(report.success(), "{}", report.public_json(&[]).unwrap());
        assert_eq!(server.join().unwrap(), [original, rotated]);
        let public = report.public_json(&[]).unwrap().to_string();
        assert!(!public.contains(original) && !public.contains(rotated));
        assert!(session.environment[0].secret);
        let missing = vec![secret(
            "TOKEN",
            &format!("{{{{reqonSecret:{}}}}}", "de".repeat(32)),
        )];
        let reply = crate::engine::Engine::default()
            .execute_scoped(Request::default(), vec![], missing, BTreeMap::new())
            .await;
        assert!(reply.error.contains("TOKEN") && reply.status == 0);
    }
}
