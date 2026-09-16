//! Subtitle-length-aware segmentation via dynamic programming.
//!
//! After `semantic.rs` pre-splits the word stream at terminal punctuation,
//! some resulting spans still exceed the subtitle length budget. This module
//! re-splits those overlong spans using a DP cost-minimization algorithm that
//! finds the **globally optimal** segmentation — no greedy "first fit" tail
//! artifacts.
//!
//! Length policy (quality first, readability second):
//! 1) Within the word/char target → never split, even with VAD pauses inside.
//! 2) Slightly over (grace band ≈ +2 units / +11 display chars): split ONLY at
//!    linguistically good cut points (punctuation, connectors, phrase-closing
//!    particles, real pauses); keep the whole line when no good cut exists.
//! 3) Far over: force cuts so every multi-token segment fits word AND display-
//!    char hard limits; a single pathological token may stand alone.
//!
//! Grammar guard rules (function words, phrase-closing particles, discourse
//! markers, "to"-binding, Japanese orthography) live in `boundary_rules` and
//! are queried from the DP cost + quality functions below.

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::text_rules::has_break_terminal_punctuation;
use super::preset::SubtitleLengthPreset;

use super::boundary_rules::{
    COMMA_COST, CONNECTOR_COST, FORBIDDEN_COST, GLUE_GAP_SEC, GLUED_WORD_COST, GOOD_SILENCE_SEC,
    WORD_COST,
    LENGTH_GRACE_CHARS, MIN_FRAGMENT_UNITS, SHORT_SEGMENT_PENALTY,
    SOFT_COST, TERMINAL_COST, is_bound_connector, is_closing_punctuation,
    is_case_particle_before_predicate, is_connector_like, is_discourse_marker_comma,
    is_function_word_left, is_japanese_lexical_bind, is_japanese_spoken_end,
    is_japanese_orthographic_bind, is_line_start_bound_particle, is_numeric_continuation,
    is_open_genitive_link, is_opening_punctuation, is_phrase_close_particle,
    is_soft_punctuation, is_split_connector_pair, is_split_hai, is_time_glued_content,
    is_to_binding_left, lexical_cut_cost, strip_token, token_gap_sec,
};
use super::profile::{Advisor, LanguageProfile};
use super::types::SplitReason;
use super::vad_align::SpeechSegmentIndex;

/// Weight of the length-penalty term relative to boundary base-costs. Kept
/// small (0.3) so boundary quality dominates: a soft-punctuation cut (cost 1.0)
/// always beats a plain word boundary regardless of length fit.
const LENGTH_PENALTY_WEIGHT: f64 = 0.3;

/// A VAD silence at least this wide (seconds) counts as a linguistically good
/// cut point (a real pause, not a breath).
const VAD_QUALITY_SILENCE_SEC: f64 = 0.5;

/// Cost of cutting after word `i` (between `words[i]` and `words[i+1]`).
/// Lower = better place to cut.
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
        return 0.0; // end of stream — free cut
    };

    // Never cut inside paired punctuation, numbers, or Japanese lexical
    // binds (orthography, する-compounds, さん, て-aux, 盛りだくさん).
    if ends_with_opening_punctuation(&left.word) || starts_with_closing_punctuation(&right.word) {
        return FORBIDDEN_COST;
    }
    if is_numeric_continuation(&left.word, &right.word) {
        return FORBIDDEN_COST;
    }
    if is_japanese_lexical_bind(&left.word, &right.word) {
        return FORBIDDEN_COST;
    }
    let next2 = peek_word(words, i + 2);
    // ます|は|い… is はい, not a dangling particle.
    if is_line_start_bound_particle(&right.word) && !is_split_hai(&right.word, next2) {
        return FORBIDDEN_COST;
    }

    // Terminal punctuation — best cut (rare inside a span; semantic.rs
    // pre-split most of them).
    if has_break_terminal_punctuation(&left.word) {
        return TERMINAL_COST;
    }
    // Spoken Japanese clause end (です/ます/ました). Same role as a period
    // when ASR omitted 。 Linguistic cuts beat 0ms time-glue.
    if spoken_end_at(words, i) {
        return TERMINAL_COST;
    }
    // Soft clause punctuation (; : ； ：).
    if ends_with_soft_punctuation(&left.word) {
        return SOFT_COST;
    }
    // Comma — clause-internal pause; a discourse marker ("Okay," / "Now,")
    // gets NO discount so it can't be isolated as a flash line.
    if is_comma(&left.word) && !is_discourse_marker_comma(&left.word) {
        return COMMA_COST;
    }
    // Genitive の + following head: not a cheap/quality cut. Force mode
    // may still cut here as a last resort so の stays at line END (kinsoku)
    // instead of starting the next cue.
    if is_open_genitive_link(&left.word, &right.word) {
        return WORD_COST;
    }
    // で/に/を/と immediately before their predicate: not a cheap cut.
    if is_case_particle_before_predicate(&left.word, &right.word) {
        return WORD_COST;
    }
    // Phrase-closing particle (的/了/は/を/을...): cutting after it closes a
    // phrase — a good cut. Japanese の is handled above when the head follows.
    if is_phrase_close_particle(&left.word) {
        return COMMA_COST;
    }
    // Function word must never end a line ("of | the", "在 | 教育").
    if is_function_word_left(&left.word, profile.function_words_left()) {
        return FORBIDDEN_COST;
    }

    // Infinitive/preposition "to" starting the next line — a good cut, unless
    // the left word binds it ("need to", "want to").
    let right_stripped = strip_token(&right.word).to_lowercase();
    if right_stripped == "to" && !is_to_binding_left(&left.word) {
        return CONNECTOR_COST;
    }
    // Before a connector (and/but/所以/しかし...), unless the left token binds
    // it into a compound ("只因为"). Checked before time-glue so 0ms ASR
    // collapse cannot hide はい / もっと / 皆さん.
    if is_connector_like(&right.word, profile.connectors())
        && !is_connector_like(&left.word, profile.connectors())
        && !is_bound_connector(&left.word, &right.word, profile.connectors())
    {
        return CONNECTOR_COST;
    }
    if is_split_connector_pair(&right.word, next2, profile.connectors())
        && !is_connector_like(&left.word, profile.connectors())
    {
        return CONNECTOR_COST;
    }

    // Time-glued content is a poor cut, not an illegal one — aligner
    // collapse can glue a whole breath group to 0ms, and forbidding
    // those cuts would produce 50-char cues.
    if is_time_glued_content(
        &left.word,
        &right.word,
        token_gap_sec(Some(left.end), Some(right.start)),
    ) {
        return GLUED_WORD_COST;
    }

    // VAD silence crossing — acoustic boundary, cost scales with pause width.
    if vad_index.crosses_silence(left.end, right.start) {
        let sil = vad_index.silence_duration_sec(left.end, right.start);
        return 2.0 - super::vad_align::vad_strength(sil);
    }
    // Word-gap pause fallback (when VAD data is missing or misses the gap).
    if let Some(gap) = token_gap_sec(Some(left.end), Some(right.start))
        && gap >= GOOD_SILENCE_SEC
    {
        return 2.0 - 0.5 * (gap - GOOD_SILENCE_SEC).min(0.9) / 0.9;
    }
    // Plain word boundary — least preferred legal cut. The word-segmentation
    // advisor (jieba for zh) forbids gaps inside a segmented word.
    match advisor.is_word_boundary(byte_offset) {
        Some(false) => FORBIDDEN_COST,
        _ => lexical_cut_cost(token_gap_sec(Some(left.end), Some(right.start))),
    }
}

/// Is a cut after `words[i]` linguistically GOOD (allowed in quality mode)?
fn is_quality_cut_boundary(
    words: &[WordTokenDto],
    i: usize,
    profile: &dyn LanguageProfile,
    vad_index: &SpeechSegmentIndex,
) -> bool {
    let Some(left) = words.get(i) else { return false };
    let Some(right) = words.get(i + 1) else { return false };

    if ends_with_opening_punctuation(&left.word) || starts_with_closing_punctuation(&right.word) {
        return false;
    }
    if is_numeric_continuation(&left.word, &right.word) {
        return false;
    }
    if is_japanese_lexical_bind(&left.word, &right.word) {
        return false;
    }
    let next2 = peek_word(words, i + 2);
    if has_break_terminal_punctuation(&left.word) {
        return true;
    }
    if spoken_end_at(words, i) {
        return true;
    }
    if ends_with_soft_punctuation(&left.word) {
        return true;
    }
    if is_discourse_marker_comma(&left.word) {
        return false;
    }
    if is_comma(&left.word) {
        return true;
    }
    if is_line_start_bound_particle(&right.word) && !is_split_hai(&right.word, next2) {
        return false;
    }
    if is_open_genitive_link(&left.word, &right.word)
        || is_case_particle_before_predicate(&left.word, &right.word)
    {
        return false;
    }
    if is_phrase_close_particle(&left.word) {
        return true;
    }
    if is_function_word_left(&left.word, profile.function_words_left()) {
        return false;
    }
    if is_connector_like(&right.word, profile.connectors())
        && !is_connector_like(&left.word, profile.connectors())
        && !is_bound_connector(&left.word, &right.word, profile.connectors())
    {
        return true;
    }
    if is_split_connector_pair(&right.word, next2, profile.connectors())
        && !is_connector_like(&left.word, profile.connectors())
    {
        return true;
    }
    if is_time_glued_content(
        &left.word,
        &right.word,
        token_gap_sec(Some(left.end), Some(right.start)),
    ) {
        return false;
    }
    // A real acoustic pause is a good cut: VAD crossing (≥0.5s silence) or a
    // word-gap pause (≥0.35s) when VAD has no data for it.
    if vad_index.crosses_silence(left.end, right.start)
        && vad_index.silence_duration_sec(left.end, right.start) >= VAD_QUALITY_SILENCE_SEC
    {
        return true;
    }
    if let Some(gap) = token_gap_sec(Some(left.end), Some(right.start))
        && gap >= GOOD_SILENCE_SEC
    {
        return true;
    }
    false
}

/// Split overlong semantic spans into subtitle-length segments via DP.
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
    let Some(budget) = SpanBudget::for_profile(profile, preset) else {
        return Vec::new();
    };

    let mut out = Vec::<(usize, SplitReason)>::new();
    for &(span_start, span_end) in semantic_spans {
        if span_start >= words.len() || span_end >= words.len() || span_start >= span_end {
            continue;
        }
        if let Some(cuts) = dp_split_span(words, span_start, span_end, profile, &budget, vad_index)
        {
            for cut in cuts {
                out.push((cut.index, cut.reason));
            }
        }
    }
    out
}

/// One DP-chosen cut: absolute word index + the dominant boundary reason.
struct DpCut {
    index: usize,
    reason: SplitReason,
}

/// Hard limits a multi-token segment must satisfy.
#[derive(Debug, Clone, Copy)]
struct HardLimits {
    /// Preferred length (penalty target).
    target: f64,
    /// Force-mode / validity ceiling (JA bunsetsu may exceed target).
    max_unit: f64,
    char: f64,
}

impl HardLimits {
    fn valid(&self, token_count: usize, units: f64, chars: f64) -> bool {
        if token_count <= 1 {
            return true; // single tokens may stand alone (URLs, artifacts)
        }
        if units > self.max_unit {
            return false;
        }
        if self.char.is_finite() && chars > self.char {
            return false;
        }
        true
    }
    fn char_limited(&self) -> bool {
        self.char.is_finite()
    }
}

/// DP-split `start..=end` so every resulting segment stays within the hard
/// limits, minimizing total boundary cost.
///
/// Returns `None` when the span must be kept intact: under target, or within
/// the grace band with no linguistically good cut reaching the target.
/// The length budget one layout pass runs under.
///
/// Derived once from the language profile + preset, then applied to every
/// semantic span. The four numbers always travel together — passing them
/// individually made the DP call read as a wall of floats.
struct SpanBudget {
    /// Target length in profile units (the profile's source limit, already
    /// scaled by the preset).
    limit: f64,
    /// Character ceiling: the hard stop no unit count may override.
    char_limit: f64,
    /// Slack around `limit` inside which a cut still needs a linguistic reason.
    grace: f64,
    /// Unit count past which a cut is forced regardless of boundary quality.
    force_ceiling: f64,
}

impl SpanBudget {
    /// `None` when the profile asks for no limit at all — the caller then skips
    /// layout entirely rather than splitting on a meaningless budget.
    fn for_profile(profile: &dyn LanguageProfile, preset: SubtitleLengthPreset) -> Option<Self> {
        let limit = f64::from(profile.source_limit(preset));
        if limit <= 0.0 {
            return None;
        }
        Some(Self {
            limit,
            char_limit: profile.source_char_limit(preset),
            grace: profile.length_grace_units(),
            force_ceiling: profile.force_unit_ceiling(limit),
        })
    }
}

fn dp_split_span(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    profile: &dyn LanguageProfile,
    budget: &SpanBudget,
    vad_index: &SpeechSegmentIndex,
) -> Option<Vec<DpCut>> {
    let n = end - start + 1;
    if n < 2 {
        return Some(Vec::new());
    }

    // Word-segmentation advisor for zh (jieba); no-op elsewhere.
    let span_text: String = words[start..=end]
        .iter()
        .map(|w| w.word.as_str())
        .collect();
    let advisor = profile.word_boundary_advisor(&span_text);
    let mut byte_offset = vec![0usize; n + 1];
    let mut acc = 0usize;
    for k in 0..n {
        acc += words[start + k].word.len();
        byte_offset[k + 1] = acc;
    }

    // Prefix sums of language-aware length units.
    let mut prefix = vec![0.0_f64; n + 1];
    for k in 0..n {
        prefix[k + 1] = prefix[k] + profile.token_units(&words[start + k].word);
    }

    // Display-char accounting (only when the profile caps characters).
    let hard = HardLimits {
        target: budget.limit,
        max_unit: budget.force_ceiling.max(budget.limit),
        char: budget.char_limit,
    };
    let span_words = words[start..=end].to_vec();
    let char_of: Box<dyn Fn(usize, usize) -> f64 + Send> = if hard.char_limited() {
        let span_words = span_words.clone();
        Box::new(move |a, b| display_chars(&span_words[a..=b]))
    } else {
        Box::new(|_, _| 0.0)
    };

    let total_units = prefix[n];
    let total_chars = char_of(0, n - 1);

    // ① Within target → keep whole (core guarantee: short sentences are
    // never fragmented, even with VAD pauses inside).
    if total_units <= hard.target && (!hard.char_limited() || total_chars <= hard.char) {
        return Some(Vec::new());
    }

    // ② Grace band: only good cuts, fall back to keeping the whole line.
    let in_grace = total_units <= hard.target + budget.grace
        && (!hard.char_limited() || total_chars <= hard.char + LENGTH_GRACE_CHARS);
    let mode = if in_grace {
        DpMode::Quality
    } else {
        DpMode::Force
    };

    let base_cost = compute_base_costs(words, start, end, vad_index, profile, &advisor, &byte_offset);
    // quality_ok[k] == cutting after word start+k-1 is linguistically good.
    let quality_ok: Vec<bool> = (1..n)
        .map(|k| is_quality_cut_boundary(words, start + k - 1, profile, vad_index))
        .collect();

    let mut dp = vec![f64::INFINITY; n + 1];
    let mut prev = vec![0usize; n + 1];
    dp[0] = 0.0;

    let char_based = profile.is_char_based();
    for i in 1..=n {
        // Last segment = words[start+j .. start+i]; scan candidate starts from
        // i-1 downward. Word units are monotonic → break once over max_unit.
        for j in (0..i).rev() {
            let token_count = i - j;
            let seg_units = prefix[i] - prefix[j];
            if token_count > 1 && seg_units > hard.max_unit {
                break;
            }
            let seg_chars = char_of(j, i - 1);
            if !hard.valid(token_count, seg_units, seg_chars) {
                continue;
            }
            if base_cost[j].is_infinite() || dp[j].is_infinite() {
                continue;
            }
            if mode == DpMode::Quality && j > 0 && j < n && !quality_ok[j - 1] {
                continue;
            }
            // Grace is "only split at good cuts", not "pack until 28".
            // If a good interior cut exists, do not keep the whole span.
            if mode == DpMode::Quality
                && j == 0
                && i == n
                && quality_ok.iter().any(|ok| *ok)
            {
                continue;
            }
            let length_penalty =
                LENGTH_PENALTY_WEIGHT * (seg_units - budget.limit).abs() / budget.limit;
            let mut cost = dp[j] + base_cost[j] + length_penalty;
            if hard.char_limited() && hard.char > 0.0 {
                cost += LENGTH_PENALTY_WEIGHT * 0.5 * (seg_chars - hard.char).abs() / hard.char;
            }
            if seg_units > 0.0 && seg_units <= 2.0 {
                cost += SHORT_SEGMENT_PENALTY; // avoid "台" / "风" alone
            }
            // Tie-break by writing system: Latin → prefer earlier j (balanced
            // lines); CJK → prefer later j (fuller first line, cuts land closer
            // to real word boundaries).
            let better = if char_based { cost < dp[i] } else { cost <= dp[i] };
            if better {
                dp[i] = cost;
                prev[i] = j;
            }
        }
    }

    if dp[n].is_infinite() {
        if mode == DpMode::Quality {
            return Some(Vec::new()); // no good cut → keep the whole line
        }
        // Force mode without a DP solution: fall back to greedy first-fit.
        return Some(greedy_cuts_by_hard_limit(
            words,
            start,
            end,
            &prefix,
            &*char_of,
            &hard,
            profile,
        ));
    }

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

    absorb_short_fragments(
        &mut cuts_rel,
        &prefix,
        &*char_of,
        n,
        &hard,
        &|cut_k| should_keep_short_cut(words, start, cut_k, profile),
    );

    if mode == DpMode::Quality && !all_cuts_quality(&cuts_rel, &quality_ok) {
        return Some(Vec::new());
    }

    Some(
        cuts_rel
            .into_iter()
            .map(|k| DpCut {
                index: start + k - 1,
                reason: SplitReason::SubtitleLayout,
            })
            .collect(),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DpMode {
    Quality, // grace band: only good cuts, else keep whole
    Force,
}

fn compute_base_costs(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    vad_index: &SpeechSegmentIndex,
    profile: &dyn LanguageProfile,
    advisor: &Advisor,
    byte_offset: &[usize],
) -> Vec<f64> {
    let n = end - start + 1;
    let mut base_cost = vec![FORBIDDEN_COST; n + 1];
    base_cost[0] = 0.0;
    for k in 1..n {
        base_cost[k] =
            boundary_base_cost(words, start + k - 1, vad_index, profile, advisor, byte_offset[k]);
    }
    base_cost[n] = 0.0;
    base_cost
}

fn all_cuts_quality(cuts_rel: &[usize], quality_ok: &[bool]) -> bool {
    cuts_rel.iter().all(|&k| k > 0 && k - 1 < quality_ok.len() && quality_ok[k - 1])
}

fn peek_word(words: &[WordTokenDto], index: usize) -> &str {
    words.get(index).map(|w| w.word.as_str()).unwrap_or("")
}

fn spoken_end_at(words: &[WordTokenDto], i: usize) -> bool {
    let Some(left) = words.get(i) else {
        return false;
    };
    let prev = if i == 0 { "" } else { peek_word(words, i - 1) };
    is_japanese_spoken_end(prev, &left.word, peek_word(words, i + 1), peek_word(words, i + 2))
}

fn should_keep_short_cut(
    words: &[WordTokenDto],
    start: usize,
    cut_k: usize,
    profile: &dyn LanguageProfile,
) -> bool {
    let left_idx = start + cut_k - 1;
    let right_idx = start + cut_k;
    let Some(left) = words.get(left_idx) else {
        return false;
    };
    let Some(right) = words.get(right_idx) else {
        return false;
    };
    let next2 = peek_word(words, right_idx + 1);
    if spoken_end_at(words, left_idx) {
        return true;
    }
    if is_split_connector_pair(&right.word, next2, profile.connectors()) {
        return true;
    }
    is_connector_like(&right.word, profile.connectors())
        && !is_connector_like(&left.word, profile.connectors())
        && !is_bound_connector(&left.word, &right.word, profile.connectors())
}

/// Merge DP segments at or below [`MIN_FRAGMENT_UNITS`] into an adjacent
/// segment by dropping a cut — but ONLY when the merged segment stays within
/// the hard limits (readability over length precision).
fn absorb_short_fragments(
    cuts_rel: &mut Vec<usize>,
    prefix: &[f64],
    char_of: &dyn Fn(usize, usize) -> f64,
    n: usize,
    hard: &HardLimits,
    keep_cut: &dyn Fn(usize) -> bool,
) {
    // display chars are indexed within the span slice (char_of uses span_words)
    // — same coordinate space as prefix (indexed from span start).
    loop {
        if cuts_rel.is_empty() {
            return;
        }
        let mut bounds: Vec<usize> = vec![0];
        bounds.extend(cuts_rel.iter().copied());
        bounds.push(n);
        let mut absorbed = false;
        for seg_idx in 0..bounds.len() - 1 {
            let a = bounds[seg_idx];
            let b = bounds[seg_idx + 1];
            let units = prefix[b] - prefix[a];
            if units <= 0.0 || units > MIN_FRAGMENT_UNITS {
                continue;
            }
            // Merge into the FOLLOWING segment (drop the cut after this one).
            if seg_idx + 2 < bounds.len() && !keep_cut(b) {
                let c = bounds[seg_idx + 2];
                if hard.valid(c - a, prefix[c] - prefix[a], char_of(a, c - 1))
                    && let Some(ix) = cuts_rel.iter().position(|&k| k == b) {
                    cuts_rel.remove(ix);
                    absorbed = true;
                    break;
                }
            }
            // Merge into the PREVIOUS segment (drop the cut before this one).
            if seg_idx > 0 && !keep_cut(a) {
                let z = bounds[seg_idx - 1];
                if hard.valid(b - z, prefix[b] - prefix[z], char_of(z, b - 1))
                    && let Some(ix) = cuts_rel.iter().position(|&k| k == a) {
                    cuts_rel.remove(ix);
                    absorbed = true;
                    break;
                }
            }
        }
        if !absorbed {
            return;
        }
    }
}

/// Greedy first-fit fallback for the force mode when the DP has no solution.
/// Retreats the cut across function words, Japanese orthographic binds, and
/// time-glued single CJK chars so the fallback never splits a phrase.
fn greedy_cuts_by_hard_limit(
    words: &[WordTokenDto],
    start: usize,
    end: usize,
    prefix: &[f64],
    char_of: &dyn Fn(usize, usize) -> f64,
    hard: &HardLimits,
    profile: &dyn LanguageProfile,
) -> Vec<DpCut> {
    let n = end - start + 1;
    let mut cuts: Vec<usize> = Vec::new();
    let mut seg_start = 0usize;
    let mut i = 1usize;
    while i < n {
        let units = prefix[i + 1] - prefix[seg_start];
        let tokens = i + 1 - seg_start;
        let chars = char_of(seg_start, i);
        // Not an overflow: a single token cannot be split further, and a span
        // the hard limit already accepts needs no cut.
        if tokens <= 1 || hard.valid(tokens, units, chars) {
            i += 1;
            continue;
        }
        // Overflow: keep bunsetsu / kinsoku attachments on this line.
        let overflow_left = words[start + i - 1].word.as_str();
        let overflow_right = words[start + i].word.as_str();
        let overflow_gap = token_gap_sec(
            Some(words[start + i - 1].end),
            Some(words[start + i].start),
        );
        let overflow_next2 = peek_word(words, start + i + 1);
        let emergency = units > hard.max_unit + 8.0;
        let structural_hold = (is_line_start_bound_particle(overflow_right)
            && !is_split_hai(overflow_right, overflow_next2))
            || is_open_genitive_link(overflow_left, overflow_right)
            || is_case_particle_before_predicate(overflow_left, overflow_right)
            || is_japanese_lexical_bind(overflow_left, overflow_right)
            || is_time_glued_content(overflow_left, overflow_right, overflow_gap);
        if structural_hold && !emergency {
            i += 1;
            continue;
        }
        if emergency && is_japanese_orthographic_bind(overflow_left, overflow_right) {
            i += 1;
            continue;
        }
        // Cut back from i while the boundary is structurally bad.
        // Do NOT retreat across open genitive — that would put の on the next
        // line. の stays on the left (kinsoku).
        let mut cut = i;
        let mut glue_steps = 0;
        while cut > seg_start + 1 {
            let left = words[start + cut - 1].word.as_str();
            let right = words[start + cut].word.as_str();
            if is_function_word_left(left, profile.function_words_left())
                || is_japanese_orthographic_bind(left, right)
            {
                cut -= 1;
                continue;
            }
            let glued = is_single_cjk_char_token(left)
                && is_single_cjk_char_token(right)
                && token_gap_sec(
                    Some(words[start + cut - 1].end),
                    Some(words[start + cut].start),
                )
                .map(|g| g <= GLUE_GAP_SEC)
                .unwrap_or(false);
            if glued && glue_steps < 2 {
                cut -= 1;
                glue_steps += 1;
                continue;
            }
            break;
        }
        cuts.push(cut);
        seg_start = cut;
        i = cut;
    }
    cuts
        .into_iter()
        .map(|k| DpCut {
            index: start + k - 1,
            reason: SplitReason::SubtitleLayout,
        })
        .collect()
}

fn is_single_cjk_char_token(token: &str) -> bool {
    let t = strip_token(token);
    let mut chars = t.chars();
    let Some(c) = chars.next() else { return false };
    if chars.next().is_some() {
        return false;
    }
    let v = c as u32;
    (0x3040..=0x30ff).contains(&v)
        || (0x3400..=0x4dbf).contains(&v)
        || (0x4e00..=0x9fff).contains(&v)
        || (0xf900..=0xfaff).contains(&v)
        || (0xac00..=0xd7af).contains(&v)
}

// ---- punctuation / spacing helpers ----

fn ends_with_opening_punctuation(token: &str) -> bool {
    token.trim_end().chars().last().map(is_opening_punctuation).unwrap_or(false)
}

fn starts_with_closing_punctuation(token: &str) -> bool {
    token.trim_start().chars().next().map(is_closing_punctuation).unwrap_or(false)
}

fn ends_with_soft_punctuation(token: &str) -> bool {
    token.trim_end().chars().last().map(is_soft_punctuation).unwrap_or(false)
}

fn is_comma(token: &str) -> bool {
    let ch = token.trim_end().chars().last();
    matches!(ch, Some(',') | Some('，') | Some('、'))
}

/// Display length of the joined segment text (spaces included, Latin style),
/// matching how the subtitle line is rendered.
fn display_chars(words: &[WordTokenDto]) -> f64 {
    super::util::join_words(words.iter().map(|w| w.word.as_str()))
        .chars()
        .count() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_limits_validate_both_axes() {
        let hard = HardLimits {
            target: 2.0,
            max_unit: 2.0,
            char: 12.0,
        };
        // Single tokens may stand alone even over every limit (URLs).
        assert!(hard.valid(1, 5.0, 60.0));
        // Units over → invalid.
        assert!(!hard.valid(2, 5.0, 60.0));
        // Units fine, chars over → invalid.
        assert!(!hard.valid(2, 1.0, 13.0));
        // Both within → valid.
        assert!(hard.valid(2, 1.0, 10.0));
        // No char cap → char axis never blocks.
        let uncapped = HardLimits {
            target: 2.0,
            max_unit: 2.0,
            char: f64::INFINITY,
        };
        assert!(uncapped.valid(2, 1.0, 10_000.0));
        assert!(!uncapped.valid(2, 3.0, 0.0));
    }
}
