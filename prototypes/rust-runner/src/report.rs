use crate::{Outcome, RunReport};

fn xml(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            matches!(*c, '\t' | '\n' | '\r')
                || (*c >= '\u{20}' && *c != '\u{fffe}' && *c != '\u{ffff}')
        })
        .collect::<String>()
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn junit(report: &RunReport) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<testsuites>{}</testsuites>\n",
        junit_suite(report, &report.scenario_id)
    )
}

pub(crate) fn junit_suite(report: &RunReport, classname: &str) -> String {
    let count = |outcome| report.steps.iter().filter(|s| s.outcome == outcome).count();
    let mut output = format!(
        "<testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"{}\" time=\"{:.6}\">\n",
        xml(&report.scenario_id),
        report.steps.len(),
        count(Outcome::Failed),
        count(Outcome::Error) + count(Outcome::Cancelled),
        count(Outcome::Skipped),
        report.duration_ms / 1000.0
    );
    for step in &report.steps {
        output.push_str(&format!(
            "<testcase classname=\"{}\" name=\"{}\" time=\"{:.6}\">",
            xml(classname),
            xml(&step.id),
            step.duration_ms / 1000.0
        ));
        let message = xml(step.error.as_deref().unwrap_or(""));
        match step.outcome {
            Outcome::Failed => output.push_str(&format!(
                "<failure message=\"{message}\">{}</failure>",
                xml(&serde_json::to_string(&step.checks).unwrap_or_default())
            )),
            Outcome::Error | Outcome::Cancelled => output.push_str(&format!(
                "<error type=\"{:?}\" message=\"{message}\"/>",
                step.outcome
            )),
            Outcome::Skipped => output.push_str(&format!("<skipped message=\"{message}\"/>")),
            Outcome::Passed => {}
        }
        output.push_str("</testcase>\n");
    }
    output.push_str("</testsuite>\n");
    output
}
