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
            let mut text = cue.text.replace("\r\n", "\n");
            text = text
                .lines()
                .map(str::trim_end)
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

    normalized.sort_by_key(|cue| cue.start_ms);
    for (idx, cue) in normalized.iter_mut().enumerate() {
        cue.index = idx + 1;
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

/// Parse standard SRT content into cues. Multi-line cue bodies keep `\n`.
/// Empty or unparseable blocks are skipped. Returns an error only when no
/// valid cues remain. A leading UTF-8 BOM is ignored.
pub fn parse_srt_content(content: &str) -> Result<Vec<SrtCue>, String> {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let blocks = normalized
        .split("\n\n")
        .map(str::trim)
        .filter(|block| !block.is_empty());

    let mut cues = Vec::new();
    for block in blocks {
        let lines: Vec<&str> = block.lines().collect();
        if lines.is_empty() {
            continue;
        }

        let mut line_offset = 0usize;
        if lines[0].trim().chars().all(|c| c.is_ascii_digit()) {
            line_offset = 1;
        }
        if line_offset >= lines.len() {
            continue;
        }

        let ts_line = lines[line_offset].trim();
        let Some((start_raw, end_raw)) = ts_line.split_once("-->") else {
            continue;
        };
        let Some(start_ms) = parse_srt_time(start_raw.trim()) else {
            continue;
        };
        let end_token = end_raw.trim().split_whitespace().next().unwrap_or("");
        let Some(end_ms) = parse_srt_time(end_token) else {
            continue;
        };

        let text = lines
            .iter()
            .skip(line_offset + 1)
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }

        cues.push(SrtCue {
            index: cues.len() + 1,
            start_ms,
            end_ms: end_ms.max(start_ms),
            text,
        });
    }

    if cues.is_empty() {
        return Err("SRT contains no valid subtitle cues".to_string());
    }
    Ok(normalize_cues(&cues))
}

fn parse_srt_time(value: &str) -> Option<u64> {
    // HH:MM:SS,mmm or HH:MM:SS.mmm
    let value = value.trim();
    let (hms, ms_part) = value
        .split_once(',')
        .or_else(|| value.split_once('.'))?;
    let mut parts = hms.split(':');
    let hours: u64 = parts.next()?.parse().ok()?;
    let minutes: u64 = parts.next()?.parse().ok()?;
    let seconds: u64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() || minutes >= 60 || seconds >= 60 {
        return None;
    }
    let millis: u64 = ms_part.trim().parse().ok()?;
    if millis >= 1000 {
        return None;
    }
    Some(hours * 3_600_000 + minutes * 60_000 + seconds * 1_000 + millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_srt_content_basic_and_multiline() {
        let raw = "1\n00:00:01,000 --> 00:00:02,500\nHello\nWorld\n\n2\n00:00:03,000 --> 00:00:04,000\nNext\n";
        let cues = parse_srt_content(raw).expect("parse");
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].text, "Hello\nWorld");
        assert_eq!(cues[0].start_ms, 1000);
        assert_eq!(cues[0].end_ms, 2500);
        assert_eq!(cues[1].text, "Next");
    }

    #[test]
    fn parse_srt_content_rejects_empty() {
        assert!(parse_srt_content("").is_err());
        assert!(parse_srt_content("not an srt").is_err());
    }
}
