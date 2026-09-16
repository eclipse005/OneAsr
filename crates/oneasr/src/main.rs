//! OneAsr — light-themed batch list for local Qwen ASR + align.
//!
//! This file is the process entry point only: it registers the native library
//! path, installs the runtime and panic hooks, and opens the window. The
//! window's state and logic live in [`app`]; its rendering lives in `app::ui`.

// Release / portable: GUI PE subsystem (no black console on double-click).
// Debug: keep the CONSOLE subsystem so `cargo run` logs work without stdio hacks.
// Release is additionally forced in `build.rs` + verified by `pack-release.ps1`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod assets;
mod crashlog;
mod sfx;
mod shell;
mod theme;
mod ui_font;
mod widgets;

use gpui::{
    App, Application, Bounds, KeyBinding, WindowBounds, WindowOptions, actions, prelude::*,
    px, size,
};
use oneasr_core::{init_native_library_path, init_runtime};

use crate::app::OneAsrApp;

actions!(oneasr, [DismissMenus]);

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
