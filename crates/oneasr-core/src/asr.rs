//! ASR pipeline: prepare → VAD → transcribe → align → export.
//!
//! ```text
//! [optional vocal separation] → 16 kHz master
//! load ASR once → all VAD chunks transcribed → unload ASR
//! load Aligner once → all chunks aligned → unload Aligner
//! word normalize → sentence_boundary → SRT / TXT
//! ```
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
    invalidate_all_model_checks, invalidate_model_check, is_model_ready,
};

use std::cell::RefCell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::diagnostics::{pipeline_trace, trace_log};
use crate::engine::{
    AlignRequest, AlignedToken, EngineProvider, SeparateRequest, SeparationEvent, TranscribeRequest,
};
use crate::i18n::{self, UiLang, t};
use crate::lang::{to_lang_key, to_qwen_language_label};
use crate::media::{MediaError, convert_to_16k_mono_wav, slice_wav, wav_duration_sec};
use crate::paths::media_stem;
use crate::settings::Settings;
use crate::subtitle::segmenter::{WordToken, normalize_word_tokens};
use crate::timeline::{self, RenderOptions, Timeline};
use crate::vad;

#[derive(Debug)]
pub enum AsrError {
    Io(std::io::Error),
    Media(MediaError),
    EmptyTranscribe(usize),
    EmptyAlignment,
    /// The transcript attached to the task is empty — nothing to align against.
    EmptyTranscript,
    EmptySentenceBoundary,
    LoadAsr(String),
    LoadAligner(String),
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
            Self::EmptySentenceBoundary => {
                write!(f, "{}", t(i18n::ERR_EMPTY_SENTENCE_BOUNDARY))
            }
            Self::LoadAsr(e) => write!(f, "{}", i18n::load_asr_failed(e)),
            Self::LoadAligner(e) => write!(f, "{}", i18n::load_aligner_failed(e)),
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

const MIN_SILENCE_FALLBACK: f32 = 0.3;

/// Fine-grained pipeline stage for live UI status and timing breakdown.
///
/// Order in a successful run:
/// `Converting` → `Planning` → `LoadingAsr` → `Transcribing` →
/// `LoadingAligner` → `Aligning` → `Exporting`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrStage {
    /// ffmpeg → 16 kHz mono master WAV.
    Converting,
    /// Optional HTDemucs vocal separation (vocals replace the master WAV).
    Separating,
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
    pub fn label(self, lang: UiLang) -> &'static str {
        match (self, lang) {
            (Self::Converting, UiLang::Zh) => "转码音频",
            (Self::Converting, UiLang::En) => "Converting audio",
            (Self::Separating, UiLang::Zh) => "人声分离",
            (Self::Separating, UiLang::En) => "Separating vocals",
            (Self::Planning, UiLang::Zh) => "分段规划",
            (Self::Planning, UiLang::En) => "Planning segments",
            (Self::LoadingAsr, UiLang::Zh) => "加载识别模型",
            (Self::LoadingAsr, UiLang::En) => "Loading ASR model",
            (Self::Transcribing, UiLang::Zh) => "转写中",
            (Self::Transcribing, UiLang::En) => "Transcribing",
            (Self::LoadingAligner, UiLang::Zh) => "加载对齐模型",
            (Self::LoadingAligner, UiLang::En) => "Loading aligner model",
            (Self::Aligning, UiLang::Zh) => "打轴中",
            (Self::Aligning, UiLang::En) => "Aligning",
            (Self::Exporting, UiLang::Zh) => "导出字幕",
            (Self::Exporting, UiLang::En) => "Exporting subtitles",
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
///
/// `data_root` 是**数据目录**（`crate::paths::data_dir()`）：本次运行的临时目录
/// 开在它下面。调用方传入而不是在这里重新解析，测试才能塞一个 scratch 目录。
pub fn process_media_file_with_progress<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    data_root: &Path,
    on_stage: impl FnMut(StageUpdate) + 'a,
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
    on_stage: impl FnMut(StageUpdate) + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    let provider = crate::engine::local::LocalEngineProvider::from_settings(settings)
        .map_err(|e| AsrError::Other(e.message().to_string()))?;
    process_media_file_with_provider(
        input, media_name, settings, data_root, &provider, on_stage, export,
    )
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
    on_stage: impl FnMut(StageUpdate) + 'a,
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
    on_stage: impl FnMut(StageUpdate) + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    let provider = crate::engine::local::LocalEngineProvider::from_settings(settings)
        .map_err(|e| AsrError::Other(e.message().to_string()))?;
    process_media_file_with_transcript(
        input, media_name, transcript, settings, data_root, &provider, on_stage, export,
    )
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
    on_stage: impl FnMut(StageUpdate) + 'a,
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

/// 两个路径是不是同一个文件。
///
/// 必须先归一：输出目录来自设置（绝对），而文稿路径是用户或 CLI 给的（可能是
/// 相对），`/out/a.srt` 与 `out/a.srt` 文本不相等但就是同一个文件——不做这一步，
/// 撞名保护永远不触发，而它一失效就是**覆盖用户自己的字幕**。
///
/// 大小写按平台语义走：Windows 不区分，Linux 区分。macOS 的 APFS 默认不区分、
/// 但可以格式化成区分，**这里按区分处理**——判断偏保守只会漏掉一次改名（用户
/// 得到覆盖后的文件、还能从回收站拿回来），反过来误判则会白白把输出改名。
/// 方向不对的那一边更贵。
fn same_file(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| -> PathBuf {
        std::fs::canonicalize(p).unwrap_or_else(|_| {
            if p.is_absolute() {
                p.to_path_buf()
            } else {
                std::env::current_dir().unwrap_or_default().join(p)
            }
        })
    };
    // 按**分量**比，不比字符串：分隔符、斜杠方向、盘符大小写都会让字符串不等，
    // 而它们都是同一个文件。
    let parts = |p: &Path| -> Vec<String> {
        norm(p)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect()
    };
    let (pa, pb) = (parts(a), parts(b));
    if cfg!(windows) {
        pa.len() == pb.len() && pa.iter().zip(&pb).all(|(a, b)| a.eq_ignore_ascii_case(b))
    } else {
        pa == pb
    }
}

/// 文稿的原始文件路径会不会被这一轮的输出覆盖。
///
/// 纯函数，好测：只在「文稿就在输出目录里、且文件名正好是 `{stem}.{ext}`」时
/// 为真——也就是用户把旧字幕放在源视频旁边、而输出默认也写在那里的那种情形。
fn transcript_would_be_overwritten(
    transcript: &Path,
    dirs: &[&Path],
    stem: &str,
    exts: &[&str],
) -> bool {
    dirs.iter().any(|dir| {
        exts.iter()
            .any(|ext| same_file(&dir.join(format!("{stem}.{ext}")), transcript))
    })
}
struct VadPlan {
    chunks: Vec<vad::Chunk>,
    speech_segments: Vec<(f64, f64)>,
    multi_chunk: bool,
}

/// Completed ASR transcription result.
struct AsrOutput {
    transcripts: Vec<ChunkTranscript>,
}

/// Completed ForcedAligner result.
struct AlignOutput {
    words: Vec<WordToken>,
}

/// Convert → VAD → ASR → align → SRT. ASR is dropped before the aligner loads.
struct Pipeline<'a> {
    settings: &'a Settings,
    provider: &'a dyn EngineProvider,
    on_stage: RefCell<Box<dyn FnMut(StageUpdate) + 'a>>,
}

impl<'a> Pipeline<'a> {
    fn new(
        settings: &'a Settings,
        provider: &'a dyn EngineProvider,
        on_stage: impl FnMut(StageUpdate) + 'a,
    ) -> Self {
        Self {
            settings,
            provider,
            on_stage: RefCell::new(Box::new(on_stage)),
        }
    }

    fn emit(&self, update: StageUpdate) {
        (self.on_stage.borrow_mut())(update);
    }

    fn run(
        self,
        input: &Path,
        media_name: &str,
        data_root: &Path,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        let conv = self.stage_prepare(input, data_root)?;
        let vad = self.stage_vad_plan(&conv)?;
        let asr = self.stage_transcribe_all(&conv, &vad)?;
        let align = self.stage_align_all(&conv, &vad, &asr)?;
        self.stage_export(input, media_name, vad.speech_segments, None, align, export)
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
        if transcript.text.trim().is_empty() {
            return Err(AsrError::EmptyTranscript);
        }
        let conv = self.stage_prepare(input, data_root)?;
        let align = self.stage_align_transcript(&conv, &transcript.text)?;
        // 文稿模式没有 VAD：speech segments 只喂给排版 DP 算间隔，而文稿分行
        // 根本不跑那个 DP。
        self.stage_export(
            input,
            media_name,
            Vec::new(),
            Some(transcript),
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
                        vad::speech_to_silences(&speech, conv.duration, MIN_SILENCE_FALLBACK);
                    let planned = vad::plan_chunks(conv.duration, &silences, conv.chunk_sec);
                    #[cfg(debug_assertions)]
                    eprintln!(
                        "[vad] duration={:.1}s speech={} silence_gaps={} chunks={}",
                        conv.duration,
                        speech.len(),
                        silences.len(),
                        planned.len(),
                    );
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

        if pipeline_trace() {
            eprintln!(
                "[pipeline] duration={:.1}s chunk_target={} chunks={}",
                conv.duration,
                conv.chunk_sec,
                chunks.len(),
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

        Ok(VadPlan {
            chunks,
            speech_segments,
            multi_chunk,
        })
    }

    // ── Stage 3: load ASR + transcribe all chunks ───────────────────────

    fn stage_transcribe_all(
        &self,
        conv: &ConvertedAudio,
        vad: &VadPlan,
    ) -> Result<AsrOutput, AsrError> {
        self.emit(StageUpdate::new(AsrStage::LoadingAsr));
        check_asr_model_dir(&self.settings.asr_model_dir)?;
        // The provider owns backend resolution + the GPU→CPU *load* fallback;
        // a failure in the middle of a run is never retried (see `engine`).
        let asr = self
            .provider
            .load_asr()
            .map_err(|e| AsrError::LoadAsr(e.message().to_string()))?;
        let force_lang = to_qwen_language_label(&self.settings.language);
        let chunk_tmp = conv.chunk_tmp();

        let mut transcripts = Vec::new();
        #[cfg(debug_assertions)]
        let mut empty_chunks = Vec::new();

        for (i, chunk) in vad.chunks.iter().enumerate() {
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

            let report = asr
                .transcribe(TranscribeRequest {
                    wav: chunk_path,
                    language: &force_lang,
                    max_new_tokens: self.settings.max_new_tokens,
                })
                .map_err(|e| AsrError::TranscribeChunk(i + 1, e.message().to_string()))?;
            let text = clean_asr_text(&report.text);

            if vad.multi_chunk {
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

        if transcripts.is_empty() {
            return Err(AsrError::EmptyTranscribe(vad.chunks.len().max(1)));
        }
        #[cfg(debug_assertions)]
        if !empty_chunks.is_empty() {
            eprintln!(
                "[asr] skipped empty chunks: {}/{} indices={empty_chunks:?}",
                empty_chunks.len(),
                vad.chunks.len().max(1),
            );
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
        let aligner = self
            .provider
            .load_aligner()
            .map_err(|e| AsrError::LoadAligner(e.message().to_string()))?;

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
        // 进度是真的：分母由 CTC 对齐器自己数（编码窗口），不是这里估的秒数。
        // 用户给一份 21 分钟的文稿要对着一个不动的「打轴中」等二十几秒，
        // 那二十几秒里唯一能告诉他「还在动」的东西就是这条进度。
        let mut sink = |done: usize, total: usize| {
            self.emit(StageUpdate::with_chunk(AsrStage::Aligning, done, total));
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
    /// write the subtitle and the timeline it came from.
    ///
    /// The two phases meet here and nowhere else: everything before this point
    /// *measured* (two models, once), everything [`timeline::render`] does after
    /// it *presents* (pure, re-runnable). Writing the timeline next to the
    /// subtitle is what makes that split usable — without the file, the split
    /// is only a comment.
    fn stage_export(
        &self,
        input: &Path,
        media_name: &str,
        speech_segments: Vec<(f64, f64)>,
        transcript: Option<&TranscriptInput>,
        align: AlignOutput,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
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
                txt: self.settings.output_txt,
                ass: self.settings.output_ass,
            },
        )
        .map_err(AsrError::Other)?;
        let srt_body = rendered.srt;
        let txt_body = rendered.txt;
        let ass_body = rendered.ass;
        if srt_body.as_deref().is_some_and(|b| b.trim().is_empty())
            || txt_body.as_deref().is_some_and(|b| b.trim().is_empty())
            || ass_body.as_deref().is_some_and(|b| b.trim().is_empty())
        {
            return Err(AsrError::EmptySentenceBoundary);
        }

        // Primary target: next to the source media when enabled; fall back to
        // the configured output dir when that write fails (permissions, …).
        let fallback_dir = self.settings.resolved_output_dir();
        let target_dir = self.settings.srt_target_dir(input);

        // 撞名保护：文稿常常就放在源文件旁边，而输出默认也写在那里——
        // `杂谈.mp4` + `杂谈.srt` 会让输出**覆盖用户自己的字幕**。撞上了就整体
        // 换名，三种格式一起挪，免得 SRT 避开了而 ASS 撞上。
        let mut stem = stem;
        if let Some(path) = transcript.and_then(|t| t.path.as_deref())
            && transcript_would_be_overwritten(
                path,
                &[target_dir.as_path(), fallback_dir.as_path()],
                &stem,
                &["srt", "txt", "ass"],
            )
        {
            stem = format!("{stem}.aligned");
            eprintln!("{}", i18n::transcript_renamed(&stem));
            trace_log(format!(
                "transcript at {:?} would be overwritten; outputs renamed to {stem}",
                path
            ));
        }

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

        // The measured timeline, beside the subtitle it produced. Best-effort
        // like the words dump: a timeline that could not be written must not
        // fail a run whose subtitle is already on disk.
        match serde_json::to_string_pretty(&measured) {
            Ok(body) => {
                match write_export_file(&target_dir, &fallback_dir, &stem, "timeline.json", &body) {
                    Ok(path) => trace_log(format!("timeline={}", path.display())),
                    Err(e) => {
                        eprintln!("warning: timeline write failed (SRT still OK): {e}");
                        trace_log(format!("timeline write failed (non-fatal): {e}"));
                    }
                }
            }
            Err(e) => {
                eprintln!("warning: timeline serialize failed (SRT still OK): {e}");
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
    use crate::paths::output_srt_path;

    /// 文稿就放在源视频旁边时，输出**不能**覆盖它——那是用户自己的字幕。撞上了
    /// 就整体换成 `{stem}.aligned`，三种格式一起挪，免得 SRT 避开了而 ASS 撞上。
    ///
    /// 跨平台：用 `join` 拼路径，不写死 `D:\`——写死的话这条断言在 Linux 上
    /// 比的是分隔符，不是被测的规则。相对 vs 绝对那一格是真实踩到的坑：输出
    /// 目录来自设置（绝对），文稿路径是用户给的（可能是相对），文本不相等但
    /// 就是同一个文件，不归一的话这道保护永远不触发。
    #[test]
    fn a_transcript_sitting_on_an_output_path_is_renamed_not_overwritten() {
        let dir = std::path::PathBuf::from("/videos");
        let exts = ["srt", "txt", "ass"];

        // 用户把自己的旧字幕放在源视频旁边 —— 最容易踩的那一种。
        assert!(super::transcript_would_be_overwritten(
            &dir.join("clip.srt"),
            &[dir.as_path()],
            "clip",
            &exts
        ));
        // 输出目录里那份也撞。
        assert!(super::transcript_would_be_overwritten(
            &dir.join("clip.ass"),
            &[dir.as_path()],
            "clip",
            &exts
        ));
        // 相对 vs 绝对：输出目录来自设置（绝对），文稿路径是用户/CLI 给的
        // （可能是相对），文本不相等但就是同一个文件。真实踩到的就是这一格。
        let dir = std::env::current_dir().unwrap().join("output");
        assert!(super::transcript_would_be_overwritten(
            &Path::new("output").join("clip.srt"),
            &[dir.as_path()],
            "clip",
            &exts
        ));

        // 文稿在别处：不改名，输出照常用 `{stem}`。
        assert!(!super::transcript_would_be_overwritten(
            Path::new("/scripts/clip.txt"),
            &[dir.as_path()],
            "clip",
            &exts
        ));
        // 没开 ASS 就不会因为 ASS 撞名而改名。
        assert!(!super::transcript_would_be_overwritten(
            &dir.join("clip.ass"),
            &[dir.as_path()],
            "clip",
            &["srt", "txt"]
        ));
        // 同名不同扩展名不算撞。
        assert!(!super::transcript_would_be_overwritten(
            &dir.join("clip.vtt"),
            &[dir.as_path()],
            "clip",
            &exts
        ));
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
}
