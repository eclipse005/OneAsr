//! Pure helpers shared by the segmentation stages.
//!
//! Everything here is stateless and free of I/O: millisecond conversion, the
//! DTO ↔ core word conversion, and language-aware word joining. They used to
//! live in three single-purpose files (`timing` / `words` / `text`).

use crate::subtitle::segmenter::WordToken;

use super::WordTokenDto;

/// Gap between two words in milliseconds; non-finite or negative gaps clamp to 0.
pub(super) fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    let gap = right_start_sec - left_end_sec;
    if !gap.is_finite() || gap <= 0.0 {
        return 0;
    }
    (gap * 1000.0).round() as u64
}

pub(super) fn seconds_to_ms(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1000.0).round() as u64
}

pub(super) fn to_core_words(words: Vec<WordTokenDto>) -> Vec<WordToken> {
    words
        .into_iter()
        .map(|word| WordToken {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

pub(super) fn from_core_words(words: Vec<WordToken>) -> Vec<WordTokenDto> {
    words
        .into_iter()
        .map(|word| WordTokenDto {
            start: word.start,
            end: word.end,
            word: word.word,
        })
        .collect()
}

/// One token as it will appear in a rendered cue: its text, and whether a
/// space stands in front of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SpacingPiece {
    pub space_before: bool,
    pub text: String,
}

/// The rendered pieces of a cue: one per token, in order, each carrying the gap
/// that precedes it.
///
/// This is the single answer to "does a space go here", and there are two
/// readers of it: [`join_words`], which concatenates the pieces into the cue
/// text an SRT shows, and the karaoke renderer, which has to put a `\h` exactly
/// where a space would be. Two implementations of this decision would drift,
/// and the drift would show up as an ASS file whose text no longer matches the
/// SRT next to it — so there is one, and [`join_words`] is defined in terms of
/// it rather than beside it.
pub(crate) fn spacing_pieces<'a>(parts: impl Iterator<Item = &'a str>) -> Vec<SpacingPiece> {
    let mut out: Vec<SpacingPiece> = Vec::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_spacing_word = token_has_spacing_word(token);
        // The gap this function inserts used to be eaten afterwards by a pass
        // over the whole joined string, which also swallowed a gap that landed
        // in front of a token *starting* with one of the six marks — "a" + ".5"
        // came out as "a.5", not "a .5". The tightening now lives on the piece,
        // so the same thing has to be decided here or the two stop agreeing.
        let opens_with_tight_mark = matches!(
            token.chars().next(),
            Some(',' | '.' | '!' | '?' | ':' | ';')
        );
        out.push(SpacingPiece {
            space_before: !out.is_empty()
                && next_has_spacing_word
                && (prev_has_spacing_word || prev_allows_space_after)
                && !opens_with_tight_mark,
            // The gap an ASR put inside one token ("N .") is removed here rather
            // than by a pass over the joined string, for the same reason: the
            // space in question is inside the token.
            text: tighten_marks(token),
        });
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = token_allows_space_after(token);
    }

    out
}

/// The six ASCII marks that never take a space in front of them.
fn tighten_marks(token: &str) -> String {
    token
        .replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
}

/// Join word tokens into a cue string, inserting spaces only where the scripts
/// involved actually use them (Han/kana do not; Latin/Cyrillic/Hangul do).
pub(super) fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for piece in spacing_pieces(parts) {
        if piece.space_before {
            out.push(' ');
        }
        out.push_str(&piece.text);
    }
    out
}

fn token_allows_space_after(token: &str) -> bool {
    token
        .chars()
        .last()
        .map(|ch| {
            matches!(
                ch,
                ',' | ';' | ':' | '?' | '!' | '.' | '，' | '；' | '：' | '？' | '！' | '。'
            )
        })
        .unwrap_or(false)
}

fn token_has_spacing_word(token: &str) -> bool {
    token
        .chars()
        .any(|ch| ch.is_alphanumeric() && !is_no_space_script(ch))
}

/// Scripts that do not separate words with spaces: Han ideographs and kana
/// (including half-width katakana). Hangul is deliberately absent — Korean
/// writes word spacing, so Hangul must keep the same behavior as Latin.
///
/// Also the test fixture's notion of "the aligners emit this one character at a
/// time", which is the same question asked of the same characters.
pub(crate) fn is_no_space_script(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3040..=0x30FF        // Hiragana + Katakana
            | 0x3400..=0x4DBF  // CJK Unified Ideographs Extension A
            | 0x4E00..=0x9FFF  // CJK Unified Ideographs
            | 0xF900..=0xFAFF  // CJK Compatibility Ideographs
            | 0xFF66..=0xFF9D  // Half-width Katakana
            | 0x20000..=0x2FA1F // CJK Extensions B–F + Compatibility Supplement
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn join(parts: &[&str]) -> String {
        join_words(parts.iter().copied())
    }

    #[test]
    fn latin_uses_spaces() {
        assert_eq!(join(&["Hello", "world"]), "Hello world");
    }

    #[test]
    fn cyrillic_uses_spaces() {
        assert_eq!(join(&["Привет", "мир"]), "Привет мир");
        assert_eq!(join(&["Привет,", "мир!"]), "Привет, мир!");
    }

    #[test]
    fn accented_latin_words_use_spaces() {
        assert_eq!(join(&["il", "va", "à", "Paris"]), "il va à Paris");
        assert_eq!(join(&["lui", "è", "andato"]), "lui è andato");
        assert_eq!(join(&["été", "là"]), "été là");
    }

    #[test]
    fn cjk_has_no_spaces() {
        assert_eq!(join(&["你好", "世界"]), "你好世界");
        assert_eq!(join(&["こんにちは", "世界"]), "こんにちは世界");
        assert_eq!(join(&["カタカナ", "テスト"]), "カタカナテスト");
    }

    #[test]
    fn korean_uses_spaces() {
        assert_eq!(join(&["안녕", "하세요"]), "안녕 하세요");
    }

    #[test]
    fn punctuation_glues_to_previous_word() {
        assert_eq!(join(&["Hello", ",", "world", "!"]), "Hello, world!");
        assert_eq!(join(&["你好", "，", "世界", "。"]), "你好，世界。");
    }

    /// The karaoke renderer builds its text from the pieces, so the two have to
    /// be the same answer: if they ever stop being, the ASS file's text silently
    /// stops matching the SRT beside it.
    ///
    /// Checked against the **pre-refactor** joiner rather than against
    /// `spacing_pieces`. `join_words` is now *defined* in terms of the pieces, so
    /// comparing the two against each other would be comparing a thing with
    /// itself — a test that cannot fail. The oracle below is the old code, which
    /// is the only thing here that can disagree.
    #[test]
    fn join_words_still_says_what_it_said_before_the_pieces_refactor() {
        for parts in [
            &["Hello", "world"][..],
            &["你好", "世界"][..],
            &["Hello", ",", "world", "!"][..],
            &["你好", "，", "世界", "。"][..],
            &["原版", "Whisper", "，", "剪映"][..],
            &["AI", "语音识别", "工具"][..],
            &["N", "四六", "Whisper", "和", "Cap", "V"][..],
            &["안녕", "하세요"][..],
            &["Hello", "N", ".", "world"][..],
            &["il", "va", "à", "Paris"][..],
        ] {
            assert_eq!(join(parts), joiner_before_the_refactor(parts), "{parts:?}");
        }
    }

    /// The same guarantee over every combination of an alphabet chosen to hit
    /// the awkward cases: pure marks, marks that open a token that also has
    /// letters (".5"), marks inside a token, blank tokens, and both scripts.
    ///
    /// The hand-written cases above missed exactly that one. This is the case
    /// that has teeth: a mark at the front of a token whose space is inserted
    /// ("a" + ".5") used to be eaten by the pass over the joined string, and a
    /// per-token tightening alone would leave the space in — so the piece
    /// decision has to know about it too.
    #[test]
    fn join_words_agrees_with_the_old_joiner_over_every_short_combination() {
        const ALPHABET: &[&str] = &[
            "a", "N", ".", ",", "!", "?", ":", ";", "，", "。", "你", "好", " ", "il", "à", ".5",
            "N .",
        ];
        for a in ALPHABET {
            assert_eq!(join(&[a]), joiner_before_the_refactor(&[a]), "{a:?}");
            for b in ALPHABET {
                assert_eq!(
                    join(&[a, b]),
                    joiner_before_the_refactor(&[a, b]),
                    "{a:?} {b:?}"
                );
                for c in ALPHABET {
                    assert_eq!(
                        join(&[a, b, c]),
                        joiner_before_the_refactor(&[a, b, c]),
                        "{a:?} {b:?} {c:?}"
                    );
                }
            }
        }
    }

    /// The joiner as it was before `spacing_pieces` existed: the same loop, and
    /// then a pass for the six marks over the whole joined string. Kept as the
    /// oracle — not because it is better, but because it is what shipped and the
    /// refactor has to be invisible.
    fn joiner_before_the_refactor(parts: &[&str]) -> String {
        let mut out = String::new();
        let mut prev_has_spacing_word = false;
        let mut prev_allows_space_after = false;
        for raw in parts {
            let token = raw.trim();
            if token.is_empty() {
                continue;
            }
            let next = token_has_spacing_word(token);
            if !out.is_empty() && next && (prev_has_spacing_word || prev_allows_space_after) {
                out.push(' ');
            }
            out.push_str(token);
            prev_has_spacing_word = next;
            prev_allows_space_after = token_allows_space_after(token);
        }
        out.replace(" ,", ",")
            .replace(" .", ".")
            .replace(" !", "!")
            .replace(" ?", "?")
            .replace(" :", ":")
            .replace(" ;", ";")
    }
}
