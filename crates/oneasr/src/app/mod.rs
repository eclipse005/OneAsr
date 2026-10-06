//! Application state and logic for the OneAsr window.
//!
//! The window's state (`OneAsrApp`) lives here with `new()`; everything else is
//! grouped by concern into sibling modules. The view layer sits in [`ui`] as a
//! **child module** on purpose: view code reads `OneAsrApp`'s private fields and
//! calls its private methods, and Rust privacy lets descendants of the defining
//! module do that without widening anything to `pub(crate)`.

use crate::app::prelude::*;
use crate::app::ui::transcript_card::TranscriptCardView;

pub(crate) mod downloads;
pub(crate) mod overlays;
pub(crate) mod prelude;
pub(crate) mod rows;
pub(crate) mod run_control;
pub(crate) mod settings;
pub(crate) mod stats;
pub(crate) mod task;
pub(crate) mod transcript;
pub(crate) mod transcript_ui;
pub(crate) mod ui;
pub(crate) mod worker;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModelStatus {
    /// Directory has required model files.
    Ready,
    /// Missing / incomplete model files.
    NotReady,
}

pub(crate) struct OneAsrApp {
    /// Root focus so Escape / key bindings reach the app (menus, dismiss).
    pub(crate) focus_handle: FocusHandle,
    /// Machine-resolved UI font (primary + CJK DirectWrite fallbacks).
    pub(crate) ui_font: ui_font::UiFontPlan,
    settings: Settings,
    /// Unsaved settings edits (backend / paths).
    settings_dirty: bool,
    /// Settings as they were when the drawer opened. Closing the drawer
    /// without saving is a cancel, and the edits roll back to this.
    settings_snapshot: Option<Settings>,
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
    // ── 文稿卡片（悬停打开 / 点击粘住）────────────────────────────────────
    //
    // 和「用时」卡片同一套机制。`pinned` 是唯一的差别：纯悬停的卡片上按按钮是
    // 难受的，点一下芯片把它粘住，它就变成一个随时可点的面板。
    /// Task id whose transcript card is open (hover or pinned).
    transcript_card: Option<String>,
    /// Pinned by a click: survives the pointer leaving chip and card.
    transcript_card_pinned: bool,
    /// Hover candidate for the open delay (id + first-hover Instant).
    transcript_hover_since: Option<(String, Instant)>,
    /// Leave grace before an unpinned card closes.
    transcript_leave_since: Option<(String, Instant)>,
    transcript_card_from: f32,
    transcript_card_to: f32,
    transcript_card_anim_t0: Instant,
    /// One-shot result of the last card action (断句行数 / 重新读取的结果).
    transcript_card_note: Option<String>,
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
    demucs_download: Option<DownloadProgress>,
    /// Active download cancel handles.
    asr_dl_handle: Option<DownloadHandle>,
    align_dl_handle: Option<DownloadHandle>,
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
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx, job_tx) = spawn_asr_worker();
        Self::start_worker_poller(cx);

        let settings = load_settings_or_fallback();
        // 界面语言先于任何窗口构建生效（settings.json 的 ui_language）。
        oneasr_core::i18n::set_ui_lang(settings.resolved_ui_language());
        let app_root = resolve_app_root_dir();
        log_environment_snapshot(&settings, &app_root);

        let mut app = Self {
            focus_handle: cx.focus_handle(),
            ui_font: ui_font::UiFontPlan::default(),
            settings,
            settings_dirty: false,
            settings_snapshot: None,
            settings_open: false,
            settings_from: 0.0,
            settings_to: 0.0,
            settings_anim_t0: Instant::now(),
            hover_row: None,
            timing_popover: None,
            timing_hover_since: None,
            transcript_card: None,
            transcript_card_pinned: false,
            transcript_hover_since: None,
            transcript_leave_since: None,
            transcript_card_from: 0.0,
            transcript_card_to: 0.0,
            transcript_card_anim_t0: Instant::now(),
            transcript_card_note: None,
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
            demucs_ready: false,
            status_hint: None,
            status_hint_until: None,
            status_hint_good: false,
            nudge_saved: None,
            active_stage: None,
            asr_download: None,
            align_download: None,
            demucs_download: None,
            asr_dl_handle: None,
            align_dl_handle: None,
            demucs_dl_handle: None,
            stats_open: false,
            stats_hover_day: None,
            // Read the ledger once at startup; refreshed on every finished task.
            stats: oneasr_core::stats::summarize(&oneasr_core::stats::load(
                &oneasr_core::paths::data_dir(),
            )),
            tx: tx.clone(),
            rx,
            job_tx,
        };

        app.finish_startup();
        app
    }

    /// Drain the worker channel on a timer.
    ///
    /// A passive pump rather than a per-handler callback: the worker outlives
    /// any single task, so something has to collect its messages while the UI
    /// sits idle.
    fn start_worker_poller(cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                Timer::after(Duration::from_millis(80)).await;
                this.update(cx, |app, cx| app.poll_worker(cx)).ok();
            }
        })
        .detach();
    }

    /// Startup probes that need a constructed app, because they write state.
    fn finish_startup(&mut self) {
        // Probe files only — never load multi-GB weights on startup.
        self.refresh_model_probe();
    }
}

/// Long-lived ASR worker, plus the two channels the UI talks to it over.
///
/// The worker is spawned once and lives for the process — loading and dropping
/// multi-GB weights per task would cost more than it saves — and is demoted so
/// the UI thread keeps priority.
fn spawn_asr_worker() -> (Sender<WorkerMsg>, Receiver<WorkerMsg>, Sender<AsrJob>) {
    let (tx, rx) = mpsc::channel();
    let (job_tx, job_rx) = mpsc::channel::<AsrJob>();

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
                        transcript,
                    } => {
                        let id_for_progress = id.clone();
                        let ptx = worker_tx.clone();
                        let mut clock = StageClock::new();
                        // A panic inside the pipeline must not kill the shared
                        // worker (that would strand `Processing` rows forever).
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            run_task(&path, &name, &settings, transcript.as_ref(), |update| {
                                clock.note(&update);
                                let warning = update.warning.as_ref().map(SharedString::from);
                                let _ = ptx.send(WorkerMsg::Progress {
                                    id: id_for_progress.clone(),
                                    stage: SharedString::from(update.label(ui_lang())),
                                    warning,
                                });
                            })
                        }))
                        .unwrap_or_else(|payload| {
                            let message = panic_message(payload);
                            crashlog::log_error(format!("ASR worker panic (task {id}): {message}"));
                            Err(crate::i18n::task_thread_panic(&message))
                        });
                        let timing = clock.finish();
                        let _ = worker_tx.send(WorkerMsg::Finished { id, result, timing });
                    }
                }
            }
        })
        .expect("spawn ASR worker");

    (tx, rx, job_tx)
}

fn load_settings_or_fallback() -> Settings {
    let (settings, report) = Settings::load_with_report();
    log_settings_report(&report);
    settings
}

/// Log why settings were repaired or fell back to defaults. A user report of
/// "设置丢了" must be answerable from a pasted log alone.
fn log_settings_report(report: &oneasr_core::SettingsLoadReport) {
    if !report.is_notable() {
        return;
    }
    let path = report
        .config_path
        .clone()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<unknown>".into());
    if let Some(e) = &report.read_error {
        crashlog::log_error(format!(
            "settings read failed (using defaults)\n  path: {path}\n  error: {e}"
        ));
    }
    if let Some(e) = &report.parse_error {
        crashlog::log_error(format!(
            "settings parse failed (using defaults)\n  path: {path}\n  error: {e}"
        ));
    }
    for repair in &report.repairs {
        crashlog::log_warn(format!("settings repaired: {repair}"));
    }
}

/// Environment snapshot: everything support asks for in one block — install
/// location + writability (the os-error-5 class of report), the resolved data
/// directory and where it came from, ffmpeg, backend and per-model readiness.
/// Probes files only; never loads weights.
fn log_environment_snapshot(settings: &Settings, app_root: &std::path::Path) {
    let writable = match probe_writable(app_root) {
        Ok(()) => "yes".to_string(),
        Err(e) => format!("NO ({e})"),
    };
    let models_line = [
        ModelId::Qwen3Asr06B,
        ModelId::Qwen3Asr17B,
        ModelId::Qwen3Asr06BInt8,
        ModelId::Qwen3Asr17BInt8,
        // 两个对齐引擎都列：默认是 CTC，只列 Qwen 的话用户贴日志来问「为什么
        // 跑不起来」时，恰恰查不到当前那个引擎的权重在不在。
        ModelId::OmniAsrCtc300M,
        ModelId::QwenAlign06B,
        ModelId::HtdemucsFt,
    ]
    .map(|id| {
        format!(
            "{}={}",
            id.as_str(),
            if is_model_ready(id) {
                "ready"
            } else {
                "missing"
            }
        )
    })
    .join(" ");
    crashlog::log_info(format!(
        "environment:\n  settings: {}\n  app_root: {}\n  app_root writable: {writable}\n  data_root: {}\n  ffmpeg: {}\n  backend: {}\n  output_dir: {}\n  models: {models_line}",
        Settings::config_path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "<unknown>".into()),
        app_root.display(),
        oneasr_core::paths::data_root_log_line(),
        ffmpeg_source()
            .map(|source| source.to_string())
            .unwrap_or_else(|| "MISSING".into()),
        settings.backend,
        settings.output_dir.display(),
    ));
}

/// Human-readable message from a caught panic payload.
pub(crate) fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        crate::i18n::unknown_panic().into()
    }
}

pub(crate) fn run_task(
    path: &std::path::Path,
    name: &str,
    settings: &Settings,
    transcript: Option<&oneasr_core::TranscriptInput>,
    on_stage: impl FnMut(StageUpdate),
) -> Result<PathBuf, String> {
    // `bin/ffmpeg` is the usual app-root marker, but a system ffmpeg on PATH
    // leaves the install dir without one — fall back to the exe directory.
    // The pipeline's scratch dir belongs to the **data** directory, not the
    // install directory: a read-only install (system package) still runs.
    let data_root = oneasr_core::paths::data_dir();
    // Primary deliverable: {target_dir}/{stem}.srt, or .txt when SRT output is
    // switched off (real ASR, no stubs). Runs only on the dedicated worker thread.
    let run = match transcript {
        Some(t) => oneasr_core::process_media_file_with_transcript_export(
            path,
            name,
            t,
            settings,
            &data_root,
            on_stage,
            oneasr_core::ProcessExportOptions { words_json: None },
        ),
        None => process_media_file_with_progress(path, name, settings, &data_root, on_stage),
    };
    run.map_err(|e| e.to_string())
}

/// Per-frame paint snapshot of a list row. Owned so the children closure
/// does not clone `Task` (path, output path, …).
pub(crate) struct TaskRowView {
    /// 这行挂了文稿没有（`false` = 芯片是「+ 文稿」）。
    pub(crate) has_transcript: bool,
    /// 卡片这一帧该不该进渲染树（`progress > 0.01`）。开着但还在淡入的第一帧
    /// 不该进树——进去了就是一块看不见却带 `occlude()` 的遮挡层。
    pub(crate) transcript_card_visible: bool,
    /// 卡片淡入淡出进度。
    pub(crate) transcript_card_progress: f32,
    /// 卡片内容；没挂文稿时是 `None`。
    pub(crate) transcript_card: Option<TranscriptCardView>,
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
    /// `6.5 倍速` chip in the timing card header: media length ÷ this run's wall
    /// clock. `None` when either side is unusable — see `realtime_factor_label`.
    rtfx_label: Option<String>,
    /// 文稿芯片上的量级（`None` = 还没挂文稿）。
    /// 字幕比文稿旧。
    /// 卡片正为这行开着。
    /// 卡片淡入淡出进度。
    /// 卡片内容；没挂文稿时是 `None`。
    is_video: bool,
    timing: Option<TaskTiming>,
    opacity: f32,
    interactive: bool,
    queue_rank: Option<usize>,
}

#[derive(Clone)]
pub(crate) enum LangSelectTarget {
    Task(String),
    Settings,
}

#[derive(Clone, Copy)]
pub(crate) enum LangMenuLayout {
    /// Compact chip under the task-row language control.
    Chip,
    /// Stretch to the settings field width.
    FullWidth,
}
