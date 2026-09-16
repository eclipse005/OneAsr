//! Turning raw ASR words into subtitle text: normalization, alignment,
//! beautification and SRT serialization.

pub mod alignment;
pub mod beautify;
pub mod segmenter;
pub mod srt;
pub mod text_rules;
