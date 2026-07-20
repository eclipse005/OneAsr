#[derive(Debug, Clone)]
pub struct WordToken {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

pub fn normalize_word_tokens(raw_words: Vec<WordToken>) -> Vec<WordToken> {
    let mut out: Vec<WordToken> = Vec::with_capacity(raw_words.len());
    for token in raw_words {
        let text = token.word.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if is_standalone_punctuation_token(&text)
            && !is_numeric_prefix_symbol(&text)
            && let Some(prev) = out.last_mut()
        {
            prev.word.push_str(&text);
            prev.end = prev.end.max(token.end);
            continue;
        }
        out.push(WordToken {
            start: token.start,
            end: token.end,
            word: text,
        });
    }
    let out = merge_compound_abbreviation_tokens(out);
    // Pass 1: merge numeric compounds (e.g. "3" + "D" -> "3D") that ASR
    // sometimes splits between digit and letter runs.
    let out = merge_compound_numeric_tokens(out);
    // merge_prefixed_numeric_tokens may surface new prefix+digit
    // adjacencies that were previously blocked by an intervening token
    // (now merged). Pass 2 catches those — without it, "U" + "3" left
    // over after a prefix merge would survive as separate tokens.
    let out = merge_prefixed_numeric_tokens(out);
    let out = merge_compound_numeric_tokens(out);
    merge_numeric_unit_suffix_tokens(out)
}

fn is_standalone_punctuation_token(token: &str) -> bool {
    token
        .chars()
        .all(|c| !c.is_alphanumeric() && !c.is_whitespace())
}

fn merge_compound_abbreviation_tokens(mut words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_abbreviation_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                continue;
            }
        }

        // Move the token out instead of cloning its String; `words` is
        // consumed by this pass and idx only advances forward.
        out.push(WordToken {
            start: words[idx].start,
            end: words[idx].end,
            word: std::mem::take(&mut words[idx].word),
        });
        idx += 1;
    }

    out
}

fn merge_compound_numeric_tokens(words: Vec<WordToken>) -> Vec<WordToken> {
    let mut current = words;
    loop {
        let (next, changed) = merge_compound_numeric_tokens_once(current);
        if !changed {
            return next;
        }
        current = next;
    }
}

fn merge_prefixed_numeric_tokens(mut words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_prefix_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                continue;
            }
        }

        // Move the token out instead of cloning its String; `words` is
        // consumed by this pass and idx only advances forward.
        out.push(WordToken {
            start: words[idx].start,
            end: words[idx].end,
            word: std::mem::take(&mut words[idx].word),
        });
        idx += 1;
    }

    out
}

fn merge_numeric_unit_suffix_tokens(mut words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_suffix_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                continue;
            }
        }

        // Move the token out instead of cloning its String; `words` is
        // consumed by this pass and idx only advances forward.
        out.push(WordToken {
            start: words[idx].start,
            end: words[idx].end,
            word: std::mem::take(&mut words[idx].word),
        });
        idx += 1;
    }

    out
}

fn merge_compound_numeric_tokens_once(mut words: Vec<WordToken>) -> (Vec<WordToken>, bool) {
    if words.len() < 2 {
        return (words, false);
    }

    let mut out: Vec<WordToken> = Vec::with_capacity(words.len());
    let mut idx = 0usize;
    let mut changed = false;
    while idx < words.len() {
        if idx + 1 < words.len() {
            let left = words[idx].word.trim();
            let right = words[idx + 1].word.trim();
            if should_merge_numeric_pair(left, right) {
                out.push(WordToken {
                    start: words[idx].start,
                    end: words[idx + 1].end,
                    word: format!("{left}{right}"),
                });
                idx += 2;
                changed = true;
                continue;
            }
        }

        // Move the token out instead of cloning its String; `words` is
        // consumed by this pass and idx only advances forward.
        out.push(WordToken {
            start: words[idx].start,
            end: words[idx].end,
            word: std::mem::take(&mut words[idx].word),
        });
        idx += 1;
    }

    (out, changed)
}

fn should_merge_abbreviation_pair(left: &str, right: &str) -> bool {
    if left.len() != 1 || !left.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let l = left.to_ascii_lowercase();
    let r = right.to_ascii_lowercase();
    matches!(
        (l.as_str(), r.as_str()),
        ("a", ".m.") | ("p", ".m.") | ("u", ".s.") | ("u", ".k.")
    )
}

fn should_merge_numeric_pair(left: &str, right: &str) -> bool {
    should_merge_decimal_pair(left, right)
        || should_merge_time_pair(left, right)
        || should_merge_thousands_pair(left, right)
        || should_merge_slash_pair(left, right)
        || should_merge_hyphen_pair(left, right)
}

fn should_merge_decimal_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('.') else {
        return false;
    };
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }

    let mut right_chars = right.chars();
    let Some(first) = right_chars.next() else {
        return false;
    };
    if !first.is_ascii_digit() {
        return false;
    }

    right
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '%' | '.' | ',' | '+' | '-'))
}

fn should_merge_time_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix(':') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split(':').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    !right.is_empty()
        && right
            .split(':')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_thousands_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix(',') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let prefix_parts: Vec<&str> = prefix.split(',').collect();
    if prefix_parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    right.len() == 3 && right.chars().all(|c| c.is_ascii_digit())
}

fn should_merge_slash_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('/') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    !right.is_empty()
        && right
            .split('/')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_hyphen_pair(left: &str, right: &str) -> bool {
    let Some(prefix) = left.strip_suffix('-') else {
        return false;
    };
    if prefix.is_empty() {
        return false;
    }
    let parts: Vec<&str> = prefix.split('-').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || !part.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    !right.is_empty()
        && right
            .split('-')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn should_merge_numeric_prefix_pair(left: &str, right: &str) -> bool {
    is_numeric_prefix_symbol(left) && starts_with_numeric_value(right)
}

fn should_merge_numeric_suffix_pair(left: &str, right: &str) -> bool {
    is_numeric_value(left) && is_numeric_unit_suffix(right)
}

fn is_numeric_prefix_symbol(token: &str) -> bool {
    matches!(token, "$" | "€" | "£" | "¥" | "￥" | "₹")
}

fn starts_with_numeric_value(token: &str) -> bool {
    token
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
}

fn is_numeric_value(token: &str) -> bool {
    let trimmed = token.trim();
    !trimmed.is_empty()
        && trimmed.chars().any(|c| c.is_ascii_digit())
        && trimmed.chars().all(|c| {
            c.is_ascii_digit()
                || matches!(
                    c,
                    '.' | ',' | ':' | '/' | '-' | '%' | '+' | '$' | '€' | '£' | '¥' | '￥' | '₹'
                )
        })
}

fn is_numeric_unit_suffix(token: &str) -> bool {
    const KNOWN_UNITS: &[&str] = &[
        "k", "m", "b", "t", "x", "s", "ms", "kg", "g", "mg", "lb", "lbs", "km", "m", "cm", "mm",
        "ft", "in", "h", "hr", "hrs", "min", "mins", "usd", "eur", "gbp", "jpy", "cny", "bp",
        "bps",
    ];
    let lower = token.trim().to_ascii_lowercase();
    !lower.is_empty() && KNOWN_UNITS.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::{WordToken, normalize_word_tokens};

    #[test]
    fn merges_am_pm_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "p".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.2,
                word: ".m.".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "p.m.");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_decimal_percentage_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "2".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.15,
                word: ".".to_string(),
            },
            WordToken {
                start: 0.15,
                end: 0.3,
                word: "5%".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2.5%");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.3);
    }

    #[test]
    fn merges_time_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "6".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.15,
                word: ":".to_string(),
            },
            WordToken {
                start: 0.15,
                end: 0.25,
                word: "20".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "6:20");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.25);
    }

    #[test]
    fn merges_multi_part_times_across_multiple_passes() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "12".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.15,
                word: ":".to_string(),
            },
            WordToken {
                start: 0.15,
                end: 0.25,
                word: "30".to_string(),
            },
            WordToken {
                start: 0.25,
                end: 0.3,
                word: ":".to_string(),
            },
            WordToken {
                start: 0.3,
                end: 0.4,
                word: "45".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "12:30:45");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.4);
    }

    #[test]
    fn merges_thousands_separator_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "1".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: ",".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "000".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "1,000");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_date_slash_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "03".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: "/".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "23".to_string(),
            },
            WordToken {
                start: 0.2,
                end: 0.22,
                word: "/".to_string(),
            },
            WordToken {
                start: 0.22,
                end: 0.32,
                word: "2026".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "03/23/2026");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.32);
    }

    #[test]
    fn merges_date_hyphen_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "2026".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: "-".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "03".to_string(),
            },
            WordToken {
                start: 0.2,
                end: 0.22,
                word: "-".to_string(),
            },
            WordToken {
                start: 0.22,
                end: 0.3,
                word: "23".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2026-03-23");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.3);
    }

    #[test]
    fn merges_fraction_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "3".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: "/".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "4".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "3/4");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_currency_prefix_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.05,
                word: "$".to_string(),
            },
            WordToken {
                start: 0.05,
                end: 0.2,
                word: "12.5".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "$12.5");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn keeps_currency_prefix_separate_from_previous_word() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.1,
                word: "cost".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: "$".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "10".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].word, "cost");
        assert_eq!(normalized[1].word, "$10");
        assert_eq!(normalized[1].start, 0.1);
        assert_eq!(normalized[1].end, 0.2);
    }

    #[test]
    fn merges_currency_amount_with_thousands_separator() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.02,
                word: "$".to_string(),
            },
            WordToken {
                start: 0.02,
                end: 0.1,
                word: "1".to_string(),
            },
            WordToken {
                start: 0.1,
                end: 0.12,
                word: ",".to_string(),
            },
            WordToken {
                start: 0.12,
                end: 0.2,
                word: "000".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "$1,000");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn merges_numeric_unit_suffix_pairs() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.15,
                word: "10".to_string(),
            },
            WordToken {
                start: 0.15,
                end: 0.22,
                word: "kg".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "10kg");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.22);
    }

    #[test]
    fn keeps_already_normalized_numeric_token_unchanged() {
        let words = vec![WordToken {
            start: 0.0,
            end: 0.2,
            word: "2.5%".to_string(),
        }];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "2.5%");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.2);
    }

    #[test]
    fn standalone_punctuation_extends_previous_end_timestamp() {
        let words = vec![
            WordToken {
                start: 0.0,
                end: 0.3,
                word: "notes".to_string(),
            },
            WordToken {
                start: 0.3,
                end: 0.35,
                word: ".".to_string(),
            },
        ];

        let normalized = normalize_word_tokens(words);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].word, "notes.");
        assert_eq!(normalized[0].start, 0.0);
        assert_eq!(normalized[0].end, 0.35);
    }
}
