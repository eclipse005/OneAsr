//! Subtitle parse & export engine (product: SRT + debug JSON/plain).
//!
//! ```text
//! MOSS raw → TranscriptEngine::parse_moss_compact → TranscriptDocument
//!         → to_srt / to_json / to_plain
//! ```

mod compact;
mod export;
mod segment;
mod split;

pub use compact::parse_transcript;
pub use export::{export_json, export_plain, export_srt, ExportOptions};
pub use segment::Segment;
pub use split::split_long_segments;

/// Parsed transcript ready for export.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptDocument {
    pub segments: Vec<Segment>,
    /// Original compact raw (optional, for debug).
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

/// Facade for the parse/export engine.
pub struct TranscriptEngine;

impl TranscriptEngine {
    /// Parse MOSS compact output: `[start][Sxx]text[end]` chained.
    pub fn parse_moss_compact(raw: &str) -> TranscriptDocument {
        let segments = parse_transcript(raw);
        TranscriptDocument::new(segments).with_raw(raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_parse_extracts_time_speaker_text() {
        let doc = TranscriptEngine::parse_moss_compact(
            "[1.92][S01]Hello world[8.46][8.82][S02]Second line[13.08]",
        );
        assert_eq!(doc.len(), 2);
        assert_eq!(doc.segments[0].start, 1.92);
        assert_eq!(doc.segments[0].end, 8.46);
        assert_eq!(doc.segments[0].speaker, "S01");
        assert_eq!(doc.segments[0].text, "Hello world");
        assert_eq!(doc.segments[1].speaker, "S02");
        assert!(doc.raw.is_some());
    }

    #[test]
    fn engine_export_formats_from_document() {
        let doc = TranscriptEngine::parse_moss_compact("[0.0][S01]Hi[1.0]");
        let opts = ExportOptions {
            show_speaker: true,
        };
        let srt = doc.to_srt(&opts);
        assert!(srt.contains("-->"));
        assert!(srt.contains("S01: Hi"));

        let json = doc.to_json();
        assert!(json.contains("\"speaker\": \"S01\""));

        let plain = doc.to_plain(&opts);
        assert!(plain.contains("S01: Hi"));
    }
}
