//! Map Qwen ASR language labels to sentence-boundary / aligner codes.

/// Normalize free-form language labels to a short key used by sentence_boundary
/// (`en`, `ja`, `zh`, …). Accepts Qwen labels (`English`, `Japanese`) and tags.
pub fn to_lang_key(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s == "unknown" || s == "forced" {
        return "en".into();
    }
    // Already a short tag?
    let head = s.split(['-', '_']).next().unwrap_or(&s);
    match head {
        "en" | "english" => "en".into(),
        "zh" | "chinese" | "cmn" | "mandarin" => "zh".into(),
        "yue" | "cantonese" => "yue".into(),
        "ja" | "japanese" => "ja".into(),
        "ko" | "korean" => "ko".into(),
        "fr" | "french" => "fr".into(),
        "de" | "german" => "de".into(),
        "es" | "spanish" => "es".into(),
        "pt" | "portuguese" => "pt".into(),
        "it" | "italian" => "it".into(),
        "ru" | "russian" => "ru".into(),
        "ar" | "arabic" => "ar".into(),
        "th" | "thai" => "th".into(),
        other => other.to_string(),
    }
}

/// Language string for Qwen ASR `TranscribeOptions` / ForcedAligner
/// (`English`, `Japanese`, …). Empty input means auto-detect (ASR only).
pub fn to_qwen_language_label(raw: &str) -> Option<String> {
    let key = to_lang_key(raw);
    if raw.trim().is_empty() {
        return None;
    }
    let label = match key.as_str() {
        "en" => "English",
        "zh" => "Chinese",
        "yue" => "Cantonese",
        "ja" => "Japanese",
        "ko" => "Korean",
        "fr" => "French",
        "de" => "German",
        "es" => "Spanish",
        "pt" => "Portuguese",
        "it" => "Italian",
        "ru" => "Russian",
        "ar" => "Arabic",
        "th" => "Thai",
        other => {
            // Capitalize first letter of whatever we got
            let mut c = other.chars();
            return Some(match c.next() {
                None => return None,
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            });
        }
    };
    Some(label.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_qwen_labels() {
        assert_eq!(to_lang_key("Japanese"), "ja");
        assert_eq!(to_lang_key("English"), "en");
        assert_eq!(to_qwen_language_label("ja").as_deref(), Some("Japanese"));
        assert!(to_qwen_language_label("").is_none());
    }
}
