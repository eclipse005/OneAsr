//! Local engine adapters: Qwen3-ASR, Qwen3-ForcedAligner, HTDemucs.
//!
//! Everything that knows about concrete model crates lives here — the pipeline
//! only sees [`crate::engine`] ports. Backend resolution (CPU/GPU, the
//! CUDA→CPU fallback policy, probing) is also owned here, because that is
//! engine knowledge rather than pipeline policy.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use qwen_forced_aligner_rs::{
    AlignRequest as QwenAlignRequest, AudioInput, DeviceRequest, ModelOptions, Qwen3ForcedAligner,
    TextInput,
};
use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions};

#[cfg(debug_assertions)]
use crate::diagnostics::pipeline_trace;
use crate::diagnostics::trace_log;
use crate::engine::{
    AlignRequest, AlignedToken, Aligner, AsrEngine, EngineError, EngineProvider, SeparateRequest,
    SeparationEvent, Separator, TranscribeRequest, Transcript,
};
use crate::settings::Settings;

/// Resolved compute target for both ASR and Aligner (one policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputeBackend {
    Cpu,
    Cuda,
}

impl ComputeBackend {
    fn label(self) -> &'static str {
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
fn probe_cuda_device() -> &'static Result<CudaProbe, String> {
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
fn cuda_probe_reject_reason(p: &CudaProbe) -> Option<String> {
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
fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, EngineError> {
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
fn is_forced_cuda(backend: &str) -> bool {
    backend.trim().eq_ignore_ascii_case("cuda")
}

/// Append actionable guidance to a CUDA engine load failure.
#[cfg(feature = "cuda")]
fn cuda_load_failure_msg(e: &impl std::fmt::Display) -> String {
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

/// Real engines backed by local weights, with the product's backend policy.
///
/// The resolved backend is remembered after the ASR load: when a GPU load
/// falls back to CPU, the aligner must follow (one policy per run).
pub struct LocalEngineProvider {
    asr_model_dir: PathBuf,
    aligner_model_dir: PathBuf,
    demucs_model_dir: PathBuf,
    backend_pref: String,
    resolved: Cell<ComputeBackend>,
}

impl LocalEngineProvider {
    /// Resolve the compute backend for this run (cheap; no weights are read).
    pub fn from_settings(settings: &Settings) -> Result<Self, EngineError> {
        let resolved = resolve_compute_backend(&settings.backend)?;
        let summary = format!(
            "setting={} resolved={} cuda_dlls={}",
            settings.backend,
            resolved.label(),
            crate::model::is_cuda_runtime_ready(),
        );
        trace_log(format!("backend {summary}"));
        // Debug builds keep the one-line backend summary on stderr (support
        // logs / CLI output); release runs stay quiet unless tracing is on.
        #[cfg(debug_assertions)]
        if !pipeline_trace() {
            eprintln!("[backend] {summary}");
        }
        Ok(Self {
            asr_model_dir: settings.asr_model_dir.clone(),
            aligner_model_dir: settings.aligner_model_dir.clone(),
            demucs_model_dir: settings.resolved_demucs_model_dir(),
            backend_pref: settings.backend.clone(),
            resolved: Cell::new(resolved),
        })
    }

    /// Backend currently in use (after any fallback).
    pub fn resolved_backend(&self) -> &'static str {
        self.resolved.get().label()
    }

    fn forced_cuda(&self) -> bool {
        is_forced_cuda(&self.backend_pref)
    }
}

impl EngineProvider for LocalEngineProvider {
    fn load_asr(&self) -> Result<Box<dyn AsrEngine>, EngineError> {
        let dir = &self.asr_model_dir;
        let first = self.resolved.get();
        match QwenAsrAdapter::load(dir, first) {
            Ok(engine) => Ok(Box::new(engine)),
            // Same rule as `auto` everywhere else: a failed GPU *load* retries
            // on CPU, an explicit 「GPU」 choice fails loudly, and a failure in
            // the middle of a run is never retried (see the pipeline).
            Err(e) if first == ComputeBackend::Cuda && !self.forced_cuda() => {
                trace_log(format!("cuda load failed, falling back to cpu: {e}"));
                self.resolved.set(ComputeBackend::Cpu);
                QwenAsrAdapter::load(dir, ComputeBackend::Cpu)
                    .map(|engine| Box::new(engine) as Box<dyn AsrEngine>)
            }
            Err(e) if first == ComputeBackend::Cuda => {
                #[cfg(feature = "cuda")]
                {
                    Err(EngineError::new(cuda_load_failure_msg(&e)))
                }
                #[cfg(not(feature = "cuda"))]
                {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn load_aligner(&self) -> Result<Box<dyn Aligner>, EngineError> {
        QwenAlignerAdapter::load(&self.aligner_model_dir, self.resolved.get())
            .map(|engine| Box::new(engine) as Box<dyn Aligner>)
    }

    fn load_separator(&self) -> Result<Box<dyn Separator>, EngineError> {
        DemucsSeparatorAdapter::load(&self.demucs_model_dir, &self.backend_pref, |msg| {
            trace_log(msg)
        })
        .map(|engine| Box::new(engine) as Box<dyn Separator>)
    }
}

// ── Qwen3-ASR ────────────────────────────────────────────────────────

struct QwenAsrAdapter {
    inner: AsrInference,
}

impl QwenAsrAdapter {
    fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let backend = match backend {
            ComputeBackend::Cpu => AsrBackend::Cpu,
            ComputeBackend::Cuda => AsrBackend::Cuda,
        };
        AsrInference::load(model_dir, backend)
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl AsrEngine for QwenAsrAdapter {
    fn transcribe(&self, req: TranscribeRequest<'_>) -> Result<Transcript, EngineError> {
        let path = req
            .wav
            .to_str()
            .ok_or_else(|| EngineError::new("音频路径非 UTF-8"))?;
        let opts = TranscribeOptions::default()
            .with_max_new_tokens(req.max_new_tokens)
            .with_language(req.language.to_string());
        self.inner
            .transcribe(path, opts)
            .map(|report| Transcript { text: report.text })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

// ── Qwen3-ForcedAligner ──────────────────────────────────────────────

struct QwenAlignerAdapter {
    inner: Qwen3ForcedAligner,
}

impl QwenAlignerAdapter {
    fn load(model_dir: &Path, backend: ComputeBackend) -> Result<Self, EngineError> {
        let device = match backend {
            ComputeBackend::Cpu => DeviceRequest::Cpu,
            ComputeBackend::Cuda => DeviceRequest::Cuda(0),
        };
        Qwen3ForcedAligner::load(model_dir, ModelOptions { device })
            .map(|inner| Self { inner })
            .map_err(|e| EngineError::new(format!("{e:#}")))
    }
}

impl Aligner for QwenAlignerAdapter {
    fn align(&self, req: AlignRequest<'_>) -> Result<Vec<AlignedToken>, EngineError> {
        let result = self
            .inner
            .align(QwenAlignRequest::new(
                AudioInput::Path(req.wav.to_path_buf()),
                TextInput::Text(req.text.to_string()),
                req.language.to_string(),
            ))
            .map_err(|e| EngineError::new(format!("{e:#}")))?;
        Ok(result
            .items
            .into_iter()
            .map(|item| AlignedToken {
                text: item.text,
                start_sec: item.start_time,
                end_sec: item.end_time,
            })
            .collect())
    }
}

// ── HTDemucs ─────────────────────────────────────────────────────────

/// Weights file expected inside the configured Demucs model directory.
pub const DEMUCS_WEIGHTS_FILE: &str = "htdemucs_ft.safetensors";

/// Backend for the first separation attempt.
///
/// `auto` becomes an explicit GPU attempt (when this build has CUDA) so the
/// fallback is observable and reported, instead of being hidden inside the
/// engine's own `Auto` probing.
fn preferred_separator_backend(pref: &str) -> demucs_core_native::Backend {
    use demucs_core_native::Backend;
    match pref.trim().to_ascii_lowercase().as_str() {
        "cpu" => Backend::Cpu,
        #[cfg(feature = "cuda")]
        _ => Backend::Cuda,
        #[cfg(not(feature = "cuda"))]
        _ => Backend::Cpu,
    }
}

struct DemucsSeparatorAdapter {
    inner: demucs_core_native::Demucs,
    /// Set when `auto` had to fall back to CPU — reported once, on the first run.
    fell_back: Cell<bool>,
}

impl DemucsSeparatorAdapter {
    fn load(
        model_dir: &Path,
        backend_pref: &str,
        log: impl Fn(String),
    ) -> Result<Self, EngineError> {
        use demucs_core_native::{Demucs, LoadOptions, ModelVariant, StemId, StemSelection};

        let weights = model_dir.join(DEMUCS_WEIGHTS_FILE);
        if !weights.is_file() {
            return Err(EngineError::new(format!(
                "人声分离模型不存在: {}（请在设置中下载）",
                weights.display()
            )));
        }

        let opts = LoadOptions {
            variant: ModelVariant::FineTuned,
            stems: StemSelection::Some(vec![StemId::Vocals]),
        };
        let attempt = preferred_separator_backend(backend_pref);
        let mut fell_back = false;

        let inner = match Demucs::load(&weights, opts.clone(), attempt) {
            Ok(d) => d,
            Err(e)
                if attempt != demucs_core_native::Backend::Cpu && !is_forced_cuda(backend_pref) =>
            {
                log(format!(
                    "人声分离 GPU 不可用（{e}），按「自动」改用 CPU（会慢很多）"
                ));
                fell_back = true;
                Demucs::load(&weights, opts, demucs_core_native::Backend::Cpu)
                    .map_err(|e2| EngineError::new(format!("加载人声分离模型失败（cpu）: {e2}")))?
            }
            Err(e) if is_forced_cuda(backend_pref) => {
                return Err(EngineError::new(format!(
                    "加载人声分离模型失败（{}）: {e}\n\
                     已按设置强制使用 GPU，不会自动改用 CPU；\
                     可在设置中把「推理后端」改为「自动」或「CPU」",
                    attempt.tag()
                )));
            }
            Err(e) => {
                return Err(EngineError::new(format!(
                    "加载人声分离模型失败（{}）: {e}",
                    attempt.tag()
                )));
            }
        };

        Ok(Self {
            inner,
            fell_back: Cell::new(fell_back),
        })
    }
}

impl Separator for DemucsSeparatorAdapter {
    fn separate(
        &self,
        req: SeparateRequest<'_>,
        on_event: &mut dyn FnMut(SeparationEvent),
    ) -> Result<PathBuf, EngineError> {
        use demucs_core_native::{SeparationProgress, StemId};

        if self.fell_back.replace(false) {
            on_event(SeparationEvent::FellBackToCpu {
                reason: "GPU 加载失败".into(),
            });
        }

        let source_wav =
            crate::media::extract_audio_wav(req.input, &req.out_dir.join("separate_input.wav"))
                .map_err(|e| EngineError::new(format!("{e}")))?;
        let (left, right, sample_rate) = read_stereo_f32(&source_wav).map_err(EngineError::new)?;

        let stems = self
            .inner
            .separate_with_progress(&left, &right, sample_rate, &mut |p: SeparationProgress| {
                on_event(SeparationEvent::Progress {
                    done: p.done,
                    total: p.total,
                });
            })
            .map_err(|e| EngineError::new(format!("人声分离失败: {e}")))?;

        let vocals = stems
            .iter()
            .find(|s| s.id == StemId::Vocals)
            .ok_or_else(|| EngineError::new("人声分离没有返回 vocals 轨道"))?;
        let out = req.out_dir.join("vocals.wav");
        write_stereo_f32(&out, &vocals.left, &vocals.right, sample_rate)
            .map_err(EngineError::new)?;
        Ok(out)
    }
}

/// Read a WAV as two f32 channels (mono was duplicated by ffmpeg).
fn read_stereo_f32(path: &Path) -> Result<(Vec<f32>, Vec<f32>, u32), String> {
    use hound::SampleFormat;

    let mut reader = hound::WavReader::open(path)
        .map_err(|e| format!("读取分离输入失败 {}: {e}", path.display()))?;
    let spec = reader.spec();
    if spec.channels == 0 {
        return Err("分离输入没有声道".into());
    }
    let samples: Vec<f32> = match spec.sample_format {
        SampleFormat::Float => reader
            .samples::<f32>()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("读取分离输入失败: {e}"))?,
        SampleFormat::Int => {
            // ffmpeg wrote PCM s16le; stay tolerant of other integer widths.
            let bits = spec.bits_per_sample.max(1);
            let scale = (1i64 << (bits - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 / scale))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("读取分离输入失败: {e}"))?
        }
    };

    let channels = spec.channels as usize;
    let mut left = Vec::with_capacity(samples.len() / channels);
    let mut right = Vec::with_capacity(samples.len() / channels);
    for frame in samples.chunks_exact(channels) {
        left.push(frame[0]);
        right.push(frame[channels - 1]);
    }
    Ok((left, right, spec.sample_rate))
}

fn write_stereo_f32(
    path: &Path,
    left: &[f32],
    right: &[f32],
    sample_rate: u32,
) -> Result<(), String> {
    use hound::{SampleFormat, WavSpec, WavWriter};

    let spec = WavSpec {
        channels: 2,
        sample_rate,
        bits_per_sample: 32,
        sample_format: SampleFormat::Float,
    };
    let mut writer = WavWriter::create(path, spec)
        .map_err(|e| format!("写入人声轨道失败 {}: {e}", path.display()))?;
    for (l, r) in left.iter().zip(right.iter()) {
        writer
            .write_sample(*l)
            .and_then(|_| writer.write_sample(*r))
            .map_err(|e| format!("写入人声轨道失败: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("写入人声轨道失败: {e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separator_backend_pref_picks_the_first_attempt() {
        use demucs_core_native::Backend;
        assert_eq!(preferred_separator_backend("cpu"), Backend::Cpu);
        assert_eq!(preferred_separator_backend(" CPU "), Backend::Cpu);
        #[cfg(feature = "cuda")]
        {
            assert_eq!(preferred_separator_backend("auto"), Backend::Cuda);
            assert_eq!(preferred_separator_backend("nonsense"), Backend::Cuda);
        }
        #[cfg(not(feature = "cuda"))]
        {
            assert_eq!(preferred_separator_backend("auto"), Backend::Cpu);
            assert_eq!(preferred_separator_backend("nonsense"), Backend::Cpu);
        }
        assert!(is_forced_cuda("cuda"));
        assert!(is_forced_cuda(" CUDA "));
        assert!(!is_forced_cuda("auto"));
        assert!(!is_forced_cuda("cpu"));
    }

    #[test]
    fn missing_weights_is_a_clear_error() {
        let dir = std::env::temp_dir().join(format!("oneasr_sep_missing_{}", std::process::id()));
        let err = match DemucsSeparatorAdapter::load(&dir, "cpu", |_| {}) {
            Ok(_) => panic!("missing weights must fail to load"),
            Err(e) => e.to_string(),
        };
        assert!(err.contains("人声分离模型不存在"), "{err}");
    }

    #[test]
    fn provider_resolves_cpu_when_asked() {
        let settings = Settings {
            backend: "cpu".into(),
            ..Settings::default()
        };
        let provider = LocalEngineProvider::from_settings(&settings).unwrap();
        assert_eq!(provider.resolved_backend(), "cpu");
    }

    /// GPU smoke: exercise the real device probe used by backend resolution.
    /// Skips gracefully on machines without a CUDA device/driver.
    #[cfg(feature = "cuda")]
    #[test]
    fn probe_cuda_device_smoke() {
        match probe_cuda_device() {
            Ok(p) => {
                eprintln!(
                    "probe: {} sm_{}{} vram={:.1}GB reject={:?}",
                    p.name,
                    p.cc.0,
                    p.cc.1,
                    p.total_vram as f64 / 1e9,
                    cuda_probe_reject_reason(p)
                );
            }
            Err(e) => eprintln!("no usable CUDA device: {e}"),
        }
    }
}
