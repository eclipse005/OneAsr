use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::media::resolve_app_root;
use crate::prompt::{build_prompt, PromptError};

/// User-facing settings. Switches map 1:1 to product plan.
///
/// Bundled tools (`bin/ffmpeg`) are **not** settings — they always live under
/// the install/app root (see [`crate::media`]).
///
/// Persisted as `{app_root}/settings.json` when possible.
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
    /// Switch: split overlong subtitle segments at clause/connector boundaries.
    #[serde(default = "default_split_long_sentences")]
    pub split_long_sentences: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            model_dir: PathBuf::from(
                r"D:\MOSS-Transcribe-Diarize\pretrained\moss-transcribe-diarize",
            ),
            backend: "auto".into(),
            max_new_tokens: 2048,
            use_hotwords: false,
            hotwords: String::new(),
            export_show_speaker: true,
            split_long_sentences: default_split_long_sentences(),
        }
    }
}

fn default_split_long_sentences() -> bool {
    true
}

impl Settings {
    /// Prefer `{app_root}/settings.json`, else next to the executable.
    pub fn config_path() -> Option<PathBuf> {
        if let Some(root) = resolve_app_root() {
            return Some(root.join("settings.json"));
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("settings.json")))
    }

    /// Load from disk or fall back to defaults.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Settings>(&text) {
                Ok(s) => s,
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// Persist to `{app_root}/settings.json`.
    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::config_path().ok_or_else(|| "找不到应用目录，无法保存设置".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn build_prompt(&self) -> Result<String, PromptError> {
        build_prompt(self.use_hotwords, &self.hotwords)
    }

    pub fn can_start(&self) -> Result<(), PromptError> {
        self.build_prompt().map(|_| ())
    }

    #[cfg(test)]
    pub fn config_path_for(root: &std::path::Path) -> PathBuf {
        root.join("settings.json")
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

    #[test]
    fn save_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oneasr_settings_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Write via the same shape as save(), without needing app root.
        let mut s = Settings::default();
        s.use_hotwords = true;
        s.hotwords = "Alpha, Beta".into();
        s.backend = "cpu".into();
        s.export_show_speaker = false;
        s.model_dir = PathBuf::from(r"D:\models\demo");
        let path = Settings::config_path_for(&dir);
        let text = serde_json::to_string_pretty(&s).unwrap();
        std::fs::write(&path, text).unwrap();
        let loaded: Settings = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(loaded.use_hotwords);
        assert_eq!(loaded.hotwords, "Alpha, Beta");
        assert_eq!(loaded.backend, "cpu");
        assert!(!loaded.export_show_speaker);
        assert_eq!(loaded.model_dir, PathBuf::from(r"D:\models\demo"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
