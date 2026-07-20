use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::lang::{default_source_language, normalize_source_language};
use crate::media::resolve_app_root;
use crate::model::{
    default_aligner_model_dir, default_asr_model_dir, resolve_model_dir, ModelId, ModelKind,
    QWEN3_ASR_06B,
};

/// User-facing settings for the Qwen ASR + ForcedAligner pipeline.
///
/// Persisted as `{app_root}/settings.json` when possible.
///
/// # ASR selection model
/// - [`Self::asr_model`] is the **active** catalog id (`Qwen3-ASR-0.6B` | `1.7B`).
/// - [`Self::asr_model_dir`] is the path loaded at runtime.
/// - Switching size via [`Self::select_asr_model`] always binds install-layout
///   `{app}/models/{name}`.
/// - Completing a download for a **non-selected** size only installs files on
///   disk; it must not change the active selection (see
///   [`Self::bind_download_if_active`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Active ASR catalog id (`Qwen3-ASR-0.6B` | `Qwen3-ASR-1.7B`).
    #[serde(default = "default_asr_model")]
    pub asr_model: String,
    /// Directory loaded for inference (install layout or user-picked).
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
    /// Default source language for **new** tasks (`zh`, `en`, …).
    /// Each task can override; pipeline uses the task language at run time.
    #[serde(default = "default_source_language")]
    pub language: String,
    /// `short` | `standard` | `loose`
    #[serde(default = "default_subtitle_length_preset")]
    pub subtitle_length_preset: String,
    /// VAD chunk target seconds (clamped 30–180 at runtime).
    #[serde(default = "default_chunk_target_seconds")]
    pub chunk_target_seconds: u32,
}

fn default_asr_model() -> String {
    QWEN3_ASR_06B.into()
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
            asr_model: default_asr_model(),
            asr_model_dir: default_asr_model_dir(),
            aligner_model_dir: default_aligner_model_dir(),
            backend: default_backend(),
            max_new_tokens: default_max_new_tokens(),
            language: default_source_language(),
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
                Ok(mut s) => {
                    s.normalize();
                    s
                }
                Err(_) => Self::default(),
            },
            Err(_) => Self::default(),
        }
    }

    /// Reconcile language, ASR catalog id, and model path after load / edit.
    ///
    /// Authority:
    /// 1. If `asr_model_dir` is a known catalog folder → that name **is** `asr_model`
    ///    (covers legacy `model_dir` pointing at 1.7B with default 0.6B field).
    /// 2. Else if path empty → install-layout for `asr_model`.
    /// 3. Else custom path → keep path; clamp `asr_model` to a valid catalog id
    ///    for the size picker only (inference still uses the custom path).
    pub fn normalize(&mut self) {
        self.language = normalize_source_language(&self.language);

        if let Some(id) = ModelId::try_from_asr_dir(&self.asr_model_dir) {
            self.asr_model = id.as_str().into();
            return;
        }

        let id = ModelId::parse_asr(&self.asr_model);
        self.asr_model = id.as_str().into();
        if self.asr_model_dir.as_os_str().is_empty() {
            self.asr_model_dir = resolve_model_dir(id.as_str());
        }
    }

    pub fn selected_asr_id(&self) -> ModelId {
        ModelId::parse_asr(&self.asr_model)
    }

    /// Switch active ASR size and bind install-layout path for that size.
    pub fn select_asr_model(&mut self, id: ModelId) {
        if id.kind() != ModelKind::Asr {
            return;
        }
        self.asr_model = id.as_str().into();
        self.asr_model_dir = resolve_model_dir(id.as_str());
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

    /// After a successful download: bind paths **only if** this id is the active
    /// selection (or is the single Align slot). Non-selected ASR installs leave
    /// selection alone — files already live under install-layout dirs.
    ///
    /// Returns `true` when active settings changed (caller should save / re-probe).
    pub fn bind_download_if_active(&mut self, id: ModelId, model_dir: PathBuf) -> bool {
        match id.kind() {
            ModelKind::Asr => {
                if self.selected_asr_id() != id {
                    return false;
                }
                self.asr_model = id.as_str().into();
                self.asr_model_dir = model_dir;
                true
            }
            ModelKind::Align => {
                self.aligner_model_dir = model_dir;
                true
            }
            ModelKind::CudaRuntime => false,
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
    use crate::model::QWEN3_ASR_17B;

    #[test]
    fn default_can_start() {
        assert!(Settings::default().can_start().is_ok());
        assert_eq!(Settings::default().language, "zh");
        assert_eq!(Settings::default().selected_asr_id(), ModelId::Qwen3Asr06B);
    }

    #[test]
    fn loads_legacy_model_dir_alias() {
        let json = r#"{
            "model_dir": "D:\\legacy\\asr",
            "backend": "cuda"
        }"#;
        let mut s: Settings = serde_json::from_str(json).unwrap();
        s.normalize();
        assert_eq!(s.asr_model_dir, PathBuf::from(r"D:\legacy\asr"));
        assert_eq!(s.backend, "cuda");
        assert_eq!(s.language, "zh");
    }

    #[test]
    fn normalize_syncs_asr_model_from_catalog_path() {
        let mut s = Settings::default();
        s.asr_model = QWEN3_ASR_06B.into();
        s.asr_model_dir = PathBuf::from(r"C:\App\models\Qwen3-ASR-1.7B");
        s.normalize();
        assert_eq!(s.asr_model, QWEN3_ASR_17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
    }

    #[test]
    fn bind_download_only_when_selected() {
        let mut s = Settings::default();
        s.select_asr_model(ModelId::Qwen3Asr17B);
        let dir_06 = PathBuf::from(r"C:\App\models\Qwen3-ASR-0.6B");
        assert!(!s.bind_download_if_active(ModelId::Qwen3Asr06B, dir_06));
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(s
            .asr_model_dir
            .to_string_lossy()
            .contains("Qwen3-ASR-1.7B"));

        let dir_17 = PathBuf::from(r"C:\App\models\Qwen3-ASR-1.7B");
        assert!(s.bind_download_if_active(ModelId::Qwen3Asr17B, dir_17.clone()));
        assert_eq!(s.asr_model_dir, dir_17);
    }

    #[test]
    fn select_17b_updates_path() {
        let mut s = Settings::default();
        s.select_asr_model(ModelId::Qwen3Asr17B);
        assert_eq!(s.selected_asr_id(), ModelId::Qwen3Asr17B);
        assert!(s
            .asr_model_dir
            .to_string_lossy()
            .contains("Qwen3-ASR-1.7B"));
    }
}
