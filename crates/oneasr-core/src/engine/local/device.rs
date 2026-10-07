//! GPU 设备选择：`ONEASR_DEVICE` 环境变量 → 各 wgpu 引擎的 `DeviceSelector`。
//!
//! 未设置、空白或 `auto` 时不钉卡。引擎自己先试独显，再试集显，虚拟 GPU 排在
//! 后面；CPU 适配器（例如 Microsoft Basic Render Driver）不算显卡。
//! 写了具体 spec（`vulkan:0`、`nvidia`、`#1`）就只开那一张，失败不改去另一张。
//!
//! spec 语法：`<runtime>[:<index>]`（vulkan/dx12/…）| `#<n>` / `<n>` | 适配器名子串
//! （大小写不敏感，如 `nvidia`）。`gpu` 和 `cpu` 是后端档，不是卡名。
//!
//! 显式指定的 spec 解析失败是用户输入错误：**报错退出，不静默改走别的卡**——
//! 否则拼错卡名会落在集显上。

use crate::diagnostics::trace_log;
use crate::engine::EngineError;

/// 规整原始环境变量值：`None` / 空白 / `auto`（不分大小写）→ `None`（保持引擎默认），
/// 其余去掉首尾空白原样返回（大小写交给引擎侧 `parse` / 名称匹配处理）。
fn normalize_device_spec(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("auto") {
        return None;
    }
    Some(s.to_string())
}

/// 读 `ONEASR_DEVICE`。调用方在 backend 为 CPU 时应忽略返回值。
fn oneasr_device_spec() -> Option<String> {
    normalize_device_spec(std::env::var("ONEASR_DEVICE").ok().as_deref())
}

/// 用户钉了一张卡。未设置 / `auto` 不是钉卡，引擎可以先独显再集显。
pub(super) fn device_pinned() -> bool {
    oneasr_device_spec().is_some()
}

/// Qwen3-ASR（wgpu）的设备选择；命中环境变量时打一条 trace 便于核对选中的卡。
pub(super) fn asr_selector_from_env() -> Result<qwen3_asr_wgpu::DeviceSelector, EngineError> {
    let Some(spec) = oneasr_device_spec() else {
        return Ok(qwen3_asr_wgpu::DeviceSelector::Auto);
    };
    let selector = qwen3_asr_wgpu::DeviceSelector::parse(&spec)
        .map_err(|e| EngineError::new(crate::i18n::device_spec_invalid(&spec, &e.to_string())))?;
    trace_log(format!("ONEASR_DEVICE={spec} → Qwen3-ASR wgpu selector"));
    Ok(selector)
}

/// Qwen3-ForcedAligner（wgpu）的设备选择，策略与 [`asr_selector_from_env`] 一致。
pub(super) fn aligner_selector_from_env()
-> Result<qwen3_aligner_wgpu::gpu::DeviceSelector, EngineError> {
    let Some(spec) = oneasr_device_spec() else {
        return Ok(qwen3_aligner_wgpu::gpu::DeviceSelector::Auto);
    };
    let selector = qwen3_aligner_wgpu::gpu::DeviceSelector::parse(&spec)
        .map_err(|e| EngineError::new(crate::i18n::device_spec_invalid(&spec, &e.to_string())))?;
    trace_log(format!(
        "ONEASR_DEVICE={spec} → ForcedAligner wgpu selector"
    ));
    Ok(selector)
}

/// HTDemucs（wgpu）的设备选择，策略与 [`asr_selector_from_env`] 一致。
pub(super) fn demucs_selector_from_env() -> Result<demucs_core::gpu::DeviceSelector, EngineError> {
    let Some(spec) = oneasr_device_spec() else {
        return Ok(demucs_core::gpu::DeviceSelector::Auto);
    };
    let selector = demucs_core::gpu::DeviceSelector::parse(&spec)
        .map_err(|e| EngineError::new(crate::i18n::device_spec_invalid(&spec, &e.to_string())))?;
    trace_log(format!("ONEASR_DEVICE={spec} → HTDemucs wgpu selector"));
    Ok(selector)
}

/// CTC 对齐器（wgpu）的设备选择，策略与 [`asr_selector_from_env`] 一致。
pub(super) fn ctc_selector_from_env() -> Result<ctc_forced_aligner_wgpu::DeviceSelector, EngineError>
{
    let Some(spec) = oneasr_device_spec() else {
        return Ok(ctc_forced_aligner_wgpu::DeviceSelector::Auto);
    };
    let selector = ctc_forced_aligner_wgpu::DeviceSelector::parse(&spec)
        .map_err(|e| EngineError::new(crate::i18n::device_spec_invalid(&spec, &e.to_string())))?;
    trace_log(format!("ONEASR_DEVICE={spec} → CTC aligner wgpu selector"));
    Ok(selector)
}

#[cfg(test)]
mod tests {
    use super::normalize_device_spec;

    #[test]
    fn empty_blank_and_auto_mean_engine_default() {
        assert_eq!(normalize_device_spec(None), None);
        assert_eq!(normalize_device_spec(Some("")), None);
        assert_eq!(normalize_device_spec(Some("   ")), None);
        assert_eq!(normalize_device_spec(Some("auto")), None);
        assert_eq!(normalize_device_spec(Some("Auto")), None);
        assert_eq!(normalize_device_spec(Some(" AUTO ")), None);
    }

    #[test]
    fn spec_passes_through_trimmed() {
        assert_eq!(
            normalize_device_spec(Some("nvidia")).as_deref(),
            Some("nvidia")
        );
        assert_eq!(
            normalize_device_spec(Some(" vulkan:1 ")).as_deref(),
            Some("vulkan:1")
        );
        assert_eq!(normalize_device_spec(Some("#1")).as_deref(), Some("#1"));
    }

    #[test]
    fn gpu_and_cpu_words_stay_pins_so_parse_can_reject_them() {
        assert_eq!(normalize_device_spec(Some("gpu")).as_deref(), Some("gpu"));
        assert_eq!(normalize_device_spec(Some("cpu")).as_deref(), Some("cpu"));
    }
}
