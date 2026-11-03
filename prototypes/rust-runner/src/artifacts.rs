//! Append-only recovery journal and atomic, streaming report publication.
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub type SharedJournal = Arc<Mutex<Journal>>;
const MAX_RECORD: usize = 32 * 1024 * 1024;
pub struct Journal {
    pub path: PathBuf,
    pub run_id: String,
    writer: BufWriter<File>,
    bytes: u64,
}

impl Journal {
    pub fn create(path: &Path, run_id: String) -> Result<SharedJournal> {
        let mut journal = Self {
            path: path.into(),
            run_id,
            writer: BufWriter::new(File::create(path)?),
            bytes: 0,
        };
        journal.record(&json!({"type":"journal_started","schema_version":"reqon-journal/v1","run_id":journal.run_id,"engine_version":env!("CARGO_PKG_VERSION")}))?;
        Ok(Arc::new(Mutex::new(journal)))
    }
    pub fn record(&mut self, value: &impl Serialize) -> Result<()> {
        let bytes = serde_json::to_vec(value)?;
        if bytes.len() > MAX_RECORD {
            bail!("journal record exceeds 32 MiB");
        }
        self.bytes += bytes.len() as u64 + 1;
        if self.bytes > 1024 * 1024 * 1024 {
            bail!("journal exceeds 1 GiB disk budget");
        }
        self.writer.write_all(&bytes)?;
        self.writer.write_all(b"\n")?;
        self.writer.flush()?;
        Ok(())
    }
    pub fn sync(&mut self) -> Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_data()?;
        Ok(())
    }
}

pub fn atomic(
    path: &Path,
    write: impl FnOnce(&mut BufWriter<&mut File>) -> Result<()>,
) -> Result<()> {
    let mut file =
        tempfile::NamedTempFile::new_in(path.parent().context("report parent missing")?)?;
    {
        let mut writer = BufWriter::new(file.as_file_mut());
        write(&mut writer)?;
        writer.flush()?;
    }
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    Ok(())
}
pub fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    atomic(path, |w| {
        serde_json::to_writer(w, value)?;
        Ok(())
    })
}

fn records(path: &Path, mut visit: impl FnMut(Value) -> Result<()>) -> Result<()> {
    let mut reader = BufReader::new(File::open(path)?);
    loop {
        let mut line = Vec::new();
        std::io::Read::take(&mut reader, MAX_RECORD as u64 + 1).read_until(b'\n', &mut line)?;
        if line.is_empty() {
            break;
        }
        if line.len() > MAX_RECORD {
            bail!("journal record exceeds size limit");
        }
        // A kill during the final write may leave a partial last record.
        if !line.ends_with(b"\n") {
            break;
        }
        visit(serde_json::from_slice(&line).context("invalid journal record")?)?;
    }
    Ok(())
}

pub struct Destinations {
    pub json: PathBuf,
    pub junit: PathBuf,
    pub html: Option<PathBuf>,
    pub manifest: PathBuf,
}

pub fn publish(
    journal: &SharedJournal,
    mut metadata: Value,
    xml: Option<String>,
    dest: &Destinations,
    exit_code: i32,
) -> Result<()> {
    let (path, run_id) = {
        let mut j = journal.lock().unwrap();
        j.sync()?;
        (j.path.clone(), j.run_id.clone())
    };
    metadata["run_id"] = json!(run_id);
    metadata["capture_policy"] = json!({"response_bytes_per_step":65536,"check_value_bytes":4096,"case_payload_bytes":1048576,"variables_bytes":65536});
    let package = metadata["schema_version"] == "reqon-package-result/v1";
    atomic(&dest.json, |writer| {
        if !package {
            serde_json::to_writer(writer, &metadata)?;
            return Ok(());
        }
        metadata.as_object_mut().unwrap().remove("cases");
        let mut header = serde_json::to_vec(&metadata)?;
        header.pop();
        writer.write_all(&header)?;
        writer.write_all(b",\"cases\":[")?;
        let mut first = true;
        records(&path, |record| {
            if record["type"] == "case" {
                if !first {
                    writer.write_all(b",")?;
                }
                first = false;
                serde_json::to_writer(&mut *writer, &record["result"])?;
            }
            Ok(())
        })?;
        writer.write_all(b"]}")?;
        Ok(())
    })?;
    atomic(&dest.junit, |writer| {
        if let Some(xml) = &xml {
            writer.write_all(xml.as_bytes())?;
            return Ok(());
        }
        writer.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>\n")?;
        records(&path, |record| {
            if record["type"] == "case" {
                writer.write_all(
                    record["junit"]
                        .as_str()
                        .context("case JUnit missing")?
                        .as_bytes(),
                )?;
            }
            Ok(())
        })?;
        if metadata["error"].is_string() {
            writer.write_all(lifecycle_xml(metadata["error"].as_str().unwrap()).as_bytes())?;
        }
        writer.write_all(b"</testsuites>\n")?;
        Ok(())
    })?;
    if let Some(html) = &dest.html {
        atomic(html, |writer| {
            writer.write_all(b"<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><title>Reqon run report</title><style>body{font:16px system-ui;background:#10151e;color:#e5edf8;max-width:1100px;margin:36px auto;padding:0 20px}details{background:#1c2534;margin:12px 0;padding:16px;border-radius:10px}pre{white-space:pre-wrap;overflow-wrap:anywhere}summary{cursor:pointer}small{color:#adc0d9}</style><h1>Reqon run report</h1>")?;
            write!(
                writer,
                "<p>Run <code>{}</code> · <strong>{}</strong></p><small>Large payloads may be omitted. Complete verdicts are preserved.</small>",
                escape(&run_id),
                escape(metadata["outcome"].as_str().unwrap_or("error"))
            )?;
            if package {
                records(&path, |record| {
                    if record["type"] == "case" {
                        write!(
                            writer,
                            "<details><summary>{} · {}</summary><pre>{}</pre></details>",
                            escape(record["result"]["instance_id"].as_str().unwrap_or("case")),
                            escape(record["result"]["outcome"].as_str().unwrap_or("error")),
                            escape(&serde_json::to_string_pretty(&record["result"])?)
                        )?;
                    }
                    Ok(())
                })?;
            } else {
                write!(
                    writer,
                    "<pre>{}</pre>",
                    escape(&serde_json::to_string_pretty(&metadata)?)
                )?;
            }
            writer.write_all(b"</html>")?;
            Ok(())
        })?;
    }
    // The manifest is the commit marker for this set of independently atomic files.
    atomic_json(
        &dest.manifest,
        &json!({"schema_version":"reqon-artifacts/v1","run_id":run_id,"state":"complete","outcome":metadata["outcome"],"exit_code":exit_code,"json":dest.json,"junit":dest.junit,"html":dest.html,"journal":path}),
    )?;
    journal.lock().unwrap().record(
        &json!({"type":"journal_finished","exit_code":exit_code,"outcome":metadata["outcome"]}),
    )?;
    journal.lock().unwrap().sync()?;
    Ok(())
}

pub fn recover(path: &Path, dest: &Destinations) -> Result<()> {
    let mut run_id = None;
    let mut count = 0;
    records(path, |record| {
        if record["type"] == "journal_started" {
            if record["schema_version"] != "reqon-journal/v1" {
                bail!("unsupported journal");
            }
            run_id = record["run_id"].as_str().map(str::to_owned);
        }
        if record["type"] == "step" {
            count += 1;
        }
        Ok(())
    })?;
    let run_id = run_id.context("journal header missing")?;
    atomic(&dest.json, |writer| {
        write!(
            writer,
            "{{\"schema_version\":\"reqon-recovered-result/v1\",\"run_id\":{},\"outcome\":\"error\",\"error\":\"run interrupted; recovered records are partial\",\"completed_steps\":{},\"records\":[",
            serde_json::to_string(&run_id)?,
            count
        )?;
        let mut first = true;
        records(path, |record| {
            if matches!(record["type"].as_str(), Some("step" | "case")) {
                if !first {
                    writer.write_all(b",")?;
                }
                first = false;
                serde_json::to_writer(&mut *writer, &record)?;
            }
            Ok(())
        })?;
        writer.write_all(b"]}")?;
        Ok(())
    })?;
    atomic(&dest.junit, |writer| {
        writer.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\"?><testsuites>")?;
        records(path, |record| {
            if record["type"] == "case" {
                writer.write_all(record["junit"].as_str().unwrap_or("").as_bytes())?;
            }
            Ok(())
        })?;
        writer.write_all(
            lifecycle_xml("run interrupted; recovered records are partial").as_bytes(),
        )?;
        writer.write_all(b"</testsuites>")?;
        Ok(())
    })?;
    if let Some(html) = &dest.html {
        atomic(html, |w| {
            write!(
                w,
                "<!doctype html><meta charset=\"utf-8\"><title>Interrupted Reqon run</title><h1>Run interrupted</h1><p>{} completed step records recovered. See JSON/JUnit for details.</p>",
                count
            )?;
            Ok(())
        })?;
    }
    atomic_json(
        &dest.manifest,
        &json!({"schema_version":"reqon-artifacts/v1","run_id":run_id,"state":"recovered","outcome":"error","exit_code":2,"json":dest.json,"junit":dest.junit,"html":dest.html}),
    )?;
    Ok(())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn lifecycle_xml(message: &str) -> String {
    format!(
        "<testsuite name=\"runner-lifecycle\" tests=\"1\" errors=\"1\"><testcase name=\"runner-lifecycle\"><error type=\"Interrupted\" message=\"{}\"/></testcase></testsuite>",
        escape(message)
    )
}
