//! OneAsr core: batch jobs, prompts, **subtitle engine**, media, ASR.
//!
//! ## Layers
//!
//! 1. **ASR** (`asr`) — media → raw MOSS compact string  
//! 2. **Engine** (`engine`) — raw → structured segments → SRT/VTT/JSON/plain  
//! 3. **Job / UI** — batch list, paths under `output/{stem}.srt`

pub mod asr;
pub mod engine;
pub mod job;
pub mod media;
pub mod paths;
pub mod prompt;
pub mod settings;
pub mod ui_labels;

// Thin compatibility modules (re-exports into `engine`).
pub mod export;
pub mod parse;
pub mod segment;

pub use asr::{
    planned_output_path, preload_model, process_media_file, session_matches, unload_session,
    AsrError,
};
pub use engine::{
    export_json, export_plain, export_srt, export_vtt, format_srt_time, format_vtt_time,
    parse_transcript, ExportOptions, ParseError, Segment, SubtitleFormat, TranscriptDocument,
    TranscriptEngine,
};
pub use job::{
    accept_input_path, format_bytes, format_duration, is_media_path, DurationState, Task,
    TaskStatus,
};
pub use media::{
    convert_to_16k_mono_wav, ffmpeg_available, probe_duration_sec, resolve_app_root, resolve_bin_dir,
    resolve_ffmpeg, MediaError,
};
pub use paths::{media_stem, output_srt_path};
pub use prompt::{build_prompt, prompt_preview, PROMPT_BASE};
pub use settings::Settings;
pub use ui_labels::{
    can_open_output, containing_folder, empty_state_subtitle, empty_state_title, format_queue_status,
};
