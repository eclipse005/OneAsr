//! Backend resolution: CPU vs wgpu GPU, device probing, fallback policy.

use crate::diagnostics::trace_log;
use crate::engine::EngineError;

/// Resolved compute target for both ASR and Aligner (one policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComputeBackend {
    Cpu,
    Gpu,
}

impl ComputeBackend {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
        }
    }
}

/// Live GPU facts from wgpu adapter enumeration (cached — first call is slow).
pub(crate) struct GpuProbe {
    pub(crate) description: String,
}

/// Probe adapters once per process. A driver is enough; there is no extra
/// runtime pack to download.
///
/// **只缓存成功**：适配器枚举失败可能只是瞬时的（驱动正在重装、远程会话里
/// 第一次枚举超时、另一个进程正占着显存），把它缓存下来等于让整个进程终身
/// 回落 CPU —— 调用方下一次会重新探测，成功后才写进缓存。
pub(super) fn probe_gpu_device() -> Result<&'static GpuProbe, String> {
    static PROBE: std::sync::OnceLock<GpuProbe> = std::sync::OnceLock::new();
    if let Some(probe) = PROBE.get() {
        return Ok(probe);
    }
    let devices = qwen3_asr_wgpu::AsrInference::devices();
    let gpu = devices.iter().find(|d| {
        // `DeviceInfo::device_type` is `wgpu::DeviceType`; Debug of Cpu is "Cpu".
        format!("{:?}", d.device_type) != "Cpu"
    });
    match gpu {
        Some(d) => Ok(PROBE.get_or_init(|| GpuProbe {
            description: d.describe(),
        })),
        None => Err(crate::i18n::no_gpu_detected()),
    }
}

/// Resolve the inference backend from settings.
///
/// Product rule (one binary, driver-only GPU):
/// - **cpu** → always CPU
/// - **gpu** → require a live wgpu adapter; error otherwise
/// - **auto** → GPU when an adapter exists, otherwise CPU (never hard-fail auto)
pub(super) fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, EngineError> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "cpu" => Ok(ComputeBackend::Cpu),
        "gpu" => match probe_gpu_device() {
            Ok(_) => Ok(ComputeBackend::Gpu),
            Err(e) => Err(EngineError::new(crate::i18n::gpu_forced_but_unavailable(
                &e,
            ))),
        },
        // auto
        _ => match probe_gpu_device() {
            Ok(p) => {
                trace_log(format!("auto backend → gpu: {}", p.description));
                Ok(ComputeBackend::Gpu)
            }
            Err(e) => {
                trace_log(format!("auto backend → cpu: {e}"));
                Ok(ComputeBackend::Cpu)
            }
        },
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
