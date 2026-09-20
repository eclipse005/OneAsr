//! Install-layout paths.
//!
//! Models live at `{app_root}/models/{name}`. GPU inference uses wgpu and the
//! system graphics driver.

use std::path::PathBuf;

use crate::media::resolve_app_root;

use super::catalog::{HTDEMUCS_FT, QWEN3_ASR_06B, QWEN_ALIGN_06B};

/// Directory of the running `oneasr.exe` (install dir or `target/*/`).
pub fn resolve_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Install / project root (folder that contains `bin/ffmpeg`), else exe dir.
pub fn resolve_app_root_dir() -> PathBuf {
    if let Some(root) = resolve_app_root() {
        return root;
    }
    resolve_exe_dir()
}

/// `{app_root}/models`
pub fn resolve_models_root() -> PathBuf {
    resolve_app_root_dir().join("models")
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

/// `{app_root}/models/htdemucs_ft` — optional vocal-separation weights.
pub fn default_demucs_model_dir() -> PathBuf {
    resolve_model_dir(HTDEMUCS_FT)
}
