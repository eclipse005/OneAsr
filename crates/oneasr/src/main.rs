//! OneAsr — light-themed batch list for local MOSS transcription.

mod assets;
mod hotword_input;
mod sfx;
mod shell;
mod theme;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use gpui::{
    div, hsla, linear, percentage, point, prelude::*, px, size, svg, Animation, AnimationExt as _,
    App, Application, Bounds, BoxShadow, ClickEvent, Context, Entity, ExternalPaths, KeyBinding,
    SharedString, Timer, Transformation, Window, WindowBounds, WindowOptions,
};
use oneasr_core::{
    accept_input_path, check_model_dir, demote_current_thread, empty_state_subtitle,
    empty_state_title, format_queue_status, init_runtime, next_queue_seq,
    process_media_file_with_progress, probe_duration_sec, resolve_app_root, unload_session,
    AsrStage, DurationState, Settings, Task, TaskStatus,
};

use hotword_input::{
    Backspace, Copy, Cut, Delete, End, Home, HotwordInput, Left, Paste, Right, SelectAll,
    SelectLeft, SelectRight,
};
use theme::{
    ACCENT, ACCENT_MIST, ACCENT_SOFT, BG, DANGER, DANGER_SOFT, LINE, LINE_SOFT, LOGO, MEDIA_PLATE,
    MUTED, MUTED_SOFT, PANEL, ROW_HOVER, TEXT, WARN,
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
    // Cap rayon before any MOSS load so the UI thread keeps a free core.
    init_runtime();

    Application::new()
        .with_assets(assets::AppAssets::new())
        .run(|cx: &mut App| {
            // Hotword field key bindings (Windows: ctrl; also cmd for consistency).
            cx.bind_keys([
                KeyBinding::new("backspace", Backspace, Some("HotwordInput")),
                KeyBinding::new("delete", Delete, Some("HotwordInput")),
                KeyBinding::new("left", Left, Some("HotwordInput")),
                KeyBinding::new("right", Right, Some("HotwordInput")),
                KeyBinding::new("shift-left", SelectLeft, Some("HotwordInput")),
                KeyBinding::new("shift-right", SelectRight, Some("HotwordInput")),
                KeyBinding::new("ctrl-a", SelectAll, Some("HotwordInput")),
                KeyBinding::new("cmd-a", SelectAll, Some("HotwordInput")),
                KeyBinding::new("ctrl-v", Paste, Some("HotwordInput")),
                KeyBinding::new("cmd-v", Paste, Some("HotwordInput")),
                KeyBinding::new("ctrl-c", Copy, Some("HotwordInput")),
                KeyBinding::new("cmd-c", Copy, Some("HotwordInput")),
                KeyBinding::new("ctrl-x", Cut, Some("HotwordInput")),
                KeyBinding::new("cmd-x", Cut, Some("HotwordInput")),
                KeyBinding::new("home", Home, Some("HotwordInput")),
                KeyBinding::new("end", End, Some("HotwordInput")),
            ]);

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
    /// Desired drawer state (open / closed).
    settings_open: bool,
    /// Drawer slide progress source → target (0 = hidden, 1 = open).
    settings_from: f32,
    settings_to: f32,
    settings_anim_t0: Instant,
    /// Row whose action icons are fading in.
    hover_row: Option<String>,
    hover_t0: Instant,
    tasks: Vec<Task>,
    batch_mode: bool,
    busy: bool,
    picking: bool,
    model_status: ModelStatus,
    model_error: Option<SharedString>,
    /// Soft status-bar hint (no toast). Auto-clears after a few seconds.
    status_hint: Option<SharedString>,
    status_hint_until: Option<Instant>,
    /// Advances while any task is probing duration (drives soft spinner).
    ui_phase: u8,
    /// Live stage label for the row currently Processing (from worker Progress).
    active_stage: Option<(String, SharedString)>,
    hotword_input: Entity<HotwordInput>,
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
                            let result = run_task(&path, &name, &settings, move |stage| {
                                let _ = ptx.send(WorkerMsg::Progress {
                                    id: id_for_progress.clone(),
                                    stage: SharedString::from(stage.label()),
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
        let hotword_input =
            cx.new(|cx| HotwordInput::new(cx, &settings.hotwords));

        let mut app = Self {
            settings,
            settings_open: false,
            settings_from: 0.0,
            settings_to: 0.0,
            settings_anim_t0: Instant::now(),
            hover_row: None,
            hover_t0: Instant::now(),
            tasks: Vec::new(),
            batch_mode: false,
            busy: false,
            picking: false,
            model_status: ModelStatus::NotReady,
            model_error: None,
            status_hint: None,
            status_hint_until: None,
            ui_phase: 0,
            active_stage: None,
            hotword_input,
            tx: tx.clone(),
            rx,
            job_tx,
        };

        // Probe files only — never load multi-GB weights on startup.
        app.refresh_model_probe();
        app
    }

    /// Pull hotword text from the real input field into settings (before ASR).
    fn sync_hotwords_from_ui(&mut self, cx: &mut Context<Self>) {
        let text = self.hotword_input.read(cx).text();
        self.settings.hotwords = text;
    }

    /// Fast FS check for status bar. Does not touch GPU / weights.
    fn refresh_model_probe(&mut self) {
        match check_model_dir(&self.settings.model_dir) {
            Ok(()) => {
                self.model_status = ModelStatus::Ready;
                self.model_error = None;
            }
            Err(e) => {
                self.model_status = ModelStatus::NotReady;
                self.model_error = Some(e.to_string().into());
            }
        }
    }

    /// Drop any in-memory session and re-probe the configured folder.
    fn reset_model_config(&mut self, cx: &mut Context<Self>) {
        unload_session();
        self.refresh_model_probe();
        cx.notify();
    }

    /// Sync hotwords from UI, write `settings.json`, re-check model files.
    fn save_settings(&mut self, cx: &mut Context<Self>) {
        self.sync_hotwords_from_ui(cx);
        match self.settings.save() {
            Ok(_) => {
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
                    self.settings.model_dir = dir;
                    // Probe immediately after pick (no separate “re-detect” button).
                    self.reset_model_config(cx);
                    // Persist path so next launch keeps the choice.
                    if let Err(e) = self.settings.save() {
                        self.flash_hint(format!("目录已更新，但保存失败: {e}"), cx);
                    } else {
                        self.flash_hint(
                            match self.model_status {
                                ModelStatus::Ready => "模型目录已更新 · 模型就绪",
                                ModelStatus::NotReady => "模型目录已更新 · 模型未就绪",
                            },
                            cx,
                        );
                    }
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
                    // Corner status stays file-probe only; load errors belong on the task row.
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
        let start = self.settings.model_dir.clone();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title("选择模型目录");
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::ModelDirPicked(dir));
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
            self.tasks.push(task);
        }
        // Feedback = list itself (no toast).
        cx.notify();
    }

    fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        let open = !self.settings_open;
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
        // Drawer slide + processing status dots ("处理中...").
        let drawer = (self.settings_progress() - self.settings_to).abs() > 0.002;
        let processing = self
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Processing);
        drawer || processing
    }

    /// Brief message in the bottom status bar (replaces toast).
    fn flash_hint(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status_hint = Some(msg.into());
        self.status_hint_until = Some(Instant::now() + Duration::from_secs(3));
        cx.notify();
    }

    /// Model / hotwords gate for starting work. Does **not** block when another
    /// job is running — those requests join the FIFO queue instead.
    fn ensure_can_start(&mut self, cx: &mut Context<Self>) -> bool {
        self.refresh_model_probe();
        if self.model_status == ModelStatus::NotReady {
            self.flash_hint("模型未就绪，请在设置中选择完整模型目录", cx);
            if !self.settings_open {
                self.toggle_settings(cx);
            }
            return false;
        }
        self.sync_hotwords_from_ui(cx);
        if self.settings.can_start().is_err() {
            self.flash_hint("已开启热词：请至少填写一个", cx);
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
            if matches!(t.status, TaskStatus::Pending | TaskStatus::Error) {
                t.status = TaskStatus::Queued;
                t.queue_seq = Some(next_queue_seq());
                t.error = None;
                enqueued += 1;
            }
        }
        if enqueued == 0 {
            if self.tasks.is_empty() {
                self.flash_hint("请先添加音视频文件", cx);
            } else if self.tasks.iter().any(|t| t.status == TaskStatus::Processing)
                || self.tasks.iter().any(|t| t.status == TaskStatus::Queued)
            {
                self.flash_hint("任务已在处理或排队中", cx);
            } else {
                self.flash_hint("没有可开始的任务", cx);
            }
            return;
        }
        sfx::play(sfx::Sfx::Click);
        self.batch_mode = true;
        self.try_start_next(cx);
        cx.notify();
    }

    fn start_one(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.ensure_can_start(cx) {
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
                .filter(|t| t.status == TaskStatus::Queued)
                .collect();
            queued.sort_by_key(|t| t.queue_seq.unwrap_or(u64::MAX));
            queued.first().map(|t| t.id.clone())
        };
        match next_id {
            Some(id) => self.launch_task(&id, cx),
            None => {
                self.batch_mode = false;
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
        }
        sfx::play(sfx::Sfx::Click);
        self.tasks.retain(|t| t.id != id);
        cx.notify();
    }

    /// Clear the whole list. Keeps the active Processing row if any.
    fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.tasks.is_empty() {
            self.flash_hint("列表已空", cx);
            return;
        }
        let had_proc = self
            .tasks
            .iter()
            .any(|t| t.status == TaskStatus::Processing);
        sfx::play(sfx::Sfx::Click);
        self.tasks
            .retain(|t| t.status == TaskStatus::Processing);
        if !had_proc {
            self.batch_mode = false;
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
    on_stage: impl FnMut(AsrStage),
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
                            .m_3()
                            .rounded_xl()
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
        // Soften primary while model files aren't ready (still clickable → hint).
        let start_emphasized = self.model_status == ModelStatus::Ready;

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
                    .gap_3()
                    .child(app_logo())
                    .child(
                        div()
                            .text_lg()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(TEXT)
                            .child("OneAsr"),
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
                        start_emphasized,
                        cx.listener(|this, _, _, cx| this.start_all(cx)),
                    ))
                    .child(btn(
                        "清空",
                        BtnKind::Secondary,
                        true,
                        cx.listener(|this, _, _, cx| this.clear_all(cx)),
                    ))
                    .child(settings_gear_btn(
                        settings_open,
                        cx.listener(|this, _, _, cx| this.toggle_settings(cx)),
                    )),
            )
    }

    fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let count = self.tasks.len();
        if count == 0 {
            // Model loading stays in the status bar only — center is empty list + icon.
            let title = empty_state_title(false);
            let failed = self.model_status == ModelStatus::NotReady;
            let subtitle = empty_state_subtitle(false, failed);
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .px_6()
                        .child(empty_state_icon())
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
                                .child(subtitle),
                        ),
                )
                .into_any_element();
        }

        let rows: Vec<Task> = self.tasks.clone();
        let phase = self.ui_phase;
        let hover_id = self.hover_row.clone();
        let active_stage = self.active_stage.clone();
        // Snapshot ranks for 排队中#n labels.
        let queue_ranks: Vec<(String, usize)> = {
            let mut q: Vec<&Task> = rows
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
            .children(rows.into_iter().enumerate().map(move |(ix, task)| {
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
                let primary_enabled = if done_with_out {
                    true
                } else {
                    matches!(task.status, TaskStatus::Pending | TaskStatus::Error)
                };
                let can_delete = !task.status.locks_row_actions();
                let err = task.error.clone();
                let name = task.name.clone();
                let name_tip = task.name.clone();
                let row_id = task.id.clone();
                let row_id_hover = task.id.clone();
                let size_l = task.size_label();
                let duration = task.duration;
                let status = task.status;
                let is_video = is_video_format(&task.format);
                let is_hovered = hover_id.as_ref() == Some(&task.id);
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
                let (status_label, status_color) =
                    status_line(status, phase, qn, stage_for_row);
                let meta_left = format!("{size_l}  ·  {dur}");

                let row_bg = if is_hovered {
                    ROW_HOVER
                } else if ix % 2 == 1 {
                    // Ultra-light zebra for scanability.
                    gpui::Rgba {
                        r: 0xfa as f32 / 255.0,
                        g: 0xfb as f32 / 255.0,
                        b: 0xfc as f32 / 255.0,
                        a: 1.0,
                    }
                } else {
                    PANEL
                };

                div()
                    .id(SharedString::from(format!("task-{row_id}")))
                    .px_4()
                    .py_3()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .border_b_1()
                    .border_color(LINE_SOFT)
                    .bg(row_bg)
                    .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                        if *hovered {
                            this.hover_row = Some(row_id_hover.clone());
                            this.hover_t0 = Instant::now();
                        } else if this.hover_row.as_ref() == Some(&row_id_hover) {
                            this.hover_row = None;
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .w_full()
                            .min_w_0()
                            .gap_3()
                            // Media plate + status accent border (status lives here + meta line).
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
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("name-{row_id}")))
                                            .w_full()
                                            .overflow_hidden()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .whitespace_nowrap()
                                            .text_ellipsis()
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
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .min_w_0()
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED_SOFT)
                                                    .whitespace_nowrap()
                                                    .child(meta_left),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED_SOFT)
                                                    .child("·"),
                                            )
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
                            // Two fixed slots: primary (开始 | 打开字幕) + 删除 — gray when disabled.
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
                    )
                    .when(err.is_some(), |el| {
                        el.child(
                            div()
                                .mt_1()
                                .ml(px(48.))
                                .px_2()
                                .py_1()
                                .rounded_md()
                                .bg(DANGER_SOFT)
                                .text_xs()
                                .text_color(DANGER)
                                .child(err.unwrap_or_default()),
                        )
                    })
            }))
            .into_any_element()
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let use_hot = self.settings.use_hotwords;
        let show_spk = self.settings.export_show_speaker;
        let hotwords_now = self.hotword_input.read(cx).text();
        let backend = self.settings.backend.clone();
        let model = self.settings.model_dir.display().to_string();
        let model_tip = model.clone();
        let hotword_field = self.hotword_input.clone();

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
                    .text_base()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT)
                    .child("设置"),
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
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(TEXT)
                                    .child("模型目录"),
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
                                            .child("使用热词"),
                                    )
                                    .child(toggle(
                                        "hot",
                                        use_hot,
                                        cx.listener(|this, _, _, cx| {
                                            this.settings.use_hotwords =
                                                !this.settings.use_hotwords;
                                            cx.notify();
                                        }),
                                    )),
                            )
                            .child(hotword_field)
                            .when(use_hot && hotwords_now.trim().is_empty(), |el| {
                                el.child(
                                    div()
                                        .text_xs()
                                        .text_color(DANGER)
                                        .child("请至少填写一个热词"),
                                )
                            })
                            .into_any_element(),
                    ))
                    .child(section(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("导出显示说话人"),
                            )
                            .child(toggle(
                                "spk",
                                show_spk,
                                cx.listener(|this, _, _, cx| {
                                    this.settings.export_show_speaker =
                                        !this.settings.export_show_speaker;
                                    cx.notify();
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
                                    ["auto", "cuda", "cpu"].into_iter().map(|b| {
                                        let active = backend == b;
                                        btn(
                                            b,
                                            if active {
                                                BtnKind::Primary
                                            } else {
                                                BtnKind::Secondary
                                            },
                                            true,
                                            cx.listener(move |this, _, _, cx| {
                                                if this.settings.backend != b {
                                                    this.settings.backend = b.into();
                                                    unload_session();
                                                    this.refresh_model_probe();
                                                    cx.notify();
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
                    .justify_end()
                    .child(
                        div().flex_shrink_0().child(btn(
                            "保存设置",
                            BtnKind::Primary,
                            true,
                            cx.listener(|this, _, _, cx| this.save_settings(cx)),
                        )),
                    ),
            )
    }

    fn render_status_bar(&self) -> impl IntoElement {
        let total = self.tasks.len();
        let pending = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
            .count();
        let queued = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Queued)
            .count();
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
        let proc = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Processing)
            .count();
        let queue = format_queue_status(total, pending, queued, proc, done, err);
        let batch = self.batch_mode;
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
            // Left: queue only (never interaction errors).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .child(queue)
                    .when(batch, |el| {
                        el.child(div().text_color(ACCENT).child("顺序处理中"))
                    }),
            )
            // Right: model probe (常驻) OR transient interaction hint (几秒后恢复).
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_w_0()
                    .max_w(px(360.))
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .child(match hint {
                        Some(h) => div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .bg(gpui::Rgba {
                                r: 0xff as f32 / 255.0,
                                g: 0xf7 as f32 / 255.0,
                                b: 0xed as f32 / 255.0,
                                a: 1.0,
                            })
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

/// Simple brand mark: rounded tile + waveform bars.
fn app_logo() -> impl IntoElement {
    div()
        .size(px(32.))
        .rounded_lg()
        .bg(LOGO)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .child(logo_bar(10.))
        .child(logo_bar(16.))
        .child(logo_bar(12.))
        .child(logo_bar(18.))
        .child(logo_bar(8.))
}

fn logo_bar(h: f32) -> impl IntoElement {
    div()
        .w(px(2.5))
        .h(px(h))
        .rounded_full()
        .bg(gpui::rgb(0xffffff))
}

#[derive(Clone, Copy)]
enum BtnKind {
    /// Sole solid CTA (全部开始).
    Primary,
    /// Outlined secondary (添加 / 清空 / 后端选项).
    Secondary,
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

/// Primary CTA: full brand when model files are ready; softer when not.
fn btn_cta(
    label: &str,
    emphasized: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("cta-{label}-{emphasized}"));
    let (bg, fg, border, opacity) = if emphasized {
        (ACCENT, PANEL, ACCENT, 1.0)
    } else {
        // Still branded, not “dead gray” — but clearly not the moment to start.
        (ACCENT, PANEL, ACCENT, 0.48)
    };
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(bg)
        .text_color(fg)
        .border_1()
        .border_color(border)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .opacity(opacity)
        .cursor_pointer()
        .hover(|s| s.opacity(if emphasized { 0.92 } else { 0.62 }))
        .on_click(on_click)
        .child(label.to_string())
}

fn btn(
    label: &str,
    kind: BtnKind,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("btn-{label}-{}-{enabled}", kind as u8));
    // Primary disabled keeps brand teal (softer).
    let (bg, fg, border, opacity) = match (kind, enabled) {
        (BtnKind::Primary, true) => (ACCENT, PANEL, ACCENT, 1.0),
        (BtnKind::Primary, false) => (ACCENT, PANEL, ACCENT, 0.42),
        (BtnKind::Secondary, true) => (PANEL, TEXT, LINE, 1.0),
        (BtnKind::Secondary, false) => (PANEL, MUTED_SOFT, LINE, 1.0),
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
            BtnKind::Secondary => gpui::FontWeight::NORMAL,
        })
        .opacity(opacity)
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                BtnKind::Primary => s.opacity(0.92),
                BtnKind::Secondary => s.border_color(ACCENT),
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

/// Meta-line status fragment (text + color). Processing animates trailing dots.
fn status_line(
    status: TaskStatus,
    phase: u8,
    queue_n: Option<usize>,
    stage: Option<&str>,
) -> (String, gpui::Rgba) {
    match status {
        TaskStatus::Pending => ("待处理".into(), MUTED),
        TaskStatus::Queued => {
            let n = queue_n.unwrap_or(0);
            (format!("排队中#{n}"), MUTED)
        }
        TaskStatus::Processing => {
            let base = stage.unwrap_or("处理中");
            let dots = match (phase / 4) % 3 {
                0 => ".",
                1 => "..",
                _ => "...",
            };
            (format!("{base}{dots}"), WARN)
        }
        TaskStatus::Done => ("完成".into(), ACCENT),
        TaskStatus::Error => ("错误".into(), DANGER),
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

/// Soft empty-state mark: mist circle + waveform bars (drop hint).
fn empty_state_icon() -> impl IntoElement {
    div()
        .size(px(64.))
        .rounded_full()
        .bg(ACCENT_MIST)
        .flex()
        .items_center()
        .justify_center()
        .gap_1()
        .child(empty_bar(12., 0.45))
        .child(empty_bar(22., 0.7))
        .child(empty_bar(16., 0.55))
        .child(empty_bar(26., 0.85))
        .child(empty_bar(14., 0.5))
}

fn empty_bar(h: f32, opacity: f32) -> impl IntoElement {
    div()
        .w(px(3.))
        .h(px(h))
        .rounded_full()
        .bg(ACCENT)
        .opacity(opacity)
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
                IconKind::Folder => s.bg(ACCENT).border_color(ACCENT),
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

fn toggle(
    id: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(40.))
        .h(px(22.))
        .rounded_full()
        .bg(if on { ACCENT } else { LINE })
        .flex()
        .items_center()
        .px_1()
        .cursor_pointer()
        .on_click(on_click)
        .child(
            div()
                .size(px(16.))
                .rounded_full()
                .bg(PANEL)
                .when(on, |el| el.ml_auto()),
        )
}
