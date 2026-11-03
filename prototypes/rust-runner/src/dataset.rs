use crate::{
    input,
    runtime::{Variables, validate_variables},
};
use anyhow::{Result, bail};
use serde_json::Value;
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Dataset {
    pub rows: Vec<Variables>,
    pub sha256: String,
    pub path: PathBuf,
}

impl Dataset {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            input::read(path).map_err(|_| anyhow::anyhow!("cannot read bounded data file"))?;
        let rows = match path.extension().and_then(|s| s.to_str()) {
            Some("json") => {
                let value = input::json_value(&bytes)
                    .map_err(|_| anyhow::anyhow!("dataset must be JSON with unique keys"))?;
                serde_json::from_value::<Vec<Variables>>(value)
                    .map_err(|_| anyhow::anyhow!("JSON dataset must be an array of objects"))?
            }
            Some("csv") => {
                let mut reader = csv::ReaderBuilder::new().from_reader(bytes.as_slice());
                let headers = reader.headers()?.clone();
                let mut seen = HashSet::new();
                if headers.is_empty()
                    || headers
                        .iter()
                        .any(|s| !crate::runtime::valid_name(s) || !seen.insert(s))
                {
                    bail!("CSV requires unique safe column names");
                }
                reader
                    .records()
                    .map(|row| {
                        Ok(headers
                            .iter()
                            .zip(row?.iter())
                            .map(|(k, v)| (k.into(), Value::String(v.into())))
                            .collect())
                    })
                    .collect::<Result<Vec<Variables>>>()?
            }
            _ => bail!("dataset extension must be .json or .csv"),
        };
        if rows.is_empty() || rows.len() > 10_000 {
            bail!("dataset requires 1..10000 rows");
        }
        for row in &rows {
            validate_variables(row)?;
        }
        Ok(Self {
            rows,
            sha256: crate::package::hash_bytes(&bytes),
            path: path.canonicalize()?,
        })
    }
}
