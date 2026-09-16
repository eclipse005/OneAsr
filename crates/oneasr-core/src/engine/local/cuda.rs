//! Backend resolution: CPU vs CUDA, device probing, fallback policy.

use crate::diagnostics::trace_log;
use crate::engine::EngineError;
/// Resolved compute target for both ASR and Aligner (one policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComputeBackend {
    Cpu,
    Cuda,
}

impl ComputeBackend {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
        }
    }
}

/// Engines ship prebuilt PTX for sm_61+ (see the engine crates'
/// `prebuilt_ptx`). Older cards physically cannot run the kernels, so this
/// stays a hard gate. VRAM size is only a settings-page hint, not a gate —
/// small-VRAM GPUs degrade gracefully via WDDM paging and the user may still
/// prefer GPU.
#[cfg(feature = "cuda")]
const MIN_CUDA_CC: (i32, i32) = (6, 1);

/// Live GPU facts from a real device probe (cached — probe cost is ~0.5s).
#[cfg(feature = "cuda")]
pub(crate) struct CudaProbe {
    pub(crate) name: String,
    pub(crate) total_vram: usize,
    pub(crate) cc: (i32, i32),
}

/// Probe the real GPU once per process. DLL presence
/// (`is_cuda_runtime_ready`) only proves files exist; this proves a usable
/// NVIDIA device + driver.
#[cfg(feature = "cuda")]
pub(super) fn probe_cuda_device() -> &'static Result<CudaProbe, String> {
    static PROBE: std::sync::OnceLock<Result<CudaProbe, String>> = std::sync::OnceLock::new();
    PROBE.get_or_init(|| {
        let ctx = cudarc::driver::CudaContext::new(0)
            .map_err(|e| format!("CUDA 初始化失败（无可用 NVIDIA 显卡或驱动异常）: {e:?}"))?;
        let name = ctx.name().map_err(|e| format!("读取显卡名称失败: {e:?}"))?;
        let cc = ctx
            .compute_capability()
            .map_err(|e| format!("读取 compute capability 失败: {e:?}"))?;
        let total_vram = ctx
            .total_mem()
            .map_err(|e| format!("读取显存大小失败: {e:?}"))?;
        Ok(CudaProbe {
            name,
            total_vram,
            cc,
        })
    })
}

/// Why a probed GPU is still unsuitable for the CUDA engine.
#[cfg(feature = "cuda")]
pub(super) fn cuda_probe_reject_reason(p: &CudaProbe) -> Option<String> {
    if p.cc < MIN_CUDA_CC {
        return Some(format!(
            "显卡 {name} 过旧（sm_{maj}{min}），GPU 加速最低需要 sm_61（GTX 10 系）",
            name = p.name,
            maj = p.cc.0,
            min = p.cc.1
        ));
    }
    None
}

/// Resolve the inference backend from settings.
///
/// Product rule (single installer + optional Settings「安装组件」):
/// - **cpu** → always CPU
/// - **cuda** → require app `dll/` CUDA runtime **and** a live probe of a
///   usable NVIDIA GPU (≥ sm_61); error with guidance otherwise
/// - **auto** → CUDA only when DLLs are ready *and* the probe passes;
///   otherwise CPU with a trace log (never hard-fail auto on GPU issues)
pub(super) fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, EngineError> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "cpu" => Ok(ComputeBackend::Cpu),
        "cuda" => {
            #[cfg(feature = "cuda")]
            {
                if !crate::model::is_cuda_runtime_ready() {
                    return Err(EngineError::new(
                        "未检测到 CUDA 运行库，请在设置中下载后再使用 GPU",
                    ));
                }
                match probe_cuda_device() {
                    Ok(p) => {
                        if let Some(reason) = cuda_probe_reject_reason(p) {
                            return Err(EngineError::new(format!(
                                "{reason}，请在设置中改用 CPU 或自动"
                            )));
                        }
                        Ok(ComputeBackend::Cuda)
                    }
                    Err(e) => Err(EngineError::new(format!(
                        "{e}。GPU 加速需要 NVIDIA 显卡（显存 4GB 起）并更新驱动；\
                         或在设置中改用 CPU"
                    ))),
                }
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err(EngineError::new("此构建未启用 CUDA，请使用 backend=cpu"))
            }
        }
        // auto
        _ => {
            #[cfg(feature = "cuda")]
            {
                if !crate::model::is_cuda_runtime_ready() {
                    return Ok(ComputeBackend::Cpu);
                }
                match probe_cuda_device() {
                    Ok(p) => {
                        if let Some(reason) = cuda_probe_reject_reason(p) {
                            trace_log(format!("auto backend → cpu: {reason}"));
                            Ok(ComputeBackend::Cpu)
                        } else {
                            trace_log(format!(
                                "auto backend → cuda: {} sm_{}{} vram={:.1}GB",
                                p.name,
                                p.cc.0,
                                p.cc.1,
                                p.total_vram as f64 / 1e9
                            ));
                            Ok(ComputeBackend::Cuda)
                        }
                    }
                    Err(e) => {
                        trace_log(format!("auto backend → cpu: {e}"));
                        Ok(ComputeBackend::Cpu)
                    }
                }
            }
            #[cfg(not(feature = "cuda"))]
            {
                Ok(ComputeBackend::Cpu)
            }
        }
    }
}

/// Settings explicitly pinned to GPU (vs auto/cpu) — no silent CPU fallback.
pub(super) fn is_forced_cuda(backend: &str) -> bool {
    backend.trim().eq_ignore_ascii_case("cuda")
}

/// Append actionable guidance to a CUDA engine load failure.
#[cfg(feature = "cuda")]
pub(super) fn cuda_load_failure_msg(e: &impl std::fmt::Display) -> String {
    let probe_hint = match probe_cuda_device() {
        Ok(p) => format!(
            "（显卡 {}，显存 {:.1}GB）",
            p.name,
            p.total_vram as f64 / 1e9
        ),
        Err(_) => String::new(),
    };
    format!(
        "加载语音识别模型失败: {e:#}{probe_hint}\n\
         请检查：1) 显存 ≥ 4GB 且未被其他程序占满；2) NVIDIA 驱动已更新；\
         3) 关闭占用 GPU 的程序后重试。或在设置中将后端改为 CPU"
    )
}
