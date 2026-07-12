//! Structured dialogue unit after parsing.

use serde::{Deserialize, Serialize};

/// One subtitle cue: time range + speaker + text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    pub id: String,
    pub start: f64,
    pub end: f64,
    pub speaker: String,
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
}
