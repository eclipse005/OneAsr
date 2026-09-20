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
pub(super) fn probe_gpu_device() -> &'static Result<GpuProbe, String> {
    static PROBE: std::sync::OnceLock<Result<GpuProbe, String>> = std::sync::OnceLock::new();
    PROBE.get_or_init(|| {
        let devices = qwen3_asr_wgpu::AsrInference::devices();
        let gpu = devices.iter().find(|d| {
            // `DeviceInfo::device_type` is `wgpu::DeviceType`; Debug of Cpu is "Cpu".
            format!("{:?}", d.device_type) != "Cpu"
        });
        match gpu {
            Some(d) => Ok(GpuProbe {
                description: d.describe(),
            }),
            None => Err("未检测到可用 GPU（需要已安装的显卡驱动）".into()),
        }
    })
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
            Err(e) => Err(EngineError::new(format!(
                "{e}。GPU 加速需要已安装的显卡驱动；或在设置中改用 CPU"
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
        Ok(p) => format!("（{}）", p.description),
        Err(_) => String::new(),
    };
    format!(
        "加载语音识别模型失败: {e:#}{probe_hint}\n\
         请检查：1) 显卡驱动已安装并更新；2) 关闭占用 GPU 的程序后重试。\
         或在设置中将后端改为 CPU"
    )
}
