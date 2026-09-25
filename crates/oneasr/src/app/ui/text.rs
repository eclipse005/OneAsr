//! Pure UI label helpers (no GPUI, unit-testable).
//!
//! Lives in the GUI crate: these are interface copy, not pipeline logic.
//! Kept free of GPUI so the wording stays unit-testable. All of them take a
//! [`UiLang`] explicitly — no global state — so tests can cover both languages.

use oneasr_core::i18n::UiLang;

/// Short status bar text for the batch queue (no percentages).
pub fn format_queue_status(
    lang: UiLang,
    total: usize,
    pending: usize,
    queued: usize,
    processing: usize,
    done: usize,
    error: usize,
) -> String {
    if total == 0 {
        return match lang {
            UiLang::Zh => "暂无任务".into(),
            UiLang::En => "No tasks".into(),
        };
    }
    if processing == 0 && queued == 0 && done == 0 && error == 0 {
        return match lang {
            UiLang::Zh => format!("{total} 个任务 · 待处理 {pending}"),
            UiLang::En if total == 1 => format!("1 task · {pending} pending"),
            UiLang::En => format!("{total} tasks · {pending} pending"),
        };
    }
    let mut parts = Vec::with_capacity(6);
    match lang {
        UiLang::Zh => {
            parts.push(format!("共 {total}"));
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
        }
        UiLang::En => {
            parts.push(format!("{total} total"));
            if processing > 0 {
                parts.push(format!("{processing} processing"));
            }
            if queued > 0 {
                parts.push(format!("{queued} queued"));
            }
            if pending > 0 {
                parts.push(format!("{pending} pending"));
            }
            if done > 0 {
                parts.push(format!("{done} done"));
            }
            if error > 0 {
                parts.push(format!("{error} failed"));
            }
        }
    }
    parts.join("  ·  ")
}

/// Batch run progress, e.g. `进度 2/5`.
pub fn format_batch_progress(lang: UiLang, finished: usize, goal: usize) -> String {
    let goal = goal.max(1);
    let finished = finished.min(goal);
    match lang {
        UiLang::Zh => format!("进度 {finished}/{goal}"),
        UiLang::En => format!("Progress {finished}/{goal}"),
    }
}

pub fn empty_state_title(lang: UiLang) -> &'static str {
    match lang {
        UiLang::Zh => "列表为空",
        UiLang::En => "No tasks yet",
    }
}

/// Empty-state subtitle. `model_not_ready` when probe failed.
pub fn empty_state_subtitle(lang: UiLang, model_not_ready: bool) -> &'static str {
    match (lang, model_not_ready) {
        (UiLang::Zh, true) => "请先在设置中选择完整模型目录，再添加文件",
        (UiLang::Zh, false) => "拖入音视频到此处，或点击下方按钮添加",
        (UiLang::En, true) => {
            "Pick complete model folders in Settings first, then add files"
        }
        (UiLang::En, false) => {
            "Drop audio / video files here, or add them with the buttons below"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_status_has_remaining_no_percent() {
        let s = format_queue_status(UiLang::Zh, 5, 2, 1, 1, 1, 0);
        assert!(s.contains("待处理 2"));
        assert!(s.contains("排队 1"));
        assert!(s.contains("处理中 1"));
        assert!(!s.contains('%'));
        assert_eq!(format_queue_status(UiLang::Zh, 0, 0, 0, 0, 0, 0), "暂无任务");
        assert_eq!(
            format_queue_status(UiLang::Zh, 1, 1, 0, 0, 0, 0),
            "1 个任务 · 待处理 1"
        );
    }

    #[test]
    fn queue_status_english() {
        let s = format_queue_status(UiLang::En, 5, 2, 1, 1, 1, 0);
        assert!(s.contains("2 pending"));
        assert!(s.contains("1 queued"));
        assert!(s.contains("1 processing"));
        assert!(!s.contains('%'));
        assert_eq!(format_queue_status(UiLang::En, 0, 0, 0, 0, 0, 0), "No tasks");
        assert_eq!(
            format_queue_status(UiLang::En, 1, 1, 0, 0, 0, 0),
            "1 task · 1 pending"
        );
    }

    #[test]
    fn batch_progress_clamps() {
        assert_eq!(format_batch_progress(UiLang::Zh, 0, 5), "进度 0/5");
        assert_eq!(format_batch_progress(UiLang::Zh, 5, 5), "进度 5/5");
        assert_eq!(format_batch_progress(UiLang::Zh, 9, 5), "进度 5/5");
        assert_eq!(format_batch_progress(UiLang::En, 2, 5), "Progress 2/5");
    }

    #[test]
    fn empty_center_copy() {
        assert_eq!(empty_state_title(UiLang::Zh), "列表为空");
        assert!(!empty_state_title(UiLang::Zh).contains("加载模型"));
        assert!(empty_state_subtitle(UiLang::Zh, true).contains("设置"));
        assert!(empty_state_subtitle(UiLang::Zh, false).contains("拖入"));
        assert_eq!(empty_state_title(UiLang::En), "No tasks yet");
        assert!(empty_state_subtitle(UiLang::En, true).contains("Settings"));
        assert!(empty_state_subtitle(UiLang::En, false).contains("Drop"));
    }
}
