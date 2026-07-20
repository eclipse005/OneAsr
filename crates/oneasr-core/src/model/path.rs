//! Install-layout model paths: `{app_root}/models/{ModelName}`.

use std::path::PathBuf;

use crate::media::resolve_app_root;

use super::catalog::{QWEN3_ASR_06B, QWEN_ALIGN_06B};

/// `{app_root}/models` — prefers install/dev root that contains `bin/ffmpeg`.
pub fn resolve_models_root() -> PathBuf {
    if let Some(root) = resolve_app_root() {
        return root.join("models");
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("models");
        }
    }
    PathBuf::from("models")
}

pub fn resolve_model_dir(model_name: &str) -> PathBuf {
    resolve_models_root().join(model_name)
}

pub fn default_asr_model_dir() -> PathBuf {
    resolve_model_dir(QWEN3_ASR_06B)
}

pub fn default_aligner_model_dir() -> PathBuf {
    resolve_model_dir(QWEN_ALIGN_06B)
}
