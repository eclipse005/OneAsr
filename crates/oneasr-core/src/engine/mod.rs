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
    fn align(&self, req: AlignRequest<'_>) -> Result<Vec<AlignedToken>, EngineError>;
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
