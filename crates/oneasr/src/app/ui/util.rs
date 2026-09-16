//! Small pure helpers shared by the view layer (and by a few state methods).
//!
//! Nothing here touches `OneAsrApp`; each function maps its arguments to a
//! value, which keeps them unit-testable without a GPUI context.

use gpui::{IntoElement, Rgba, div, prelude::*, px, svg};

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
pub(crate) fn saved_tale(secs: f64) -> Option<&'static str> {
    let min = secs / 60.0;
    match min {
        m if m < 10.0 => None,
        m if m < 40.0 => Some("≈ 一集播客"),
        m if m < 100.0 => Some("≈ 一集电视剧"),
        m if m < 240.0 => Some("≈ 一部电影"),
        m if m < 480.0 => Some("≈ 半个工作日"),
        _ => Some("≈ 一个工作日"),
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
        "mp4" | "mkv" | "mov" | "webm" | "avi" | "flv" | "ts" | "m4v" | "mpeg" | "mpg" | "wmv"
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

/// Status pill: (label, foreground, soft background).
pub(crate) fn status_pill_style(
    status: TaskStatus,
    queue_n: Option<usize>,
    stage: Option<&str>,
) -> (String, Rgba, Rgba) {
    match status {
        TaskStatus::Pending => ("待处理".into(), MUTED, MEDIA_PLATE),
        TaskStatus::Queued => {
            let n = queue_n.unwrap_or(0);
            (format!("排队#{n}"), MUTED, MEDIA_PLATE)
        }
        TaskStatus::Processing => {
            let base = stage.unwrap_or("处理中");
            (base.to_string(), WARN, WARN_SOFT)
        }
        TaskStatus::Done => ("完成".into(), ACCENT, ACCENT_SOFT),
        TaskStatus::Error => ("错误".into(), DANGER, DANGER_SOFT),
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
        .child(
            svg()
                .size(px(18.))
                .path(icon_path)
                .text_color(ink),
        )
}
