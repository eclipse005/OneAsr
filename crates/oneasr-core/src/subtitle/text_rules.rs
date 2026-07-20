pub fn should_split_after_terminal_token(
    current_token: &str,
    next_token: Option<&str>,
) -> bool {
    let normalized = strip_trailing_closers(current_token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    if is_abbreviation_continuation(normalized, next_token) {
        return false;
    }
    true
}

pub fn has_break_terminal_punctuation(token: &str) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    !normalized.is_empty()
        && ends_with_terminal_punctuation(normalized)
        && !is_non_break_terminal_case(normalized)
}

pub fn ends_with_terminal_punctuation(word: &str) -> bool {
    word.chars()
        .last()
        .map(is_terminal_punctuation)
        .unwrap_or(false)
}

pub fn strip_trailing_closers(token: &str) -> &str {
    token.trim_end_matches(|c: char| {
        matches!(
            c,
            '"' | '\''
                | ')'
                | ']'
                | '}'
                | '”'
                | '’'
                | '»'
                | '›'
                | '）'
                | '】'
                | '｝'
                | '〉'
                | '》'
                | '」'
                | '』'
                | '〕'
                | '〗'
                | '〙'
                | '〛'
        )
    })
}

pub(crate) fn is_non_break_terminal_case(token: &str) -> bool {
    is_common_abbreviation(token)
        || looks_like_dotted_abbreviation(token)
        || looks_like_decimal_number(token)
}

/// Does `token` represent a sentence-ending terminal mark that should NOT cause
/// a split — i.e. the mark is part of an abbreviation that continues into the
/// next token?
///
/// Multi-letter abbreviations (`Mr.`, `p.m.`, `U.S.`) are language-specific and
/// resolved purely from the token via [`is_common_abbreviation`] etc. They
/// never break regardless of context.
///
/// A single-letter dotted token (`B.`, `A.`, `J.`) is ambiguous: it can be a
/// spoken enumeration ("step one B."), a one-letter answer ("B."), or a name
/// initial (`J. K. Rowling`). Single-letter initials almost never end a
/// real sentence in transcription, but they are extremely common in speech
/// for enumeration. We therefore resolve the ambiguity with `next_token`:
/// only a *chain* of consecutive single-letter initials (`J. K.`) is treated
/// as an abbreviation; an isolated single-letter token always splits.
fn is_abbreviation_continuation(token: &str, next_token: Option<&str>) -> bool {
    // Multi-letter / dotted / numeric abbreviations: context-independent.
    if is_common_abbreviation(token)
        || looks_like_dotted_abbreviation(token)
        || looks_like_decimal_number(token)
    {
        return true;
    }

    // Single-letter token (B. A. J.): only a continuing initial chain counts.
    if is_single_letter_initial(token) {
        return next_token
            .map(|next| {
                let next_norm = strip_trailing_closers(next.trim());
                is_single_letter_initial(&next_norm.to_ascii_lowercase())
            })
            .unwrap_or(false);
    }

    false
}

fn is_terminal_punctuation(c: char) -> bool {
    matches!(
        c,
        '.' | '!'
            | '?'
            | '。'
            | '！'
            | '？'
            | '｡'
            | '．'
            | '﹒'
            | '…'
            | '‥'
            | '‼'
            | '⁇'
            | '⁈'
            | '⁉'
            | '؟'
            | '۔'
            | '።'
            | '፧'
            | '፨'
            | '။'
            | '।'
            | '॥'
            | '։'
    )
}

fn is_common_abbreviation(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    matches!(
        lower.as_str(),
        "mr."
            | "mrs."
            | "ms."
            | "mx."
            | "dr."
            | "prof."
            | "rev."
            | "hon."
            | "fr."
            | "pres."
            | "gov."
            | "sen."
            | "rep."
            | "amb."
            | "sr."
            | "jr."
            | "esq."
            | "capt."
            | "cmdr."
            | "col."
            | "gen."
            | "lt."
            | "maj."
            | "sgt."
            | "adm."
            | "st."
            | "mt."
            | "ave."
            | "blvd."
            | "rd."
            | "ln."
            | "ct."
            | "pl."
            | "no."
            | "vs."
            | "etc."
            | "al."
            | "cf."
            | "fig."
            | "figs."
            | "ed."
            | "eds."
            | "vol."
            | "vols."
            | "ch."
            | "pp."
            | "dept."
            | "univ."
            | "assn."
            | "assoc."
            | "e.g."
            | "i.e."
            | "a.m."
            | "p.m."
            | "u.s."
            | "u.k."
            | "u.n."
            | "e.u."
            | "d.c."
            | "n.y."
            | "n.y.c."
            | "l.a."
            | "inc."
            | "ltd."
            | "co."
            | "corp."
            | "bros."
            | "llc."
            | "plc."
            | "jan."
            | "feb."
            | "mar."
            | "apr."
            | "jun."
            | "jul."
            | "aug."
            | "sep."
            | "sept."
            | "oct."
            | "nov."
            | "dec."
    )
}

fn is_single_letter_initial(token: &str) -> bool {
    let chars: Vec<char> = token.chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

fn looks_like_dotted_abbreviation(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    if !lower.ends_with('.') || lower.matches('.').count() < 2 {
        return false;
    }

    let mut part_count = 0usize;
    for part in lower.trim_end_matches('.').split('.') {
        if part.is_empty() || part.len() > 3 || !part.chars().all(|c| c.is_ascii_alphabetic()) {
            return false;
        }
        part_count += 1;
    }
    part_count >= 2
}

fn looks_like_decimal_number(token: &str) -> bool {
    let t = strip_trailing_closers(token);
    let mut parts = t.split('.');
    let left = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    let right = match parts.next() {
        Some(v) => v,
        None => return false,
    };
    if parts.next().is_some() {
        return false;
    }
    !left.is_empty()
        && !right.is_empty()
        && left.chars().all(|c| c.is_ascii_digit())
        && right.chars().all(|c| c.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::{
        has_break_terminal_punctuation, is_non_break_terminal_case,
        should_split_after_terminal_token,
    };

    #[test]
    fn am_pm_is_non_break() {
        assert!(is_non_break_terminal_case("a.m."));
        assert!(is_non_break_terminal_case("p.m."));
        assert!(!has_break_terminal_punctuation("a.m."));
        assert!(!has_break_terminal_punctuation("p.m."));
    }

    #[test]
    fn common_titles_and_business_abbreviations_are_non_break() {
        for token in [
            "Mr.", "Mrs.", "Ms.", "Dr.", "Prof.", "Rev.", "Gov.", "Sen.", "Rep.", "Capt.", "Col.",
            "Gen.", "Lt.", "Sgt.", "Inc.", "Ltd.", "Corp.", "Co.",
        ] {
            assert!(is_non_break_terminal_case(token), "{token}");
            assert!(!has_break_terminal_punctuation(token), "{token}");
        }
    }

    #[test]
    fn dotted_initialisms_are_non_break() {
        for token in [
            "U.S.", "U.K.", "U.N.", "E.U.", "D.C.", "N.Y.C.", "Ph.D.", "M.D.",
        ] {
            assert!(is_non_break_terminal_case(token), "{token}");
            assert!(!has_break_terminal_punctuation(token), "{token}");
        }
    }

    #[test]
    fn decimal_is_non_break() {
        assert!(is_non_break_terminal_case("3.14"));
        assert!(!has_break_terminal_punctuation("3.14"));
    }

    #[test]
    fn regular_sentence_terminal_breaks() {
        assert!(has_break_terminal_punctuation("world."));
        assert!(should_split_after_terminal_token("world.", Some("Next")));
    }

    #[test]
    fn supported_terminal_punctuation_breaks() {
        for token in [
            "done!",
            "done?",
            "结束。",
            "終わり｡",
            "終わり．",
            "끝！",
            "끝？",
            "fin…",
            "fin‥",
            "really⁈",
            "really⁉",
            "حسنا؟",
            "끝。」",
            "fin.»",
        ] {
            assert!(has_break_terminal_punctuation(token), "{token}");
        }
    }

    #[test]
    fn lowercase_next_word_still_breaks_after_real_terminal_punctuation() {
        assert!(should_split_after_terminal_token("hello.", Some("world")));
    }

    // ---- single-letter dotted token (`B.`, `A.`, `J.`) ambiguity ----

    #[test]
    fn single_letter_token_followed_by_non_initial_splits() {
        // "step one B. So ..." — the . is a real sentence end, not a name initial.
        assert!(should_split_after_terminal_token("B.", Some("So")));
        assert!(should_split_after_terminal_token("A.", Some("And")));
        assert!(should_split_after_terminal_token("A.", Some("a")));
    }

    #[test]
    fn single_letter_token_at_end_of_stream_splits() {
        // "step one B." at the very end — no next token, must split.
        assert!(should_split_after_terminal_token("B.", None));
    }

    #[test]
    fn consecutive_single_letter_initials_do_not_split() {
        // "J. K. Rowling" — a chain of initials is an abbreviation.
        assert!(!should_split_after_terminal_token("J.", Some("K.")));
        // The second initial followed by a normal word still splits — only the
        // chain itself is protected, not the token after it.
        assert!(should_split_after_terminal_token("K.", Some("Rowling")));
    }

    #[test]
    fn multi_letter_abbreviations_remain_non_break_regardless_of_next() {
        // Mr./Dr./p.m./U.S. must not split no matter what follows.
        assert!(!should_split_after_terminal_token("Mr.", Some("Smith")));
        assert!(!should_split_after_terminal_token("Mr.", Some("smith")));
        assert!(!should_split_after_terminal_token("p.m.", Some("We")));
        assert!(!should_split_after_terminal_token("U.S.", Some("Army")));
        assert!(!should_split_after_terminal_token("Dr.", None));
    }

    #[test]
    fn single_letter_token_is_break_terminal_for_dp_cost() {
        // has_break_terminal_punctuation (Layer 2 DP cost) has no next-token
        // context; it treats an isolated single-letter token as a real terminal
        // so the DP can prefer cutting there. The split is soft (cost 0.5),
        // never forced.
        assert!(has_break_terminal_punctuation("B."));
        assert!(has_break_terminal_punctuation("A."));
    }
}
