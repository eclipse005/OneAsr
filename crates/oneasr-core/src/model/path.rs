//! Install-layout + data-layout paths.
//!
//! 安装目录（含 `bin/ffmpeg`）只负责放程序与 ffmpeg；`models/` 的落点由数据目录
//! 决定（见 [`crate::paths`]）：安装目录可写时就是它自己（便携），只读安装目录
//! （`/opt/OneAsr` 之类）则落用户数据目录。GPU inference uses wgpu and the system
//! graphics driver.

use std::path::PathBuf;

use crate::media::resolve_app_root;
use crate::paths::{DataRootSource, MODELS_DIR, choose_model_dir, data_root};

use super::catalog::{HTDEMUCS_FT, QWEN_ALIGN_06B, QWEN3_ASR_06B};

/// Directory of the running `oneasr.exe` (install dir or `target/*/`).
pub fn resolve_exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Install / project root (folder that contains `bin/ffmpeg`), else exe dir.
///
/// 只读也能用：**从不**在这里判断可写性，数据目录的落点由 [`crate::paths`] 决定。
pub fn resolve_app_root_dir() -> PathBuf {
    if let Some(root) = resolve_app_root() {
        return root;
    }
    resolve_exe_dir()
}

/// 模型根的默认落点：`{data}/models`（数据目录里）。
pub fn resolve_models_root() -> PathBuf {
    crate::paths::models_root()
}

/// 一个模型的目录：数据目录优先，其次沿用安装目录里现成的同名目录
/// （只读安装目录里已下好的 GB 权重照旧可用），都没有则落数据目录 —— 也就是
/// 下次下载的落点，一定可写。
pub fn resolve_model_dir(model_name: &str) -> PathBuf {
    let install = resolve_app_root_dir();
    let data = crate::paths::data_dir();
    let source = data_root()
        .as_ref()
        .map(|root| root.source)
        .unwrap_or(DataRootSource::Portable);
    let has = |root: &PathBuf| root.join(MODELS_DIR).join(model_name).is_dir();
    choose_model_dir(
        &data,
        &install,
        source,
        model_name,
        has(&data),
        has(&install),
    )
}

pub fn default_asr_model_dir() -> PathBuf {
    resolve_model_dir(QWEN3_ASR_06B)
}

pub fn default_aligner_model_dir() -> PathBuf {
    resolve_model_dir(QWEN_ALIGN_06B)
}

/// `{data}/models/htdemucs_ft` — optional vocal-separation weights.
pub fn default_demucs_model_dir() -> PathBuf {
    resolve_model_dir(HTDEMUCS_FT)
}
