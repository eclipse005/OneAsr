//! Batch task model for the list rows.
//!
//! Lives in the GUI crate: `Task` / `TaskStatus` / `DurationState` describe the
//! batch list, and nothing in `oneasr-core` consumes them. `TaskTiming` stays
//! in core (it is produced by the pipeline).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use oneasr_core::{TaskTiming, normalize_source_language};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    /// 待处理（未加入运行队列）
    Pending,
    /// 已点开始，等待轮到（同一时间只跑一个）
    Queued,
    /// 处理中
    Processing,
    /// 完成
    Done,
    /// 错误
    Error,
}

impl TaskStatus {
    /// Only the active ASR job locks start/delete.
    pub fn locks_row_actions(self) -> bool {
        matches!(self, Self::Processing)
    }
}

/// Duration probe lifecycle for list display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DurationState {
    /// Background probe in flight.
    Probing,
    /// Known length in seconds.
    Known(f64),
    /// Probe finished but duration unavailable.
    Unknown,
}

impl DurationState {
    pub fn label(self) -> String {
        match self {
            Self::Probing => "…".into(),
            Self::Known(s) if s.is_finite() && s >= 0.0 => format_duration(s),
            Self::Known(_) | Self::Unknown => "—".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    pub duration: DurationState,
    /// Extension / container, e.g. `wav`, `mp4`.
    pub format: String,
    /// Source language short id (`zh`, `en`, …). Per-task; defaults from settings.
    pub language: String,
    /// Run HTDemucs vocal separation for this task; defaults from settings,
    /// overridable per row (same model as [`Self::language`]).
    pub vocal_separation: bool,
    pub status: TaskStatus,
    pub error: Option<String>,
    /// FIFO order when [`TaskStatus::Queued`] (lower runs first).
    pub queue_seq: Option<u64>,
    /// Primary deliverable written by a finished run (`.srt`, or `.txt` when
    /// SRT output is switched off) — what the row's “打开” action reveals.
    pub output_file: Option<PathBuf>,
    /// Per-stage processing wall time (set when a run finishes or fails mid-way).
    pub timing: Option<TaskTiming>,
}

impl Task {
    /// Build a task; `language` is normalized to a catalog short id (e.g. `zh`).
    /// `vocal_separation` is the settings default captured when the row is added.
    pub fn from_path(
        path: impl Into<PathBuf>,
        language: impl Into<String>,
        vocal_separation: bool,
    ) -> Self {
        let path = path.into();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let format = path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_else(|| "—".into());
        Self {
            id: new_id(),
            path,
            name,
            size_bytes,
            duration: DurationState::Probing,
            format,
            language: normalize_source_language(&language.into()),
            vocal_separation,
            status: TaskStatus::Pending,
            error: None,
            queue_seq: None,
            output_file: None,
            timing: None,
        }
    }

    /// Set source language (normalized to a known catalog id).
    pub fn set_language(&mut self, language: impl AsRef<str>) {
        self.language = normalize_source_language(language.as_ref());
    }

    /// Toggle per-task vocal separation.
    pub fn set_vocal_separation(&mut self, enabled: bool) {
        self.vocal_separation = enabled;
    }

    pub fn size_label(&self) -> String {
        format_bytes(self.size_bytes)
    }

    pub fn set_duration(&mut self, sec: Option<f64>) {
        self.duration = match sec {
            Some(s) if s.is_finite() && s >= 0.0 => DurationState::Known(s),
            _ => DurationState::Unknown,
        };
    }
}

fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("t{n}")
}

/// Monotonic queue sequence for FIFO start order.
pub fn next_queue_seq() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let x = n as f64;
    if x >= GB {
        format!("{:.2} GB", x / GB)
    } else if x >= MB {
        format!("{:.2} MB", x / MB)
    } else if x >= KB {
        format!("{:.1} KB", x / KB)
    } else {
        format!("{n} B")
    }
}

fn format_duration(sec: f64) -> String {
    let s = sec.max(0.0).round() as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let r = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{r:02}")
    } else {
        format!("{m}:{r:02}")
    }
}

fn is_media_path(path: &Path) -> bool {
    const EXT: &[&str] = &[
        "wav", "mp3", "m4a", "aac", "flac", "ogg", "opus", "wma", "mp4", "mkv", "mov", "webm",
        "avi", "flv", "ts", "m4v", "mpeg", "mpg", "wmv", "3gp",
    ];
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| EXT.iter().any(|x| e.eq_ignore_ascii_case(x)))
        .unwrap_or(false)
}

/// Accept existing **media** files only (rejects `.srt` and other non-media).
pub fn accept_input_path(path: &Path) -> bool {
    if !is_media_path(path) {
        return false;
    }
    if path.is_file() {
        return true;
    }
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rejects_srt_and_non_media() {
        assert!(!is_media_path(Path::new("a.srt")));
        assert!(!is_media_path(Path::new("notes.txt")));
        assert!(!accept_input_path(Path::new("a.srt")));
        assert!(is_media_path(Path::new("clip.WAV")));
        assert!(is_media_path(Path::new("v.mp4")));
    }

    #[test]
    fn duration_labels() {
        assert_eq!(DurationState::Probing.label(), "…");
        assert_eq!(DurationState::Unknown.label(), "—");
        assert_eq!(DurationState::Known(90.0).label(), "1:30");
    }

    #[test]
    fn task_ids_are_unique() {
        let a = Task::from_path("a.wav", "zh", false);
        let b = Task::from_path("b.wav", "en", true);
        let c = Task::from_path("c.wav", "ja", false);
        assert_ne!(a.id, b.id);
        assert_ne!(b.id, c.id);
        assert_ne!(a.id, c.id);
        assert_eq!(a.language, "zh");
        assert_eq!(b.language, "en");
        assert!(!a.vocal_separation);
        assert!(b.vocal_separation);
    }
}
