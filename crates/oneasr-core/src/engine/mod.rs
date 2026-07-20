//! Subtitle segment types and simple export helpers.
//!
//! Primary product path builds SRT via `sentence_boundary` + `subtitle::srt`.
//! These types remain for debug JSON and legacy call sites.

mod export;
mod segment;

pub use export::{export_json, export_plain, export_srt, ExportOptions};
pub use segment::Segment;

/// Parsed transcript ready for export.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptDocument {
    pub segments: Vec<Segment>,
    pub raw: Option<String>,
}

impl TranscriptDocument {
    pub fn new(segments: Vec<Segment>) -> Self {
        Self {
            segments,
            raw: None,
        }
    }

    pub fn with_raw(mut self, raw: impl Into<String>) -> Self {
        self.raw = Some(raw.into());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }

    pub fn len(&self) -> usize {
        self.segments.len()
    }

    pub fn to_srt(&self, opts: &ExportOptions) -> String {
        export_srt(&self.segments, opts)
    }

    pub fn to_json(&self) -> String {
        export_json(&self.segments)
    }

    pub fn to_plain(&self, opts: &ExportOptions) -> String {
        export_plain(&self.segments, opts)
    }
}
