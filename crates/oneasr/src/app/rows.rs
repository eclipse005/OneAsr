//! List-row lifecycle: the per-frame paint snapshot and the enter/exit fades.
//!
//! A deleted row leaves `tasks` at once but is kept as a tombstone in
//! `exiting` until its fade finishes, so the list does not jump.

use crate::app::prelude::*;

impl OneAsrApp {
    pub(crate) fn is_exiting(&self, id: &str) -> bool {
        self.exiting.contains_key(id)
    }

    /// Drop finished enter/exit fades (call once per frame from list render).
    /// Exit tombstones are removed from `tasks` only after the fade completes.
    pub(crate) fn drain_row_anims(&mut self) {
        let now = Instant::now();
        for id in finished_exits(&self.exiting, now) {
            self.exiting.remove(&id);
            self.entering.remove(&id);
            self.tasks.retain(|t| t.id != id);
            if self.hover_row.as_ref().is_some_and(|h| h == &id) {
                self.hover_row = None;
            }
        }
        self.entering
            .retain(|id, t0| {
                !self.exiting.contains_key(id)
                    && now.duration_since(*t0).as_secs_f32() < ROW_ENTER_SECS
            });
    }

    pub(crate) fn snapshot_task_rows(&self, now: Instant) -> Vec<TaskRowView> {
        let ranks = queue_ranks(&self.tasks);

        self.tasks
            .iter()
            .map(|t| {
                let (opacity, interactive) = row_fade(
                    now,
                    self.exiting.get(&t.id).copied(),
                    self.entering.get(&t.id).copied(),
                );
                // The realtime factor pairs the media length with the wall clock,
                // so an unknown length (probe failed) drops the figure rather
                // than guessing one.
                let media_sec = match t.duration {
                    DurationState::Known(s) => Some(s),
                    _ => None,
                };
                TaskRowView {
                    id: t.id.clone(),
                    status: t.status,
                    has_output: t.output_file.is_some(),
                    language: t.language.clone(),
                    vocal_separation: t.vocal_separation,
                    error: t.error.clone(),
                    name: t.name.clone(),
                    size_label: t.size_label(),
                    duration_label: t.duration.label(),
                    rtfx_label: t
                        .timing
                        .as_ref()
                        .and_then(|timing| {
                            realtime_factor_label(media_sec, timing.total_ms, ui_lang())
                        }),
                    is_video: is_video_format(&t.format),
                    timing: t.timing.clone(),
                    opacity,
                    interactive,
                    queue_rank: ranks.get(t.id.as_str()).copied(),
                }
            })
            .collect()
    }

    /// Brief message in the bottom status bar (replaces toast).
    pub(crate) fn flash_hint(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_hint_for(msg, Duration::from_secs(3), cx);
    }

    pub(crate) fn flash_hint_for(
        &mut self,
        msg: impl Into<SharedString>,
        dur: Duration,
        cx: &mut Context<Self>,
    ) {
        self.status_hint = Some(msg.into());
        self.status_hint_until = Some(Instant::now() + dur);
        self.status_hint_good = false;
        cx.notify();
    }

    /// Same slot, success colouring: green for "this worked", amber (the
    /// default) for "this needs you". Tone is the whole message here — the
    /// hint is one line of text and nothing else carries the verdict.
    pub(crate) fn flash_good_hint_for(
        &mut self,
        msg: impl Into<SharedString>,
        dur: Duration,
        cx: &mut Context<Self>,
    ) {
        self.status_hint = Some(msg.into());
        self.status_hint_until = Some(Instant::now() + dur);
        self.status_hint_good = true;
        cx.notify();
    }
}

// ── Row animation and queue ranking (pure: no gpui, unit-tested below) ──────

/// Exit tombstones whose fade has finished, and can be dropped for good.
pub(crate) fn finished_exits(exiting: &HashMap<String, Instant>, now: Instant) -> Vec<String> {
    exiting
        .iter()
        .filter(|(_, t0)| now.duration_since(**t0).as_secs_f32() >= ROW_EXIT_SECS)
        .map(|(id, _)| id.clone())
        .collect()
}

/// Paint state of one row: `(opacity, interactive)`.
///
/// A row that is both exiting and entering takes the **exit** path — the delete
/// won — and a row in neither map is simply settled: fully opaque, clickable.
pub(crate) fn row_fade(
    now: Instant,
    exiting_since: Option<Instant>,
    entering_since: Option<Instant>,
) -> (f32, bool) {
    if let Some(t0) = exiting_since {
        let p = (now.duration_since(t0).as_secs_f32() / ROW_EXIT_SECS).clamp(0.0, 1.0);
        (1.0 - ease_out_cubic(p), false)
    } else if let Some(t0) = entering_since {
        let p = (now.duration_since(t0).as_secs_f32() / ROW_ENTER_SECS).clamp(0.0, 1.0);
        (ease_out_cubic(p), true)
    } else {
        (1.0, true)
    }
}

/// `6.5 倍速` / `6.5×` for the timing card header — media seconds ÷ wall-clock.
///
/// This is the **end-to-end** factor, so it agrees with the `用时` total printed
/// next to it: the model-load stages are part of both. Anything the figure would
/// misrepresent is dropped instead of approximated — an unknown media length, a
/// run the clock never saw, or a non-finite duration all give `None`, so the
/// header hides the number rather than printing `∞` or `NaN`.
///
/// A hardware-comparison number would strip `AsrStage::LoadingAsr` /
/// `LoadingAligner` from the denominator; that is deliberately **not** what this
/// returns, because the user is looking at one row's own wall clock here.
pub(crate) fn realtime_factor_label(
    media_sec: Option<f64>,
    process_ms: u64,
    lang: UiLang,
) -> Option<String> {
    let media = media_sec.filter(|s| s.is_finite() && *s > 0.0)?;
    let wall = process_ms as f64 / 1000.0;
    if wall <= 0.0 {
        return None;
    }
    let factor = media / wall;
    // Past 100× the decimal is noise and would outgrow the card.
    let number = if factor >= 100.0 {
        format!("{factor:.0}")
    } else {
        format!("{factor:.1}")
    };
    Some(match lang {
        UiLang::Zh => format!("{number} 倍速"),
        UiLang::En => format!("{number}×"),
    })
}

/// 1-based queue rank per queued row id, in FIFO order.
///
/// A queued row with no sequence yet sorts **last** (`u64::MAX`) rather than
/// jumping to the front of the line. The sort is stable, so rows sharing a
/// sequence keep their list order and the ranking never shuffles between frames.
pub(crate) fn queue_ranks(tasks: &[Task]) -> HashMap<&str, usize> {
    let mut queued: Vec<(&str, u64)> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Queued)
        .map(|t| (t.id.as_str(), t.queue_seq.unwrap_or(u64::MAX)))
        .collect();
    queued.sort_by_key(|(_, seq)| *seq);
    queued
        .iter()
        .enumerate()
        .map(|(i, (id, _))| (*id, i + 1))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(status: TaskStatus, seq: Option<u64>) -> Task {
        let mut t = Task::from_path("media/clip.wav", "zh", false);
        t.status = status;
        t.queue_seq = seq;
        t
    }

    #[test]
    fn queue_ranks_follow_the_sequence_not_the_list_order() {
        let tasks = vec![
            task(TaskStatus::Done, None),
            task(TaskStatus::Queued, Some(30)),
            task(TaskStatus::Queued, Some(10)),
            task(TaskStatus::Queued, Some(20)),
        ];
        let ranks = queue_ranks(&tasks);
        assert_eq!(ranks.len(), 3, "only queued rows are ranked");
        assert_eq!(ranks[tasks[2].id.as_str()], 1);
        assert_eq!(ranks[tasks[3].id.as_str()], 2);
        assert_eq!(ranks[tasks[1].id.as_str()], 3);
    }

    #[test]
    fn a_queued_row_without_a_sequence_sorts_last() {
        let tasks = vec![
            task(TaskStatus::Queued, None),
            task(TaskStatus::Queued, Some(5)),
        ];
        let ranks = queue_ranks(&tasks);
        assert_eq!(ranks[tasks[1].id.as_str()], 1);
        assert_eq!(ranks[tasks[0].id.as_str()], 2);
    }

    #[test]
    fn equal_sequences_keep_their_list_order() {
        // Stable sort: a tie must not make two rows swap between frames.
        let tasks = vec![
            task(TaskStatus::Queued, Some(7)),
            task(TaskStatus::Queued, Some(7)),
        ];
        let ranks = queue_ranks(&tasks);
        assert_eq!(ranks[tasks[0].id.as_str()], 1);
        assert_eq!(ranks[tasks[1].id.as_str()], 2);
    }

    #[test]
    fn a_settled_row_is_opaque_and_clickable() {
        assert_eq!(row_fade(Instant::now(), None, None), (1.0, true));
    }

    #[test]
    fn a_row_mid_exit_fades_out_and_stops_responding() {
        let now = Instant::now();
        // Just started: still fully opaque, but already inert.
        assert_eq!(row_fade(now, Some(now), None), (1.0, false));
        // Past the end of the fade: gone, and clamped rather than negative.
        let done = now - Duration::from_secs_f32(ROW_EXIT_SECS + 0.01);
        assert_eq!(row_fade(now, Some(done), None), (0.0, false));
        let long_ago = now - Duration::from_secs(60);
        assert_eq!(row_fade(now, Some(long_ago), None), (0.0, false));
    }

    #[test]
    fn a_row_mid_enter_fades_in_and_stays_clickable() {
        let now = Instant::now();
        assert_eq!(row_fade(now, None, Some(now)), (0.0, true));
        let done = now - Duration::from_secs_f32(ROW_ENTER_SECS + 0.01);
        assert_eq!(row_fade(now, None, Some(done)), (1.0, true));
    }

    #[test]
    fn exit_wins_over_enter() {
        // A row deleted while still fading in must not keep accepting clicks.
        let now = Instant::now();
        assert!(!row_fade(now, Some(now), Some(now)).1);
    }

    #[test]
    fn exits_are_collected_only_once_the_fade_finished() {
        let now = Instant::now();
        let mut exiting = HashMap::new();
        exiting.insert(
            "old".to_string(),
            now - Duration::from_secs_f32(ROW_EXIT_SECS + 0.01),
        );
        exiting.insert("fresh".to_string(), now);
        assert_eq!(finished_exits(&exiting, now), vec!["old".to_string()]);
    }

    #[test]
    fn realtime_factor_needs_a_usable_length_and_clock() {
        assert_eq!(realtime_factor_label(None, 37_000, UiLang::Zh), None);
        assert_eq!(realtime_factor_label(Some(0.0), 37_000, UiLang::Zh), None);
        assert_eq!(realtime_factor_label(Some(f64::NAN), 37_000, UiLang::Zh), None);
        assert_eq!(realtime_factor_label(Some(239.0), 0, UiLang::Zh), None);
    }

    #[test]
    fn realtime_factor_is_media_over_the_same_wall_clock() {
        // 3:59 of media finished in 37 s → 6.5× realtime.
        assert_eq!(
            realtime_factor_label(Some(239.0), 37_000, UiLang::Zh).as_deref(),
            Some("6.5 倍速")
        );
        // Large factors stay short instead of spilling decimals.
        assert_eq!(
            realtime_factor_label(Some(600.0), 1_000, UiLang::Zh).as_deref(),
            Some("600 倍速")
        );
        assert_eq!(
            realtime_factor_label(Some(239.0), 37_000, UiLang::En).as_deref(),
            Some("6.5×")
        );
    }
}
