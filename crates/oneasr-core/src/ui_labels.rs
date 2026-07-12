//! Pure UI label helpers (no GPUI, unit-testable).

/// Short status bar text for the batch queue (no percentages).
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

/// Batch run progress, e.g. `进度 2/5`.
pub fn format_batch_progress(finished: usize, goal: usize) -> String {
    let goal = goal.max(1);
    let finished = finished.min(goal);
    format!("进度 {finished}/{goal}")
}

pub fn empty_state_title() -> &'static str {
    "列表为空"
}

/// Empty-state subtitle. `model_not_ready` when probe failed.
pub fn empty_state_subtitle(model_not_ready: bool) -> &'static str {
    if model_not_ready {
        "请先在设置中选择完整模型目录，再添加文件"
    } else {
        "拖入音视频到此处，或点击下方按钮添加"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn batch_progress_clamps() {
        assert_eq!(format_batch_progress(0, 5), "进度 0/5");
        assert_eq!(format_batch_progress(5, 5), "进度 5/5");
        assert_eq!(format_batch_progress(9, 5), "进度 5/5");
    }

    #[test]
    fn empty_center_copy() {
        assert_eq!(empty_state_title(), "列表为空");
        assert!(!empty_state_title().contains("加载模型"));
        assert!(empty_state_subtitle(true).contains("设置"));
        assert!(empty_state_subtitle(false).contains("拖入"));
    }
}
