use crate::{model::*, storage};
pub(crate) fn import_environment(path: &std::path::Path) -> anyhow::Result<Environment> {
    let source = std::fs::read_to_string(path)?;
    if crate::yaml::is_yaml(path) {
        let mut env = crate::yaml::environment(&source)?;
        crate::environment_vault::resolve_rows(&mut env.vars, false)?;
        return Ok(env);
    }
    let mut name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let mut vars: Vec<Pair> = if path.extension().is_some_and(|s| s == "json") {
        let value: serde_json::Value = serde_json::from_str(&source)?;
        if let Some(n) = value["name"].as_str() {
            name = n.into();
        }
        value["values"]
            .as_array()
            .context("Environment must contain values")?
            .iter()
            .filter_map(|p| {
                let value = p["value"].as_str().map(str::to_owned).unwrap_or_else(|| {
                    if p["value"].is_null() {
                        String::new()
                    } else {
                        p["value"].to_string()
                    }
                });
                let mut pair = Pair::new(p["key"].as_str()?, value);
                pair.description = p["description"].as_str().unwrap_or_default().into();
                pair.enabled = p["enabled"].as_bool().unwrap_or(true);
                pair.secret = p["type"].as_str() == Some("secret");
                Some(pair)
            })
            .collect()
    } else if crate::native_format::is_source(path) {
        storage::blocks(&source)
            .into_iter()
            .filter(|(k, _)| k == "vars")
            .flat_map(|(_, v)| crate::bru::pairs(&v))
            .collect()
    } else {
        source
            .lines()
            .filter_map(|line| {
                let line = line.trim().trim_start_matches("export ");
                if line.starts_with('#') {
                    return None;
                }
                let (k, v) = line.split_once('=')?;
                Some(Pair::new(k.trim(), v.trim().trim_matches(['\'', '"'])))
            })
            .collect()
    };
    if crate::native_format::is_source(path) {
        crate::environment_vault::mark_secret_names(&mut vars, &storage::secret_names(&source));
    }
    crate::environment_vault::resolve_rows(&mut vars, false)?;
    Ok(Environment { name, vars })
}
pub(crate) fn bru_environment_source(env: &Environment) -> String {
    let public = env
        .vars
        .iter()
        .filter(|p| !p.secret)
        .cloned()
        .collect::<Vec<_>>();
    let mut source = format!("vars {{\n{}\n}}\n", crate::bru::rows_text(&public));
    let secret = env
        .vars
        .iter()
        .filter(|p| p.secret && !p.key.is_empty())
        .map(|p| format!("  {}", p.key))
        .collect::<Vec<_>>();
    if !secret.is_empty() {
        source.push_str(&format!("\nvars:secret [\n{}\n]\n", secret.join("\n")));
    }
    source
}
use anyhow::Context;
