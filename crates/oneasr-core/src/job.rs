//! Batch task model for the list UI (not a subtitle editor).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskStatus {
    /// 待处理
    Pending,
    /// 处理中
    Processing,
    /// 完成
    Done,
    /// 错误
    Error,
}

impl TaskStatus {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pending => "待处理",
            Self::Processing => "处理中",
            Self::Done => "完成",
            Self::Error => "错误",
        }
    }
}

/// Duration probe lifecycle for list display.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub size_bytes: u64,
    /// Duration probe state (… → value / —).
    pub duration: DurationState,
    /// Extension / container, e.g. `wav`, `mp4`.
    pub format: String,
    pub status: TaskStatus,
    pub error: Option<String>,
    /// Output SRT path when done.
    pub output_srt: Option<PathBuf>,
}

impl Task {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
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
        let id = new_id();
        Self {
            id,
            path,
            name,
            size_bytes,
            duration: DurationState::Probing,
            format,
            status: TaskStatus::Pending,
            error: None,
            output_srt: None,
        }
    }

    pub fn size_label(&self) -> String {
        format_bytes(self.size_bytes)
    }

    pub fn duration_label(&self) -> String {
        self.duration.label()
    }

    pub fn set_duration(&mut self, sec: Option<f64>) {
        self.duration = match sec {
            Some(s) if s.is_finite() && s >= 0.0 => DurationState::Known(s),
            _ => DurationState::Unknown,
        };
    }
}

/// Unique task id (atomic counter — batch add must not collide).
fn new_id() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("t{n}")
}

pub fn format_bytes(n: u64) -> String {
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

pub fn format_duration(sec: f64) -> String {
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

pub fn is_media_path(path: &Path) -> bool {
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
    // Some Windows dialogs return paths that are files but is_file is flaky.
    std::fs::metadata(path).map(|m| m.is_file()).unwrap_or(false)
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
        let a = Task::from_path("a.wav");
        let b = Task::from_path("b.wav");
        let c = Task::from_path("c.wav");
        assert_ne!(a.id, b.id);
        assert_ne!(b.id, c.id);
        assert_ne!(a.id, c.id);
    }
}
