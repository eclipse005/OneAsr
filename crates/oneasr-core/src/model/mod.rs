//! Model catalog + ModelScope download (VoxTrans-style) into the data
//! directory's `models/` (`{data}/models`, see [`crate::paths`]).

mod catalog;
mod download;
mod http;
mod path;
mod ready;

pub use catalog::{
    HTDEMUCS_FT, ModelDefinition, ModelDownloadFile, ModelId, ModelKind, OMNI_ASR_CTC_300M,
    QWEN_ALIGN_06B, QWEN3_ASR_06B, QWEN3_ASR_06B_INT8, QWEN3_ASR_17B, QWEN3_ASR_17B_INT8,
    model_definition,
};
pub use download::{
    DownloadHandle, DownloadOutcome, DownloadProgress, DownloadState, download_model,
};
pub use path::{
    default_aligner_model_dir, default_asr_model_dir, default_demucs_model_dir,
    resolve_app_root_dir, resolve_exe_dir, resolve_model_dir, resolve_models_root,
};
pub use ready::{file_meets_ready_threshold, probe_writable};
