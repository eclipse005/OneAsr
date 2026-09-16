//! The SRT cue model: overlap normalization and serialization to `.srt`.

#[derive(Debug, Clone)]
pub struct SrtCue {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn normalize_cues(cues: &[SrtCue]) -> Vec<SrtCue> {
    let mut normalized: Vec<SrtCue> = cues
        .iter()
        .map(|cue| {
            let text = cue.text.replace("\r\n", "\n");
            // Blank lines would terminate the SRT cue early; drop them and
            // trim line ends so multi-line text stays inside one block.
            let text = text
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let start_ms = cue.start_ms;
            let end_ms = cue.end_ms.max(start_ms);
            SrtCue {
                index: cue.index,
                start_ms,
                end_ms,
                text: text.trim().to_string(),
            }
        })
        .collect();

    normalized.sort_by_key(|cue| (cue.start_ms, cue.end_ms));
    for (idx, cue) in normalized.iter_mut().enumerate() {
        cue.index = idx + 1;
    }
    // Keep the timeline monotonic: give zero-length cues a 1ms minimum, then
    // clamp each cue's end to the next cue's start (aligner jitter can overlap).
    for i in 0..normalized.len() {
        if normalized[i].end_ms <= normalized[i].start_ms {
            normalized[i].end_ms = normalized[i].start_ms.saturating_add(1);
        }
        if i + 1 < normalized.len() {
            let next_start = normalized[i + 1].start_ms;
            if normalized[i].end_ms > next_start {
                normalized[i].end_ms = next_start.max(normalized[i].start_ms);
            }
        }
    }

    normalized
}

pub fn to_srt_from_cues(cues: &[SrtCue]) -> String {
    let normalized = normalize_cues(cues);
    if normalized.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    for cue in normalized {
        out.push_str(&cue.index.to_string());
        out.push('\n');
        out.push_str(&format!(
            "{} --> {}\n",
            format_ms_to_srt_time(cue.start_ms),
            format_ms_to_srt_time(cue.end_ms)
        ));
        out.push_str(cue.text.trim());
        out.push_str("\n\n");
    }

    out
}

fn format_ms_to_srt_time(total_ms: u64) -> String {
    let hours = total_ms / 3_600_000;
    let minutes = (total_ms % 3_600_000) / 60_000;
    let seconds = (total_ms % 60_000) / 1_000;
    let millis = total_ms % 1_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cue(index: usize, start_ms: u64, end_ms: u64, text: &str) -> SrtCue {
        SrtCue {
            index,
            start_ms,
            end_ms,
            text: text.to_string(),
        }
    }

    #[test]
    fn to_srt_formats_cues() {
        let srt = to_srt_from_cues(&[SrtCue {
            index: 1,
            start_ms: 1000,
            end_ms: 2500,
            text: "Hello\nWorld".into(),
        }]);
        assert!(srt.contains("00:00:01,000 --> 00:00:02,500"));
        assert!(srt.contains("Hello\nWorld"));
    }

    #[test]
    fn normalize_sorts_and_renumbers() {
        let out = normalize_cues(&[
            cue(9, 500, 900, "second"),
            cue(3, 100, 400, "first"),
        ]);
        assert_eq!(out[0].index, 1);
        assert_eq!(out[0].text, "first");
        assert_eq!(out[1].index, 2);
        assert_eq!(out[1].text, "second");
    }

    #[test]
    fn normalize_clamps_overlaps() {
        let out = normalize_cues(&[cue(1, 0, 2000, "a"), cue(2, 1500, 3000, "b")]);
        assert_eq!(out[0].end_ms, 1500);
        assert_eq!(out[1].start_ms, 1500);
    }

    #[test]
    fn normalize_gives_zero_length_cues_a_minimum() {
        let out = normalize_cues(&[cue(1, 1000, 1000, "a")]);
        assert_eq!(out[0].end_ms, 1001);
    }

    #[test]
    fn normalize_collapses_internal_blank_lines_and_crlf() {
        let out = normalize_cues(&[cue(1, 0, 1000, "a\r\n\r\nb\r\n")]);
        assert_eq!(out[0].text, "a\nb");
        let srt = to_srt_from_cues(&out);
        // Exactly one blank separator between cue text and end of string.
        assert!(srt.ends_with("a\nb\n\n"));
    }
}
