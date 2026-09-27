//! Opt-in pipeline diagnostics shared by the pipeline and its engine adapters.

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

pub(crate) fn trace_log(msg: impl AsRef<str>) {
    if pipeline_trace() {
        eprintln!("[pipeline] {}", msg.as_ref());
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
}
