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
    App, Application, Bounds, BoxShadow, ClickEvent, ClipboardItem, Context, Entity, ExternalPaths,
    KeyBinding, SharedString, Timer, Transformation, Window, WindowBounds, WindowOptions,
};
use oneasr_core::{
    accept_input_path, empty_state_subtitle, empty_state_title, format_queue_status, preload_model,
    process_media_file, probe_duration_sec, resolve_app_root, unload_session, DurationState,
    Settings, Task, TaskStatus,
};

use hotword_input::{
    Backspace, Copy, Cut, Delete, End, Home, HotwordInput, Left, Paste, Right, SelectAll,
    SelectLeft, SelectRight,
};
use theme::{
    ACCENT, ACCENT_MIST, ACCENT_SOFT, BG, DANGER, DANGER_SOFT, LINE, LINE_SOFT, LOGO, MUTED,
    MUTED_SOFT, PANEL, PILL_NEUTRAL_BG, PILL_NEUTRAL_FG, TEXT, WARN,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum ModelStatus {
    Loading,
    Ready,
    Failed,
}

enum WorkerMsg {
    FilesPicked(Vec<PathBuf>),
    PickCancelled,
    ModelDirPicked(PathBuf),
    ModelLoadResult(Result<(), String>),
    Probed {
        id: String,
        duration_sec: Option<f64>,
    },
    Finished {
        id: String,
        result: Result<PathBuf, String>,
    },
}

fn main() {
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

            let bounds = Bounds::centered(None, size(px(1080.), px(680.)), cx);
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
    hotword_input: Entity<HotwordInput>,
    tx: Sender<WorkerMsg>,
    rx: Receiver<WorkerMsg>,
}

impl OneAsrApp {
    fn new(cx: &mut Context<Self>) -> Self {
        let (tx, rx) = mpsc::channel();

        cx.spawn(async move |this, cx| loop {
            Timer::after(Duration::from_millis(80)).await;
            this.update(cx, |app, cx| app.poll_worker(cx)).ok();
        })
        .detach();

        let settings = Settings::default();
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
            model_status: ModelStatus::Loading,
            model_error: None,
            status_hint: None,
            status_hint_until: None,
            ui_phase: 0,
            hotword_input,
            tx: tx.clone(),
            rx,
        };

        app.begin_model_load();
        app
    }

    /// Pull hotword text from the real input field into settings (before ASR).
    fn sync_hotwords_from_ui(&mut self, cx: &mut Context<Self>) {
        let text = self.hotword_input.read(cx).text();
        self.settings.hotwords = text;
    }

    fn begin_model_load(&mut self) {
        self.model_status = ModelStatus::Loading;
        self.model_error = None;
        let model = self.settings.model_dir.clone();
        let backend = self.settings.backend.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = preload_model(&model, &backend).map_err(|e| e.to_string());
            // Always send so UI never stays stuck on Loading.
            let _ = tx.send(WorkerMsg::ModelLoadResult(result));
        });
    }

    fn request_model_reload(&mut self, cx: &mut Context<Self>) {
        unload_session();
        self.begin_model_load();
        cx.notify();
    }

    fn poll_worker(&mut self, cx: &mut Context<Self>) {
        // Soft UI phase tick: duration probe · pending breath · processing dots.
        let need_phase = self.tasks.iter().any(|t| {
            t.duration == DurationState::Probing
                || t.status == TaskStatus::Pending
                || t.status == TaskStatus::Processing
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
                    self.request_model_reload(cx);
                }
                Ok(WorkerMsg::ModelLoadResult(result)) => match result {
                    Ok(()) => {
                        self.model_status = ModelStatus::Ready;
                        self.model_error = None;
                        // No toast — status bar is the only channel.
                        cx.notify();
                    }
                    Err(e) => {
                        self.model_status = ModelStatus::Failed;
                        self.model_error = Some(e.clone().into());
                        cx.notify();
                    }
                },
                Ok(WorkerMsg::Probed { id, duration_sec }) => {
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        t.set_duration(duration_sec);
                    }
                    cx.notify();
                }
                Ok(WorkerMsg::Finished { id, result }) => {
                    self.busy = false;
                    if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
                        match result {
                            Ok(srt) => {
                                t.status = TaskStatus::Done;
                                t.output_srt = Some(srt);
                                t.error = None;
                            }
                            Err(e) => {
                                t.status = TaskStatus::Error;
                                t.error = Some(e);
                            }
                        }
                    }
                    cx.notify();
                    if self.batch_mode {
                        self.try_start_next(cx);
                    }
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

    fn actions_opacity(&self, row_id: &str) -> f32 {
        if self.hover_row.as_deref() == Some(row_id) {
            let t = (self.hover_t0.elapsed().as_secs_f32() / ROW_HOVER_ANIM_SECS).min(1.0);
            ACTIONS_DIM + (1.0 - ACTIONS_DIM) * t
        } else {
            ACTIONS_DIM
        }
    }

    fn animations_active(&self) -> bool {
        let drawer = (self.settings_progress() - self.settings_to).abs() > 0.002;
        let hover = self.hover_row.is_some()
            && self.hover_t0.elapsed().as_secs_f32() < ROW_HOVER_ANIM_SECS;
        drawer || hover
    }

    /// Brief message in the bottom status bar (replaces toast).
    fn flash_hint(&mut self, msg: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status_hint = Some(msg.into());
        self.status_hint_until = Some(Instant::now() + Duration::from_secs(3));
        cx.notify();
    }

    /// Shared preflight for start_all / start_one. Returns false if blocked.
    fn ensure_can_start(&mut self, cx: &mut Context<Self>) -> bool {
        match self.model_status {
            ModelStatus::Loading => {
                self.flash_hint("模型加载中，请稍候…", cx);
                return false;
            }
            ModelStatus::Failed => {
                self.flash_hint("模型未就绪，请在设置中检查目录与后端", cx);
                if !self.settings_open {
                    self.toggle_settings(cx);
                }
                return false;
            }
            ModelStatus::Ready => {}
        }
        if self.busy {
            self.flash_hint("正在处理其他任务，请稍候…", cx);
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
        if self.tasks.iter().all(|t| t.status != TaskStatus::Pending) {
            if self.tasks.is_empty() {
                self.flash_hint("请先添加音视频文件", cx);
            } else {
                self.flash_hint("没有待处理的任务", cx);
            }
            return;
        }
        sfx::play(sfx::Sfx::Click);
        self.batch_mode = true;
        self.try_start_next(cx);
    }

    fn start_one(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.ensure_can_start(cx) {
            return;
        }
        let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if t.status != TaskStatus::Pending && t.status != TaskStatus::Error {
            self.flash_hint("该任务当前无法开始", cx);
            return;
        }
        t.status = TaskStatus::Pending;
        t.error = None;
        self.batch_mode = false;
        sfx::play(sfx::Sfx::Click);
        self.launch_task(id, cx);
    }

    fn try_start_next(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let next_id = self
            .tasks
            .iter()
            .find(|t| t.status == TaskStatus::Pending)
            .map(|t| t.id.clone());
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
        if task.status != TaskStatus::Pending {
            return;
        }
        task.status = TaskStatus::Processing;
        task.error = None;
        self.busy = true;

        let id = task.id.clone();
        let path = task.path.clone();
        let name = task.name.clone();
        let settings = self.settings.clone();
        let tx = self.tx.clone();

        thread::spawn(move || {
            let result = run_task(&path, &name, &settings);
            let _ = tx.send(WorkerMsg::Finished { id, result });
        });
        cx.notify();
    }

    fn delete_task(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(t) = self.tasks.iter().find(|t| t.id == id) {
            if t.status == TaskStatus::Processing {
                // Processing rows hide delete; no toast.
                return;
            }
        }
        sfx::play(sfx::Sfx::Click);
        self.tasks.retain(|t| t.id != id);
        cx.notify();
    }

    /// Clear the whole queue. No-op when already empty (button stays lit).
    fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.tasks.is_empty() {
            self.flash_hint("列表已空", cx);
            return;
        }
        // Don't wipe a row that is mid-ASR.
        if self.tasks.iter().any(|t| t.status == TaskStatus::Processing) {
            self.flash_hint("有任务处理中，无法清空", cx);
            return;
        }
        sfx::play(sfx::Sfx::Click);
        self.tasks.clear();
        self.batch_mode = false;
        self.hover_row = None;
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

    fn copy_task_output(&mut self, id: &str, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.output_srt.as_ref().map(|p| p.display().to_string()))
        else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(path));
        cx.notify();
    }
}

fn run_task(path: &std::path::Path, name: &str, settings: &Settings) -> Result<PathBuf, String> {
    let app_root =
        resolve_app_root().ok_or_else(|| "找不到应用目录（需含 bin/ffmpeg.exe）".to_string())?;
    // Primary deliverable: {app_root}/output/{stem}.srt (real ASR, no stubs).
    process_media_file(path, name, settings, &app_root).map_err(|e| e.to_string())
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
const SETTINGS_W: f32 = 300.;
const DRAWER_ANIM_SECS: f32 = 0.34;
const ROW_HOVER_ANIM_SECS: f32 = 0.13;
const ACTIONS_DIM: f32 = 0.32;
/// Meta columns: size + dur + fmt + status + icon actions.
/// Actions column: 2–4 icon buttons with comfortable gap.
const ACTIONS_COL_PX: f32 = 136.;

/// Smooth deceleration (approx cubic-bezier ease-out).
fn ease_out_cubic(t: f32) -> f32 {
    let u = 1.0 - t;
    1.0 - u * u * u
}

impl OneAsrApp {
    fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings_open = self.settings_open;
        let picking = self.picking;
        // 全部开始 / 清空 stay visually lit always; handlers no-op when there's nothing to do.
        // Settings is a fixed-size gear icon (no "关闭设置" label → no toolbar shift).

        div()
            .h(px(56.))
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
                    .child(btn(
                        "全部开始",
                        BtnKind::Primary,
                        true,
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
            let loading = self.model_status == ModelStatus::Loading;
            let failed = self.model_status == ModelStatus::Failed;
            let title = empty_state_title(loading);
            let subtitle = empty_state_subtitle(loading, failed);
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
        let busy = self.busy;
        let phase = self.ui_phase;
        // Snapshot hover opacities before the move into children closure.
        let hover_opacities: Vec<(String, f32)> = rows
            .iter()
            .map(|t| (t.id.clone(), self.actions_opacity(&t.id)))
            .collect();

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
                let id_copy = task.id.clone();
                let can_start = !busy
                    && (task.status == TaskStatus::Pending || task.status == TaskStatus::Error);
                let can_delete = task.status != TaskStatus::Processing;
                let done_with_out =
                    task.status == TaskStatus::Done && task.output_srt.is_some();
                let err = task.error.clone();
                let name = task.name.clone();
                let name_tip = task.name.clone();
                let row_id = task.id.clone();
                let row_id_hover = task.id.clone();
                let size_l = task.size_label();
                let duration = task.duration;
                let status = task.status;
                let is_video = is_video_format(&task.format);
                let act_opacity = hover_opacities
                    .get(ix)
                    .map(|(_, o)| *o)
                    .unwrap_or(ACTIONS_DIM);

                // Quiet secondary line — size · duration (no format tags).
                let meta = match duration {
                    DurationState::Probing => format!("{size_l}  ·  …"),
                    DurationState::Known(_) | DurationState::Unknown => {
                        format!("{size_l}  ·  {}", duration.label())
                    }
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
                    .hover(|s| s.bg(BG))
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
                            // Media type — leading visual identity (audio / video)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .child(media_type_icon(is_video)),
                            )
                            // Title + quiet meta
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
                                            .text_xs()
                                            .text_color(MUTED_SOFT)
                                            .whitespace_nowrap()
                                            .truncate()
                                            .child(meta),
                                    ),
                            )
                            // Status near actions (not leading)
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .child(status_mark(&row_id, status, phase)),
                            )
                            // Actions — far right
                            .child(
                                div()
                                    .w(px(ACTIONS_COL_PX))
                                    .flex_shrink_0()
                                    .flex()
                                    .gap_2()
                                    .justify_end()
                                    .items_center()
                                    .opacity(act_opacity)
                                    .child(icon_btn(
                                        IconKind::Play,
                                        "开始",
                                        can_start,
                                        cx.listener(move |this, _, _, cx| {
                                            this.start_one(&id_start, cx);
                                        }),
                                    ))
                                    .when(done_with_out, |el| {
                                        el.child(icon_btn(
                                            IconKind::Folder,
                                            "打开",
                                            true,
                                            cx.listener(move |this, _, _, cx| {
                                                this.open_task_output(&id_open, cx);
                                            }),
                                        ))
                                        .child(icon_btn(
                                            IconKind::Copy,
                                            "复制",
                                            true,
                                            cx.listener(move |this, _, window, cx| {
                                                this.copy_task_output(&id_copy, window, cx);
                                            }),
                                        ))
                                    })
                                    .child(
                                        div().ml_1().child(icon_btn(
                                            IconKind::Trash,
                                            "删除",
                                            can_delete,
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
                                .mt_1()
                                .ml(px(48.)) // align under title, past media icon
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
        let hotword_field = self.hotword_input.clone();

        div()
            .w(px(SETTINGS_W))
            .h_full()
            .bg(PANEL)
            .p_4()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("设置"),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(MUTED).child("模型目录"))
                    .child(
                        div()
                            .id("model-dir")
                            .flex()
                            .items_center()
                            .rounded_lg()
                            .border_1()
                            .border_color(LINE_SOFT)
                            .bg(BG)
                            .overflow_hidden()
                            .hover(|s| s.border_color(ACCENT))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .px_3()
                                    .py_2()
                                    .text_xs()
                                    .truncate()
                                    .child(model),
                            )
                            .child(
                                div()
                                    .id("model-dir-browse")
                                    .flex_shrink_0()
                                    .h_full()
                                    .px_2()
                                    .py_2()
                                    .border_l_1()
                                    .border_color(LINE_SOFT)
                                    .cursor_pointer()
                                    .hover(|s| s.bg(ACCENT_SOFT))
                                    .child(folder_glyph())
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.pick_model_dir(cx)),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child("使用热词"))
                    .child(toggle(
                        "hot",
                        use_hot,
                        cx.listener(|this, _, _, cx| {
                            this.settings.use_hotwords = !this.settings.use_hotwords;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(MUTED).child("热词（逗号分隔）"))
                    .child(hotword_field)
                    .when(use_hot && hotwords_now.trim().is_empty(), |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(DANGER)
                                .child("已开启热词：请至少填写一个"),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_sm().child("导出显示说话人"))
                    .child(toggle(
                        "spk",
                        show_spk,
                        cx.listener(|this, _, _, cx| {
                            this.settings.export_show_speaker = !this.settings.export_show_speaker;
                            cx.notify();
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().text_xs().text_color(MUTED).child("推理后端"))
                    .child(div().flex().gap_2().children(["auto", "cuda", "cpu"].into_iter().map(
                        |b| {
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
                                        this.request_model_reload(cx);
                                    }
                                }),
                            )
                        },
                    ))),
            )
            .child(btn(
                "重新加载模型",
                BtnKind::Ghost,
                true,
                cx.listener(|this, _, _, cx| this.request_model_reload(cx)),
            ))
    }

    fn render_status_bar(&self) -> impl IntoElement {
        let total = self.tasks.len();
        let pending = self
            .tasks
            .iter()
            .filter(|t| t.status == TaskStatus::Pending)
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
        let queue = format_queue_status(total, pending, proc, done, err);
        let batch = self.batch_mode;
        let model = self.model_status;
        let model_color = match model {
            ModelStatus::Loading => WARN,
            ModelStatus::Ready => ACCENT,
            ModelStatus::Failed => DANGER,
        };
        let model_detail = self.model_error.clone().unwrap_or_default();
        let hint = self.status_hint.clone();
        let has_hint = hint.is_some();

        div()
            .h(px(32.))
            .px_4()
            .flex()
            .items_center()
            .justify_between()
            .bg(PANEL)
            .border_t_1()
            .border_color(LINE)
            .text_xs()
            .text_color(MUTED)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    // Hint temporarily replaces queue text — soft, no toast banner.
                    .child(match hint {
                        Some(h) => div()
                            .text_color(WARN)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .truncate()
                            .child(h)
                            .into_any_element(),
                        None => div().child(queue).into_any_element(),
                    })
                    .when(batch && !has_hint, |el| {
                        el.child(div().text_color(ACCENT).child("顺序处理中"))
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().size(px(7.)).rounded_full().bg(model_color))
                    .child(
                        div()
                            .text_color(model_color)
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .child({
                                match model {
                                    ModelStatus::Loading => "模型加载中",
                                    ModelStatus::Ready => "模型就绪",
                                    ModelStatus::Failed => "模型失败",
                                }
                            }),
                    )
                    .when(model == ModelStatus::Failed && !model_detail.is_empty(), |el| {
                        el.child(
                            div()
                                .max_w(px(420.))
                                .text_color(DANGER)
                                .child(model_detail),
                        )
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
    /// Outlined secondary (添加 / 设置).
    Secondary,
    /// Weak text-like action (设置).
    Ghost,
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

fn btn(
    label: &str,
    kind: BtnKind,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("btn-{label}-{}-{enabled}", kind as u8));
    // Primary disabled keeps brand teal (softer). Ghost is pure text — never low-contrast fill.
    let (bg, fg, border, opacity) = match (kind, enabled) {
        (BtnKind::Primary, true) => (ACCENT, PANEL, ACCENT, 1.0),
        (BtnKind::Primary, false) => (ACCENT, PANEL, ACCENT, 0.42),
        (BtnKind::Secondary, true) => (PANEL, TEXT, LINE, 1.0),
        (BtnKind::Secondary, false) => (PANEL, MUTED_SOFT, LINE, 1.0),
        (BtnKind::Ghost, true) => (PANEL, MUTED, PANEL, 1.0),
        (BtnKind::Ghost, false) => (PANEL, MUTED_SOFT, PANEL, 1.0),
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
            _ => gpui::FontWeight::NORMAL,
        })
        .opacity(opacity)
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                BtnKind::Primary => s.opacity(0.92),
                BtnKind::Secondary => s.border_color(ACCENT),
                BtnKind::Ghost => s.text_color(TEXT).bg(BG),
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

/// Leading media glyph: soft plate + audio waveform / video frame mark.
fn media_type_icon(is_video: bool) -> impl IntoElement {
    if is_video {
        div()
            .size(px(36.))
            .rounded_lg()
            .bg(ACCENT_MIST)
            .flex()
            .items_center()
            .justify_center()
            .child(
                // Rounded screen + play triangle (inherits no text; pure geometry).
                div()
                    .relative()
                    .w(px(18.))
                    .h(px(14.))
                    .rounded_sm()
                    .border_1()
                    .border_color(ACCENT)
                    .opacity(0.85)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_color(ACCENT)
                            .text_size(gpui::rems(0.55))
                            .child("▶"),
                    ),
            )
            .into_any_element()
    } else {
        div()
            .size(px(36.))
            .rounded_lg()
            .bg(ACCENT_MIST)
            .flex()
            .items_center()
            .justify_center()
            .gap_0p5()
            .child(media_wave_bar(8., 0.45))
            .child(media_wave_bar(14., 0.75))
            .child(media_wave_bar(10., 0.55))
            .child(media_wave_bar(16., 0.9))
            .child(media_wave_bar(9., 0.5))
            .into_any_element()
    }
}

fn media_wave_bar(h: f32, opacity: f32) -> impl IntoElement {
    div()
        .w(px(2.5))
        .h(px(h))
        .rounded_full()
        .bg(ACCENT)
        .opacity(opacity)
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

/// Tiny folder glyph for the model-dir browse control.
fn folder_glyph() -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .items_start()
        .gap_0()
        .child(
            div()
                .w(px(8.))
                .h(px(3.))
                .rounded_t_sm()
                .bg(MUTED),
        )
        .child(
            div()
                .w(px(14.))
                .h(px(10.))
                .rounded_sm()
                .bg(MUTED_SOFT),
        )
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
    Folder,
    Copy,
}

/// Compact icon action — rest = muted gray; hover carries emotional tint.
/// Glyphs are pure text so `text_color` on hover actually recolors them.
fn icon_btn(
    kind: IconKind,
    tip: &'static str,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("ico-{tip}-{enabled}-{}", kind as u8));
    let tip_s: SharedString = tip.into();
    // Rest: quiet gray. Emotion only on hover (play→teal, trash→rose).
    let fg = if enabled { MUTED } else { MUTED_SOFT };
    let mut el = div()
        .id(id)
        .size(px(30.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_color(fg)
        .opacity(if enabled { 1.0 } else { 0.4 })
        .child(
            div()
                .text_sm()
                .font_weight(gpui::FontWeight::MEDIUM)
                .child(icon_char(kind)),
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
                IconKind::Play => s.bg(ACCENT_SOFT).text_color(ACCENT),
                IconKind::Trash => s.bg(DANGER_SOFT).text_color(DANGER),
                IconKind::Folder | IconKind::Copy => s.bg(BG).text_color(TEXT),
            })
            .on_click(on_click);
    }
    el
}

/// Text glyphs inherit parent `text_color` (geometry fills would not recolor on hover).
fn icon_char(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Play => "▶",
        // Clear "remove" mark — backspace glyph looked glued to play.
        IconKind::Trash => "×",
        IconKind::Folder => "↗",
        IconKind::Copy => "❐",
    }
}

/// Compact status mark (icon + soft plate). Tooltip carries Chinese label.
/// Pending breathes; processing steps a 3-dot pulse via `phase`.
fn status_mark(row_id: &str, status: TaskStatus, phase: u8) -> impl IntoElement {
    let tip: SharedString = status.label().into();
    let warn_soft = gpui::Rgba {
        r: 0xff as f32 / 255.0,
        g: 0xf7 as f32 / 255.0,
        b: 0xed as f32 / 255.0,
        a: 1.0,
    };

    // Gentle triangle-wave breath 0.45‥1.0 for idle pending.
    let breath = {
        let t = (phase % 20) as f32 / 19.0;
        let tri = if t < 0.5 { t * 2.0 } else { (1.0 - t) * 2.0 };
        0.45 + 0.55 * tri
    };
    let step = (phase / 4) % 3;
    let core = 0.55 + 0.45 * ((breath - 0.45) / 0.55);

    let glyph = match status {
        TaskStatus::Pending => div()
            .size(px(22.))
            .rounded_full()
            .border_1()
            .border_color(MUTED_SOFT)
            .bg(PILL_NEUTRAL_BG)
            .opacity(breath)
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .size(px(6.))
                    .rounded_full()
                    .bg(PILL_NEUTRAL_FG)
                    .opacity(core),
            )
            .into_any_element(),
        TaskStatus::Processing => div()
            .size(px(22.))
            .rounded_full()
            .bg(warn_soft)
            .flex()
            .items_center()
            .justify_center()
            .gap_0p5()
            .child(status_dot(step == 0, WARN))
            .child(status_dot(step == 1, WARN))
            .child(status_dot(step == 2, WARN))
            .into_any_element(),
        TaskStatus::Done => div()
            .size(px(22.))
            .rounded_full()
            .bg(ACCENT_SOFT)
            .flex()
            .items_center()
            .justify_center()
            .text_color(ACCENT)
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("✓"),
            )
            .into_any_element(),
        TaskStatus::Error => div()
            .size(px(22.))
            .rounded_full()
            .bg(DANGER_SOFT)
            .flex()
            .items_center()
            .justify_center()
            .text_color(DANGER)
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("!"),
            )
            .into_any_element(),
    };

    div()
        .id(SharedString::from(format!("st-{row_id}")))
        .child(glyph)
        .tooltip(move |_, cx| {
            cx.new(|_| NameTooltip { text: tip.clone() }).into()
        })
}

fn status_dot(on: bool, color: gpui::Rgba) -> impl IntoElement {
    div()
        .size(px(3.5))
        .rounded_full()
        .bg(color)
        .opacity(if on { 1.0 } else { 0.28 })
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
