use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::prompt::{build_prompt, PromptError};

/// User-facing settings. Switches map 1:1 to product plan.
///
/// Bundled tools (`bin/ffmpeg`) are **not** settings — they always live under
/// the install/app root (see [`crate::media`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub model_dir: PathBuf,
    /// `auto` | `cuda` | `cpu`
    pub backend: String,
    pub max_new_tokens: usize,
    /// Switch: use hotword prompt B.
    pub use_hotwords: bool,
    pub hotwords: String,
    /// Switch: prefix speaker in exports / list.
    pub export_show_speaker: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from(r"D:\MOSS-Transcribe-Diarize\pretrained\moss-transcribe-diarize"),
            backend: "auto".into(),
            max_new_tokens: 2048,
            use_hotwords: false,
            hotwords: String::new(),
            export_show_speaker: true,
        }
    }
}

impl Settings {
    pub fn build_prompt(&self) -> Result<String, PromptError> {
        build_prompt(self.use_hotwords, &self.hotwords)
    }

    pub fn can_start(&self) -> Result<(), PromptError> {
        self.build_prompt().map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_can_start() {
        assert!(Settings::default().can_start().is_ok());
    }

    #[test]
    fn hotwords_on_requires_text() {
        let mut s = Settings::default();
        s.use_hotwords = true;
        assert!(s.can_start().is_err());
        s.hotwords = "OpenAI".into();
        assert!(s.can_start().is_ok());
    }
}
