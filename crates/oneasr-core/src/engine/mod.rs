//! # Subtitle parse & export engine
//!
//! Best-practice pipeline (separated from ASR inference):
//!
//! ```text
//! MOSS raw: [0.13][S01]正文[1.37]…
//!        │
//!        ▼  TranscriptEngine::parse_moss_compact
//! TranscriptDocument { segments: [ Segment { start, end, speaker, text }, … ] }
//!        │
//!        ▼  document.export(SubtitleFormat::…) / to_srt / to_vtt / …
//! SRT | VTT | JSON | plain text
//! ```
//!
//! - **Parse once** into a stable intermediate model (`Segment` / `TranscriptDocument`).
//! - **Export many times** from that model without re-touching raw text.
//! - ASR (`crate::asr`) only produces raw strings; it must not format SRT itself.

mod compact;
mod export;
mod segment;

pub use compact::{parse_transcript, ParseError};
pub use export::{
    export_json, export_plain, export_srt, export_vtt, format_srt_time, format_vtt_time,
    ExportOptions,
};
pub use segment::Segment;

/// Supported export formats for structured transcripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubtitleFormat {
    Srt,
    Vtt,
    Json,
    Plain,
}

/// Parsed, structured transcript ready for any exporter.
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptDocument {
    /// Ordered cues: time + speaker + text.
    pub segments: Vec<Segment>,
    /// Original compact raw (optional, for debug / re-parse).
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

    /// Export using a format tag.
    pub fn export(&self, format: SubtitleFormat, opts: &ExportOptions) -> String {
        match format {
            SubtitleFormat::Srt => export_srt(&self.segments, opts),
            SubtitleFormat::Vtt => export_vtt(&self.segments, opts),
            SubtitleFormat::Json => export_json(&self.segments),
            SubtitleFormat::Plain => export_plain(&self.segments, opts),
        }
    }

    pub fn to_srt(&self, opts: &ExportOptions) -> String {
        self.export(SubtitleFormat::Srt, opts)
    }

    pub fn to_vtt(&self, opts: &ExportOptions) -> String {
        self.export(SubtitleFormat::Vtt, opts)
    }

    pub fn to_json(&self) -> String {
        export_json(&self.segments)
    }

    pub fn to_plain(&self, opts: &ExportOptions) -> String {
        self.export(SubtitleFormat::Plain, opts)
    }
}

/// Facade for the parse/export engine.
pub struct TranscriptEngine;

impl TranscriptEngine {
    /// Parse MOSS compact output into a structured document.
    ///
    /// Input shape per cue: `[start][Sxx]text[end]` chained.
    pub fn parse_moss_compact(raw: &str) -> TranscriptDocument {
        let segments = parse_transcript(raw);
        TranscriptDocument::new(segments).with_raw(raw)
    }

    /// Convenience: parse + export SRT in one call (still goes through document).
    pub fn moss_compact_to_srt(raw: &str, opts: &ExportOptions) -> String {
        Self::parse_moss_compact(raw).to_srt(opts)
    }

    /// Convenience: parse + export plain text.
    pub fn moss_compact_to_plain(raw: &str, opts: &ExportOptions) -> String {
        Self::parse_moss_compact(raw).to_plain(opts)
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
    fn engine_export_all_formats_from_document() {
        let doc = TranscriptEngine::parse_moss_compact("[0.0][S01]Hi[1.0]");
        let opts = ExportOptions {
            show_speaker: true,
        };
        let srt = doc.to_srt(&opts);
        assert!(srt.contains("-->"));
        assert!(srt.contains("S01: Hi"));

        let vtt = doc.to_vtt(&opts);
        assert!(vtt.starts_with("WEBVTT"));

        let json = doc.to_json();
        assert!(json.contains("\"speaker\": \"S01\""));

        let plain = doc.to_plain(&opts);
        assert!(plain.contains("S01: Hi"));

        assert_eq!(doc.export(SubtitleFormat::Srt, &opts), srt);
    }

    #[test]
    fn convenience_moss_to_srt() {
        let srt = TranscriptEngine::moss_compact_to_srt(
            "[0.5][S01]Body[1.5]",
            &ExportOptions {
                show_speaker: false,
            },
        );
        assert!(srt.contains("Body"));
        assert!(!srt.contains("S01:"));
    }
}
