//! Model catalog + ModelScope download (VoxTrans-style) into `{app}/models/`.

mod catalog;
mod download;
mod http;
mod path;
mod ready;

pub use catalog::{
    model_definition, ModelDefinition, ModelDownloadFile, ModelId, ModelKind, HTDEMUCS_FT,
    QWEN3_ASR_06B, QWEN3_ASR_06B_INT8, QWEN3_ASR_17B, QWEN3_ASR_17B_INT8, QWEN_ALIGN_06B,
};
pub use download::{
    DownloadHandle, DownloadOutcome, DownloadProgress, DownloadState, download_model,
};
pub use ready::{file_meets_ready_threshold, is_model_ready, probe_writable};
pub use path::{
    default_aligner_model_dir, default_asr_model_dir, default_demucs_model_dir,
    resolve_app_root_dir, resolve_exe_dir, resolve_model_dir, resolve_models_root,
};
