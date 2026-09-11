//! Opt-in pipeline diagnostics shared by the pipeline and its engine adapters.

/// `ONEASR_PIPELINE_TRACE=1` turns on per-chunk / per-engine trace lines.
pub(crate) fn pipeline_trace() -> bool {
    matches!(
        std::env::var("ONEASR_PIPELINE_TRACE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

pub(crate) fn trace_log(msg: impl AsRef<str>) {
    if pipeline_trace() {
        eprintln!("[pipeline] {}", msg.as_ref());
    }
}
