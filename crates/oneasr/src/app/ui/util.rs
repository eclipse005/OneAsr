//! Small pure helpers shared by the view layer (and by a few state methods).
//!
//! Nothing here touches `OneAsrApp`; each function maps its arguments to a
//! value, which keeps them unit-testable without a GPUI context.

use gpui::{IntoElement, Rgba, div, prelude::*, px, svg};

use oneasr_core::i18n::UiLang;

use crate::app::task::TaskStatus;
use crate::theme::{
    ACCENT, ACCENT_SOFT, DANGER, DANGER_SOFT, LINE, LINE_SOFT, MEDIA_PLATE, MUTED, WARN, WARN_SOFT,
};

/// Smooth deceleration (approx cubic-bezier ease-out).
pub(crate) fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

/// A felt yardstick for the hero number — "12 分" is a stopwatch reading, this
/// is what it buys. Deliberately coarse and never user-facing as a conversion:
/// the point is the shape of the amount, not a second precision. `None` under
/// ten minutes, where every comparison would sound like flattery.
pub(crate) fn saved_tale(secs: f64, lang: UiLang) -> Option<&'static str> {
    let min = secs / 60.0;
    match (lang, min) {
        (_, m) if m < 10.0 => None,
        (UiLang::Zh, m) if m < 40.0 => Some("≈ 一集播客"),
        (UiLang::Zh, m) if m < 100.0 => Some("≈ 一集电视剧"),
        (UiLang::Zh, m) if m < 240.0 => Some("≈ 一部电影"),
        (UiLang::Zh, m) if m < 480.0 => Some("≈ 半个工作日"),
        (UiLang::Zh, _) => Some("≈ 一个工作日"),
        (UiLang::En, m) if m < 40.0 => Some("≈ a podcast episode"),
        (UiLang::En, m) if m < 100.0 => Some("≈ a TV episode"),
        (UiLang::En, m) if m < 240.0 => Some("≈ a movie"),
        (UiLang::En, m) if m < 480.0 => Some("≈ half a workday"),
        (UiLang::En, _) => Some("≈ a full workday"),
    }
}

/// Sentence/line count from an exported subtitle file: SRT cue blocks are
/// bare index lines, TXT is one non-empty line per sentence — both come
/// from the same sentence list, so either file yields the same number.
/// 0 when the file cannot be read: the ledger tolerates gaps, never invents.
pub(crate) fn count_output_lines(path: &std::path::Path) -> u32 {
    let Ok(text) = std::fs::read_to_string(path) else {
        return 0;
    };
    if text.contains("-->") {
        text.lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && t.chars().all(|c| c.is_ascii_digit())
            })
            .count() as u32
    } else {
        text.lines().filter(|l| !l.trim().is_empty()).count() as u32
    }
}

/// `12483` → `12,483`, for the 输出文本 row.
pub(crate) fn format_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub(crate) fn is_video_format(fmt: &str) -> bool {
    matches!(
        fmt.to_ascii_lowercase().as_str(),
        "mp4"
            | "mkv"
            | "mov"
            | "webm"
            | "avi"
            | "flv"
            | "ts"
            | "m4v"
            | "mpeg"
            | "mpg"
            | "wmv"
            | "3gp"
    )
}

/// Status accent for media plate border (replaces free-floating status circle).
pub(crate) fn status_border_color(status: TaskStatus) -> Rgba {
    match status {
        TaskStatus::Pending => LINE,
        TaskStatus::Queued => MUTED,
        TaskStatus::Processing => WARN,
        TaskStatus::Done => ACCENT,
        TaskStatus::Error => DANGER,
    }
}

/// Status pill: (label, foreground, soft background). `stage` is the live
/// pipeline label (already translated **and already carrying `3/5`** — see
/// `StageUpdate::label`); everything else is per `lang`.
///
/// 不要再在这里拼一次块数：标签里已经有一个了。
pub(crate) fn status_pill_style(
    status: TaskStatus,
    queue_n: Option<usize>,
    stage: Option<&str>,
    lang: UiLang,
) -> (String, Rgba, Rgba) {
    let pending = match lang {
        UiLang::Zh => "待处理",
        UiLang::En => "Pending",
    };
    let processing = match lang {
        UiLang::Zh => "处理中",
        UiLang::En => "Processing",
    };
    let done = match lang {
        UiLang::Zh => "完成",
        UiLang::En => "Done",
    };
    let error = match lang {
        UiLang::Zh => "错误",
        UiLang::En => "Error",
    };
    match status {
        TaskStatus::Pending => (pending.into(), MUTED, MEDIA_PLATE),
        TaskStatus::Queued => {
            let n = queue_n.unwrap_or(0);
            let label = match lang {
                UiLang::Zh => format!("排队#{n}"),
                UiLang::En => format!("Queued #{n}"),
            };
            (label, MUTED, MEDIA_PLATE)
        }
        TaskStatus::Processing => {
            let base = stage.unwrap_or(processing);
            (base.to_string(), WARN, WARN_SOFT)
        }
        TaskStatus::Done => (done.into(), ACCENT, ACCENT_SOFT),
        TaskStatus::Error => (error.into(), DANGER, DANGER_SOFT),
    }
}

/// Left edge accent for scannable error / active rows.
pub(crate) fn row_accent(status: TaskStatus) -> Rgba {
    match status {
        TaskStatus::Processing => WARN,
        TaskStatus::Error => DANGER,
        TaskStatus::Done => ACCENT,
        TaskStatus::Pending | TaskStatus::Queued => LINE_SOFT,
    }
}

/// Compact SVG media mark (video clapper / audio speaker); border = status tint.
pub(crate) fn media_type_icon(is_video: bool, status: TaskStatus) -> impl IntoElement {
    let border = status_border_color(status);
    let icon_path = if is_video {
        "icons/video.svg"
    } else {
        "icons/audio.svg"
    };
    let ink = match status {
        TaskStatus::Processing => WARN,
        TaskStatus::Done => ACCENT,
        TaskStatus::Error => DANGER,
        TaskStatus::Pending | TaskStatus::Queued => MUTED,
    };
    div()
        .size(px(36.))
        .rounded_lg()
        .bg(MEDIA_PLATE)
        .border_1()
        .border_color(border)
        .flex()
        .items_center()
        .justify_center()
        .child(svg().size(px(18.)).path(icon_path).text_color(ink))
}

/// `(done, total)` → 行内进度条的填充比例（0.0–1.0）。
///
/// 三种输入都收在同一个出口，因为**每一个都能从真实管线里走到**：
/// `total == 0` 是管线还没数出块（分母未知 → 不画条，而不是除零画满），
/// `done > total` 是管线换了分母（换段、换阶段）而旧值还在飞越的间隙，
/// 正常情况才是 `0 <= done <= total`。夹到 1.0 就好——界面只需要一个比例，
/// 倒退一格比短暂画满更难解释。
pub(crate) fn stage_fraction(chunk: Option<(usize, usize)>) -> Option<f32> {
    let (done, total) = chunk?;
    (total > 0).then(|| (done as f32 / total as f32).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::stage_fraction;

    #[test]
    fn no_chunk_means_no_bar() {
        assert_eq!(stage_fraction(None), None);
    }

    #[test]
    fn zero_total_never_becomes_a_full_bar() {
        // 分母还没数出来的时候画满，比不画条更像「已经做完了」。
        assert_eq!(stage_fraction(Some((0, 0))), None);
    }

    #[test]
    fn a_chunked_stage_reads_as_its_own_fraction() {
        let f = stage_fraction(Some((12, 42))).expect("chunked stage has a bar");
        assert!((f - 12.0 / 42.0).abs() < 1e-6, "got {f}");
    }

    #[test]
    fn a_denominator_that_shrinks_clamps_instead_of_overshooting() {
        assert_eq!(stage_fraction(Some((9, 3))), Some(1.0));
    }

    use super::{TaskStatus, status_pill_style};
    use oneasr_core::i18n::UiLang;

    /// 块数是 `StageUpdate::label` 拼进标签的，胶囊里**已经有一个**。
    /// 在这里再拼一次就成了 `转写中 3/5 3/5` —— 这条钉住「不在这里拼」。
    #[test]
    fn the_pill_does_not_append_a_second_count() {
        let (label, _, _) =
            status_pill_style(TaskStatus::Processing, None, Some("转写中 3/5"), UiLang::Zh);
        assert_eq!(label, "转写中 3/5");
        assert_eq!(label.matches('/').count(), 1, "count got printed twice");
    }

    /// 文稿路径那一段是空的：没有分母就没有条，胶囊照旧只显示「打轴中」。
    #[test]
    fn a_countless_stage_pill_is_the_bare_label() {
        let (label, _, _) =
            status_pill_style(TaskStatus::Processing, None, Some("打轴中"), UiLang::Zh);
        assert_eq!(label, "打轴中");
        assert_eq!(stage_fraction(None), None, "no denominator means no bar");
    }
}
