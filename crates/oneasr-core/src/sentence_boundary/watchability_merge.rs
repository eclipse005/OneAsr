//! Source-side watchability merge. Runs after DP, before translation, so the
//! translator never sees flash/orphan cues.

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle_length::SubtitleLengthPreset;

use super::boundary_rules::{
    is_case_particle_before_predicate, is_connector_like, is_discourse_marker_text,
    is_function_word_left, is_ja_address_greeting_bind, is_ja_turn_start_after,
    is_japanese_spoken_end,
    is_japanese_lexical_bind, is_line_start_bound_particle, is_open_genitive_link,
    is_split_hai, strip_token,
};
use super::language::LanguageProfile;
use super::text::join_words;

const FLASH_SEC: f64 = 0.8;
const ORPHAN_TAIL_MAX_SEC: f64 = 1.5;
const WATCHABILITY_GAP_SEC: f64 = 0.8;
const MERGE_BUDGET_SEC: f64 = 6.0;
const ORPHAN_TAIL_LATIN_UNITS: f64 = 4.0;
const ORPHAN_TAIL_CJK_UNITS: f64 = 8.0;
const ORPHAN_MERGE_GRACE_UNITS: f64 = 4.0;
const CHARS_PER_WORD_BUDGET: f64 = 5.5;

#[derive(Debug, Clone, Copy)]
struct Cue {
    start: usize,
    end: usize,
}

pub(super) fn merge_watchability_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
    profile: &dyn LanguageProfile,
    preset: SubtitleLengthPreset,
) -> Vec<(usize, usize)> {
    if spans.len() < 2 || words.is_empty() {
        return spans.to_vec();
    }
    let cues: Vec<Cue> = spans
        .iter()
        .copied()
        .filter(|(s, e)| *s < words.len() && *e < words.len() && s <= e)
        .map(|(start, end)| Cue { start, end })
        .collect();
    if cues.len() < 2 {
        return cues.into_iter().map(|c| (c.start, c.end)).collect();
    }

    let hard_limit = f64::from(profile.source_limit(preset));
    let char_limit = profile.source_char_limit(preset);

    let mut pass1: Vec<Cue> = Vec::with_capacity(cues.len());
    let mut i = 0usize;
    while i < cues.len() {
        if i + 1 >= cues.len() {
            pass1.push(cues[i]);
            break;
        }
        let left = cues[i];
        let right = cues[i + 1];
        if can_merge(words, left, right, hard_limit, char_limit, profile) {
            pass1.push(merge_pair(left, right));
            i += 2;
        } else {
            pass1.push(left);
            i += 1;
        }
    }

    let absorbed = absorb_flash(words, pass1, hard_limit, char_limit, profile);
    absorbed.into_iter().map(|c| (c.start, c.end)).collect()
}

fn merge_pair(left: Cue, right: Cue) -> Cue {
    Cue {
        start: left.start,
        end: right.end,
    }
}

fn can_merge(
    words: &[WordTokenDto],
    left: Cue,
    right: Cue,
    unit_cap: f64,
    char_cap: f64,
    profile: &dyn LanguageProfile,
) -> bool {
    if left.end + 1 != right.start {
        return false;
    }
    let gap = cue_start(words, right) - cue_end(words, left);
    let last_word = words[left.end].word.as_str();
    let first_right = words[right.start].word.as_str();
    let next2 = words
        .get(right.start + 1)
        .map(|w| w.word.as_str())
        .unwrap_or("");
    // Clitic の / bound particle starting the next cue: glue back even if the
    // left cue already has terminal punctuation (ドラクエ。の進みか).
    // ます|は|い… is はい, not a particle that should glue backward.
    if is_line_start_bound_particle(first_right) && !is_split_hai(first_right, next2) {
        if gap > ORPHAN_TAIL_MAX_SEC {
            return false;
        }
        if cue_end(words, right) - cue_start(words, left) > MERGE_BUDGET_SEC {
            return false;
        }
        return !pair_exceeds_caps(
            words,
            left,
            right,
            profile,
            unit_cap,
            char_cap,
            profile.length_grace_units(),
        );
    }
    if is_japanese_lexical_bind(last_word, first_right) {
        if gap > ORPHAN_TAIL_MAX_SEC {
            return false;
        }
        if cue_end(words, right) - cue_start(words, left) > MERGE_BUDGET_SEC {
            return false;
        }
        return !pair_exceeds_caps(
            words,
            left,
            right,
            profile,
            unit_cap,
            char_cap,
            profile.length_grace_units(),
        );
    }
    // Spoken です/ます is a sentence end even without 。. A Japanese
    // turn-taker starting the next cue is a new move. Do not reglue either.
    let prev = if left.end == 0 {
        ""
    } else {
        words[left.end - 1].word.as_str()
    };
    if is_japanese_spoken_end(prev, last_word, first_right, next2) {
        return false;
    }
    if profile.key() == "ja"
        && (is_ja_turn_start_after(last_word, first_right, next2)
            || (is_connector_like(first_right, profile.connectors())
                && !is_connector_like(last_word, profile.connectors())))
        && !is_ja_address_greeting_bind(last_word, first_right)
    {
        return false;
    }
    if gap > WATCHABILITY_GAP_SEC {
        return false;
    }
    if cue_end(words, right) - cue_start(words, left) > MERGE_BUDGET_SEC {
        return false;
    }
    let left_text = cue_text(words, left);
    if is_closed_sentence_pair(words, left, right, profile, &left_text) {
        return false;
    }

    let right_orphan = is_orphan_tail(words, right, profile) && !is_orphan_tail(words, left, profile);
    if right_orphan {
        return !pair_exceeds_caps(
            words,
            left,
            right,
            profile,
            unit_cap,
            char_cap,
            ORPHAN_MERGE_GRACE_UNITS,
        );
    }
    let left_orphan = is_orphan_tail(words, left, profile) && !is_orphan_tail(words, right, profile);
    if left_orphan {
        let grace = if cue_duration(words, right) >= FLASH_SEC {
            ORPHAN_MERGE_GRACE_UNITS
        } else {
            0.0
        };
        return !pair_exceeds_caps(words, left, right, profile, unit_cap, char_cap, grace);
    }

    if is_open_genitive_link(last_word, first_right)
        || is_case_particle_before_predicate(last_word, first_right)
        || is_function_word_left(last_word, profile.function_words_left())
    {
        let right_units = cue_units(words, right, profile);
        let right_cap = if profile.is_char_based() { 12.0 } else { 8.0 };
        if right_units > 0.0 && right_units <= right_cap {
            return !pair_exceeds_caps(
                words,
                left,
                right,
                profile,
                unit_cap,
                char_cap,
                ORPHAN_MERGE_GRACE_UNITS,
            );
        }
    }

    if is_line_end_connector_token(last_word, profile)
        && is_continuation_token(words[right.start].word.as_str(), profile)
    {
        return !pair_exceeds_caps(
            words,
            left,
            right,
            profile,
            unit_cap,
            char_cap,
            ORPHAN_MERGE_GRACE_UNITS,
        );
    }

    false
}

fn is_line_end_connector_token(token: &str, profile: &dyn LanguageProfile) -> bool {
    let t = strip_token(token).to_lowercase();
    if t.is_empty() {
        return false;
    }
    const LATIN: &[&str] = &[
        "and", "or", "to", "for", "with", "that", "which", "when", "if", "but", "so",
    ];
    const CJK: &[&str] = &[
        "然后", "而且", "并且", "因为", "所以", "但是", "如果", "为了", "以及", "还有", "并",
        "和", "与", "及", "或", "来", "去", "在", "对", "把", "将", "大约", "这", "那", "这个",
        "那个", "一个",
    ];
    LATIN.contains(&t.as_str())
        || CJK.contains(&t.as_str())
        || is_connector_like(token, profile.connectors())
}

fn is_continuation_token(token: &str, profile: &dyn LanguageProfile) -> bool {
    let t = strip_token(token).to_lowercase();
    if t.is_empty() {
        return false;
    }
    const LATIN: &[&str] = &[
        "a", "an", "the", "to", "of", "and", "or", "with", "for", "this", "that", "if", "so",
        "then", "while", "it", "you", "we", "they",
    ];
    const CJK: &[&str] = &[
        "个", "这个", "那个", "这", "那", "然后", "并且", "而且", "而", "并", "因为", "所以",
        "如果", "还", "继续", "将", "与", "和",
    ];
    LATIN.contains(&t.as_str())
        || CJK.contains(&t.as_str())
        || is_connector_like(token, profile.connectors())
}

fn can_flash_merge(
    words: &[WordTokenDto],
    left: Cue,
    right: Cue,
    unit_cap: f64,
    char_cap: f64,
    profile: &dyn LanguageProfile,
) -> bool {
    if left.end + 1 != right.start {
        return false;
    }
    let gap = cue_start(words, right) - cue_end(words, left);
    if gap > WATCHABILITY_GAP_SEC {
        return false;
    }
    if cue_end(words, right) - cue_start(words, left) > MERGE_BUDGET_SEC {
        return false;
    }
    let left_text = cue_text(words, left);
    if is_closed_sentence_pair(words, left, right, profile, &left_text) {
        return false;
    }
    let last_word = words[left.end].word.as_str();
    let first_right = words[right.start].word.as_str();
    let next2 = words
        .get(right.start + 1)
        .map(|w| w.word.as_str())
        .unwrap_or("");
    let prev = if left.end == 0 {
        ""
    } else {
        words[left.end - 1].word.as_str()
    };
    if is_japanese_spoken_end(prev, last_word, first_right, next2) {
        return false;
    }
    if is_japanese_lexical_bind(last_word, first_right) {
        return !pair_exceeds_caps(
            words,
            left,
            right,
            profile,
            unit_cap,
            char_cap,
            profile.length_grace_units(),
        );
    }
    if profile.key() == "ja"
        && (is_ja_turn_start_after(last_word, first_right, next2)
            || (is_connector_like(first_right, profile.connectors())
                && !is_connector_like(last_word, profile.connectors())))
        && !is_ja_address_greeting_bind(last_word, first_right)
    {
        return false;
    }
    let grace = if is_orphan_tail(words, right, profile) && !is_orphan_tail(words, left, profile) {
        ORPHAN_MERGE_GRACE_UNITS
    } else {
        0.0
    };
    !pair_exceeds_caps(words, left, right, profile, unit_cap, char_cap, grace)
}

fn absorb_flash(
    words: &[WordTokenDto],
    cues: Vec<Cue>,
    unit_cap: f64,
    char_cap: f64,
    profile: &dyn LanguageProfile,
) -> Vec<Cue> {
    if cues.len() < 2 {
        return cues;
    }
    let mut out: Vec<Cue> = Vec::with_capacity(cues.len());
    let mut i = 0usize;
    while i < cues.len() {
        let cur = cues[i];
        let dur = cue_duration(words, cur);
        if dur >= FLASH_SEC || i + 1 == cues.len() {
            if dur < FLASH_SEC
                && let Some(prev) = out.last().copied()
                && can_flash_merge(words, prev, cur, unit_cap, char_cap, profile)
            {
                let last = out.len() - 1;
                out[last] = merge_pair(prev, cur);
                i += 1;
                continue;
            }
            out.push(cur);
            i += 1;
            continue;
        }
        let next = cues[i + 1];
        if can_flash_merge(words, cur, next, unit_cap, char_cap, profile) {
            let merged = merge_pair(cur, next);
            if cue_duration(words, merged) < FLASH_SEC
                && let Some(prev) = out.last().copied()
                && can_flash_merge(words, prev, merged, unit_cap, char_cap, profile)
            {
                let last = out.len() - 1;
                out[last] = merge_pair(prev, merged);
                i += 2;
                continue;
            }
            out.push(merged);
            i += 2;
            continue;
        }
        if let Some(prev) = out.last().copied()
            && can_flash_merge(words, prev, cur, unit_cap, char_cap, profile)
        {
            let last = out.len() - 1;
            out[last] = merge_pair(prev, cur);
            i += 1;
            continue;
        }
        out.push(cur);
        i += 1;
    }
    out
}

fn is_closed_sentence_pair(
    words: &[WordTokenDto],
    left: Cue,
    right: Cue,
    profile: &dyn LanguageProfile,
    left_text: &str,
) -> bool {
    let Some(last) = left_text.chars().last() else {
        return false;
    };
    if !is_sentence_close_char(last) {
        return false;
    }
    if is_short_interjection(words, left, profile) {
        return false;
    }
    let right_units = cue_units(words, right, profile);
    let afterthought = right_units > 0.0 && right_units <= if profile.is_char_based() { 4.0 } else { 2.0 };
    if afterthought || is_short_interjection(words, right, profile) {
        return false;
    }
    true
}

fn is_short_interjection(words: &[WordTokenDto], cue: Cue, profile: &dyn LanguageProfile) -> bool {
    let units = cue_units(words, cue, profile);
    let max = if profile.is_char_based() { 6.0 } else { 2.0 };
    if units <= 0.0 || units > max {
        return false;
    }
    let text = cue_text(words, cue);
    is_discourse_marker_text(&text) || text.chars().last().is_some_and(is_sentence_close_char)
}

fn is_orphan_tail(words: &[WordTokenDto], cue: Cue, profile: &dyn LanguageProfile) -> bool {
    let dur = cue_duration(words, cue);
    if dur <= 0.0 || dur > ORPHAN_TAIL_MAX_SEC {
        return false;
    }
    let units = cue_units(words, cue, profile);
    let cap = if profile.is_char_based() {
        ORPHAN_TAIL_CJK_UNITS
    } else {
        ORPHAN_TAIL_LATIN_UNITS
    };
    units > 0.0 && units <= cap
}

fn pair_exceeds_caps(
    words: &[WordTokenDto],
    left: Cue,
    right: Cue,
    profile: &dyn LanguageProfile,
    unit_cap: f64,
    char_cap: f64,
    unit_grace: f64,
) -> bool {
    let pair_units = cue_units(words, left, profile) + cue_units(words, right, profile);
    let token_count = right.end - left.start + 1;
    if token_count > 1 && pair_units > unit_cap + unit_grace {
        return true;
    }
    if char_cap.is_finite() {
        let merged = join_words(words[left.start..=right.end].iter().map(|w| w.word.as_str()));
        let char_grace = if unit_grace > 0.0 {
            (unit_grace * CHARS_PER_WORD_BUDGET).round()
        } else {
            0.0
        };
        if merged.chars().count() as f64 > char_cap + char_grace {
            return true;
        }
    }
    false
}

fn is_sentence_close_char(ch: char) -> bool {
    matches!(
        ch,
        '.' | '!' | '?' | ';' | '。' | '！' | '？' | '；' | '，' | '、' | ','
    )
}

fn cue_text(words: &[WordTokenDto], cue: Cue) -> String {
    join_words(words[cue.start..=cue.end].iter().map(|w| w.word.as_str()))
}

fn cue_units(words: &[WordTokenDto], cue: Cue, profile: &dyn LanguageProfile) -> f64 {
    words[cue.start..=cue.end]
        .iter()
        .map(|w| profile.token_units(&w.word))
        .sum()
}

fn cue_start(words: &[WordTokenDto], cue: Cue) -> f64 {
    words[cue.start].start
}

fn cue_end(words: &[WordTokenDto], cue: Cue) -> f64 {
    words[cue.end].end.max(words[cue.end].start)
}

fn cue_duration(words: &[WordTokenDto], cue: Cue) -> f64 {
    (cue_end(words, cue) - cue_start(words, cue)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(index: usize, text: &str) -> WordTokenDto {
        let start = index as f64 * 0.5;
        WordTokenDto {
            start,
            end: start + 0.3,
            word: text.to_string(),
        }
    }

    fn timed(text: &str, start: f64, end: f64) -> WordTokenDto {
        WordTokenDto {
            start,
            end,
            word: text.to_string(),
        }
    }

    #[test]
    fn english_orphan_tail_glues_to_previous_line() {
        let mut words: Vec<WordTokenDto> = (0..16)
            .map(|i| timed("word", i as f64 * 0.25, i as f64 * 0.25 + 0.2))
            .collect();
        words.extend([
            timed("or", 4.0, 4.2),
            timed("extra", 4.2, 4.4),
            timed("bits", 4.4, 4.6),
        ]);
        let spans = vec![(0usize, 15), (16, 18)];
        let profile = super::super::language::profile_for_lang("en");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged, vec![(0, 18)]);
    }

    #[test]
    fn period_blocks_orphan_merge_unless_afterthought() {
        let mut words: Vec<WordTokenDto> = (0..7)
            .map(|i| timed("word", i as f64 * 0.25, i as f64 * 0.25 + 0.2))
            .collect();
        words.push(timed("end.", 1.75, 2.0));
        words.extend([
            timed("or", 2.05, 2.25),
            timed("extra", 2.25, 2.45),
            timed("bits", 2.45, 2.65),
        ]);
        let spans = vec![(0usize, 7), (8, 10)];
        let profile = super::super::language::profile_for_lang("en");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn okay_interjection_glues_to_next_line() {
        let words = vec![
            timed("Okay.", 0.0, 0.2),
            timed("You", 0.28, 0.5),
            timed("have", 0.5, 0.7),
            timed("to", 0.7, 0.9),
            timed("go.", 0.9, 1.1),
        ];
        let spans = vec![(0usize, 0), (1, 4)];
        let profile = super::super::language::profile_for_lang("en");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged, vec![(0, 4)]);
    }

    #[test]
    fn japanese_desu_clause_is_not_merged_into_next() {
        let words = vec![
            timed("私は", 0.0, 0.3),
            timed("学生です", 0.3, 0.7),
            timed("今日から", 0.72, 1.1),
            timed("新しい", 1.1, 1.4),
            timed("学校に", 1.4, 1.8),
            timed("通います", 1.8, 2.3),
        ];
        let spans = vec![(0usize, 1), (2, 5)];
        let profile = super::super::language::profile_for_lang("ja");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged.len(), 2, "です must not reglue: {merged:?}");
    }

    #[test]
    fn japanese_hai_turn_is_not_merged_backward() {
        let words = vec![
            timed("説明は", 0.0, 0.4),
            timed("ここまで", 0.4, 0.9),
            timed("はい", 0.95, 1.15),
            timed("次は", 1.15, 1.45),
            timed("質問を", 1.45, 1.8),
            timed("どうぞ", 1.8, 2.2),
        ];
        let spans = vec![(0usize, 1), (2, 5)];
        let profile = super::super::language::profile_for_lang("ja");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged.len(), 2, "はい turn must not reglue: {merged:?}");
    }

    #[test]
    fn wide_gap_does_not_merge() {
        let words = vec![w(0, "hello"), w(1, "there"), w(8, "later")];
        // w(8) starts at 4.0s, w(1) ends at 0.8 — gap >> 0.8s
        let spans = vec![(0usize, 1), (2, 2)];
        let profile = super::super::language::profile_for_lang("en");
        let merged = merge_watchability_spans(
            &words,
            &spans,
            &*profile,
            SubtitleLengthPreset::Standard,
        );
        assert_eq!(merged.len(), 2);
    }
}
