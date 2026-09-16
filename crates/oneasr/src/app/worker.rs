//! The worker protocol ([`WorkerMsg`] / [`AsrJob`]) and the pump that applies
//! it to the UI state.
//!
//! The pump is a dispatcher only: each variant is handled by a small function,
//! so a new message type does not grow a branch inside a long match.

use crate::app::prelude::*;

pub(crate) enum WorkerMsg {
    FilesPicked(Vec<PathBuf>),
    PickCancelled,
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
    },
}

impl OneAsrApp {
    pub(crate) fn poll_worker(&mut self, cx: &mut Context<Self>) {
        self.tick_status_hint(cx);
        loop {
            match self.rx.try_recv() {
                Ok(WorkerMsg::FilesPicked(paths)) => self.handle_files_picked(paths, cx),
                Ok(WorkerMsg::PickCancelled) => self.handle_pick_cancelled(cx),
                Ok(WorkerMsg::ModelDirPicked(dir)) => self.handle_model_dir_picked(dir, cx),
                Ok(WorkerMsg::AlignerDirPicked(dir)) => self.handle_aligner_dir_picked(dir, cx),
                Ok(WorkerMsg::DemucsDirPicked(dir)) => self.handle_demucs_dir_picked(dir, cx),
                Ok(WorkerMsg::OutputDirPicked(dir)) => self.handle_output_dir_picked(dir, cx),
                Ok(WorkerMsg::ModelDownload(progress)) => self.handle_model_download(progress, cx),
                Ok(WorkerMsg::Probed { id, duration_sec }) => self.handle_probed(id, duration_sec, cx),
                Ok(WorkerMsg::Progress { id, stage, warning }) => self.handle_progress(id, stage, warning, cx),
                Ok(WorkerMsg::Finished { id, result, timing }) => self.handle_finished(id, result, timing, cx),
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
                                t.error = Some("识别工作线程已退出".into());
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

    /// ASR model directory chosen: rebind, probe and persist it.
    fn handle_model_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.asr_model = ModelId::from_asr_dir(&dir).as_str().into();
        self.settings.asr_model_dir = dir;
        self.settings_dirty = false;
        // Probe + persist path immediately after pick.
        self.reset_model_config(cx);
        if let Err(e) = self.settings.save() {
            crashlog::log_error(format!("ASR model dir save failed: {e}"));
            self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
        } else {
            self.flash_hint(
                match self.model_status {
                    ModelStatus::Ready => "语音识别模型目录已更新 · 就绪",
                    ModelStatus::NotReady => "语音识别模型目录已更新 · 未就绪",
                },
                cx,
            );
        }
    }

    /// Aligner directory chosen: rebind, probe and persist it.
    fn handle_aligner_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.aligner_model_dir = dir;
        self.settings_dirty = false;
        self.reset_model_config(cx);
        if let Err(e) = self.settings.save() {
            crashlog::log_error(format!("aligner model dir save failed: {e}"));
            self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
        } else {
            self.flash_hint(
                match self.model_status {
                    ModelStatus::Ready => "对齐模型目录已更新 · 就绪",
                    ModelStatus::NotReady => "对齐模型目录已更新 · 未就绪",
                },
                cx,
            );
        }
    }

    /// Separation weights directory chosen: rebind, probe, persist.
    fn handle_demucs_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.demucs_model_dir = dir;
        self.settings_dirty = false;
        self.refresh_model_probe();
        // A row can only have separation on while the weights are
        // present, so a dir swap that loses them also clears the
        // default for new tasks.
        if !self.demucs_ready && self.settings.vocal_separation {
            self.settings.vocal_separation = false;
        }
        if let Err(e) = self.settings.save() {
            crashlog::log_error(format!("demucs model dir save failed: {e}"));
            self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
        } else if self.demucs_ready {
            self.flash_hint("人声分离模型目录已更新 · 就绪", cx);
        } else {
            self.flash_hint("人声分离模型目录已更新 · 未就绪", cx);
        }
    }

    /// Output directory chosen: rebind, probe and persist it.
    fn handle_output_dir_picked(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        self.settings.output_dir = dir;
        self.settings_dirty = false;
        if let Err(e) = self.settings.save() {
            crashlog::log_error(format!("output dir save failed: {e}"));
            self.flash_hint(format!("输出目录已更新，但保存失败: {e}"), cx);
        } else {
            self.flash_hint("字幕输出目录已更新", cx);
        }
    }

    /// Model download progress / terminal state (background thread).
    fn handle_model_download(&mut self, progress: DownloadProgress, cx: &mut Context<Self>) {
        let id = progress.model_id;
        let terminal = matches!(
            progress.state,
            DownloadState::Completed
                | DownloadState::Failed
                | DownloadState::Cancelled
        );
        self.set_download_progress(progress.clone());
        if terminal {
            self.clear_download_handle(id);
        }
        if progress.state == DownloadState::Completed {
            crashlog::log_info(format!(
                "download completed: {} → {}",
                id.label(),
                progress.model_dir.display()
            ));
            // Install layout already has files. Bind active selection only
            // when this download is for the currently selected ASR (or Align).
            // CUDA never binds settings paths (`bind_download_if_active` → false);
            // dll/ was registered at process start so no re-init is needed.
            match id.kind() {
                ModelKind::CudaRuntime => {
                    self.refresh_model_probe();
                    self.flash_hint(format!("{} 已安装", id.label()), cx);
                }
                ModelKind::Demucs => {
                    self.refresh_model_probe();
                    self.flash_hint(format!("{} 已就绪", id.label()), cx);
                }
                ModelKind::Asr | ModelKind::Align => {
                    let bound = self
                        .settings
                        .bind_download_if_active(id, progress.model_dir.clone());
                    if bound {
                        if let Err(e) = self.settings.save() {
                            crashlog::log_error(format!(
                                "save after {} download: {e}",
                                id.label()
                            ));
                        }
                        self.settings_dirty = false;
                        self.reset_model_config(cx);
                        self.flash_hint(format!("{} 下载完成", id.label()), cx);
                    } else {
                        // Non-selected ASR size finished installing on disk.
                        self.asr_download = None;
                        self.flash_hint(
                            format!("{} 已就绪，可在设置中切换使用", id.label()),
                            cx,
                        );
                    }
                }
            }
        } else if progress.state == DownloadState::Failed {
            let fail = if id.kind() == ModelKind::CudaRuntime {
                format!("{} 安装失败: {}", id.label(), progress.message)
            } else {
                format!("{} 下载失败: {}", id.label(), progress.message)
            };
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
            crashlog::log_info(format!(
                "download cancelled: {} at {}/{} bytes",
                id.label(),
                progress.downloaded_bytes,
                progress.total_bytes
            ));
            self.flash_hint(format!("{} 已取消", id.label()), cx);
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
    fn handle_progress(&mut self, id: String, stage: SharedString, warning: Option<SharedString>, cx: &mut Context<Self>) {
        self.active_stage = Some((id, stage));
        // Non-fatal pipeline warnings (e.g. separation fell back to
        // CPU) must be visible: this window has no console.
        if let Some(w) = warning {
            self.flash_hint(w, cx);
        }
        cx.notify();
    }

    /// A run finished: record the outcome, the ledger row and the timing.
    fn handle_finished(&mut self, id: String, result: Result<PathBuf, String>, timing: TaskTiming, cx: &mut Context<Self>) {
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
                    crashlog::log_error(format!("task {id} failed: {e}"));
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
            if rec.ok && self.stats.tasks_ok == 1
                && let Some(s) = self.stats.saved_sec() {
                self.nudge_saved = Some(oneasr_core::stats::format_span_secs(s).into());
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
