//! Post-processing: split overlong subtitle segments at natural boundaries.
//!
//! ASR segments are usually sentence-sized, but some exceed comfortable reading
//! length (e.g. 20+ words). This module splits those at the best available
//! boundary (comma/semicolon > connector word) and distributes the original
//! time span proportionally by word count.
//!
//! Only English/Latin text is handled (whitespace-delimited words). CJK text
//! is left untouched for now — it will be handled by a future word-segmentation
//! pass.

use super::segment::Segment;

/// Maximum words per subtitle line. Segments exceeding this are split.
const DEFAULT_MAX_WORDS: usize = 14;
/// Minimum words per fragment after splitting. Shorter fragments are absorbed
/// into the neighbouring half.
const DEFAULT_MIN_WORDS: usize = 3;

/// Words that, when they start the *right* half, indicate a good split point.
const CONNECTORS: &[&str] = &[
    "and", "but", "so", "or", "yet", "for", "nor",
    "because", "although", "though", "while", "whereas", "if", "unless",
    "when", "whenever", "before", "after", "since", "until", "as",
    "which", "that", "meaning",
    "however", "therefore", "moreover", "thus", "hence",
    "meanwhile", "instead", "then", "also",
];

/// Characters whose presence at the end of a word signals a clause boundary.
const CLAUSE_END: &[char] = &[',', ';', ':'];

/// Split overlong segments in-place, returning a new `Vec<Segment>`.
///
/// Segments at or below `max_words` are preserved unchanged. Overlong segments
/// are split recursively until every fragment fits. IDs are renumbered
/// sequentially in the output.
pub fn split_long_segments(segments: &[Segment]) -> Vec<Segment> {
    split_long_segments_with(segments, DEFAULT_MAX_WORDS, DEFAULT_MIN_WORDS)
}

/// Same as [`split_long_segments`] but with custom thresholds (mainly for tests).
pub fn split_long_segments_with(
    segments: &[Segment],
    max_words: usize,
    min_words: usize,
) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for seg in segments {
        let words: Vec<&str> = seg.text.split_whitespace().collect();
        if words.len() <= max_words {
            out.push(seg.clone());
            continue;
        }
        // Recursively split this segment.
        let parts = split_segment(&words, max_words, min_words);
        let total_words: usize = parts.iter().map(|(s, e)| e - s).sum();
        // We need cumulative word counts for proportional time distribution.
        let dur = seg.end - seg.start;
        let mut cum_words = 0usize;
        let n_parts = parts.len();
        for (pi, &(s, e)) in parts.iter().enumerate() {
            let part_words = e - s;
            cum_words += part_words;
            let part_text = words[s..e].join(" ");
            // Proportional time: first part starts at seg.start, last part ends
            // at seg.end. Intermediate boundaries are placed by cumulative ratio.
            let part_start = if pi == 0 {
                seg.start
            } else {
                let prev_cum = cum_words - part_words;
                seg.start + (prev_cum as f64 / total_words as f64) * dur
            };
            let part_end = if pi == n_parts - 1 {
                seg.end
            } else {
                seg.start + (cum_words as f64 / total_words as f64) * dur
            };
            out.push(Segment::new(
                0, // renumber later
                part_start,
                part_end,
                seg.speaker.as_str(),
                part_text.as_str(),
            ));
        }
    }
    // Renumber.
    for (i, seg) in out.iter_mut().enumerate() {
        seg.id = format!("seg_{:04}", i + 1);
    }
    out
}

/// Recursively split a word slice into parts of at most `max_words`.
///
/// Returns `Vec<(start_idx, end_idx)>` into the original word slice.
fn split_segment(words: &[&str], max_words: usize, min_words: usize) -> Vec<(usize, usize)> {
    let n = words.len();
    if n <= max_words {
        return vec![(0, n)];
    }

    // Find the best single split point in the entire range.
    let cut = find_best_split(words, max_words, min_words);

    let (left_end, right_start) = cut.unwrap_or((max_words, max_words));

    // Left part.
    let mut result = vec![(0, left_end)];

    // Right part — recurse if still too long.
    if right_start < n {
        let right_words: Vec<&str> = words[right_start..].to_vec();
        let right_parts = split_segment(&right_words, max_words, min_words);
        for (rs, re) in right_parts {
            result.push((right_start + rs, right_start + re));
        }
    }

    result
}

/// Find the best split point for a word slice that exceeds `max_words`.
///
/// Scans the range `[min_words .. n - min_words]` for the boundary with the
/// lowest cost:
/// - comma/semicolon/colon after a word → cost 1 (best)
/// - connector word starting the right half → cost 2
///
/// Returns `Some((left_end, right_start))` where `left_end` is the index
/// after the last word of the left part, and `right_start` is the index of
/// the first word of the right part. For punctuation splits these are equal
/// (split is between word `i` and word `i+1`). For connector splits,
/// `right_start` points at the connector.
fn find_best_split(
    words: &[&str],
    max_words: usize,
    min_words: usize,
) -> Option<(usize, usize)> {
    let n = words.len();
    // Search window: we want each half to have at least min_words and at most
    // max_words (approximately). Centre the window around 50%.
    let lo = min_words.max(max_words.saturating_sub(4));
    let hi = (n - min_words).min(max_words + 4);

    if lo >= hi || hi >= n {
        // Can't find a balanced split — just cut at max_words.
        return None;
    }

    let mut best_cost = u32::MAX;
    let mut best: Option<(usize, usize)> = None;

    for i in lo..=hi {
        // Cost 1: word i ends with a clause-ending punctuation (comma etc.)
        let left_word = words[i - 1].trim_end_matches(|c: char| c == '"' || c == '\'');
        if left_word.ends_with(|c: char| CLAUSE_END.contains(&c)) {
            // Split right after word i-1, right starts at i.
            if best_cost > 1 {
                best_cost = 1;
                best = Some((i, i));
            }
            continue;
        }

        // Cost 2: word i is a connector → split before it.
        let lower = words[i].to_lowercase();
        let lower_clean = lower.trim_end_matches(|c: char| !c.is_alphanumeric());
        if CONNECTORS.contains(&lower_clean) {
            // Split after word i-1, right starts at i (the connector).
            if best_cost > 2 {
                best_cost = 2;
                best = Some((i, i));
            }
            continue;
        }
    }

    // If nothing found, fall back to cutting at the midpoint within the window.
    match best {
        Some(_) => best,
        None => {
            // No natural boundary found — cut at max_words to keep halves balanced.
            let mid = max_words.min(n - min_words);
            if mid > 0 && mid < n {
                Some((mid, mid))
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, start: f64, end: f64) -> Segment {
        Segment::new(1, start, end, "S01", text)
    }

    #[test]
    fn short_segment_unchanged() {
        let s = seg("Hello world this is short", 0.0, 3.0);
        let out = split_long_segments_with(&[s.clone()], 14, 3);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, s.text);
    }

    #[test]
    fn splits_at_comma() {
        let s = seg(
            "There are two ranges, the first one is New York which is eight twelve AM",
            10.0,
            16.0,
        );
        let out = split_long_segments_with(&[s], 8, 3);
        assert!(out.len() >= 2, "should split into 2+, got {}", out.len());
        // First part should end with comma
        assert!(
            out[0].text.ends_with(','),
            "first part should end with comma: {}",
            out[0].text
        );
        // Time distribution: first part starts at 10.0
        assert!((out[0].start - 10.0).abs() < 0.01);
        // Last part ends at 16.0
        assert!((out[out.len() - 1].end - 16.0).abs() < 0.01);
    }

    #[test]
    fn splits_at_connector() {
        let s = seg(
            "I went to the store today to buy some milk and then I drove back home to cook",
            0.0,
            8.0,
        );
        let out = split_long_segments_with(&[s], 10, 3);
        assert!(out.len() >= 2);
        // One of the splits should be before "and" or "to"
        let has_connector_split = out
            .iter()
            .skip(1)
            .any(|s| s.text.starts_with("and") || s.text.starts_with("to"));
        assert!(
            has_connector_split,
            "expected a split before a connector"
        );
    }

    #[test]
    fn timestamps_proportional() {
        // 20 words over 10 seconds → if split in half, each gets 5s.
        let s = seg(
            "one two three four five six seven eight nine ten \
             eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty",
            0.0,
            10.0,
        );
        let out = split_long_segments_with(&[s], 10, 3);
        assert!(out.len() >= 2);
        // First starts at 0.0
        assert!((out[0].start - 0.0).abs() < 0.01);
        // Last ends at 10.0
        assert!((out[out.len() - 1].end - 10.0).abs() < 0.01);
        // No gaps or overlaps between parts
        for i in 1..out.len() {
            assert!(
                (out[i].start - out[i - 1].end).abs() < 0.01,
                "gap/overlap at part {i}: {} vs {}",
                out[i].start,
                out[i - 1].end
            );
        }
    }

    #[test]
    fn preserves_speaker() {
        let s = Segment::new(1, 0.0, 10.0, "S03", "one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen");
        let out = split_long_segments_with(&[s], 10, 3);
        assert!(out.len() >= 2);
        for part in &out {
            assert_eq!(part.speaker, "S03");
        }
    }

    #[test]
    fn multiple_segments_mixed() {
        let segs = vec![
            seg("short one", 0.0, 1.0),
            seg("this is a much longer segment that definitely needs to be split into smaller pieces for readability", 1.0, 10.0),
            seg("another short", 10.0, 11.0),
        ];
        let out = split_long_segments_with(&segs, 10, 3);
        assert!(out.len() > 3, "the long one should have been split");
        // First and last should be the short ones unchanged.
        assert_eq!(out[0].text, "short one");
        assert_eq!(out[out.len() - 1].text, "another short");
    }
}
