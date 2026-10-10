//! Turning raw ASR words into subtitle text: normalization, alignment,
//! removal-only cleanup and SRT serialization.
//!
//! Invariant for everything here except [`polish`]: the display layer may
//! **remove** from the ASR output but never **add** to it (see `beautify`).
//! [`polish`] is the opt-in presentation pass. It may replace punctuation with
//! a space and insert a space between Han and Latin or digits. It does not
//! rebreak lines.

pub mod alignment;
pub mod ass;
pub mod beautify;
pub mod polish;
pub mod segmenter;
pub mod srt;
pub mod text_rules;
