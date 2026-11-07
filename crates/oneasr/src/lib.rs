//! OneAsr 桌面壳。界面是 Native Shell，转写、对齐和设置仍是 `oneasr-core`。

// 双语表整份留着：校验脚本按文件对账，这一版壳只用其中一部分。
#[allow(dead_code)]
mod i18n;
mod session;
mod ui;

use std::cell::RefCell;
use std::rc::Rc;

use nsh_core::prelude::*;

/// TSX 入口可以导入的根组件。这个壳本身不跑 QuickJS。
#[native_module("oneasr/native")]
fn native(registry: &mut Registry) {
    registry.register::<App>();
}

/// 窗口里的整页应用。
#[component]
pub fn App() -> impl IntoElement {
    // 三端同一份主题。官方组件（Button、Switch、Tabs、Progress）读这些 token。
    if let Ok(theme) = nsh_base::Theme::from_json(include_str!("theme.json")) {
        set_tokens(theme);
    }
    let (session, rx) = session::Session::boot();
    ui::root(Rc::new(RefCell::new(session)), rx)
}

/// 在当前系统的桌面后端上打开窗口，直到用户关掉它。
pub fn run() -> std::process::ExitCode {
    #[cfg(windows)]
    {
        nsh_platform_windows::run_app(
            nsh_platform_windows::AppSource::Rust(nsh_platform_windows::Root::of::<App>()),
            nsh_platform_windows::AppOptions {
                app_id: "com.eclipse005.oneasr".into(),
                title: "OneAsr".into(),
                size: (1080, 720),
                appearance: nsh_platform_windows::Appearance::Light,
                ..nsh_platform_windows::AppOptions::default()
            },
        )
    }
    #[cfg(target_os = "macos")]
    {
        nsh_platform_macos::run_app(
            nsh_platform_macos::AppSource::Rust(nsh_platform_macos::Root::of::<App>()),
            nsh_platform_macos::AppOptions {
                app_id: "com.eclipse005.oneasr".into(),
                title: "OneAsr".into(),
                size: (1080, 720),
                appearance: nsh_platform_macos::Appearance::Light,
                ..nsh_platform_macos::AppOptions::default()
            },
        )
    }
    #[cfg(target_os = "linux")]
    {
        nsh_platform_linux::run_app(
            nsh_platform_linux::AppSource::Rust(nsh_platform_linux::Root::of::<App>()),
            nsh_platform_linux::AppOptions {
                app_id: "com.eclipse005.oneasr".into(),
                title: "OneAsr".into(),
                size: (1080, 720),
                appearance: nsh_platform_linux::Appearance::Light,
                ..nsh_platform_linux::AppOptions::default()
            },
        )
    }
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        eprintln!("OneAsr NSH shell has no desktop backend on this target");
        std::process::ExitCode::FAILURE
    }
}
