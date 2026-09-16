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

/// Join word tokens into a cue string, inserting spaces only where the scripts
/// involved actually use them (Han/kana do not; Latin/Cyrillic/Hangul do).
pub(super) fn join_words<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    let mut prev_has_spacing_word = false;
    let mut prev_allows_space_after = false;

    for raw in parts {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let next_has_spacing_word = token_has_spacing_word(token);
        if !out.is_empty()
            && next_has_spacing_word
            && (prev_has_spacing_word || prev_allows_space_after)
        {
            out.push(' ');
        }
        out.push_str(token);
        prev_has_spacing_word = next_has_spacing_word;
        prev_allows_space_after = token_allows_space_after(token);
    }

    out.replace(" ,", ",")
        .replace(" .", ".")
        .replace(" !", "!")
        .replace(" ?", "?")
        .replace(" :", ":")
        .replace(" ;", ";")
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
    token.chars().any(|ch| ch.is_alphanumeric() && !is_no_space_script(ch))
}

/// Scripts that do not separate words with spaces: Han ideographs and kana
/// (including half-width katakana). Hangul is deliberately absent — Korean
/// writes word spacing, so Hangul must keep the same behavior as Latin.
fn is_no_space_script(ch: char) -> bool {
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
}
