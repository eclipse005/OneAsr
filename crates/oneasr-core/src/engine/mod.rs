//! Engine ports — the dependency-inversion seam of the pipeline.
//!
//! The pipeline (`crate::asr`) is the high-level policy: staging, the
//! "one model resident at a time" rule, VAD chunking, export formats. It talks
//! to engines only through the traits below, so:
//!
//! - concrete engines (Qwen ASR / Qwen ForcedAligner / HTDemucs) live in
//!   [`local`] as adapters and can be replaced without touching the pipeline;
//! - pipeline behaviour (stage order, engine lifetime, export rules, warnings)
//!   is testable with the in-memory doubles in [`testing`] — no weights, no
//!   GPU, no ffmpeg.
//!
//! Types crossing this seam are plain domain values (paths, strings, seconds),
//! never engine-specific ones.

use std::fmt;
use std::path::{Path, PathBuf};

pub mod local;
// 测试替身（无权重、无 GPU、无 ffmpeg）本来是公共 API 的一部分，会随 release
// 二进制一起编译。它既被 `cargo test` 的外部集成测试用（`tests/*.rs` 是独立
// crate，拿不到 `#[cfg(test)]`），所以门控取 `debug_assertions`：
// 调试/测试构建里有它，`--release` 构建里没有，也不必为它加一个 feature
// 开关去改 CI 的命令行。
//
// **release 构建不含本模块**：引用它的集成测试（`tests/pipeline_behaviour.rs`）
// 自带 `#![cfg(debug_assertions)]` 与之对齐。所以别在 release profile 下跑
// tests —— `cargo test --release` 会把那个文件整个 cfg 掉，管线行为只有调试
// profile 验得到。要让它在 release 也能编译，就得把本模块挪出这个门控。
#[cfg(any(test, debug_assertions))]
pub mod testing;

/// Failure raised by an engine adapter.
///
/// The pipeline adds context (which chunk, which stage) when it converts this
/// into a user-facing [`crate::asr::AsrError`]; call sites know whether they
/// were loading or running, so no extra kind field is needed here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineError(pub String);

impl EngineError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EngineError {}

impl From<String> for EngineError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

/// One transcription call: a prepared 16 kHz mono WAV plus the language hint
/// the engine expects (Qwen label such as `Chinese`).
#[derive(Debug, Clone, Copy)]
pub struct TranscribeRequest<'a> {
    pub wav: &'a Path,
    pub language: &'a str,
    pub max_new_tokens: usize,
}

/// Transcript of one call (kept minimal: the pipeline owns cleaning/punctuation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcript {
    pub text: String,
}

/// One forced-alignment call: a prepared WAV plus the *raw* transcript to align.
#[derive(Debug, Clone, Copy)]
pub struct AlignRequest<'a> {
    pub wav: &'a Path,
    pub text: &'a str,
    pub language: &'a str,
}

/// `(done, total)` 进度回调，**只在引擎自己分块时**才会有。
///
/// 分母是引擎的工作单元（CTC 是编码窗口），不是秒：秒只能估，单元是数出来的。
/// 引擎整段一次算完的路径（Qwen 对齐器按 ASR 段调用）传 `None`——上层据此
/// 显示「没有分母」，而不是一个假的一跳到底。
pub type AlignProgress<'a> = &'a mut dyn FnMut(usize, usize);

/// One aligned token with seconds relative to the request's audio.
#[derive(Debug, Clone, PartialEq)]
pub struct AlignedToken {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
}

/// One vocal-separation call: source media (or a decoded WAV) plus the scratch
/// directory that receives the vocals stem.
#[derive(Debug, Clone, Copy)]
pub struct SeparateRequest<'a> {
    pub input: &'a Path,
    pub out_dir: &'a Path,
}

/// Events raised while separating: chunk progress and non-fatal backend
/// decisions (so the UI can show「人声分离 n/N」and warn about a CPU fallback).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeparationEvent {
    /// One finished inference chunk (always ends at `done == total`).
    Progress { done: usize, total: usize },
    /// `auto` could not use the GPU and continued on CPU.
    FellBackToCpu { reason: String },
}

/// ASR engine: transcribes a prepared WAV.
pub trait AsrEngine {
    fn transcribe(&self, req: TranscribeRequest<'_>) -> Result<Transcript, EngineError>;
}

/// Forced-alignment engine: turns text + audio into timed tokens.
pub trait Aligner {
    /// `on_progress` is the optional sink described on [`AlignProgress`].
    fn align(
        &self,
        req: AlignRequest<'_>,
        on_progress: Option<AlignProgress<'_>>,
    ) -> Result<Vec<AlignedToken>, EngineError>;
}

/// Vocal-separation engine: writes a vocals-only WAV and returns its path.
pub trait Separator {
    fn separate(
        &self,
        req: SeparateRequest<'_>,
        on_event: &mut dyn FnMut(SeparationEvent),
    ) -> Result<PathBuf, EngineError>;
}

/// Lazily constructs engines.
///
/// Lazy on purpose: the pipeline's "one model resident at a time" rule depends
/// on the ASR engine being *dropped* before the aligner is created, so a
/// provider must not preload both. [`load_separator`](Self::load_separator) is
/// only called by runs that enabled vocal separation.
pub trait EngineProvider {
    fn load_asr(&self) -> Result<Box<dyn AsrEngine>, EngineError>;
    fn load_aligner(&self) -> Result<Box<dyn Aligner>, EngineError>;
    fn load_separator(&self) -> Result<Box<dyn Separator>, EngineError>;
}
