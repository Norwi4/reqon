//! Bounded input data for collection runs. Values stay typed in JavaScript.
use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;
use std::{collections::BTreeMap, io::Read, path::Path};

pub type Row = BTreeMap<String, Value>;
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_COLUMNS: usize = 1024;

#[derive(Clone, Debug)]
pub struct Dataset {
    rows: Vec<Row>,
}
impl Dataset {
    pub fn read(path: &Path) -> Result<Self> {
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .context("Cannot open data file")?
            .take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut bytes)
            .context("Cannot read data file")?;
        ensure!(bytes.len() <= MAX_BYTES, "Data file exceeds 16 MiB");
        let text = std::str::from_utf8(&bytes).context("Data file must be UTF-8")?;
        Self::parse(
            text,
            path.extension().and_then(|s| s.to_str()).unwrap_or(""),
        )
    }
    pub fn parse(text: &str, format: &str) -> Result<Self> {
        ensure!(text.len() <= MAX_BYTES, "Data file exceeds 16 MiB");
        let text = text.trim_start_matches('\u{feff}');
        let rows = match format.to_ascii_lowercase().as_str() {
            "json" => Self::json(text)?,
            "csv" => Self::csv(text)?,
            _ => bail!("Data file must have a .csv or .json extension"),
        };
        ensure!(!rows.is_empty(), "Data file has no rows");
        ensure!(rows.len() <= MAX_ROWS, "Data file exceeds 10000 rows");
        Ok(Self { rows })
    }
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }
    fn json(text: &str) -> Result<Vec<Row>> {
        // serde_json errors can contain input values; expose only the position.
        let value: Value = serde_json::from_str(text).map_err(|e| {
            anyhow::anyhow!(
                "Invalid JSON data at line {}, column {}",
                e.line(),
                e.column()
            )
        })?;
        let values = match value {
            Value::Array(rows) => rows,
            Value::Object(mut object) => {
                match object.remove("data").or_else(|| object.remove("rows")) {
                    Some(Value::Array(rows)) => rows,
                    _ => bail!(
                        "JSON data must be an array of objects or an object with a data/rows array"
                    ),
                }
            }
            _ => bail!("JSON data must be an array of objects or an object with a data/rows array"),
        };
        ensure!(values.len() <= MAX_ROWS, "Data file exceeds 10000 rows");
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let Value::Object(object) = value else {
                    bail!("Data row {} must be an object", index + 1)
                };
                ensure!(
                    object.len() <= MAX_COLUMNS,
                    "Data row {} exceeds 1024 columns",
                    index + 1
                );
                let mut row = Row::new();
                for (key, value) in object {
                    let key = key.trim();
                    ensure!(
                        !key.is_empty(),
                        "Data row {} contains an empty column name",
                        index + 1
                    );
                    ensure!(
                        row.insert(key.to_owned(), value).is_none(),
                        "Data row {} contains duplicate column names",
                        index + 1
                    );
                }
                Ok(row)
            })
            .collect()
    }
    fn csv(text: &str) -> Result<Vec<Row>> {
        let mut reader = csv::ReaderBuilder::new().from_reader(text.as_bytes());
        let headers = reader
            .headers()
            .map_err(|_| anyhow::anyhow!("Cannot read CSV header"))?
            .clone();
        ensure!(
            !headers.is_empty() && headers.len() <= MAX_COLUMNS,
            "CSV requires 1 to 1024 header columns"
        );
        let mut names = std::collections::BTreeSet::new();
        for key in &headers {
            ensure!(
                !key.trim().is_empty(),
                "CSV header contains an empty column name"
            );
            ensure!(
                names.insert(key.trim()),
                "CSV header contains duplicate column names"
            );
        }
        let mut rows = Vec::new();
        for (index, record) in reader.records().enumerate() {
            ensure!(index < MAX_ROWS, "Data file exceeds 10000 rows");
            let record = record.map_err(|_| {
                anyhow::anyhow!(
                    "Invalid CSV data row {}: check encoding and column count",
                    index + 1
                )
            })?;
            rows.push(
                headers
                    .iter()
                    .zip(record.iter())
                    .map(|(k, v)| (k.trim().to_owned(), Value::String(v.to_owned())))
                    .collect(),
            );
        }
        Ok(rows)
    }
}

pub(crate) fn variables(row: &Row) -> BTreeMap<String, String> {
    row.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                match value {
                    Value::Null => String::new(),
                    Value::String(value) => value.clone(),
                    value => value.to_string(),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn csv_quotes_newlines_bom_and_json_types() {
        let data = Dataset::parse(
            "\u{feff}name,note,empty\r\nAda,\"first,\"\"line\"\"\nnext\",\r\n",
            "CSV",
        )
        .unwrap();
        assert_eq!(data.rows()[0]["note"], "first,\"line\"\nnext");
        assert_eq!(data.rows()[0]["empty"], "");
        for text in [
            r#"[{"n":9007199254740993,"flag":true,"nil":null,"obj":{"x":1}}]"#,
            r#"{"rows":[{"n":9007199254740993,"flag":true,"nil":null,"obj":{"x":1}}]}"#,
        ] {
            let data = Dataset::parse(text, "json").unwrap();
            assert_eq!(variables(&data.rows()[0])["n"], "9007199254740993");
            assert_eq!(data.rows()[0]["flag"], true);
            assert!(data.rows()[0]["nil"].is_null());
        }
    }
    #[test]
    fn invalid_datasets_fail_without_echoing_values() {
        for (text, kind) in [
            ("a,a\nx,y", "csv"),
            ("a,b\nx", "csv"),
            ("a\n", "csv"),
            ("[]", "json"),
            ("[1]", "json"),
            ("{\"data\":7}", "json"),
            ("[{\"a\":1,\" a \":2}]", "json"),
            ("[private-sentinel]", "json"),
        ] {
            let error = Dataset::parse(text, kind).unwrap_err().to_string();
            assert!(!error.contains("private-sentinel"));
        }
    }
}
