//! Subtitle length presets (limits live on each language profile).
//!
//! Lives in the segmentation layer: every consumer of `SubtitleLengthPreset`
//! is a boundary stage (`profile` / `subtitle_layout` / `watchability_merge`),
//! and the budgets it selects are defined on [`super::profile`]. `Settings`
//! only stores the id as a string, so this type stays off the crate's public
//! surface — see [`subtitle_length_preset_from_id`].

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubtitleLengthPreset {
    Short,
    #[default]
    Standard,
    Loose,
}

impl SubtitleLengthPreset {
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
