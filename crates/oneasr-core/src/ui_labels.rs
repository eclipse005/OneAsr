//! Pure UI label / path helpers (no GPUI, unit-testable).

use std::path::{Path, PathBuf};

use crate::job::TaskStatus;

/// Parent directory of a file path (for “open folder”).
pub fn containing_folder(file: &Path) -> Option<PathBuf> {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_path_buf())
}

/// Short status bar text for the batch queue (no percentages).
/// Sparse queues stay short so the bar doesn’t feel like a dashboard.
pub fn format_queue_status(
    total: usize,
    pending: usize,
    queued: usize,
    processing: usize,
    done: usize,
    error: usize,
) -> String {
    if total == 0 {
        return "暂无任务".into();
    }
    if processing == 0 && queued == 0 && done == 0 && error == 0 {
        return format!("{total} 个任务 · 待处理 {pending}");
    }
    let mut parts = vec![format!("共 {total}")];
    if processing > 0 {
        parts.push(format!("处理中 {processing}"));
    }
    if queued > 0 {
        parts.push(format!("排队 {queued}"));
    }
    if pending > 0 {
        parts.push(format!("待处理 {pending}"));
    }
    if done > 0 {
        parts.push(format!("完成 {done}"));
    }
    if error > 0 {
        parts.push(format!("错误 {error}"));
    }
    parts.join("  ·  ")
}

/// Empty-state headline (list center — never shows model-loading; that belongs in status bar).
pub fn empty_state_title(_model_loading: bool) -> &'static str {
    "列表为空"
}

/// Empty-state subtitle.
pub fn empty_state_subtitle(model_loading: bool, model_failed: bool) -> &'static str {
    if model_failed {
        "模型加载失败，请在设置中检查模型目录与后端"
    } else if model_loading {
        "可拖入或添加文件；模型状态见右下角"
    } else {
        "拖入音视频到此处，或点击「添加」"
    }
}

/// Whether a completed task should show open-folder actions.
pub fn can_open_output(status: TaskStatus, output_srt: Option<&Path>) -> bool {
    status == TaskStatus::Done && output_srt.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn containing_folder_of_srt() {
        let p = PathBuf::from(r"D:\OneAsr\output\clip.srt");
        assert_eq!(
            containing_folder(&p),
            Some(PathBuf::from(r"D:\OneAsr\output"))
        );
    }

    #[test]
    fn queue_status_has_remaining_no_percent() {
        let s = format_queue_status(5, 2, 1, 1, 1, 0);
        assert!(s.contains("待处理 2"));
        assert!(s.contains("排队 1"));
        assert!(s.contains("处理中 1"));
        assert!(!s.contains('%'));
        assert_eq!(format_queue_status(0, 0, 0, 0, 0, 0), "暂无任务");
        assert_eq!(format_queue_status(1, 1, 0, 0, 0, 0), "1 个任务 · 待处理 1");
    }

    #[test]
    fn empty_center_not_model_loading() {
        assert_eq!(empty_state_title(true), "列表为空");
        assert!(!empty_state_title(true).contains("加载模型"));
        assert!(empty_state_subtitle(true, false).contains("右下角"));
        assert!(empty_state_subtitle(false, false).contains("拖入"));
    }

    #[test]
    fn can_open_only_when_done_with_path() {
        let p = PathBuf::from(r"D:\OneAsr\output\a.srt");
        assert!(can_open_output(TaskStatus::Done, Some(&p)));
        assert!(!can_open_output(TaskStatus::Pending, Some(&p)));
        assert!(!can_open_output(TaskStatus::Done, None));
    }
}
