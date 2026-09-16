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
//! **Scratch lifecycle**: `runs/{stem}_{ts}/` holds only the full 16 kHz WAV
//! plus at most one temporary chunk file. Product output is
//! `{settings.output_dir}/{stem}.srt` (default `{app}/output`, atomic write).
//! The scratch dir is removed when `ConvertedAudio` drops — success, failure,
//! or panic — unless `ONEASR_KEEP_SCRATCH=1` is set for debugging.

use export::{MIN_ALIGN_SEC, attach_transcript_punctuation, has_alignable_word, write_export_file, write_words_json};

mod export;
mod model_check;

pub use model_check::{
    check_aligner_model_dir, check_asr_model_dir, check_demucs_model_dir,
};

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::time::Instant;

use thiserror::Error;

use crate::diagnostics::{pipeline_trace, trace_log};
use crate::engine::{
    AlignRequest, AlignedToken, EngineProvider, SeparateRequest, SeparationEvent, TranscribeRequest,
};
use crate::lang::{to_lang_key, to_qwen_language_label};
use crate::media::{
    MediaError, convert_to_16k_mono_wav, slice_wav, wav_duration_sec,
};
use crate::paths::media_stem;
use crate::sentence_boundary::{
    SentenceBoundaryRequest, WordTokenDto, build_source_sentences_from_words,
    source_sentences_to_srt, source_sentences_to_txt,
};
use crate::settings::Settings;
use crate::subtitle::segmenter::{WordToken, normalize_word_tokens};
use crate::text_script;
use crate::vad;

#[derive(Debug, Error)]
pub enum AsrError {
    #[error("I/O 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("音频处理错误: {0}")]
    Media(#[from] MediaError),
    #[error("语音识别未产生任何有效文本（{0} 段全部为空）")]
    EmptyTranscribe(usize),
    #[error("对齐后词列表为空")]
    EmptyAlignment,
    #[error("断句后字幕为空")]
    EmptySentenceBoundary,
    #[error("加载语音识别模型失败: {0}")]
    LoadAsr(String),
    #[error("加载对齐模型失败: {0}")]
    LoadAligner(String),
    #[error("转写失败 chunk {0}: {1}")]
    TranscribeChunk(usize, String),
    #[error("对齐失败 chunk {0}（{1}）: {2}")]
    AlignChunk(usize, String, String),
    #[error("{role}模型文件不完整（{missing}）: {dir}")]
    ModelIncomplete {
        role: &'static str,
        missing: String,
        dir: String,
    },
    #[error("{0}")]
    Other(String),
}

impl AsrError {
    /// Wrap an opaque error with context about which pipeline stage failed.
    pub fn with_stage(self, stage: &str) -> Self {
        match self {
            Self::Io(e) => Self::Other(format!("[{stage}] I/O 错误: {e}")),
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
    pub fn label(self) -> &'static str {
        match self {
            Self::Converting => "转码音频",
            Self::Separating => "人声分离",
            Self::Planning => "分段规划",
            Self::LoadingAsr => "加载识别模型",
            Self::Transcribing => "转写中",
            Self::LoadingAligner => "加载对齐模型",
            Self::Aligning => "打轴中",
            Self::Exporting => "导出字幕",
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
    if let Some(last) = stages.last_mut()
        && last.stage == stage {
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
pub fn process_media_file_with_progress<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    on_stage: impl FnMut(StageUpdate) + 'a,
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
pub fn process_media_file_with_export<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    on_stage: impl FnMut(StageUpdate) + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    let provider = crate::engine::local::LocalEngineProvider::from_settings(settings)
        .map_err(|e| AsrError::Other(e.message().to_string()))?;
    process_media_file_with_provider(
        input, media_name, settings, app_root, &provider, on_stage, export,
    )
}

/// [`process_media_file_with_export`] with an explicit
/// `EngineProvider`.
///
/// Production calls go through the wrapper above (local Qwen / HTDemucs
/// engines); embedding and tests inject their own provider — see
/// [`crate::engine::testing`].
pub fn process_media_file_with_provider<'a>(
    input: &Path,
    media_name: &str,
    settings: &Settings,
    app_root: &Path,
    provider: &dyn crate::engine::EngineProvider,
    on_stage: impl FnMut(StageUpdate) + 'a,
    export: ProcessExportOptions,
) -> Result<PathBuf, AsrError> {
    Pipeline::new(settings, provider, on_stage).run(input, media_name, app_root, export)
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

/// VAD segmentation result + chunk plan.
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
        app_root: &Path,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        let conv = self.stage_prepare(input, app_root)?;
        let vad = self.stage_vad_plan(&conv)?;
        let asr = self.stage_transcribe_all(&conv, &vad)?;
        let align = self.stage_align_all(&conv, &vad, &asr)?;
        self.stage_export(input, media_name, vad.speech_segments, align, export)
    }

    // ── Stage 1: optional vocal separation → 16 kHz mono master ─────────

    /// Build the master WAV the rest of the pipeline consumes.
    ///
    /// With separation on, the input is decoded **once** at its native rate for
    /// HTDemucs and the vocals are transcoded to the 16 kHz master; without it
    /// the input is transcoded directly. Either way exactly one full decode
    /// feeds the run.
    ///
    /// The scratch dir is owned by [`ConvertedAudio`] from before the first
    /// decode, so every early return (and panic) cleans up through `Drop`.
    fn stage_prepare(&self, input: &Path, app_root: &Path) -> Result<ConvertedAudio, AsrError> {
        let stem = media_stem(input);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let work_dir = app_root.join("runs").join(format!("{stem}_{stamp}"));
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
            .ok_or_else(|| AsrError::Other("无法读取转码后音频时长".into()))?
            as f32;
        conv.wav_path = wav_path;
        Ok(conv)
    }

    /// Separate the vocals stem (HTDemucs v4, native Rust inference) and return
    /// the vocals WAV. Runs before VAD so chunk planning and ASR both see the
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
                        self.emit(StageUpdate::new(AsrStage::Separating).with_warning(format!(
                            "人声分离 GPU 不可用，已改用 CPU（速度明显变慢）：{reason}"
                        )));
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
                    segment_words.push(WordToken {
                        start: round_millis(seg.start_sec),
                        end: round_millis(seg.end_sec.max(seg.start_sec)),
                        word: word.to_string(),
                    });
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
                    .align(AlignRequest {
                        wav: chunk_path,
                        text: &seg.text,
                        language: &seg.language,
                    })
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
                    segment_words.push(WordToken {
                        start: round_millis(seg.start_sec + start_sec.max(0.0)),
                        end: round_millis(seg.start_sec + end_sec.max(start_sec)),
                        word: word.to_string(),
                    });
                }
            }

            // Qwen aligner strips punctuation — restore from ASR transcript.
            let restored = attach_transcript_punctuation(&seg.text, &segment_words);
            all_words.extend(restored);
        }

        Ok(AlignOutput { words: all_words })
    }

    // ── Stage 5: sentence boundary + SRT write + side exports ────────────

    /// Normalize tokens, run sentence boundary detection, write SRT + JSON.
    fn stage_export(
        &self,
        input: &Path,
        media_name: &str,
        speech_segments: Vec<(f64, f64)>,
        align: AlignOutput,
        export: ProcessExportOptions,
    ) -> Result<PathBuf, AsrError> {
        let stem = media_stem(Path::new(media_name));
        self.emit(StageUpdate::new(AsrStage::Exporting));

        let words = normalize_word_tokens(align.words);
        if words.is_empty() {
            return Err(AsrError::EmptyAlignment);
        }

        let word_dtos: Vec<WordTokenDto> = words
            .iter()
            .map(|w| WordTokenDto {
                start: w.start,
                end: w.end,
                word: w.word.clone(),
            })
            .collect();

        let source_lang_key = to_lang_key(&self.settings.language);
        let mut step2 = build_source_sentences_from_words(SentenceBoundaryRequest {
            task_id: stem.clone(),
            media_path: media_name.to_string(),
            source_lang: source_lang_key.clone(),
            subtitle_length_preset: self.settings.subtitle_length_preset.clone(),
            words: word_dtos,
            vad_speech_segments: speech_segments,
        })
        .map_err(AsrError::Other)?;

        // Chinese output script (zh / yue only). Timing fields are untouched:
        // conversion rewrites cue text only, after alignment and segmentation.
        if let Some(script) = self.settings.text_script_for(&source_lang_key) {
            text_script::convert_sentences(&mut step2, script);
        }

        let srt_body = self
            .settings
            .output_srt
            .then(|| source_sentences_to_srt(&step2));
        let txt_body = self
            .settings
            .output_txt
            .then(|| source_sentences_to_txt(&step2));
        if srt_body.as_deref().is_some_and(|b| b.trim().is_empty())
            || txt_body.as_deref().is_some_and(|b| b.trim().is_empty())
        {
            return Err(AsrError::EmptySentenceBoundary);
        }

        // Primary target: next to the source media when enabled; fall back to
        // the configured output dir when that write fails (permissions, …).
        let fallback_dir = self.settings.resolved_output_dir();
        let target_dir = self.settings.srt_target_dir(input);

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
        let primary = primary.ok_or_else(|| AsrError::Other("未启用任何输出格式".into()))?;

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
            AsrStage::LoadingAsr.label(),
            AsrStage::LoadingAligner.label()
        );
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

        let err = check_asr_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("model-00001-of-00001.safetensors"), "{err}");
        assert!(err.contains("不完整"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn asr_and_aligner_ready_caches_do_not_alias() {
        let dir = std::env::temp_dir().join(format!("oneasr_role_cache_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config.json"), b"{}").unwrap();
        std::fs::write(dir.join("model.safetensors"), vec![0u8; 2048]).unwrap();

        assert!(check_aligner_model_dir(&dir).is_ok());
        let err = check_asr_model_dir(&dir).unwrap_err().to_string();
        assert!(err.contains("tokenizer.json"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
