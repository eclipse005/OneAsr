//! Structured dialogue unit after parsing.

use serde::{Deserialize, Serialize};

/// One subtitle / dialogue cue: time range + speaker + text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    /// Start time in seconds.
    pub start: f64,
    /// End time in seconds.
    pub end: f64,
    /// Normalized speaker id, e.g. `S01`.
    pub speaker: String,
    /// Spoken text without tags.
    pub text: String,
}

impl Segment {
    pub fn new(
        index: usize,
        start: f64,
        end: f64,
        speaker: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: format!("seg_{index:04}"),
            start,
            end,
            speaker: speaker.into(),
            text: text.into(),
        }
    }

    pub fn duration(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
}
