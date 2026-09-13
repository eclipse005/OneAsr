//! OneAsr core: Qwen ASR + ForcedAligner pipeline, sentence boundary, jobs.

pub mod asr;
mod diagnostics;
pub mod engine;
pub mod job;
pub mod lang;
pub mod media;
pub mod model;
pub mod paths;
pub mod runtime;
pub mod sentence_boundary;
pub mod settings;
pub mod stats;
pub mod subtitle;
pub mod subtitle_length;
pub mod text_script;
pub mod ui_labels;
pub mod vad;

pub use asr::{
    AsrStage, ProcessExportOptions, StageClock, StageTiming, StageUpdate, TaskTiming,
    check_aligner_model_dir, check_asr_model_dir, check_demucs_model_dir, format_process_ms,
    process_media_file_with_export, process_media_file_with_progress,
    process_media_file_with_provider,
};
pub use engine::{
    AlignRequest, AlignedToken, Aligner, AsrEngine, EngineError, EngineProvider, SeparateRequest,
    SeparationEvent, Separator, TranscribeRequest, Transcript,
};
pub use job::{DurationState, Task, TaskStatus, accept_input_path, next_queue_seq};
pub use lang::{
    SOURCE_LANGUAGES, SourceLanguage, normalize_source_language, source_language_by_id,
};
pub use media::{ffmpeg_present, probe_duration_async, probe_duration_sec, resolve_app_root};
pub use model::{
    DownloadHandle, DownloadOutcome, DownloadProgress, DownloadState, HTDEMUCS_FT, ModelId,
    ModelKind, QWEN3_ASR_06B, QWEN3_ASR_17B, default_aligner_model_dir, default_asr_model_dir,
    default_demucs_model_dir, download_model, init_native_library_path, is_cuda_runtime_ready,
    is_model_ready, probe_writable, resolve_app_root_dir, resolve_cuda_runtime_dir,
    resolve_dll_dir, resolve_models_root,
};
pub use runtime::{demote_current_thread, init_runtime};
pub use settings::{
    CHUNK_TARGET_DEFAULT_SEC, CHUNK_TARGET_MAX_SEC, CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_PRESETS,
    Settings, SettingsLoadReport, clamp_chunk_target_seconds,
};
pub use text_script::{TextScript, applies_to_language as script_applies_to_language};
pub use ui_labels::{
    empty_state_subtitle, empty_state_title, format_batch_progress, format_queue_status,
};
