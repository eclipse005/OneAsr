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
    token
        .chars()
        .any(|ch| ch.is_ascii_alphanumeric() || is_hangul(ch))
}

fn is_hangul(ch: char) -> bool {
    matches!(
        ch as u32,
        0x1100..=0x11FF
            | 0x3130..=0x318F
            | 0xA960..=0xA97F
            | 0xAC00..=0xD7AF
            | 0xD7B0..=0xD7FF
    )
}
