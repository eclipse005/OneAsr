//! OneAsr — light-themed batch list for local Qwen ASR + align.

// Release / portable: GUI PE subsystem (no black console on double-click).
// Debug: keep the CONSOLE subsystem so `cargo run` logs work without stdio hacks.
// Release is additionally forced in `build.rs` + verified by `pack-release.ps1`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod assets;
mod crashlog;
mod sfx;
mod shell;
mod theme;
mod ui_font;
mod widgets;

use crate::widgets::{
    floating_lang_menu, timing_breakdown_popover, app_logo, btn, btn_cta, caption_btn,
    component_install_row, icon_btn, model_download_row, pill, popover_dismiss_layer,
    popover_menu_shadow, settings_gear_btn, BtnKind, IconKind, NameTooltip,
};

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    actions, canvas, deferred, div, hsla, point, prelude::*, px, size, svg,
    App, Application, Bounds, BoxShadow, Context,
    ExternalPaths, FocusHandle, KeyBinding, MouseMoveEvent, Pixels, Rgba, SharedString,
    Timer, Window, WindowBounds, WindowControlArea, WindowOptions,
};
use oneasr_core::{
    accept_input_path, check_asr_model_dir, demote_current_thread, download_model,
    empty_state_subtitle, empty_state_title, ffmpeg_present, format_batch_progress,
    format_queue_status, format_process_ms, init_native_library_path, init_runtime,
    is_cuda_runtime_ready, is_model_ready, next_queue_seq, normalize_source_language,
    process_media_file_with_progress, probe_duration_async, probe_writable, resolve_app_root,
    resolve_app_root_dir, resolve_cuda_runtime_dir, source_language_by_id,
    stats::{self, StatsRecord, StatsSummary},
    AsrStage,
    DownloadHandle, DownloadProgress, DownloadState, DurationState, ModelId, ModelKind,
    Settings, StageClock, StageUpdate, Task, TaskStatus, TaskTiming, CHUNK_TARGET_MAX_SEC,
    CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_PRESETS, SOURCE_LANGUAGES, TextScript,
};

actions!(oneasr, [DismissMenus]);

use theme::{
    ACCENT, ACCENT_MIST, ACCENT_SOFT, BG, DANGER, DANGER_SOFT, LINE, LINE_SOFT, LOGO, MEDIA_PLATE,
    MUTED, MUTED_SOFT, PANEL, ROW_HOVER, STATS_GHOST, STATS_L0, STATS_L1, STATS_L2, STATS_L3,
    STATS_L4, TEXT, WARN, WARN_SOFT, ZEBRA,
};

/// Version shown in the titlebar, e.g. `v0.1.9`.
///
/// Single source is the Cargo package version (same one `crashlog` reports at
/// startup). Never hand-write a version string here — it will drift from the
/// build that actually shipped.
const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));

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
enum AsrJob {
    Run {
        id: String,
        path: PathBuf,
        name: String,
        settings: Settings,
    },
}

fn main() {
    // Native DLL dir must be registered before any worker threads exist.
    // (`SetDllDirectoryW` only — no PATH mutation; see oneasr_core::model::path.)
    init_native_library_path();
    // Cap rayon before any model load so the UI thread keeps a free core.
    init_runtime();
    // Install-dir error log + panic capture (see `oneasr-error.log`).
    crashlog::install_panic_hook();
    crashlog::log_session_start();

    Application::new()
        .with_assets(assets::AppAssets::new())
        .run(|cx: &mut App| {
            cx.bind_keys([KeyBinding::new("escape", DismissMenus, None)]);
            // Resolve UI font against this machine (CJK fallbacks for stripped Win10).
            let font_plan = ui_font::resolve(cx);
            crashlog::log_font_plan(&ui_font::diagnose_text(&font_plan));
            if !font_plan
                .installed_hits
                .iter()
                .any(|h| h.contains("YaHei") || h.contains("雅黑"))
            {
                crashlog::log_warn(
                    "Microsoft YaHei missing — UI uses alternate CJK fallbacks (see font plan)",
                );
            }

            // Compact default: list + toolbar, not a full-HD empty canvas.
            let bounds = Bounds::centered(None, size(px(860.), px(560.)), cx);
            // Custom-drawn title bar (`appears_transparent`): GPUI never sets WS_CAPTION,
            // so the native caption is only a DWM fallback — broken on some Win10 machines
            // (no drag / min / max / close). We draw our own and route hits through
            // `WindowControlArea`, which works regardless of DWM state. The `title` string
            // still feeds the taskbar / Alt-Tab label.
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("OneAsr".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                {
                    let font_plan = font_plan.clone();
                    move |window, cx| {
                        cx.new(|cx| {
                            let mut app = OneAsrApp::new(cx);
                            app.ui_font = font_plan;
                            app.focus_handle.focus(window);
                            app
                        })
                    }
                },
            )
            .expect("open window");
            cx.activate(true);
        });
}

struct OneAsrApp {
    /// Root focus so Escape / key bindings reach the app (menus, dismiss).
    focus_handle: FocusHandle,
    /// Machine-resolved UI font (primary + CJK DirectWrite fallbacks).
    ui_font: ui_font::UiFontPlan,
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
    /// Task id whose processing-time popover is open (hover).
    timing_popover: Option<String>,
    /// Hover candidate for open delay (id + first-hover Instant).
    timing_hover_since: Option<(String, Instant)>,
    /// Leave grace: close only if still left after TIMING_LEAVE_DELAY_MS.
    timing_leave_since: Option<(String, Instant)>,
    /// Popover open animation: progress source → target (0 hidden, 1 shown).
    timing_pop_from: f32,
    timing_pop_to: f32,
    timing_pop_t0: Instant,
    /// Task id whose language dropdown is open (`None` = closed).
    lang_menu: Option<String>,
    /// Settings panel: default-language dropdown open.
    settings_lang_open: bool,
    /// Empty-state wave: pointer currently over the strip.
    empty_wave_hover: bool,
    /// Brand logo: pointer currently over the mark (wiggle animation).
    logo_hover: bool,
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
    /// Finished (ok or err) count within the current batch.
    batch_done: usize,
    /// Outcomes of the current run, counted as each row finishes. The run's
    /// own tally, never the list's: rows left over from an earlier run must not
    /// colour today's chime, and rows deleted mid-run must not colour it either.
    batch_ok: usize,
    batch_err: usize,
    busy: bool,
    /// One-shot: the ASR worker channel was found dead (log/recover only once).
    worker_channel_dead: bool,
    picking: bool,
    model_status: ModelStatus,
    /// Cached settings-panel readiness (refreshed in [`Self::refresh_model_probe`], not per frame).
    asr_ready: bool,
    align_ready: bool,
    cuda_ready: bool,
    /// Optional HTDemucs weights present (vocal separation).
    demucs_ready: bool,
    /// Soft status-bar hint (no toast). Auto-clears after a few seconds.
    status_hint: Option<SharedString>,
    status_hint_until: Option<Instant>,
    /// The live hint is a success notice (accent) rather than a notice-to-act
    /// (amber). Only the run-complete family sets it: warnings keep the default.
    status_hint_good: bool,
    /// Saved time of the first task ever finished, handed to the run-complete
    /// notice so the one moment the number is news does not pass unread.
    nudge_saved: Option<SharedString>,
    /// Live stage label for the row currently Processing (from worker Progress).
    active_stage: Option<(String, SharedString)>,
    /// Latest download progress (settings panel).
    asr_download: Option<DownloadProgress>,
    align_download: Option<DownloadProgress>,
    cuda_download: Option<DownloadProgress>,
    demucs_download: Option<DownloadProgress>,
    /// Active download cancel handles.
    asr_dl_handle: Option<DownloadHandle>,
    align_dl_handle: Option<DownloadHandle>,
    cuda_dl_handle: Option<DownloadHandle>,
    demucs_dl_handle: Option<DownloadHandle>,
    /// Stats panel open (floats above the status bar; shares MENU_Z with menus).
    stats_open: bool,
    /// Day cell under the pointer in the year grid, `YYYY-MM-DD`.
    stats_hover_day: Option<String>,
    /// Cached ledger aggregation. Recomputed on load and after every finished
    /// task — **never per frame**, since the panel repaints on each cell hover.
    stats: StatsSummary,
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
                            let mut clock = StageClock::new();
                            // A panic inside the pipeline must not kill the shared
                            // worker (that would strand `Processing` rows forever).
                            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                                || {
                                    run_task(
                                        &path,
                                        &name,
                                        &settings,
                                        |update| {
                                            clock.note(&update);
                                            let warning = update
                                                .warning
                                                .as_ref()
                                                .map(SharedString::from);
                                            let _ = ptx.send(WorkerMsg::Progress {
                                                id: id_for_progress.clone(),
                                                stage: SharedString::from(update.label()),
                                                warning,
                                            });
                                        },
                                    )
                                },
                            ))
                            .unwrap_or_else(|payload| {
                                let message = panic_message(payload);
                                crashlog::log_error(format!(
                                    "ASR worker panic (task {id}): {message}"
                                ));
                                Err(format!("处理线程异常: {message}"))
                            });
                            let timing = clock.finish();
                            let _ = worker_tx.send(WorkerMsg::Finished {
                                id,
                                result,
                                timing,
                            });
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

        let (mut settings, settings_report) = Settings::load_with_report();
        // Force-GPU without runtime DLLs is invalid → fall back to auto and persist.
        let mut cuda_fallback_hint: Option<String> = None;
        if settings.backend.eq_ignore_ascii_case("cuda") && !is_cuda_runtime_ready() {
            crashlog::log_warn(
                "backend forced cuda but CUDA runtime missing — reset to auto",
            );
            settings.backend = "auto".into();
            if let Err(e) = settings.save() {
                crashlog::log_error(format!("cuda→auto fallback save failed: {e}"));
            }
            cuda_fallback_hint =
                Some("未检测到 CUDA 运行库，已改用自动（可在设置中安装组件后选 GPU）".into());
        }
        // Log why settings were repaired / fell back to defaults (user report:
        // "设置丢了" must be answerable from a pasted log alone).
        if settings_report.is_notable() {
            let path = settings_report
                .config_path
                .clone()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".into());
            if let Some(e) = &settings_report.read_error {
                crashlog::log_error(format!(
                    "settings read failed (using defaults)\n  path: {path}\n  error: {e}"
                ));
            }
            if let Some(e) = &settings_report.parse_error {
                crashlog::log_error(format!(
                    "settings parse failed (using defaults)\n  path: {path}\n  error: {e}"
                ));
            }
            for repair in &settings_report.repairs {
                crashlog::log_warn(format!("settings repaired: {repair}"));
            }
        }

        // Environment snapshot: everything support asks for in one block —
        // install location + writability (os error 5 class), ffmpeg, backend,
        // per-model readiness. Probe files only, never load weights.
        let app_root = resolve_app_root_dir();
        let writable = match probe_writable(&app_root) {
            Ok(()) => "yes".to_string(),
            Err(e) => format!("NO ({e})"),
        };
        let models_line = [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::QwenAlign06B,
            ModelId::HtdemucsFt,
            ModelId::CudaRuntime,
        ]
        .map(|id| {
            format!(
                "{}={}",
                id.as_str(),
                if is_model_ready(id) { "ready" } else { "missing" }
            )
        })
        .join(" ");
        crashlog::log_info(format!(
            "environment:\n  settings: {}\n  app_root: {}\n  app_root writable: {writable}\n  ffmpeg: {}\n  backend: {}\n  output_dir: {}\n  cuda dir: {}\n  models: {models_line}",
            Settings::config_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<unknown>".into()),
            app_root.display(),
            if ffmpeg_present() { "yes" } else { "MISSING" },
            settings.backend,
            settings.output_dir.display(),
            resolve_cuda_runtime_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "<none>".into()),
        ));

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            ui_font: ui_font::UiFontPlan::default(),
            settings,
            settings_dirty: false,
            settings_open: false,
            settings_from: 0.0,
            settings_to: 0.0,
            settings_anim_t0: Instant::now(),
            hover_row: None,
            timing_popover: None,
            timing_hover_since: None,
            timing_leave_since: None,
            timing_pop_from: 0.0,
            timing_pop_to: 0.0,
            timing_pop_t0: Instant::now(),
            lang_menu: None,
            settings_lang_open: false,
            empty_wave_hover: false,
            logo_hover: false,
            empty_wave_bounds: None,
            empty_wave_cursor_x: 0.5,
            empty_wave_smooth_x: 0.5,
            empty_wave_amp: 0.0,
            empty_wave_clock: Instant::now(),
            tasks: Vec::new(),
            entering: HashMap::new(),
            exiting: HashMap::new(),
            batch_mode: false,
            batch_done: 0,
            batch_ok: 0,
            batch_err: 0,
            busy: false,
            worker_channel_dead: false,
            picking: false,
            model_status: ModelStatus::NotReady,
            asr_ready: false,
            align_ready: false,
            cuda_ready: false,
            demucs_ready: false,
            status_hint: None,
            status_hint_until: None,
            status_hint_good: false,
            nudge_saved: None,
            active_stage: None,
            asr_download: None,
            align_download: None,
            cuda_download: None,
            demucs_download: None,
            asr_dl_handle: None,
            align_dl_handle: None,
            cuda_dl_handle: None,
            demucs_dl_handle: None,
            stats_open: false,
            stats_hover_day: None,
            // Read the ledger once at startup; refreshed on every finished task.
            stats: stats::summarize(&stats::load(&app_root)),
            tx: tx.clone(),
            rx,
            job_tx,
        };

        // Probe files only — never load multi-GB weights on startup.
        app.refresh_model_probe();
        if let Some(hint) = cuda_fallback_hint {
            app.status_hint = Some(SharedString::from(hint));
            app.status_hint_until = Some(Instant::now() + Duration::from_secs(5));
        }
        app
    }

    /// Start ModelScope download into `{app}/models/...` (background).
    /// Does **not** rewrite settings until `DownloadOutcome::Completed`.
    fn start_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        // One job per kind — 0.6B / 1.7B share the ASR slot.
        if self.download_kind_busy(id.kind()) {
            self.flash_hint("已有同类下载任务进行中", cx);
            return;
        }

        let handle = DownloadHandle::new(id);
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
            ModelKind::CudaRuntime => self.cuda_dl_handle = Some(handle.clone()),
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

    fn cancel_model_download(&mut self, id: ModelId, cx: &mut Context<Self>) {
        let handle = match id.kind() {
            ModelKind::Asr => self.asr_dl_handle.as_ref(),
            ModelKind::Align => self.align_dl_handle.as_ref(),
            ModelKind::CudaRuntime => self.cuda_dl_handle.as_ref(),
            ModelKind::Demucs => self.demucs_dl_handle.as_ref(),
        };
        if let Some(h) = handle {
            if h.model_id == id {
                h.cancel();
                self.flash_hint("正在取消下载…", cx);
            }
        }
        cx.notify();
    }

    /// True while **this** model id is downloading (UI busy / cancel for that row).
    fn download_busy(&self, id: ModelId) -> bool {
        let handle_match = match id.kind() {
            ModelKind::Asr => self.asr_dl_handle.as_ref().is_some_and(|h| h.model_id == id),
            ModelKind::Align => self
                .align_dl_handle
                .as_ref()
                .is_some_and(|h| h.model_id == id),
            ModelKind::CudaRuntime => self
                .cuda_dl_handle
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
    fn download_kind_busy(&self, kind: ModelKind) -> bool {
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
            ModelKind::CudaRuntime => {
                self.cuda_dl_handle.is_some()
                    || self
                        .cuda_download
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

    /// Any model / CUDA component download in flight (drives settings gear spin).
    fn any_download_busy(&self) -> bool {
        self.download_kind_busy(ModelKind::Asr)
            || self.download_kind_busy(ModelKind::Align)
            || self.download_kind_busy(ModelKind::CudaRuntime)
            || self.download_kind_busy(ModelKind::Demucs)
    }

    /// Progress snapshot for this exact model id (never another ASR size).
    fn progress_for(&self, id: ModelId) -> Option<&DownloadProgress> {
        let p = match id.kind() {
            ModelKind::Asr => self.asr_download.as_ref(),
            ModelKind::Align => self.align_download.as_ref(),
            ModelKind::CudaRuntime => self.cuda_download.as_ref(),
            ModelKind::Demucs => self.demucs_download.as_ref(),
        }?;
        if p.model_id == id {
            Some(p)
        } else {
            None
        }
    }

    fn set_download_progress(&mut self, progress: DownloadProgress) {
        match progress.model_id.kind() {
            ModelKind::Asr => self.asr_download = Some(progress),
            ModelKind::Align => self.align_download = Some(progress),
            ModelKind::CudaRuntime => self.cuda_download = Some(progress),
            ModelKind::Demucs => self.demucs_download = Some(progress),
        }
    }

    fn clear_download_handle(&mut self, id: ModelId) {
        match id.kind() {
            ModelKind::Asr => self.asr_dl_handle = None,
            ModelKind::Align => self.align_dl_handle = None,
            ModelKind::CudaRuntime => self.cuda_dl_handle = None,
            ModelKind::Demucs => self.demucs_dl_handle = None,
        }
    }

    /// Drop progress snapshot when it no longer belongs to the visible selection.
    fn clear_stale_asr_progress(&mut self) {
        if let Some(p) = &self.asr_download {
            if p.model_id != self.settings.selected_asr_id()
                && p.state != DownloadState::Downloading
            {
                self.asr_download = None;
            }
        }
    }

    /// Fast FS check for status bar + settings dots. Does not touch GPU / weights.
    /// Call after path / download / backend changes — not on every scroll paint.
    fn refresh_model_probe(&mut self) {
        self.asr_ready = check_asr_model_dir(&self.settings.asr_model_dir).is_ok();
        self.align_ready =
            oneasr_core::check_aligner_model_dir(&self.settings.aligner_model_dir).is_ok();
        self.cuda_ready = is_cuda_runtime_ready();
        self.demucs_ready =
            oneasr_core::check_demucs_model_dir(&self.settings.resolved_demucs_model_dir())
                .is_ok();
        self.model_status = if self.settings.can_start().is_ok() {
            ModelStatus::Ready
        } else {
            ModelStatus::NotReady
        };
    }

    /// Re-probe the configured model folders after a path change.
    fn reset_model_config(&mut self, cx: &mut Context<Self>) {
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

    /// The one sound gate (「提示音」): interaction taps AND the run reminder
    /// together.
    ///
    /// Never attach this to hover: it fires tens of times a second and is the
    /// fastest way to make an app feel noisy.
    fn play_ui(&self, kind: sfx::Sfx) {
        if self.settings.sound {
            sfx::play(kind);
        }
    }

    /// Ledger root — the app folder, next to `settings.json`.
    fn stats_root() -> PathBuf {
        resolve_app_root_dir()
    }

    /// Re-aggregate the ledger into the cached summary.
    ///
    /// Called at startup, on panel open, and after each finished task — **never
    /// per frame**: the panel repaints on every cell hover, and folding the
    /// whole ledger per paint is exactly the mistake the settings panel's probe
    /// cache exists to avoid.
    fn reload_stats(&mut self) {
        self.stats = stats::summarize(&stats::load(&Self::stats_root()));
    }

    /// Append one finished task, then refresh the cache.
    ///
    /// Failure is logged and swallowed: a usage ledger must never be able to
    /// break a transcription run, and must never surface an error the user
    /// cannot act on.
    fn record_stats(&mut self, rec: &StatsRecord) {
        if let Err(e) = stats::append(&Self::stats_root(), rec) {
            crashlog::log_warn(format!("stats append failed: {e}"));
            return;
        }
        self.reload_stats();
    }

    fn toggle_stats(&mut self, cx: &mut Context<Self>) {
        if self.stats_open {
            self.stats_open = false;
            self.stats_hover_day = None;
        } else {
            // Fresh numbers on open, and only one floating surface at a time
            // (they share MENU_Z).
            self.reload_stats();
            self.stats_hover_day = None;
            self.close_lang_selects();
            self.close_timing_popover();
            // Navigation, same rule as the gear: one tap on open.
            self.play_ui(sfx::Sfx::Click);
            self.stats_open = true;
        }
        cx.notify();
    }

    /// Write `settings.json`, re-check model files.
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        match self.settings.save() {
            Ok(_) => {
                self.settings_dirty = false;
                self.reset_model_config(cx);
                // Only the committed path taps. The drawer's auto-save on close
                // is navigation, not a commit, and stays silent.
                self.play_ui(sfx::Sfx::Click);
                self.flash_hint(
                    match self.model_status {
                        ModelStatus::Ready => "设置已保存 · 模型就绪",
                        ModelStatus::NotReady => "设置已保存 · 模型未就绪",
                    },
                    cx,
                );
            }
            Err(e) => {
                crashlog::log_error(format!("settings save failed: {e}"));
                self.flash_hint(format!("保存失败: {e}"), cx);
            }
        }
    }

    fn poll_worker(&mut self, cx: &mut Context<Self>) {
        // Expire status-bar hints.
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
                Ok(WorkerMsg::AlignerDirPicked(dir)) => {
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
                Ok(WorkerMsg::DemucsDirPicked(dir)) => {
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
                Ok(WorkerMsg::OutputDirPicked(dir)) => {
                    self.settings.output_dir = dir;
                    self.settings_dirty = false;
                    if let Err(e) = self.settings.save() {
                        crashlog::log_error(format!("output dir save failed: {e}"));
                        self.flash_hint(format!("输出目录已更新，但保存失败: {e}"), cx);
                    } else {
                        self.flash_hint("字幕输出目录已更新", cx);
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
                Ok(WorkerMsg::Probed { id, duration_sec }) => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        t.set_duration(duration_sec);
                    }
                    cx.notify();
                }
                Ok(WorkerMsg::Progress { id, stage, warning }) => {
                    self.active_stage = Some((id, stage));
                    // Non-fatal pipeline warnings (e.g. separation fell back to
                    // CPU) must be visible: this window has no console.
                    if let Some(w) = warning {
                        self.flash_hint(w, cx);
                    }
                    cx.notify();
                }
                Ok(WorkerMsg::Finished { id, result, timing }) => {
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
                            v: stats::LEDGER_VERSION,
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
                        if rec.ok && self.stats.tasks_ok == 1 {
                            if let Some(s) = self.stats.saved_sec() {
                                self.nudge_saved = Some(stats::format_span_secs(s).into());
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
            let mut dlg = rfd::FileDialog::new().set_title("选择语音识别模型目录");
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
            let mut dlg = rfd::FileDialog::new().set_title("选择对齐模型目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::AlignerDirPicked(dir));
            }
        });
        cx.notify();
    }

    fn pick_demucs_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.resolved_demucs_model_dir();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title("选择人声分离模型目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::DemucsDirPicked(dir));
            }
        });
        cx.notify();
    }

    fn pick_output_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.resolved_output_dir();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title("选择字幕输出目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::OutputDirPicked(dir));
            }
        });
        cx.notify();
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // New tasks inherit the settings defaults (language + vocal separation);
        // both stay overridable per row afterwards.
        let default_lang = self.settings.language.clone();
        let default_sep = self.settings.vocal_separation;
        let before = self.tasks.len();
        for path in paths {
            let path = path.canonicalize().unwrap_or(path);
            if !accept_input_path(&path) {
                // Silent skip in the UI is intentional ("no reaction"), but a
                // dropped file that never appears must be explainable later.
                crashlog::log_info(format!("input rejected (unsupported): {}", path.display()));
                continue;
            }
            if self.tasks.iter().any(|t| t.path == path) {
                continue;
            }
            let task = Task::from_path(&path, default_lang.clone(), default_sep);
            let id = task.id.clone();
            let p = task.path.clone();
            let tx = self.tx.clone();
            // Bounded process-wide probe pool (not one thread per file).
            probe_duration_async(p, move |dur| {
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
        // Feedback = list itself (no toast) — plus one tap for the whole drop.
        // Deliberately NOT one per file: dropping 20 files must not stutter.
        if self.tasks.len() > before {
            self.play_ui(sfx::Sfx::Click);
        }
        cx.notify();
    }

    /// True when the task-row language panel can actually paint for `lang_menu`.
    fn task_lang_menu_active(&self) -> bool {
        let Some(id) = self.lang_menu.as_ref() else {
            return false;
        };
        self.tasks.iter().any(|t| {
            &t.id == id
                && !t.status.locks_row_actions()
                && !self.exiting.contains_key(&t.id)
        })
    }

    /// Drop select flags that cannot render a panel (stale id / locked / exiting).
    fn sync_lang_select_state(&mut self) {
        if self.lang_menu.is_some() && !self.task_lang_menu_active() {
            self.lang_menu = None;
        }
        // Settings select only while the drawer is open or animating.
        if self.settings_lang_open && !self.settings_open && self.settings_progress() < 0.01 {
            self.settings_lang_open = false;
        }
    }

    /// Clear both language selects (no notify — caller owns the frame).
    fn close_lang_selects(&mut self) {
        self.lang_menu = None;
        self.settings_lang_open = false;
    }

    /// Language menus and the timing card share MENU_Z — only one floating surface.
    fn close_floating_overlays(&mut self) {
        self.close_lang_selects();
        self.close_timing_popover();
    }

    /// Eased 0..=1 progress for the processing-time popover.
    fn timing_popover_progress(&self) -> f32 {
        let t = (self.timing_pop_t0.elapsed().as_secs_f32() / TIMING_POP_ANIM_SECS).min(1.0);
        let e = ease_out_cubic(t);
        self.timing_pop_from + (self.timing_pop_to - self.timing_pop_from) * e
    }

    /// Whether the popover still paints (open or closing animation).
    fn timing_popover_visible(&self) -> bool {
        self.timing_popover.is_some() && self.timing_popover_progress() > 0.01
    }

    fn open_timing_popover(&mut self, id: &str) {
        if self.timing_popover.as_deref() == Some(id) && self.timing_pop_to >= 1.0 {
            return;
        }
        let cur = self.timing_popover_progress();
        self.timing_popover = Some(id.to_string());
        self.timing_pop_from = cur;
        self.timing_pop_to = 1.0;
        self.timing_pop_t0 = Instant::now();
    }

    fn close_timing_popover(&mut self) {
        self.timing_hover_since = None;
        self.timing_leave_since = None;
        if self.timing_popover.is_none() && self.timing_pop_to <= 0.0 {
            return;
        }
        let cur = self.timing_popover_progress();
        self.timing_pop_from = cur;
        self.timing_pop_to = 0.0;
        self.timing_pop_t0 = Instant::now();
        // Keep id until anim finishes so exit still paints the right card.
        if cur < 0.01 {
            self.timing_popover = None;
        }
    }

    /// Hover entered the timing chip / card for `id`.
    fn timing_hover_enter(&mut self, id: &str, cx: &mut Context<Self>) {
        // Do not stack under an open language menu.
        if self.lang_menu.is_some() || self.settings_lang_open {
            return;
        }
        // Cancel pending leave.
        self.timing_leave_since = None;
        match &self.timing_hover_since {
            Some((hid, _)) if hid == id => {}
            _ => {
                self.timing_hover_since = Some((id.to_string(), Instant::now()));
            }
        }
        // Already open: keep to=1.
        if self.timing_popover.as_deref() == Some(id) {
            if self.timing_pop_to < 1.0 {
                let cur = self.timing_popover_progress();
                self.timing_pop_from = cur;
                self.timing_pop_to = 1.0;
                self.timing_pop_t0 = Instant::now();
            }
            cx.notify();
            return;
        }
        // Open delay handled in `tick_timing_popover`.
        cx.notify();
    }

    /// Hover left chip/card; schedule close after a short grace.
    fn timing_hover_leave(&mut self, id: &str, cx: &mut Context<Self>) {
        if self
            .timing_hover_since
            .as_ref()
            .is_some_and(|(hid, _)| hid == id)
        {
            self.timing_hover_since = None;
        }
        if self.timing_popover.as_deref() == Some(id) || self.timing_pop_to > 0.0 {
            self.timing_leave_since = Some((id.to_string(), Instant::now()));
        }
        cx.notify();
    }

    /// Promote delayed hover → open; honor leave grace; clear id when closed.
    fn tick_timing_popover(&mut self) {
        if let Some((id, since)) = self.timing_hover_since.clone() {
            if since.elapsed() >= Duration::from_millis(TIMING_HOVER_DELAY_MS)
                && self.timing_popover.as_deref() != Some(id.as_str())
            {
                self.open_timing_popover(&id);
            }
        }
        if let Some((id, since)) = self.timing_leave_since.clone() {
            if since.elapsed() >= Duration::from_millis(TIMING_LEAVE_DELAY_MS)
                && self.timing_hover_since.is_none()
            {
                if self.timing_popover.as_deref() == Some(id.as_str()) {
                    self.close_timing_popover();
                } else {
                    self.timing_leave_since = None;
                }
            }
        }
        if self.timing_pop_to <= 0.0
            && self.timing_popover.is_some()
            && self.timing_popover_progress() < 0.01
        {
            self.timing_popover = None;
        }
    }

    /// Open / close the per-task language dropdown (closes the other select).
    fn toggle_lang_menu(&mut self, id: &str, cx: &mut Context<Self>) {
        let locked = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .is_some_and(|t| t.status.locks_row_actions() || self.exiting.contains_key(id));
        if locked {
            self.close_floating_overlays();
            self.flash_hint("处理中的任务不能改语言", cx);
            return;
        }
        let was_open = self.lang_menu.as_deref() == Some(id);
        self.close_floating_overlays();
        if !was_open {
            self.lang_menu = Some(id.to_string());
        }
        cx.notify();
    }

    /// Open / close settings default-language dropdown (closes list select).
    fn toggle_settings_lang(&mut self, cx: &mut Context<Self>) {
        let was_open = self.settings_lang_open;
        self.close_floating_overlays();
        if !was_open {
            self.settings_lang_open = true;
        }
        cx.notify();
    }

    /// Apply a source-language pick and close every language select.
    fn pick_source_language(
        &mut self,
        target: LangSelectTarget,
        language: &str,
        cx: &mut Context<Self>,
    ) {
        match target {
            LangSelectTarget::Task(id) => {
                let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
                    self.close_floating_overlays();
                    cx.notify();
                    return;
                };
                if task.status.locks_row_actions() {
                    self.close_floating_overlays();
                    self.flash_hint("处理中的任务不能改语言", cx);
                    return;
                }
                task.set_language(language);
            }
            LangSelectTarget::Settings => {
                self.settings.language = language.into();
                self.mark_settings_dirty(cx);
            }
        }
        // Single exit boundary: any successful (or abandoned) pick leaves no open select.
        self.close_floating_overlays();
        cx.notify();
    }

    /// Flip per-task vocal separation (row pill). Blocked while the row is
    /// processing; enabling needs the Demucs weights to be installed.
    fn toggle_task_separation(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if task.status.locks_row_actions() {
            self.flash_hint("处理中的任务不能修改人声分离", cx);
            return;
        }
        let next = !task.vocal_separation;
        if next && !self.demucs_ready {
            self.flash_hint("请先在设置中下载人声分离模型", cx);
            return;
        }
        task.set_vocal_separation(next);
        self.play_ui(sfx::Sfx::Click);
        self.flash_hint(
            if next {
                "本任务已开启人声分离"
            } else {
                "本任务已关闭人声分离"
            },
            cx,
        );
        cx.notify();
    }

    /// Close language selects (Escape / click-outside scrim). Timing is hover-only.
    fn dismiss_menus(&mut self, cx: &mut Context<Self>) {
        // Escape closes the stats panel too. It lives on MENU_Z with the menus,
        // so it must honour the same key — even though its own dismiss layer,
        // not a key, is what normally closes it. Behaviour when it is closed is
        // byte-for-byte what it was.
        let closed_stats = self.stats_open;
        if closed_stats {
            self.stats_open = false;
            self.stats_hover_day = None;
        }
        if self.lang_menu.is_none() && !self.settings_lang_open {
            if closed_stats {
                cx.notify();
            }
            return;
        }
        self.close_lang_selects();
        cx.notify();
    }

    /// Whether a language panel is live (scrim / Escape target).
    fn any_menu_open(&self) -> bool {
        self.task_lang_menu_active() || self.settings_lang_open
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
        // Drawer chrome shares the list surface — drop any floating overlays.
        self.close_floating_overlays();
        // The stats panel shares MENU_Z with those overlays; the drawer wins.
        self.stats_open = false;
        self.stats_hover_day = None;
        if open {
            self.play_ui(sfx::Sfx::Click);
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
        // Drawer slide + row enter/exit + empty-wave hover. Processing rows are
        // deliberately excluded: their visuals are static, and a multi-hour ASR
        // run must not force an animation frame for the whole window.
        let drawer = (self.settings_progress() - self.settings_to).abs() > 0.002;
        let row_anim = !self.exiting.is_empty() || !self.entering.is_empty();
        // Keep RAF while amp eases out after mouse leaves (smooth collapse to flat).
        let empty_wave =
            self.tasks.is_empty() && (self.empty_wave_hover || self.empty_wave_amp > 0.008);
        let timing_pop = (self.timing_popover_progress() - self.timing_pop_to).abs() > 0.002
            || self.timing_hover_since.is_some()
            || self.timing_leave_since.is_some();
        drawer || row_anim || empty_wave || timing_pop
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

    fn snapshot_task_rows(&self, now: Instant) -> Vec<TaskRowView> {
        let mut queued: Vec<(&str, u64)> = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued)
            .map(|t| (t.id.as_str(), t.queue_seq.unwrap_or(u64::MAX)))
            .collect();
        queued.sort_by_key(|(_, seq)| *seq);
        let ranks: HashMap<&str, usize> = queued
            .iter()
            .enumerate()
            .map(|(i, (id, _))| (*id, i + 1))
            .collect();

        self.tasks
            .iter()
            .map(|t| {
                let (opacity, interactive) = if let Some(t0) = self.exiting.get(&t.id) {
                    let p = (now.duration_since(*t0).as_secs_f32() / ROW_EXIT_SECS).clamp(0.0, 1.0);
                    (1.0 - ease_out_cubic(p), false)
                } else if let Some(t0) = self.entering.get(&t.id) {
                    let p = (now.duration_since(*t0).as_secs_f32() / ROW_ENTER_SECS).clamp(0.0, 1.0);
                    (ease_out_cubic(p), true)
                } else {
                    (1.0, true)
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
        self.status_hint_good = false;
        cx.notify();
    }

    /// Same slot, success colouring: green for "this worked", amber (the
    /// default) for "this needs you". Tone is the whole message here — the
    /// hint is one line of text and nothing else carries the verdict.
    fn flash_good_hint_for(
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

    /// Mark a batch as running.
    ///
    /// The progress denominator is **derived at paint time** from live rows
    /// (`batch_done` + still-active) and never stored: deleting or adding
    /// tasks mid-run, or clicking 开始 again, cannot leave the counter
    /// pointing at rows that no longer exist.
    fn begin_batch(&mut self, job_count: usize) {
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
        let done = self.batch_ok;
        let err = self.batch_err;
        self.batch_mode = false;
        self.batch_done = 0;
        self.batch_ok = 0;
        self.batch_err = 0;
        if done == 0 && err == 0 {
            return;
        }
        let msg = if err == 0 {
            format!("全部完成 · {done} 个任务")
        } else if done == 0 {
            format!("批次结束 · {err} 个失败")
        } else {
            format!("批次结束 · 完成 {done} · 失败 {err}")
        };
        // The first success's saved time rides the run-complete line; later runs
        // are a plain tally.
        let msg = match self.nudge_saved.take() {
            Some(saved) => format!("{msg} · 已省 {saved} · 左下角看统计"),
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
    fn ensure_can_start(&mut self, cx: &mut Context<Self>) -> bool {
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
        self.play_ui(sfx::Sfx::Click);
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
        self.play_ui(sfx::Sfx::Click);
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
            SharedString::from(first_stage.label()),
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
                t.error = Some("识别工作线程已退出".into());
            }
            // Same rule as the pre-flight failure: a settled row must not stall
            // the rest of the batch.
            self.try_start_next(cx);
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
    fn clear_all(&mut self, cx: &mut Context<Self>) {
        let live_any = self.tasks.iter().any(|t| !self.exiting.contains_key(&t.id));
        if !live_any {
            self.flash_hint("列表已空", cx);
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
            self.flash_hint("已清空队列，当前任务继续处理", cx);
        }
        cx.notify();
    }

    fn open_task_output(&mut self, id: &str, cx: &mut Context<Self>) {
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

/// Human-readable message from a caught panic payload.
fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "未知 panic".into()
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
    // Primary deliverable: {target_dir}/{stem}.srt, or .txt when SRT output is
    // switched off (real ASR, no stubs). Runs only on the dedicated worker thread.
    process_media_file_with_progress(path, name, settings, &app_root, on_stage)
        .map_err(|e| e.to_string())
}

/// Per-frame paint snapshot of a list row. Owned so the children closure
/// does not clone `Task` (path, output path, …).
struct TaskRowView {
    id: String,
    status: TaskStatus,
    has_output: bool,
    language: String,
    /// Per-task vocal separation (settings default, overridable per row).
    vocal_separation: bool,
    error: Option<String>,
    name: String,
    size_label: String,
    duration_label: String,
    is_video: bool,
    timing: Option<TaskTiming>,
    opacity: f32,
    interactive: bool,
    queue_rank: Option<usize>,
}

impl Render for OneAsrApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drop select state that can no longer paint (locked / deleted / drawer closed).
        self.sync_lang_select_state();
        self.tick_timing_popover();

        // Drive drawer / row-hover fades at display refresh.
        if self.animations_active() {
            window.request_animation_frame();
        }

        let drawer_p = self.settings_progress();
        let menu_open = self.any_menu_open();
        let stats_open = self.stats_open;

        div()
            .id("oneasr-root")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(BG)
            .text_color(TEXT)
            // Primary + explicit CJK fallbacks (stripped Win10 / deleted 雅黑).
            .font(self.ui_font.font.clone())
            .on_action(cx.listener(|this, _: &DismissMenus, _, cx| {
                this.dismiss_menus(cx);
            }))
            .child(self.render_titlebar())
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
                    // `occlude`: block hits + scroll to the list underneath (otherwise
                    // overflow_y_scroll on both layers double-notifies every wheel tick —
                    // a likely amplifier for the hard-to-repro settings scroll crash).
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
                                .occlude()
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
            .child(self.render_status_bar(cx))
            .when(stats_open, |el| el.child(self.render_stats_panel(cx)))
            .when(menu_open || stats_open, |el| {
                el.child(popover_dismiss_layer(cx))
            })
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
/// Language chip column (fixed so status changes never shove it).
const LANG_COL_PX: f32 = 64.;
/// Per-task vocal-separation toggle column (right of the language chip).
const SEPARATE_COL_PX: f32 = 76.;
/// Status pill column — wide enough for「人声分离 1800/1800」「转写中 99/99」.
const STATUS_COL_PX: f32 = 132.;
/// List language menu width (absolute panel under the chip).
const LANG_MENU_W: f32 = 168.;
/// Floating language menu max height before it scrolls.
const LANG_MENU_MAX_H: f32 = 280.;
/// Processing-time popover enter/exit (seconds).
const TIMING_POP_ANIM_SECS: f32 = 0.18;
/// Hover delay before opening the timing card (ms).
const TIMING_HOVER_DELAY_MS: u64 = 100;
/// Leave grace so pointer can travel chip → card without flicker (ms).
const TIMING_LEAVE_DELAY_MS: u64 = 120;
/// Timing breakdown card width.
const TIMING_POP_W: f32 = 220.;

// ── Popover select layer model (GPUI has no built-in Select) ─────────────────
// Two deferred layers, sorted by priority (higher paints + hit-tests on top):
//
//   MENU_DISMISS_Z  full-window scrim on the app root, `occlude`
//                   → click-outside closes (toolbar / list / status bar)
//   MENU_Z          floating panel, `occlude`
//                   → owns hits above the scrim so option `on_click` completes
//
// Deferred escapes the list's overflow stack. `occlude` is required so the
// scrim is not also "hovered" under the panel (GPUI hit-test walks every
// Normal hitbox under the cursor until it hits BlockMouse).
//
// Invariant: `lang_menu` / `settings_lang_open` only count as "open" when a
// panel can actually render (`task_lang_menu_active` / settings drawer). Scrim
// visibility follows that derived state, never a stale flag alone.
const MENU_DISMISS_Z: usize = 5;
const MENU_Z: usize = 10;

/// Smooth deceleration (approx cubic-bezier ease-out).
fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

impl OneAsrApp {
    /// Custom title bar. GPUI never sets `WS_CAPTION`, so the "native" caption is only
    /// a DWM fallback — missing or dead on some Win10 machines (the user report:
    /// click grays out, no drag / min / max / close). Drawing our own + routing hits
    /// through `WindowControlArea` works regardless of DWM state.
    ///
    /// Windows-only contract (same as gpui-component's TitleBar): **no `on_click` here** —
    /// GPUI maps the areas to HTCAPTION / HTMINBUTTON / HTMAXBUTTON / HTCLOSE in
    /// `WM_NCHITTEST` and the OS performs the action. Double-click on the drag strip
    /// also toggles maximize for free (DefWindowProc on HTCAPTION).
    fn render_titlebar(&mut self) -> impl IntoElement {
        div()
            .h(px(32.))
            .w_full()
            .flex()
            .items_center()
            .bg(PANEL)
            .border_b_1()
            .border_color(LINE)
            .child(
                // Drag strip: everything left of the buttons moves the window.
                div()
                    .id("titlebar-drag")
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .pl_3()
                    .overflow_hidden()
                    .window_control_area(WindowControlArea::Drag)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_SOFT)
                                    .whitespace_nowrap()
                                    .child("OneAsr"),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_SOFT)
                                    .opacity(0.72)
                                    .whitespace_nowrap()
                                    .child(APP_VERSION_LABEL),
                            ),
                    ),
            )
            .child(caption_btn(
                "titlebar-min",
                "icons/win-min.svg",
                WindowControlArea::Min,
                false,
            ))
            .child(caption_btn(
                "titlebar-max",
                "icons/win-max.svg",
                WindowControlArea::Max,
                false,
            ))
            .child(caption_btn(
                "titlebar-close",
                "icons/win-close.svg",
                WindowControlArea::Close,
                true,
            ))
    }

    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings_open = self.settings_open;
        let dl_busy = self.any_download_busy();
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
                    .child(
                        div()
                            .id("app-logo-hit")
                            .cursor_pointer()
                            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                                if this.logo_hover != *hovered {
                                    this.logo_hover = *hovered;
                                    cx.notify();
                                }
                            }))
                            .child(app_logo(self.logo_hover)),
                    )
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
                        dl_busy,
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
        let items = self.snapshot_task_rows(Instant::now());
        let hover_id = self.hover_row.clone();
        let lang_menu = self.lang_menu.clone();
        let active_stage = self.active_stage.clone();
        let timing_popover = self.timing_popover.clone();
        let timing_visible = self.timing_popover_visible();
        let timing_progress = self.timing_popover_progress();

        div()
            .id("task-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(items.into_iter().enumerate().map(move |(ix, row)| {
                let interactive = row.interactive;
                let row_opacity = row.opacity;
                let id_start = row.id.clone();
                let id_del = row.id.clone();
                let id_open = row.id.clone();
                let id_lang = row.id.clone();
                let row_id = row.id.clone();
                let row_id_hover = row.id.clone();
                let id_lang_pick = row.id.clone();
                let done_with_out = row.status == TaskStatus::Done && row.has_output;
                let primary_kind = if done_with_out {
                    IconKind::Folder
                } else {
                    IconKind::Play
                };
                let primary_tip = if done_with_out {
                    "打开输出位置"
                } else {
                    "开始"
                };
                let primary_enabled = interactive
                    && if done_with_out {
                        true
                    } else {
                        matches!(row.status, TaskStatus::Pending | TaskStatus::Error)
                    };
                let can_delete = interactive && !row.status.locks_row_actions();
                let can_edit_lang = interactive && !row.status.locks_row_actions();
                let lang_open = lang_menu.as_ref() == Some(&row.id);
                let lang_meta = source_language_by_id(&row.language);
                let lang_short = lang_meta.map(|l| l.short).unwrap_or("?");
                let lang_current = row.language.clone();
                let id_sep = row.id.clone();
                let sep_on = row.vocal_separation;
                let err = row.error.clone();
                let name = row.name.clone();
                let name_tip = row.name.clone();
                let size_l = row.size_label.clone();
                let status = row.status;
                let is_video = row.is_video;
                let is_hovered = interactive && hover_id.as_ref() == Some(&row.id);
                let qn = row.queue_rank;

                // Meta: size · duration · status text (no free-floating status circle).
                let dur = row.duration_label.clone();
                let stage_for_row = active_stage
                    .as_ref()
                    .filter(|(sid, _)| sid == &row.id)
                    .map(|(_, s)| s.as_ref());
                let (status_label, status_color, status_bg) =
                    status_pill_style(status, qn, stage_for_row);
                let timing_total = row
                    .timing
                    .as_ref()
                    .filter(|t| t.has_breakdown())
                    .map(|t| t.total_label());
                let timing_for_card = row
                    .timing
                    .as_ref()
                    .filter(|t| t.has_breakdown());
                let timing_open = timing_popover.as_ref() == Some(&row.id) && timing_visible;
                let timing_pop_p = if timing_open || timing_popover.as_ref() == Some(&row.id) {
                    timing_progress
                } else {
                    0.0
                };
                let meta_media = format!("{size_l}  ·  {dur}");
                let accent = row_accent(status);

                let row_bg = if is_hovered {
                    ROW_HOVER
                } else if ix % 2 == 1 {
                    ZEBRA
                } else {
                    PANEL
                };

                let lang_menu_float: Option<gpui::AnyElement> = if lang_open && can_edit_lang {
                    Some(
                        floating_lang_menu(
                            SharedString::from(format!("lang-menu-{id_lang_pick}")),
                            &lang_current,
                            &format!("lang-opt-{id_lang_pick}"),
                            LangSelectTarget::Task(id_lang_pick.clone()),
                            LangMenuLayout::Chip,
                            cx,
                        )
                        .into_any_element(),
                    )
                } else {
                    None
                };

                div()
                    .id(SharedString::from(format!("task-{row_id}")))
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    // Never shrink: the list is a scroll container, so rows
                    // past one screenful must overflow (→ scrollbar), not
                    // compress — every child here truncates, so Taffy's
                    // min-content floor is ~0 and without this 11+ rows
                    // squash into each other.
                    .flex_shrink_0()
                    // Allow floating language / timing menus to paint outside the row box.
                    .when(!lang_open && !timing_open, |el| el.overflow_hidden())
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
                                            // Keep overflow when timing card is closed so long names clip.
                                            .when(!timing_open, |el| el.overflow_hidden())
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
                                            .child({
                                                // Meta: size · media length · [用时 chip + hover card]
                                                let id_timing = row.id.clone();
                                                let id_timing_leave = row.id.clone();
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .gap_1p5()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(MUTED_SOFT)
                                                            .whitespace_nowrap()
                                                            .child(meta_media),
                                                    )
                                                    .when_some(
                                                        timing_total.zip(timing_for_card),
                                                        |el, (total_label, timing)| {
                                                            let id_card = id_timing.clone();
                                                            let id_card_leave =
                                                                id_timing_leave.clone();
                                                            let show_card = timing_pop_p > 0.01;
                                                            el.child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(MUTED_SOFT)
                                                                    .child("·"),
                                                            )
                                                            // Chip + card share one relative box so
                                                            // the popover anchors under 用时, not the row.
                                                            .child(
                                                                div()
                                                                    .relative()
                                                                    .flex_shrink_0()
                                                                    .child(
                                                                        div()
                                                                            .id(SharedString::from(
                                                                                format!(
                                                                                    "timing-chip-{id_timing}"
                                                                                ),
                                                                            ))
                                                                            .px_1p5()
                                                                            .py_0p5()
                                                                            .rounded_md()
                                                                            .cursor_default()
                                                                            .bg(if timing_open {
                                                                                ACCENT_SOFT
                                                                            } else {
                                                                                MEDIA_PLATE
                                                                            })
                                                                            .hover(|s| {
                                                                                s.bg(ACCENT_SOFT)
                                                                            })
                                                                            .on_hover(cx.listener(
                                                                                move |this,
                                                                                      hovered: &bool,
                                                                                      _,
                                                                                      cx| {
                                                                                    if *hovered {
                                                                                        this.timing_hover_enter(
                                                                                            &id_timing,
                                                                                            cx,
                                                                                        );
                                                                                    } else {
                                                                                        this.timing_hover_leave(
                                                                                            &id_timing_leave,
                                                                                            cx,
                                                                                        );
                                                                                    }
                                                                                },
                                                                            ))
                                                                            .child(
                                                                                div()
                                                                                    .text_xs()
                                                                                    .text_color(
                                                                                        if timing_open {
                                                                                            ACCENT
                                                                                        } else {
                                                                                            MUTED
                                                                                        },
                                                                                    )
                                                                                    .whitespace_nowrap()
                                                                                    .child(format!(
                                                                                        "用时 {total_label}"
                                                                                    )),
                                                                            ),
                                                                    )
                                                                    .when(show_card, |wrap| {
                                                                        wrap.child(
                                                                            timing_breakdown_popover(
                                                                                SharedString::from(
                                                                                    format!(
                                                                                        "timing-pop-{id_card}"
                                                                                    ),
                                                                                ),
                                                                                &timing,
                                                                                timing_pop_p,
                                                                                cx.listener(
                                                                                    move |this,
                                                                                          hovered: &bool,
                                                                                          _,
                                                                                          cx| {
                                                                                        if *hovered {
                                                                                            this.timing_hover_enter(
                                                                                                &id_card,
                                                                                                cx,
                                                                                            );
                                                                                        } else {
                                                                                            this.timing_hover_leave(
                                                                                                &id_card_leave,
                                                                                                cx,
                                                                                            );
                                                                                        }
                                                                                    },
                                                                                ),
                                                                            ),
                                                                        )
                                                                    }),
                                                            )
                                                        },
                                                    )
                                            }),
                                    )
                                    // Status — informational, left of the control
                                    // cluster so all clickable pills (语言 / 分离 /
                                    // 开始 / 删除) sit together on the right.
                                    .child(
                                        div()
                                            .w(px(STATUS_COL_PX))
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_center()
                                            .items_center()
                                            .child(
                                                div()
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
                                            ),
                                    )
                                    // Language select — fixed column so status length never shifts it.
                                    .child(
                                        div()
                                            .w(px(LANG_COL_PX))
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_center()
                                            .items_center()
                                            .child(
                                                div()
                                                    .relative()
                                                    .child(
                                                        div()
                                                            .id(SharedString::from(format!(
                                                                "lang-{row_id}"
                                                            )))
                                                            .px_2()
                                                            .py_0p5()
                                                            .rounded_md()
                                                            .border_1()
                                                            .border_color(if lang_open {
                                                                ACCENT
                                                            } else if can_edit_lang {
                                                                LINE
                                                            } else {
                                                                LINE_SOFT
                                                            })
                                                            .bg(if lang_open {
                                                                ACCENT_SOFT
                                                            } else if can_edit_lang {
                                                                BG
                                                            } else {
                                                                PANEL
                                                            })
                                                            .when(can_edit_lang, |el| {
                                                                el.cursor_pointer().hover(|s| {
                                                                    s.bg(ACCENT_SOFT)
                                                                        .border_color(ACCENT)
                                                                })
                                                            })
                                                            .when(can_edit_lang, |el| {
                                                                el.on_click(cx.listener(
                                                                    move |this, _, _, cx| {
                                                                        this.toggle_lang_menu(
                                                                            &id_lang, cx,
                                                                        );
                                                                    },
                                                                ))
                                                            })
                                                            .child(
                                                                div()
                                                                    .flex()
                                                                    .items_center()
                                                                    .gap_1()
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .font_weight(
                                                                                gpui::FontWeight::MEDIUM,
                                                                            )
                                                                            .text_color(
                                                                                if can_edit_lang {
                                                                                    TEXT
                                                                                } else {
                                                                                    MUTED_SOFT
                                                                                },
                                                                            )
                                                                            .whitespace_nowrap()
                                                                            .child(
                                                                                lang_short
                                                                                    .to_string(),
                                                                            ),
                                                                    )
                                                                    .child(
                                                                        div()
                                                                            .text_xs()
                                                                            .text_color(MUTED)
                                                                            .child(if lang_open {
                                                                                "▴"
                                                                            } else {
                                                                                "▾"
                                                                            }),
                                                                    ),
                                                            ),
                                                    )
                                                    .children(lang_menu_float),
                                            ),
                                    )
                                    // Vocal separation toggle — same row idiom as
                                    // the language chip: compact pill, filled when
                                    // on, per-task override of the settings default.
                                    .child(
                                        div()
                                            .w(px(SEPARATE_COL_PX))
                                            .flex_shrink_0()
                                            .flex()
                                            .justify_center()
                                            .items_center()
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "sep-{id_sep}"
                                                    )))
                                                    .px_2()
                                                    .py_1()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(if sep_on {
                                                        ACCENT
                                                    } else if can_edit_lang {
                                                        LINE
                                                    } else {
                                                        LINE_SOFT
                                                    })
                                                    .bg(if sep_on {
                                                        ACCENT
                                                    } else if can_edit_lang {
                                                        BG
                                                    } else {
                                                        PANEL
                                                    })
                                                    .when(can_edit_lang, |el| {
                                                        el.cursor_pointer().hover(|s| {
                                                            s.bg(ACCENT_SOFT)
                                                                .border_color(ACCENT)
                                                        })
                                                    })
                                                    .when(can_edit_lang, |el| {
                                                        el.on_click(cx.listener(
                                                            move |this, _, _, cx| {
                                                                this.toggle_task_separation(
                                                                    &id_sep, cx,
                                                                );
                                                            },
                                                        ))
                                                    })
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .whitespace_nowrap()
                                                            .text_color(if sep_on {
                                                                PANEL
                                                            } else if can_edit_lang {
                                                                TEXT
                                                            } else {
                                                                MUTED_SOFT
                                                            })
                                                            .child("分离"),
                                                    )
                                                    .tooltip({
                                                        let tip = if sep_on {
                                                            "人声分离：开启（转录前分离人声，点击关闭）"
                                                        } else {
                                                            "人声分离：关闭（点击开启）"
                                                        };
                                                        move |_, cx| {
                                                            cx.new(|_| NameTooltip {
                                                                text: tip.into(),
                                                            })
                                                            .into()
                                                        }
                                                    }),
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
                                                    this.close_floating_overlays();
                                                    if this
                                                        .tasks
                                                        .iter()
                                                        .find(|t| t.id == id_start)
                                                        .map(|t| {
                                                            t.status == TaskStatus::Done
                                                                && t.output_file.is_some()
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
                                                    this.close_floating_overlays();
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
        let language = self.settings.language.clone();
        let length_preset = self.settings.subtitle_length_preset.clone();
        let chunk_target = self.settings.chunk_target_seconds_clamped();
        let asr_id = self.settings.selected_asr_id();
        let model = self.settings.asr_model_dir.display().to_string();
        let model_tip = model.clone();
        let aligner = self.settings.aligner_model_dir.display().to_string();
        let aligner_tip = aligner.clone();
        let output_dir = self.settings.resolved_output_dir().display().to_string();
        let output_dir_tip = output_dir.clone();
        let save_next = self.settings.save_next_to_source;
        let output_srt = self.settings.output_srt;
        let output_txt = self.settings.output_txt;
        let text_script = self.settings.text_script_choice();
        let vocal_sep = self.settings.vocal_separation;
        let sound = self.settings.sound;
        let demucs_ready = self.demucs_ready;
        let demucs_dl = self.progress_for(ModelId::HtdemucsFt).cloned();
        let demucs_dl_busy = self.download_busy(ModelId::HtdemucsFt);
        let demucs_dir = self
            .settings
            .resolved_demucs_model_dir()
            .display()
            .to_string();
        let demucs_dir_tip = demucs_dir.clone();
        let dirty = self.is_settings_dirty(cx);
        // Use probe cache — never re-stat model dirs on every scroll paint.
        let asr_ready = self.asr_ready;
        let align_ready = self.align_ready;
        let cuda_ready = self.cuda_ready;
        // Progress is keyed by model id — never show another size’s snapshot here.
        let asr_dl = self.progress_for(asr_id).cloned();
        let align_dl = self.progress_for(ModelId::QwenAlign06B).cloned();
        let cuda_dl = self.progress_for(ModelId::CudaRuntime).cloned();
        let asr_dl_busy = self.download_busy(asr_id);
        let asr_size_locked = self.download_kind_busy(ModelKind::Asr);
        let align_dl_busy = self.download_busy(ModelId::QwenAlign06B);
        let cuda_dl_busy = self.download_busy(ModelId::CudaRuntime);

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
                    .child(section({
                        let open = self.settings_lang_open;
                        let cur_label = source_language_by_id(&language)
                            .map(|l| l.label)
                            .unwrap_or("中文普通话");

                        // 语言 | 字幕长度 并排平分
                        div()
                            .flex()
                            .items_start()
                            .gap_2p5()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child("默认语言"),
                                    )
                                    .child(
                                        div()
                                            .relative()
                                            .w_full()
                                            .child(
                                                div()
                                                    .id("settings-lang-trigger")
                                                    .w_full()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .rounded_md()
                                                    .border_1()
                                                    .border_color(if open {
                                                        ACCENT
                                                    } else {
                                                        LINE
                                                    })
                                                    .bg(if open { ACCENT_SOFT } else { BG })
                                                    .cursor_pointer()
                                                    .hover(|s| {
                                                        s.bg(ACCENT_SOFT).border_color(ACCENT)
                                                    })
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.toggle_settings_lang(cx);
                                                    }))
                                                    .child(
                                                        div()
                                                            .flex()
                                                            .items_center()
                                                            .justify_between()
                                                            .child(
                                                                div()
                                                                    .text_sm()
                                                                    .text_color(TEXT)
                                                                    .whitespace_nowrap()
                                                                    .child(cur_label.to_string()),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_xs()
                                                                    .text_color(MUTED)
                                                                    .child(if open {
                                                                        "▴"
                                                                    } else {
                                                                        "▾"
                                                                    }),
                                                            ),
                                                    ),
                                            )
                                            .when(open, |el| {
                                                el.child(floating_lang_menu(
                                                    "settings-lang-menu".into(),
                                                    &language,
                                                    "settings-lang-opt",
                                                    LangSelectTarget::Settings,
                                                    LangMenuLayout::FullWidth,
                                                    cx,
                                                ))
                                            }),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child("字幕长度"),
                                    )
                                    .child(
                                        div().flex().gap_1p5().children(
                                            [
                                                ("short", "短"),
                                                ("standard", "标准"),
                                                ("loose", "宽松"),
                                            ]
                                            .into_iter()
                                            .map(|(id, label)| {
                                                let active = length_preset == id;
                                                btn(
                                                    label,
                                                    if active {
                                                        BtnKind::Primary
                                                    } else {
                                                        BtnKind::Secondary
                                                    },
                                                    true,
                                                    cx.listener(move |this, _, _, cx| {
                                                        if this.settings.subtitle_length_preset
                                                            == id
                                                        {
                                                            return;
                                                        }
                                                        this.settings.subtitle_length_preset =
                                                            id.into();
                                                        this.mark_settings_dirty(cx);
                                                    }),
                                                )
                                            }),
                                        ),
                                    ),
                            )
                            .into_any_element()
                    }))
                    .child(section({
                        // 分段时长：30–180s 预设（默认 60）；短尾 <15s 运行时合并
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
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
                                            .child("分段时长"),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED)
                                            .child(format!(
                                                "{chunk_target} 秒 · {CHUNK_TARGET_MIN_SEC}–{CHUNK_TARGET_MAX_SEC}"
                                            )),
                                    ),
                            )
                            .child(
                                div().flex().gap_1p5().children(
                                    CHUNK_TARGET_PRESETS.iter().copied().map(|(sec, label)| {
                                        let active = chunk_target == sec;
                                        btn(
                                            label,
                                            if active {
                                                BtnKind::Primary
                                            } else {
                                                BtnKind::Secondary
                                            },
                                            true,
                                            cx.listener(move |this, _, _, cx| {
                                                if this.settings.chunk_target_seconds == sec {
                                                    return;
                                                }
                                                this.settings.chunk_target_seconds = sec;
                                                this.mark_settings_dirty(cx);
                                            }),
                                        )
                                    }),
                                ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_0p5()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED)
                                            .child("建议 4GB 显存使用 60 秒分段时长"),
                                    ),
                            )
                            .into_any_element()
                    }))
                    // 输出：格式 | 中文字形 并排平分（同「默认语言 | 字幕长度」）
                    .child(section(
                        div()
                            .flex()
                            .items_start()
                            .gap_2p5()
                            .child(
                                div()
                                    // 中文输出 takes its natural width (3 pills) and
                                    // may shrink; 输出格式 absorbs the remaining space.
                                    .flex()
                                    .flex_col()
                                    .gap_1()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .whitespace_nowrap()
                                                    .child("输出格式"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .gap_1p5()
                                            .child(btn(
                                                "SRT",
                                                if output_srt {
                                                    BtnKind::Primary
                                                } else {
                                                    BtnKind::Secondary
                                                },
                                                true,
                                                cx.listener(|this, _, _, cx| {
                                                    if !this.settings.output_txt {
                                                        this.flash_hint(
                                                            "至少保留一种输出格式",
                                                            cx,
                                                        );
                                                        return;
                                                    }
                                                    this.settings.output_srt =
                                                        !this.settings.output_srt;
                                                    this.mark_settings_dirty(cx);
                                                }),
                                            ))
                                            .child(btn(
                                                "TXT",
                                                if output_txt {
                                                    BtnKind::Primary
                                                } else {
                                                    BtnKind::Secondary
                                                },
                                                true,
                                                cx.listener(|this, _, _, cx| {
                                                    if !this.settings.output_srt {
                                                        this.flash_hint(
                                                            "至少保留一种输出格式",
                                                            cx,
                                                        );
                                                        return;
                                                    }
                                                    this.settings.output_txt =
                                                        !this.settings.output_txt;
                                                    this.mark_settings_dirty(cx);
                                                }),
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_1()
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
                                                    .whitespace_nowrap()
                                                    .child("中文输出"),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED)
                                                    .whitespace_nowrap()
                                                    .child("仅中文/粤语"),
                                            ),
                                    )
                                    .child(
                                        div().flex().gap_1().children(TextScript::ALL.map(
                                            |script| {
                                                let active = text_script == script;
                                                btn(
                                                    script.label(),
                                                    if active {
                                                        BtnKind::Primary
                                                    } else {
                                                        BtnKind::Secondary
                                                    },
                                                    true,
                                                    cx.listener(move |this, _, _, cx| {
                                                        if this.settings.text_script == script.id() {
                                                            return;
                                                        }
                                                        this.settings.text_script =
                                                            script.id().into();
                                                        this.mark_settings_dirty(cx);
                                                    }),
                                                )
                                            },
                                        )),
                                    ),
                            )
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
                                    .text_color(TEXT)
                                    .child("字幕输出位置"),
                            )
                            .child(
                                div().flex().gap_1p5().children(
                                    [(true, "视频同目录"), (false, "指定目录")]
                                        .into_iter()
                                        .map(|(next, label)| {
                                            let active = save_next == next;
                                            btn(
                                                label,
                                                if active {
                                                    BtnKind::Primary
                                                } else {
                                                    BtnKind::Secondary
                                                },
                                                true,
                                                cx.listener(move |this, _, _, cx| {
                                                    if this.settings.save_next_to_source == next {
                                                        return;
                                                    }
                                                    this.settings.save_next_to_source = next;
                                                    this.mark_settings_dirty(cx);
                                                }),
                                            )
                                        }),
                                ),
                            )
                            .child(
                                div()
                                    .id("output-dir")
                                    .flex()
                                    .items_center()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(LINE)
                                    .bg(PANEL)
                                    .overflow_hidden()
                                    .opacity(if save_next { 0.45 } else { 1.0 })
                                    .hover(|s| s.border_color(ACCENT))
                                    .child(
                                        div()
                                            .id("output-dir-path")
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(TEXT)
                                            .whitespace_normal()
                                            .line_clamp(2)
                                            .child(output_dir)
                                            .tooltip(move |_, cx| {
                                                cx.new(|_| NameTooltip {
                                                    text: output_dir_tip.clone().into(),
                                                })
                                                .into()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("output-dir-browse")
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
                                                this.pick_output_dir(cx)
                                            })),
                                    ),
                            )
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
                                            .child("语音识别模型"),
                                    )
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(if asr_ready { ACCENT } else { DANGER }),
                                    ),
                            )
                            .child(
                                div().flex().gap_1p5().children(
                                    ModelId::ASR_CHOICES.into_iter().map(|id| {
                                        let active = asr_id == id;
                                        let can_switch = !asr_size_locked || active;
                                        btn(
                                            id.short_label(),
                                            if active {
                                                BtnKind::Primary
                                            } else {
                                                BtnKind::Secondary
                                            },
                                            can_switch,
                                            cx.listener(move |this, _, _, cx| {
                                                if this.settings.selected_asr_id() == id {
                                                    return;
                                                }
                                                if this.download_kind_busy(ModelKind::Asr) {
                                                    this.flash_hint(
                                                        "ASR 下载进行中，请稍后再切换尺寸",
                                                        cx,
                                                    );
                                                    return;
                                                }
                                                this.settings.select_asr_model(id);
                                                this.clear_stale_asr_progress();
                                                this.refresh_model_probe();
                                                this.mark_settings_dirty(cx);
                                            }),
                                        )
                                    }),
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
                                ModelKind::Asr,
                                cx.listener(move |this, _, _, cx| {
                                    let id = this.settings.selected_asr_id();
                                    this.start_model_download(id, cx);
                                }),
                                cx.listener(move |this, _, _, cx| {
                                    let id = this.settings.selected_asr_id();
                                    this.cancel_model_download(id, cx);
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
                                            .child("对齐模型"),
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
                                ModelKind::Align,
                                cx.listener(|this, _, _, cx| {
                                    this.start_model_download(ModelId::QwenAlign06B, cx);
                                }),
                                cx.listener(|this, _, _, cx| {
                                    this.cancel_model_download(ModelId::QwenAlign06B, cx);
                                }),
                            ))
                            .into_any_element(),
                    ))
                    // 人声分离（可选）：HTDemucs 原生 Rust 推理，转录前压掉 BGM
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
                                            .child("人声分离"),
                                    )
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(if demucs_ready { ACCENT } else { DANGER }),
                                    ),
                            )
                            // 开关 = 新任务默认值（任务行里可单独覆盖）
                            .child(
                                div().flex().gap_1p5().children(
                                    [(false, "关闭"), (true, "开启")]
                                        .into_iter()
                                        .map(|(on, label)| {
                                            let active = vocal_sep == on;
                                            btn(
                                                label,
                                                if active {
                                                    BtnKind::Primary
                                                } else {
                                                    BtnKind::Secondary
                                                },
                                                true,
                                                cx.listener(move |this, _, _, cx| {
                                                    if this.settings.vocal_separation == on {
                                                        return;
                                                    }
                                                    if on && !this.demucs_ready {
                                                        this.flash_hint(
                                                            "请先下载人声分离模型",
                                                            cx,
                                                        );
                                                        return;
                                                    }
                                                    this.settings.vocal_separation = on;
                                                    this.mark_settings_dirty(cx);
                                                }),
                                            )
                                        }),
                                ),
                            )
                            .child(
                                div()
                                    .id("demucs-dir")
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
                                            .id("demucs-dir-path")
                                            .flex_1()
                                            .min_w_0()
                                            .px_2p5()
                                            .py_1p5()
                                            .text_xs()
                                            .text_color(TEXT)
                                            .whitespace_normal()
                                            .line_clamp(2)
                                            .child(demucs_dir)
                                            .tooltip(move |_, cx| {
                                                cx.new(|_| NameTooltip {
                                                    text: demucs_dir_tip.clone().into(),
                                                })
                                                .into()
                                            }),
                                    )
                                    .child(
                                        div()
                                            .id("demucs-dir-browse")
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
                                                this.pick_demucs_dir(cx)
                                            })),
                                    ),
                            )
                            .child(model_download_row(
                                "demucs-dl-btn",
                                "demucs-dl-cancel",
                                demucs_ready,
                                demucs_dl_busy,
                                demucs_dl.as_ref(),
                                ModelKind::Demucs,
                                cx.listener(|this, _, _, cx| {
                                    this.start_model_download(ModelId::HtdemucsFt, cx);
                                }),
                                cx.listener(|this, _, _, cx| {
                                    this.cancel_model_download(ModelId::HtdemucsFt, cx);
                                }),
                            ))
                            .into_any_element(),
                    ))
                    // Compute: backend first, then CUDA pack (only needed for GPU).
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
                                                if this.settings.backend == id {
                                                    return;
                                                }
                                                if id == "cuda" && !this.cuda_ready {
                                                    this.flash_hint(
                                                        "请先下载 CUDA 运行库，再选择 GPU",
                                                        cx,
                                                    );
                                                    return;
                                                }
                                                this.settings.backend = id.into();
                                                this.refresh_model_probe();
                                                this.mark_settings_dirty(cx);
                                            }),
                                        )
                                    }),
                                ),
                            )
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
                                            .child("CUDA 运行库"),
                                    )
                                    .child(
                                        div()
                                            .size(px(8.))
                                            .rounded_full()
                                            .bg(if cuda_ready { ACCENT } else { DANGER }),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED)
                                    .child("需要 N 卡（NVIDIA 显卡），显存 4GB 起"),
                            )
                            .child(component_install_row(
                                "cuda-dl-btn",
                                "cuda-dl-cancel",
                                cuda_ready,
                                cuda_dl_busy,
                                cuda_dl.as_ref(),
                                cx.listener(|this, _, _, cx| {
                                    this.start_model_download(ModelId::CudaRuntime, cx);
                                }),
                                cx.listener(|this, _, _, cx| {
                                    this.cancel_model_download(ModelId::CudaRuntime, cx);
                                }),
                            ))
                            .into_any_element(),
                    ))
                    // One switch, no essay: taps and the run reminder together.
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
                                            .child("提示音"),
                                    )
                                    .child(
                                        div().flex().gap_1p5().children(
                                            [(false, "关闭"), (true, "开启")]
                                                .into_iter()
                                                .map(|(on, label)| {
                                                    let active = sound == on;
                                                    pill(
                                                        if on {
                                                            "sound-on"
                                                        } else {
                                                            "sound-off"
                                                        },
                                                        label,
                                                        active,
                                                        cx.listener(move |this, _, _, cx| {
                                                            if this.settings.sound == on {
                                                                return;
                                                            }
                                                            this.settings.sound = on;
                                                            // Audible the moment it
                                                            // comes back on.
                                                            this.play_ui(sfx::Sfx::Click);
                                                            this.mark_settings_dirty(cx);
                                                        }),
                                                    )
                                                }),
                                        ),
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

    /// Bottom status bar: queue/batch progress on the left with the persistent
    /// stats chip, model/hint indicator on the right.
    fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
        // Denominator = finished + live unfinished rows, derived at paint time:
        // deleting or adding tasks mid-run keeps it pointing at real rows.
        let left: SharedString = if batch {
            format_batch_progress(self.batch_done, self.batch_done + pending + queued + proc)
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
        let hint_good = self.status_hint_good;
        let stats = &self.stats;
        let stats_label: SharedString = match stats.saved_sec() {
            Some(secs) => format!("已省 {}", stats::format_span_secs(secs)).into(),
            None => "统计".into(),
        };
        let stats_has_data = !stats.is_empty();

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
                    )
                    // Persistent clickable chip: the stats entry point, and the
                    // emotional payload itself (it changes as you use the app).
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
                    }),
            )
    }

    /// Floating stats panel, anchored above the status bar (`MENU_Z`).
    ///
    /// Purely additive: nothing here touches task state. Every number comes
    /// from the cached [`StatsSummary`], refreshed on open and on each finished
    /// task — never per frame.
    fn render_stats_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let stats = &self.stats;
        let saved = stats.saved_sec();
        let speed = stats.avg_speed();

        // Hover card payload, resolved once here instead of per cell.
        let hover = self.stats_hover_day.clone().map(|day| {
            let media = stats.per_day.get(&day).copied().unwrap_or(0.0);
            let tasks = stats.per_day_tasks.get(&day).copied().unwrap_or(0);
            let errs = stats.per_day_err.get(&day).copied().unwrap_or(0);
            let text = if tasks == 0 && errs == 0 {
                format!("{} · 无任务", stats::format_day_cn(&day))
            } else {
                // Split outcomes, and drop the minutes when the day has none to
                // claim (probe found no duration): "0 分" would be a number the
                // ledger never measured.
                let mut line = stats::format_day_cn(&day);
                if media > 0.0 {
                    line.push_str(&format!(" · {}", stats::format_span_secs(media)));
                }
                if tasks > 0 {
                    line.push_str(&format!(" · 成功 {tasks} 个"));
                }
                if errs > 0 {
                    line.push_str(&format!(" · 失败 {errs} 个"));
                }
                line
            };
            (day, text)
        });

        // ── year grid: 12 month blocks. Horizontal = months left to right;
        // each month fills 7-day columns in order (1–7, 8–14, …), so a month
        // spans 4 full columns plus a partial 5th (28–31 days). Rows are the
        // position within those groups, NOT fixed weekdays — day 1 is never
        // guaranteed to be a Monday. The 12-month frame always paints — it is
        // the canvas the year fills in — but a cell only exists for a day this
        // install actually lived through: before the ledger began there is no
        // "did nothing" to report, and after today there is nothing to report
        // yet. (This is the `first_day` the summary has always carried.) ──
        let today = crashlog::local_day_ymd();
        // A record stamped in the future (clock moved back) must not widen the
        // window: fall back to today, which paints a single cell at worst.
        let range_start = stats
            .first_day
            .as_deref()
            .filter(|d| *d <= today.as_str())
            .unwrap_or(today.as_str())
            .to_string();
        let year: i64 = today[..4].parse().unwrap_or(0);
        let mut cells: Vec<gpui::AnyElement> = Vec::new();
        let mut month_labels: Vec<gpui::AnyElement> = Vec::new();
        let mut hover_cell: Option<(usize, usize)> = None;
        let mut col_cursor = 0usize;
        for m in 1..=12u32 {
            let dim = stats::days_in_month(year, m) as usize;
            if dim == 0 {
                continue;
            }
            let first_col = col_cursor;
            // The layout guarantees every month ≥4 columns, so its label can
            // never collide with a neighbour's.
            let label = div()
                .absolute()
                .left(px(first_col as f32 * (STATS_CELL + STATS_GAP)))
                .text_xs()
                .text_color(MUTED)
                .whitespace_nowrap()
                .child(format!("{m} 月"));
            month_labels.push(label.into_any_element());
            for c in 0..dim.div_ceil(7) {
                for row in 0..7usize {
                    let day_num = c * 7 + row + 1;
                    if day_num > dim {
                        continue;
                    }
                    let day = format!("{year:04}-{m:02}-{day_num:02}");
                    // Frame vs data: a day the ledger cannot speak about still
                    // holds its place in the grid, but it is not a cell anyone
                    // can hover, and it never claims "nothing was done".
                    let in_range =
                        day.as_str() >= range_start.as_str() && day.as_str() <= today.as_str();
                    if in_range && self.stats_hover_day.as_deref() == Some(day.as_str()) {
                        hover_cell = Some((first_col + c, row));
                    }
                    let media = stats.per_day.get(&day).copied().unwrap_or(0.0);
                    let color = if !in_range {
                        STATS_GHOST
                    } else if media > 0.0 {
                        stats_level_color(media)
                    } else {
                        STATS_L0
                    };
                    let cell = div()
                        .absolute()
                        .left(px((first_col + c) as f32 * (STATS_CELL + STATS_GAP)))
                        .top(px(row as f32 * (STATS_CELL + STATS_GAP)))
                        .size(px(STATS_CELL))
                        // Explicit radius: `rounded_sm` resolves larger
                        // than half of a 7px box, which turns the cell
                        // into a circle — dot-matrix, not GitHub.
                        .rounded(px(1.5))
                        .bg(color);
                    cells.push(if in_range {
                        cell
                            // Stateful: on_hover in this gpui lives on stateful
                            // elements only. Day strings are unique per cell.
                            .id(SharedString::from(format!("stat-cell-{day}")))
                            .on_hover(cx.listener(move |this, over: &bool, _, cx| {
                                let next = over.then(|| day.clone());
                                if this.stats_hover_day != next {
                                    this.stats_hover_day = next;
                                    cx.notify();
                                }
                            }))
                            .into_any_element()
                    } else {
                        cell.into_any_element()
                    });
                }
            }
            col_cursor += dim.div_ceil(7);
        }
        let grid_w = (col_cursor as f32 * (STATS_CELL + STATS_GAP) - STATS_GAP).max(0.0);

        // Early on the grid is mostly canvas. Say where the record starts rather
        // than letting a blank year read as silence — and retire the line once
        // the span is wide enough to speak for itself.
        let early_caption = stats
            .first_day
            .as_deref()
            .and_then(|d| stats::days_between(d, &today))
            .filter(|span| (0..84).contains(span))
            .map(|span| {
                format!(
                    "记录从 {} 开始 · 已积累 {} 天",
                    stats::format_day_cn(&range_start),
                    span + 1
                )
            });

        // One shared hover card for the whole grid: 371 per-cell tooltips would
        // destroy/recreate on every cell boundary and flicker.
        let hover_card = hover.zip(hover_cell).map(|((_day, text), (col, row))| {
            // Right-edge columns flip the card so it cannot leave the panel.
            let right_align = col + 16 >= col_cursor;
            div()
                .absolute()
                .when(right_align, |el| {
                    el.right(px((col_cursor.saturating_sub(1 + col) * 9) as f32))
                })
                .when(!right_align, |el| el.left(px((col * 9) as f32)))
                .when(row == 0, |el| el.top(px(STATS_CELL + 4.0)))
                .when(row > 0, |el| {
                    el.bottom(px(STATS_GRID_H - row as f32 * 9.0 + 4.0))
                })
                .rounded_md()
                .px_2()
                .py_1()
                .bg(hsla(0.0, 0.0, 0.13, 0.94))
                .text_xs()
                .text_color(hsla(0.0, 0.0, 1.0, 0.96))
                .whitespace_nowrap()
                .child(text)
        });

        let metric = |label: &'static str, value: String| -> gpui::AnyElement {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(div().text_xs().text_color(MUTED).child(label))
                .child(
                    div()
                        .text_sm()
                        .text_color(TEXT)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .truncate()
                        .child(value),
                )
                .into_any_element()
        };
        let row = |label: &'static str, value: String| -> gpui::AnyElement {
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .py_0p5()
                .child(div().text_xs().text_color(MUTED).child(label))
                .child(
                    div()
                        .text_xs()
                        .text_color(TEXT)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(value),
                )
                .into_any_element()
        };
        let legend = div()
            .flex()
            .items_center()
            .gap_1()
            .text_xs()
            .text_color(MUTED)
            .child("少")
            .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L0))
            .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L1))
            .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L2))
            .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L3))
            .child(div().size(px(8.)).rounded(px(2.)).bg(STATS_L4))
            .child("多");

        let mut panel = div()
            .absolute()
            .left(px(14.))
            .bottom(px(34.))
            .w(px(STATS_PANEL_W))
            .flex()
            .flex_col()
            .gap_3()
            .p_4()
            .rounded_lg()
            .border_1()
            .border_color(LINE)
            .bg(PANEL)
            .shadow(popover_menu_shadow())
            .occlude()
            .child(
                div()
                    .text_sm()
                    .text_color(TEXT)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("统计"),
            );

        if stats.is_empty() {
            // An empty panel must not be a wall of zeros — that reads as an
            // accusation, not an invitation.
            panel = panel.child(
                div()
                    .py_2()
                    .text_xs()
                    .text_color(MUTED)
                    .child("完成第一个任务后，这里开始记账。"),
            );
        } else {
            let mut rows: Vec<gpui::AnyElement> = Vec::new();
            rows.push(row(
                "完成任务",
                if stats.tasks_err == 0 {
                    format!("{} 个 · 全部成功", stats.tasks_ok)
                } else {
                    format!(
                        "{} 个 · 成功 {} · 失败 {}",
                        stats.tasks_total(),
                        stats.tasks_ok,
                        stats.tasks_err
                    )
                },
            ));
            if stats.cues > 0 {
                rows.push(row(
                    "输出文本",
                    format!("{} 行", format_thousands(stats.cues)),
                ));
            }
            if !stats.langs.is_empty() {
                let lang_label = |id: &str| {
                    source_language_by_id(id)
                        .map(|l| l.label.to_string())
                        .unwrap_or_else(|| id.to_string())
                };
                let mut line = format!("{} {}", lang_label(&stats.langs[0].0), stats.langs[0].1);
                if let Some((id, n)) = stats.langs.get(1) {
                    line.push_str(&format!(" · {} {}", lang_label(id), n));
                }
                if stats.langs.len() > 2 {
                    line.push_str(&format!(" · 等 {} 种", stats.langs.len()));
                }
                rows.push(row("语种", line));
            }
            if stats.sep_tasks > 0 {
                rows.push(row("人声分离", format!("{} 个任务", stats.sep_tasks)));
            }
            if let Some(m) = stats.longest_media_sec {
                rows.push(row("最长一次", stats::format_span_secs(m)));
            }
            if let Some(f) = stats.fastest_speed {
                rows.push(row("最快一次", format!("{f:.1}× 实时")));
            }

            panel = panel
                // Hero is cumulative by design and never follows a time filter:
                // the accumulation IS the value, and no "0 分" until a claim
                // can actually be made.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().text_xs().text_color(MUTED).child("累计省下"))
                        .child(
                            div()
                                .text_size(px(20.))
                                .text_color(TEXT)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(match saved {
                                    Some(s) => stats::format_span_secs(s),
                                    None => "—".to_string(),
                                }),
                        )
                        // The number is a duration; this is what it buys. Kept
                        // to the unit the user can feel, never a second figure.
                        .when_some(saved.and_then(saved_tale), |el, tale| {
                            el.child(div().text_xs().text_color(MUTED_SOFT).child(tale))
                        }),
                )
                .child(
                    div().flex().gap_3().child(
                        div()
                            .flex()
                            .flex_1()
                            .gap_3()
                            .child(metric(
                                "素材总时长",
                                (stats.media_sec > 0.0)
                                    .then(|| stats::format_span_secs(stats.media_sec))
                                    .unwrap_or_else(|| "—".into()),
                            ))
                            .child(metric(
                                "机器耗时",
                                (stats.process_ms > 0)
                                    .then(|| stats::format_span_secs(stats.process_ms as f64 / 1000.0))
                                    .unwrap_or_else(|| "—".into()),
                            ))
                            .child(metric(
                                "平均速度",
                                speed.map_or_else(|| "—".into(), |f| format!("{f:.1}× 实时")),
                            )),
                    ),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(LINE)
                        .pt_3()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(TEXT)
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .child("每天处理的素材时长"),
                                )
                                .child(legend),
                        )
                        // Absolute children inside a fixed-size relative box:
                        // the pitch (9px) is the single source of truth for
                        // cells, labels, and the hover card alike.
                        .child(
                            div()
                                .relative()
                                .h(px(14.))
                                .w(px(grid_w))
                                .children(month_labels),
                        )
                        .child(
                            div()
                                .relative()
                                .w(px(grid_w))
                                .h(px(STATS_GRID_H))
                                .children(cells)
                                .children(hover_card),
                        )
                        .when_some(early_caption, |el, caption| {
                            el.child(
                                div()
                                    .text_xs()
                                    .text_color(MUTED_SOFT)
                                    .child(caption),
                            )
                        }),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(LINE)
                        .pt_2()
                        .flex()
                        .flex_col()
                        .children(rows),
                );
        }

        deferred(panel).with_priority(MENU_Z)
    }

    fn render_empty_wave(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.empty_wave_clock.elapsed().as_secs_f32();
        let heights = empty_wave_heights(t, self.empty_wave_smooth_x, self.empty_wave_amp);
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

const EMPTY_WAVE_BARS: usize = 64;
const EMPTY_WAVE_MAX_H: f32 = 78.0;
const EMPTY_WAVE_FLAT_H: f32 = 5.0;
const EMPTY_WAVE_SIGMA: f32 = 0.09;

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

// ── Stats panel shared bits (module-level: the panel methods read them bare) ──

/// Stats panel width, sized around the year grid: 12 month blocks span 59–60
/// columns (≈538px worst case), plus the panel's own padding.
const STATS_PANEL_W: f32 = 580.;
/// Cell size and pitch. 7px is the smallest that still reads as a *cell*
/// rather than noise; the width is the only thing that can buy more.
const STATS_CELL: f32 = 7.;
const STATS_GAP: f32 = 2.;
/// Seven weekday rows, fixed.
const STATS_GRID_H: f32 = 7. * STATS_CELL + 6. * STATS_GAP;

/// Colour for a day that saw work. Thresholds are **absolute** (15 min / 1 h /
/// 3 h): scaling them against the busiest day would make the same shade mean
/// different things on different screens, which is the one thing a heat map
/// must never do.
fn stats_level_color(media_sec: f64) -> Rgba {
    if media_sec >= 3.0 * 3600.0 {
        STATS_L4
    } else if media_sec >= 3600.0 {
        STATS_L3
    } else if media_sec >= 15.0 * 60.0 {
        STATS_L2
    } else {
        STATS_L1
    }
}

/// A felt yardstick for the hero number — "12 分" is a stopwatch reading, this
/// is what it buys. Deliberately coarse and never user-facing as a conversion:
/// the point is the shape of the amount, not a second precision. `None` under
/// ten minutes, where every comparison would sound like flattery.
fn saved_tale(secs: f64) -> Option<&'static str> {
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
fn count_output_lines(path: &std::path::Path) -> u32 {
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
fn format_thousands(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
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

// ── Language select (shared list chip + settings field) ──────────────────────

/// Where a language pick should be written.
#[derive(Clone)]
enum LangSelectTarget {
    Task(String),
    Settings,
}

/// Floating menu geometry under the trigger.
#[derive(Clone, Copy)]
enum LangMenuLayout {
    /// Compact chip under the task-row language control.
    Chip,
    /// Stretch to the settings field width.
    FullWidth,
}
