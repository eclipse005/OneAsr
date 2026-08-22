//! OneAsr core: Qwen ASR + ForcedAligner pipeline, sentence boundary, jobs.

pub mod asr;
pub mod job;
pub mod lang;
pub mod media;
pub mod model;
pub mod paths;
pub mod runtime;
pub mod sentence_boundary;
pub mod settings;
pub mod subtitle;
pub mod subtitle_length;
pub mod ui_labels;
pub mod vad;

pub use asr::{
    check_aligner_model_dir, check_asr_model_dir, format_process_ms,
    process_media_file_with_export, process_media_file_with_progress, AsrStage,
    ProcessExportOptions, StageClock, StageTiming, StageUpdate, TaskTiming,
};
pub use job::{accept_input_path, next_queue_seq, DurationState, Task, TaskStatus};
pub use lang::{
    normalize_source_language, source_language_by_id, SOURCE_LANGUAGES, SourceLanguage,
};
pub use media::{probe_duration_async, probe_duration_sec, resolve_app_root};
pub use model::{
    default_aligner_model_dir, default_asr_model_dir, download_model, init_native_library_path,
    is_cuda_runtime_ready, is_model_ready, resolve_cuda_runtime_dir, resolve_dll_dir,
    resolve_models_root, DownloadHandle,
    DownloadOutcome, DownloadProgress, DownloadState, ModelId, ModelKind, QWEN3_ASR_06B,
    QWEN3_ASR_17B,
};
pub use runtime::{demote_current_thread, init_runtime};
pub use settings::{
    clamp_chunk_target_seconds, Settings, CHUNK_TARGET_DEFAULT_SEC, CHUNK_TARGET_MAX_SEC,
    CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_PRESETS,
};
pub use ui_labels::{
    empty_state_subtitle, empty_state_title, format_batch_progress, format_queue_status,
};
