//! OneAsr core: batch jobs, prompts, subtitle engine, media, ASR.
//!
//! Standalone app support library — not a public crate API surface.

pub mod asr;
pub mod engine;
pub mod job;
pub mod media;
pub mod paths;
pub mod prompt;
pub mod runtime;
pub mod settings;
pub mod ui_labels;
pub mod vad;

// App + integration tests.
pub use asr::{
    check_model_dir, planned_output_path, process_media_file, process_media_file_with_progress,
    unload_session, AsrStage, StageUpdate,
};
pub use job::{
    accept_input_path, next_queue_seq, DurationState, Task, TaskStatus,
};
pub use media::{ffmpeg_available, probe_duration_sec, resolve_app_root};
pub use runtime::{demote_current_thread, init_runtime};
pub use settings::Settings;
pub use ui_labels::{
    empty_state_subtitle, empty_state_title, format_batch_progress, format_queue_status,
};
