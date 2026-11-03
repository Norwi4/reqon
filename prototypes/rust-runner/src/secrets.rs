//! Output protection is separate from execution values. Never serialize secrets.
use crate::{Outcome, RunReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const REDACTED: &str = "[REDACTED]";

#[derive(Clone, Default)]
pub struct OutputPolicy {
    values: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RedactionInfo {
    pub mode: String,
    pub dynamic_values_withheld: bool,
}

impl OutputPolicy {
    pub fn new(mut values: Vec<String>) -> Self {
        values.retain(|s| !s.is_empty());
        values.sort_by_key(|s| std::cmp::Reverse(s.len()));
        values.dedup();
        Self { values }
    }

    pub fn sensitive(&self) -> bool {
        !self.values.is_empty()
    }

    pub fn info(&self) -> Option<RedactionInfo> {
        self.sensitive().then_some(RedactionInfo {
            mode: "secret-context/v1".into(),
            dynamic_values_withheld: true,
        })
    }

    pub fn text(&self, input: &str) -> String {
        // Scan original text once. Do not reprocess replacement markers or
        // overlook overlapping matches after a previous replacement.
        let mut out = String::new();
        let mut rest = input;
        while !rest.is_empty() {
            if let Some(secret) = self.values.iter().find(|s| rest.starts_with(s.as_str())) {
                out.push_str(REDACTED);
                rest = &rest[secret.len()..];
            } else {
                let ch = rest.chars().next().unwrap();
                out.push(ch);
                rest = &rest[ch.len_utf8()..];
            }
        }
        out
    }

    /// For public, authored metadata, not arbitrary runtime data.
    pub fn value(&self, value: &mut Value) {
        match value {
            Value::String(s) => *s = self.text(s),
            Value::Array(items) => items.iter_mut().for_each(|v| self.value(v)),
            Value::Object(items) => {
                *items = std::mem::take(items)
                    .into_iter()
                    .map(|(k, mut v)| {
                        self.value(&mut v);
                        (self.text(&k), v)
                    })
                    .collect();
            }
            scalar => {
                let representation = scalar.to_string();
                if self.values.contains(&representation) {
                    *scalar = Value::String(REDACTED.into());
                }
            }
        }
    }

    pub fn event(&self, mut event: Value) -> Value {
        // Event envelopes contain fixed machine fields and authored IDs only.
        // Never redact protocol constants (a short secret could equal "passed").
        for key in [
            "package_id",
            "case_id",
            "instance_id",
            "scenario_id",
            "step_id",
        ] {
            if let Some(Value::String(s)) = event.get_mut(key) {
                *s = self.text(s);
            }
        }
        if self.sensitive() {
            event["secret_context"] = Value::Bool(true);
        }
        event
    }

    pub fn report(&self, report: &mut RunReport) {
        if !self.sensitive() {
            return;
        }
        report.redaction = self.info();
        report.scenario_id = self.text(&report.scenario_id);
        report.variables.clear();
        for step in &mut report.steps {
            self.step(step);
        }
    }

    pub fn step(&self, step: &mut crate::StepReport) {
        if self.sensitive() {
            step.id = self.text(&step.id);
            step.response = None;
            step.request = None;
            step.logs.clear();
            for check in &mut step.script_checks {
                check.name = REDACTED.into();
                check.message = check.message.as_ref().map(|_| REDACTED.into());
            }
            if let Some(transport) = &mut step.transport {
                transport.headers.clear();
                transport.trailers.clear();
                transport.url = None;
            }
            for check in &mut step.checks {
                check.path = self.text(&check.path);
                check.expected = Value::String(REDACTED.into());
                check.actual = check
                    .actual
                    .as_ref()
                    .map(|_| Value::String(REDACTED.into()));
            }
            if step.error.is_some() {
                step.error = Some(
                    match step.outcome {
                        Outcome::Failed => "check failed; details withheld in secret mode",
                        Outcome::Cancelled => "run cancelled",
                        _ => "execution error; details withheld in secret mode",
                    }
                    .into(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn masking_handles_unicode_overlaps_and_empty_inputs() {
        let policy = OutputPolicy::new(vec![
            "".into(),
            "пароль😀".into(),
            "abc".into(),
            "abcdef".into(),
        ]);
        assert_eq!(
            policy.text("xпароль😀abcdefabc"),
            "x[REDACTED][REDACTED][REDACTED]"
        );
        assert_eq!(policy.text(""), "");
    }

    #[test]
    fn short_secrets_do_not_corrupt_protocol_constants() {
        let policy = OutputPolicy::new(vec!["passed".into()]);
        let event =
            policy.event(json!({"type":"step_finished","outcome":"passed","step_id":"passed"}));
        assert_eq!(event["outcome"], "passed");
        assert_eq!(event["step_id"], REDACTED);
    }

    #[test]
    fn authored_metadata_keys_and_numeric_secret_copies_are_masked() {
        let policy = OutputPolicy::new(vec!["42".into()]);
        let mut metadata = json!({"42": {"copy": 42}});
        policy.value(&mut metadata);
        assert_eq!(metadata, json!({"[REDACTED]": {"copy": "[REDACTED]"}}));
    }
}
