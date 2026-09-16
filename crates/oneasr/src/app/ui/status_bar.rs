//! The bottom status bar, and the live-row tally it reports.
//!
//! The tally is separated from the painting on purpose: it is the only thing
//! here with a rule worth testing (fading rows must not be counted).

use crate::app::prelude::*;
use crate::app::{ModelStatus, OneAsrApp};

impl OneAsrApp {
    /// Bottom status bar: queue/batch progress on the left with the persistent
    /// stats chip, model/hint indicator on the right.
    pub(super) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tally = tally_tasks(&self.tasks, &self.exiting);
        let batch = self.batch_mode;
        // Sequential batch: one line only — 进度 n/m (no "共 n · 处理中" echo).
        // Denominator = finished + live unfinished rows, derived at paint time:
        // deleting or adding tasks mid-run keeps it pointing at real rows.
        let left: SharedString = if batch {
            format_batch_progress(
                self.batch_done,
                self.batch_done + tally.pending + tally.queued + tally.proc,
            )
            .into()
        } else {
            format_queue_status(
                tally.total, tally.pending, tally.queued, tally.proc, tally.done, tally.err,
            )
            .into()
        };
        let hint = self.status_hint.clone();
        let hint_good = self.status_hint_good;
        let stats = &self.stats;
        let stats_label: SharedString = match stats.saved_sec() {
            Some(secs) => format!("已省 {}", oneasr_core::stats::format_span_secs(secs)).into(),
            None => "统计".into(),
        };
        let stats_has_data = !stats.is_empty();
        let model = self.model_status;

        div()
            .h(px(34.))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .bg(PANEL)
            .border_t_1()
            .border_color(LINE)
            .text_xs()
            .text_color(MUTED)
            // Left: idle = queue summary; batch = progress only.
            .child(status_bar_left(
                left,
                batch,
                stats_label,
                stats_has_data,
                cx,
            ))
            // Right: model probe (常驻) OR transient interaction hint.
            .child(status_bar_right(hint, hint_good, model))
    }
}

// ── Live-row tally (pure: no gpui, unit-tested below) ───────────────────────

/// How many rows the status bar should report, by state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct TaskTally {
    pub total: usize,
    pub pending: usize,
    pub queued: usize,
    pub proc: usize,
    pub done: usize,
    pub err: usize,
}

/// Count rows by status.
///
/// Rows in `exiting` are **not** counted: they are tombstones kept alive only
/// to finish their fade-out, so counting them would make the tally disagree
/// with what the user can see (and make a delete look like it did nothing).
pub(super) fn tally_tasks(tasks: &[Task], exiting: &HashMap<String, Instant>) -> TaskTally {
    let mut tally = TaskTally::default();
    for task in tasks {
        if exiting.contains_key(&task.id) {
            continue;
        }
        tally.total += 1;
        match task.status {
            TaskStatus::Pending => tally.pending += 1,
            TaskStatus::Queued => tally.queued += 1,
            TaskStatus::Processing => tally.proc += 1,
            TaskStatus::Done => tally.done += 1,
            TaskStatus::Error => tally.err += 1,
        }
    }
    tally
}

// ── Painting ────────────────────────────────────────────────────────────────

/// Progress text plus the persistent stats chip.
fn status_bar_left(
    left: SharedString,
    batch: bool,
    stats_label: SharedString,
    stats_has_data: bool,
    cx: &mut Context<OneAsrApp>,
) -> impl IntoElement + use<> {
    div()
        .flex()
        .items_center()
        .gap_3()
        .min_w_0()
        .child(
            div()
                .when(batch, |el| {
                    el.text_color(ACCENT)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .child(left),
        )
        // Persistent clickable chip: the stats entry point, and the emotional
        // payload itself (it changes as you use the app).
        .child(
            div()
                .id("stats-chip")
                .flex_shrink_0()
                .px_2()
                .py_0p5()
                .rounded_full()
                .cursor_pointer()
                .text_color(if stats_has_data { ACCENT } else { MUTED })
                .when(stats_has_data, |el| {
                    el.bg(ACCENT_SOFT)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .hover(|s| s.bg(ACCENT_MIST))
                .child(stats_label)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_stats(cx))),
        )
}

/// Transient interaction hint when there is one, otherwise the model probe.
fn status_bar_right(
    hint: Option<SharedString>,
    hint_good: bool,
    model: ModelStatus,
) -> impl IntoElement + use<> {
    let model_color = match model {
        ModelStatus::Ready => ACCENT,
        ModelStatus::NotReady => DANGER,
    };
    div()
        .flex()
        .items_center()
        .gap_2()
        .min_w_0()
        .max_w(px(360.))
        .child(match hint {
            Some(h) => div()
                .flex()
                .items_center()
                .gap_2()
                .min_w_0()
                .bg(if hint_good { ACCENT_SOFT } else { WARN_SOFT })
                .px_2()
                .py_0p5()
                .rounded_full()
                .child(
                    div()
                        .size(px(7.))
                        .rounded_full()
                        .bg(if hint_good { ACCENT } else { WARN }),
                )
                .child(
                    div()
                        .text_color(if hint_good { ACCENT } else { WARN })
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .truncate()
                        .child(h),
                )
                .into_any_element(),
            None => div()
                .flex()
                .items_center()
                .gap_2()
                .bg(match model {
                    ModelStatus::Ready => ACCENT_MIST,
                    ModelStatus::NotReady => DANGER_SOFT,
                })
                .px_2()
                .py_0p5()
                .rounded_full()
                .child(div().size(px(7.)).rounded_full().bg(model_color))
                .child(
                    div()
                        .text_color(model_color)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(match model {
                            ModelStatus::Ready => "模型就绪",
                            ModelStatus::NotReady => "模型未就绪",
                        }),
                )
                .into_any_element(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(status: TaskStatus) -> Task {
        let mut t = Task::from_path("media/clip.wav", "zh", false);
        t.status = status;
        t
    }

    #[test]
    fn tally_counts_every_state() {
        let tasks = vec![
            task(TaskStatus::Pending),
            task(TaskStatus::Queued),
            task(TaskStatus::Processing),
            task(TaskStatus::Done),
            task(TaskStatus::Error),
        ];
        let t = tally_tasks(&tasks, &HashMap::new());
        assert_eq!(
            t,
            TaskTally {
                total: 5,
                pending: 1,
                queued: 1,
                proc: 1,
                done: 1,
                err: 1,
            }
        );
    }

    #[test]
    fn fading_rows_are_not_counted() {
        let mut tasks = vec![task(TaskStatus::Done), task(TaskStatus::Pending)];
        let fading = tasks[0].id.clone();
        let mut exiting = HashMap::new();
        exiting.insert(fading, Instant::now());

        let t = tally_tasks(&tasks, &exiting);
        assert_eq!(t.total, 1);
        assert_eq!(t.done, 0);
        assert_eq!(t.pending, 1);

        // A row mid-fade is still present once the tombstone is dropped.
        tasks.remove(0);
        assert_eq!(tally_tasks(&tasks, &exiting).total, 1);
    }

    #[test]
    fn empty_list_tallies_to_zero() {
        assert_eq!(tally_tasks(&[], &HashMap::new()), TaskTally::default());
    }
}
