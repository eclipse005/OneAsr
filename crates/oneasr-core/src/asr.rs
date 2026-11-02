//! ASR pipeline: prepare → VAD → transcribe → [align] → export.
//!
//! ```text
//! [optional vocal separation] → 16 kHz master
//! load ASR once → all VAD chunks transcribed → unload ASR
//! load Aligner once → all chunks aligned → unload Aligner      ← 只有需要时间轴时才走
//! word normalize → sentence_boundary → SRT / ASS
//! text → 句末标点断句 → TXT                                   ← 不吃时间轴
//! ```
//!
//! 打轴只在 SRT / ASS（或 CLI 的词级 sidecar）真的要产出时才有消费者；只勾
//! TXT 的任务连对齐模型都不校验。见 [`crate::txt`]。
//!
//! Never keeps ASR and Aligner in VRAM at the same time. Engines come from a
//! [`crate::engine::EngineProvider`], so this module states the policy
//! (staging, engine lifetime, export rules) and knows nothing about concrete
//! model crates — see `crate::engine` for the ports and their adapters.
//!
//! **Scratch lifecycle**: `{data}/runs/{stem}_{ts}/` holds only the full 16 kHz
//! WAV plus at most one temporary chunk file. Product output is
//! `{settings.output_dir}/{stem}.srt` (default `{data}/output`, atomic write).
//! The scratch dir is removed when `ConvertedAudio` drops — success, failure,
//! or panic — unless `ONEASR_KEEP_SCRATCH=1` is set for debugging.

use export::{
    MIN_ALIGN_SEC, attach_transcript_punctuation, has_alignable_word, write_export_file,
    write_words_json,
};

mod export;
mod model_check;

pub use model_check::{
    check_aligner_model_dir, check_asr_model_dir, check_demucs_model_dir,
    check_sensevoice_model_dir, invalidate_all_model_checks, invalidate_model_check,
    is_model_ready,
};

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::diagnostics::trace_log;
use crate::engine::{
    AlignRequest, AlignedToken, EngineProvider, SeparateRequest, SeparationEvent,
    TranscribeRequest, Transcript, degenerate_transcript,
};
use crate::hotword::HotwordRun;
use crate::i18n::{self, UiLang, t};
use crate::lang::{to_lang_key, to_qwen_language_label};
use crate::media::{MediaError, convert_to_16k_mono_wav, slice_wav, wav_duration_sec};
use crate::paths::media_stem;
use crate::settings::Settings;
use crate::subtitle::segmenter::{WordToken, normalize_word_tokens};
use crate::timeline::{self, RenderOptions, Timeline};
use crate::txt;
use crate::vad;

#[derive(Debug)]
pub enum AsrError {
    Io(std::io::Error),
    Media(MediaError),
    EmptyTranscribe(usize),
    EmptyAlignment,
    /// The transcript attached to the task is empty — nothing to align against.
    EmptyTranscript,
    /// Transcript matching needs an output format that can store timecodes.
    TranscriptRequiresTimedOutput,
    EmptySentenceBoundary,
    /// TXT-only export found no text to write. Unlike [`Self::EmptySentenceBoundary`]
    /// this is not about subtitles: the TXT path has no sentence boundary or
    /// timeline, so an empty body means the transcription itself was empty.
    EmptyTxtBody,
    LoadAsr(String),
    LoadAligner(String),
    /// 热词粗转模型（SenseVoiceSmall）加载失败。
    LoadRoughAsr(String),
    TranscribeChunk(usize, String),
    AlignChunk(usize, String, String),
    ModelIncomplete {
        role: i18n::Str,
        missing: String,
        dir: String,
    },
    Other(String),
}

impl From<std::io::Error> for AsrError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<MediaError> for AsrError {
    fn from(e: MediaError) -> Self {
        Self::Media(e)
    }
}

impl fmt::Display for AsrError {
    /// 文案在**展示时**按进程当前语言取词（[`crate::i18n`]）；已在错误里
    /// 定格的底层消息（io::Error、EngineError 等）保持原样。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{}: {e}", t(i18n::ERR_IO)),
            Self::Media(e) => write!(f, "{}: {e}", t(i18n::ERR_MEDIA)),
            Self::EmptyTranscribe(n) => write!(f, "{}", i18n::empty_transcribe(*n)),
            Self::EmptyAlignment => write!(f, "{}", t(i18n::ERR_EMPTY_ALIGNMENT)),
            Self::EmptyTranscript => write!(f, "{}", t(i18n::ERR_EMPTY_TRANSCRIPT)),
            Self::TranscriptRequiresTimedOutput => {
                write!(f, "{}", t(i18n::ERR_TRANSCRIPT_REQUIRES_TIMED_OUTPUT))
            }
            Self::EmptySentenceBoundary => {
                write!(f, "{}", t(i18n::ERR_EMPTY_SENTENCE_BOUNDARY))
            }
            Self::EmptyTxtBody => write!(f, "{}", t(i18n::ERR_EMPTY_TXT_BODY)),
            Self::LoadAsr(e) => write!(f, "{}", i18n::load_asr_failed(e)),
            Self::LoadAligner(e) => write!(f, "{}", i18n::load_aligner_failed(e)),
            Self::LoadRoughAsr(e) => write!(f, "{}", i18n::load_rough_asr_failed(e)),
            Self::TranscribeChunk(i, e) => write!(f, "{}", i18n::transcribe_chunk(*i, e)),
            Self::AlignChunk(i, inner, e) => write!(f, "{}", i18n::align_chunk(*i, inner, e)),
            Self::ModelIncomplete { role, missing, dir } => {
                write!(f, "{}", i18n::model_incomplete(*role, missing, dir))
            }
            Self::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for AsrError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Media(e) => Some(e),
            _ => None,
        }
    }
}

impl AsrError {
    /// Wrap an opaque error with context about which pipeline stage failed.
    pub fn with_stage(self, stage: &str) -> Self {
        match self {
            Self::Io(e) => Self::Other(i18n::stage_io_error(stage, &e)),
            Self::Media(e) => Self::Other(format!("[{stage}] {e}")),
            Self::LoadAsr(e) => Self::LoadAsr(format!("[{stage}] {e}")),
            Self::LoadAligner(e) => Self::LoadAligner(format!("[{stage}] {e}")),
            _ => Self::Other(format!("[{stage}] {self}")),
        }
    }
}

/// Fine-grained pipeline stage for live UI status and timing breakdown.
///
/// Order in a successful run:
/// `Converting` → `Planning` → [`AsrStage::RoughTranscribing`]（只在选了热词组时）
/// → `LoadingAsr` → `Transcribing` → `LoadingAligner` → `Aligning` → `Exporting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrStage {
    /// ffmpeg → 16 kHz mono master WAV.
    Converting,
    /// Optional HTDemucs vocal separation (vocals replace the master WAV).
    Separating,
    /// VAD + chunk plan (or single-chunk plan for short audio).
    Planning,
    /// 热词第一趟：SenseVoice 粗转 + 逐块召回（只有选了热词组时出现）。
    ///
    /// 界面标签叫「热词提取」——这一步的产出就是**从音频里挑出要注入的热词**，
    /// 「提取」说的正是这件事；「粗转」是内部机制名（引擎 trait 也叫
    /// [`crate::engine::RoughAsr`]），别把它抄进界面文案。
    RoughTranscribing,
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
    pub fn label(self, lang: UiLang) -> &'static str {
        match (self, lang) {
            (Self::Converting, UiLang::Zh) => "转码音频",
            (Self::Converting, UiLang::En) => "Converting audio",
            (Self::Separating, UiLang::Zh) => "人声分离",
            (Self::Separating, UiLang::En) => "Separating vocals",
            (Self::Planning, UiLang::Zh) => "分段规划",
            (Self::Planning, UiLang::En) => "Planning segments",
            (Self::RoughTranscribing, UiLang::Zh) => "热词提取",
            (Self::RoughTranscribing, UiLang::En) => "Extracting hotwords",
            (Self::LoadingAsr, UiLang::Zh) => "加载识别模型",
            (Self::LoadingAsr, UiLang::En) => "Loading ASR model",
            (Self::Transcribing, UiLang::Zh) => "转写",
            (Self::Transcribing, UiLang::En) => "Transcribing",
            (Self::LoadingAligner, UiLang::Zh) => "加载对齐模型",
            (Self::LoadingAligner, UiLang::En) => "Loading aligner model",
            (Self::Aligning, UiLang::Zh) => "打轴",
            (Self::Aligning, UiLang::En) => "Aligning",
            (Self::Exporting, UiLang::Zh) => "导出字幕",
            (Self::Exporting, UiLang::En) => "Exporting subtitles",
        }
    }

    /// 机器可读的稳定标识（`--events=json` 的 `key` 字段）。
    ///
    /// 与 [`Self::label`] 分工：label 跟着界面语言变，key 永远是同一个英文蛇形名，
    /// 上层按它做状态机 / 图标映射，不去解析人读文案。**改名等于改接口**——加阶段
    /// 可以，别顺手改已有阶段的名字。
    pub fn key(self) -> &'static str {
        match self {
            Self::Converting => "converting",
            Self::Separating => "separating",
            Self::Planning => "planning",
            Self::RoughTranscribing => "rough_transcribing",
            Self::LoadingAsr => "loading_asr",
            Self::Transcribing => "transcribing",
            Self::LoadingAligner => "loading_aligner",
            Self::Aligning => "aligning",
            Self::Exporting => "exporting",
        }
    }
}

#[derive(Debug, Clone)]
pub struct StageUpdate {
    pub stage: AsrStage,
    pub chunk: Option<(usize, usize)>,
    /// Non-fatal message for the user (e.g.「人声分离已改用 CPU」).
    /// The GUI flashes it in the status bar; the CLI prints it once.
    pub warning: Option<String>,
}

impl StageUpdate {
    pub fn new(stage: AsrStage) -> Self {
        Self {
            stage,
            chunk: None,
            warning: None,
        }
    }

    pub fn with_chunk(stage: AsrStage, current: usize, total: usize) -> Self {
        Self {
            stage,
            chunk: Some((current, total)),
            warning: None,
        }
    }

    /// Attach a user-visible, non-fatal message to this update.
    pub fn with_warning(mut self, message: impl Into<String>) -> Self {
        self.warning = Some(message.into());
        self
    }

    pub fn label(&self, lang: UiLang) -> String {
        match self.chunk {
            Some((cur, total)) if total > 1 => {
                format!("{} {cur}/{total}", self.stage.label(lang))
            }
            _ => self.stage.label(lang).to_string(),
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
    if let Some(last) = stages.last_mut()
        && last.stage == stage
    {
        last.elapsed_ms = last.elapsed_ms.saturating_add(elapsed_ms);
        return;
    }
    stages.push(StageTiming { stage, elapsed_ms });
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

/// One non-empty ASR segment; times are on the global timeline (seconds).
/// Audio is re-sliced from the master 16 kHz WAV on demand — no persistent chunk files.
struct ChunkTranscript {
    start_sec: f64,
    end_sec: f64,
    text: String,
    language: String,
}

/// 复读兜底能切到多细（秒）。再细就失去上下文，不如留着告警。
const MIN_SPLIT_SEC: f32 = 6.0;
/// 对半递归的最大层数：60s 块 → 30s → 15s → 7.5s。
const MAX_SPLIT_DEPTH: u32 = 3;

/// 把一段“解码退化”的音频对半切开重解，必要时再切一层。
///
/// 为什么只能靠换窗口：同一窗口重解是**确定性**的（实测两次 md5 相同），
/// 所以“去掉热词再解一次”对复读无效。而换窗口有效：同一段齐喊音频
/// 整块 60s 只出 11 字，对半切出的两半各出 300–600 字。
///
/// 返回 `None` = 切到最细仍然退化，调用方保留原结果并告警。
fn recover_by_splitting<F>(
    decode: &F,
    start: f32,
    end: f32,
    depth: u32,
) -> Option<Vec<(f32, f32, String)>>
where
    F: Fn(f32, f32) -> Result<Transcript, AsrError>,
{
    if depth == 0 || end - start < MIN_SPLIT_SEC {
        return None;
    }
    let mid = 0.5 * (start + end);
    let mut out = Vec::new();
    for (lo, hi) in [(start, mid), (mid, end)] {
        let report = decode(lo, hi).ok()?;
        if degenerate_transcript(&report) {
            out.extend(recover_by_splitting(decode, lo, hi, depth - 1)?);
        } else {
            let text = clean_asr_text(&report.text);
            if !text.is_empty() {
                out.push((lo, hi, text));
            }
        }
    }
    Some(out)
}

/// Optional side exports from the pipeline (CLI / eval harness).
///
/// The product GUI writes the selected subtitle formats under
/// `settings.output_dir`. CLI/eval harnesses may also request ForcedAligner
/// word timestamps or a reusable measured-timeline sidecar.
#[derive(Debug, Clone, Default)]
pub struct ProcessExportOptions {
    /// Write normalized aligner tokens as JSON (`words: [{text,start,end}, …]`).
    /// Best-effort: errors are logged and do not fail the pipeline after SRT.
    pub words_json: Option<PathBuf>,
    /// Write the measured timeline sidecar. GUI exports omit this; CLI exports
    /// opt in so users can render alternate subtitle layouts later.
    pub timeline_json: bool,
}

/// Full pipeline: convert → VAD plan → ASR all → unload → [align all] → export.
///
/// The align stage runs only when something consumes the timeline — see
/// [`needs_alignment`]. TXT is built from the text alone either way.
///
/// `data_root` 是**数据目录**（`crate::paths::data_dir()`）：本次运行的临时目录
/// 开在它下面。调用方传入而不是在这里重新解析，测试才能塞一个 scratch 目录。
pub fn process_media_file_with_progress<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    data_root: &Path,
    on_stage: impl FnMut(StageUpdate) + Send + 'a,
) -> Result<PathBuf, AsrError> {
    process_media_file_with_export(
        input,
        media_name,
        settings,
        data_root,
        on_stage,
        ProcessExportOptions::default(),
    )
}

/// Same as [`process_media_file_with_progress`] plus optional word-timestamp dump.
pub fn process_media_file_with_export<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    data_root: &Path,
    mut on_stage: impl FnMut(StageUpdate) + Send + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    // 记录最后一个阶段：设备丢失可能落在两次进度更新之间，重跑告警要挂在
    // 用户此刻正看到的阶段上，而不是凭空写一个。
    let last_stage = std::sync::Mutex::new(AsrStage::Converting);
    let mut forward = |update: StageUpdate| {
        *last_stage.lock().unwrap_or_else(|e| e.into_inner()) = update.stage;
        on_stage(update);
    };
    run_local_with_device_loss_retry(settings, &mut forward, &last_stage, |settings, on_stage| {
        let provider = crate::engine::local::LocalEngineProvider::from_settings(settings)
            .map_err(|e| AsrError::Other(e.message().to_string()))?;
        process_media_file_with_provider(
            input,
            media_name,
            settings,
            data_root,
            &provider,
            on_stage,
            export.clone(),
        )
    })
}

/// 本地（真引擎）入口的抗震层：**GPU 设备丢失时，把整次任务改用 CPU 重跑一次**。
///
/// 设备丢失（驱动重置 / TDR / DXGI device removed）在 wgpu 里是 `Device::poll`
/// 的致命错误——引擎内部直接 panic，而那颗设备已经不可恢复：同一进程里换回
/// GPU 继续是做不到的。这里把 panic 接住，换 `backend = cpu` 的 settings 用同一
/// 条管线再跑一遍，并把这件事作为 warning 播给用户：跑了 3/14 的任务不该变成
/// 一条卡死的行。非设备丢失的 panic 原样 `resume_unwind`，交给外层（GUI worker）
/// 既有的 panic 处理，行为不变。
///
/// 只包在**创建本地引擎**的两个入口上；`with_provider` 那条（测试 / 嵌入）不动
/// ——测试替身不碰 GPU，也不该被重跑规则影响。
fn run_local_with_device_loss_retry<T>(
    settings: &Settings,
    on_stage: &mut (dyn FnMut(StageUpdate) + Send),
    last_stage: &std::sync::Mutex<AsrStage>,
    run: impl Fn(&Settings, &mut (dyn FnMut(StageUpdate) + Send)) -> Result<T, AsrError>,
) -> Result<T, AsrError> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(settings, on_stage))) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => {
            if should_retry_on_cpu(&error.to_string(), settings) {
                warn_device_lost_retry(on_stage, last_stage);
                run(&cpu_backend_settings(settings), on_stage)
            } else {
                Err(error)
            }
        }
        Err(payload) => {
            let message = crate::engine::panic_message(&*payload);
            if should_retry_on_cpu(&message, settings) {
                warn_device_lost_retry(on_stage, last_stage);
                run(&cpu_backend_settings(settings), on_stage)
            } else {
                std::panic::resume_unwind(payload)
            }
        }
    }
}

/// 设备丢失且当前不是 CPU——才值得重跑。已是 CPU 还报设备丢失就不是这颗雷。
fn should_retry_on_cpu(message: &str, settings: &Settings) -> bool {
    crate::engine::is_device_lost(message) && !settings.backend.trim().eq_ignore_ascii_case("cpu")
}

fn warn_device_lost_retry(
    on_stage: &mut (dyn FnMut(StageUpdate) + Send),
    last_stage: &std::sync::Mutex<AsrStage>,
) {
    let stage = *last_stage.lock().unwrap_or_else(|e| e.into_inner());
    on_stage(StageUpdate::new(stage).with_warning(crate::i18n::gpu_device_lost_retry_cpu()));
}

/// 重跑用的 settings 副本：只动后端，其余（模型、语言、热词、分块）原样。
fn cpu_backend_settings(settings: &Settings) -> Settings {
    let mut cpu = settings.clone();
    cpu.backend = "cpu".into();
    cpu
}

/// [`process_media_file_with_export`] with an explicit
/// `EngineProvider`.
///
/// Production calls go through the wrapper above (local Qwen / HTDemucs
/// engines); embedding and tests pass their own
/// [`crate::engine::EngineProvider`] — the in-memory doubles live in the
/// `engine::testing` module, which only debug builds ship.
pub fn process_media_file_with_provider<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    data_root: &Path,
    provider: &dyn crate::engine::EngineProvider,
    on_stage: impl FnMut(StageUpdate) + Send + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    Pipeline::new(settings, provider, on_stage).run(input, media_name, data_root, export)
}

/// 文稿匹配任务的输入：文字 + 它的来源文件。
///
/// 两者捆在一起而不是分成两个参数：来源路径只被**输出撞名保护**用到，而两个
/// 平行参数里总有一个是 `None` 而另一个不是——那正是最容易把用户自己的原字幕
/// 盖掉的时候。
#[derive(Debug, Clone, Default)]
pub struct TranscriptInput {
    /// **一行 = 一条字幕**的文稿（见 [`crate::transcript`]）。
    pub text: String,
    /// 原始文件路径。有它才知道输出会不会写到它头上。
    pub path: Option<PathBuf>,
}

/// 文稿匹配：音频 + 文稿 → 字幕，**整个 ASR 阶段被跳过**。
///
/// 与 [`process_media_file_with_export`] 的差别只有一句：文字从哪来。管线其余
/// 部件（转码、人声分离、模型加载、归一化、呈现、产物落盘）全部共用，所以这个
/// 功能没有第二条管线，只是少了一段。
///
/// `RenderOptions.script` 在这条路径上被忽略——用户的字一个都不改。
pub fn process_media_file_with_transcript_export<'a>(
    input: &Path,
    media_name: &str,
    transcript: &TranscriptInput,
    settings: &Settings,
    data_root: &Path,
    mut on_stage: impl FnMut(StageUpdate) + Send + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    let last_stage = std::sync::Mutex::new(AsrStage::Converting);
    let mut forward = |update: StageUpdate| {
        *last_stage.lock().unwrap_or_else(|e| e.into_inner()) = update.stage;
        on_stage(update);
    };
    run_local_with_device_loss_retry(settings, &mut forward, &last_stage, |settings, on_stage| {
        let provider = crate::engine::local::LocalEngineProvider::from_settings(settings)
            .map_err(|e| AsrError::Other(e.message().to_string()))?;
        process_media_file_with_transcript(
            input,
            media_name,
            transcript,
            settings,
            data_root,
            &provider,
            on_stage,
            export.clone(),
        )
    })
}

/// [`process_media_file_with_transcript_export`] with an explicit
/// `EngineProvider` — the shape tests use. The extra argument is the whole point
/// of the seam: the two entries stay drop-in comparable, differing only in where
/// the text comes from.
#[allow(clippy::too_many_arguments)]
pub fn process_media_file_with_transcript<'a>(
    input: &Path,
    media_name: &str,
    transcript: &TranscriptInput,
    settings: &Settings,
    data_root: &Path,
    provider: &dyn crate::engine::EngineProvider,
    on_stage: impl FnMut(StageUpdate) + Send + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    Pipeline::new(settings, provider, on_stage)
        .run_with_transcript(input, media_name, data_root, transcript, export)
}

/// Converted 16 kHz mono PCM WAV + the scratch dir that owns it.
///
/// The scratch dir is removed on drop (success, failure, or panic). Set
/// `ONEASR_KEEP_SCRATCH=1` to keep it for post-mortem debugging.
struct ConvertedAudio {
    work_dir: PathBuf,
    wav_path: PathBuf,
    duration: f32,
    chunk_sec: f32,
}

impl ConvertedAudio {
    fn chunk_tmp(&self) -> PathBuf {
        self.work_dir.join("chunk_tmp.wav")
    }
}

impl Drop for ConvertedAudio {
    fn drop(&mut self) {
        if keep_scratch() {
            return;
        }
        let _ = std::fs::remove_dir_all(&self.work_dir);
    }
}

/// Opt-in scratch retention (`ONEASR_KEEP_SCRATCH=1`); unset/empty/`0` disables.
fn keep_scratch() -> bool {
    std::env::var_os("ONEASR_KEEP_SCRATCH").is_some_and(|v| !v.is_empty() && v != "0")
}

/// 视频转录使用原文件名；文稿匹配固定加 `.aligned`，与文稿路径无关。
fn output_stem(media_stem: &str, transcript: Option<&TranscriptInput>) -> String {
    if transcript.is_some() {
        format!("{media_stem}.aligned")
    } else {
        media_stem.to_string()
    }
}
struct VadPlan {
    chunks: Vec<vad::Chunk>,
    speech_segments: Vec<(f64, f64)>,
    multi_chunk: bool,
}

/// 打轴只在**时间轴真的要被写出去**时才跑。
///
/// TXT 不吃时间轴（见 [`crate::txt`]），所以只勾 TXT 的任务连对齐模型都不用校验：
/// 打轴产物在这条路上没有任何消费者。CLI 的 words-json / timeline sidecar 是例外
/// ——它们要的正是测量结果。
///
/// **公开**是因为它同时是「这一趟会不会有加载对齐模型 / 打轴两个阶段」的唯一判据：
/// CLI 的 `--events=json` 阶段编号按它算，命令行不再重抄一遍条件。
pub fn needs_alignment(settings: &Settings, export: &ProcessExportOptions) -> bool {
    settings.output_srt
        || settings.output_ass
        || export.words_json.is_some()
        || export.timeline_json
}

/// Completed ASR transcription result.
struct AsrOutput {
    transcripts: Vec<ChunkTranscript>,
}

/// Completed ForcedAligner result.
struct AlignOutput {
    words: Vec<WordToken>,
}

/// 呈现阶段需要的输入。与 [`AlignOutput`] 分开是因为**TXT 完全不看它**——
/// 那部分输入在只勾 TXT 时甚至不需要生成。
struct ExportInput<'a> {
    input: &'a Path,
    media_name: &'a str,
    speech_segments: Vec<(f64, f64)>,
    transcript: Option<&'a TranscriptInput>,
    /// TXT 正文，已按句末标点断好句（见 [`crate::txt`]）。与字幕格式无关。
    txt_body: Option<String>,
}

/// 线程安全的进度出口。
///
/// 不是 `RefCell`：引擎**可能从它自己的线程**回调打轴进度（CTC 的编码线程 /
/// DP worker 就是这样），所以出口必须是 `Send + Sync`，否则连"把进度转发出去"
///这件事都做不到。
///
/// 有了它，打轴阶段的 sink 闭包只需要捕获 `&self.on_stage` 一个字段——那是
/// `Sync` 的，于是闭包本身是 `Send`，可以被引擎从任何线程调用；`Pipeline`
/// 其余部分（`provider` 是 `!Sync` 的 trait object）完全不必变成 `Sync`。
///
/// 锁中毒时把守卫取回来继续，而不是让别人的 panic 带走整次运行。
struct ProgressSink<'a> {
    inner: std::sync::Mutex<Box<dyn FnMut(StageUpdate) + Send + 'a>>,
}

impl<'a> ProgressSink<'a> {
    fn new(f: impl FnMut(StageUpdate) + Send + 'a) -> Self {
        Self {
            inner: std::sync::Mutex::new(Box::new(f)),
        }
    }

    fn send(&self, update: StageUpdate) {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard(update);
    }
}

/// Convert → VAD → ASR → align → SRT. ASR is dropped before the aligner loads.
struct Pipeline<'a> {
    settings: &'a Settings,
    provider: &'a dyn EngineProvider,
    on_stage: ProgressSink<'a>,
}

impl<'a> Pipeline<'a> {
    fn new(
        settings: &'a Settings,
        provider: &'a dyn EngineProvider,
        on_stage: impl FnMut(StageUpdate) + Send + 'a,
    ) -> Self {
        Self {
            settings,
            provider,
            on_stage: ProgressSink::new(on_stage),
        }
    }

    fn emit(&self, update: StageUpdate) {
        self.on_stage.send(update);
    }

    fn run(
        self,
        input: &Path,
        media_name: &str,
        data_root: &Path,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        // 需要对齐产物的运行先确认模型在：转码、VAD、ASR 都可能很贵，缺模型
        // 不该等它们跑完才报。TXT-only（无字幕、无侧车）不经过这里，所以它
        // 仍然可以不装对齐器。
        if needs_alignment(self.settings, &export) {
            check_aligner_model_dir(&self.settings.aligner_model_dir)?;
        }
        // 热词粗转同样在昂贵阶段之前确认模型在位：缺模型不该等转码 / VAD 跑完才报。
        //
        // 判据是 `is_usable()` 而不是「有没有选组」：组已被删除时
        // `resolve_run` 返回 `None`，这里直接跳过——那一行本来就显示「无热词」，
        // 不该反过来拿一个用不到的粗转模型把任务挡在门外。
        //
        // 语言门禁同源：非中英的转录压根不接热词（见 `stage_rough_transcribe`），
        // 选了组也不该被粗转模型挡住。
        if crate::lang::supports_hotwords(&self.settings.language)
            && self
                .settings
                .hotword_run
                .as_ref()
                .is_some_and(HotwordRun::is_usable)
        {
            check_sensevoice_model_dir(&self.settings.resolved_sensevoice_model_dir())?;
        }
        let conv = self.stage_prepare(input, data_root)?;
        let vad = self.stage_vad_plan(&conv)?;
        // 热词第一趟：SenseVoice 粗转 → 逐块召回 → 拼成 Qwen 的 context。
        // 它在正式 ASR **之前**跑完并释放模型，两个模型不会同时占显存。
        let hotword_contexts = self.stage_rough_transcribe(&conv, &vad)?;
        let asr = self.stage_transcribe_all(&conv, &vad, hotword_contexts.as_deref())?;
        let txt_body = self.txt_body(&asr);
        if !needs_alignment(self.settings, &export) {
            return self.stage_export_txt_only(input, media_name, txt_body);
        }
        let align = self.stage_align_all(&conv, &vad, &asr)?;
        self.stage_export(
            ExportInput {
                input,
                media_name,
                speech_segments: vad.speech_segments,
                transcript: None,
                txt_body,
            },
            align,
            export,
        )
    }

    /// TXT 正文：**纯句末标点断句**，与打轴无关（见 [`crate::txt`]）。
    fn txt_body(&self, asr: &AsrOutput) -> Option<String> {
        if !self.settings.output_txt {
            return None;
        }
        Some(txt::body_from_chunks(
            asr.transcripts.iter().map(|t| t.text.as_str()),
            self.settings
                .text_script_for(&to_lang_key(&self.settings.language))
                .unwrap_or(crate::text_script::TextScript::Original),
        ))
    }

    /// TXT-only 的出口：不加载对齐器、不建时间轴，只把断好句的正文落盘。
    fn stage_export_txt_only(
        &self,
        input: &Path,
        media_name: &str,
        txt_body: Option<String>,
    ) -> Result<PathBuf, AsrError> {
        self.emit(StageUpdate::new(AsrStage::Exporting));
        let body = txt_body.ok_or_else(|| AsrError::Other(t(i18n::ERR_NO_OUTPUT_FORMAT).into()))?;
        if body.trim().is_empty() {
            // 这里没有「断句」可怪：TXT 走的是纯文本路，空就是转写本身没有字。
            return Err(AsrError::EmptyTxtBody);
        }
        let fallback_dir = self.settings.resolved_output_dir();
        let target_dir = self.settings.srt_target_dir(input);
        let stem = media_stem(Path::new(media_name));
        write_export_file(&target_dir, &fallback_dir, &stem, "txt", &body)
    }
    /// 文稿匹配：转码 → [人声分离] → **打轴整段文稿** → 呈现。
    ///
    /// 跳过 VAD 与 ASR，两条理由都不是为了快：VAD 的产物是「静音处硬切」和排版
    /// DP 的间隔代价，而文稿已经给了边界，两个消费者都不需要；ASR 整个不需要
    /// ——文字是用户给的。
    ///
    /// 所以这条路径是现有管线**唯一一处会让对齐器见到整段长音频**的地方：识别
    /// 路径的对齐是逐 ASR 段调用、段长受分段时长约束在 30–180 s，从来没有超过三
    /// 分钟。文稿没有 ASR 来定义段，段就只能是「整段」。
    fn run_with_transcript(
        &self,
        input: &Path,
        media_name: &str,
        data_root: &Path,
        transcript: &TranscriptInput,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        if !self.settings.output_srt && !self.settings.output_ass {
            return Err(AsrError::TranscriptRequiresTimedOutput);
        }
        if transcript.text.trim().is_empty() {
            return Err(AsrError::EmptyTranscript);
        }
        // 文稿路径永远要打轴，模型校验同样提前到转码之前；
        // `stage_align_transcript` 里的原检查保留作防御。
        check_aligner_model_dir(&self.settings.aligner_model_dir)?;
        let conv = self.stage_prepare(input, data_root)?;
        let align = self.stage_align_transcript(&conv, &transcript.text)?;
        // TXT 同样是纯标点断句。文稿是用户自己写的，所以不做简繁转换——
        // 与 SRT/ASS 那条「机器不碰你的字」的口径一致。
        let txt_body = self
            .settings
            .output_txt
            .then(|| txt::body(&transcript.text));
        // 文稿模式没有 VAD：speech segments 只喂给排版 DP 算间隔，而文稿分行
        // 根本不跑那个 DP。
        self.stage_export(
            ExportInput {
                input,
                media_name,
                speech_segments: Vec::new(),
                transcript: Some(transcript),
                txt_body,
            },
            align,
            export,
        )
    }

    // ── Stage 1: optional vocal separation → 16 kHz mono master ─────────

    /// Build the master WAV the rest of the pipeline consumes.
    ///
    /// With separation on, the input is decoded **once** at 44.1 kHz for
    /// HTDemucs and the vocals are transcoded to the 16 kHz master; without it
    /// the input is transcoded directly. Either way exactly one full decode
    /// feeds the run.
    ///
    /// The scratch dir is owned by [`ConvertedAudio`] from before the first
    /// decode, so every early return (and panic) cleans up through `Drop`.
    fn stage_prepare(&self, input: &Path, data_root: &Path) -> Result<ConvertedAudio, AsrError> {
        let stem = media_stem(input);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let work_dir = data_root
            .join(crate::paths::RUNS_DIR)
            .join(format!("{stem}_{stamp}"));
        std::fs::create_dir_all(&work_dir)?;

        let mut conv = ConvertedAudio {
            work_dir,
            wav_path: PathBuf::new(),
            duration: 0.0,
            chunk_sec: self.settings.chunk_target_seconds_clamped() as f32,
        };

        let master_source = if self.settings.vocal_separation {
            self.stage_separate_vocals(input, &conv.work_dir)?
        } else {
            input.to_path_buf()
        };

        self.emit(StageUpdate::new(AsrStage::Converting));
        let wav_path = conv.work_dir.join("input_16k.wav");
        convert_to_16k_mono_wav(&master_source, &wav_path)?;
        conv.duration = wav_duration_sec(&wav_path)
            .ok_or_else(|| AsrError::Other(t(i18n::ERR_READ_DURATION).into()))?
            as f32;
        conv.wav_path = wav_path;
        trace_log(format!(
            "converted: {:.3}s mono 16 kHz; work_dir={} keep_scratch={}",
            conv.duration,
            conv.work_dir.display(),
            keep_scratch(),
        ));
        Ok(conv)
    }

    /// Separate the vocals stem (HTDemucs v4, wgpu/CPU) and return the vocals
    /// WAV. Runs before VAD so chunk planning and ASR both see the
    /// music-suppressed audio.
    fn stage_separate_vocals(&self, input: &Path, work_dir: &Path) -> Result<PathBuf, AsrError> {
        self.emit(StageUpdate::new(AsrStage::Separating));
        let model_dir = self.settings.resolved_demucs_model_dir();
        let separator = self
            .provider
            .load_separator()
            .map_err(|e| AsrError::Other(e.message().to_string()))?;
        let vocals = separator
            .separate(
                SeparateRequest {
                    input,
                    out_dir: work_dir,
                },
                // Chunk-level progress → 「人声分离 n/N」, backend fallbacks →
                // a status-bar warning; both ride the existing stage channel.
                &mut |event| match event {
                    SeparationEvent::Progress { done, total } => {
                        self.emit(StageUpdate::with_chunk(AsrStage::Separating, done, total));
                    }
                    SeparationEvent::FellBackToCpu { reason } => {
                        self.emit(
                            StageUpdate::new(AsrStage::Separating)
                                .with_warning(i18n::sep_gpu_fallback(&reason)),
                        );
                    }
                },
            )
            .map_err(|e| AsrError::Other(e.message().to_string()))?;

        trace_log(format!(
            "vocal separation: vocals stem ready ({})",
            model_dir.display()
        ));
        Ok(vocals)
    }

    // ── Stage 2: VAD speech segmentation + chunk plan ───────────────────

    /// Run FireRedVAD and derive silence-cut chunk boundaries.
    fn stage_vad_plan(&self, conv: &ConvertedAudio) -> Result<VadPlan, AsrError> {
        self.emit(StageUpdate::new(AsrStage::Planning));

        let (chunks, speech_segments) = if conv.duration > conv.chunk_sec {
            match vad::run_vad(&conv.wav_path) {
                Ok(speech) => {
                    let silences =
                        vad::speech_to_silences(&speech, conv.duration, vad::MIN_SILENCE_MICRO);
                    let planned = vad::plan_chunks(conv.duration, &silences, conv.chunk_sec);
                    trace_log(format!(
                        "vad: duration={:.1}s speech={} silence_gaps={} chunks={}",
                        conv.duration,
                        speech.len(),
                        silences.len(),
                        planned.len(),
                    ));
                    let pairs: Vec<(f64, f64)> =
                        speech.iter().map(|&(s, e)| (s as f64, e as f64)).collect();
                    (planned, pairs)
                }
                Err(e) => {
                    // VAD improves cut placement but is not a hard requirement:
                    // fall back to fixed-duration cuts so a VAD failure does
                    // not abort the whole job (boundary scoring then degrades
                    // to punctuation + length budget).
                    trace_log(format!(
                        "VAD failed, falling back to {:.0}s fixed chunks: {e}",
                        conv.chunk_sec
                    ));
                    (
                        vad::plan_chunks(conv.duration, &[], conv.chunk_sec),
                        Vec::new(),
                    )
                }
            }
        } else {
            (
                vec![vad::Chunk {
                    start: 0.0,
                    end: conv.duration,
                }],
                vec![(0.0, conv.duration as f64)],
            )
        };

        let multi_chunk = chunks.len() > 1;

        trace_log(format!(
            "plan: duration={:.1}s chunk_target={:.0}s chunks={}",
            conv.duration,
            conv.chunk_sec,
            chunks.len(),
        ));
        for (i, c) in chunks.iter().enumerate() {
            trace_log(format!(
                "plan#{i} {:.3}-{:.3} ({:.1}s)",
                c.start,
                c.end,
                c.end - c.start
            ));
        }

        Ok(VadPlan {
            chunks,
            speech_segments,
            multi_chunk,
        })
    }

    // ── Stage 3: load ASR + transcribe all chunks ───────────────────────

    /// 热词第一趟：SenseVoice 粗转 + 逐块召回，返回**与 `vad.chunks` 一一对应**
    /// 的 context 串；`None` = 本次不用热词。
    ///
    /// 两趟共用同一批分块，召回出来的词才不会跨块错位；粗转引擎用完即弃
    /// （`drop`），所以它不会和 Qwen 同时驻留显存。召回是**召回**不是纠错：
    /// 阈值与 `top_k` 都偏松，多给几条不致命，而漏掉一条就是白跑一趟。
    fn stage_rough_transcribe(
        &self,
        conv: &ConvertedAudio,
        vad: &VadPlan,
    ) -> Result<Option<Vec<String>>, AsrError> {
        // 语言门禁：热词只接中文与英文（见 [`crate::lang::supports_hotwords`]）。
        // 其他语言的任务**整趟不跑**——粗转模型不加载、context 不注入。
        // 这道闸放在这里而不是只放 `can_start`：库调用方与测试都能绕过开跑检查
        // 直接进管线，而「没有消费者」的粗转是纯浪费（日语素材实测白跑一趟）。
        if !crate::lang::supports_hotwords(&self.settings.language) {
            return Ok(None);
        }
        // 只认**已经解析好**的运行参数：这里没有组 id 可查，也就没有「组被删了
        // 却仍按热词去要求模型」的那类悬空状态。
        let Some(run) = self
            .settings
            .hotword_run
            .as_ref()
            .filter(|run| run.is_usable())
        else {
            return Ok(None);
        };
        let index = crate::hotword::HotwordIndex::new(&run.text, run.threshold);
        if index.is_empty() {
            // 词表里一个能音素化的词都没有（全是标点之类）：没有可注入的提示。
            return Ok(None);
        }
        self.emit(StageUpdate::new(AsrStage::RoughTranscribing));
        let mut rough = self
            .provider
            .load_rough_asr()
            .map_err(|e| AsrError::LoadRoughAsr(e.message().to_string()))?;
        let chunks: Vec<(f32, f32)> = vad
            .chunks
            .iter()
            .map(|chunk| (chunk.start, chunk.end))
            .collect();
        let texts = rough
            .transcribe_chunks(&conv.wav_path, &chunks)
            .map_err(|e| AsrError::Other(e.message().to_string()))?;
        drop(rough);

        let top_k = run.top_k;
        let contexts: Vec<String> = texts
            .iter()
            .map(|text| {
                let words: Vec<String> = index
                    .recall(text, top_k)
                    .into_iter()
                    .map(|(word, _score)| word)
                    .collect();
                crate::hotword::context_line(&words)
            })
            .collect();
        Ok(Some(contexts))
    }

    fn stage_transcribe_all(
        &self,
        conv: &ConvertedAudio,
        vad: &VadPlan,
        hotword_contexts: Option<&[String]>,
    ) -> Result<AsrOutput, AsrError> {
        self.emit(StageUpdate::new(AsrStage::LoadingAsr));
        check_asr_model_dir(&self.settings.asr_model_dir)?;
        // The provider owns backend resolution + the GPU→CPU *load* fallback;
        // a failure in the middle of a run is never retried (see `engine`).
        let load_start = std::time::Instant::now();
        let asr = self
            .provider
            .load_asr()
            .map_err(|e| AsrError::LoadAsr(e.message().to_string()))?;
        trace_log(format!(
            "asr model loaded in {} ms",
            load_start.elapsed().as_millis()
        ));
        let force_lang = to_qwen_language_label(&self.settings.language);
        let chunk_tmp = conv.chunk_tmp();

        let mut transcripts = Vec::new();
        let mut empty_chunks = Vec::new();

        for (i, chunk) in vad.chunks.iter().enumerate() {
            // Logged *before* the progress update: the worker drains this ring
            // inside its on_stage callback, so the chunk span and WAV path hit
            // the journal before any GPU work can hang the thread.
            let wav = if vad.multi_chunk {
                chunk_tmp.as_path()
            } else {
                conv.wav_path.as_path()
            };
            trace_log(format!(
                "asr chunk {}/{} span {:.3}-{:.3}s ({:.1}s) wav={}",
                i + 1,
                vad.chunks.len().max(1),
                chunk.start,
                chunk.end,
                chunk.end - chunk.start,
                wav.display(),
            ));
            self.emit(StageUpdate::with_chunk(
                AsrStage::Transcribing,
                i + 1,
                vad.chunks.len().max(1),
            ));

            let chunk_path = if vad.multi_chunk {
                slice_wav(&conv.wav_path, chunk.start, chunk.end, &chunk_tmp)?;
                chunk_tmp.as_path()
            } else {
                conv.wav_path.as_path()
            };

            // 热词提示词逐块给：这一段的召回结果只对这一段有效（位置越靠前越有效，
            // 所以顺序就是 `recall` 给的降序）。
            let context = hotword_contexts
                .and_then(|all| all.get(i))
                .map(String::as_str);
            // 带 context 的**非空**提示才算「这一趟真的注入了热词」：空串在适配层
            // 就被过滤掉（等于没给），去掉 context 重试不会有任何不同。
            let had_context = context.is_some_and(|c| !c.is_empty());
            let mut report = asr
                .transcribe(TranscribeRequest {
                    wav: chunk_path,
                    language: &force_lang,
                    max_new_tokens: self.settings.max_new_tokens,
                    context,
                })
                .map_err(|e| AsrError::TranscribeChunk(i + 1, e.message().to_string()))?;

            // 自愈：带热词提示的解码退化（撞顶截断 / 复读被折叠）时，去掉 context
            // 重试一次。提示词是**建议**，它引发的跑飞不该让这一段的内容静默丢失。
            // 只重试一次、且只在重试干净时替换；救不回来就保留原结果——这个守卫
            // 的最坏情况等价于今天的行为（多花一次解码）。
            if had_context && degenerate_transcript(&report) {
                trace_log(format!(
                    "asr#{} degenerate with context (stop={:?} raw={} chars) → retry without it",
                    i + 1,
                    report.stop_reason,
                    report.raw_output.chars().count(),
                ));
                match asr.transcribe(TranscribeRequest {
                    wav: chunk_path,
                    language: &force_lang,
                    max_new_tokens: self.settings.max_new_tokens,
                    context: None,
                }) {
                    Ok(retry) if !degenerate_transcript(&retry) => {
                        trace_log(format!("asr#{} recovered without context", i + 1));
                        report = retry;
                    }
                    Ok(_) => trace_log(format!(
                        "asr#{} still degenerate without context; keeping the hotword result",
                        i + 1
                    )),
                    Err(e) => trace_log(format!(
                        "asr#{} retry failed ({}); keeping the hotword result",
                        i + 1,
                        e.message()
                    )),
                }
            }
            let text = clean_asr_text(&report.text);

            // 复读兜底：退化解码（撞顶 / 复读被折叠）就换窗口重解。
            // 判据只管“是否退化了”，不与热词挂钩——日语从来没有热词 context，
            // 以前那双重的 `had_context &&` 把它们全放过去了（一整块台词静默丢失）。
            // 单块（短视频）同样走这条路：`slice_wav` 对它也成立。
            if degenerate_transcript(&report) {
                let decode_span = |lo: f32, hi: f32| -> Result<Transcript, AsrError> {
                    slice_wav(&conv.wav_path, lo, hi, &chunk_tmp)?;
                    asr.transcribe(TranscribeRequest {
                        wav: chunk_tmp.as_path(),
                        language: &force_lang,
                        max_new_tokens: self.settings.max_new_tokens,
                        context: None,
                    })
                    .map_err(|e| AsrError::TranscribeChunk(i + 1, e.message().to_string()))
                };
                if let Some(parts) =
                    recover_by_splitting(&decode_span, chunk.start, chunk.end, MAX_SPLIT_DEPTH)
                {
                    trace_log(format!(
                        "asr#{} degenerate (stop={:?}, {} chars) → recovered as {} piece(s)",
                        i + 1,
                        report.stop_reason,
                        text.chars().count(),
                        parts.len(),
                    ));
                    for (lo, hi, piece) in parts {
                        transcripts.push(ChunkTranscript {
                            start_sec: lo as f64,
                            end_sec: hi as f64,
                            text: piece,
                            language: force_lang.clone(),
                        });
                    }
                    continue;
                }
                trace_log(format!(
                    "asr#{} degenerate and unrecoverable (stop={:?}, {} chars)",
                    i + 1,
                    report.stop_reason,
                    text.chars().count(),
                ));
                self.emit(
                    StageUpdate::with_chunk(AsrStage::Transcribing, i + 1, vad.chunks.len().max(1))
                        .with_warning(i18n::asr_span_incomplete(
                            chunk.start as f64,
                            chunk.end as f64,
                        )),
                );
            }

            // 单块（短视频）也可能因为兜底切出临时文件，无条件删即可。
            let _ = std::fs::remove_file(&chunk_tmp);

            if text.is_empty() {
                empty_chunks.push(i);
                continue;
            }

            trace_log(format!(
                "asr#{} {:.3}-{:.3}s ({:.1}s) chars={} stop={:?}",
                i + 1,
                chunk.start,
                chunk.end,
                chunk.end - chunk.start,
                text.chars().count(),
                report.stop_reason
            ));
            transcripts.push(ChunkTranscript {
                start_sec: chunk.start as f64,
                end_sec: chunk.end as f64,
                text,
                language: force_lang.clone(),
            });
        }

        if transcripts.is_empty() {
            return Err(AsrError::EmptyTranscribe(vad.chunks.len().max(1)));
        }
        if !empty_chunks.is_empty() {
            trace_log(format!(
                "asr skipped empty chunks: {}/{} indices={empty_chunks:?}",
                empty_chunks.len(),
                vad.chunks.len().max(1),
            ));
        }

        Ok(AsrOutput { transcripts })
    }

    // ── Stage 4: load Aligner + align all transcripts ───────────────────

    fn stage_align_all(
        &self,
        conv: &ConvertedAudio,
        vad: &VadPlan,
        asr: &AsrOutput,
    ) -> Result<AlignOutput, AsrError> {
        self.emit(StageUpdate::new(AsrStage::LoadingAligner));
        check_aligner_model_dir(&self.settings.aligner_model_dir)?;
        let load_start = std::time::Instant::now();
        let aligner = self
            .provider
            .load_aligner()
            .map_err(|e| AsrError::LoadAligner(e.message().to_string()))?;
        trace_log(format!(
            "aligner model loaded in {} ms",
            load_start.elapsed().as_millis()
        ));

        let mut all_words: Vec<WordToken> = Vec::new();
        for (i, seg) in asr.transcripts.iter().enumerate() {
            self.emit(StageUpdate::with_chunk(
                AsrStage::Aligning,
                i + 1,
                asr.transcripts.len(),
            ));

            let seg_dur = (seg.end_sec - seg.start_sec) as f32;
            let seg_chars = seg.text.chars().count();
            trace_log(format!(
                "align#{} begin {:.3}-{:.3} ({:.1}s) chars={}",
                i + 1,
                seg.start_sec,
                seg.end_sec,
                seg_dur,
                seg_chars,
            ));

            let mut segment_words = Vec::new();
            if seg_dur < MIN_ALIGN_SEC || !has_alignable_word(&seg.text) {
                trace_log(format!(
                    "align#{} degenerate chunk → whole-span fallback",
                    i + 1
                ));
                let word = seg.text.trim();
                if !word.is_empty() {
                    segment_words.push(timeline::place_whole(word, seg.start_sec, seg.end_sec));
                }
            } else {
                let chunk_tmp = conv.chunk_tmp();
                let chunk_path = if vad.multi_chunk {
                    slice_wav(
                        &conv.wav_path,
                        seg.start_sec as f32,
                        seg.end_sec as f32,
                        &chunk_tmp,
                    )?;
                    chunk_tmp.as_path()
                } else {
                    conv.wav_path.as_path()
                };

                let result = aligner
                    .align(
                        AlignRequest {
                            wav: chunk_path,
                            text: &seg.text,
                            language: &seg.language,
                        },
                        // 这里循环本身已经是分母：第 i 段 / 共 n 段。引擎内部再报
                        // 一层窗口进度会和它重叠，所以传 `None`。
                        None,
                    )
                    .map_err(|e| {
                        AsrError::AlignChunk(
                            i + 1,
                            format!("{seg_dur:.1}s"),
                            e.message().to_string(),
                        )
                    })?;
                trace_log(format!("align#{} ok words={}", i + 1, result.len()));

                if vad.multi_chunk {
                    let _ = std::fs::remove_file(chunk_path);
                }

                for AlignedToken {
                    text,
                    start_sec,
                    end_sec,
                } in result
                {
                    let word = text.trim();
                    if word.is_empty() {
                        continue;
                    }
                    segment_words.push(timeline::place_aligned(
                        word,
                        seg.start_sec,
                        start_sec,
                        end_sec,
                    ));
                }
            }

            // Qwen 对齐器剥离标点 —— 从 ASR 转写把标点贴回词上；CTC 对齐器的
            // token 原生携带标点（零时长搭在前一字符上），直通即可。
            let restored = if self.settings.aligner_strips_punctuation() {
                attach_transcript_punctuation(&seg.text, &segment_words)
            } else {
                segment_words
            };
            all_words.extend(restored);
        }

        Ok(AlignOutput { words: all_words })
    }

    /// 文稿路径的对齐阶段：一次调用，整段文稿对整段音频。
    ///
    /// 没有块偏移可言——对齐器拿到的就是文件本身，所以 `chunk_start` 是 0。
    /// 阶段标签只发「加载对齐模型 / 对齐中」两个：**没有「转写」**，
    /// 这就是用户在处理中判断「这次是文稿匹配」的全部依据。
    fn stage_align_transcript(
        &self,
        conv: &ConvertedAudio,
        transcript: &str,
    ) -> Result<AlignOutput, AsrError> {
        self.emit(StageUpdate::new(AsrStage::LoadingAligner));
        check_aligner_model_dir(&self.settings.aligner_model_dir)?;
        let aligner = self
            .provider
            .load_aligner()
            .map_err(|e| AsrError::LoadAligner(e.message().to_string()))?;

        self.emit(StageUpdate::new(AsrStage::Aligning));
        // 进度是真的：分母由对齐器自己数——CTC 现在报的是**整条 run** 的刻度
        // （编码窗口 + Viterbi + 回溯 + 分数回放 + 时间线），所以 100% 意味着
        // 最后一步也做完了，而不是"窗口都提交上去了"（那会儿活还挂在设备上）。
        // 用户给一份 21 分钟的文稿要对着一个不动的「打轴」等二十几秒，
        // 那二十几秒里唯一能告诉他「还在动」的东西就是这条进度。
        //
        // 只捕获 `self.on_stage` 这一个字段：它是 `Sync` 的，所以这个闭包是
        // `Send` 的，能被对齐器从它自己的线程调用；`Pipeline` 其余部分
        // （`provider` 是 `!Sync` 的 trait object）因此不必变成 `Sync`。
        let progress = &self.on_stage;
        let mut sink = |done: usize, total: usize| {
            progress.send(StageUpdate::with_chunk(AsrStage::Aligning, done, total));
        };
        let result = aligner
            .align(
                AlignRequest {
                    wav: conv.wav_path.as_path(),
                    text: transcript,
                    language: &self.settings.language,
                },
                Some(&mut sink),
            )
            .map_err(|e| {
                AsrError::AlignChunk(1, format!("{:.1}s", conv.duration), e.message().to_string())
            })?;
        trace_log(format!("align#transcript ok words={}", result.len()));

        let words: Vec<WordToken> = result
            .into_iter()
            .filter_map(
                |AlignedToken {
                     text,
                     start_sec,
                     end_sec,
                 }| {
                    let word = text.trim();
                    (!word.is_empty())
                        .then(|| timeline::place_aligned(word, 0.0, start_sec, end_sec))
                },
            )
            .collect();
        if words.is_empty() {
            return Err(AsrError::EmptyAlignment);
        }
        Ok(AlignOutput { words })
    }

    // ── Stage 5: sentence boundary + SRT write + side exports ────────────

    /// Normalize tokens, hand the measured timeline to the presentation phase,
    /// and write the selected subtitle formats plus requested side exports.
    ///
    /// The two phases meet here and nowhere else: everything before this point
    /// *measured* (two models, once), everything [`timeline::render`] does after
    /// it *presents* (pure, re-runnable). Writing the timeline next to the
    /// subtitle is what makes that split usable for CLI re-rendering; GUI runs
    /// keep the timeline in memory and do not leave a sidecar behind.
    fn stage_export(
        &self,
        src: ExportInput<'_>,
        align: AlignOutput,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        let ExportInput {
            input,
            media_name,
            speech_segments,
            transcript,
            txt_body,
        } = src;
        let stem = media_stem(Path::new(media_name));
        self.emit(StageUpdate::new(AsrStage::Exporting));

        let words = normalize_word_tokens(align.words);
        if words.is_empty() {
            return Err(AsrError::EmptyAlignment);
        }

        let source_lang_key = to_lang_key(&self.settings.language);
        let measured = Timeline::from_words(
            media_name,
            source_lang_key.clone(),
            self.settings.subtitle_length_preset.clone(),
            speech_segments,
            words.clone(),
        );
        let rendered = timeline::render(
            &measured,
            &RenderOptions {
                preset: None,
                // A transcription run has no transcript: the models produced the
                // text, so the layout DP owns the line breaks. With one, its
                // lines are the cues and `script` is ignored — the transcript
                // is the user's, and we do not rewrite it.
                transcript: transcript.map(|t| t.text.clone()),
                // Chinese output script (zh / yue only). Timing fields are
                // untouched: conversion rewrites cue text only, after alignment
                // and segmentation.
                script: self
                    .settings
                    .text_script_for(&source_lang_key)
                    .unwrap_or(crate::text_script::TextScript::Original),
                srt: self.settings.output_srt,
                ass: self.settings.output_ass,
                beautify: self.settings.subtitle_beautify,
            },
        )
        .map_err(AsrError::Other)?;
        let srt_body = rendered.srt;
        let ass_body = rendered.ass;
        if srt_body.as_deref().is_some_and(|b| b.trim().is_empty())
            || ass_body.as_deref().is_some_and(|b| b.trim().is_empty())
        {
            return Err(AsrError::EmptySentenceBoundary);
        }
        // TXT 不过断句-字幕那条链：「断句后字幕为空」对它是错的说法。
        if txt_body.as_deref().is_some_and(|b| b.trim().is_empty()) {
            return Err(AsrError::EmptyTxtBody);
        }

        // Primary target: next to the source media when enabled; fall back to
        // the configured output dir when that write fails (permissions, …).
        let fallback_dir = self.settings.resolved_output_dir();
        let target_dir = self.settings.srt_target_dir(input);

        let stem = output_stem(&stem, transcript);

        // SRT first: it stays the primary output (task row “打开” target).
        let mut primary: Option<PathBuf> = None;
        if let Some(body) = srt_body.as_deref() {
            primary = Some(write_export_file(
                &target_dir,
                &fallback_dir,
                &stem,
                "srt",
                body,
            )?);
        }
        if let Some(body) = txt_body.as_deref() {
            let path = write_export_file(&target_dir, &fallback_dir, &stem, "txt", body)?;
            if primary.is_none() {
                primary = Some(path);
            }
        }
        if let Some(body) = ass_body.as_deref() {
            let path = write_export_file(&target_dir, &fallback_dir, &stem, "ass", body)?;
            if primary.is_none() {
                primary = Some(path);
            }
        }
        let primary =
            primary.ok_or_else(|| AsrError::Other(t(i18n::ERR_NO_OUTPUT_FORMAT).into()))?;

        if export.timeline_json {
            // Optional CLI sidecar for re-rendering measured timestamps with
            // another presentation preset. It is never needed by GUI users.
            match serde_json::to_string_pretty(&measured) {
                Ok(body) => {
                    match write_export_file(
                        &target_dir,
                        &fallback_dir,
                        &stem,
                        "timeline.json",
                        &body,
                    ) {
                        Ok(path) => trace_log(format!("timeline={}", path.display())),
                        Err(e) => {
                            eprintln!("warning: timeline write failed (subtitle still OK): {e}");
                            trace_log(format!("timeline write failed (non-fatal): {e}"));
                        }
                    }
                }
                Err(e) => {
                    eprintln!("warning: timeline serialize failed (subtitle still OK): {e}");
                }
            }
        }

        if let Some(words_path) = export.words_json.as_ref() {
            match write_words_json(words_path, &stem, media_name, &source_lang_key, &words) {
                Ok(()) => trace_log(format!("words_json={}", words_path.display())),
                Err(e) => {
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

        Ok(primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::StopReason;
    use crate::paths::output_srt_path;

    fn ok_transcript(text: &str) -> Transcript {
        Transcript {
            text: text.to_string(),
            raw_output: text.to_string(),
            stop_reason: StopReason::Eos,
        }
    }

    fn degenerate() -> Transcript {
        Transcript {
            text: "ああ".to_string(),
            raw_output: "ああ".to_string(),
            stop_reason: StopReason::MaxTokens,
        }
    }

    /// 断骨验证：窗口宽于 25s 就退化，窄于就正常；复读兜底必须把它切成
    /// <25s 的子窗口并把文字拼回来。把 `recover_by_splitting` 的递归去掉
    /// （直接返回 None）后，这个测试会变红。
    #[test]
    fn degenerate_chunk_is_recovered_by_splitting() {
        let decode = |lo: f32, hi: f32| -> Result<Transcript, AsrError> {
            if hi - lo > 25.0 {
                Ok(degenerate())
            } else {
                Ok(ok_transcript(&format!("piece {lo:.1}-{hi:.1}")))
            }
        };
        let parts = recover_by_splitting(&decode, 0.0, 60.0, MAX_SPLIT_DEPTH)
            .expect("60s 块对半切后应该全部收敛为正常窗口");
        assert!(
            parts.len() >= 4,
            "60s → 15s 应为 4 片，实际 {}",
            parts.len()
        );
        // 覆盖连续、不重不漏
        assert!((parts[0].0 - 0.0).abs() < 1e-3);
        assert!((parts.last().unwrap().1 - 60.0).abs() < 1e-3);
        for w in parts.windows(2) {
            assert!((w[0].1 - w[1].0).abs() < 1e-3, "子窗口不连续");
        }
        for (_, _, text) in &parts {
            assert!(text.starts_with("piece "), "子窗口文字丢了: {text}");
        }
    }

    /// 切到最细仍然退化 → `None`，调用方保留原结果并发告警（不能假装成功、
    /// 也不能把半截文字当成整块内容）。
    #[test]
    fn unrecoverable_chunk_returns_none() {
        let decode = |_: f32, _: f32| -> Result<Transcript, AsrError> { Ok(degenerate()) };
        assert!(recover_by_splitting(&decode, 0.0, 60.0, MAX_SPLIT_DEPTH).is_none());
    }

    /// 深度用尽也别再切（避免为了救一块而无限切片，把耗时打穿）。
    #[test]
    fn depth_zero_never_splits() {
        let decode = |_: f32, _: f32| -> Result<Transcript, AsrError> { Ok(degenerate()) };
        assert!(recover_by_splitting(&decode, 0.0, 60.0, 0).is_none());
    }

    /// 普通转录保留视频文件名，文稿匹配固定加 `.aligned`。
    #[test]
    fn subtitle_output_stem_distinguishes_transcription_from_matching() {
        let transcript = TranscriptInput::default();
        assert_eq!(output_stem("clip", None), "clip");
        assert_eq!(output_stem("clip", Some(&transcript)), "clip.aligned");
    }
    /// TXT-only 的空结果有自己的成对文案，不冒充「断句后字幕为空」。
    #[test]
    fn empty_txt_body_message_is_bilingual_and_not_about_subtitles() {
        for (lang, expected) in [
            (UiLang::Zh, i18n::ERR_EMPTY_TXT_BODY.zh),
            (UiLang::En, i18n::ERR_EMPTY_TXT_BODY.en),
        ] {
            crate::i18n::with_ui_lang(lang, || {
                assert_eq!(AsrError::EmptyTxtBody.to_string(), expected);
            });
        }
        assert_ne!(
            i18n::ERR_EMPTY_TXT_BODY.zh,
            i18n::ERR_EMPTY_SENTENCE_BOUNDARY.zh
        );
        assert_ne!(
            i18n::ERR_EMPTY_TXT_BODY.en,
            i18n::ERR_EMPTY_SENTENCE_BOUNDARY.en
        );
    }

    /// TXT-only 的空正文在出口处返回 `EmptyTxtBody`，而不是伪装成字幕空。
    /// 文案由上面那条双语测试盖；这条盖的是守卫 → 变体的映射。
    #[test]
    fn txt_only_empty_body_returns_empty_txt_body_error() {
        let settings = Settings::default();
        let provider = crate::engine::testing::FakeProvider::new("", Vec::new());
        let pipeline = Pipeline::new(&settings, &provider, |_| {});
        let err = pipeline
            .stage_export_txt_only(Path::new("clip.mp4"), "clip.mp4", Some(String::new()))
            .expect_err("空 TXT 正文必须报错");
        assert!(matches!(err, AsrError::EmptyTxtBody), "{err:?}");
    }

    /// 混合导出里 TXT 为空的那支同样返回 `EmptyTxtBody`（只有字幕空才报
    /// 「断句后字幕为空」）。
    #[test]
    fn mixed_export_empty_txt_body_returns_empty_txt_body_error() {
        let settings = Settings {
            output_srt: false,
            output_ass: false,
            output_txt: true,
            ..Settings::default()
        };
        let provider = crate::engine::testing::FakeProvider::new("", Vec::new());
        let pipeline = Pipeline::new(&settings, &provider, |_| {});
        let err = pipeline
            .stage_export(
                ExportInput {
                    input: Path::new("clip.mp4"),
                    media_name: "clip.mp4",
                    speech_segments: Vec::new(),
                    transcript: None,
                    txt_body: Some(String::new()),
                },
                AlignOutput {
                    words: vec![WordToken {
                        start: 0.0,
                        end: 1.0,
                        word: "你好".into(),
                    }],
                },
                ProcessExportOptions::default(),
            )
            .expect_err("空 TXT 正文必须报错");
        assert!(matches!(err, AsrError::EmptyTxtBody), "{err:?}");
    }

    #[test]
    fn output_path_uses_stem() {
        let out = Path::new("Install").join("OneAsr").join("output");
        let input = Path::new("clips").join("lecture_01.mp4");
        assert_eq!(
            output_srt_path(&out, &media_stem(&input)),
            out.join("lecture_01.srt")
        );
    }

    #[test]
    fn check_asr_model_dir_rejects_missing() {
        let dir =
            std::env::temp_dir().join(format!("oneasr_empty_model_probe_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let err = check_asr_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("不完整"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
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
        assert_ne!(
            AsrStage::LoadingAsr.label(UiLang::Zh),
            AsrStage::LoadingAligner.label(UiLang::Zh)
        );
        assert_eq!(AsrStage::Planning.label(UiLang::Zh), "分段规划");
        assert_eq!(
            AsrStage::RoughTranscribing.label(UiLang::Zh),
            "热词提取",
            "这一步对外的名字是「热词提取」——「粗转」是内部机制名，不进界面"
        );
        // 阶段名是一串名词短语（转码音频 / 分段规划 / …），不该混进「进行时」标记：
        // 耗时卡片里的这些行是**做完的事**，不是「正在…中」。
        assert_eq!(AsrStage::Transcribing.label(UiLang::Zh), "转写");
        assert_eq!(AsrStage::Aligning.label(UiLang::Zh), "打轴");
        assert!(timing.has_breakdown());
    }

    /// 机器可读 key：9 个阶段各一个、互不相同、全是小写蛇形。
    ///
    /// 断骨锚点：把 `key()` 里任意两个分支写成同一个字符串（或改掉一个已有的名字），
    /// 本用例立刻变红 —— 上层按 key 做状态机，重名 / 改名都是接口破坏。
    #[test]
    fn every_stage_has_a_distinct_machine_key() {
        let keys: Vec<&str> = [
            AsrStage::Converting,
            AsrStage::Separating,
            AsrStage::Planning,
            AsrStage::RoughTranscribing,
            AsrStage::LoadingAsr,
            AsrStage::Transcribing,
            AsrStage::LoadingAligner,
            AsrStage::Aligning,
            AsrStage::Exporting,
        ]
        .into_iter()
        .map(AsrStage::key)
        .collect();
        assert_eq!(keys.len(), 9);
        for key in &keys {
            assert!(
                !key.is_empty()
                    && key.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                    && !key.starts_with('_'),
                "key 必须是英文小写蛇形名: {key}"
            );
        }
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), keys.len(), "阶段 key 不能重名: {keys:?}");
        assert_eq!(AsrStage::Transcribing.key(), "transcribing");
        assert_eq!(AsrStage::RoughTranscribing.key(), "rough_transcribing");
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

    #[test]
    fn model_validation_cache_returns_consistent_results() {
        let dir = std::env::temp_dir().join(format!("oneasr_cache_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        // Single-file model: must be >= 1024 bytes to pass size check.
        let big = vec![0u8; 2048];
        std::fs::write(dir.join("model.safetensors"), &big).unwrap();

        // Both calls should return Ok — second call hits the cache.
        let r1 = check_asr_model_dir(&dir);
        let r2 = check_asr_model_dir(&dir);
        assert!(r1.is_ok(), "first call failed: {r1:?}");
        assert!(r2.is_ok(), "second call failed: {r2:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_asr_model_dir_uses_shard_filenames_not_tensor_names() {
        let dir = std::env::temp_dir().join(format!("oneasr_shard_index_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        std::fs::write(
            dir.join("model.safetensors.index.json"),
            br#"{"weight_map":{"thinker.audio_tower.conv2d1.bias":"model-00001-of-00001.safetensors"}}"#,
        )
        .unwrap();
        std::fs::write(
            dir.join("model-00001-of-00001.safetensors"),
            vec![0u8; 2048],
        )
        .unwrap();

        let err_before = check_asr_model_dir(&dir);
        assert!(err_before.is_ok(), "{err_before:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_asr_model_dir_names_missing_shard() {
        let dir = std::env::temp_dir().join(format!("oneasr_missing_shard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
        std::fs::write(
            dir.join("model.safetensors.index.json"),
            br#"{"weight_map":{"t":"model-00001-of-00001.safetensors"}}"#,
        )
        .unwrap();

        let err = i18n::with_ui_lang(UiLang::Zh, || {
            check_asr_model_dir(&dir).unwrap_err().to_string()
        });
        assert!(err.contains("model-00001-of-00001.safetensors"), "{err}");
        assert!(err.contains("不完整"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn check_demucs_rejects_legacy_merged_weights() {
        let parent =
            std::env::temp_dir().join(format!("oneasr_demucs_legacy_{}", std::process::id()));
        let dir = parent.join("htdemucs_ft");
        let _ = std::fs::remove_dir_all(&parent);
        std::fs::create_dir_all(&dir).unwrap();
        // Old CUDA-era merged bag must not count as ready.
        std::fs::write(dir.join("htdemucs_ft.safetensors"), vec![0u8; 4096]).unwrap();
        let err = i18n::with_ui_lang(UiLang::Zh, || {
            check_demucs_model_dir(&dir).unwrap_err().to_string()
        });
        assert!(err.contains("htdemucs_ft_vocals.safetensors"), "{err}");
        assert!(err.contains("人声分离"), "{err}");

        // Wrong size of the vocals shard is still incomplete.
        std::fs::write(
            dir.join(crate::engine::local::DEMUCS_WEIGHTS_FILE),
            vec![0u8; 4096],
        )
        .unwrap();
        let err = i18n::with_ui_lang(UiLang::Zh, || {
            check_demucs_model_dir(&dir).unwrap_err().to_string()
        });
        assert!(err.contains("htdemucs_ft_vocals.safetensors"), "{err}");
        assert!(err.contains("字节"), "{err}");
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn check_demucs_custom_dir_looks_for_vocals_file() {
        let dir = std::env::temp_dir().join(format!("oneasr_demucs_custom_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("htdemucs_ft.safetensors"), vec![0u8; 4096]).unwrap();
        let err = check_demucs_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("htdemucs_ft_vocals.safetensors"), "{err}");

        std::fs::write(
            dir.join(crate::engine::local::DEMUCS_WEIGHTS_FILE),
            vec![0u8; 2048],
        )
        .unwrap();
        assert!(
            check_demucs_model_dir(&dir).is_ok(),
            "{:?}",
            check_demucs_model_dir(&dir)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asr_and_aligner_ready_caches_do_not_alias() {
        let dir = std::env::temp_dir().join(format!("oneasr_role_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("tokenizer.json"), vec![0u8; 2048]).unwrap();
        std::fs::write(dir.join("model.safetensors"), vec![0u8; 2048]).unwrap();

        assert!(check_asr_model_dir(&dir).is_ok());
        let err = check_aligner_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("tokenizer_config.json"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 设备丢失的 panic：恰好重跑一次，且重跑落在 CPU 上、挂了原阶段的 warning。
    #[test]
    fn device_loss_panic_retries_once_on_cpu() {
        let settings = Settings {
            backend: "gpu".into(),
            ..Settings::default()
        };
        let updates = std::sync::Mutex::new(Vec::new());
        let mut on_stage = |update: StageUpdate| {
            updates
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(update);
        };
        let calls = std::cell::Cell::new(0usize);
        // 预期中的 panic 不往测试输出里刷栈。
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = run_local_with_device_loss_retry::<()>(
            &settings,
            &mut on_stage,
            &std::sync::Mutex::new(AsrStage::Transcribing),
            |settings, _| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    panic!(
                        "Error in Device::poll: Validation Error\n\nCaused by:\n  Parent device is lost"
                    );
                }
                assert_eq!(settings.backend, "cpu", "重跑必须换成 CPU");
                Ok(())
            },
        );
        std::panic::set_hook(hook);

        assert!(result.is_ok());
        assert_eq!(calls.get(), 2, "设备丢失恰好重跑一次");
        let updates = updates.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(updates.len(), 1, "只该有重跑那一条 warning 更新");
        assert_eq!(updates[0].stage, AsrStage::Transcribing);
        assert!(updates[0].warning.is_some(), "用户要被告知已改 CPU");
    }

    /// 引擎把设备丢失当返回错误而不是 panic 时，同一条重跑规则也要生效。
    #[test]
    fn device_loss_error_also_retries_on_cpu() {
        let settings = Settings {
            backend: "auto".into(),
            ..Settings::default()
        };
        let mut on_stage = |_: StageUpdate| {};
        let calls = std::cell::Cell::new(0usize);
        let result = run_local_with_device_loss_retry::<()>(
            &settings,
            &mut on_stage,
            &std::sync::Mutex::new(AsrStage::Aligning),
            |settings, _| {
                calls.set(calls.get() + 1);
                if calls.get() == 1 {
                    return Err(AsrError::Other("dxgi_error_device_removed".into()));
                }
                assert_eq!(settings.backend, "cpu");
                Ok(())
            },
        );
        assert!(result.is_ok());
        assert_eq!(calls.get(), 2);
    }

    /// 非设备丢失的 panic 原样还给外层（GUI worker 的既有 panic 处理），不许重跑。
    #[test]
    fn non_device_loss_panic_is_resumed_not_retried() {
        let settings = Settings {
            backend: "gpu".into(),
            ..Settings::default()
        };
        let mut on_stage = |_: StageUpdate| {};
        let calls = std::cell::Cell::new(0usize);
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_local_with_device_loss_retry::<()>(
                &settings,
                &mut on_stage,
                &std::sync::Mutex::new(AsrStage::Exporting),
                |_, _| {
                    calls.set(calls.get() + 1);
                    panic!("decode blew up");
                },
            )
        }));
        std::panic::set_hook(hook);

        let payload = outcome.expect_err("panic 必须继续往外传");
        assert_eq!(crate::engine::panic_message(&*payload), "decode blew up");
        assert_eq!(calls.get(), 1, "非设备丢失不许重跑");
    }

    /// 已经在 CPU 上还报设备丢失，就不是这颗雷——照旧失败，别原地重跑。
    #[test]
    fn device_loss_on_cpu_does_not_rerun() {
        let settings = Settings {
            backend: "cpu".into(),
            ..Settings::default()
        };
        let mut on_stage = |_: StageUpdate| {};
        let calls = std::cell::Cell::new(0usize);
        let result = run_local_with_device_loss_retry::<()>(
            &settings,
            &mut on_stage,
            &std::sync::Mutex::new(AsrStage::Transcribing),
            |_, _| {
                calls.set(calls.get() + 1);
                Err(AsrError::Other("Parent device is lost".into()))
            },
        );
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    /// 重跑告警是成对的用户可见文案。
    #[test]
    fn device_lost_retry_warning_is_bilingual() {
        let zh = i18n::with_ui_lang(UiLang::Zh, i18n::gpu_device_lost_retry_cpu);
        let en = i18n::with_ui_lang(UiLang::En, i18n::gpu_device_lost_retry_cpu);
        assert!(zh.contains("CPU"), "{zh}");
        assert!(en.contains("CPU"), "{en}");
        assert_ne!(zh, en);
    }
}
