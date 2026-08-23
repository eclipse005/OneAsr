//! Glue ASR-split digits in the word stream.
//!
//! Aligners often emit a multi-digit number as one-digit tokens
//! (`"1" "4歳"` → `"14歳"`, `"1 5 0 0人"` → `"1500人"`). Downstream
//! DP, SRT, and translation all see the glued form. This is a stream
//! rewrite, not a language-specific exception list.
//!
//! Decimal / thousand separators (`1.` `4`, `1,` `000`) are left alone;
//! those are numeric continuation, not split digits.

use super::WordTokenDto;

pub(super) fn glue_asr_split_digits(words: Vec<WordTokenDto>) -> Vec<WordTokenDto> {
    if words.is_empty() {
        return words;
    }
    let words: Vec<WordTokenDto> = words
        .into_iter()
        .map(|mut word| {
            word.word = glue_internal_digit_spaces(&word.word);
            word
        })
        .collect();

    let mut out = Vec::with_capacity(words.len());
    let mut i = 0usize;
    while i < words.len() {
        if i + 1 < words.len()
            && trailing_singleton_digit(&words[i].word)
            && leading_singleton_digit(&words[i + 1].word)
        {
            let start = i;
            i += 1;
            while i < words.len() && leading_singleton_digit(&words[i].word) {
                let ends_number = !trailing_singleton_digit(&words[i].word);
                i += 1;
                if ends_number {
                    break;
                }
            }
            out.push(merge_run(&words[start..i]));
            continue;
        }
        out.push(words[i].clone());
        i += 1;
    }
    out
}

fn has_internal_decimal(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.windows(3).any(|w| {
        is_decimal_digit(w[0]) && matches!(w[1], '.' | ',' | '。') && is_decimal_digit(w[2])
    })
}

fn trailing_singleton_digit(token: &str) -> bool {
    // Left side of a decimal (`1.` `1,`) must not start a glue run.
    if has_internal_decimal(token) {
        return false;
    }
    let trimmed = token.trim();
    if trimmed.ends_with('.') || trimmed.ends_with(',') {
        return false;
    }
    let core = trimmed.trim_end_matches(|c| matches!(c, '。' | '！' | '？' | '、' | '!' | '?'));
    digit_run_len(core.chars().rev()) == 1
}

fn leading_singleton_digit(token: &str) -> bool {
    // Right side of a decimal (`.14`) must not continue a glue run.
    if has_internal_decimal(token) {
        return false;
    }
    let trimmed = token.trim();
    if trimmed.starts_with('.') || trimmed.starts_with(',') {
        return false;
    }
    digit_run_len(trimmed.chars()) == 1
}

fn digit_run_len(chars: impl Iterator<Item = char>) -> usize {
    chars.take_while(|ch| is_decimal_digit(*ch)).count()
}

fn merge_run(words: &[WordTokenDto]) -> WordTokenDto {
    let mut text = String::new();
    for word in words {
        text.push_str(&word.word);
    }
    WordTokenDto {
        start: words[0].start,
        end: words[words.len() - 1].end,
        word: glue_internal_digit_spaces(&text),
    }
}

fn glue_internal_digit_spaces(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        if is_decimal_digit(chars[i]) {
            let prev_digit = out.chars().last().is_some_and(is_decimal_digit);
            out.push(chars[i]);
            i += 1;
            if prev_digit {
                continue;
            }
            loop {
                let mut j = i;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && is_decimal_digit(chars[j]) {
                    let isolated = j + 1 >= chars.len() || !is_decimal_digit(chars[j + 1]);
                    if isolated && j > i {
                        out.push(chars[j]);
                        i = j + 1;
                        continue;
                    }
                }
                break;
            }
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn is_decimal_digit(ch: char) -> bool {
    ch.is_ascii_digit() || ('０'..='９').contains(&ch)
}

/// ASR sometimes fuses ます+まず / です+だから into one token.
pub(super) fn unglue_fused_ja_copula(words: Vec<WordTokenDto>) -> Vec<WordTokenDto> {
    const COPULA: &[&str] = &["ました", "でした", "ません", "です", "ます"];
    const REST: &[&str] = &[
        "まず", "まずは", "はい", "じゃあ", "だから", "でも", "それから", "そして",
    ];
    let mut out = Vec::with_capacity(words.len());
    for word in words {
        let text = word.word.as_str();
        let split = COPULA.iter().find_map(|cop| {
            let idx = text.rfind(cop)?;
            let rest = &text[idx + cop.len()..];
            if rest.is_empty() {
                return None;
            }
            if REST.iter().any(|r| rest == *r || rest.starts_with(r)) {
                Some((text[..idx + cop.len()].to_string(), rest.to_string()))
            } else {
                None
            }
        });
        let Some((left, rest)) = split else {
            out.push(word);
            continue;
        };
        let left_chars = left.chars().count() as f64;
        let total = left_chars + rest.chars().count() as f64;
        let span = (word.end - word.start).max(0.0);
        let mid = if total <= 0.0 {
            word.start
        } else {
            word.start + span * (left_chars / total)
        };
        out.push(WordTokenDto {
            start: word.start,
            end: mid,
            word: left,
        });
        out.push(WordTokenDto {
            start: mid,
            end: word.end,
            word: rest,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(word: &str) -> WordTokenDto {
        WordTokenDto {
            start: 0.0,
            end: 0.1,
            word: word.to_string(),
        }
    }

    fn glued(words: &[&str]) -> Vec<String> {
        glue_asr_split_digits(words.iter().map(|s| w(s)).collect())
            .into_iter()
            .map(|x| x.word)
            .collect()
    }

    #[test]
    fn unglues_masu_mazu_fusion() {
        let out = unglue_fused_ja_copula(vec![
            w("お願いし"),
            WordTokenDto {
                start: 0.0,
                end: 1.0,
                word: "ますまず".into(),
            },
        ]);
        let texts: Vec<&str> = out.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(texts, ["お願いし", "ます", "まず"]);
        let out = unglue_fused_ja_copula(vec![w("ございますはい")]);
        assert_eq!(
            out.iter().map(|w| w.word.as_str()).collect::<Vec<_>>(),
            ["ございます", "はい"]
        );
    }

    #[test]
    fn glues_single_digit_plus_counter() {
        assert_eq!(glued(&["自称", "1", "4歳", "です"]), ["自称", "14歳", "です"]);
    }

    #[test]
    fn glues_four_split_digits_into_one_number() {
        assert_eq!(glued(&["1", "5", "0", "0人"]), ["1500人"]);
    }

    #[test]
    fn glues_internal_spaces_in_one_token() {
        assert_eq!(glued(&["1 4歳"]), ["14歳"]);
        assert_eq!(glued(&["1 5 0 0人"]), ["1500人"]);
    }

    #[test]
    fn glues_date_split_across_tokens() {
        assert_eq!(glued(&["8月2", "9日"]), ["8月29日"]);
    }

    #[test]
    fn leaves_decimals_and_thousands_alone() {
        assert_eq!(glued(&["1.", "4"]), ["1.", "4"]);
        assert_eq!(glued(&["1,", "000"]), ["1,", "000"]);
        assert_eq!(glued(&["12", "3"]), ["12", "3"]);
        assert_eq!(glued(&["5.1", "0"]), ["5.1", "0"]);
    }

    #[test]
    fn glues_digit_with_trailing_sentence_punct() {
        assert_eq!(glued(&["1", "9。"]), ["19。"]);
        assert_eq!(glued(&["1", "5."]), ["15."]);
    }

    #[test]
    fn leaves_non_digit_neighbors_alone() {
        assert_eq!(glued(&["episode", "1", "season"]), ["episode", "1", "season"]);
    }

    #[test]
    fn glues_fullwidth_digits() {
        assert_eq!(glued(&["１", "４歳"]), ["１４歳"]);
    }
}
