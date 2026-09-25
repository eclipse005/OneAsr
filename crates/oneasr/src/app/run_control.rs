//! Batch and queue orchestration: which task runs next, what a finished run
//! means for the rows behind it, and the batch counters.

use crate::app::prelude::*;

impl OneAsrApp {
    /// Mark a batch as running.
    ///
    /// The progress denominator is **derived at paint time** from live rows
    /// (`batch_done` + still-active) and never stored: deleting or adding
    /// tasks mid-run, or clicking 开始 again, cannot leave the counter
    /// pointing at rows that no longer exist.
    pub(crate) fn begin_batch(&mut self, job_count: usize) {
        if job_count == 0 {
            return;
        }
        if !self.batch_mode {
            self.batch_mode = true;
            self.batch_done = 0;
            self.batch_ok = 0;
            self.batch_err = 0;
        }
    }

    pub(crate) fn end_batch_if_idle(&mut self, cx: &mut Context<Self>) {
        if !self.batch_mode {
            return;
        }
        let still =
            self.tasks.iter().any(|t| {
                matches!(t.status, TaskStatus::Queued | TaskStatus::Processing)
            });
        if still {
            return;
        }
        let done = self.batch_ok;
        let err = self.batch_err;
        self.batch_mode = false;
        self.batch_done = 0;
        self.batch_ok = 0;
        self.batch_err = 0;
        let Some(msg) = batch_summary(done, err, ui_lang()) else {
            return;
        };
        // The first success's saved time rides the run-complete line; later runs
        // are a plain tally.
        let msg = match self.nudge_saved.take() {
            Some(saved) => crate::i18n::run_complete_with_saved(&msg, &saved),
            None => msg,
        };
        // One chime per run — the same "never one sound per item" rule the
        // click family follows. Success and failure share the single reminder:
        // the verdict is carried by the hint's colour and wording
        // (`flash_good_hint_for` vs `flash_hint_for`), not by a second chime.
        // A mixed run still lands on the amber hint, because something in there
        // wants the user's eyes.
        self.play_ui(sfx::Sfx::Reminder);
        if err == 0 {
            self.flash_good_hint_for(msg, Duration::from_secs(6), cx);
        } else {
            self.flash_hint_for(msg, Duration::from_secs(6), cx);
        }
    }

    /// Soft gate before enqueue. Single source: [`Settings::can_start`].
    pub(crate) fn ensure_can_start(&mut self, cx: &mut Context<Self>) -> bool {
        if let Err(msg) = self.settings.can_start() {
            self.model_status = ModelStatus::NotReady;
            self.flash_hint(msg, cx);
            if !self.settings_open {
                self.toggle_settings(cx);
            }
            return false;
        }
        self.model_status = ModelStatus::Ready;
        true
    }

    pub(crate) fn start_all(&mut self, cx: &mut Context<Self>) {
        if !self.ensure_can_start(cx) {
            return;
        }
        let mut enqueued = 0usize;
        for t in self.tasks.iter_mut() {
            if self.exiting.contains_key(&t.id) {
                continue;
            }
            if matches!(t.status, TaskStatus::Pending | TaskStatus::Error) {
                t.status = TaskStatus::Queued;
                t.queue_seq = Some(next_queue_seq());
                t.error = None;
                enqueued += 1;
            }
        }
        if enqueued == 0 {
            let live: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| !self.exiting.contains_key(&t.id))
                .collect();
            self.flash_hint(no_start_reason(&live).message(ui_lang()), cx);
            return;
        }
        self.play_ui(sfx::Sfx::Click);
        self.begin_batch(enqueued);
        self.try_start_next(cx);
        cx.notify();
    }

    pub(crate) fn start_one(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.ensure_can_start(cx) {
            return;
        }
        if self.is_exiting(id) {
            return;
        }
        let status = match self.tasks.iter().find(|t| t.id == id) {
            Some(t) => t.status,
            None => return,
        };
        if let Some(msg) = single_start_blocker(status, ui_lang()) {
            self.flash_hint(msg, cx);
            return;
        }
        let slot_free = !self.busy
            && !self
                .tasks
                .iter()
                .any(|x| x.status == TaskStatus::Processing);

        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.error = None;
            t.status = TaskStatus::Queued;
            t.queue_seq = Some(next_queue_seq());
        }
        self.play_ui(sfx::Sfx::Click);
        self.begin_batch(1);
        if slot_free {
            self.try_start_next(cx);
        } else {
            cx.notify();
        }
    }

    /// Start the oldest queued job if the worker slot is free.
    pub(crate) fn try_start_next(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.tasks.iter().any(|t| t.status == TaskStatus::Processing) {
            return;
        }
        let next_id = next_queued_id(&self.tasks, &self.exiting);
        match next_id {
            Some(id) => self.launch_task(&id, cx),
            None => {
                self.end_batch_if_idle(cx);
                cx.notify();
            }
        }
    }

    pub(crate) fn launch_task(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(task) = self.tasks.iter().find(|t| t.id == id) else {
            return;
        };
        if task.status != TaskStatus::Queued && task.status != TaskStatus::Pending {
            return;
        }
        let id = task.id.clone();
        let path = task.path.clone();
        let name = task.name.clone();
        let task_lang = task.language.clone();
        let task_sep = task.vocal_separation;

        // Effective settings for **this row**: per-task language + separation
        // override the settings defaults. Built before anything is marked
        // Processing so the start gate judges what will actually run.
        let mut settings = self.settings.clone();
        settings.language = normalize_source_language(&task_lang);
        settings.vocal_separation = task_sep;
        if let Err(e) = settings.can_start() {
            crashlog::log_warn(format!(
                "task blocked before start: {id}\n  reason: {e}\n  vocal_separation: {task_sep}"
            ));
            if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                t.status = TaskStatus::Error;
                t.queue_seq = None;
                t.error = Some(e.clone());
            }
            self.flash_hint(e, cx);
            // The row is settled without a worker job — keep the queue moving.
            self.try_start_next(cx);
            cx.notify();
            return;
        }

        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.status = TaskStatus::Processing;
            t.queue_seq = None;
            t.error = None;
            t.timing = None;
        }
        // Processing locks row language edit — drop a live select on this row.
        if self.lang_menu.as_deref() == Some(id.as_str()) {
            self.lang_menu = None;
        }
        if self.timing_popover.as_deref() == Some(id.as_str()) {
            self.close_timing_popover();
        }
        self.busy = true;
        // Match the pipeline's first real `on_stage`: separation runs before
        // the 16 kHz transcode when the row has it enabled.
        let first_stage = if task_sep {
            AsrStage::Separating
        } else {
            AsrStage::Converting
        };
        self.active_stage = Some((
            id.clone(),
            SharedString::from(first_stage.label(ui_lang())),
        ));

        // Start context: failures log only `{id}` + message, so this entry is
        // what makes a pasted log self-sufficient (which file/model/backend).
        crashlog::log_info(format!(
            "task start: {id}\n  file: {}\n  model: {}\n  backend: {}\n  language: {}\n  vocal_separation: {}",
            path.display(),
            settings.asr_model_dir.display(),
            settings.backend,
            settings.language,
            settings.vocal_separation,
        ));

        // Hand off to the dedicated ASR worker — never block the UI thread.
        if self
            .job_tx
            .send(AsrJob::Run {
                id: id.clone(),
                path,
                name,
                settings,
            })
            .is_err()
        {
            // The worker thread is gone (e.g. it panicked earlier) — every
            // further start click would otherwise look like a silent no-op.
            crashlog::log_error(format!("asr worker channel closed — task {id} cannot start"));
            self.busy = false;
            self.active_stage = None;
            if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                t.status = TaskStatus::Error;
                t.queue_seq = None;
                t.error = Some(oneasr_core::i18n::t(L::WORKER_EXITED).into());
            }
            // Same rule as the pre-flight failure: a settled row must not stall
            // the rest of the batch.
            self.try_start_next(cx);
        }
        cx.notify();
    }

    pub(crate) fn delete_task(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(task) = self.tasks.iter().find(|t| t.id == id) {
            if task.status.locks_row_actions() {
                self.flash_hint(t(L::LOCKED_DELETE), cx);
                return;
            }
        } else {
            return;
        }
        // Already fading out — ignore double-clicks.
        if self.is_exiting(id) {
            return;
        }
        self.play_ui(sfx::Sfx::Click);
        // Tombstone in place so the row fades without jumping to the list bottom.
        self.entering.remove(id);
        self.exiting.insert(id.to_string(), Instant::now());
        if self.hover_row.as_ref().is_some_and(|h| h == id) {
            self.hover_row = None;
        }
        if self.lang_menu.as_deref() == Some(id) {
            self.lang_menu = None;
        }
        if self.timing_popover.as_deref() == Some(id)
            || self
                .timing_hover_since
                .as_ref()
                .is_some_and(|(hid, _)| hid == id)
        {
            self.close_timing_popover();
        }
        cx.notify();
    }

    /// Clear the whole list. Keeps the active Processing row if any.
    /// Non-processing rows fade out in place (same 0.22s exit as single delete).
    pub(crate) fn clear_all(&mut self, cx: &mut Context<Self>) {
        let live_any = self.tasks.iter().any(|t| !self.exiting.contains_key(&t.id));
        if !live_any {
            self.flash_hint(t(L::LIST_EMPTY), cx);
            return;
        }
        let had_proc = self.tasks.iter().any(|t| {
            t.status == TaskStatus::Processing && !self.exiting.contains_key(&t.id)
        });
        self.play_ui(sfx::Sfx::Click);
        let now = Instant::now();
        for t in &self.tasks {
            if t.status == TaskStatus::Processing {
                continue;
            }
            self.entering.remove(&t.id);
            self.exiting.entry(t.id.clone()).or_insert(now);
        }
        if !had_proc {
            self.batch_mode = false;
        }
        self.batch_done = 0;
        self.hover_row = None;
        // List select targets a row; bulk clear invalidates any open chip menu.
        self.lang_menu = None;
        self.close_timing_popover();
        if had_proc {
            self.flash_hint(t(L::CLEARED_KEEP_RUNNING), cx);
        }
        cx.notify();
    }

    pub(crate) fn open_task_output(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(path) = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.output_file.clone())
        else {
            return;
        };
        // OS explorer is the feedback; no in-app banner — but a dead click must
        // still leave a trace for "点开文件夹没反应" reports.
        if let Err(e) = shell::open_containing_folder(&path) {
            crashlog::log_warn(format!("open output folder failed: {e}\n  file: {}", path.display()));
        }
        cx.notify();
    }
}

// ── Start gate and batch rules (pure: no gpui, unit-tested below) ───────────

/// Why 「开始」 found nothing to enqueue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NoStart {
    /// Nothing on the list at all.
    NoFiles,
    /// Everything left is already running or waiting its turn.
    AlreadyRunning,
    /// Rows exist, but none in a state that can be started.
    NothingToStart,
}

impl NoStart {
    pub(crate) fn message(self, lang: UiLang) -> &'static str {
        match (self, lang) {
            (Self::NoFiles, UiLang::Zh) => "请先添加音视频文件",
            (Self::NoFiles, UiLang::En) => "Add audio / video files first",
            (Self::AlreadyRunning, UiLang::Zh) => "任务已在处理或排队中",
            (Self::AlreadyRunning, UiLang::En) => "Tasks are already processing or queued",
            (Self::NothingToStart, UiLang::Zh) => "没有可开始的任务",
            (Self::NothingToStart, UiLang::En) => "Nothing to start",
        }
    }
}

/// Classify an 「开始」 click that enqueued nothing. `live` is the list with
/// fade-out tombstones already removed.
pub(crate) fn no_start_reason(live: &[&Task]) -> NoStart {
    if live.is_empty() {
        NoStart::NoFiles
    } else if live
        .iter()
        .any(|t| matches!(t.status, TaskStatus::Processing | TaskStatus::Queued))
    {
        NoStart::AlreadyRunning
    } else {
        NoStart::NothingToStart
    }
}

/// Why a single row cannot start, or `None` to proceed.
pub(crate) fn single_start_blocker(status: TaskStatus, lang: UiLang) -> Option<&'static str> {
    match (status, lang) {
        (TaskStatus::Processing, UiLang::Zh) => Some("该任务正在处理中"),
        (TaskStatus::Processing, UiLang::En) => Some("This task is processing"),
        (TaskStatus::Queued, UiLang::Zh) => Some("该任务已在队列中"),
        (TaskStatus::Queued, UiLang::En) => Some("This task is already queued"),
        (TaskStatus::Done, UiLang::Zh) => Some("该任务已完成"),
        (TaskStatus::Done, UiLang::En) => Some("This task is done"),
        (TaskStatus::Pending | TaskStatus::Error, _) => None,
    }
}

/// The next job to run: the queued row with the lowest sequence, skipping rows
/// that are fading out.
///
/// A queued row with no sequence sorts last rather than jumping the line, and
/// the sort is stable, so equal sequences keep their list order.
pub(crate) fn next_queued_id(tasks: &[Task], exiting: &HashMap<String, Instant>) -> Option<String> {
    let mut queued: Vec<&Task> = tasks
        .iter()
        .filter(|t| t.status == TaskStatus::Queued && !exiting.contains_key(&t.id))
        .collect();
    queued.sort_by_key(|t| t.queue_seq.unwrap_or(u64::MAX));
    queued.first().map(|t| t.id.clone())
}

/// The run-complete line, or `None` when the batch has nothing to report — an
/// empty summary would read as an accusation rather than a tally.
pub(crate) fn batch_summary(done: usize, err: usize, lang: UiLang) -> Option<String> {
    if done == 0 && err == 0 {
        None
    } else if err == 0 {
        Some(match lang {
            UiLang::Zh => format!("全部完成 · {done} 个任务"),
            UiLang::En => format!("All done · {}", crate::i18n::n_tasks(lang, done)),
        })
    } else if done == 0 {
        Some(match lang {
            UiLang::Zh => format!("批次结束 · {err} 个失败"),
            UiLang::En => format!("Batch ended · {err} failed"),
        })
    } else {
        Some(match lang {
            UiLang::Zh => format!("批次结束 · 完成 {done} · 失败 {err}"),
            UiLang::En => format!("Batch ended · {done} ok · {err} failed"),
        })
    }
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
    fn start_reports_why_there_was_nothing_to_start() {
        let empty: Vec<&Task> = Vec::new();
        assert_eq!(no_start_reason(&empty), NoStart::NoFiles);

        let running = task(TaskStatus::Processing, None);
        assert_eq!(no_start_reason(&[&running]), NoStart::AlreadyRunning);
        let waiting = task(TaskStatus::Queued, Some(1));
        assert_eq!(no_start_reason(&[&waiting]), NoStart::AlreadyRunning);

        let finished = task(TaskStatus::Done, None);
        assert_eq!(no_start_reason(&[&finished]), NoStart::NothingToStart);
    }

    #[test]
    fn single_start_blockers_match_the_row_state() {
        // Pending and Error are both startable — a failed row can be retried.
        assert_eq!(single_start_blocker(TaskStatus::Pending, UiLang::Zh), None);
        assert_eq!(single_start_blocker(TaskStatus::Error, UiLang::Zh), None);
        assert_eq!(
            single_start_blocker(TaskStatus::Queued, UiLang::Zh),
            Some("该任务已在队列中")
        );
        assert_eq!(
            single_start_blocker(TaskStatus::Processing, UiLang::Zh),
            Some("该任务正在处理中")
        );
        assert_eq!(
            single_start_blocker(TaskStatus::Done, UiLang::Zh),
            Some("该任务已完成")
        );
        assert_eq!(
            single_start_blocker(TaskStatus::Queued, UiLang::En),
            Some("This task is already queued")
        );
    }

    #[test]
    fn next_queued_job_is_the_lowest_sequence() {
        let tasks = vec![
            task(TaskStatus::Queued, Some(30)),
            task(TaskStatus::Queued, Some(10)),
            task(TaskStatus::Queued, Some(20)),
        ];
        assert_eq!(
            next_queued_id(&tasks, &HashMap::new()),
            Some(tasks[1].id.clone())
        );
    }

    #[test]
    fn a_fading_row_is_skipped_and_the_one_behind_it_runs() {
        let tasks = vec![
            task(TaskStatus::Queued, Some(10)),
            task(TaskStatus::Queued, Some(20)),
        ];
        let mut exiting = HashMap::new();
        exiting.insert(tasks[0].id.clone(), Instant::now());
        assert_eq!(next_queued_id(&tasks, &exiting), Some(tasks[1].id.clone()));
    }

    #[test]
    fn only_queued_rows_are_candidates() {
        let tasks = vec![
            task(TaskStatus::Processing, Some(1)),
            task(TaskStatus::Pending, None),
            task(TaskStatus::Done, None),
        ];
        assert_eq!(next_queued_id(&tasks, &HashMap::new()), None);
    }

    #[test]
    fn batch_summary_stays_silent_when_nothing_ran() {
        assert_eq!(batch_summary(0, 0, UiLang::Zh), None);
        assert_eq!(
            batch_summary(3, 0, UiLang::Zh).as_deref(),
            Some("全部完成 · 3 个任务")
        );
        assert_eq!(
            batch_summary(0, 2, UiLang::Zh).as_deref(),
            Some("批次结束 · 2 个失败")
        );
        assert_eq!(
            batch_summary(3, 1, UiLang::Zh).as_deref(),
            Some("批次结束 · 完成 3 · 失败 1")
        );
        assert_eq!(
            batch_summary(3, 0, UiLang::En).as_deref(),
            Some("All done · 3 tasks")
        );
        assert_eq!(
            batch_summary(0, 2, UiLang::En).as_deref(),
            Some("Batch ended · 2 failed")
        );
        assert_eq!(
            batch_summary(3, 1, UiLang::En).as_deref(),
            Some("Batch ended · 3 ok · 1 failed")
        );
    }
}
