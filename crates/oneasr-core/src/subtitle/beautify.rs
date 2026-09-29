//! Readability pass over ASR words.
//!
//! **Invariant — the display layer may remove, but never add.** Whatever the
//! ASR produced is the text; segmentation may drop, rearrange or space tokens,
//! but it must not invent characters the recogniser never emitted. Punctuation
//! is the evidence of where a sentence ends, and a *synthesised* mark is a
//! fabricated sentence boundary: it silently overrides the real punctuation and
//! cannot be told apart from the real thing downstream (translation, SRT,
//! user-facing review).
//!
//! This module used to append `.` after any ≥2s pause and sentence-case the
//! following token. That is exactly the fabrication the invariant forbids:
//!
//! * It invented sentence boundaries in languages that do not mark them
//!   (Japanese / Chinese), where a long pause is just a pause.
//! * It overwrote the recogniser's own casing, so the user could not see what
//!   the model actually produced.
//!
//! A real pause is still a perfectly good *cut point* — but that belongs to the
//! layout layer, which is free to choose a boundary at it (`vad_align` plus the
//! DP cost). Choosing a cut is not the same as writing a character into the
//! text, and only the latter is forbidden.
//!
//! The pass is therefore a no-op today; it stays as the named, tested place for
//! *removal-only* cleanups.

use super::segmenter::WordToken;

/// Removal-only readability pass. See the module docs for the invariant.
pub fn beautify_words_for_subtitle(words: Vec<WordToken>) -> Vec<WordToken> {
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok(text: &str, start: f64, end: f64) -> WordToken {
        WordToken {
            start,
            end,
            word: text.to_string(),
        }
    }

    /// A ≥2s pause after a multi-word run used to append `.` and uppercase the
    /// next token. It must now leave the ASR text byte-for-byte alone.
    #[test]
    fn long_pause_does_not_fabricate_punctuation_or_casing() {
        // 4+ words, then a 3s gap, then a lowercase continuation — the exact
        // shape the old rule fired on.
        let words = vec![
            tok("it", 0.0, 0.2),
            tok("was", 0.2, 0.4),
            tok("a", 0.4, 0.5),
            tok("mistake", 0.5, 0.9),
            tok("and", 3.9, 4.1),
            tok("then", 4.1, 4.3),
            tok("we", 4.3, 4.5),
            tok("left", 4.5, 4.8),
        ];
        let before: Vec<String> = words.iter().map(|w| w.word.clone()).collect();

        let out = beautify_words_for_subtitle(words);

        let after: Vec<String> = out.iter().map(|w| w.word.clone()).collect();
        assert_eq!(after, before, "beautify must not change ASR text");
        assert!(
            !after.iter().any(|w| w.ends_with('.')),
            "no invented period"
        );
    }

    /// Japanese/Chinese have no sentence-marking punctuation convention here; a
    /// long pause must not manufacture one.
    #[test]
    fn long_pause_does_not_fabricate_a_cjk_period() {
        let words = vec![
            tok("それ", 0.0, 0.4),
            tok("は", 0.4, 0.6),
            tok("とても", 0.6, 1.0),
            tok("大変", 1.0, 1.4),
            tok("だった", 4.4, 4.9),
        ];
        let out = beautify_words_for_subtitle(words);
        let texts: Vec<&str> = out.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(texts, ["それ", "は", "とても", "大変", "だった"]);
    }

    /// Timing is data, not decoration: the pass must not touch it either.
    #[test]
    fn timestamps_are_untouched() {
        let words = vec![
            tok("one", 0.0, 0.3),
            tok("two", 0.3, 0.6),
            tok("three", 0.6, 0.9),
            tok("four", 0.9, 1.2),
            tok("five", 5.2, 5.5),
        ];
        let out = beautify_words_for_subtitle(words.clone());
        for (a, b) in words.iter().zip(out.iter()) {
            assert_eq!(a.start, b.start);
            assert_eq!(a.end, b.end);
        }
    }
}
