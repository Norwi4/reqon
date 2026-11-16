//! Standalone HTML generated only from the runner's already-redacted public JSON.
use serde_json::Value;
use std::fmt::Write;

pub(crate) fn escape(value: &str) -> String {
    value
        .chars()
        .filter(|c| *c == '\t' || *c == '\n' || *c == '\r' || *c >= ' ')
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}
fn text(value: &Value) -> String {
    escape(value.as_str().unwrap_or_default())
}
pub(crate) fn label(row: &Value) -> String {
    let folder = row["folder"].as_str().unwrap_or_default();
    let name = row["name"].as_str().unwrap_or_default();
    let path = if folder.is_empty() {
        name.to_owned()
    } else {
        format!("{folder}/{name}")
    };
    let data = row["data_row"]
        .as_u64()
        .map(|i| format!(", data row {i}"))
        .unwrap_or_default();
    format!("{path} [iteration {}{data}]", row["iteration"])
}

pub(crate) fn html(report: &Value) -> String {
    let rows = report["results"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let failed = rows
        .iter()
        .filter(|r| r["passed"] != true && r["response"]["cancelled"] != true)
        .count();
    let cancelled = rows
        .iter()
        .filter(|r| r["response"]["cancelled"] == true)
        .count();
    let skipped = rows
        .iter()
        .filter(|r| r["passed"] == true && r["response"]["reason"] == "Skipped")
        .count();
    let passed = rows.len() - failed - cancelled - skipped;
    let error = report["error"].as_str().unwrap_or_default();
    let status = if report["cancelled"] == true {
        "Cancelled"
    } else if failed > 0 || !error.is_empty() {
        "Failed"
    } else {
        "Passed"
    };
    let mut out = format!(
        r##"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Reqon run — {status}</title><style>
:root{{color-scheme:light dark;font-family:Segoe UI,system-ui,sans-serif;background:#101722;color:#e6edf5}}body{{max-width:1160px;margin:40px auto;padding:0 24px}}h1{{font-size:32px;margin:8px 0}}.muted{{color:#a1b3ca}}.stats{{display:flex;flex-wrap:wrap;gap:12px;margin:24px 0}}.stat{{background:#1a2739;padding:18px 24px;border-radius:12px;min-width:100px}}.stat b{{display:block;font-size:28px}}.table{{overflow:auto}}table{{border-collapse:collapse;width:100%;font-size:14px}}th,td{{text-align:left;border-bottom:1px solid #34465e;padding:12px;vertical-align:top}}a{{color:#8fc9ff}}.pass{{color:#8ce4ba}}.fail{{color:#ffa6a6}}.skip{{color:#efcb83}}details{{border:1px solid #34465e;border-radius:10px;margin:12px 0;padding:16px}}summary{{cursor:pointer;overflow-wrap:anywhere}}pre{{white-space:pre-wrap;overflow-wrap:anywhere;background:#172131;padding:16px;border-radius:8px;font-size:13px;max-height:420px;overflow:auto}}.error{{border-left:4px solid #ffa6a6;padding:12px;background:#302332}}small{{font-size:12px}}footer{{margin:32px 0}}@media print{{:root{{color-scheme:light;background:white;color:black}}.stat,pre{{background:#f2f4f8}}.muted{{color:#444}}}}
</style></head><body><div class="muted">REQON · COLLECTION RUN</div><h1>{status}</h1>
<p class="muted">Results in collection order. Iterations and data rows are numbered from 1.</p>
<div class="stats"><div class="stat"><b>{}</b>Requests</div><div class="stat pass"><b>{passed}</b>Passed</div><div class="stat fail"><b>{failed}</b>Failed</div><div class="stat skip"><b>{skipped}</b>Skipped</div><div class="stat skip"><b>{cancelled}</b>Cancelled</div></div>"##,
        rows.len()
    );
    if !error.is_empty() {
        let _ = write!(out, "<p class=\"error\">{}</p>", escape(error));
    }
    out.push_str("<div class=\"table\"><table><thead><tr><th>Request</th><th>Result</th><th>Status</th><th>Time</th><th>Tests</th></tr></thead><tbody>");
    for (i, row) in rows.iter().enumerate() {
        let response = &row["response"];
        let (class, status) = if response["cancelled"] == true {
            ("skip", "Cancelled")
        } else if row["passed"] != true {
            ("fail", "Failed")
        } else if response["reason"] == "Skipped" {
            ("skip", "Skipped")
        } else {
            ("pass", "Passed")
        };
        let tests = response["tests"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        let _ = write!(
            out,
            "<tr><td><a href=\"#request-{i}\">{}</a></td><td class=\"{class}\">{status}</td><td>{}</td><td>{} ms</td><td>{}/{}</td></tr>",
            escape(&label(row)),
            response["status"].as_u64().unwrap_or(0),
            response["elapsed_ms"].as_u64().unwrap_or(0),
            tests.iter().filter(|t| t["passed"] == true).count(),
            tests.len()
        );
    }
    out.push_str("</tbody></table></div><h2>Request details</h2>");
    for (i, row) in rows.iter().enumerate() {
        let r = &row["response"];
        let _ = write!(
            out,
            "<details id=\"request-{i}\"><summary>{}</summary><p>{}</p>",
            escape(&label(row)),
            text(&r["url"])
        );
        if let Some(error) = r["error"].as_str().filter(|s| !s.is_empty()) {
            let _ = write!(out, "<p class=\"error\">{}</p>", escape(error));
        }
        for (title, key) in [
            ("Tests", "tests"),
            ("Headers", "headers"),
            ("Console", "logs"),
        ] {
            let value = serde_json::to_string_pretty(&r[key]).unwrap_or_default();
            let _ = write!(out, "<h3>{title}</h3><pre>{}</pre>", escape(&value));
        }
        let _ = write!(
            out,
            "<h3>Response body</h3><pre>{}</pre></details>",
            text(&r["body"])
        );
    }
    out.push_str("<footer class=\"muted\"><small>Generated by Reqon. Marked secrets are redacted. This report works offline.</small></footer></body></html>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn report_escapes_untrusted_content_and_names_data_rows() {
        let report = serde_json::json!({"cancelled":true,"error":"<script>alert(1)</script>","results":[{"name":"<img src=x>","folder":"A&B","iteration":2,"data_row":2,"passed":false,"response":{"status":0,"cancelled":true,"body":"</pre><script>bad()</script>","tests":[]}}]});
        let html = html(&report);
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("A&amp;B/&lt;img src=x&gt; [iteration 2, data row 2]"));
        assert!(html.contains("default-src 'none'"));
    }
}
