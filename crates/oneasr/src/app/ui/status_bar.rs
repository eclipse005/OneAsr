//! The bottom status bar, and the live-row tally it reports.
//!
//! The tally is separated from the painting on purpose: it is the only thing
//! here with a rule worth testing (fading rows must not be counted).

use crate::app::prelude::*;
use crate::app::{ModelStatus, OneAsrApp};

impl OneAsrApp {
    /// Bottom status bar: queue/batch progress on the left with the persistent
    /// stats and guide chips, model/hint indicator on the right.
    pub(super) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tally = tally_tasks(&self.tasks, &self.exiting);
        // Sequential batch: one line only — 进度 n/m (no "共 n · 处理中" echo).
        // Denominator = finished + live unfinished rows, derived at paint time:
        // deleting or adding tasks mid-run keeps it pointing at real rows.
        self.status_bar_text.refresh(StatusBarKey {
            lang: ui_lang(),
            batch: self.batch_mode,
            batch_done: self.batch_done,
            tally,
            saved_sec: self.stats.saved_sec().map(f64::to_bits),
        });
        let left = self.status_bar_text.left.clone();
        let batch = self.batch_mode;
        let hint = self.status_hint.clone();
        let hint_good = self.status_hint_good;
        let stats_label = self.status_bar_text.chip.clone();
        let stats_has_data = !self.stats.is_empty();
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

// ── Text cache (pure: no gpui, unit-tested below) ───────────────────────────

/// 状态栏两处文案的**全部**输入。
///
/// 缓存键刻意就是格式化函数本身的入参，而不是一个「任务状态脏了」的标记：
/// 键逐位相同 ⇒ 输出必然逐字节相同，命中即等价，于是**没有任何地方需要去
/// 失效它**。代价是任务状态每次变化都要重算一次——那本来就是必须的。
/// 反过来，若改成脏标记，置脏点散落在 `worker.rs` / `run_control.rs` /
/// `rows.rs` / `settings.rs` 四处（状态迁移与进出淡出），漏一处就是状态栏
/// 显示上一帧的计数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatusBarKey {
    /// 影响两处文案：`format_*` 收它，`saved_prefix` 自己读全局。
    lang: UiLang,
    batch: bool,
    batch_done: usize,
    tally: TaskTally,
    /// `saved_sec()` 的位模式。f64 不能直接比较键：`0.0 == -0.0` 会让两种
    /// 位模式共用一个键，而它们格式化出来的文案未必相同。
    saved_sec: Option<u64>,
}

/// 上一次算好的两条文案。`left` 是进度行，`chip` 是「已省 …」/「统计」。
#[derive(Default)]
pub(crate) struct StatusBarTextCache {
    key: Option<StatusBarKey>,
    left: SharedString,
    chip: SharedString,
}

impl StatusBarTextCache {
    /// 键没变就复用；变了重算两条。
    ///
    /// 两条文案共用一个键：任一条的输入变了顺手把另一条也重算，是白算一次，
    /// 但换来的是「一个键对两条文案」的简单关系——省下的那点格式化开销不值得
    /// 换来两份互不同步的键。
    fn refresh(&mut self, key: StatusBarKey) {
        if self.key == Some(key) {
            return;
        }
        let (left, chip) = build_status_bar_text(key);
        self.key = Some(key);
        self.left = left.into();
        self.chip = chip.into();
    }
}

/// 键 → 两条文案。纯函数：输出只由键决定（`saved_prefix` 内部另读一次全局
/// 语言，与键里的 `lang` 同一时刻读出，二者一致）。
fn build_status_bar_text(key: StatusBarKey) -> (String, String) {
    let n = key.tally;
    let left = if key.batch {
        format_batch_progress(
            key.lang,
            key.batch_done,
            key.batch_done + n.pending + n.queued + n.proc,
        )
    } else {
        format_queue_status(
            key.lang, n.total, n.pending, n.queued, n.proc, n.done, n.err,
        )
    };
    let chip = match key.saved_sec {
        Some(bits) => crate::i18n::saved_prefix(&oneasr_core::stats::format_span_secs(
            key.lang,
            f64::from_bits(bits),
        )),
        None => t(L::STATS).to_string(),
    };
    (left, chip)
}

// ── Painting ────────────────────────────────────────────────────────────────

/// Progress text plus the persistent stats and guide chips.
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
                    el.bg(ACCENT_SOFT).font_weight(gpui::FontWeight::SEMIBOLD)
                })
                .hover(|s| s.bg(ACCENT_MIST))
                .child(stats_label)
                .on_click(cx.listener(|this, _, _, cx| this.toggle_stats(cx))),
        )
        // 常驻「指南」chip：指南面板的开关入口（首启自动展开一次，之后随时
        // 从这里重看）。词不用图标——受众恰恰是不认识图标的人。
        .child(
            div()
                .id("guide-chip")
                .flex_shrink_0()
                .px_2()
                .py_0p5()
                .rounded_full()
                .cursor_pointer()
                .text_color(MUTED)
                .hover(|s| s.bg(ACCENT_MIST))
                .child(t(L::GUIDE))
                .on_click(cx.listener(|this, _, _, cx| this.toggle_welcome(cx))),
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
                            ModelStatus::Ready => t(L::MODEL_READY),
                            ModelStatus::NotReady => t(L::MODEL_NOT_READY),
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

    /// 队列态的样例键（无批量、无统计记录）。
    fn queue_key() -> StatusBarKey {
        StatusBarKey {
            lang: UiLang::Zh,
            batch: false,
            batch_done: 0,
            tally: TaskTally {
                total: 3,
                pending: 1,
                queued: 1,
                proc: 1,
                done: 0,
                err: 0,
            },
            saved_sec: None,
        }
    }

    #[test]
    fn cached_left_text_is_the_uncached_formatter_output() {
        // 逐字节比对：缓存只是把同一个函数的结果留下来，不改一个字。
        let k = queue_key();
        let (left, _) = build_status_bar_text(k);
        assert_eq!(left, format_queue_status(UiLang::Zh, 3, 1, 1, 1, 0, 0));

        let batch = StatusBarKey {
            batch: true,
            batch_done: 2,
            ..queue_key()
        };
        let (left, _) = build_status_bar_text(batch);
        // 分母 = batch_done + 未完成的活着的行（pending + queued + proc）。
        assert_eq!(left, format_batch_progress(UiLang::Zh, 2, 5));
    }

    #[test]
    fn an_unchanged_key_keeps_the_text_and_a_changed_key_rebuilds_it() {
        let mut cache = StatusBarTextCache::default();
        cache.refresh(queue_key());
        let first = cache.left.clone();

        // 同一个键：什么都不用重算。
        cache.refresh(queue_key());
        assert_eq!(cache.left, first);

        // 一行从排队变成完成 → 键变了 → 重算，且结果仍是同一批函数的输出。
        let moved = StatusBarKey {
            tally: TaskTally {
                total: 3,
                pending: 1,
                queued: 0,
                proc: 1,
                done: 1,
                err: 0,
            },
            ..queue_key()
        };
        assert_ne!(cache.key, Some(moved));
        cache.refresh(moved);
        assert_ne!(cache.left, first);
        assert_eq!(
            cache.left.as_str(),
            format_queue_status(UiLang::Zh, 3, 1, 0, 1, 1, 0)
        );
    }

    #[test]
    fn key_separates_a_negative_zero_saved_span_from_a_positive_one() {
        // 用位模式而非 `==` 存 f64：`-0.0 == 0.0` 会让两个不同的键撞在一起。
        let zero = StatusBarKey {
            saved_sec: Some(0.0f64.to_bits()),
            ..queue_key()
        };
        let neg_zero = StatusBarKey {
            saved_sec: Some((-0.0f64).to_bits()),
            ..queue_key()
        };
        assert_ne!(zero, neg_zero);
    }
}
