use super::segmenter::WordToken;

const SENTENCE_GAP_SEC: f64 = 2.0;
const MIN_WORDS_PER_SENTENCE: usize = 4;

pub fn beautify_words_for_subtitle(mut words: Vec<WordToken>) -> Vec<WordToken> {
    if words.len() < 2 {
        return words;
    }

    let mut sentence_word_count = 0_usize;
    let mut capitalize_next = false;

    for idx in 0..words.len() {
        if capitalize_next {
            capitalize_first_alpha(&mut words[idx].word);
            capitalize_next = false;
        }

        if token_has_word(&words[idx].word) {
            sentence_word_count += 1;
        }

        if idx + 1 >= words.len() {
            continue;
        }

        let gap_sec = (words[idx + 1].start - words[idx].end).max(0.0);
        if gap_sec < SENTENCE_GAP_SEC {
            continue;
        }

        if has_terminal_or_pause_punctuation(&words[idx].word) {
            sentence_word_count = 0;
            continue;
        }

        if sentence_word_count < MIN_WORDS_PER_SENTENCE {
            continue;
        }

        words[idx].word.push('.');
        sentence_word_count = 0;
        capitalize_next = true;
    }

    words
}

fn has_terminal_or_pause_punctuation(token: &str) -> bool {
    token
        .trim()
        .chars()
        .last()
        .map(|c| {
            matches!(
                c,
                ',' | ';' | ':' | '.' | '!' | '?' | '，' | '；' | '：' | '。' | '！' | '？'
            )
        })
        .unwrap_or(false)
}

fn token_has_word(token: &str) -> bool {
    token.chars().any(|c| c.is_alphanumeric())
}

fn capitalize_first_alpha(token: &mut String) {
    let mut chars: Vec<char> = token.chars().collect();
    for ch in &mut chars {
        if ch.is_alphabetic() {
            if ch.is_lowercase() {
                *ch = ch.to_ascii_uppercase();
            }
            break;
        }
    }
    *token = chars.into_iter().collect();
}
