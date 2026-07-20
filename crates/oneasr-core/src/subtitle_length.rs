//! Subtitle length presets and language-aware limits (from VoxTrans).

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubtitleLengthPreset {
    Short,
    #[default]
    Standard,
    Loose,
}

impl SubtitleLengthPreset {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Short => "short",
            Self::Standard => "standard",
            Self::Loose => "loose",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim() {
            "short" => Self::Short,
            "loose" => Self::Loose,
            _ => Self::Standard,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubtitleLengthLimits {
    pub source_limit: u32,
    pub target_limit: u32,
}

pub fn subtitle_length_preset_from_id(value: &str) -> SubtitleLengthPreset {
    SubtitleLengthPreset::parse(value)
}

pub fn source_limit_for_preset(source_lang: &str, preset_id: &str) -> u32 {
    source_limit_for_language(source_lang, subtitle_length_preset_from_id(preset_id))
}

pub fn target_limit_for_preset(target_lang: &str, preset_id: &str) -> u32 {
    target_limit_for_language(target_lang, subtitle_length_preset_from_id(preset_id))
}

pub fn effective_subtitle_limits_from_preset(
    source_lang: &str,
    target_lang: &str,
    preset_id: &str,
) -> SubtitleLengthLimits {
    effective_subtitle_limits(
        source_lang,
        target_lang,
        subtitle_length_preset_from_id(preset_id),
    )
}

pub fn effective_subtitle_limits(
    source_lang: &str,
    target_lang: &str,
    preset: SubtitleLengthPreset,
) -> SubtitleLengthLimits {
    SubtitleLengthLimits {
        source_limit: source_limit_for_language(source_lang, preset),
        target_limit: target_limit_for_language(target_lang, preset),
    }
}

fn source_limit_for_language(lang: &str, preset: SubtitleLengthPreset) -> u32 {
    match language_key(lang).as_str() {
        "zh" | "yue" | "ja" => cjk_limits(preset),
        "ko" => korean_limits(preset),
        "de" | "fr" => long_word_source_limits(preset),
        "en" | "it" | "es" | "pt" => source_word_limits(preset),
        _ => source_word_limits(preset),
    }
}

fn target_limit_for_language(lang: &str, preset: SubtitleLengthPreset) -> u32 {
    match language_key(lang).as_str() {
        "zh" | "ja" => cjk_limits(preset),
        "ko" => korean_limits(preset),
        "th" => thai_limits(preset),
        "vi" => vietnamese_limits(preset),
        "de" | "tr" | "pl" | "ru" => long_word_target_limits(preset),
        "fr" | "es" | "it" | "pt" | "nl" | "id" => medium_word_target_limits(preset),
        "en" | "ar" => target_word_limits(preset),
        _ => target_word_limits(preset),
    }
}

fn language_key(lang: &str) -> String {
    let trimmed = lang.trim();
    let end = trimmed.find(['-', '_']).unwrap_or(trimmed.len());
    trimmed[..end].to_ascii_lowercase()
}

fn source_word_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 12,
        SubtitleLengthPreset::Standard => 16,
        SubtitleLengthPreset::Loose => 20,
    }
}

fn long_word_source_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 11,
        SubtitleLengthPreset::Standard => 14,
        SubtitleLengthPreset::Loose => 18,
    }
}

fn target_word_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 10,
        SubtitleLengthPreset::Standard => 12,
        SubtitleLengthPreset::Loose => 16,
    }
}

fn medium_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 10,
        SubtitleLengthPreset::Standard => 12,
        SubtitleLengthPreset::Loose => 15,
    }
}

fn long_word_target_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 9,
        SubtitleLengthPreset::Standard => 11,
        SubtitleLengthPreset::Loose => 14,
    }
}

fn vietnamese_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 11,
        SubtitleLengthPreset::Standard => 14,
        SubtitleLengthPreset::Loose => 18,
    }
}

fn cjk_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 16,
        SubtitleLengthPreset::Standard => 22,
        SubtitleLengthPreset::Loose => 28,
    }
}

fn korean_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 15,
        SubtitleLengthPreset::Standard => 20,
        SubtitleLengthPreset::Loose => 26,
    }
}

fn thai_limits(preset: SubtitleLengthPreset) -> u32 {
    match preset {
        SubtitleLengthPreset::Short => 24,
        SubtitleLengthPreset::Standard => 32,
        SubtitleLengthPreset::Loose => 42,
    }
}
