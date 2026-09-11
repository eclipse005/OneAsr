//! Output script for Chinese source languages: keep the recognizer's own
//! script, or normalize every cue to Simplified / Traditional.
//!
//! The ASR transcript itself is never rewritten: alignment, punctuation
//! restore, and sentence boundary all run on the raw model output. Conversion
//! happens on the finished subtitle text, right before SRT / TXT assembly, so
//! word timings cannot move. See [`convert_sentences`].
//!
//! [`TextScript::Original`] is the product default: a subtitle tool should not
//! silently re-write what the model heard unless the user asks for it.
//!
//! Conversion tables come from OpenCC (Apache-2.0), compiled into the binary
//! by `ferrous-opencc` — no runtime dictionary files, no network.

use std::sync::OnceLock;

use ferrous_opencc::{OpenCC, config::BuiltinConfig};

use crate::sentence_boundary::SourceSentences;

/// Output script for Chinese subtitles (`zh` / `yue` sources only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextScript {
    /// Leave the recognizer output untouched.
    Original,
    Simplified,
    Traditional,
}

impl TextScript {
    pub const ORIGINAL_ID: &'static str = "original";
    pub const SIMPLIFIED_ID: &'static str = "simplified";
    pub const TRADITIONAL_ID: &'static str = "traditional";

    /// Settings / JSON id.
    pub fn id(self) -> &'static str {
        match self {
            Self::Original => Self::ORIGINAL_ID,
            Self::Simplified => Self::SIMPLIFIED_ID,
            Self::Traditional => Self::TRADITIONAL_ID,
        }
    }

    /// Compact UI label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Original => "原文",
            Self::Simplified => "简体",
            Self::Traditional => "繁体",
        }
    }

    /// Parse a settings value; unknown / empty keeps the model output
    /// ([`Self::Original`]) — never silently re-write a user's subtitles.
    pub fn from_id(raw: &str) -> Self {
        let s = raw.trim().to_ascii_lowercase();
        match s.as_str() {
            "traditional" | "trad" | "hant" | "zh-hant" | "zh-tw" | "tw" | "hk" => {
                Self::Traditional
            }
            "simplified" | "simp" | "hans" | "zh-hans" | "zh-cn" | "cn" => Self::Simplified,
            _ => Self::Original,
        }
    }

    /// Every option in UI order.
    pub const ALL: [TextScript; 3] = [Self::Original, Self::Simplified, Self::Traditional];
}

/// True for the Chinese source languages this setting applies to.
pub fn applies_to_language(lang_key: &str) -> bool {
    matches!(lang_key, "zh" | "yue")
}

/// Convert `text` into `script`; returns `text` unchanged when the converter is
/// unavailable (or the script is [`TextScript::Original`]) so a missing table
/// can never fail a finished job.
pub fn convert(text: &str, script: TextScript) -> String {
    if script == TextScript::Original {
        return text.to_string();
    }
    if text.is_empty() {
        return String::new();
    }
    match converter(script) {
        Some(c) => c.convert(text),
        None => text.to_string(),
    }
}

/// Rewrite every finished subtitle cue in `step2` (timings untouched).
pub fn convert_sentences(step2: &mut SourceSentences, script: TextScript) {
    if script == TextScript::Original {
        return;
    }
    for sentence in &mut step2.translation_sentences {
        sentence.text = convert(&sentence.text, script);
    }
}

fn converter(script: TextScript) -> Option<&'static OpenCC> {
    static S2T: OnceLock<Option<OpenCC>> = OnceLock::new();
    static T2S: OnceLock<Option<OpenCC>> = OnceLock::new();
    let (cell, config) = match script {
        TextScript::Original => return None,
        TextScript::Traditional => (&S2T, BuiltinConfig::S2t),
        TextScript::Simplified => (&T2S, BuiltinConfig::T2s),
    };
    cell.get_or_init(|| OpenCC::from_config(config).ok())
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_unknown_keeps_model_output() {
        assert_eq!(
            TextScript::from_id(TextScript::ORIGINAL_ID),
            TextScript::Original
        );
        assert_eq!(
            TextScript::from_id(TextScript::SIMPLIFIED_ID),
            TextScript::Simplified
        );
        assert_eq!(
            TextScript::from_id(TextScript::TRADITIONAL_ID),
            TextScript::Traditional
        );
        assert_eq!(TextScript::from_id("zh-Hant"), TextScript::Traditional);
        assert_eq!(TextScript::from_id("zh-CN"), TextScript::Simplified);
        assert_eq!(TextScript::from_id(""), TextScript::Original);
        assert_eq!(TextScript::from_id("nope"), TextScript::Original);
        assert_eq!(TextScript::Original.label(), "原文");
        assert_eq!(TextScript::Traditional.label(), "繁体");
        assert_eq!(
            TextScript::ALL.map(|s| s.id()),
            ["original", "simplified", "traditional"]
        );
    }

    #[test]
    fn original_is_a_pure_no_op() {
        let mixed = "我们測試 mixed 內容。";
        assert_eq!(convert(mixed, TextScript::Original), mixed);

        use crate::sentence_boundary::SourceSentence;
        let mut step2 = SourceSentences {
            task_id: "t".into(),
            media_path: "m.mp4".into(),
            source_lang: "zh".into(),
            micro_chunk_total: 0,
            boundary_total: 0,
            sentence_total: 1,
            micro_chunks: Vec::new(),
            boundaries: Vec::new(),
            translation_sentences: vec![SourceSentence {
                sentence_id: 1,
                start_ms: 0,
                end_ms: 1000,
                text: mixed.into(),
                word_start: 0,
                word_end: 1,
                chunk_start: 0,
                chunk_end: 1,
            }],
            words: Vec::new(),
        };
        convert_sentences(&mut step2, TextScript::Original);
        assert_eq!(step2.translation_sentences[0].text, mixed);
    }

    #[test]
    fn scope_is_chinese_only() {
        assert!(applies_to_language("zh"));
        assert!(applies_to_language("yue"));
        assert!(!applies_to_language("ja"));
        assert!(!applies_to_language("en"));
    }

    #[test]
    fn converts_between_scripts_without_touching_latin_or_digits() {
        let simplified = "我们在2026年测试 OneAsr。";
        let traditional = convert(simplified, TextScript::Traditional);
        assert!(traditional.contains("我們"), "got {traditional}");
        assert!(traditional.contains("測試"), "got {traditional}");
        assert!(traditional.contains("2026"));
        assert!(traditional.contains("OneAsr"));
        assert_eq!(convert(&traditional, TextScript::Simplified), simplified);
    }

    #[test]
    fn keeps_timing_fields_untouched() {
        use crate::sentence_boundary::SourceSentence;
        let mut step2 = SourceSentences {
            task_id: "t".into(),
            media_path: "m.mp4".into(),
            source_lang: "zh".into(),
            micro_chunk_total: 0,
            boundary_total: 0,
            sentence_total: 1,
            micro_chunks: Vec::new(),
            boundaries: Vec::new(),
            translation_sentences: vec![SourceSentence {
                sentence_id: 1,
                start_ms: 120,
                end_ms: 3400,
                text: "开发工具".into(),
                word_start: 0,
                word_end: 3,
                chunk_start: 0,
                chunk_end: 1,
            }],
            words: Vec::new(),
        };
        convert_sentences(&mut step2, TextScript::Traditional);
        assert_eq!(step2.translation_sentences[0].text, "開發工具");
        assert_eq!(step2.translation_sentences[0].start_ms, 120);
        assert_eq!(step2.translation_sentences[0].end_ms, 3400);
    }
}
