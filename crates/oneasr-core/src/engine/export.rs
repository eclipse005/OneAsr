//! Multi-format exporters — **only** consume structured [`Segment`]s.
//!
//! Never parse raw MOSS text here; always go through the parse engine first.

use super::segment::Segment;

#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    /// When true, prefix subtitle lines with `S01: ` (or equivalent).
    pub show_speaker: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { show_speaker: true }
    }
}

fn display_text(seg: &Segment, opts: &ExportOptions) -> String {
    if opts.show_speaker {
        format!("{}: {}", seg.speaker, seg.text)
    } else {
        seg.text.clone()
    }
}

/// Format seconds as SRT timestamp `HH:MM:SS,mmm`.
pub fn format_srt_time(seconds: f64) -> String {
    let ms_total = (seconds.max(0.0) * 1000.0).round() as u64;
    let hours = ms_total / 3_600_000;
    let minutes = (ms_total % 3_600_000) / 60_000;
    let secs = (ms_total % 60_000) / 1000;
    let ms = ms_total % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02},{ms:03}")
}

/// Format seconds as VTT timestamp `HH:MM:SS.mmm`.
pub fn format_vtt_time(seconds: f64) -> String {
    let ms_total = (seconds.max(0.0) * 1000.0).round() as u64;
    let hours = ms_total / 3_600_000;
    let minutes = (ms_total % 3_600_000) / 60_000;
    let secs = (ms_total % 60_000) / 1000;
    let ms = ms_total % 1000;
    format!("{hours:02}:{minutes:02}:{secs:02}.{ms:03}")
}

pub fn export_srt(segments: &[Segment], opts: &ExportOptions) -> String {
    let mut blocks = Vec::with_capacity(segments.len());
    for (i, seg) in segments.iter().enumerate() {
        blocks.push(format!(
            "{}\n{} --> {}\n{}",
            i + 1,
            format_srt_time(seg.start),
            format_srt_time(seg.end),
            display_text(seg, opts)
        ));
    }
    if blocks.is_empty() {
        String::new()
    } else {
        blocks.join("\n\n") + "\n"
    }
}

pub fn export_vtt(segments: &[Segment], opts: &ExportOptions) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for (i, seg) in segments.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            format_vtt_time(seg.start),
            format_vtt_time(seg.end),
            display_text(seg, opts)
        ));
    }
    out
}

pub fn export_json(segments: &[Segment]) -> String {
    serde_json::to_string_pretty(segments).unwrap_or_else(|_| "[]".into()) + "\n"
}

pub fn export_plain(segments: &[Segment], opts: &ExportOptions) -> String {
    let mut lines = Vec::with_capacity(segments.len());
    for seg in segments {
        lines.push(display_text(seg, opts));
    }
    if lines.is_empty() {
        String::new()
    } else {
        lines.join("\n") + "\n"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::Segment;

    fn sample() -> Vec<Segment> {
        vec![
            Segment::new(1, 1.92, 8.46, "S01", "Hello there"),
            Segment::new(2, 8.82, 13.08, "S02", "Hi back"),
        ]
    }

    #[test]
    fn srt_with_speaker() {
        let srt = export_srt(&sample(), &ExportOptions { show_speaker: true });
        assert!(srt.contains("00:00:01,920 --> 00:00:08,460"));
        assert!(srt.contains("S01: Hello there"));
        assert!(srt.contains("S02: Hi back"));
    }

    #[test]
    fn srt_without_speaker() {
        let srt = export_srt(&sample(), &ExportOptions { show_speaker: false });
        assert!(srt.contains("Hello there"));
        assert!(!srt.contains("S01:"));
    }

    #[test]
    fn vtt_header() {
        let vtt = export_vtt(&sample(), &ExportOptions::default());
        assert!(vtt.starts_with("WEBVTT"));
        assert!(vtt.contains("00:00:01.920"));
    }

    #[test]
    fn json_roundtrip_shape() {
        let j = export_json(&sample());
        assert!(j.contains("\"speaker\": \"S01\""));
    }
}
