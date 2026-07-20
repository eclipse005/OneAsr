//! OneAsr — light-themed batch list for local Qwen ASR + align.

mod assets;
mod sfx;
mod shell;
mod theme;

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    canvas, div, hsla, linear, percentage, point, prelude::*, px, size, svg, Animation,
    AnimationExt as _, App, Application, Bounds, BoxShadow, ClickEvent, Context,
    ExternalPaths, MouseMoveEvent, Pixels, SharedString, Timer, Transformation, Window,
    WindowBounds, WindowOptions,
};
use oneasr_core::{
    accept_input_path, check_model_dir, demote_current_thread, download_model, empty_state_subtitle,
    empty_state_title, format_batch_progress, format_queue_status, init_runtime, next_queue_seq,
    process_media_file_with_progress, probe_duration_sec, resolve_app_root, unload_session,
    AsrStage, DownloadHandle, DownloadProgress, DownloadState, DurationState, ModelId, ModelKind,
    Settings, StageUpdate, Task, TaskStatus,
};

use theme::{
    ACCENT, ACCENT_MIST, ACCENT_SOFT, BG, DANGER, DANGER_SOFT, LINE, LINE_SOFT, LOGO, MEDIA_PLATE,
    MUTED, MUTED_SOFT, PANEL, ROW_HOVER, TEXT, WARN, WARN_SOFT, ZEBRA,
};

/// Status-bar model indicator: **file probe only** (not weight load).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelStatus {
    /// Directory has required model files.
    Ready,
    /// Missing / incomplete model files.
    NotReady,
}

enum WorkerMsg {
    FilesPicked(Vec<PathBuf>),
    PickCancelled,
    ModelDirPicked(PathBuf),
    AlignerDirPicked(PathBuf),
    Probed {
        id: String,
        duration_sec: Option<f64>,
    },
    /// Live stage from the dedicated ASR worker (UI-only, never blocks).
    Progress {
        id: String,
        stage: SharedString,
    },
    Finished {
        id: String,
        result: Result<PathBuf, String>,
    },
    /// Model download progress / completion (background thread).
    ModelDownload(DownloadProgress),
}

/// Jobs for the long-lived ASR worker (one active job at a time by design).
enum AsrJob {
    Run {
        id: String,
        path: PathBuf,
        name: String,
        settings: Settings,
    },
}

fn main() {
    // Cap rayon before any model load so the UI thread keeps a free core.
    init_runtime();

    Application::new()
        .with_assets(assets::AppAssets::new())
        .run(|cx: &mut App| {
            // Compact default: list + toolbar, not a full-HD empty canvas.
            let bounds = Bounds::centered(None, size(px(860.), px(560.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |_, cx| cx.new(OneAsrApp::new),
            )
            .expect("open window");
            cx.activate(true);
        });
}

struct OneAsrApp {
    settings: Settings,
    /// Unsaved settings edits (backend / paths).
    settings_dirty: bool,
    /// Desired drawer state (open / closed).
    settings_open: bool,
    /// Drawer slide progress source → target (0 = hidden, 1 = open).
    settings_from: f32,
    settings_to: f32,
    settings_anim_t0: Instant,
    /// Row currently under the pointer (action highlight).
    hover_row: Option<String>,
    /// Empty-state wave: pointer currently over the strip.
    empty_wave_hover: bool,
    /// Last layout bounds of the wave hit area (window coords).
    empty_wave_bounds: Option<Bounds<Pixels>>,
    /// Target cursor X along the strip (0..=1).
    empty_wave_cursor_x: f32,
    /// Smoothed cursor X (follows target).
    empty_wave_smooth_x: f32,
    /// 0 = flat line, 1 = full local bulge (eased in/out).
    empty_wave_amp: f32,
    /// Epoch for time-based shimmer under the bulge.
    empty_wave_clock: Instant,
    tasks: Vec<Task>,
    /// Rows just added — opacity 0→1 over `ROW_ENTER_SECS`.
    entering: HashMap<String, Instant>,
    /// Rows marked for delete/clear, still in `tasks` until fade completes
    /// (tombstones keep list order — do not move to the bottom while fading).
    exiting: HashMap<String, Instant>,
    batch_mode: bool,
    /// Jobs planned for the current batch run (for 进度 n/m).
    batch_goal: Option<usize>,
    /// Finished (ok or err) count within the current batch.
    batch_done: usize,
    busy: bool,
    picking: bool,
    model_status: ModelStatus,
    /// Soft status-bar hint (no toast). Auto-clears after a few seconds.
    status_hint: Option<SharedString>,
    status_hint_until: Option<Instant>,
    /// Advances while any task is probing duration (drives soft spinner).
    ui_phase: u8,
    /// Live stage label for the row currently Processing (from worker Progress).
    active_stage: Option<(String, SharedString)>,
    /// Latest download progress for ASR / Aligner (settings panel).
    asr_download: Option<DownloadProgress>,
    align_download: Option<DownloadProgress>,
    /// Active download cancel handles (install-layout destination only).
    asr_dl_handle: Option<DownloadHandle>,
    align_dl_handle: Option<DownloadHandle>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
    /// Dedicated ASR worker (never run heavy work on the UI thread).
    job_tx: Sender<AsrJob>,
}

impl OneAsrApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx) = mpsc::channel();
        let (job_tx, job_rx) = mpsc::channel::<AsrJob>();

        // Long-lived worker: load/transcribe here only; demoted priority.
        let worker_tx = tx.clone();
        thread::Builder::new()
            .name("oneasr-asr-worker".into())
            .spawn(move || {
                demote_current_thread();
                while let Ok(job) = job_rx.recv() {
                    match job {
                        AsrJob::Run {
                            id,
                            path,
                            name,
                            settings,
                        } => {
                            let id_for_progress = id.clone();
                            let ptx = worker_tx.clone();
                            let result = run_task(&path, &name, &settings, move |update| {
                                let _ = ptx.send(WorkerMsg::Progress {
                                    id: id_for_progress.clone(),
                                    stage: SharedString::from(update.label()),
                                });
                            });
                            let _ = worker_tx.send(WorkerMsg::Finished { id, result });
                        }
                    }
                }
            })
            .expect("spawn ASR worker");

        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(80)).await;
            this.update(cx, |app, cx| app.poll_worker(cx)).ok();
        })
        .detach();

        let settings = Settings::load();

        let mut app = Self {
            settings,
            settings_dirty: false,
            settings_open: false,
            settings_from: 0.0,
            settings_to: 0.0,
            settings_anim_t0: Instant::now(),
            hover_row: None,
            empty_wave_hover: false,
            empty_wave_bounds: None,
            empty_wave_cursor_x: 0.5,
            empty_wave_smooth_x: 0.5,
            empty_wave_amp: 0.0,
            empty_wave_clock: Instant::now(),
            tasks: Vec::new(),
            entering: HashMap::new(),
            exiting: HashMap::new(),
            batch_mode: false,
            batch_goal: None,
            batch_done: 0,
            busy: false,
            picking: false,
            model_status: ModelStatus::NotReady,
            status_hint: None,
            status_hint_until: None,
            ui_phase: 0,
            active_stage: None,
            asr_download: None,
            align_download: None,
            asr_dl_handle: None,
            align_dl_handle: None,
            tx: tx.clone(),
            rx,
            job_tx,
        };

        // Probe files only — never load multi-GB weights on startup.
        app.refresh_model_probe();
        app
    }

    /// Start ModelScope download into `{app}/models/...` (background).
    /// Does **not** rewrite settings until `DownloadOutcome::Completed`.
    fn start_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        if self.download_busy(id) {
            self.flash_hint("该模型已有下载任务进行中", cx);
            return;
        }

        let handle = DownloadHandle::new(id);
        let model_dir = handle.model_dir.clone();
        match id.kind() {
            ModelKind::Asr => self.asr_dl_handle = Some(handle.clone()),
            ModelKind::Align => self.align_dl_handle = Some(handle.clone()),
        }
        self.set_download_progress(
            DownloadProgress {
                state: DownloadState::Downloading,
                model_id: id,
                model_dir: model_dir.clone(),
                downloaded_bytes: 0,
                total_bytes: 0,
                speed_bytes_per_sec: 0,
                message: format!("→ {}", model_dir.display()),
            },
        );

        let tx = self.tx.clone();
        thread::Builder::new()
            .name(format!("oneasr-dl-{}", id.as_str()))
            .spawn(move || {
                let outcome = download_model(&handle, |p| {
                    let _ = tx.send(WorkerMsg::ModelDownload(p));
                });
                // Single terminal event — never double-emit Failed from Err.
                let snap = DownloadProgress::from_outcome(id, handle.model_dir.clone(), &outcome);
                let _ = tx.send(WorkerMsg::ModelDownload(snap));
            })
            .ok();
        cx.notify();
    }

    fn cancel_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        match id.kind() {
            ModelKind::Asr => {
                if let Some(h) = &self.asr_dl_handle {
                    h.cancel();
                }
            }
            ModelKind::Align => {
                if let Some(h) = &self.align_dl_handle {
                    h.cancel();
                }
            }
        }
        self.flash_hint("正在取消下载…", cx);
        cx.notify();
    }

    fn download_busy(&self, id: ModelId) -> bool {
        let p = match id.kind() {
            ModelKind::Asr => self.asr_download.as_ref(),
            ModelKind::Align => self.align_download.as_ref(),
        };
        p.is_some_and(|x| x.state == DownloadState::Downloading)
    }

    fn set_download_progress(&mut self, progress: DownloadProgress) {
        match progress.model_id.kind() {
            ModelKind::Asr => self.asr_download = Some(progress),
            ModelKind::Align => self.align_download = Some(progress),
        }
    }

    fn clear_download_handle(&mut self, id: ModelId) {
        match id.kind() {
            ModelKind::Asr => self.asr_dl_handle = None,
            ModelKind::Align => self.align_dl_handle = None,
        }
    }

        /// Fast FS check for status bar. Does not touch GPU / weights.
    fn refresh_model_probe(&mut self) {
        self.model_status = if check_model_dir(&self.settings.asr_model_dir).is_ok()
            && oneasr_core::check_aligner_model_dir(&self.settings.aligner_model_dir).is_ok()
        {
            ModelStatus::Ready
        } else {
            ModelStatus::NotReady
        };
    }

    /// Drop any in-memory session and re-probe the configured folder.
    fn reset_model_config(&mut self, cx: &mut Context<Self>) {
        unload_session();
        self.refresh_model_probe();
        cx.notify();
    }

    fn mark_settings_dirty(&mut self, cx: &mut Context<Self>) {
        if !self.settings_dirty {
            self.settings_dirty = true;
            cx.notify();
        }
    }

    fn is_settings_dirty(&self, _cx: &Context<Self>) -> bool {
        self.settings_dirty
    }

    /// Write `settings.json`, re-check model files.
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        match self.settings.save() {
            Ok(_) => {
                self.settings_dirty = false;
                self.reset_model_config(cx);
                self.flash_hint(
                    match self.model_status {
                        ModelStatus::Ready => "设置已保存 · 模型就绪",
                        ModelStatus::NotReady => "设置已保存 · 模型未就绪",
                    },
                    cx,
                );
            }
            Err(e) => {
                self.flash_hint(format!("保存失败: {e}"), cx);
            }
        }
    }

    fn poll_worker(&mut self, cx: &mut Context<Self>) {
        // Soft UI phase tick: duration probe · pending breath · processing dots.
        let need_phase = self.tasks.iter().any(|t| {
            t.duration == DurationState::Probing || t.status == TaskStatus::Processing
        });
        if need_phase {
            self.ui_phase = self.ui_phase.wrapping_add(1);
            cx.notify();
        }
        // Expire status-bar hints.
        if let Some(until) = self.status_hint_until {
            if Instant::now() >= until {
                self.status_hint = None;
                self.status_hint_until = None;
                cx.notify();
            } else {
                cx.notify(); // keep bar live while hint is visible
            }
        }
        loop {
            match self.rx.try_recv() {
                Ok(WorkerMsg::FilesPicked(paths)) => {
                    self.picking = false;
                    self.add_paths(paths, cx);
                }
                Ok(WorkerMsg::PickCancelled) => {
                    self.picking = false;
                    cx.notify();
                }
                Ok(WorkerMsg::ModelDirPicked(dir)) => {
                    self.settings.asr_model_dir = dir;
                    self.settings_dirty = false;
                    // Probe + persist path immediately after pick.
                    self.reset_model_config(cx);
                    if let Err(e) = self.settings.save() {
                        self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
                    } else {
                        self.flash_hint(
                            match self.model_status {
                                ModelStatus::Ready => "ASR 模型目录已更新 · 就绪",
                                ModelStatus::NotReady => "ASR 模型目录已更新 · 未就绪",
                            },
                            cx,
                        );
                    }
                }
                Ok(WorkerMsg::AlignerDirPicked(dir)) => {
                    self.settings.aligner_model_dir = dir;
                    self.settings_dirty = false;
                    self.reset_model_config(cx);
                    if let Err(e) = self.settings.save() {
                        self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
                    } else {
                        self.flash_hint(
                            match self.model_status {
                                ModelStatus::Ready => "Aligner 目录已更新 · 就绪",
                                ModelStatus::NotReady => "Aligner 目录已更新 · 未就绪",
                            },
                            cx,
                        );
                    }
                }
                Ok(WorkerMsg::ModelDownload(progress)) => {
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
                        // Only now bind settings to install-layout path.
                        self.settings.apply_downloaded_model(id, progress.model_dir.clone());
                        let _ = self.settings.save();
                        self.settings_dirty = false;
                        self.reset_model_config(cx);
                        self.flash_hint(format!("{} 下载完成", id.label()), cx);
                    } else if progress.state == DownloadState::Failed {
                        self.flash_hint(
                            format!("{} 下载失败: {}", id.label(), progress.message),
                            cx,
                        );
                    } else if progress.state == DownloadState::Cancelled {
                        self.flash_hint(format!("{} 已取消", id.label()), cx);
                    }
                    cx.notify();
                }
                Ok(WorkerMsg::Probed { id, duration_sec }) => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        t.set_duration(duration_sec);
                    }
                    cx.notify();
                }
                Ok(WorkerMsg::Progress { id, stage }) => {
                    self.active_stage = Some((id, stage));
                    cx.notify();
                }
                Ok(WorkerMsg::Finished { id, result }) => {
                    self.busy = false;
                    if self
                        .active_stage
                        .as_ref()
                        .is_some_and(|(sid, _)| sid == &id)
                    {
                        self.active_stage = None;
                    }
                    self.refresh_model_probe();
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        match result {
                            Ok(srt) => {
                                t.status = TaskStatus::Done;
                                t.queue_seq = None;
                                t.output_srt = Some(srt);
                                t.error = None;
                            }
                            Err(e) => {
                                t.status = TaskStatus::Error;
                                t.queue_seq = None;
                                t.error = Some(e);
                            }
                        }
                    }
                    if self.batch_mode {
                        self.batch_done = self.batch_done.saturating_add(1);
                    }
                    cx.notify();
                    // Always drain the FIFO queue (one at a time).
                    self.try_start_next(cx);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    fn add_files_dialog(&mut self, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title("添加音视频")
                .add_filter(
                    "Media",
                    &[
                        "wav", "mp3", "m4a", "flac", "ogg", "opus", "mp4", "mkv", "mov", "webm",
                        "avi", "m4v", "aac", "wma",
                    ],
                )
                .pick_files();
            match files {
                Some(paths) => {
                    let _ = tx.send(WorkerMsg::FilesPicked(paths));
                }
                None => {
                    let _ = tx.send(WorkerMsg::PickCancelled);
                }
            }
        });
        cx.notify();
    }

    fn pick_model_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.asr_model_dir.clone();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title("选择 ASR 模型目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::ModelDirPicked(dir));
            }
        });
        cx.notify();
    }

    fn pick_aligner_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.aligner_model_dir.clone();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title("选择 Aligner 模型目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::AlignerDirPicked(dir));
            }
        });
        cx.notify();
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        for path in paths {
            let path = path.canonicalize().unwrap_or(path);
            if !accept_input_path(&path) {
                continue;
            }
            if self.tasks.iter().any(|t| t.path == path) {
                continue;
            }
            let task = Task::from_path(&path);
            let id = task.id.clone();
            let p = task.path.clone();
            let tx = self.tx.clone();
            thread::spawn(move || {
                let dur = probe_duration_sec(&p);
                let _ = tx.send(WorkerMsg::Probed {
                    id,
                    duration_sec: dur,
                });
            });
            let anim_id = task.id.clone();
            self.tasks.push(task);
            // Wave: fade the new row in (symmetric with delete exit).
            self.entering
                .entry(anim_id)
                .or_insert_with(Instant::now);
        }
        // Feedback = list itself (no toast).
        cx.notify();
    }

    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        let open = !self.settings_open;
        // Closing with unsaved edits → auto-save (desktop-tool default).
        if !open && self.is_settings_dirty(cx) {
            self.save_settings(cx);
        }
        let cur = self.settings_progress();
        self.settings_from = cur;
        self.settings_to = if open { 1.0 } else { 0.0 };
        self.settings_anim_t0 = Instant::now();
        self.settings_open = open;
        if open {
            sfx::play(sfx::Sfx::Drawer);
        }
        cx.notify();
    }

    /// Eased 0..=1 drawer progress (works mid-animation when retoggled).
    fn settings_progress(&self) -> f32 {
        let t = (self.settings_anim_t0.elapsed().as_secs_f32() / DRAWER_ANIM_SECS).min(1.0);
        let e = ease_out_cubic(t);
        self.settings_from + (self.settings_to - self.settings_from) * e
    }

    fn animations_active(&self) -> bool {
        // Drawer slide + processing status dots + row enter/exit + empty-wave hover.
        let drawer = (self.settings_progress() - self.settings_to).abs() > 0.002;
        let processing = self
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Processing);
        let row_anim = !self.exiting.is_empty() || !self.entering.is_empty();
        // Keep RAF while amp eases out after mouse leaves (smooth collapse to flat).
        let empty_wave =
            self.tasks.is_empty() && (self.empty_wave_hover || self.empty_wave_amp > 0.008);
        drawer || processing || row_anim || empty_wave
    }

    /// Advance empty-wave smoothing (cursor follow + amp ease). Call once per frame while active.
    fn tick_empty_wave(&mut self) {
        let target_amp = if self.empty_wave_hover { 1.0 } else { 0.0 };
        // Snappy but not instant — ~120–180ms feel at 60fps.
        self.empty_wave_amp += (target_amp - self.empty_wave_amp) * 0.18;
        if self.empty_wave_amp < 0.004 && !self.empty_wave_hover {
            self.empty_wave_amp = 0.0;
        }
        self.empty_wave_smooth_x +=
            (self.empty_wave_cursor_x - self.empty_wave_smooth_x) * 0.22;
    }

    fn is_exiting(&self, id: &str) -> bool {
        self.exiting.contains_key(id)
    }

    /// Drop finished enter/exit fades (call once per frame from list render).
    /// Exit tombstones are removed from `tasks` only after the fade completes.
    fn drain_row_anims(&mut self) {
        let now = Instant::now();
        let finished: Vec<String> = self
            .exiting
            .iter()
            .filter(|(_, t0)| now.duration_since(**t0).as_secs_f32() >= ROW_EXIT_SECS)
            .map(|(id, _)| id.clone())
            .collect();
        for id in finished {
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

    /// Brief message in the bottom status bar (replaces toast).
    fn flash_hint(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.flash_hint_for(msg, Duration::from_secs(3), cx);
    }

    fn flash_hint_for(
        &mut self,
        msg: impl Into<SharedString>,
        dur: Duration,
        cx: &mut Context<Self>,
    ) {
        self.status_hint = Some(msg.into());
        self.status_hint_until = Some(Instant::now() + dur);
        cx.notify();
    }

    fn begin_batch(&mut self, job_count: usize) {
        if job_count == 0 {
            return;
        }
        if self.batch_mode {
            if let Some(g) = self.batch_goal.as_mut() {
                *g = g.saturating_add(job_count);
            } else {
                self.batch_goal = Some(job_count);
            }
        } else {
            self.batch_mode = true;
            self.batch_goal = Some(job_count);
            self.batch_done = 0;
        }
    }

    fn end_batch_if_idle(&mut self, cx: &mut Context<Self>) {
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
        let done = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let err = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Error)
            .count();
        let goal = self.batch_goal.unwrap_or(self.batch_done);
        self.batch_mode = false;
        self.batch_goal = None;
        self.batch_done = 0;
        if goal == 0 && done == 0 && err == 0 {
            return;
        }
        let msg = if err == 0 {
            format!("全部完成 · {done} 个任务")
        } else if done == 0 {
            format!("批次结束 · {err} 个失败")
        } else {
            format!("批次结束 · 完成 {done} · 失败 {err}")
        };
        self.flash_hint_for(msg, Duration::from_secs(6), cx);
    }

    /// Soft gate before enqueue. Queue/start buttons also check model files.
    fn ensure_can_start(&mut self, cx: &mut Context<Self>) -> bool {
        if self.model_status != ModelStatus::Ready {
            self.flash_hint("模型未就绪，请在设置中下载或选择模型目录", cx);
            if !self.settings_open {
                self.toggle_settings(cx);
            }
            return false;
        }
        true
    }

    fn start_all(&mut self, cx: &mut Context<Self>) {
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
            let live: Vec<_> = self
                .tasks
                .iter()
                .filter(|t| !self.exiting.contains_key(&t.id))
                .collect();
            if live.is_empty() {
                self.flash_hint("请先添加音视频文件", cx);
            } else if live.iter().any(|t| t.status == TaskStatus::Processing)
                || live.iter().any(|t| t.status == TaskStatus::Queued)
            {
                self.flash_hint("任务已在处理或排队中", cx);
            } else {
                self.flash_hint("没有可开始的任务", cx);
            }
            return;
        }
        sfx::play(sfx::Sfx::Click);
        self.begin_batch(enqueued);
        self.try_start_next(cx);
        cx.notify();
    }

    fn start_one(&mut self, id: &str, cx: &mut Context<Self>) {
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
        match status {
            TaskStatus::Processing => {
                self.flash_hint("该任务正在处理中", cx);
                return;
            }
            TaskStatus::Queued => {
                self.flash_hint("该任务已在队列中", cx);
                return;
            }
            TaskStatus::Done => {
                self.flash_hint("该任务已完成", cx);
                return;
            }
            TaskStatus::Pending | TaskStatus::Error => {}
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
        sfx::play(sfx::Sfx::Click);
        self.begin_batch(1);
        if slot_free {
            self.try_start_next(cx);
        } else {
            cx.notify();
        }
    }

    /// Start the oldest queued job if the worker slot is free.
    fn try_start_next(&mut self, cx: &mut Context<Self>) {
        if self.busy || self.tasks.iter().any(|t| t.status == TaskStatus::Processing) {
            return;
        }
        let next_id = {
            let mut queued: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Queued && !self.exiting.contains_key(&t.id))
                .collect();
            queued.sort_by_key(|t| t.queue_seq.unwrap_or(u64::MAX));
            queued.first().map(|t| t.id.clone())
        };
        match next_id {
            Some(id) => self.launch_task(&id, cx),
            None => {
                self.end_batch_if_idle(cx);
                cx.notify();
            }
        }
    }

    fn launch_task(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if task.status != TaskStatus::Queued && task.status != TaskStatus::Pending {
            return;
        }
        task.status = TaskStatus::Processing;
        task.queue_seq = None;
        task.error = None;
        self.busy = true;
        self.active_stage = Some((
            task.id.clone(),
            SharedString::from(AsrStage::LoadingModel.label()),
        ));

        let id = task.id.clone();
        let path = task.path.clone();
        let name = task.name.clone();
        let settings = self.settings.clone();

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
            self.busy = false;
            self.active_stage = None;
            if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                t.status = TaskStatus::Error;
                t.error = Some("ASR 工作线程已退出".into());
            }
        }
        cx.notify();
    }

    fn delete_task(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
            if t.status.locks_row_actions() {
                self.flash_hint("处理中的任务不能删除", cx);
                return;
            }
        } else {
            return;
        }
        // Already fading out — ignore double-clicks.
        if self.is_exiting(id) {
            return;
        }
        sfx::play(sfx::Sfx::Click);
        // Tombstone in place so the row fades without jumping to the list bottom.
        self.entering.remove(id);
        self.exiting.insert(id.to_string(), Instant::now());
        if self.hover_row.as_ref().is_some_and(|h| h == id) {
            self.hover_row = None;
        }
        cx.notify();
    }

    /// Clear the whole list. Keeps the active Processing row if any.
    /// Non-processing rows fade out in place (same 0.22s exit as single delete).
    fn clear_all(&mut self, cx: &mut Context<Self>) {
        let live_any = self.tasks.iter().any(|t| !self.exiting.contains_key(&t.id));
        if !live_any {
            self.flash_hint("列表已空", cx);
            return;
        }
        let had_proc = self.tasks.iter().any(|t| {
            t.status == TaskStatus::Processing && !self.exiting.contains_key(&t.id)
        });
        sfx::play(sfx::Sfx::Click);
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
            self.batch_goal = None;
            self.batch_done = 0;
        } else if let Some(g) = self.batch_goal.as_mut() {
            // Keep only the active job in the batch goal.
            *g = 1;
            self.batch_done = 0;
        }
        self.hover_row = None;
        if had_proc {
            self.flash_hint("已清空队列，当前任务继续处理", cx);
        }
        cx.notify();
    }

    fn open_task_output(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(path) = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.output_srt.clone())
        else {
            return;
        };
        // OS explorer is the feedback; no in-app banner.
        let _ = shell::open_containing_folder(&path);
        cx.notify();
    }

}

fn run_task(
    path: &std::path::Path,
    name: &str,
    settings: &Settings,
    on_stage: impl FnMut(StageUpdate),
) -> Result<PathBuf, String> {
    let app_root =
        resolve_app_root().ok_or_else(|| "找不到应用目录（需含 bin/ffmpeg.exe）".to_string())?;
    // Primary deliverable: {app_root}/output/{stem}.srt (real ASR, no stubs).
    // Runs only on the dedicated ASR worker thread.
    process_media_file_with_progress(path, name, settings, &app_root, on_stage)
        .map_err(|e| e.to_string())
}

impl Render for OneAsrApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drive drawer / row-hover fades at display refresh.
        if self.animations_active() {
            window.request_animation_frame();
        }

        let drawer_p = self.settings_progress();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(BG)
            .text_color(TEXT)
            .font_family("Segoe UI")
            .child(self.render_toolbar(cx))
            .child(
                // Relative shell: list fills; settings drawer overlays from the right.
                div()
                    .flex_1()
                    .relative()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("task-drop-zone")
                            .size_full()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .bg(PANEL)
                            .border_2()
                            .border_color(LINE)
                            .overflow_hidden()
                            .can_drop(|drag, _, _| drag.is::<ExternalPaths>())
                            .drag_over::<ExternalPaths>(|style, _, _, _| {
                                style
                                    .border_color(ACCENT)
                                    .border_2()
                                    .border_dashed()
                                    .bg(ACCENT_SOFT)
                            })
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                                this.add_paths(paths.paths().to_vec(), cx);
                            }))
                            .child(self.render_list(cx)),
                    )
                    // Keep mounted while animating closed (p > 0).
                    .when(drawer_p > 0.001, |el| {
                        let slide = (1.0 - drawer_p) * SETTINGS_W;
                        let fade = 0.25 + 0.75 * drawer_p;
                        el.child(
                            div()
                                .id("settings-drawer")
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(-slide))
                                .w(px(SETTINGS_W))
                                .opacity(fade)
                                .border_l_1()
                                .border_color(LINE)
                                .bg(PANEL)
                                .shadow(vec![
                                    BoxShadow {
                                        color: hsla(0.0, 0.0, 0.0, 0.06 * drawer_p),
                                        blur_radius: px(4.0),
                                        spread_radius: px(0.0),
                                        offset: point(px(-1.0), px(0.0)),
                                    },
                                    BoxShadow {
                                        color: hsla(0.0, 0.0, 0.0, 0.12 * drawer_p),
                                        blur_radius: px(28.0),
                                        spread_radius: px(-2.0),
                                        offset: point(px(-10.0), px(0.0)),
                                    },
                                ])
                                .child(self.render_settings(cx)),
                        )
                    }),
            )
            .child(self.render_status_bar())
    }
}

/// Settings drawer width (overlay, does not shrink the list).
const SETTINGS_W: f32 = 400.;
const DRAWER_ANIM_SECS: f32 = 0.34;
/// Row add fade-in duration (seconds).
const ROW_ENTER_SECS: f32 = 0.30;
/// Row delete / clear fade-out duration (seconds).
const ROW_EXIT_SECS: f32 = 0.22;
/// Actions column: play + delete (+ open/copy when done).
const ACTIONS_COL_PX: f32 = 120.;

/// Smooth deceleration (approx cubic-bezier ease-out).
fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

impl OneAsrApp {
    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings_open = self.settings_open;
        let picking = self.picking;
        let can_start = self.model_status == ModelStatus::Ready;
        let has_tasks = self.tasks.iter().any(|t| !self.exiting.contains_key(&t.id));

        div()
            .h(px(52.))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .bg(PANEL)
            .border_b_1()
            .border_color(LINE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2p5()
                    .child(app_logo())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_center()
                            .gap_0()
                            .child(
                                div()
                                    .flex()
                                    .items_baseline()
                                    .gap_0()
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(TEXT)
                                            .child("One"),
                                    )
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(ACCENT)
                                            .child("Asr"),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_SOFT)
                                    .child("本地转写"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(btn(
                        if picking { "选择中…" } else { "添加" },
                        BtnKind::Secondary,
                        true,
                        cx.listener(|this, _, _, cx| this.add_files_dialog(cx)),
                    ))
                    .child(btn_cta(
                        "全部开始",
                        can_start,
                        "模型未就绪，请先在设置中选择完整模型目录",
                        cx.listener(|this, _, _, cx| this.start_all(cx)),
                    ))
                    .child(btn(
                        "清空",
                        BtnKind::Quiet,
                        has_tasks,
                        cx.listener(|this, _, _, cx| this.clear_all(cx)),
                    ))
                    .child(settings_gear_btn(
                        settings_open,
                        cx.listener(|this, _, _, cx| this.toggle_settings(cx)),
                    )),
            )
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_row_anims();

        if self.tasks.is_empty() {
            let title = empty_state_title();
            let not_ready = self.model_status == ModelStatus::NotReady;
            let subtitle = empty_state_subtitle(not_ready);
            self.tick_empty_wave();
            let wave = self.render_empty_wave(cx);
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .w_full()
                .min_w_0()
                // Full-width interactive wave sits above the copy block.
                .child(wave)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .px_6()
                        .mt_4()
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(MUTED)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(MUTED_SOFT)
                                .text_center()
                                .max_w(px(320.))
                                .child(subtitle),
                        )
                        .child(
                            div()
                                .mt_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(btn(
                                    "添加文件",
                                    BtnKind::Primary,
                                    true,
                                    cx.listener(|this, _, _, cx| this.add_files_dialog(cx)),
                                ))
                                .when(not_ready, |el| {
                                    el.child(btn(
                                        "打开设置",
                                        BtnKind::Secondary,
                                        true,
                                        cx.listener(|this, _, _, cx| {
                                            if !this.settings_open {
                                                this.toggle_settings(cx);
                                            }
                                        }),
                                    ))
                                }),
                        ),
                )
                .into_any_element();
        }

        // Rows stay in `tasks` order. Exit tombstones fade in place (1→0);
        // enter map fades new rows (0→1). Exiting rows are non-interactive.
        let now = Instant::now();
        let items: Vec<(Task, f32, bool)> = self
            .tasks
            .iter()
            .cloned()
            .map(|t| {
                if let Some(t0) = self.exiting.get(&t.id) {
                    let p = (now.duration_since(*t0).as_secs_f32() / ROW_EXIT_SECS).clamp(0.0, 1.0);
                    let opacity = 1.0 - ease_out_cubic(p);
                    (t, opacity, false)
                } else {
                    let opacity = self
                        .entering
                        .get(&t.id)
                        .map(|t0| {
                            let p = (now.duration_since(*t0).as_secs_f32() / ROW_ENTER_SECS)
                                .clamp(0.0, 1.0);
                            ease_out_cubic(p)
                        })
                        .unwrap_or(1.0);
                    (t, opacity, true)
                }
            })
            .collect();

        let phase = self.ui_phase;
        let hover_id = self.hover_row.clone();
        let active_stage = self.active_stage.clone();
        // Snapshot ranks for 排队中#n labels (live queue only).
        let queue_ranks: Vec<(String, usize)> = {
            let mut q: Vec<&Task> = self
                .tasks
                .iter()
                .filter(|t| t.status == TaskStatus::Queued)
                .collect();
            q.sort_by_key(|t| t.queue_seq.unwrap_or(u64::MAX));
            q.into_iter()
                .enumerate()
                .map(|(i, t)| (t.id.clone(), i + 1))
                .collect()
        };

        div()
            .id("task-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(items.into_iter().enumerate().map(move |(ix, (task, row_opacity, interactive))| {
                let id_start = task.id.clone();
                let id_del = task.id.clone();
                let id_open = task.id.clone();
                // Always show primary + delete slots (gray when locked — never disappear).
                let done_with_out =
                    task.status == TaskStatus::Done && task.output_srt.is_some();
                // Done → open folder (same slot as start); else start.
                let primary_kind = if done_with_out {
                    IconKind::Folder
                } else {
                    IconKind::Play
                };
                let primary_tip = if done_with_out {
                    "打开字幕位置"
                } else {
                    "开始"
                };
                let primary_enabled = interactive
                    && if done_with_out {
                        true
                    } else {
                        matches!(task.status, TaskStatus::Pending | TaskStatus::Error)
                    };
                let can_delete = interactive && !task.status.locks_row_actions();
                let err = task.error.clone();
                let name = task.name.clone();
                let name_tip = task.name.clone();
                let row_id = task.id.clone();
                let row_id_hover = task.id.clone();
                let size_l = task.size_label();
                let duration = task.duration;
                let status = task.status;
                let is_video = is_video_format(&task.format);
                let is_hovered = interactive && hover_id.as_ref() == Some(&task.id);
                let qn = queue_ranks
                    .iter()
                    .find(|(id, _)| id == &task.id)
                    .map(|(_, n)| *n);

                // Meta: size · duration · status text (no free-floating status circle).
                let dur = match duration {
                    DurationState::Probing => "…".into(),
                    other => other.label(),
                };
                let stage_for_row = active_stage
                    .as_ref()
                    .filter(|(sid, _)| sid == &task.id)
                    .map(|(_, s)| s.as_ref());
                let (status_label, status_color, status_bg) =
                    status_pill_style(status, phase, qn, stage_for_row);
                let meta_left = format!("{size_l}  ·  {dur}");
                let accent = row_accent(status);

                let row_bg = if is_hovered {
                    ROW_HOVER
                } else if ix % 2 == 1 {
                    ZEBRA
                } else {
                    PANEL
                };

                div()
                    .id(SharedString::from(format!("task-{row_id}")))
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(LINE_SOFT)
                    .bg(row_bg)
                    .opacity(row_opacity)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if !interactive {
                            return;
                        }
                        if *hovered {
                            this.hover_row = Some(row_id_hover.clone());
                        } else if this.hover_row.as_ref() == Some(&row_id_hover) {
                            this.hover_row = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .w_full()
                            .min_w_0()
                            // Left status accent as border (full row height, no stretch API needed).
                            .border_l_4()
                            .border_color(accent)
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_3()
                                    .py_3()
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .child(media_type_icon(is_video, status)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_0p5()
                                            .overflow_hidden()
                                            .pr_2()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "name-{row_id}"
                                                    )))
                                                    .w_full()
                                                    .min_w_0()
                                                    .overflow_hidden()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .whitespace_nowrap()
                                                    .child(name)
                                                    .tooltip(move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: name_tip.clone().into(),
                                                        })
                                                        .into()
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED_SOFT)
                                                    .whitespace_nowrap()
                                                    .child(meta_left),
                                            ),
                                    )
                                    // Status as its own pill (not mixed into file meta).
                                    .child(
                                        div()
                                            .flex_shrink_0()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_full()
                                            .bg(status_bg)
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(status_color)
                                                    .whitespace_nowrap()
                                                    .child(status_label),
                                            ),
                                    )
                                    // Two fixed slots: primary (开始 | 打开字幕) + 删除.
                                    .child(
                                        div()
                                            .w(px(ACTIONS_COL_PX))
                                            .flex_shrink_0()
                                            .flex()
                                            .gap_2()
                                            .justify_end()
                                            .items_center()
                                            .child(icon_btn(
                                                primary_kind,
                                                primary_tip,
                                                primary_enabled,
                                                is_hovered,
                                                cx.listener(move |this, _, _, cx| {
                                                    if this
                                                        .tasks
                                                        .iter()
                                                        .find(|t| t.id == id_start)
                                                        .map(|t| {
                                                            t.status == TaskStatus::Done
                                                                && t.output_srt.is_some()
                                                        })
                                                        .unwrap_or(false)
                                                    {
                                                        this.open_task_output(&id_open, cx);
                                                    } else {
                                                        this.start_one(&id_start, cx);
                                                    }
                                                }),
                                            ))
                                            .child(icon_btn(
                                                IconKind::Trash,
                                                "删除",
                                                can_delete,
                                                is_hovered,
                                                cx.listener(move |this, _, _, cx| {
                                                    this.delete_task(&id_del, cx);
                                                }),
                                            )),
                                    ),
                            ),
                    )
                    .when(err.is_some(), |el| {
                        el.child(
                            div()
                                .px_3()
                                .pb_2()
                                .pl(px(48.))
                                .child(
                                    div()
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .bg(DANGER_SOFT)
                                        .text_xs()
                                        .text_color(DANGER)
                                        .child(err.unwrap_or_default()),
                                ),
                        )
                    })
            }))
            .into_any_element()
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let backend = self.settings.backend.clone();
        let model = self.settings.asr_model_dir.display().to_string();
        let model_tip = model.clone();
        let aligner = self.settings.aligner_model_dir.display().to_string();
        let aligner_tip = aligner.clone();
        let dirty = self.is_settings_dirty(cx);
        let asr_ready = check_model_dir(&self.settings.asr_model_dir).is_ok();
        let align_ready =
            oneasr_core::check_aligner_model_dir(&self.settings.aligner_model_dir).is_ok();
        let asr_dl = self.asr_download.clone();
        let align_dl = self.align_download.clone();
        let asr_dl_busy = asr_dl
            .as_ref()
            .is_some_and(|p| p.state == DownloadState::Downloading);
        let align_dl_busy = align_dl
            .as_ref()
            .is_some_and(|p| p.state == DownloadState::Downloading);

        let section = |body: gpui::AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap_1p5()
                .rounded_lg()
                .border_1()
                .border_color(LINE_SOFT)
                .bg(BG)
                .px_3()
                .py_2p5()
                .child(body)
        };

        // Layout: title | scrollable body | pin footer (save always visible).
        div()
            .w(px(SETTINGS_W))
            .h_full()
            .bg(PANEL)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(TEXT)
                            .child("设置"),
                    )
                    .when(dirty, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(WARN)
                                .child("未保存"),
                        )
                    }),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .px_4()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(section(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child("ASR 模型"),
                                    )
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(if asr_ready { ACCENT } else { DANGER }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("model-dir")
                                    .flex()
                                    .items_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(LINE)
                                    .bg(PANEL)
                                    .overflow_hidden()
                                    .hover(|s| s.border_color(ACCENT))
                                    .child(
                                        div()
                                            .id("model-dir-path")
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(TEXT)
                                            .whitespace_normal()
                                            .line_clamp(2)
                                            .child(model)
                                            .tooltip(move |_, cx| {
                                                cx.new(|_| NameTooltip {
                                                    text: model_tip.clone().into(),
                                                })
                                                .into()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("model-dir-browse")
                                            .flex_shrink_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .border_l_1()
                                            .border_color(LINE_SOFT)
                                            .cursor_pointer()
                                            .hover(|s| s.bg(ACCENT_SOFT))
                                            .child(
                                                svg()
                                                    .size(px(15.))
                                                    .path("icons/folder.svg")
                                                    .text_color(MUTED),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pick_model_dir(cx)
                                            })),
                                    ),
                            )
                            .child(model_download_row(
                                "asr-dl-btn",
                                "asr-dl-cancel",
                                asr_ready,
                                asr_dl_busy,
                                asr_dl.as_ref(),
                                cx.listener(|this, _, _, cx| {
                                    this.start_model_download(ModelId::Qwen3Asr06B, cx);
                                }),
                                cx.listener(|this, _, _, cx| {
                                    this.cancel_model_download(ModelId::Qwen3Asr06B, cx);
                                }),
                            ))
                            .into_any_element(),
                    ))
                    .child(section(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child("Aligner 模型"),
                                    )
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(if align_ready { ACCENT } else { DANGER }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("aligner-dir")
                                    .flex()
                                    .items_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(LINE)
                                    .bg(PANEL)
                                    .overflow_hidden()
                                    .hover(|s| s.border_color(ACCENT))
                                    .child(
                                        div()
                                            .id("aligner-dir-path")
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(TEXT)
                                            .whitespace_normal()
                                            .line_clamp(2)
                                            .child(aligner)
                                            .tooltip(move |_, cx| {
                                                cx.new(|_| NameTooltip {
                                                    text: aligner_tip.clone().into(),
                                                })
                                                .into()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("aligner-dir-browse")
                                            .flex_shrink_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .border_l_1()
                                            .border_color(LINE_SOFT)
                                            .cursor_pointer()
                                            .hover(|s| s.bg(ACCENT_SOFT))
                                            .child(
                                                svg()
                                                    .size(px(15.))
                                                    .path("icons/folder.svg")
                                                    .text_color(MUTED),
                                            )
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.pick_aligner_dir(cx)
                                            })),
                                    ),
                            )
                            .child(model_download_row(
                                "align-dl-btn",
                                "align-dl-cancel",
                                align_ready,
                                align_dl_busy,
                                align_dl.as_ref(),
                                cx.listener(|this, _, _, cx| {
                                    this.start_model_download(ModelId::QwenAlign06B, cx);
                                }),
                                cx.listener(|this, _, _, cx| {
                                    this.cancel_model_download(ModelId::QwenAlign06B, cx);
                                }),
                            ))
                            .into_any_element(),
                    ))
                    .child(section(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("推理后端"),
                            )
                            .child(
                                div().flex().gap_1p5().children(
                                    [
                                        ("auto", "自动"),
                                        ("cuda", "GPU"),
                                        ("cpu", "CPU"),
                                    ]
                                    .into_iter()
                                    .map(|(id, label)| {
                                        let active = backend == id;
                                        btn(
                                            label,
                                            if active {
                                                BtnKind::Primary
                                            } else {
                                                BtnKind::Secondary
                                            },
                                            true,
                                            cx.listener(move |this, _, _, cx| {
                                                if this.settings.backend != id {
                                                    this.settings.backend = id.into();
                                                    unload_session();
                                                    this.refresh_model_probe();
                                                    this.mark_settings_dirty(cx);
                                                }
                                            }),
                                        )
                                    }),
                                ),
                            )
                            .into_any_element(),
                    )),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(LINE_SOFT)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED_SOFT)
                            .child(if dirty {
                                "关闭面板时将自动保存"
                            } else {
                                "所有更改已保存"
                            }),
                    )
                    .child(
                        div().flex_shrink_0().child(btn(
                            if dirty { "保存设置" } else { "已保存" },
                            if dirty {
                                BtnKind::Primary
                            } else {
                                BtnKind::Secondary
                            },
                            dirty,
                            cx.listener(|this, _, _, cx| this.save_settings(cx)),
                        )),
                    ),
            )
    }

    fn render_status_bar(&self) -> impl IntoElement {
        // Exclude exit tombstones so counts do not include rows mid-fade.
        let live: Vec<&Task> = self
            .tasks
            .iter()
            .filter(|t| !self.exiting.contains_key(&t.id))
            .collect();
        let total = live.len();
        let pending = live
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();
        let queued = live
            .iter()
            .filter(|t| t.status == TaskStatus::Queued)
            .count();
        let done = live
            .iter()
            .filter(|t| t.status == TaskStatus::Done)
            .count();
        let err = live
            .iter()
            .filter(|t| t.status == TaskStatus::Error)
            .count();
        let proc = live
            .iter()
            .filter(|t| t.status == TaskStatus::Processing)
            .count();
        let queue = format_queue_status(total, pending, queued, proc, done, err);
        let batch = self.batch_mode;
        // Sequential batch: one line only — 进度 n/m (no "共 n · 处理中" echo).
        let left: SharedString = if batch {
            self.batch_goal
                .map(|g| format_batch_progress(self.batch_done, g))
                .unwrap_or_else(|| "进度 …".into())
                .into()
        } else {
            queue.into()
        };
        let model = self.model_status;
        let model_color = match model {
            ModelStatus::Ready => ACCENT,
            ModelStatus::NotReady => DANGER,
        };
        let hint = self.status_hint.clone();

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
            .child(
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
                    ),
            )
            // Right: model probe (常驻) OR transient interaction hint.
            .child(
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
                            .bg(WARN_SOFT)
                            .px_2()
                            .py_0p5()
                            .rounded_full()
                            .child(div().size(px(7.)).rounded_full().bg(WARN))
                            .child(
                                div()
                                    .text_color(WARN)
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
                    }),
            )
    }
}

/// Compact download row under a model path field.
fn model_download_row(
    id: &'static str,
    cancel_id: &'static str,
    ready: bool,
    busy: bool,
    progress: Option<&DownloadProgress>,
    on_download: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let dest_hint = progress
        .map(|p| p.model_dir.display().to_string())
        .unwrap_or_else(|| "安装目录 models/".into());
    let status_text = if let Some(p) = progress {
        if p.state == DownloadState::Downloading {
            format!("{} · {}", p.label(), dest_hint)
        } else {
            p.label()
        }
    } else if ready {
        "已就绪".to_string()
    } else {
        format!("未下载 · 将保存到 {dest_hint}")
    };
    let status_color = if ready && !busy {
        ACCENT
    } else if busy {
        WARN
    } else if progress.is_some_and(|p| p.state == DownloadState::Failed) {
        DANGER
    } else {
        MUTED
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(status_color)
                .whitespace_normal()
                .line_clamp(2)
                .child(status_text),
        )
        .child(
            div().flex().items_center().gap_1p5().children({
                let mut kids: Vec<gpui::AnyElement> = Vec::new();
                if busy {
                    kids.push(
                        div()
                            .id(cancel_id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(LINE)
                            .bg(BG)
                            .text_xs()
                            .text_color(MUTED)
                            .cursor_pointer()
                            .hover(|s| s.bg(DANGER_SOFT).text_color(DANGER).border_color(DANGER))
                            .on_click(on_cancel)
                            .child("取消")
                            .into_any_element(),
                    );
                    kids.push(
                        div()
                            .id(id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(LINE)
                            .bg(BG)
                            .text_xs()
                            .text_color(MUTED)
                            .opacity(0.7)
                            .child("下载中…")
                            .into_any_element(),
                    );
                } else {
                    let label = if ready { "重新下载" } else { "下载模型" };
                    kids.push(
                        div()
                            .id(id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(ACCENT)
                            .bg(ACCENT_SOFT)
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(ACCENT)
                            .cursor_pointer()
                            .hover(|s| s.bg(ACCENT).text_color(gpui::rgb(0xffffff)))
                            .on_click(on_download)
                            .child(label)
                            .into_any_element(),
                    );
                }
                kids
            }),
        )
}

/// Brand mark: teal tile + SVG “waveform → subtitle lines”.
fn app_logo() -> impl IntoElement {
    div()
        .size(px(34.))
        .rounded_xl()
        .bg(LOGO)
        .shadow(vec![BoxShadow {
            color: hsla(174. / 360., 0.55, 0.28, 0.22),
            offset: point(px(0.), px(1.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
        }])
        .flex()
        .items_center()
        .justify_center()
        .child(
            svg()
                .size(px(20.))
                .path("icons/logo.svg")
                .text_color(gpui::rgb(0xffffff)),
        )
}

#[derive(Clone, Copy)]
enum BtnKind {
    /// Sole solid CTA (全部开始 / 空状态添加).
    Primary,
    /// Outlined secondary (添加 / 后端选项).
    Secondary,
    /// Low-emphasis destructive/utility (清空) — text only.
    Quiet,
}

/// Fixed-size settings gear. Spins slowly while the drawer is open; stops when closed.
fn settings_gear_btn(
    open: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let gear = svg()
        .size(px(18.))
        .path("icons/gear.svg")
        .text_color(if open { ACCENT } else { MUTED });

    let gear_el = if open {
        // Slow continuous spin while panel is open (~4s per turn).
        gear.with_animation(
            "settings-gear-spin",
            Animation::new(Duration::from_secs(4))
                .repeat()
                .with_easing(linear),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
    } else {
        gear.into_any_element()
    };

    div()
        .id("settings-gear")
        .size(px(36.))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open { ACCENT_SOFT } else { PANEL })
        .border_1()
        .border_color(if open { ACCENT_SOFT } else { LINE })
        .cursor_pointer()
        .hover(|s| {
            if open {
                s.bg(ACCENT_SOFT).border_color(ACCENT)
            } else {
                s.bg(BG).border_color(ACCENT)
            }
        })
        .on_click(on_click)
        .child(gear_el)
}

/// Primary CTA. When disabled: not clickable + tooltip explains why.
fn btn_cta(
    label: &str,
    enabled: bool,
    disabled_tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("cta-{label}-{enabled}"));
    let tip: SharedString = disabled_tip.into();
    let mut el = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(ACCENT)
        .text_color(PANEL)
        .border_1()
        .border_color(ACCENT)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .opacity(if enabled { 1.0 } else { 0.42 })
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| s.opacity(0.92))
            .on_click(on_click);
    } else {
        el = el.tooltip(move |_, cx| {
            cx.new(|_| NameTooltip { text: tip.clone() }).into()
        });
    }
    el
}

fn btn(
    label: &str,
    kind: BtnKind,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("btn-{label}-{}-{enabled}", kind as u8));
    let (bg, fg, border, opacity) = match (kind, enabled) {
        (BtnKind::Primary, true) => (ACCENT, PANEL, ACCENT, 1.0),
        (BtnKind::Primary, false) => (ACCENT, PANEL, ACCENT, 0.42),
        (BtnKind::Secondary, true) => (PANEL, TEXT, LINE, 1.0),
        (BtnKind::Secondary, false) => (PANEL, MUTED_SOFT, LINE, 1.0),
        (BtnKind::Quiet, true) => (PANEL, MUTED, PANEL, 1.0),
        (BtnKind::Quiet, false) => (PANEL, MUTED_SOFT, PANEL, 1.0),
    };
    let mut el = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(bg)
        .text_color(fg)
        .border_1()
        .border_color(border)
        .font_weight(match kind {
            BtnKind::Primary => gpui::FontWeight::SEMIBOLD,
            BtnKind::Secondary | BtnKind::Quiet => gpui::FontWeight::NORMAL,
        })
        .opacity(opacity)
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                BtnKind::Primary => s.opacity(0.92),
                BtnKind::Secondary => s.border_color(ACCENT),
                BtnKind::Quiet => s.text_color(DANGER).bg(DANGER_SOFT),
            })
            .on_click(on_click);
    }
    el
}

fn is_video_format(fmt: &str) -> bool {
    matches!(
        fmt.to_ascii_lowercase().as_str(),
        "mp4" | "mkv" | "mov" | "webm" | "avi" | "flv" | "ts" | "m4v" | "mpeg" | "mpg" | "wmv"
            | "3gp"
    )
}

/// Status accent for media plate border (replaces free-floating status circle).
fn status_border_color(status: TaskStatus) -> gpui::Rgba {
    match status {
        TaskStatus::Pending => LINE,
        TaskStatus::Queued => MUTED,
        TaskStatus::Processing => WARN,
        TaskStatus::Done => ACCENT,
        TaskStatus::Error => DANGER,
    }
}

/// Status pill: (label, foreground, soft background).
fn status_pill_style(
    status: TaskStatus,
    _phase: u8,
    queue_n: Option<usize>,
    stage: Option<&str>,
) -> (String, gpui::Rgba, gpui::Rgba) {
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
fn row_accent(status: TaskStatus) -> gpui::Rgba {
    match status {
        TaskStatus::Processing => WARN,
        TaskStatus::Error => DANGER,
        TaskStatus::Done => ACCENT,
        TaskStatus::Pending | TaskStatus::Queued => LINE_SOFT,
    }
}

/// Compact SVG media mark (video clapper / audio speaker); border = status tint.
fn media_type_icon(is_video: bool, status: TaskStatus) -> impl IntoElement {
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

/// Decorative full-width empty-state strip (not real audio FFT).
/// Idle = flat baseline across the content width. Pointer locally bulges a
/// soft waveform under the cursor; the bulge eases in/out and follows smoothly.
const EMPTY_WAVE_BARS: usize = 64;
const EMPTY_WAVE_MAX_H: f32 = 78.0;
const EMPTY_WAVE_FLAT_H: f32 = 5.0;
/// How wide the local bulge is (fraction of strip width, ~gaussian sigma).
const EMPTY_WAVE_SIGMA: f32 = 0.09;

impl OneAsrApp {
    fn render_empty_wave(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.empty_wave_clock.elapsed().as_secs_f32();
        let heights = empty_wave_heights(
            t,
            self.empty_wave_smooth_x,
            self.empty_wave_amp,
        );
        let entity = cx.entity().clone();

        div()
            .id("empty-wave")
            .w_full()
            .h(px(EMPTY_WAVE_MAX_H + 24.0))
            .relative()
            .cursor_default()
            // Capture layout bounds so mouse X can be mapped 0..=1 along the strip.
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, _window, cx| {
                            entity.update(cx, |app, _cx| {
                                app.empty_wave_bounds = Some(bounds);
                            });
                        }
                    },
                    |_bounds, (), _window, _cx| {},
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(b) = this.empty_wave_bounds {
                    let w = f32::from(b.size.width).max(1.0);
                    let local = (f32::from(event.position.x) - f32::from(b.left())) / w;
                    this.empty_wave_cursor_x = local.clamp(0.0, 1.0);
                    if !this.empty_wave_hover {
                        this.empty_wave_hover = true;
                    }
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.empty_wave_hover != *hovered {
                    this.empty_wave_hover = *hovered;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px_6()
                    .flex()
                    .items_center()
                    .justify_between()
                    .children((0..EMPTY_WAVE_BARS).map(move |i| {
                        let h = heights[i];
                        let u = ((h - EMPTY_WAVE_FLAT_H)
                            / (EMPTY_WAVE_MAX_H - EMPTY_WAVE_FLAT_H))
                            .clamp(0.0, 1.0);
                        let opacity = 0.40 + 0.55 * u;
                        div()
                            .w(px(2.5))
                            .h(px(h))
                            .rounded_full()
                            .bg(ACCENT)
                            .opacity(opacity)
                    })),
            )
    }
}

/// Bar heights in px. Flat when `amp≈0`; local gaussian bulge at `cx` (0..=1) otherwise.
fn empty_wave_heights(t_secs: f32, cx: f32, amp: f32) -> [f32; EMPTY_WAVE_BARS] {
    let mut out = [EMPTY_WAVE_FLAT_H; EMPTY_WAVE_BARS];
    if amp < 0.004 {
        return out;
    }
    let n = (EMPTY_WAVE_BARS - 1) as f32;
    let sigma2 = 2.0 * EMPTY_WAVE_SIGMA * EMPTY_WAVE_SIGMA;
    for i in 0..EMPTY_WAVE_BARS {
        let x = i as f32 / n;
        let dx = x - cx;
        let env = (-(dx * dx) / sigma2).exp();
        // Gentle shimmer only under the bulge (not whole-line jitter).
        let ripple = (x * TAU * 5.0 - t_secs * 5.5).sin() * 0.18;
        let fine = (x * TAU * 11.0 + t_secs * 3.2).sin() * 0.07;
        let u = (env * (0.88 + ripple + fine)).clamp(0.0, 1.0) * amp;
        out[i] = EMPTY_WAVE_FLAT_H + u * (EMPTY_WAVE_MAX_H - EMPTY_WAVE_FLAT_H);
    }
    out
}

/// Lightweight hover tooltip for truncated filenames.
struct NameTooltip {
    text: SharedString,
}

impl Render for NameTooltip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(TEXT)
            .text_color(PANEL)
            .text_xs()
            .max_w(px(420.))
            .child(self.text.clone())
    }
}

#[derive(Clone, Copy)]
enum IconKind {
    Play,
    Trash,
    /// Open containing folder for completed SRT.
    Folder,
}

/// Compact icon action — SVG only (no emoji). Always visible; disabled = gray.
///
/// Note: GPUI SVG needs an explicit `.text_color()` (currentColor); parent
/// cascade alone often leaves stroke/fill invisible.
fn icon_btn(
    kind: IconKind,
    tip: &'static str,
    enabled: bool,
    row_hovered: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("ico-{tip}-{enabled}-{}-{row_hovered}", kind as u8));
    let tip_s: SharedString = tip.into();
    let (fg, bg, border) = if !enabled {
        (MUTED_SOFT, BG, LINE_SOFT)
    } else if row_hovered {
        match kind {
            IconKind::Play => (ACCENT, ACCENT_SOFT, ACCENT_SOFT),
            IconKind::Trash => (MUTED, PANEL, LINE),
            IconKind::Folder => (ACCENT, ACCENT_SOFT, ACCENT_SOFT),
        }
    } else {
        match kind {
            IconKind::Folder => (ACCENT, ACCENT_SOFT, ACCENT_SOFT),
            IconKind::Play => (MUTED, PANEL, LINE),
            IconKind::Trash => (MUTED, PANEL, LINE),
        }
    };
    let mut el = div()
        .id(id)
        .size(px(32.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_color(fg)
        .opacity(if enabled { 1.0 } else { 0.55 })
        .child(
            svg()
                .size(px(17.))
                .path(icon_svg_path(kind))
                .text_color(fg),
        )
        .tooltip(move |_, cx| {
            cx.new(|_| NameTooltip {
                text: tip_s.clone(),
            })
            .into()
        });
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                IconKind::Play => s.bg(ACCENT_SOFT).border_color(ACCENT),
                IconKind::Trash => s.bg(DANGER_SOFT).border_color(DANGER),
                // Keep the bg light so the ACCENT icon stays readable (unlike a
                // solid-ACCENT fill, which would eat the icon of the same color).
                IconKind::Folder => s.bg(ACCENT_SOFT).border_color(ACCENT),
            })
            .on_click(on_click);
    }
    el
}

fn icon_svg_path(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Play => "icons/play.svg",
        IconKind::Trash => "icons/trash.svg",
        IconKind::Folder => "icons/folder.svg",
    }
}
