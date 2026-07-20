use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::media::resolve_app_root;
use crate::model::{default_aligner_model_dir, default_asr_model_dir, ModelId};

/// User-facing settings for the Qwen ASR + ForcedAligner pipeline.
///
/// Persisted as `{app_root}/settings.json` when possible.
/// Model dirs default to install layout: `{app}/models/{name}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Qwen3-ASR model directory.
    #[serde(default = "default_asr_model_dir", alias = "model_dir")]
    pub asr_model_dir: PathBuf,
    /// Qwen3-ForcedAligner directory.
    #[serde(default = "default_aligner_model_dir")]
    pub aligner_model_dir: PathBuf,
    /// `auto` | `cuda` | `cpu`
    #[serde(default = "default_backend")]
    pub backend: String,
    #[serde(default = "default_max_new_tokens")]
    pub max_new_tokens: usize,
    /// Forced language (e.g. `English`, `Japanese`). Empty = auto-detect.
    #[serde(default)]
    pub language: String,
    /// `short` | `standard` | `loose`
    #[serde(default = "default_subtitle_length_preset")]
    pub subtitle_length_preset: String,
    /// VAD chunk target seconds (clamped 30–180 at runtime).
    #[serde(default = "default_chunk_target_seconds")]
    pub chunk_target_seconds: u32,
}

fn default_backend() -> String {
    "auto".into()
}

fn default_max_new_tokens() -> usize {
    2048
}

fn default_subtitle_length_preset() -> String {
    "standard".into()
}

fn default_chunk_target_seconds() -> u32 {
    180
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            asr_model_dir: default_asr_model_dir(),
            aligner_model_dir: default_aligner_model_dir(),
            backend: default_backend(),
            max_new_tokens: default_max_new_tokens(),
            language: String::new(),
            subtitle_length_preset: default_subtitle_length_preset(),
            chunk_target_seconds: default_chunk_target_seconds(),
        }
    }
}

impl Settings {
    pub fn config_path() -> Option<PathBuf> {
        if let Some(root) = resolve_app_root() {
            return Some(root.join("settings.json"));
        }
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.join("settings.json")))
    }

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

    pub fn save(&self) -> Result<PathBuf, String> {
        let path = Self::config_path().ok_or_else(|| "找不到应用目录，无法保存设置".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn can_start(&self) -> Result<(), String> {
        Ok(())
    }

    /// Apply install-layout path for a model **after successful download**.
    pub fn apply_downloaded_model(&mut self, id: ModelId, model_dir: PathBuf) {
        match id.kind() {
            crate::model::ModelKind::Asr => self.asr_model_dir = model_dir,
            crate::model::ModelKind::Align => self.aligner_model_dir = model_dir,
        }
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
    fn loads_legacy_model_dir_alias() {
        let json = r#"{
            "model_dir": "D:\\legacy\\asr",
            "backend": "cuda"
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.asr_model_dir, PathBuf::from(r"D:\legacy\asr"));
        assert_eq!(s.backend, "cuda");
    }

    #[test]
    fn apply_download_only_updates_matching_slot() {
        let mut s = Settings::default();
        let asr = PathBuf::from(r"C:\App\models\Qwen3-ASR-0.6B");
        s.apply_downloaded_model(ModelId::Qwen3Asr06B, asr.clone());
        assert_eq!(s.asr_model_dir, asr);
    }
}
