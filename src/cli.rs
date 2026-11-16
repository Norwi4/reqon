//! Console entry point. It loads the requested collection without opening a
//! desktop profile or creating a window.
use crate::{
    collection_runner::{self, Options, Plan, Session},
    engine::Engine,
    model::Pair,
    storage,
};
use anyhow::{Context, Result, bail, ensure};
use std::path::PathBuf;

const SECRET_HELP: &str = "\nSecret migration (Windows):\n  reqon-cli secrets migrate <collection-folder>          Preview only\n  reqon-cli secrets migrate <collection-folder> --check  Exit 1 if protection is needed\n  reqon-cli secrets migrate <collection-folder> --apply  Protect files with encrypted backup\n  reqon-cli secrets backups <collection-folder>          List recovery backups\n  reqon-cli secrets restore <collection-folder> <backup-id>\n  Add --environments to migrate/check marked Secret environment variables too.\n\nMigration prints a JSON summary without secret values. Restore puts the original\nfiles back, including original plaintext secrets; edited files are never overwritten.\n";

fn secrets_command(mut args: impl Iterator<Item = String>) -> Result<i32> {
    let operation = args
        .next()
        .context("Choose secrets migrate, backups or restore")?;
    if matches!(operation.as_str(), "--help" | "-h") {
        print!("{SECRET_HELP}");
        return Ok(0);
    }
    let root = PathBuf::from(args.next().context("Choose a collection folder")?);
    if operation == "backups" {
        ensure!(args.next().is_none(), "Unexpected backups arguments");
        println!(
            "{}",
            serde_json::to_string_pretty(&crate::auth_migration::backups(&root)?)?
        );
        return Ok(0);
    }
    let (report, code) = match operation.as_str() {
        "migrate" => {
            let mut apply = false;
            let mut check = false;
            let mut environments = false;
            for option in args {
                match option.as_str() {
                    "--apply" => apply = true,
                    "--check" => check = true,
                    "--environments" => environments = true,
                    _ => bail!("Unknown migration option; use secrets --help"),
                }
            }
            ensure!(!(apply && check), "Choose either --check or --apply");
            let plan = if environments {
                crate::auth_migration::scan_with_environments(&root, true)?
            } else {
                crate::auth_migration::scan(&root)?
            };
            let code = i32::from(check && plan.report.files_to_change > 0);
            (
                if apply {
                    crate::auth_migration::apply(plan)?
                } else {
                    plan.report
                },
                code,
            )
        }
        "restore" => {
            let id = args.next().context("Choose a recovery backup ID")?;
            ensure!(args.next().is_none(), "Unexpected restore arguments");
            (crate::auth_migration::restore(&root, &id)?, 0)
        }
        _ => bail!("Choose secrets migrate, backups or restore"),
    };
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(code)
}

const HELP: &str = "Reqon collection runner\n\nUsage: reqon-cli run <collection-folder> [options]\n\n  --env NAME               Collection environment\n  --iterations N           Repeat the collection (default 1)\n  --data PATH              CSV/JSON: one iteration per row, replaces --iterations\n  --parallel N             Independent requests per iteration, 1..64 (default 1)\n  --folder PATH            Select a folder subtree (repeatable, relative path)\n  --tag NAME               Select a request tag (repeatable)\n  --delay MS               Minimum interval between request starts\n  --max-steps N            Bound total requests/script loops (default 10000)\n  --stop-on-error          Stop scheduling after transport or test failure\n  --var NAME=VALUE         Override an environment variable\n  --secret NAME=ENV_NAME   Read a secret from a process environment variable\n  --report-json PATH       Write a redacted JSON report\n  --report-junit PATH      Write a JUnit report\n  --report-html PATH       Write a standalone HTML report\n  --json                   Print the redacted report to stdout\n\nParallel requests use isolated variable snapshots; use --parallel 1 for variable\nchaining and setNextRequest. Already started requests finish on stop-on-error.\nRepeated folders/tags are ORed within each group; the two groups are ANDed.\nExit codes: 0 passed, 1 failed checks/request, 2 invalid arguments or collection, 130 cancelled.\nCtrl+C (or Ctrl+Break on Windows) cancels active requests and writes partial reports.\nHTTP status checks are explicit script/assertion checks, as in the desktop runner.\n";

#[derive(Default)]
struct Args {
    path: PathBuf,
    environment: Option<String>,
    options: Options,
    variables: Vec<Pair>,
    json: bool,
    json_path: Option<PathBuf>,
    junit_path: Option<PathBuf>,
    html_path: Option<PathBuf>,
    data_path: Option<PathBuf>,
    folders: Vec<String>,
    tags: Vec<String>,
}
fn parse(args: impl Iterator<Item = String>) -> Result<Args> {
    let mut args = args;
    ensure!(
        args.next().as_deref() == Some("run"),
        "Expected 'run'; use --help"
    );
    let mut parsed = Args {
        path: args.next().context("Choose a collection folder")?.into(),
        ..Default::default()
    };
    while let Some(option) = args.next() {
        if option == "--json" {
            parsed.json = true;
            continue;
        }
        if option == "--stop-on-error" {
            parsed.options.stop_on_error = true;
            continue;
        }
        let value = args
            .next()
            .with_context(|| format!("Missing value for {option}"))?;
        match option.as_str() {
            "--env" => parsed.environment = Some(value),
            "--iterations" => {
                parsed.options.iterations = value.parse().context("Invalid iterations")?
            }
            "--delay" => parsed.options.delay_ms = value.parse().context("Invalid delay")?,
            "--max-steps" => {
                parsed.options.max_steps = value.parse().context("Invalid step limit")?
            }
            "--report-json" => parsed.json_path = Some(value.into()),
            "--report-junit" => parsed.junit_path = Some(value.into()),
            "--report-html" => parsed.html_path = Some(value.into()),
            "--data" => parsed.data_path = Some(value.into()),
            "--folder" => parsed.folders.push(value),
            "--tag" => parsed.tags.push(value),
            "--parallel" => {
                parsed.options.concurrency = value.parse().context("Invalid parallelism")?
            }
            "--var" | "--secret" => {
                let (name, value) = value
                    .split_once('=')
                    .context("Use NAME=VALUE or NAME=ENV_NAME")?;
                ensure!(!name.is_empty(), "Variable name is empty");
                let mut row = Pair::new(name, value);
                if option == "--secret" {
                    row.value = std::env::var(value).with_context(|| {
                        format!("Secret environment variable for '{name}' is unavailable")
                    })?;
                    row.secret = true;
                }
                parsed.variables.push(row);
            }
            _ => bail!("Unknown option {option}; use --help"),
        }
    }
    ensure!(
        parsed.options.delay_ms <= 3_600_000,
        "Delay must not exceed one hour"
    );
    Ok(parsed)
}

fn escape(value: &str) -> String {
    value
        .chars()
        .filter(|c| matches!(*c, '\t' | '\n' | '\r' | '\u{20}'..='\u{d7ff}' | '\u{e000}'..='\u{fffd}' | '\u{10000}'..='\u{10ffff}'))
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn junit(report: &serde_json::Value) -> String {
    let rows = report["results"].as_array().cloned().unwrap_or_default();
    let skipped = rows
        .iter()
        .filter(|r| {
            r["response"]["cancelled"] == true
                || (r["passed"] == true && r["response"]["reason"] == "Skipped")
        })
        .count();
    let failed = rows
        .iter()
        .filter(|r| r["passed"] != true && r["response"]["cancelled"] != true)
        .count();
    let error = report["error"].as_str().unwrap_or_default();
    let mut xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuite name=\"Reqon\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\">\n",
        rows.len() + usize::from(!error.is_empty()),
        failed,
        usize::from(!error.is_empty()),
        skipped
    );
    for row in rows {
        let response = &row["response"];
        xml.push_str(&format!(
            "  <testcase name=\"{}\" time=\"{:.6}\">",
            escape(&crate::runner_report::label(&row)),
            response["elapsed_ms"].as_f64().unwrap_or(0.) / 1000.
        ));
        if response["cancelled"] == true {
            xml.push_str("<skipped message=\"Request cancelled\"/>");
        } else if row["passed"] == true && response["reason"] == "Skipped" {
            xml.push_str("<skipped message=\"Skipped by script\"/>");
        } else if row["passed"] != true {
            let mut errors = vec![response["error"].as_str().unwrap_or_default().to_owned()];
            if let Some(tests) = response["tests"].as_array() {
                errors.extend(tests.iter().filter(|t| t["passed"] != true).map(|t| {
                    format!(
                        "{}: {}",
                        t["name"].as_str().unwrap_or_default(),
                        t["error"].as_str().unwrap_or_default()
                    )
                }));
            }
            xml.push_str(&format!(
                "<failure>{}</failure>",
                escape(&errors.join("\n"))
            ));
        }
        xml.push_str("</testcase>\n");
    }
    if !error.is_empty() {
        xml.push_str(&format!(
            "  <testcase name=\"Execution\"><error>{}</error></testcase>\n",
            escape(error)
        ));
    }
    xml.push_str("</testsuite>\n");
    xml
}

async fn execute(args: Args) -> Result<i32> {
    ensure!(
        crate::native_format::is_collection(&args.path),
        "Select a folder containing reqon.json, bruno.json, or opencollection.yml"
    );
    let (collection, requests, warnings) = storage::read_collection(&args.path)?;
    ensure!(
        warnings.is_empty(),
        "Unreadable collection files: {}",
        warnings.join("; ")
    );
    let requests = collection_runner::select_requests(requests, &args.folders, &args.tags)?;
    let data = args
        .data_path
        .as_deref()
        .map(crate::runner_data::Dataset::read)
        .transpose()?;
    let mut environment = if let Some(name) = &args.environment {
        collection
            .environments
            .iter()
            .find(|e| &e.name == name)
            .with_context(|| format!("Environment '{name}' was not found"))?
            .vars
            .clone()
    } else {
        vec![]
    };
    for mut row in args.variables {
        row.secret |= environment.iter().any(|p| p.key == row.key && p.secret);
        environment.retain(|p| p.key != row.key);
        environment.push(row);
    }
    let mut secrets: Vec<_> = requests
        .iter()
        .flat_map(crate::auth_vault::request_values)
        .collect();
    secrets.extend(
        environment
            .iter()
            .filter(|p| p.secret)
            .map(|p| p.value.clone()),
    );
    let mut plan = Plan::new(requests, args.options)?;
    if let Some(data) = data {
        plan = plan.with_data(data);
    }
    let mut session = Session::new(Engine::default(), vec![], environment);
    let token = crate::cancellation::CancellationToken::new();
    // Register before starting the first request, so an early Ctrl+C is not lost.
    #[cfg(windows)]
    let signal = {
        let mut interrupt = tokio::signal::windows::ctrl_c()?;
        let mut console_break = tokio::signal::windows::ctrl_break()?;
        async move {
            tokio::select! { _ = interrupt.recv() => {}, _ = console_break.recv() => {} }
        }
    };
    #[cfg(not(windows))]
    let signal = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let execution = collection_runner::run_cancellable(&mut plan, &mut session, &token);
    tokio::pin!(execution);
    let report = tokio::select! {
        biased;
        _ = signal => { token.cancel(); execution.await }
        report = &mut execution => report,
    };
    let public = report.public_json(&secrets)?;
    if let Some(path) = args.json_path {
        storage::atomic_write(&path, serde_json::to_vec_pretty(&public)?)?;
    }
    if let Some(path) = args.junit_path {
        storage::atomic_write(&path, junit(&public))?;
    }
    if let Some(path) = args.html_path {
        storage::atomic_write(&path, crate::runner_report::html(&public))?;
    }
    if args.json {
        println!("{}", serde_json::to_string_pretty(&public)?);
    } else {
        let passed = report.results.iter().filter(|r| r.passed).count();
        println!(
            "{} requests: {} passed, {} failed",
            report.results.len(),
            passed,
            report.results.len() - passed
        );
        if let Some(error) = public["error"].as_str().filter(|s| !s.is_empty()) {
            eprintln!("{error}");
        }
    }
    Ok(if report.cancelled {
        130
    } else if report.success() {
        0
    } else {
        1
    })
}

pub fn main() -> i32 {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h") {
        print!("{HELP}{SECRET_HELP}");
        return 0;
    }
    if matches!(args[0].as_str(), "--version" | "-V") {
        println!("reqon-cli {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    let result = (|| -> Result<i32> {
        if args[0] == "secrets" {
            return secrets_command(args.into_iter().skip(1));
        }
        let args = parse(args.into_iter())?;
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?
            .block_on(Box::pin(execute(args)))
    })();
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Reqon: {error:#}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn junit_escapes_failures_and_includes_run_level_errors() {
        let report = serde_json::json!({"error":"loop <limit>","results":[{"name":"A&B","iteration":1,"passed":false,"response":{"elapsed_ms":25,"error":"bad <request>","tests":[]}}]});
        let xml = junit(&report);
        assert!(xml.contains("tests=\"2\" failures=\"1\" errors=\"1\""));
        assert!(xml.contains("A&amp;B"));
        assert!(xml.contains("time=\"0.025000\""));
        assert!(!xml.contains("bad <request>"));
    }
}
