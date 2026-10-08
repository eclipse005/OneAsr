//! The worker protocol ([`WorkerMsg`] / [`AsrJob`]) and the pump that applies
//! it to the UI state.
//!
//! The pump is a dispatcher only: each variant is handled by a small function,
//! so a new message type does not grow a branch inside a long match.

use crate::app::prelude::*;
use crate::app::transcript_ui::expand;

pub(crate) enum WorkerMsg {
    FilesPicked(Vec<PathBuf>),
    /// A transcript file chosen for one specific row (the chip's `+ 文稿`).
    TranscriptPicked {
        id: String,
        path: Option<PathBuf>,
    },
    PickCancelled,
    /// 智能断句在后台线程跑完，回来的整段文本。
    ///
    /// `input` 是送去断句时的原文：写回前先比对，**只有暂存里的文字没被换过
    /// 才落盘**。断句要读词典跑 DP，窗口不再冻结，于是「断句途中又点了重新
    /// 读取」这类并发成为可能——那一版的正文才是用户要的，不能被慢一拍的结果
    /// 盖掉。这与原来同步执行时不可能发生的交错被挡在同一个地方。
    TranscriptBroken {
        id: String,
        input: String,
        text: String,
    },
    ModelDirPicked(PathBuf),
    AlignerDirPicked(PathBuf),
    DemucsDirPicked(PathBuf),
    OutputDirPicked(PathBuf),
    Probed {
        id: String,
        duration_sec: Option<f64>,
    },
    /// Live stage from the dedicated ASR worker (UI-only, never blocks).
    Progress {
        id: String,
        stage: SharedString,
        /// Non-fatal message raised by the pipeline (e.g. separation fell back
        /// to CPU); flashed in the status bar.
        warning: Option<SharedString>,
    },
    Finished {
        id: String,
        result: Result<PathBuf, String>,
        /// Always produced by the worker; UI stores only when `has_breakdown()`.
        timing: TaskTiming,
    },
    /// Model download progress / completion (background thread).
    ModelDownload(DownloadProgress),
}

/// Jobs for the long-lived ASR worker (one active job at a time by design).
pub(crate) enum AsrJob {
    Run {
        id: String,
        path: PathBuf,
        name: String,
        settings: Settings,
        /// 挂了这行任务的文稿就整段跳过识别，只做打轴。
        transcript: Option<oneasr_core::TranscriptInput>,
    },
}

impl OneAsrApp {
    pub(crate) fn poll_worker(&mut self, cx: &mut Context<Self>) {
        self.tick_status_hint(cx);
        loop {
            match self.rx.try_recv() {
                Ok(WorkerMsg::FilesPicked(paths)) => self.handle_files_picked(paths, cx),
                Ok(WorkerMsg::TranscriptPicked { id, path }) => {
                    self.handle_transcript_picked(&id, path, cx)
                }
                Ok(WorkerMsg::PickCancelled) => self.handle_pick_cancelled(cx),
                Ok(WorkerMsg::TranscriptBroken { id, input, text }) => {
                    self.handle_transcript_broken(id, input, text, cx)
                }
                Ok(WorkerMsg::ModelDirPicked(dir)) => self.handle_model_dir_picked(dir, cx),
                Ok(WorkerMsg::AlignerDirPicked(dir)) => self.handle_aligner_dir_picked(dir, cx),
                Ok(WorkerMsg::DemucsDirPicked(dir)) => self.handle_demucs_dir_picked(dir, cx),
                Ok(WorkerMsg::OutputDirPicked(dir)) => self.handle_output_dir_picked(dir, cx),
                Ok(WorkerMsg::ModelDownload(progress)) => self.handle_model_download(progress, cx),
                Ok(WorkerMsg::Probed { id, duration_sec }) => {
                    self.handle_probed(id, duration_sec, cx)
                }
                Ok(WorkerMsg::Progress { id, stage, warning }) => {
                    self.handle_progress(id, stage, warning, cx)
                }
                Ok(WorkerMsg::Finished { id, result, timing }) => {
                    self.handle_finished(id, result, timing, cx)
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    // Worker died without reporting: do not leave rows stuck in
                    // Processing/Queued or `busy` latched forever. Guard with a
                    // one-shot flag — poll_worker keeps running every 80 ms.
                    if !self.worker_channel_dead {
                        self.worker_channel_dead = true;
                        crashlog::log_error(
                            "worker channel disconnected — marking in-flight tasks failed",
                        );
                        let mut affected = 0usize;
                        for t in &mut self.tasks {
                            if matches!(t.status, TaskStatus::Processing | TaskStatus::Queued) {
                                t.status = TaskStatus::Error;
                                t.queue_seq = None;
                                t.error = Some(oneasr_core::i18n::t(L::WORKER_EXITED).into());
                                affected += 1;
                            }
                        }
                        if affected > 0 {
                            self.busy = false;
                            self.active_stage = None;
                            // The whole collapse is one run ending badly: count
                            // it as such and let the run-complete notice own the
                            // single reminder (never one per task).
                            self.batch_err = self.batch_err.saturating_add(affected);
                            self.end_batch_if_idle(cx);
                        }
                        cx.notify();
                    }
                    break;
                }
            }
        }
    }
}

/// Message handlers, one per [`WorkerMsg`] variant. The pump in
/// [`OneAsrApp::poll_worker`] only matches and delegates; each arm's work
/// lives here so it can be read (and tested) on its own.
impl OneAsrApp {
    /// Short status-bar hints fade out on a timer.
    fn tick_status_hint(&mut self, cx: &mut Context<Self>) {
        if let Some(until) = self.status_hint_until {
            if Instant::now() >= until {
                self.status_hint = None;
                self.status_hint_until = None;
                self.status_hint_good = false;
                cx.notify();
            } else {
                cx.notify(); // keep bar live while hint is visible
            }
        }
    }

    /// Files picked in the OS dialog: add them to the list.
    fn handle_files_picked(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.picking = false;
        self.add_paths(paths, cx);
    }

    /// The file dialog was dismissed: drop the `picking` latch.
    fn handle_pick_cancelled(&mut self, cx: &mut Context<Self>) {
        self.picking = false;
        cx.notify();
    }

    /// 智能断句回来了：落盘并更新卡片上的那行提示。
    ///
    /// 写回前比对 `input`：暂存的正文在这期间被换过（重新读取、换了文稿）就
    /// **整条丢弃**，不去覆盖用户已经看到的另一份文字。丢弃时不提示——
    /// 那份文字是用户自己刚选的，不需要解释为什么旧结果没生效。
    ///
    /// 幂等由 `break_long_lines` 自己保证（连点两下算的是同一份输入，同一个
    /// 结果），所以这里不需要「正在断句」的闩锁，也就不用动 `mod.rs`。
    fn handle_transcript_broken(
        &mut self,
        id: String,
        input: String,
        text: String,
        cx: &mut Context<Self>,
    ) {
        // 先摘掉在途标记，再走下面任何一条早退路径——漏一条就等于把这一行的
        // 「智能断句」永久锁死。
        self.transcript_breaking.remove(&id);
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Some(st) = task.transcript.as_mut() else {
            return;
        };
        if st.text() != input {
            return;
        }
        st.set_text(text);
        let lines = st.line_count();
        // 提示挂在**当前打开的那张**卡片上，而 `transcript_card_note` 是 app 级
        // 单字段（关卡片时由 `overlays` 清空）。断句要跑一秒，这期间用户可能已经
        // 关掉 A 的卡片、打开了 B 的——那时把 A 的行数写到 B 上，就是一张卡片上
        // 显示着另一条文稿的「已断为 N 行」。正文按 id 落对了（上面的 CAS 保证），
        // 这里只管提示该归谁。
        if self.transcript_card.as_deref() == Some(id.as_str()) {
            self.transcript_card_note =
                Some(expand(t(L::TRANSCRIPT_BROKE), &[("n", &lines.to_string())]));
            cx.notify();
        }
    }

    /// ASR model directory chosen: rebind and probe — staged, not saved.
    ///
    /// Like every other settings edit, the pick only marks the drawer dirty;
    /// it reaches `settings.json` when the user presses 保存设置, and closing
    /// the drawer discards it.
    fn handle_model_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        // Binds to the active size, verbatim — see `Settings::set_asr_dir`.
        self.settings.set_asr_dir(dir);
        self.mark_settings_dirty(cx);
        // Probe the staged pick right away so the status dot is truthful;
        // the path itself only reaches settings.json on 保存设置.
        self.reset_model_config(cx);
    }

    /// Aligner directory chosen: rebind, probe — staged, not saved.
    fn handle_aligner_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        // 目录绑定到当前所选对齐模型（CTC / Qwen 各记各的，同 ASR 每尺寸记忆）。
        self.settings.set_aligner_dir(dir);
        self.mark_settings_dirty(cx);
        self.reset_model_config(cx);
    }

    /// Separation weights directory chosen: rebind, probe — staged, not saved.
    fn handle_demucs_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.demucs_model_dir = dir;
        self.mark_settings_dirty(cx);
        self.refresh_model_probe();
        // A row can only have separation on while the weights are
        // present, so a dir swap that loses them also clears the
        // default for new tasks.
        if !self.demucs_ready && self.settings.vocal_separation {
            self.settings.vocal_separation = false;
        }
    }

    /// Output directory chosen: rebind — staged, not saved.
    fn handle_output_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.output_dir = dir;
        self.mark_settings_dirty(cx);
    }

    /// Model download progress / terminal state (background thread).
    fn handle_model_download(&mut self, progress: DownloadProgress, cx: &mut Context<Self>) {
        let id = progress.model_id;
        let terminal = matches!(
            progress.state,
            DownloadState::Completed | DownloadState::Failed | DownloadState::Cancelled
        );
        self.set_download_progress(progress.clone());
        if terminal {
            self.clear_download_handle(id);
        }
        if progress.state == DownloadState::Completed {
            // Install layout already has files. Bind active selection only
            // when this download is for the currently selected ASR (or Align).
            match id.kind() {
                ModelKind::Demucs => {
                    self.refresh_model_probe();
                    self.flash_hint(crate::i18n::model_ready(id.label(ui_lang())), cx);
                }
                ModelKind::Asr | ModelKind::Align => {
                    let bound = self
                        .settings
                        .bind_download_if_active(id, progress.model_dir.clone());
                    if bound {
                        // No save here: a download is not a settings edit, and
                        // the paths it binds are exactly the ones the settings
                        // already point at (the download lands in the current
                        // directory for that component).
                        self.reset_model_config(cx);
                        self.flash_hint(crate::i18n::model_download_done(id.label(ui_lang())), cx);
                    } else {
                        // Non-selected ASR size finished installing on disk.
                        self.asr_download = None;
                        self.flash_hint(
                            crate::i18n::model_ready_switchable(id.label(ui_lang())),
                            cx,
                        );
                    }
                }
            }
        } else if progress.state == DownloadState::Failed {
            let fail = crate::i18n::model_download_failed(id.label(ui_lang()), &progress.message);
            // Byte counters separate dir-create failures (0 bytes)
            // from mid-file / rename failures for bare OS errors.
            crashlog::log_error(format!(
                "{fail}\n  dir: {}\n  bytes: {}/{}",
                progress.model_dir.display(),
                progress.downloaded_bytes,
                progress.total_bytes
            ));
            self.flash_hint(fail, cx);
        } else if progress.state == DownloadState::Cancelled {
            self.flash_hint(crate::i18n::model_cancelled(id.label(ui_lang())), cx);
        }
        // Hide another size's terminal snapshot when viewing this size.
        self.clear_stale_asr_progress();
        cx.notify();
    }

    /// Duration probe finished for a row.
    fn handle_probed(&mut self, id: String, duration_sec: Option<f64>, cx: &mut Context<Self>) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.set_duration(duration_sec);
        }
        cx.notify();
    }

    /// Live stage from the ASR worker (UI only, never blocks).
    fn handle_progress(
        &mut self,
        id: String,
        stage: SharedString,
        warning: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.active_stage = Some((id, stage));
        // Non-fatal pipeline warnings (e.g. separation fell back to
        // CPU) must be visible: this window has no console.
        if let Some(w) = warning {
            self.flash_hint(w, cx);
        }
        cx.notify();
    }

    /// A run finished: record the outcome, the ledger row and the timing.
    fn handle_finished(
        &mut self,
        id: String,
        result: Result<PathBuf, String>,
        timing: TaskTiming,
        cx: &mut Context<Self>,
    ) {
        self.busy = false;
        if self
            .active_stage
            .as_ref()
            .is_some_and(|(sid, _)| sid == &id)
        {
            self.active_stage = None;
        }
        // Ledger row, filled inside the borrow and appended after it.
        let mut ledger_row: Option<StatsRecord> = None;
        let mut cues: u32 = 0;
        let process_ms = timing.total_ms;
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            // Media seconds are what makes a saved-time claim
            // possible; an unknown duration just drops out of it.
            let media_sec = match t.duration {
                DurationState::Known(s) if s.is_finite() && s > 0.0 => Some(s),
                _ => None,
            };
            let lang = t.language.clone();
            let sep = t.vocal_separation;
            let transcript = t.transcript.is_some();
            t.timing = timing.has_breakdown().then_some(timing);
            let ok = match result {
                Ok(srt) => {
                    cues = count_output_lines(&srt);
                    t.status = TaskStatus::Done;
                    t.queue_seq = None;
                    t.output_file = Some(srt);
                    t.error = None;
                    true
                }
                Err(e) => {
                    t.status = TaskStatus::Error;
                    t.queue_seq = None;
                    t.error = Some(e);
                    false
                }
            };
            // Reaching this point means the row still exists: a row
            // deleted mid-run never gets an outcome recorded.
            ledger_row = Some(StatsRecord {
                v: oneasr_core::stats::LEDGER_VERSION,
                day: crashlog::local_day_ymd(),
                media_sec,
                process_ms,
                lang,
                sep,
                transcript,
                ok,
                cues,
            });
        }
        // The reminder belongs to the end of the run, not to each
        // row: a 20-file batch must not stutter 20 times (see
        // `end_batch_if_idle`). A row deleted mid-run says nothing.
        if let Some(rec) = ledger_row {
            self.record_stats(&rec);
            if rec.ok {
                self.batch_ok = self.batch_ok.saturating_add(1);
            } else {
                self.batch_err = self.batch_err.saturating_add(1);
            }
            // First success ever: the saved number is news exactly
            // once, and the run-complete line is what the user is
            // already looking at.
            if rec.ok
                && self.stats.tasks_ok == 1
                && let Some(s) = self.stats.saved_sec()
            {
                self.nudge_saved = Some(oneasr_core::stats::format_span_secs(ui_lang(), s).into());
            }
        }
        if self.batch_mode {
            self.batch_done = self.batch_done.saturating_add(1);
        }
        cx.notify();
        // Always drain the FIFO queue (one at a time).
        self.try_start_next(cx);
    }
}
