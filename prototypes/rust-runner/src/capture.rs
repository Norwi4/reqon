//! Output limits never alter values used by requests, scripts or assertions.
use serde::Serialize;
use serde_json::{Value, json};
use std::io::Write;

struct Budget(usize);
impl Write for Budget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.0 {
            return Err(std::io::Error::other("capture limit"));
        }
        self.0 -= bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub fn fits(value: &impl Serialize, limit: usize) -> bool {
    size_within(value, limit).is_some()
}
pub fn size_within(value: &impl Serialize, limit: usize) -> Option<usize> {
    let mut budget = Budget(limit);
    serde_json::to_writer(&mut budget, value)
        .ok()
        .map(|_| limit - budget.0)
}
pub fn value(value: &Value, limit: usize) -> Value {
    if fits(value, limit) {
        value.clone()
    } else {
        json!({"$reqon_capture":"omitted","reason":"size_limit","limit_bytes":limit})
    }
}
pub fn step(step: &mut crate::StepReport, remaining: &mut usize) {
    step.check_summary = Some(crate::CheckSummary {
        passed: step.checks.iter().filter(|c| c.passed).count()
            + step.script_checks.iter().filter(|c| c.passed).count(),
        failed: step.checks.iter().filter(|c| !c.passed).count()
            + step.script_checks.iter().filter(|c| !c.passed).count(),
    });
    let size = serde_json::to_vec(step).map_or(usize::MAX, |b| b.len());
    if size > *remaining {
        step.response = None;
        step.checks.clear();
        step.script_checks.clear();
        step.logs.clear();
        if let Some(t) = &mut step.transport {
            t.headers.clear();
            t.trailers.clear();
            t.url = None;
        }
        step.capture_omitted = true;
    } else {
        *remaining -= size;
    }
}
