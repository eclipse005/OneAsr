//! Qwen3-ASR + ForcedAligner pipeline.
//!
//! ```text
//! load ASR once → all VAD chunks transcribed → unload ASR
//! load Aligner once → all chunks aligned → unload Aligner
//! word normalize → sentence_boundary → SRT
//! ```
//!
//! Never keeps ASR and Aligner in VRAM at the same time.
//!
//! **Scratch lifecycle**: `runs/{stem}_{ts}/` holds only the full 16 kHz WAV
//! plus at most one temporary chunk file. Product output is
//! `{settings.output_dir}/{stem}.srt` (default `{app}/output`, atomic write).
//! On success the scratch dir is removed.

use std::path::{Path, PathBuf};
use std::time::Instant;

use qwen3_asr::{AsrInference, Backend as AsrBackend, TranscribeOptions};
use qwen_forced_aligner_rs::{
    AlignRequest, AudioInput, DeviceRequest, ModelOptions, Qwen3ForcedAligner, TextInput,
};
use thiserror::Error;

use crate::lang::{to_lang_key, to_qwen_language_label};
use crate::media::{
    convert_to_16k_mono_wav, slice_wav, wav_duration_sec, write_atomic,
};
use crate::paths::{media_stem, output_srt_path};
use crate::sentence_boundary::{
    build_source_sentences_from_words, source_sentences_to_srt, SentenceBoundaryRequest,
    WordTokenDto,
};
use crate::settings::Settings;
use crate::subtitle::alignment::align_text_to_timestamps;
use crate::subtitle::segmenter::{normalize_word_tokens, WordToken};
use crate::vad;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("{0}")]
    Msg(String),
}

const MIN_SILENCE_FALLBACK: f32 = 0.3;

/// Opt-in pipeline diagnostics (`ONEASR_PIPELINE_TRACE=1`).
fn pipeline_trace() -> bool {
    matches!(
        std::env::var("ONEASR_PIPELINE_TRACE").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes")
    )
}

fn trace_log(msg: impl AsRef<str>) {
    if pipeline_trace() {
        eprintln!("[pipeline] {}", msg.as_ref());
    }
}

/// Fine-grained pipeline stage for live UI status and timing breakdown.
///
/// Order in a successful run:
/// `Converting` → `Planning` → `LoadingAsr` → `Transcribing` →
/// `LoadingAligner` → `Aligning` → `Exporting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrStage {
    /// ffmpeg → 16 kHz mono master WAV.
    Converting,
    /// VAD + chunk plan (or single-chunk plan for short audio).
    Planning,
    /// Load Qwen ASR weights.
    LoadingAsr,
    /// ASR inference over planned chunks.
    Transcribing,
    /// Load ForcedAligner weights.
    LoadingAligner,
    /// Forced alignment over transcript chunks.
    Aligning,
    /// Sentence boundary + SRT write.
    Exporting,
}

impl AsrStage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Converting => "转码音频",
            Self::Planning => "分段规划",
            Self::LoadingAsr => "加载 ASR",
            Self::Transcribing => "转写中",
            Self::LoadingAligner => "加载 Aligner",
            Self::Aligning => "打轴中",
            Self::Exporting => "导出字幕",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StageUpdate {
    pub stage: AsrStage,
    pub chunk: Option<(usize, usize)>,
}

impl StageUpdate {
    pub fn new(stage: AsrStage) -> Self {
        Self { stage, chunk: None }
    }

    pub fn with_chunk(stage: AsrStage, current: usize, total: usize) -> Self {
        Self {
            stage,
            chunk: Some((current, total)),
        }
    }

    pub fn label(&self) -> String {
        match self.chunk {
            Some((cur, total)) if total > 1 => format!("{} {cur}/{total}", self.stage.label()),
            _ => self.stage.label().to_string(),
        }
    }
}

/// Wall time spent in one pipeline stage (same [`AsrStage`] across chunk updates).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageTiming {
    pub stage: AsrStage,
    pub elapsed_ms: u64,
}

/// End-to-end processing timing for one task (UI hover breakdown).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskTiming {
    pub stages: Vec<StageTiming>,
    /// Wall-clock from job start to finish (may exceed sum of stages slightly).
    pub total_ms: u64,
}

impl TaskTiming {
    pub fn total_label(&self) -> String {
        format_process_ms(self.total_ms)
    }

    /// No stage rows (failures before the first `on_stage`).
    ///
    /// `total_ms` alone is not enough to show a chip — stages drive the UI.
    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// Worth showing the 用时 chip + hover breakdown.
    pub fn has_breakdown(&self) -> bool {
        !self.is_empty()
    }
}

/// Human-readable process duration: `ms` / `s` / `m:ss` / `h:mm:ss`.
pub fn format_process_ms(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        let s = ms as f64 / 1000.0;
        if s < 10.0 {
            format!("{s:.1} s")
        } else {
            format!("{} s", s.round() as u64)
        }
    } else {
        let total_s = (ms + 500) / 1000;
        let h = total_s / 3600;
        let m = (total_s % 3600) / 60;
        let s = total_s % 60;
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }
}

/// Accumulates per-stage wall time from [`StageUpdate`]s on the ASR worker.
///
/// Same [`AsrStage`] across chunk progress stays one open interval; stage
/// changes close the previous interval. Consecutive identical stages are merged.
#[derive(Debug)]
pub struct StageClock {
    wall_start: Instant,
    current: Option<(AsrStage, Instant)>,
    stages: Vec<StageTiming>,
}

impl Default for StageClock {
    fn default() -> Self {
        Self::new()
    }
}

impl StageClock {
    pub fn new() -> Self {
        Self {
            wall_start: Instant::now(),
            current: None,
            stages: Vec::new(),
        }
    }

    pub fn note(&mut self, update: &StageUpdate) {
        let stage = update.stage;
        match self.current {
            Some((cur, _)) if cur == stage => {}
            Some((cur, t0)) => {
                let elapsed_ms = t0.elapsed().as_millis() as u64;
                push_or_merge_stage(&mut self.stages, cur, elapsed_ms);
                self.current = Some((stage, Instant::now()));
            }
            None => {
                self.current = Some((stage, Instant::now()));
            }
        }
    }

    pub fn finish(mut self) -> TaskTiming {
        if let Some((stage, t0)) = self.current.take() {
            let elapsed_ms = t0.elapsed().as_millis() as u64;
            push_or_merge_stage(&mut self.stages, stage, elapsed_ms);
        }
        TaskTiming {
            stages: self.stages,
            total_ms: self.wall_start.elapsed().as_millis() as u64,
        }
    }
}

fn push_or_merge_stage(stages: &mut Vec<StageTiming>, stage: AsrStage, elapsed_ms: u64) {
    if let Some(last) = stages.last_mut() {
        if last.stage == stage {
            last.elapsed_ms = last.elapsed_ms.saturating_add(elapsed_ms);
            return;
        }
    }
    stages.push(StageTiming { stage, elapsed_ms });
}

pub fn check_asr_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Msg(format!(
            "ASR 模型目录不存在: {}",
            model_dir.display()
        )));
    }
    let mut missing = Vec::new();
    for name in ["config.json", "tokenizer.json"] {
        if !model_dir.join(name).is_file() {
            missing.push(name.to_string());
        }
    }
    let index = model_dir.join("model.safetensors.index.json");
    let single = model_dir.join("model.safetensors");
    if index.is_file() {
        // Shards checked loosely — load will fail with a clear error if incomplete.
    } else if !single.is_file() {
        missing.push("model.safetensors 或 model.safetensors.index.json".into());
    }
    if !missing.is_empty() {
        return Err(AsrError::Msg(format!(
            "ASR 模型文件不全（缺少 {}）: {}",
            missing.join(", "),
            model_dir.display()
        )));
    }
    Ok(())
}

pub fn check_aligner_model_dir(model_dir: &Path) -> Result<(), AsrError> {
    if !model_dir.is_dir() {
        return Err(AsrError::Msg(format!(
            "Aligner 模型目录不存在: {}",
            model_dir.display()
        )));
    }
    let mut missing = Vec::new();
    if !model_dir.join("config.json").is_file() {
        missing.push("config.json");
    }
    if !model_dir.join("model.safetensors").is_file()
        && !model_dir.join("model.safetensors.index.json").is_file()
    {
        missing.push("model.safetensors");
    }
    if !missing.is_empty() {
        return Err(AsrError::Msg(format!(
            "Aligner 模型文件不全（缺少 {}）: {}",
            missing.join(", "),
            model_dir.display()
        )));
    }
    Ok(())
}

/// Resolved compute target for both ASR and Aligner (one policy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComputeBackend {
    Cpu,
    Cuda,
}

/// Engines ship prebuilt PTX for sm_61+ (see qwen3-asr `prebuilt_ptx`).
/// Older cards physically cannot run the kernels, so this stays a hard gate
/// (VRAM size is only a settings-page hint, not a gate — small-VRAM GPUs
/// degrade gracefully via WDDM paging and the user may still prefer GPU).
#[cfg(feature = "cuda")]
const MIN_CUDA_CC: (i32, i32) = (6, 1);

/// Live GPU facts from a real device probe (cached — probe cost is ~0.5s).
#[cfg(feature = "cuda")]
struct CudaProbe {
    name: String,
    total_vram: usize,
    cc: (i32, i32),
}

/// Probe the real GPU once per process. DLL presence (`is_cuda_runtime_ready`)
/// only proves files exist; this proves a usable NVIDIA device + driver.
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

impl ComputeBackend {
    fn to_asr(self) -> AsrBackend {
        match self {
            Self::Cpu => AsrBackend::Cpu,
            Self::Cuda => AsrBackend::Cuda,
        }
    }

    fn to_align_device(self) -> DeviceRequest {
        match self {
            Self::Cpu => DeviceRequest::Cpu,
            Self::Cuda => DeviceRequest::Cuda(0),
        }
    }
}

/// Resolve inference backend from settings.
///
/// Product rule (single installer + optional Settings「安装组件」):
/// - **cpu** → always CPU
/// - **cuda** → require app `dll/` CUDA runtime **and** a live probe of a
///   usable NVIDIA GPU (≥ sm_61); error with guidance otherwise
/// - **auto** → CUDA only when DLLs are ready *and* the probe passes;
///   otherwise CPU with a trace log (never hard-fail auto on GPU issues)
fn resolve_compute_backend(backend: &str) -> Result<ComputeBackend, AsrError> {
    match backend.trim().to_ascii_lowercase().as_str() {
        "cpu" => Ok(ComputeBackend::Cpu),
        "cuda" => {
            #[cfg(feature = "cuda")]
            {
                if !crate::model::is_cuda_runtime_ready() {
                    return Err(AsrError::Msg(
                        "未检测到 CUDA 运行库，请在设置中下载后再使用 GPU".into(),
                    ));
                }
                match probe_cuda_device() {
                    Ok(p) => {
                        if let Some(reason) = cuda_probe_reject_reason(p) {
                            return Err(AsrError::Msg(format!(
                                "{reason}，请在设置中改用 CPU 或自动"
                            )));
                        }
                        Ok(ComputeBackend::Cuda)
                    }
                    Err(e) => Err(AsrError::Msg(format!(
                        "{e}。GPU 加速需要 NVIDIA 显卡（显存 4GB 起）并更新驱动；\
                         或在设置中改用 CPU"
                    ))),
                }
            }
            #[cfg(not(feature = "cuda"))]
            {
                Err(AsrError::Msg(
                    "此构建未启用 CUDA，请使用 backend=cpu".into(),
                ))
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
        "加载 ASR 失败: {e:#}{probe_hint}\n\
         请检查：1) 显存 ≥ 4GB 且未被其他程序占满；2) NVIDIA 驱动已更新；\
         3) 关闭占用 GPU 的程序后重试。或在设置中将后端改为 CPU"
    )
}

fn clean_asr_text(raw: &str) -> String {
    let mut text = raw.trim();
    for marker in ["<asr_text>", "asr_text>"] {
        if let Some((_, rest)) = text.split_once(marker) {
            text = rest.trim();
        }
    }
    text.trim_matches(['<', '>']).trim().to_string()
}

fn round_millis(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// One non-empty ASR segment; times are on the global timeline (seconds).
/// Audio is re-sliced from the master 16 kHz WAV on demand — no persistent chunk files.
struct ChunkTranscript {
    start_sec: f64,
    end_sec: f64,
    text: String,
    language: String,
}

/// Optional side exports from the pipeline (CLI / eval harness).
///
/// Product GUI only needs the SRT under `settings.output_dir`. Eval harnesses
/// may also request ForcedAligner word/char tokens with timestamps (written
/// **after** a successful SRT; failure is non-fatal).
#[derive(Debug, Clone, Default)]
pub struct ProcessExportOptions {
    /// Write normalized aligner tokens as JSON (`words: [{text,start,end}, …]`).
    /// Best-effort: errors are logged and do not fail the pipeline after SRT.
    pub words_json: Option<PathBuf>,
}

/// Full pipeline: convert → VAD plan → ASR all → unload → align all → SRT.
pub fn process_media_file_with_progress(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    on_stage: impl FnMut(StageUpdate),
) -> Result<PathBuf, AsrError> {
    process_media_file_with_export(
        input,
        media_name,
        settings,
        app_root,
        on_stage,
        ProcessExportOptions::default(),
    )
}

/// Same as [`process_media_file_with_progress`] plus optional word-timestamp dump.
pub fn process_media_file_with_export(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    mut on_stage: impl FnMut(StageUpdate),
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    let stem = media_stem(input);
    let srt_path = output_srt_path(&settings.resolved_output_dir(), &stem);

    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let work_dir = app_root.join("runs").join(format!("{stem}_{stamp}"));
    std::fs::create_dir_all(&work_dir).map_err(|e| AsrError::Msg(e.to_string()))?;
    if let Some(parent) = srt_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AsrError::Msg(e.to_string()))?;
    }

    // 1. Convert → single master PCM under work_dir
    on_stage(StageUpdate::new(AsrStage::Converting));
    let wav = work_dir.join("input_16k.wav");
    convert_to_16k_mono_wav(input, &wav).map_err(|e| AsrError::Msg(e.to_string()))?;

    let duration = wav_duration_sec(&wav).unwrap_or(0.0) as f32;
    let chunk_sec = settings.chunk_target_seconds_clamped() as f32;

    // 2. VAD + chunk plan (own stage so convert timing stays pure)
    on_stage(StageUpdate::new(AsrStage::Planning));
    let (chunks, vad_speech) = if duration > chunk_sec {
        let speech = vad::run_vad(&wav)?;
        let silences = vad::speech_to_silences(&speech, duration, MIN_SILENCE_FALLBACK);
        let planned = vad::plan_chunks(duration, &silences, chunk_sec);
        #[cfg(debug_assertions)]
        eprintln!(
            "[vad] duration={duration:.1}s speech={} silence_gaps={} chunks={}",
            speech.len(),
            silences.len(),
            planned.len(),
        );
        let vad_pairs: Vec<(f64, f64)> = speech
            .iter()
            .map(|&(s, e)| (s as f64, e as f64))
            .collect();
        (planned, vad_pairs)
    } else {
        (
            vec![vad::Chunk {
                start: 0.0,
                end: duration,
            }],
            vec![(0.0, duration as f64)],
        )
    };

    let multi_chunk = chunks.len() > 1;
    let force_lang = to_qwen_language_label(&settings.language);
    let source_lang_key = to_lang_key(&settings.language);
    let total_chunks = chunks.len().max(1);

    if pipeline_trace() {
        eprintln!(
            "[pipeline] duration={duration:.1}s chunk_target={chunk_sec} chunks={total_chunks} lang={force_lang}"
        );
        for (i, c) in chunks.iter().enumerate() {
            eprintln!(
                "[pipeline] plan#{i} {:.3}-{:.3} ({:.1}s)",
                c.start,
                c.end,
                c.end - c.start
            );
        }
    }

    // Scratch path for the one active slice (deleted after each use when multi).
    let chunk_tmp = work_dir.join("chunk_tmp.wav");

    // 3. Load ASR once → all chunks → drop
    on_stage(StageUpdate::new(AsrStage::LoadingAsr));
    check_asr_model_dir(&settings.asr_model_dir)?;
    let mut compute = resolve_compute_backend(&settings.backend)?;
    {
        let backend_label = match compute {
            ComputeBackend::Cpu => "cpu",
            ComputeBackend::Cuda => "cuda",
        };
        trace_log(format!(
            "backend setting={} resolved={} cuda_dlls={}",
            settings.backend,
            backend_label,
            crate::model::is_cuda_runtime_ready(),
        ));
        #[cfg(debug_assertions)]
        if !pipeline_trace() {
            eprintln!(
                "[backend] setting={} resolved={} cuda_dlls={}",
                settings.backend,
                backend_label,
                crate::model::is_cuda_runtime_ready(),
            );
        }
    }
    // Auto mode only: a CUDA load that still fails (OOM, driver hiccup, …)
    // degrades to CPU instead of killing the job. Forced `cuda` gets the
    // guided error from `cuda_load_failure_msg` instead.
    #[cfg(feature = "cuda")]
    let asr = match AsrInference::load(&settings.asr_model_dir, compute.to_asr()) {
        Ok(asr) => asr,
        Err(e) if compute == ComputeBackend::Cuda && !is_forced_cuda(&settings.backend) => {
            trace_log(format!("cuda load failed, falling back to cpu: {e:#}"));
            compute = ComputeBackend::Cpu;
            AsrInference::load(&settings.asr_model_dir, ComputeBackend::Cpu.to_asr())
                .map_err(|e2| AsrError::Msg(format!("加载 ASR 失败: {e2:#}")))?
        }
        Err(e) if compute == ComputeBackend::Cuda => {
            return Err(AsrError::Msg(cuda_load_failure_msg(&e)));
        }
        Err(e) => return Err(AsrError::Msg(format!("加载 ASR 失败: {e:#}"))),
    };
    #[cfg(not(feature = "cuda"))]
    let asr = AsrInference::load(&settings.asr_model_dir, compute.to_asr())
        .map_err(|e| AsrError::Msg(format!("加载 ASR 失败: {e:#}")))?;

    let mut transcripts: Vec<ChunkTranscript> = Vec::new();
    #[cfg(debug_assertions)]
    let mut empty_chunks: Vec<usize> = Vec::new();

    for (i, chunk) in chunks.iter().enumerate() {
        on_stage(StageUpdate::with_chunk(
            AsrStage::Transcribing,
            i + 1,
            total_chunks,
        ));

        let chunk_path = if multi_chunk {
            slice_wav(&wav, chunk.start, chunk.end, &chunk_tmp)
                .map_err(|e| AsrError::Msg(e.to_string()))?;
            chunk_tmp.as_path()
        } else {
            wav.as_path()
        };

        let opts = TranscribeOptions::default()
            .with_max_new_tokens(settings.max_new_tokens)
            .with_language(force_lang.clone());

        let path_str = chunk_path
            .to_str()
            .ok_or_else(|| AsrError::Msg("路径非 UTF-8".into()))?;
        let report = asr
            .transcribe(path_str, opts)
            .map_err(|e| AsrError::Msg(format!("转写失败 chunk {}: {e:#}", i + 1)))?;
        let text = clean_asr_text(&report.text);

        if multi_chunk {
            let _ = std::fs::remove_file(&chunk_tmp);
        }

        if text.is_empty() {
            #[cfg(debug_assertions)]
            empty_chunks.push(i);
            continue;
        }

        trace_log(format!(
            "asr#{} {:.1}s chars={}",
            i + 1,
            chunk.end - chunk.start,
            text.chars().count()
        ));
        transcripts.push(ChunkTranscript {
            start_sec: chunk.start as f64,
            end_sec: chunk.end as f64,
            text,
            language: force_lang.clone(),
        });
    }
    drop(asr);
    trace_log("asr dropped");

    if transcripts.is_empty() {
        return Err(AsrError::Msg(format!(
            "ASR 未产生任何有效文本（{total_chunks} 段全部为空）"
        )));
    }
    #[cfg(debug_assertions)]
    if !empty_chunks.is_empty() {
        eprintln!(
            "[asr] skipped empty chunks: {}/{} indices={empty_chunks:?}",
            empty_chunks.len(),
            total_chunks
        );
    }

    // 4. Load Aligner once → all chunks → drop (re-slice from master as needed)
    on_stage(StageUpdate::new(AsrStage::LoadingAligner));
    check_aligner_model_dir(&settings.aligner_model_dir)?;
    let aligner = Qwen3ForcedAligner::load(
        &settings.aligner_model_dir,
        ModelOptions {
            device: compute.to_align_device(),
        },
    )
    .map_err(|e| AsrError::Msg(format!("加载 Aligner 失败: {e:#}")))?;

    let mut all_words: Vec<WordToken> = Vec::new();
    let align_total = transcripts.len();
    for (i, seg) in transcripts.iter().enumerate() {
        on_stage(StageUpdate::with_chunk(
            AsrStage::Aligning,
            i + 1,
            align_total,
        ));

        let seg_dur = (seg.end_sec - seg.start_sec) as f32;
        let seg_chars = seg.text.chars().count();
        trace_log(format!(
            "align#{} begin {:.3}-{:.3} ({:.1}s) chars={}",
            i + 1,
            seg.start_sec,
            seg.end_sec,
            seg_dur,
            seg_chars
        ));

        let mut segment_words = Vec::new();
        if seg_dur < MIN_ALIGN_SEC || !has_alignable_word(&seg.text) {
            // Degenerate chunk (pure punctuation / too short for mel frames):
            // no model call — pin the text to the whole chunk span instead.
            trace_log(format!(
                "align#{} degenerate chunk → whole-span fallback",
                i + 1
            ));
            let word = seg.text.trim();
            if !word.is_empty() {
                segment_words.push(WordToken {
                    start: round_millis(seg.start_sec),
                    end: round_millis(seg.end_sec.max(seg.start_sec)),
                    word: word.to_string(),
                });
            }
        } else {
            let chunk_path = if multi_chunk {
                slice_wav(
                    &wav,
                    seg.start_sec as f32,
                    seg.end_sec as f32,
                    &chunk_tmp,
                )
                .map_err(|e| AsrError::Msg(e.to_string()))?;
                chunk_tmp.as_path()
            } else {
                wav.as_path()
            };

            let result = aligner
                .align(AlignRequest::new(
                    AudioInput::Path(chunk_path.to_path_buf()),
                    TextInput::Text(seg.text.clone()),
                    seg.language.clone(),
                ))
                .map_err(|e| {
                    AsrError::Msg(format!(
                        "对齐失败 chunk {} ({:.1}s, {} chars): {e:#}",
                        i + 1,
                        seg_dur,
                        seg_chars
                    ))
                })?;
            trace_log(format!(
                "align#{} ok words={}",
                i + 1,
                result.items.len()
            ));

            if multi_chunk {
                let _ = std::fs::remove_file(&chunk_tmp);
            }

            for item in result.items {
                let word = item.text.trim();
                if word.is_empty() {
                    continue;
                }
                segment_words.push(WordToken {
                    start: round_millis(seg.start_sec + item.start_time.max(0.0)),
                    end: round_millis(seg.start_sec + item.end_time.max(item.start_time)),
                    word: word.to_string(),
                });
            }
        }

        // Qwen aligner strips punctuation — restore from ASR transcript.
        let restored = attach_transcript_punctuation(&seg.text, &segment_words);
        all_words.extend(restored);
    }
    drop(aligner);

    // 5. Normalize + sentence boundary → atomic SRT → drop scratch
    on_stage(StageUpdate::new(AsrStage::Exporting));
    let words = normalize_word_tokens(all_words);
    if words.is_empty() {
        return Err(AsrError::Msg("对齐后词列表为空".into()));
    }

    let word_dtos: Vec<WordTokenDto> = words
        .iter()
        .map(|w| WordTokenDto {
            start: w.start,
            end: w.end,
            word: w.word.clone(),
        })
        .collect();

    let step2 = build_source_sentences_from_words(SentenceBoundaryRequest {
        task_id: stem.clone(),
        media_path: media_name.to_string(),
        source_lang: source_lang_key.clone(),
        subtitle_length_preset: settings.subtitle_length_preset.clone(),
        words: word_dtos,
        vad_speech_segments: vad_speech,
    })
    .map_err(AsrError::Msg)?;

    let srt_body = source_sentences_to_srt(&step2);
    if srt_body.trim().is_empty() {
        return Err(AsrError::Msg("断句后字幕为空".into()));
    }

    // Primary deliverable first — side exports must not block it.
    write_atomic(&srt_path, &srt_body).map_err(|e| AsrError::Msg(e.to_string()))?;

    if let Some(words_path) = export.words_json.as_ref() {
        match write_words_json(words_path, &stem, media_name, &source_lang_key, &words) {
            Ok(()) => trace_log(format!("words_json={}", words_path.display())),
            Err(e) => {
                // CLI / eval only; product GUI leaves `words_json` unset.
                eprintln!(
                    "warning: words-json write failed (SRT still OK): {} — {e}",
                    words_path.display()
                );
                trace_log(format!(
                    "words_json failed (non-fatal): {} — {e}",
                    words_path.display()
                ));
            }
        }
    }

    // Scratch is ephemeral after a successful SRT write.
    let _ = std::fs::remove_dir_all(&work_dir);

    Ok(srt_path)
}

/// Word/char tokens after ForcedAligner + punct restore + normalize (pre-sentence-boundary).
fn write_words_json(
    path: &Path,
    stem: &str,
    media_name: &str,
    lang: &str,
    words: &[WordToken],
) -> Result<(), AsrError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| AsrError::Msg(e.to_string()))?;
    }
    let items: Vec<serde_json::Value> = words
        .iter()
        .map(|w| {
            serde_json::json!({
                "text": w.word,
                "start": w.start,
                "end": w.end,
            })
        })
        .collect();
    let body = serde_json::json!({
        "id": stem,
        "media": media_name,
        "lang": lang,
        "unit": "aligner_token",
        "note": "ForcedAligner tokens after punctuation restore + normalize_word_tokens; not sentence cues",
        "word_count": items.len(),
        "words": items,
    });
    let text = serde_json::to_string_pretty(&body)
        .map_err(|e| AsrError::Msg(format!("words json: {e}")))?;
    write_atomic(path, &text).map_err(|e| AsrError::Msg(e.to_string()))
}

/// Chunks shorter than this have no usable mel frames to align against.
const MIN_ALIGN_SEC: f32 = 0.1;

/// The Qwen aligner strips punctuation before tokenizing, so a chunk whose
/// text is punctuation-only yields zero words → zero `<timestamp>` slots →
/// the CUDA timestamp gather launches with a 0-sized grid and dies with
/// `CUDA_ERROR_INVALID_VALUE`. Detect such chunks and skip the model call.
fn has_alignable_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

fn attach_transcript_punctuation(transcript_text: &str, aligned_words: &[WordToken]) -> Vec<WordToken> {
    if transcript_text.trim().is_empty() || aligned_words.is_empty() {
        return aligned_words.to_vec();
    }
    let mapped = align_text_to_timestamps(transcript_text, aligned_words);
    if mapped.len() == aligned_words.len() {
        mapped
    } else {
        aligned_words.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::output_srt_path;

    #[test]
    fn output_path_uses_stem() {
        let out = PathBuf::from(r"C:\Install\OneAsr\output");
        let input = PathBuf::from(r"D:\clips\lecture_01.mp4");
        assert_eq!(
            output_srt_path(&out, &media_stem(&input)),
            PathBuf::from(r"C:\Install\OneAsr\output\lecture_01.srt")
        );
    }

    #[test]
    fn check_asr_model_dir_rejects_missing() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr_empty_model_probe_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = check_asr_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("不全") || err.contains("缺少") || err.contains("不存在"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
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
                assert!(p.total_vram > 0);
                assert!(!p.name.is_empty());
            }
            Err(e) => eprintln!("skip probe smoke (no CUDA device): {e}"),
        }
    }

    #[test]
    fn format_process_ms_buckets() {
        assert_eq!(format_process_ms(420), "420 ms");
        assert_eq!(format_process_ms(1500), "1.5 s");
        assert_eq!(format_process_ms(12_400), "12 s");
        assert_eq!(format_process_ms(68_000), "1:08");
        assert_eq!(format_process_ms(3_725_000), "1:02:05");
    }

    #[test]
    fn stage_clock_keeps_same_stage_across_chunks() {
        let mut clock = StageClock::new();
        clock.note(&StageUpdate::new(AsrStage::Converting));
        clock.note(&StageUpdate::with_chunk(AsrStage::Transcribing, 1, 3));
        clock.note(&StageUpdate::with_chunk(AsrStage::Transcribing, 2, 3));
        clock.note(&StageUpdate::with_chunk(AsrStage::Transcribing, 3, 3));
        clock.note(&StageUpdate::new(AsrStage::Exporting));
        let timing = clock.finish();
        assert_eq!(timing.stages.len(), 3);
        assert_eq!(timing.stages[0].stage, AsrStage::Converting);
        assert_eq!(timing.stages[1].stage, AsrStage::Transcribing);
        assert_eq!(timing.stages[2].stage, AsrStage::Exporting);
    }

    #[test]
    fn stage_clock_full_pipeline_order() {
        let mut clock = StageClock::new();
        for stage in [
            AsrStage::Converting,
            AsrStage::Planning,
            AsrStage::LoadingAsr,
            AsrStage::Transcribing,
            AsrStage::LoadingAligner,
            AsrStage::Aligning,
            AsrStage::Exporting,
        ] {
            clock.note(&StageUpdate::new(stage));
        }
        // Multi-chunk progress must not split Transcribing.
        // (already closed above — re-open path via a fresh clock below)
        let timing = clock.finish();
        let stages: Vec<_> = timing.stages.iter().map(|s| s.stage).collect();
        assert_eq!(
            stages,
            vec![
                AsrStage::Converting,
                AsrStage::Planning,
                AsrStage::LoadingAsr,
                AsrStage::Transcribing,
                AsrStage::LoadingAligner,
                AsrStage::Aligning,
                AsrStage::Exporting,
            ]
        );
        assert_ne!(AsrStage::LoadingAsr.label(), AsrStage::LoadingAligner.label());
        assert_eq!(AsrStage::Planning.label(), "分段规划");
        assert!(timing.has_breakdown());
    }

    #[test]
    fn finish_without_notes_is_empty_for_ui() {
        let timing = StageClock::new().finish();
        assert!(timing.stages.is_empty());
        assert!(timing.is_empty());
        assert!(!timing.has_breakdown());
    }

    #[test]
    fn push_or_merge_combines_adjacent_same_stage() {
        // Unit-level: defensive merge used when consecutive closes land on the same stage.
        let mut stages = Vec::new();
        push_or_merge_stage(&mut stages, AsrStage::Transcribing, 10);
        push_or_merge_stage(&mut stages, AsrStage::Transcribing, 5);
        assert_eq!(stages.len(), 1);
        assert_eq!(stages[0].elapsed_ms, 15);
    }
}
