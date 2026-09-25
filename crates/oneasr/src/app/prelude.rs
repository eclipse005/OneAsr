//! Shared imports for the GUI crate's state and view modules.
//!
//! GPUI code is import-heavy: nearly every method needs `div`/`px`, a dozen
//! theme tokens, the shared widgets and a few core types. Repeating a 40-line
//! import block per module would make the split harder to keep in sync than the
//! monolith was, so the app and view modules glob this prelude.
//!
//! Glob imports are deliberate: they do not raise unused-import warnings as
//! methods move between files, and every name below is app-layer vocabulary.

pub(crate) use std::collections::HashMap;
pub(crate) use std::f32::consts::TAU;
pub(crate) use std::path::PathBuf;
pub(crate) use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use gpui::{
    AnyElement, Bounds, BoxShadow, Context, ExternalPaths, FocusHandle, IntoElement,
    MouseMoveEvent,
    Pixels, Rgba,
    SharedString, Timer, Window, WindowControlArea, canvas, deferred, div, hsla, point,
    prelude::*, px, svg,
};

pub(crate) use oneasr_core::{
    AsrStage, DownloadHandle, DownloadProgress, DownloadState, ModelId, ModelKind, Settings,
    StageClock, StageUpdate, TaskTiming, TextScript, check_asr_model_dir, demote_current_thread,
    download_model, ffmpeg_source, is_model_ready,
    i18n::{t, UiLang, ui_lang},
    normalize_source_language, process_media_file_with_progress, probe_duration_async,
    probe_writable, resolve_app_root_dir,
    source_language_by_id,
    stats::{StatsRecord, StatsSummary},
    CHUNK_TARGET_MAX_SEC, CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_PRESETS,
};

pub(crate) use crate::i18n::L;

pub(crate) use crate::app::task::{
    DurationState, Task, TaskStatus, accept_input_path, next_queue_seq,
};
pub(crate) use crate::app::ui::metrics::*;
pub(crate) use crate::app::ui::text::*;
pub(crate) use crate::app::ui::util::*;
pub(crate) use crate::app::worker::{AsrJob, WorkerMsg};
pub(crate) use crate::app::{
    LangMenuLayout, LangSelectTarget, ModelStatus, OneAsrApp, TaskRowView,
};
pub(crate) use crate::theme::*;
pub(crate) use crate::ui_font;
pub(crate) use crate::widgets::*;
pub(crate) use crate::{crashlog, shell, sfx};
