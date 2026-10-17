//! Opt-in pipeline diagnostics shared by the pipeline and its engine adapters.
//!
//! `ONEASR_PIPELINE_TRACE=1` keeps the old behaviour (lines on stderr). Every
//! trace line is also kept in a small in-process ring so the GUI worker can
//! drain it into the per-task journal: a release GUI has no console, and a task
//! that dies on the GPU must still leave "which chunk / how long / which plan"
//! behind. The ring is bounded; nobody is expected to read it in a tight loop.

use std::sync::Mutex;

/// Cap on retained lines. A 14-chunk task produces a few dozen; the cap only
/// matters for a CLI run that never drains.
const MAX_TRACE_LINES: usize = 512;

static TRACE: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Truthy reading of an opt-in flag **value** — the **one** rule.
///
/// `1` / `true` / `TRUE` / `yes` are on; unset, empty and anything else are off.
/// `ONEASR_PIPELINE_TRACE=0` must mean *off*: a second "is it set at all?"
/// reading somewhere else would make the same variable answer two ways.
fn flag_is_on(value: Option<&str>) -> bool {
    matches!(value, Some("1") | Some("true") | Some("TRUE") | Some("yes"))
}

/// Read one opt-in flag from the process environment (see [`flag_is_on`]).
pub(crate) fn env_flag(name: &str) -> bool {
    flag_is_on(std::env::var(name).ok().as_deref())
}

/// `ONEASR_PIPELINE_TRACE=1` turns on per-chunk / per-engine trace lines.
pub(crate) fn pipeline_trace() -> bool {
    env_flag("ONEASR_PIPELINE_TRACE")
}

/// Append one line to the bounded ring, dropping the oldest when full.
///
/// Pure so the bound is testable without the process-global ring.
fn push_bounded(lines: &mut Vec<String>, line: String) {
    if lines.len() >= MAX_TRACE_LINES {
        lines.remove(0);
    }
    lines.push(line);
}

/// Record a pipeline / engine diagnostic line.
///
/// Always retained for the task journal; printed only under
/// `ONEASR_PIPELINE_TRACE=1`. `pipeline_trace()` is intentionally *not* part
/// of the retention decision: the journal is exactly where these lines must
/// show up when a release build fails.
pub(crate) fn trace_log(msg: impl AsRef<str>) {
    let msg = msg.as_ref();
    if pipeline_trace() {
        eprintln!("[pipeline] {msg}");
    }
    if let Ok(mut lines) = TRACE.lock() {
        push_bounded(&mut lines, msg.to_string());
    }
}

/// Drain the lines recorded since the last call.
///
/// The GUI worker calls this at every stage/chunk update and at task end, so
/// the journal carries the pipeline's own view (chunk plan, per-chunk spans,
/// model load timings) even though the release GUI has no stderr.
pub fn take_trace() -> Vec<String> {
    match TRACE.lock() {
        Ok(mut lines) => std::mem::take(&mut *lines),
        Err(error) => std::mem::take(&mut *error.into_inner()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule itself is pure, so it is testable without touching the process
    /// environment (`set_var` is unsafe in edition 2024 and races with the other
    /// tests' `temp_dir()` reads).
    #[test]
    fn flag_values_match_the_documented_rule() {
        for on in ["1", "true", "TRUE", "yes"] {
            assert!(flag_is_on(Some(on)), "{on} should be on");
        }
        for off in ["0", "false", "no", "FALSE", "", "on", "Yes"] {
            assert!(!flag_is_on(Some(off)), "{off} should be off");
        }
        assert!(!flag_is_on(None), "unset should be off");
    }

    /// Full ring: oldest goes first, newest stays, length never exceeds the cap.
    #[test]
    fn trace_ring_keeps_the_latest_lines() {
        let mut lines = Vec::new();
        for i in 0..(MAX_TRACE_LINES + 3) {
            push_bounded(&mut lines, format!("line {i}"));
        }
        assert_eq!(lines.len(), MAX_TRACE_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 3"));
        let last = format!("line {}", MAX_TRACE_LINES + 2);
        assert_eq!(lines.last().map(String::as_str), Some(last.as_str()));
    }

    /// One line under the cap is appended, not dropped.
    #[test]
    fn trace_ring_keeps_short_runs() {
        let mut lines = Vec::new();
        push_bounded(&mut lines, "only".into());
        assert_eq!(lines, ["only"]);
    }
}
