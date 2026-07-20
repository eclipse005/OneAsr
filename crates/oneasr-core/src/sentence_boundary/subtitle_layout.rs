//! Subtitle-length-aware segmentation via dynamic programming.
//!
//! After `semantic.rs` pre-splits the word stream at terminal punctuation,
//! some resulting spans still exceed the subtitle length budget. This module
//! re-splits those overlong spans using a DP cost-minimization algorithm that
//! finds the **globally optimal** segmentation — no greedy "first fit" tail
//! artifacts.
//!
//! Core principle: a sentence under the length budget is NEVER split, even if
//! it contains VAD silence pauses. VAD only matters once a span exceeds the
//! budget, where it contributes a lower cost (better cut) than a plain word
//! boundary.
//!
//! All length accounting and connector/abbreviation data is language-aware
//! via the [`LanguageProfile`] trait — this module never branches on language.

use crate::subtitle_length::SubtitleLengthPreset;
use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::text_rules::has_break_terminal_punctuation;

use super::language::{Advisor, LanguageProfile};
use super::types::SplitReason;
use super::vad_align::SpeechSegmentIndex;

/// Hard ceiling: a segment up to this ratio of `limit` is kept intact. Above
/// it, the DP splitter must find cuts.
const KEEP_INTACT_RATIO: f64 = 1.15;
/// Floor: a DP-produced segment at or below this many length units is an
/// unreadable fragment (e.g. a lone "Now," / "Okay," discourse marker) and is
/// absorbed into an adjacent segment after backtracking. Measured in the same
/// language-aware units as the length budget (words for Latin, chars for CJK).
const MIN_FRAGMENT_UNITS: f64 = 2.0;
/// Weight of the length-penalty term relative to boundary base-costs. Kept
/// small (0.3) so boundary quality dominates: a soft-punctuation cut (cost 1.0)
/// always beats a plain word boundary (cost 6.0) regardless of length fit.
const LENGTH_PENALTY_WEIGHT: f64 = 0.3;
/// Sentinel for a forbidden cut position (inside numbers / paired punctuation).
const FORBIDDEN_COST: f64 = f64::INFINITY;

/// Cost of cutting after word `i` (between `words[i]` and `words[i+1]`).
/// Lower = better place to cut. Replaces the old `boundary_rank` ordinal with
/// a continuous cost that the DP minimizes globally.
fn boundary_base_cost(
    words: &[WordTokenDto],
    i: usize,
    vad_index: &SpeechSegmentIndex,
    profile: &dyn LanguageProfile,
    advisor: &Advisor,
    byte_offset: usize,
) -> f64 {
    let Some(left) = words.get(i) else {
        return FORBIDDEN_COST;
    };
    let Some(right) = words.get(i + 1) else {
        // End of stream — a free cut (handled by the span boundary), cost 0.
        return 0.0;
    };

    // Never cut inside paired punctuation or numbers.
    if ends_with_opening_punctuation(&left.word) || starts_with_closing_punctuation(&right.word) {
        return FORBIDDEN_COST;
    }
    if is_numeric_continuation(&left.word, &right.word) {
        return FORBIDDEN_COST;
    }

    // Terminal punctuation — best cut (though usually pre-split by semantic.rs,
    // a span may contain one if it survived pre-splitting).
    if has_break_terminal_punctuation(&left.word) {
        return 0.5;
    }
    // Soft clause punctuation (; : ； ：).
    if ends_with_soft_punctuation(&left.word) {
        return 1.0;
    }
    // Comma (ASCII and CJK) — clause-internal pause.
    if left.word.trim_end().ends_with(',') || left.word.trim_end().ends_with('，') {
        return 1.5;
    }
    // VAD silence crossing — acoustic boundary, better than a plain word gap.
    // Cost scales with silence width: a long pause (≥1.2s, strength ~0.85)
    // costs ~1.15 (nearly as good as soft punctuation), a short breath
    // (<0.5s) costs ~1.9 (barely better than a word boundary). This makes the
    // DP prefer cutting at long acoustic pauses inside overlong spans.
    if vad_index.crosses_silence(left.end, right.start) {
        let sil = vad_index.silence_duration_sec(left.end, right.start);
        return 2.0 - super::vad_align::vad_strength(sil);
    }
    // Before a connector word (and/but/so ... / 但是/因为 ... / しかし ...).
    // The connector table is language-specific via the profile.
    if is_connector_like(right.word.as_str(), profile) && !is_connector_like(left.word.as_str(), profile) {
        return 2.5;
    }
    // Plain word boundary — least preferred legal cut. For CJK languages,
    // the advisor may report this gap is inside a word (jieba), in which case
    // we penalize heavily to discourage splitting words mid-character.
    match advisor.is_word_boundary(byte_offset) {
        Some(false) => FORBIDDEN_COST, // inside a word — never cut mid-word
        _ => 6.0,           // word boundary or no info — default
    }
}

/// Is `token` a coordinator/subordinator in this language? Queries the
/// profile's connector table (lowercased, alphabetic-only comparison).
fn is_connector_like(token: &str, profile: &dyn LanguageProfile) -> bool {
    let lower = token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_ascii_lowercase();
    profile.connectors().iter().any(|c| *c == lower.as_str())
}

/// Split overlong semantic spans into subtitle-length segments via DP.
///
/// Returns absolute word indices with the dominant `SplitReason` for each cut.
pub(super) fn build_subtitle_layout_split_points(
    words: &[WordTokenDto],
    semantic_spans: &[(usize, usize)],
    profile: &dyn LanguageProfile,
    preset: SubtitleLengthPreset,
    vad_index: &SpeechSegmentIndex,
) -> Vec<(usize, SplitReason)> {
    if words.len() < 2 {
        return Vec::new();
    }
    let limit = profile.source_limit(preset) as f64;
    if limit <= 0.0 {
        return Vec::new();
    }
    let max_units = limit * KEEP_INTACT_RATIO;

    let mut out = Vec::<(usize, SplitReason)>::new();
    for &(span_start, span_end) in semantic_spans {
        if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
            continue;
        }
        for cut in dp_split_span(
            words,
            span_start,
            span_end,
            profile,
            limit,
            max_units,
            vad_index,
        ) {
            out.push((cut.index, cut.reason));
        }
    }
    out
}



/// One DP-chosen cut: absolute word index + the dominant boundary reason.
struct DpCut {
    index: usize,
    reason: SplitReason,
}

/// DP-split `start..=end` so every resulting segment stays within `max_units`,
/// minimizing total boundary cost. Spans at or below `max_units` are returned
/// uncut (the core guarantee: short sentences are never fragmented).
fn dp_split_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    profile: &dyn LanguageProfile,
    limit: f64,
    max_units: f64,
    vad_index: &SpeechSegmentIndex,
) -> Vec<DpCut> {
    let n = end - start + 1; // words in this span (1-indexed internally)
    if n < 2 {
        return Vec::new();
    }

    // Build the advisor for this span. For Chinese (zh/yue), jieba tokenizes
    // the span text to learn word boundaries; the advisor tells the cost
    // function whether each token gap is a word boundary or inside a word.
    // All other languages use the no-op DefaultAdvisor.
    let span_text: String = words[start..=end]
        .iter()
        .map(|w| w.word.as_str())
        .collect();
    let advisor = profile.word_boundary_advisor(&span_text);
    // Precompute byte offsets: byte_offset[k] = byte position in span_text
    // immediately after the k-th token (k = 0..n). The gap between token k-1
    // and token k is queried with byte_offset[k].
    let mut byte_offset = vec![0usize; n + 1];
    let mut acc = 0usize;
    for k in 0..n {
        acc += words[start + k].word.len(); // byte length of this token
        byte_offset[k + 1] = acc;
    }

    // Prefix sums of language-aware units for O(1) segment-length queries.
    // prefix[k] = total units of words[start .. start+k].
    let mut prefix = vec![0.0_f64; n + 1];
    for k in 0..n {
        prefix[k + 1] = prefix[k] + profile.token_units(&words[start + k].word);
    }

    // Total span length under budget → no split needed (core guarantee).
    if prefix[n] <= max_units {
        return Vec::new();
    }

    // Precompute base cost at each internal boundary (after the k-th word,
    // k = 1..n-1). base_cost[k] is the cost of cutting between word start+k-1
    // and start+k.
    let mut base_cost = vec![FORBIDDEN_COST; n + 1];
    // Span start (k=0) and span end (k=n) are free boundaries — cutting there
    // costs nothing (they delimit the span, not internal word gaps).
    base_cost[0] = 0.0;
    for k in 1..n {
        base_cost[k] =
            boundary_base_cost(words, start + k - 1, vad_index, profile, &advisor, byte_offset[k]);
    }
    base_cost[n] = 0.0;

    // dp[i] = min total cost to segment words[start..start+i].
    let mut dp = vec![f64::INFINITY; n + 1];
    let mut prev = vec![0usize; n + 1];
    dp[0] = 0.0;

    for i in 1..=n {
        // The last segment is words[start+j .. start+i]. Scan candidate starts
        // j from i-1 downward; once a segment exceeds max_units, earlier j only
        // makes it longer, so we break. A single pathological token longer than
        // max_units (e.g. an ASR encoding artifact) is left intact in its own
        // segment — DP cannot split a token internally, and forcing a cut would
        // only mangle the text.
        for j in (0..i).rev() {
            let seg_len = prefix[i] - prefix[j];
            if seg_len > max_units {
                break;
            }
            if base_cost[j].is_infinite() {
                continue;
            }
            if dp[j].is_infinite() {
                continue;
            }
            let length_penalty = LENGTH_PENALTY_WEIGHT * (seg_len - limit).abs() / limit;
            let cost = dp[j] + base_cost[j] + length_penalty;
            if cost < dp[i] {
                dp[i] = cost;
                prev[i] = j;
            }
        }
    }

    // Backtrack the chosen cuts.
    let mut cuts_rel: Vec<usize> = Vec::new();
    let mut cur = n;
    while cur > 0 {
        let p = prev[cur];
        if p > 0 {
            cuts_rel.push(p);
        }
        cur = p;
    }
    cuts_rel.reverse();

    // Absorb DP fragments that are too short to stand alone as a subtitle line
    // (e.g. a lone "Now," / "Okay," discourse marker isolated by a cheap comma
    // cut). This merges them into an adjacent segment, accepting a segment that
    // slightly exceeds `max_units` — readability over length precision. Only DP
    // cuts are touched; terminal-punctuation boundaries from semantic.rs stay
    // sacred (the spans this function receives are already delimited by them).
    absorb_short_fragments(&mut cuts_rel, &prefix, n);

    // Map relative cut positions to absolute indices. All DP-chosen cuts are
    // length-budget-driven, so they carry `SubtitleLayout` regardless of which
    // boundary (comma / VAD / connector) the cost function preferred.
    cuts_rel
        .into_iter()
        .map(|k| DpCut {
            index: start + k - 1,
            reason: SplitReason::SubtitleLayout,
        })
        .collect()
}

/// Merge DP segments shorter than [`MIN_FRAGMENT_UNITS`] into an adjacent
/// segment by dropping the cut that isolates them.
///
/// Given `cuts_rel` (sorted cut positions, each `k` meaning "cut after the
/// k-th word, 1-indexed") and `prefix` (prefix sums of length units over the
/// `n`-word span), repeatedly find the first too-short segment and dissolve
/// its boundary:
/// - **Leading / interior fragment**: drop its right cut → merges with the
///   following segment. A leading fragment (e.g. `Now,` at the span head) has
///   no preceding semantic sentence to fall back into, so it must join the
///   following segment even if that briefly exceeds the budget.
/// - **Trailing fragment**: no right cut exists → drop its left cut → merges
///   with the preceding segment.
///
/// A span reduced to a single segment with no cuts is left untouched (it is a
/// complete semantic sentence, never to be fragmented or force-merged across a
/// terminal boundary).
fn absorb_short_fragments(cuts_rel: &mut Vec<usize>, prefix: &[f64], n: usize) {
    loop {
        if cuts_rel.is_empty() {
            return;
        }
        // Segment endpoints in word-index space: [c1, c2, ..., last_cut, n].
        // Segment i spans words [prev_end .. end), so its unit count is
        // prefix[end] - prefix[prev_end] (prefix[k] = units of words[0..k]).
        let mut boundaries: Vec<usize> = cuts_rel.clone();
        boundaries.push(n);
        let mut prev_end = 0usize;
        let mut found = None;
        for (seg_idx, &end) in boundaries.iter().enumerate() {
            let units = prefix[end] - prefix[prev_end];
            if units <= MIN_FRAGMENT_UNITS {
                found = Some((seg_idx, end));
                break;
            }
            prev_end = end;
        }
        let Some((seg_idx, _seg_end)) = found else { return; };

        if seg_idx < cuts_rel.len() {
            // Interior/leading fragment: its right cut is cuts_rel[seg_idx].
            cuts_rel.remove(seg_idx);
        } else {
            // Trailing fragment (seg_end == n): its left cut is the last one.
            cuts_rel.pop();
        }
    }
}

// ---- punctuation / number helpers (language-independent) ----

fn ends_with_soft_punctuation(token: &str) -> bool {
    token
        .trim_end()
        .chars()
        .last()
        .map(|ch| matches!(ch, ';' | ':' | '，' | '；' | '：'))
        .unwrap_or(false)
}

fn ends_with_opening_punctuation(token: &str) -> bool {
    token
        .trim_end()
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                '(' | '[' | '{' | '（' | '【' | '「' | '『' | '《' | '“' | '‘'
            )
        })
        .unwrap_or(false)
}

fn starts_with_closing_punctuation(token: &str) -> bool {
    token
        .trim_start()
        .chars()
        .next()
        .map(|ch| {
            matches!(
                ch,
                ')' | ']' | '}' | '）' | '】' | '」' | '』' | '》' | '”' | '’'
            )
        })
        .unwrap_or(false)
}

fn is_numeric_continuation(left: &str, right: &str) -> bool {
    let left_has_digit = left.chars().any(|ch| ch.is_ascii_digit());
    let right_has_digit = right.chars().any(|ch| ch.is_ascii_digit());
    if !left_has_digit || !right_has_digit {
        return false;
    }
    let left_tail = left.trim_end().chars().last();
    let right_head = right.trim_start().chars().next();
    matches!(left_tail, Some('$' | '¥' | '€' | '£' | '.' | ',' | '%'))
        || matches!(right_head, Some('%' | '.' | ',' | '$' | '¥' | '€' | '£'))
}
