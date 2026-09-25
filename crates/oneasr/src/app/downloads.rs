//! The three model downloads — ASR, aligner, Demucs — and the
//! progress state machine behind each one.
//!
//! The lanes are independent: the drawer shows a row per component, and one
//! finishing must not disturb another still in flight.

use crate::app::prelude::*;

impl OneAsrApp {
    /// Start a model download into the directory the settings currently point
    /// at for that component (background). Does **not** rewrite settings —
    /// the paths a completed download binds are the ones already in use.
    pub(crate) fn start_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        // One job per kind — 0.6B / 1.7B share the ASR slot.
        if self.download_kind_busy(id.kind()) {
            self.flash_hint("已有同类下载任务进行中", cx);
            return;
        }

        // Re-download gate: verify what is already on disk **before** entering
        // the download state, so clicking 重新下载 on a complete install never
        // flashes the card into progress mode — it only reports completeness.
        let verified = match id.kind() {
            ModelKind::Asr => check_asr_model_dir(&self.settings.asr_model_dir),
            ModelKind::Align => {
                oneasr_core::check_aligner_model_dir(&self.settings.aligner_model_dir)
            }
            ModelKind::Demucs => {
                oneasr_core::check_demucs_model_dir(&self.settings.resolved_demucs_model_dir())
            }
        };
        if verified.is_ok() {
            self.flash_hint("模型文件完整，无需重新下载", cx);
            return;
        }

        // Downloads land where the app looks for the weights: the user's own
        // folder when one was picked, else the install layout.
        let model_dir = match id.kind() {
            ModelKind::Asr => self.settings.asr_model_dir.clone(),
            ModelKind::Align => self.settings.aligner_model_dir.clone(),
            ModelKind::Demucs => self.settings.resolved_demucs_model_dir(),
        };
        let handle = DownloadHandle::new(id, model_dir);
        let model_dir = handle.model_dir.clone();
        // Environment snapshot before the thread starts: when a download later
        // fails with a bare OS error (e.g. 拒绝访问 / os error 5), this pins the
        // target dir and whether it accepted writes at all.
        let writable = match probe_writable(&model_dir) {
            Ok(()) => "yes".to_string(),
            Err(e) => format!("NO ({e})"),
        };
        crashlog::log_info(format!(
            "download start: {}\n  dir: {}\n  writable: {writable}",
            id.label(),
            model_dir.display(),
        ));
        match id.kind() {
            ModelKind::Asr => self.asr_dl_handle = Some(handle.clone()),
            ModelKind::Align => self.align_dl_handle = Some(handle.clone()),
            ModelKind::Demucs => self.demucs_dl_handle = Some(handle.clone()),
        }
        self.set_download_progress(
            DownloadProgress {
                state: DownloadState::Downloading,
                model_id: id,
                model_dir: model_dir.clone(),
                downloaded_bytes: 0,
                total_bytes: 0,
                speed_bytes_per_sec: 0,
                message: String::new(),
            },
        );

        let tx = self.tx.clone();
        let spawn_dir = model_dir.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("oneasr-dl-{}", id.as_str()))
            .spawn(move || {
                let outcome = download_model(&handle, |p| {
                    let _ = tx.send(WorkerMsg::ModelDownload(p));
                });
                // Single terminal event — never double-emit Failed from Err.
                let snap = DownloadProgress::from_outcome(id, handle.model_dir.clone(), &outcome);
                let _ = tx.send(WorkerMsg::ModelDownload(snap));
            });
        if let Err(e) = spawn_result {
            // Roll the optimistic Downloading state back so the user can retry
            // instead of being stuck on a handle that will never report.
            crashlog::log_error(format!("spawn download thread failed: {e}"));
            self.clear_download_handle(id);
            self.set_download_progress(DownloadProgress {
                state: DownloadState::Failed,
                model_id: id,
                model_dir: spawn_dir,
                downloaded_bytes: 0,
                total_bytes: 0,
                speed_bytes_per_sec: 0,
                message: e.to_string(),
            });
            self.flash_hint(format!("下载启动失败: {e}"), cx);
        }
        cx.notify();
    }

    pub(crate) fn cancel_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        let handle = match id.kind() {
            ModelKind::Asr => self.asr_dl_handle.as_ref(),
            ModelKind::Align => self.align_dl_handle.as_ref(),
            ModelKind::Demucs => self.demucs_dl_handle.as_ref(),
        };
        if let Some(h) = handle
            && h.model_id == id {
            h.cancel();
            self.flash_hint("正在取消下载…", cx);
        }
        cx.notify();
    }

    /// True while **this** model id is downloading (UI busy / cancel for that row).
    pub(crate) fn download_busy(&self, id: ModelId) -> bool {
        let handle_match = match id.kind() {
            ModelKind::Asr => self.asr_dl_handle.as_ref().is_some_and(|h| h.model_id == id),
            ModelKind::Align => self
                .align_dl_handle
                .as_ref()
                .is_some_and(|h| h.model_id == id),
            ModelKind::Demucs => self
                .demucs_dl_handle
                .as_ref()
                .is_some_and(|h| h.model_id == id),
        };
        handle_match
            || self.progress_for(id).is_some_and(|p| p.state == DownloadState::Downloading)
    }

    /// One concurrent download per kind (0.6B and 1.7B share the ASR slot).
    pub(crate) fn download_kind_busy(&self, kind: ModelKind) -> bool {
        match kind {
            ModelKind::Asr => {
                self.asr_dl_handle.is_some()
                    || self
                        .asr_download
                        .as_ref()
                        .is_some_and(|p| p.state == DownloadState::Downloading)
            }
            ModelKind::Align => {
                self.align_dl_handle.is_some()
                    || self
                        .align_download
                        .as_ref()
                        .is_some_and(|p| p.state == DownloadState::Downloading)
            }
            ModelKind::Demucs => {
                self.demucs_dl_handle.is_some()
                    || self
                        .demucs_download
                        .as_ref()
                        .is_some_and(|p| p.state == DownloadState::Downloading)
            }
        }
    }

    /// Any model download in flight (drives settings gear spin).
    pub(crate) fn any_download_busy(&self) -> bool {
        self.download_kind_busy(ModelKind::Asr)
            || self.download_kind_busy(ModelKind::Align)
            || self.download_kind_busy(ModelKind::Demucs)
    }

    /// Progress snapshot for this exact model id (never another ASR size).
    pub(crate) fn progress_for(&self, id: ModelId) -> Option<&DownloadProgress> {
        let p = match id.kind() {
            ModelKind::Asr => self.asr_download.as_ref(),
            ModelKind::Align => self.align_download.as_ref(),
            ModelKind::Demucs => self.demucs_download.as_ref(),
        }?;
        if p.model_id == id {
            Some(p)
        } else {
            None
        }
    }

    pub(crate) fn set_download_progress(&mut self, progress: DownloadProgress) {
        match progress.model_id.kind() {
            ModelKind::Asr => self.asr_download = Some(progress),
            ModelKind::Align => self.align_download = Some(progress),
            ModelKind::Demucs => self.demucs_download = Some(progress),
        }
    }

    pub(crate) fn clear_download_handle(&mut self, id: ModelId) {
        match id.kind() {
            ModelKind::Asr => self.asr_dl_handle = None,
            ModelKind::Align => self.align_dl_handle = None,
            ModelKind::Demucs => self.demucs_dl_handle = None,
        }
    }

    /// Drop progress snapshot when it no longer belongs to the visible selection.
    pub(crate) fn clear_stale_asr_progress(&mut self) {
        if let Some(p) = &self.asr_download
            && p.model_id != self.settings.selected_asr_id()
            && p.state != DownloadState::Downloading
        {
            self.asr_download = None;
        }
    }
}
