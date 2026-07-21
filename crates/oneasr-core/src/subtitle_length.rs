//! Subtitle length presets (limits live on each language profile).

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

pub fn subtitle_length_preset_from_id(value: &str) -> SubtitleLengthPreset {
    SubtitleLengthPreset::parse(value)
}
