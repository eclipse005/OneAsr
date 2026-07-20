//! Source languages for Qwen3-ASR + ForcedAligner (VoxTrans-aligned).
//!
//! Pipeline **requires** an explicit source language. The usable set is the
//! intersection with Qwen3-ForcedAligner (11 languages) — same as VoxTrans UI.

/// One source language option (id stored in settings / task, labels for UI / engines).
#[derive(Debug, Clone, Copy)]
pub struct SourceLanguage {
    /// Short key stored in settings / task (`zh`, `en`, …).
    pub id: &'static str,
    /// Compact chip label for task rows.
    pub short: &'static str,
    /// UI label (endonym / product Chinese labels like VoxTrans).
    pub label: &'static str,
    /// Qwen ASR + ForcedAligner language string (`Chinese`, `English`, …).
    pub qwen: &'static str,
}

/// Align-limited source languages (VoxTrans `SOURCE_LANGUAGE_OPTIONS` order).
pub const SOURCE_LANGUAGES: &[SourceLanguage] = &[
    SourceLanguage {
        id: "zh",
        short: "中文",
        label: "中文普通话",
        qwen: "Chinese",
    },
    SourceLanguage {
        id: "en",
        short: "EN",
        label: "English",
        qwen: "English",
    },
    SourceLanguage {
        id: "yue",
        short: "粤",
        label: "粤语",
        qwen: "Cantonese",
    },
    SourceLanguage {
        id: "ja",
        short: "JA",
        label: "日本語",
        qwen: "Japanese",
    },
    SourceLanguage {
        id: "ko",
        short: "KO",
        label: "한국어",
        qwen: "Korean",
    },
    SourceLanguage {
        id: "fr",
        short: "FR",
        label: "Français",
        qwen: "French",
    },
    SourceLanguage {
        id: "de",
        short: "DE",
        label: "Deutsch",
        qwen: "German",
    },
    SourceLanguage {
        id: "it",
        short: "IT",
        label: "Italiano",
        qwen: "Italian",
    },
    SourceLanguage {
        id: "es",
        short: "ES",
        label: "Español",
        qwen: "Spanish",
    },
    SourceLanguage {
        id: "pt",
        short: "PT",
        label: "Português",
        qwen: "Portuguese",
    },
    SourceLanguage {
        id: "ru",
        short: "RU",
        label: "Русский",
        qwen: "Russian",
    },
];

pub fn default_source_language() -> String {
    "zh".into()
}

pub fn source_language_by_id(id: &str) -> Option<&'static SourceLanguage> {
    let key = to_lang_key(id);
    SOURCE_LANGUAGES.iter().find(|l| l.id == key)
}

/// Normalize free-form input to a known source id (default `zh`).
pub fn normalize_source_language(raw: &str) -> String {
    source_language_by_id(raw)
        .map(|l| l.id.to_string())
        .unwrap_or_else(default_source_language)
}

/// Normalize free-form language labels to a short key used by sentence_boundary
/// (`en`, `ja`, `zh`, …). Accepts Qwen labels (`English`, `Japanese`) and tags.
pub fn to_lang_key(raw: &str) -> String {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() || s == "unknown" || s == "forced" {
        return default_source_language();
    }
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

/// Language string for Qwen ASR `TranscribeOptions` / ForcedAligner.
/// Always returns a label for known ids; unknown input falls back to Chinese.
pub fn to_qwen_language_label(raw: &str) -> String {
    if let Some(lang) = source_language_by_id(raw) {
        return lang.qwen.to_string();
    }
    let key = to_lang_key(raw);
    if let Some(lang) = source_language_by_id(&key) {
        return lang.qwen.to_string();
    }
    // Legacy free-form: capitalize, else Chinese.
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Chinese".into();
    }
    match key.as_str() {
        "en" => "English".into(),
        "zh" => "Chinese".into(),
        "yue" => "Cantonese".into(),
        "ja" => "Japanese".into(),
        "ko" => "Korean".into(),
        "fr" => "French".into(),
        "de" => "German".into(),
        "es" => "Spanish".into(),
        "pt" => "Portuguese".into(),
        "it" => "Italian".into(),
        "ru" => "Russian".into(),
        other => {
            let mut c = other.chars();
            match c.next() {
                None => "Chinese".into(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_list_matches_aligner_eleven() {
        assert_eq!(SOURCE_LANGUAGES.len(), 11);
        assert_eq!(SOURCE_LANGUAGES[0].id, "zh");
    }

    #[test]
    fn maps_qwen_labels() {
        assert_eq!(to_lang_key("Japanese"), "ja");
        assert_eq!(to_lang_key("English"), "en");
        assert_eq!(to_qwen_language_label("ja"), "Japanese");
        assert_eq!(to_qwen_language_label(""), "Chinese");
        assert_eq!(normalize_source_language("JA"), "ja");
        assert_eq!(normalize_source_language("nope"), "zh");
    }
}
