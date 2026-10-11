//! OneAsr — light-themed batch list for local Qwen ASR + align.
//!
//! This file is the process entry point only: it installs the runtime and
//! panic hooks, and opens the window. The
//! window's state and logic live in [`app`]; its rendering lives in `app::ui`.

// Release / portable: GUI PE subsystem (no black console on double-click).
// Debug: keep the CONSOLE subsystem so `cargo run` logs work without stdio hacks.
// Release is additionally forced in `build.rs` + verified by `pack-release.ps1`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod crashlog;
mod i18n;
mod sfx;
mod shell;
mod theme;
mod tray;
mod ui_font;
mod widgets;

use gpui::{
    App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, actions, prelude::*, px,
    size,
};
use oneasr_core::init_runtime;

use crate::app::OneAsrApp;
use crate::app::close::CloseSource;

actions!(oneasr, [DismissMenus, RequestClose]);

fn main() {
    // Cap rayon before any model load so the UI thread keeps a free core.
    init_runtime();
    // Install-dir error log + panic capture (see `oneasr-error.log`).
    crashlog::install_panic_hook();
    crashlog::log_session_start();

    // `on_reopen` 返回共享引用，不能插在 with_assets → run 的所有权链中间；
    // 单独注册一次（macOS dock 重开；其余平台此回调不会触发，restore 在
    // 托盘未建时是无操作）。
    let app = Application::new().with_assets(assets::AppAssets::new());
    app.on_reopen(crate::tray::restore_main_window);
    app.run(|cx: &mut App| {
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
        // Linux uses client-side decorations so the self-drawn title bar is the
        // real top strip. Wayland/X11 do not implement WindowControlArea's native
        // hit test; the title bar supplies regular mouse handlers on that path.
        #[cfg(target_os = "linux")]
        let window_decorations = Some(gpui::WindowDecorations::Client);
        #[cfg(not(target_os = "linux"))]
        let window_decorations = None;
        // 让 Wayland / X11 的窗口管理器能用 oneasr.desktop 匹配应用图标。
        #[cfg(target_os = "linux")]
        let app_id = Some("oneasr".to_owned());
        #[cfg(not(target_os = "linux"))]
        let app_id = None;
        let window = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("OneAsr".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    window_decorations,
                    app_id,
                    ..Default::default()
                },
                {
                    let font_plan = font_plan.clone();
                    move |window, cx| {
                        cx.new(|cx| {
                            let mut app = OneAsrApp::new(cx);
                            app.ui_font = font_plan;
                            app.focus_handle.focus(window);
                            // 关窗流程统一接管：should_close 永远返回 false，
                            // 窗口不被 gpui 默认销毁；退出只发生在
                            // `request_close` 决定退出后的 `cx.quit()`。
                            let weak = cx.weak_entity();
                            window.on_window_should_close(cx, move |window, cx| {
                                let _ = weak.update(cx, |this, cx| {
                                    this.request_close(window, CloseSource::WindowButton, cx);
                                });
                                false
                            });
                            app
                        })
                    }
                },
            )
            .expect("open window");
        // 托盘常驻：关窗到托盘之后的恢复 / 退出入口。
        tray::init(window);
        cx.activate(true);
    });
}
