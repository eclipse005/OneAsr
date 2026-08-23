use serde::{Deserialize, Serialize};

use super::WordTokenDto;

#[derive(Debug, Clone)]
pub struct SentenceBoundaryRequest {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub subtitle_length_preset: String,
    pub words: Vec<WordTokenDto>,
    pub vad_speech_segments: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentenceStep2 {
    pub task_id: String,
    pub media_path: String,
    pub source_lang: String,
    pub micro_chunk_total: usize,
    pub boundary_total: usize,
    pub sentence_total: usize,
    pub micro_chunks: Vec<MicroChunk>,
    pub boundaries: Vec<BoundaryDecision>,
    pub translation_sentences: Vec<SourceSentence>,
    /// Word stream the spans index into (after beautify / digit-glue).
    #[serde(default)]
    pub words: Vec<WordTokenDto>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MicroChunk {
    pub chunk_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub gap_before_ms: u64,
    pub gap_after_ms: u64,
    pub hard_split_before: bool,
    pub hard_split_after: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BoundaryDecision {
    pub left_chunk_id: usize,
    pub right_chunk_id: usize,
    pub gap_ms: u64,
    pub rule_decision: BoundaryDecisionKind,
    pub llm_decision: BoundaryDecisionKind,
    pub final_decision: BoundaryDecisionKind,
    pub confidence: f64,
    pub reason_tag: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceSentence {
    pub sentence_id: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub word_start: usize,
    pub word_end: usize,
    pub chunk_start: usize,
    pub chunk_end: usize,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BoundaryDecisionKind {
    HardSplit,
    Split,
    Merge,
    Unsure,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SplitReason {
    TerminalPunctuation,
    SubtitleLayout,
}
