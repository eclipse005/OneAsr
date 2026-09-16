//! Sentence-boundary tests: the corpus of transcript shapes the pipeline has to
//! survive.

use super::{
    BoundaryDecisionKind, build_micro_chunks, build_source_sentences_from_words,
};
use super::WordTokenDto;
use crate::subtitle::text_rules::ends_with_terminal_punctuation;

/// Deterministic spans for the span-shape tests: semantic pre-split only, with
/// no DP layout. Test-only helper, deliberately kept out of the production
/// module so `mod.rs` holds nothing `#[cfg(test)]`-gated.
fn build_deterministic_sentence_spans(words: &[WordTokenDto]) -> Vec<(usize, usize)> {
    let split_points = super::semantic::build_deterministic_split_points(words);
    super::semantic::split_points_to_spans(words.len(), &split_points)
}

fn w(index: usize, text: &str) -> WordTokenDto {
    let start = index as f64 * 0.5;
    WordTokenDto {
        start,
        end: start + 0.3,
        word: text.to_string(),
    }
}

/// Mirrors the production `MIN_FRAGMENT_UNITS` floor (boundary_rules.rs).
const MIN_FRAGMENT_WORDS: usize = 3;

fn request(words: Vec<WordTokenDto>) -> super::SentenceBoundaryRequest {
    request_with_lang_and_preset(words, "en", "standard")
}

fn request_with_lang_and_preset(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
) -> super::SentenceBoundaryRequest {
    request_with_lang_preset_and_layout(words, source_lang, subtitle_length_preset)
}

fn request_with_lang_preset_and_layout(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
) -> super::SentenceBoundaryRequest {
    request_with_vad(words, source_lang, subtitle_length_preset, Vec::new())
}

fn request_with_vad(
    words: Vec<WordTokenDto>,
    source_lang: &str,
    subtitle_length_preset: &str,
    vad_speech_segments: Vec<(f64, f64)>,
) -> super::SentenceBoundaryRequest {
    super::SentenceBoundaryRequest {
        task_id: "task-1".to_string(),
        media_path: "demo.mp4".to_string(),
        source_lang: source_lang.to_string(),
        subtitle_length_preset: subtitle_length_preset.to_string(),
        words,
        vad_speech_segments,
    }
}

#[test]
fn deterministic_spans_split_on_terminal_punctuation() {
    let words = vec![
        w(0, "Hello"),
        w(1, "world."),
        w(2, "Next"),
        w(3, "sentence?"),
    ];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 1), (2, 3)]);
}

#[test]
fn overlong_terminal_sentence_stays_intact_until_terminal_punctuation() {
    let words = "All right, in this video, we're going to be talking about daily review habits and how they affect your focus and your planning mindset."
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, words.len() - 1)]);
}

#[test]
fn soft_punctuation_does_not_create_extra_step2_split() {
    let words = (0..45)
        .map(|index| {
            let token = if index == 29 { "checkpoint," } else { "word" };
            w(index, token)
        })
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 44)]);
}

#[test]
fn duration_fallback_does_not_split_without_hard_pause() {
    let words = (0..30)
        .map(|index| WordTokenDto {
            start: index as f64,
            end: index as f64 + 0.2,
            word: format!("w{index}"),
        })
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 29)]);
}

#[test]
fn long_unpunctuated_runs_stay_intact_without_hard_pause() {
    let words = (0..45)
        .map(|index| w(index, &format!("w{index}")))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 44)]);
}

#[test]
fn long_missing_punctuation_span_stays_intact_for_later_llm_layout() {
    let text = "It's something I've been trying to do every week just to get a good idea of how I'm performing against the reference list of literally reviewing every high quality example that I see because sometimes your execution slips, you might skip examples due to hesitation or maybe you choose weaker examples because you're not thinking straight.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let spans = build_deterministic_sentence_spans(&words);
    let texts = spans
        .iter()
        .map(|(start, end)| {
            super::join_words(words[*start..=*end].iter().map(|word| word.word.as_str()))
        })
        .collect::<Vec<_>>();

    assert_eq!(texts, vec![text.to_string()]);
}

#[test]
fn terminal_punctuation_still_splits_long_runs() {
    let text = "This long sentence has no useful internal punctuation it keeps running through several separate ideas the recognizer only produced a final period";
    let mut words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();
    words[6].word = "punctuation.".to_string();

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 6), (7, words.len() - 1)]);
}

#[test]
fn broad_terminal_punctuation_splits_step2_sentences() {
    let words = vec![w(0, "你好．"), w(1, "Next⁉"), w(2, "Again")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 0), (1, 1), (2, 2)]);
}

#[test]
fn abbreviation_terminal_punctuation_does_not_split_step2_sentence() {
    let words = vec![w(0, "Mr."), w(1, "Smith"), w(2, "arrived.")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2)]);
}

#[test]
fn single_letter_enumeration_token_forces_step2_split() {
    // Reproduces the "step one B. So ..." regression: a single-letter dotted
    // token is NOT a name initial — it's a spoken enumeration end. The `.`
    // must force a split, and the following capitalized sentence becomes its
    // own span.
    let words = vec![
        w(0, "step"),
        w(1, "one"),
        w(2, "B."),
        w(3, "So"),
        w(4, "let's"),
        w(5, "go."),
    ];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2), (3, 5)]);
}

#[test]
fn consecutive_single_letter_initials_chain_only_protects_internal_pairs() {
    // "J. K. Rowling" — the chain J.→K. is protected (both single-letter), so
    // no split between them. But K.→Rowling is NOT protected (Rowling is a
    // normal word), so K. is treated as an isolated terminal and splits.
    // This is the intended design: single-letter protection is pairwise, so
    // the *internal* bond of an initial chain holds, but a trailing isolated
    // initial still acts as a sentence end. Real ASR rarely emits initial
    // chains, so the common case ("step one B.") splits correctly.
    let words = vec![w(0, "J."), w(1, "K."), w(2, "Rowling")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 1), (2, 2)]);
}

#[test]
fn short_unpunctuated_fragment_merges_into_next_punctuated_sentence() {
    let words = vec![w(0, "well"), w(1, "let's"), w(2, "start.")];

    let spans = build_deterministic_sentence_spans(&words);

    assert_eq!(spans, vec![(0, 2)]);
}

#[test]
fn hard_pause_does_not_split_short_sentence_without_punctuation() {
    // VAD detects a silence gap, but the sentence is short (2 words, well under
    // the length budget). After the DP rewrite, a sentence under budget is
    // NEVER split even with a VAD pause — preventing the Orderblock-style
    // mid-sentence fragmentation.
    let words = vec![
        WordTokenDto {
            start: 0.0,
            end: 0.2,
            word: "Okay".to_string(),
        },
        WordTokenDto {
            start: 2.4,
            end: 2.7,
            word: "next".to_string(),
        },
    ];

    // semantic.rs no longer hard-splits on VAD (only terminal punctuation).
    let split_points = super::semantic::build_deterministic_split_points(&words);
    assert!(
        split_points.is_empty(),
        "short sentence with VAD pause must not be hard-split"
    );
    let spans = super::split_points_to_spans(words.len(), &split_points);
    assert_eq!(spans, vec![(0, 1)]);
}

/// Ported EggTranslate guard: Japanese phrase-closing particle (は) is a good
/// cut, so an overlong unpunctuated Japanese span splits after the particle.
#[test]
fn japanese_particle_guides_force_cuts() {
    // Short cap 16, JA bunsetsu grace +6 → force only when over 22.
    // This span is well over 22, so DP still prefers particle cuts.
    let tokens = [
        "それ",
        "は",
        "とても",
        "面白い",
        "話で",
        "本当に",
        "感動した",
        "昨日から",
        "何度も",
        "繰り返し",
        "考えている",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();

    let ja_profile = super::profile::profile_for_lang("ja");
    let short_preset = super::preset::SubtitleLengthPreset::Short;
    let idx = super::vad_align::SpeechSegmentIndex::new(Vec::new());
    let semantic = vec![(0usize, words.len() - 1)];
    let dp_cuts = super::subtitle_layout::build_subtitle_layout_split_points(
        &words,
        &semantic,
        &*ja_profile,
        short_preset,
        &idx,
    );
    assert!(
        !dp_cuts.is_empty(),
        "DP must cut an overlong Japanese span at a particle"
    );
    for (index, _) in &dp_cuts {
        let last = tokens[*index].chars().last().unwrap_or_default();
        assert!(
            matches!(last, 'は' | 'が' | 'を' | 'に' | 'の' | 'と' | 'で' | 'も' | 'へ' | 'や'),
            "DP cut after {:?} is not a phrase-close particle",
            tokens[*index]
        );
    }

    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "short"))
    .expect("step2 should build japanese sentences");

    // A 3-char first piece ("それは") is an orphan flash line; source-side
    // watchability may glue it back. Final cues must stay within the cap.
    let joined = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(
        joined,
        "それはとても面白い話で本当に感動した昨日から何度も繰り返し考えている"
    );
    let n = response.translation_sentences.len();
    for (idx, s) in response.translation_sentences.iter().enumerate() {
        // Watchability may re-glue a 3-char particle orphan, using the same
        // grace band DP already allowed (short 16 + 2).
        assert!(s.text.chars().count() <= 22, "cue over cap: {:?}", s.text);
        if idx + 1 < n {
            let last = s.text.chars().last().unwrap_or_default();
            assert!(
                matches!(
                    last,
                    'は' | 'が' | 'を' | 'に' | 'の' | 'と' | 'で' | 'も' | 'へ' | 'や'
                ),
                "cut should follow a particle: {:?}",
                s.text
            );
        }
    }
}

/// の is a linker to the following head. Force-cut must not split
/// `新世代の | 選手たち` even when the span is over the short cap.
#[test]
fn japanese_open_genitive_stays_with_head_noun() {
    let tokens = [
        "とても",
        "可愛い",
        "新世代の",
        "選手たち",
        "が",
        "今日",
        "試合",
        "で",
        "活躍した",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "short"))
    .expect("step2 should build japanese sentences");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("");
    assert_eq!(joined, "とても可愛い新世代の選手たちが今日試合で活躍した");
    for (i, text) in texts.iter().enumerate() {
        if text.ends_with('の') {
            let next = texts.get(i + 1).copied().unwrap_or("");
            assert!(
                next.is_empty()
                    || next.starts_with('は')
                    || next.starts_with('が')
                    || next.starts_with('を')
                    || next.starts_with('に')
                    || next.starts_with('で'),
                "open genitive split: {text:?} | {next:?}"
            );
        }
    }
    assert!(
        texts.iter().any(|t| t.contains("新世代の選手")),
        "head noun must stay with の, got {texts:?}"
    );
    for text in &texts {
        let trimmed = text.trim_start();
        assert!(
            !trimmed.starts_with('の')
                && !trimmed.starts_with('は')
                && !trimmed.starts_with('が')
                && !trimmed.starts_with('を'),
            "cue must not start with a bound particle: {text:?}"
        );
    }
}

#[test]
fn japanese_does_not_start_cue_with_no() {
    let tokens = [
        "とても",
        "可愛い",
        "Z世代",
        "の",
        "選手",
        "たちが",
        "番組",
        "徹底分析",
        "で挑む",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    for text in &texts {
        assert!(
            !text.trim_start().starts_with('の'),
            "line-start の: {texts:?}"
        );
    }
    let joined = texts.join("");
    assert!(joined.contains("Z世代の"), "の should stay with the left or the NP, got {texts:?}");
}

#[test]
fn japanese_katakana_name_run_does_not_split() {
    let tokens = [
        "世界一",
        "可愛い",
        "プリンセス",
        "キャニオン",
        "です",
        "皆さん",
        "こんにちは",
        "よろしくお願いします",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "short"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("プリンセス|") && texts.iter().any(|t| t.contains("プリンセスキャニオン")),
        "katakana run split: {texts:?}"
    );
}

#[test]
fn japanese_splits_after_desu_masu_instead_of_packing() {
    let tokens = [
        "私は",
        "毎日",
        "学校に",
        "行きます",
        "それから",
        "友達と",
        "一緒に",
        "昼ご飯を",
        "食べます",
        "本当に",
        "楽しいです",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        texts.len() >= 2,
        "です/ます should split a spoken paragraph: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.ends_with("行きます") || t.ends_with("ます")),
        "a cue should end at ます: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("行きますそれから")),
        "must not pack across ます: {texts:?}"
    );
}

#[test]
fn japanese_watchability_does_not_reglue_desu_into_next_clause() {
    // Short です cue (orphan-sized) must not be glued onto the next sentence.
    let words = vec![
        WordTokenDto { start: 0.0, end: 0.3, word: "私は".into() },
        WordTokenDto { start: 0.3, end: 0.7, word: "学生です".into() },
        WordTokenDto { start: 0.72, end: 1.1, word: "今日から".into() },
        WordTokenDto { start: 1.1, end: 1.4, word: "新しい".into() },
        WordTokenDto { start: 1.4, end: 1.8, word: "学校に".into() },
        WordTokenDto { start: 1.8, end: 2.3, word: "通います".into() },
    ];
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("学生です今日")),
        "です clause must stay split: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.ends_with("です")),
        "a cue should end at です: {texts:?}"
    );
}

#[test]
fn japanese_hou_ga_ii_stays_together() {
    let tokens = [
        "もっと",
        "早めた",
        "方が",
        "いい",
        "と",
        "思います",
        "それから",
        "みんなで",
        "準備を",
        "始めましょう",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("方が|いい") && texts.iter().any(|t| t.contains("方がいい")),
        "方がいい must stay one predicate: {texts:?}"
    );
}

#[test]
fn japanese_splits_before_minasan_even_when_short() {
    let tokens = ["続々", "登場", "皆", "さん", "こんにちは"];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("登場皆さん")),
        "皆さん starts a new move even under the cap: {texts:?}"
    );
}

#[test]
fn japanese_does_not_pack_ano_after_masu() {
    let tokens = ["聞い", "た", "こと", "あり", "ます", "あの", "木原", "さん"];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("ますあの")),
        "ます must close before あの: {texts:?}"
    );
}

#[test]
fn japanese_splits_before_minasan_turn() {
    let tokens = [
        "今夜",
        "新キャラクター",
        "続々",
        "登場",
        "皆さん",
        "こんにちは",
        "よろしくお願いします",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("登場皆さん")),
        "must split before 皆さん: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.starts_with("皆さん") || t.starts_with("こんにちは")),
        "address/greeting should start a cue: {texts:?}"
    );
}

#[test]
fn japanese_node_stays_on_the_previous_line() {
    let tokens = [
        "今日は",
        "雨",
        "なので",
        "試合は",
        "中止になりました",
        "次は",
        "室内で",
        "練習します",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    for text in &texts {
        assert!(
            !text.trim_start().starts_with("なので") && !text.trim_start().starts_with("ので"),
            "ので/なので must not start a cue: {texts:?}"
        );
    }
}

#[test]
fn japanese_time_glue_does_not_hide_hai_turn() {
    // 0ms between ぜひ and はい must not pack the new move onto the previous line.
    let tokens = [
        "今夜",
        "新情報",
        "盛り",
        "だくさん",
        "シンデレラ",
        "参戦",
        "ぜひ",
        "はい",
        "見て",
        "ほしい",
        "です",
        "可愛い",
        "選手が",
        "登場します",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| WordTokenDto {
            start: i as f64 * 0.08,
            end: i as f64 * 0.08,
            word: (*t).to_string(),
        })
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("盛り|だくさん") && texts.iter().any(|t| t.contains("盛りだくさん")),
        "盛りだくさん must stay one word: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("ぜひはい")),
        "はい turn must split: {texts:?}"
    );
}

#[test]
fn japanese_suru_compound_and_minasan_stay_together() {
    let tokens = [
        "思い当たる",
        "人対",
        "する",
        "クイーンは",
        "続々",
        "登場",
        "皆",
        "さん",
        "こんにちは",
        "よろしくお願いします",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("対|する") && texts.iter().any(|t| t.contains("対する")),
        "対する must stay one verb: {texts:?}"
    );
    assert!(
        !joined.contains("皆|さん") && texts.iter().any(|t| t.contains("皆さん")),
        "皆さん must stay one address: {texts:?}"
    );
}

#[test]
fn japanese_masu_ha_i_is_hai_not_particle() {
    let tokens = [
        "紹介",
        "したい",
        "と",
        "思っ",
        "て",
        "ます",
        "は",
        "いじゃあ",
        "ぜひ",
        "お願いします",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        texts.iter().any(|t| t.ends_with("ます") || t.ends_with("てます")),
        "ます should close the clause: {texts:?}"
    );
    assert!(
        !texts.iter().any(|t| t.contains("ますは")),
        "は of はい must not stick to ます: {texts:?}"
    );
}

#[test]
fn japanese_split_mashita_closes_the_clause() {
    let tokens = [
        "始まり",
        "まし",
        "た",
        "ラスト",
        "コール",
        "皆",
        "さん",
        "よろしく",
        "お願いします",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert!(
        !texts.iter().any(|t| t.contains("ましたラスト")),
        "ました must close before ラストコール: {texts:?}"
    );
    assert!(
        texts.iter().any(|t| t.contains("始まりました") || t.ends_with("た")),
        "始まりました should stay one copula: {texts:?}"
    );
}

#[test]
fn japanese_desu_ka_stays_together() {
    let tokens = [
        "今日は",
        "いい",
        "天気",
        "です",
        "か",
        "そう",
        "です",
        "ね",
        "これから",
        "出かけます",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("です|か") && texts.iter().any(|t| t.contains("ですか")),
        "ですか is one question: {texts:?}"
    );
}

#[test]
fn japanese_te_kudasai_stays_together() {
    let tokens = [
        "ちょっと",
        "待って",
        "ください",
        "それから",
        "詳しく",
        "説明します",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("|");
    assert!(
        !joined.contains("待って|ください") && texts.iter().any(|t| t.contains("待ってください")),
        "てください is one request: {texts:?}"
    );
}

#[test]
fn asr_split_digits_are_glued_before_layout() {
    let words = ["自称", "1", "4歳", "です"]
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "ja", "standard"))
    .expect("step2");
    let joined = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    assert_eq!(joined, "自称14歳です");
    assert!(!joined.contains("1 4"));
}

/// VAD quality cut inside the grace band: 13 words (≤ 12+2) with a real
/// VAD pause → quality mode splits at the pause instead of keeping whole.
#[test]
fn vad_pause_is_a_quality_cut_in_grace_band() {
    let mut words: Vec<WordTokenDto> = (0..13)
        .map(|i| {
            let t = i as f64;
            let (start, end) = if i < 7 {
                (t, t + 0.4)
            } else {
                (t + 1.5, t + 1.9)
            };
            WordTokenDto {
                start,
                end,
                word: format!("w{i}"),
            }
        })
        .collect();
    // Give the DP a non-function-word token list and a VAD silence [6.4, 8.5].
    for wd in words.iter_mut() {
        wd.word = "word".to_string();
    }
    let response = build_source_sentences_from_words(
        request_with_vad(
            words,
            "en",
            "short",
            vec![(0.0, 6.4), (8.5, 14.4)],
        ))
    .expect("step2 should build sentences");

    assert_eq!(
        response.sentence_total,
        2,
        "13-word grace span with a VAD pause should split at the pause"
    );
    assert_eq!(response.translation_sentences[0].text.split_whitespace().count(), 7);
}

/// Grace band without any good cut keeps the whole (slightly over) line —
/// readability over length precision (ported EggTranslate behavior).
#[test]
fn grace_band_keeps_whole_line_without_good_cut() {
    // 13 words (≤ 12 + 2 grace), no punctuation/connectors/pauses → quality
    // mode has no good cut → the whole line stays intact.
    let tokens = [
        "alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa",
        "lambda", "mu", "nu",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build sentences");

    assert_eq!(response.sentence_total, 1);
}

/// Force-mode cuts must never land right after a function word, even when the
/// length budget demands a split ("I want to go to the market | and buy...").
#[test]
fn force_cuts_never_dangle_function_words() {
    // 13 words within grace (14): quality mode; the connector "and" gives a
    // good cut before it; cutting after "to"/"the"/"market" must NOT occur.
    let tokens = [
        "I", "want", "to", "go", "to", "the", "market", "and", "buy", "some", "fresh",
        "vegetables", "today",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build sentences");

    assert_eq!(response.sentence_total, 2);
    assert_eq!(response.translation_sentences[0].text, "I want to go to the market");
    assert_eq!(
        response.translation_sentences[1].text,
        "and buy some fresh vegetables today"
    );
    for s in &response.translation_sentences {
        let last = s.text.split_whitespace().last().unwrap_or("");
        assert!(
            !["I", "want", "to", "go", "the", "and", "buy", "some", "fresh"]
                .contains(&last),
            "cue ends with a function word: {:?}",
            s.text
        );
    }
}

#[test]
fn step2_builds_same_response_shape_without_llm_settings() {
    let words = vec![w(0, "Hello"), w(1, "world."), w(2, "Again.")];

    let response = build_source_sentences_from_words(
        request(words))
    .expect("step2 should not require llm settings");

    // "Again." is a 0.3s orphan tail after "Hello world."; source-side
    // watchability glues it so it does not flash on screen.
    assert_eq!(response.sentence_total, 1);
    assert_eq!(
        response.translation_sentences[0].text,
        "Hello world. Again."
    );
    assert_eq!(response.boundary_total, 2);
    assert!(
        response
            .boundaries
            .iter()
            .all(|b| b.final_decision == BoundaryDecisionKind::Merge),
        "merged orphan tail should not keep a split boundary: {:?}",
        response
            .boundaries
            .iter()
            .map(|b| &b.reason_tag)
            .collect::<Vec<_>>()
    );
}

#[test]
fn hard_pause_forces_micro_chunk_boundary() {
    let words = vec![
        WordTokenDto {
            start: 0.0,
            end: 0.2,
            word: "Hello".to_string(),
        },
        WordTokenDto {
            start: 2.4,
            end: 2.7,
            word: "world".to_string(),
        },
    ];

    // VAD detects two speech segments separated by silence [0.2, 2.4]; the
    // cut midpoint (1.3) falls inside that gap, so hard_split_after fires.
    let vad_index = super::vad_align::SpeechSegmentIndex::new(vec![(0.0, 0.2), (2.4, 2.7)]);
    let chunks = build_micro_chunks(&words, &vad_index);
    assert_eq!(chunks.len(), 2);
    assert!(chunks[0].hard_split_after);
    // gap_after_ms is the raw wall-clock gap between word.end (0.2s) and the
    // next word.start (2.4s) = 2200ms, independent of any threshold constant.
    assert_eq!(chunks[0].gap_after_ms, 2_200);
}

#[test]
fn punctuation_still_closes_atom_when_available() {
    assert!(ends_with_terminal_punctuation("you."));
    assert!(ends_with_terminal_punctuation("真的吗？"));
    assert!(!ends_with_terminal_punctuation("because"));
}

#[test]
fn standalone_ascii_punctuation_keeps_following_space() {
    let words = vec![w(0, "Alright"), w(1, ","), w(2, "welcome.")];

    let response = build_source_sentences_from_words(
        request(words))
    .expect("step2 should build sentence");

    assert_eq!(response.translation_sentences[0].text, "Alright, welcome.");
}

#[test]
fn local_subtitle_layout_splits_long_semantic_sentence_near_punctuation() {
    // Ported EggTranslate semantics: English lines carry a display-CHAR cap
    // (words × 5.5) in addition to the word cap. Long-word sentences therefore
    // split into tighter cues than the word count alone would allow.
    let text = "Today the local transcription pipeline keeps complete semantic sentences for accurate review, but it should split long subtitle lines near punctuation for comfortable offline viewing.";
    let words = text
        .split_whitespace()
        .enumerate()
        .map(|(index, token)| w(index, token))
        .collect::<Vec<_>>();

    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build local subtitle layout");

    assert!(response.sentence_total >= 2, "expected multiple cues");
    let joined = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert_eq!(joined, text, "cues must join back to the original text");
    // Every cue fits BOTH hard limits: ≤ 12 words and ≤ 66 display chars.
    for s in &response.translation_sentences {
        let words_in_cue = s.text.split_whitespace().count();
        assert!(words_in_cue <= 12, "cue too many words: {}", s.text);
        assert!(s.text.chars().count() <= 66, "cue too wide: {:?}", s.text);
        assert!(words_in_cue > 2, "fragment cue: {:?}", s.text);
    }
    assert!(
        response
            .boundaries
            .iter()
            .any(|boundary| boundary.reason_tag == "subtitle_layout")
    );
}

#[test]
fn short_sentence_with_vad_pause_stays_intact() {
    // 4 words with a VAD silence gap in the middle, but well under the length
    // budget (short preset = 12 words). After the DP rewrite, this is NOT
    // split — the VAD pause only matters for overlong spans.
    let words = vec![
        WordTokenDto {
            start: 0.0,
            end: 0.2,
            word: "Before".to_string(),
        },
        WordTokenDto {
            start: 0.3,
            end: 0.5,
            word: "pause".to_string(),
        },
        WordTokenDto {
            start: 2.8,
            end: 3.0,
            word: "after".to_string(),
        },
        WordTokenDto {
            start: 3.1,
            end: 3.3,
            word: "pause".to_string(),
        },
    ];

    let response = build_source_sentences_from_words(
        request_with_vad(
            words,
            "en",
            "short",
            // VAD detects a silence gap [0.5, 2.8] between "pause" and "after".
            vec![(0.0, 0.5), (2.8, 3.3)],
        ))
    .expect("step2 should build one sentence");

    // Under budget → one sentence, not fragmented by the VAD pause.
    assert_eq!(response.sentence_total, 1);
    assert_eq!(
        response.translation_sentences[0].text,
        "Before pause after pause"
    );
}

/// Ablation: VAD sustains segmentation when terminal punctuation is absent,
/// but ONLY for overlong spans that exceed the length budget. This proves VAD
/// adds value (overlong unpunctuated speech gets split at silence) without
/// fragmenting short sentences.
#[test]
fn vad_sustains_segmentation_when_punctuation_stripped() {
    fn word(start: f64, end: f64, text: &str) -> WordTokenDto {
        WordTokenDto {
            start,
            end,
            word: text.to_string(),
        }
    }

    // An overlong unpunctuated span (20 words, English "short" preset limit=12).
    // A silence gap sits in the middle (between word 9 and word 10).
    let words_stripped: Vec<WordTokenDto> = (0..20)
        .map(|i| {
            let t = i as f64;
            // gap after word 9: word 9 ends at 9.0, word 10 starts at 10.5
            let (start, end) = if i < 10 {
                (t, t + 0.4)
            } else {
                (t + 1.5, t + 1.9)
            };
            word(start, end, &format!("w{i}"))
        })
        .collect();

    let vad_segments = vec![
        (0.0, 9.4),  // words 0-9
        (10.5, 21.9), // words 10-19
    ];

    // semantic.rs: no terminal punctuation → no hard split (correct).
    let en_profile = super::profile::profile_for_lang("en");
    let splits_semantic =
        super::semantic::build_split_points_from_hard_boundaries(&words_stripped, &*en_profile);
    assert!(
        splits_semantic.is_empty(),
        "no punctuation → no semantic hard split"
    );

    // Build semantic spans (one span: 0..19, since no hard split).
    let semantic_spans = super::split_points_to_spans(words_stripped.len(), &splits_semantic);

    let en_profile = super::profile::profile_for_lang("en");
    let short_preset =
        super::preset::subtitle_length_preset_from_id("short");

    // DP with VAD: overlong span must be split, and the VAD silence gap
    // (cost 2.0) should be chosen over plain word boundaries (cost 6.0).
    let idx_vad = super::vad_align::SpeechSegmentIndex::new(vad_segments);
    let splits_vad = super::subtitle_layout::build_subtitle_layout_split_points(
        &words_stripped,
        &semantic_spans,
        &*en_profile,
        short_preset,
        &idx_vad,
    );
    assert!(
        splits_vad.iter().any(|(i, _)| *i == 9),
        "DP should split at the VAD silence (after word 9), got cuts: {:?}",
        splits_vad
    );

    // DP WITHOUT VAD: still splits (overlong), but at a plain word boundary
    // (cost 6.0) — proving VAD gives a *better* cut, not the only cut.
    let idx_empty = super::vad_align::SpeechSegmentIndex::new(Vec::new());
    let splits_no_vad = super::subtitle_layout::build_subtitle_layout_split_points(
        &words_stripped,
        &semantic_spans,
        &*en_profile,
        short_preset,
        &idx_empty,
    );
    assert!(
        !splits_no_vad.is_empty(),
        "overlong span must be split even without VAD"
    );
}

/// Regression: an overlong semantic span that *starts* with a short discourse
/// marker + comma (e.g. "Now, the first step ...") must NOT have that marker
/// isolated as its own subtitle line. Before the short-fragment absorption,
/// the DP preferred the cheap comma cut (cost 1.5) and produced a lone "Now,"
/// cue. After the fix the marker is absorbed into the following segment.
#[test]
fn dp_does_not_isolate_leading_discourse_marker() {
    // "Now," + an 18-word body well past the "short" preset limit (12 words).
    let body = [
        "the", "first", "step", "is", "basically", "determining", "your",
        "directional", "bias", "and", "your", "drawn", "liquidity", "on",
        "the", "daily", "time", "frame.",
    ];
    let mut words = vec![w(0, "Now,")];
    for (i, tok) in body.iter().enumerate() {
        words.push(w(i + 1, tok));
    }

    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build sentences");

    // No cue may read exactly "Now," — it must have been absorbed.
    assert!(
        !response
            .translation_sentences
            .iter()
            .any(|s| s.text.trim() == "Now,"),
        "leading discourse marker was isolated as its own cue: {:?}",
        response
            .translation_sentences
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
    );
    // And the first cue must still begin with "Now," (the marker survived, just
    // merged with the following body).
    assert!(
        response.translation_sentences[0]
            .text
            .starts_with("Now,"),
        "marker text was lost during absorption: {:?}",
        response.translation_sentences[0].text
    );
}

/// Fullwidth comma on a discourse marker must take the same path as ASCII
/// "Now," — `，` is a comma, not soft punctuation, so grace-band DP will not
/// isolate it as a flash line.
#[test]
fn dp_does_not_isolate_fullwidth_discourse_marker() {
    let body = [
        "the", "first", "step", "is", "basically", "determining", "your",
        "directional", "bias", "and", "your", "drawn", "liquidity", "on",
        "the", "daily", "time", "frame.",
    ];
    let mut words = vec![w(0, "Now，")];
    for (i, tok) in body.iter().enumerate() {
        words.push(w(i + 1, tok));
    }

    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build sentences");

    assert!(
        !response
            .translation_sentences
            .iter()
            .any(|s| s.text.trim() == "Now，"),
        "fullwidth discourse marker was isolated as its own cue: {:?}",
        response
            .translation_sentences
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        response.translation_sentences[0].text.starts_with("Now，"),
        "fullwidth marker text was lost: {:?}",
        response.translation_sentences[0].text
    );
}

/// A trailing fragment (last DP segment below the floor) is absorbed into the
/// preceding segment by dropping its left cut, not orphaned.
#[test]
fn dp_absorbs_trailing_short_fragment() {
    // Body of ~14 words (over the short limit of 12) ending in a 1-word
    // trailing fragment that DP would otherwise leave dangling.
    let tokens = [
        "this", "is", "a", "long", "unpunctuated", "run", "of", "words", "that",
        "must", "be", "split", "into", "two", "parts", "now",
    ];
    let words = tokens
        .iter()
        .enumerate()
        .map(|(i, t)| w(i, t))
        .collect::<Vec<_>>();

    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(words, "en", "short"))
    .expect("step2 should build sentences");

    // No cue may be a single short word ("now") left dangling at the end.
    let last = response
        .translation_sentences
        .last()
        .expect("at least one sentence");
    assert!(
        last.text.split_whitespace().count() > MIN_FRAGMENT_WORDS,
        "trailing single-word fragment was not absorbed: {:?}",
        last.text
    );
}

/// Smoke test: the pre-existing overlong-split behavior still produces two
/// reasonable cues (no regression from the absorption pass).
#[test]
fn overlong_split_survives_fragment_absorption() {
    let response = build_source_sentences_from_words(
        request_with_lang_and_preset(
            "Today the local transcription pipeline keeps complete semantic sentences for accurate review, but it should split long subtitle lines near punctuation for comfortable offline viewing."
                .split_whitespace()
                .enumerate()
                .map(|(i, t)| w(i, t))
                .collect::<Vec<_>>(),
            "en",
            "short",
        ))
    .expect("step2 should split overlong sentence");

    // Char-capped segmentation: every cue within word AND display-char limits,
    // none of which is a ≤2-word fragment.
    assert!(response.sentence_total >= 2);
    for s in &response.translation_sentences {
        let wc = s.text.split_whitespace().count();
        assert!(
            wc > MIN_FRAGMENT_WORDS,
            "a cue collapsed to a fragment after absorption: {:?}",
            s.text
        );
        assert!(wc <= 12, "cue over word cap: {:?}", s.text);
        assert!(s.text.chars().count() <= 66, "cue over char cap: {:?}", s.text);
    }
}

#[test]
fn replay_saved_asr_applies_digit_glue_and_blocks_open_genitive() {
    let path = std::env::var("VOXTRANS_REPLAY_ASR_JSON").unwrap_or_else(|_| {
        r"C:\Users\ADMIN\AppData\Local\Temp\vt_asr_mfkge5.json".to_string()
    });
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return;
    };
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct AsrDump {
        source_lang: Option<String>,
        words: Vec<WordTokenDto>,
        vad_speech_segments: Vec<(f64, f64)>,
    }
    let dump: AsrDump = serde_json::from_str(&raw).expect("asr dump json");
    if dump.words.is_empty() {
        return;
    }
    let response = build_source_sentences_from_words(
        super::SentenceBoundaryRequest {
            task_id: "replay".to_string(),
            media_path: "replay.mp4".to_string(),
            source_lang: dump.source_lang.unwrap_or_else(|| "ja".to_string()),
            subtitle_length_preset: "standard".to_string(),
            words: dump.words,
            vad_speech_segments: dump.vad_speech_segments,
        })
    .expect("replay step2");
    let texts: Vec<&str> = response
        .translation_sentences
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    let joined = texts.join("\n");
    let counter_split: Vec<&str> = texts
        .iter()
        .copied()
        .filter(|t| has_split_digit_counter(t))
        .collect();
    assert!(
        counter_split.is_empty(),
        "split digits before counters must be glued; leftover={counter_split:?}"
    );
    assert!(
        joined.contains("10代") && !joined.contains("1 0代"),
        "leading 1 0代 must glue to 10代"
    );
    let mut open_genitive_splits = 0usize;
    let mut leftovers = Vec::<String>::new();
    for pair in texts.windows(2) {
        if pair[0].ends_with('の') {
            let next = pair[1];
            let head = next.chars().next().unwrap_or_default();
            let particle_start = matches!(
                head,
                'は' | 'が' | 'を' | 'に' | 'で' | 'と' | 'も' | 'へ' | 'や'
            );
            if !particle_start && (head.is_alphanumeric() || ('ぁ'..='ん').contains(&head) || ('ァ'..='ン').contains(&head) || ('一'..='龯').contains(&head))
            {
                open_genitive_splits += 1;
                if leftovers.len() < 6 {
                    leftovers.push(format!("{} | {}", pair[0], pair[1]));
                }
            }
        }
    }
    // Last-resort kinsoku may still end a line on の (never start the next
    // one with it). Copula/turn splits create more force-mode spans, so a
    // small number of の|noun leftovers is expected.
    assert!(
        open_genitive_splits <= 20,
        "too many の|noun splits ({open_genitive_splits}): {leftovers:?}"
    );
    let line_start_no: Vec<&str> = texts
        .iter()
        .copied()
        .filter(|t| {
            let t = t.trim_start();
            t.starts_with('の') && !t.starts_with("ので") && !t.starts_with("のに")
        })
        .collect();
    assert!(
        line_start_no.len() <= 2,
        "cues must not start with bound の (merge when it fits): {line_start_no:?}"
    );
}

fn has_split_digit_counter(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    const COUNTERS: &[char] = &['歳', '代', '人', '日', '名', '年', '月'];
    chars.windows(4).any(|w| {
        w[0].is_ascii_digit() && w[1] == ' ' && w[2].is_ascii_digit() && COUNTERS.contains(&w[3])
    })
}
