//! Backend resolution: CPU vs wgpu GPU, device probing, fallback policy.

use crate::engine::EngineError;

/// Resolved compute target for both ASR and Aligner (one policy).
///
/// `Gpu` means the user required a GPU. `Auto` lets the engine walk a discrete
/// GPU, then an integrated one, and use CPU only when that walk fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComputeBackend {
    Cpu,
    Auto,
    Gpu,
}

impl ComputeBackend {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Auto => "auto",
            Self::Gpu => "gpu",
        }
    }
}

/// Live GPU facts from wgpu adapter enumeration (cached — first call is slow).
pub(crate) struct GpuProbe {
    pub(crate) description: String,
    /// Multi-line adapter list + the one Auto would pick. Written to the task
    /// journal before any model is loaded, so a GPU that dies later still has
    /// its make/model/API/driver on record.
    pub(crate) report: String,
}

/// Multi-line adapter report for the task journal.
///
/// Lines come from the engine's own `DeviceTarget::describe()` (name, backend,
/// device type, driver, binding size, subgroup) — the same enumeration Auto
/// uses, so the journal and the engine cannot disagree about the order.
fn format_gpu_report(descriptions: &[String], chosen: Option<&str>) -> String {
    let mut report = String::from("adapters (auto order):");
    if descriptions.is_empty() {
        report.push_str("\n  <none>");
    } else {
        for line in descriptions {
            report.push_str("\n  ");
            report.push_str(line);
        }
    }
    match chosen {
        Some(spec) => {
            report.push_str("\nchosen: ");
            report.push_str(spec);
        }
        None => report.push_str("\nchosen: <none>"),
    }
    report
}

/// Adapter report for the GUI's task journal.
///
/// Never empty: a machine with no usable adapter gets an `unavailable (...)`
/// line instead, which is itself the answer to "why did this run fall back?".
pub fn gpu_report() -> String {
    match probe_gpu_device() {
        Ok(probe) => probe.report.clone(),
        Err(error) => format!("unavailable ({error})"),
    }
}

/// Probe adapters once per process. A driver is enough; there is no extra
/// runtime pack to download.
///
/// **只缓存成功**：适配器枚举失败可能只是瞬时的（驱动正在重装、远程会话里
/// 第一次枚举超时、另一个进程正占着显存），把它缓存下来等于让整个进程终身
/// 回落 CPU —— 调用方下一次会重新探测，成功后才写进缓存。
///
/// 报的是 `device_targets()` 里 `is_default` 的那一个，也就是 `DeviceSelector::Auto`
/// **先尝试**的那张卡（独显优先于集显）。不要用 `devices()`（`list_devices()`）自己
/// `find`：那是 wgpu 的**原始枚举序**，没有「独显优先」，在这台双显卡机器上它把
/// Intel 核显排在 NVIDIA 前面，于是日志报核显、引擎却跑独显。选卡策略只该有一个
/// 出处，就是引擎 crate 自己那份排序。探测失败只说明「现在没看到卡」，`Auto`
/// 仍交给引擎再走一遍，不在这里改判成 CPU。
pub(super) fn probe_gpu_device() -> Result<&'static GpuProbe, String> {
    static PROBE: std::sync::OnceLock<GpuProbe> = std::sync::OnceLock::new();
    if let Some(probe) = PROBE.get() {
        return Ok(probe);
    }
    let targets = qwen3_asr_wgpu::AsrInference::device_targets();
    // `is_default` 就是 Auto 会选的那个；找不到（非 CPU 但没标默认）再退一档，
    // 只要**不是 CPU**就算有 GPU —— 这一段的职责是判断「有没有卡」，不是选卡。
    let gpu = targets.iter().find(|t| t.is_default).or_else(|| {
        targets
            .iter()
            .find(|t| format!("{:?}", t.info.device_type) != "Cpu")
    });
    match gpu {
        Some(t) => {
            let descriptions: Vec<String> = targets
                .iter()
                .map(|target| {
                    let mut line = target.describe();
                    // Vulkan's AdapterInfo.driver is the vendor name; the version
                    // lives in driver_info. Normalize it to one line first:
                    // Intel's string contains a newline ("101.886\n0").
                    let driver_info = target
                        .info
                        .driver_info
                        .replace(['\n', '\r'], " ")
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ");
                    if !driver_info.is_empty() && !line.contains(&driver_info) {
                        line.push_str(" | ");
                        line.push_str(&driver_info);
                    }
                    line
                })
                .collect();
            Ok(PROBE.get_or_init(|| GpuProbe {
                description: t.describe(),
                report: format_gpu_report(&descriptions, Some(t.spec.as_str())),
            }))
        }
        None => Err(crate::i18n::no_gpu_detected()),
    }
}

/// Resolve the inference backend from settings.
///
/// Product rule (one binary, driver-only GPU):
/// - **cpu** → always CPU
/// - **gpu** → require a live wgpu adapter; error otherwise. The engine then
///   tries a discrete GPU before an integrated one, and does not fall back.
/// - **auto** → [`ComputeBackend::Auto`]. The engine walks discrete, then
///   integrated, then CPU. A probe miss must not collapse this to CPU first.
pub(super) fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, EngineError> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "cpu" => Ok(ComputeBackend::Cpu),
        "gpu" => match probe_gpu_device() {
            Ok(_) => Ok(ComputeBackend::Gpu),
            Err(e) => Err(EngineError::new(crate::i18n::gpu_forced_but_unavailable(
                &e,
            ))),
        },
        _ => Ok(ComputeBackend::Auto),
    }
}

/// Settings explicitly pinned to GPU (vs auto/cpu) — no silent CPU fallback.
pub(super) fn is_forced_gpu(backend: &str) -> bool {
    backend.trim().eq_ignore_ascii_case("gpu")
}

/// Append actionable guidance to a GPU engine load failure.
pub(super) fn gpu_load_failure_msg(e: &impl std::fmt::Display) -> String {
    let probe_hint = match probe_gpu_device() {
        Ok(p) => crate::i18n::gpu_probe_hint(&p.description),
        Err(_) => String::new(),
    };
    crate::i18n::asr_gpu_load_failed(&format!("{e:#}"), &probe_hint)
}

#[cfg(test)]
mod tests {
    use super::format_gpu_report;

    /// The journal needs the API/driver of every adapter and the one Auto picks.
    #[test]
    fn gpu_report_lists_adapters_and_choice() {
        let report = format_gpu_report(
            &[
                "* vulkan:0  NVIDIA GeForce RTX 3060 (Vulkan, DiscreteGpu, driver 580.88)".into(),
                "  dx12:1  Intel UHD Graphics 770 (Dx12, IntegratedGpu, driver 31.0.101.5333)"
                    .into(),
            ],
            Some("vulkan:0"),
        );
        assert!(report.starts_with("adapters (auto order):"), "{report}");
        assert!(report.contains("vulkan:0"), "{report}");
        assert!(report.contains("NVIDIA GeForce RTX 3060"), "{report}");
        assert!(report.contains("Intel UHD Graphics 770"), "{report}");
        assert!(report.contains("chosen: vulkan:0"), "{report}");
    }

    /// No adapter is a fact to record, not an empty block.
    #[test]
    fn gpu_report_records_absence() {
        let report = format_gpu_report(&[], None);
        assert!(report.contains("<none>"), "{report}");
        assert!(report.contains("chosen: <none>"), "{report}");
    }
}
