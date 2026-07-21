//! Model catalog + ModelScope download (VoxTrans-style) into `{app}/models/`.

mod catalog;
mod download;
mod path;

pub use catalog::{
    model_definition, ModelDefinition, ModelDownloadFile, ModelId, ModelKind, QWEN3_ASR_06B,
    QWEN3_ASR_17B, QWEN_ALIGN_06B,
};
pub use download::{
    download_model, is_cuda_runtime_ready, is_model_ready, DownloadHandle, DownloadOutcome,
    DownloadProgress, DownloadState,
};
pub use path::{
    default_aligner_model_dir, default_asr_model_dir, init_native_library_path,
    resolve_app_root_dir, resolve_dll_dir, resolve_exe_dir, resolve_model_dir, resolve_models_root,
};
