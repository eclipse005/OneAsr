//! Turning raw ASR words into subtitle text: normalization, alignment,
//! removal-only cleanup and SRT serialization.
//!
//! Invariant for everything here: the display layer may **remove** from the ASR
//! output but never **add** to it (see `beautify`).

pub mod alignment;
pub mod ass;
pub mod beautify;
pub mod segmenter;
pub mod srt;
pub mod text_rules;
