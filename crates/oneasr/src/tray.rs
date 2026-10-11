//! 系统托盘：关窗到托盘后的常驻入口。
//!
//! 全部状态住在主线程的 `thread_local` 里——托盘的创建（Windows 的
//! `Shell_NotifyIconW`、macOS 的 `NSStatusItem` 都要求主线程）与事件轮询
//! 都在主线程跑，不需要跨线程共享。
//!
//! 事件走「排水 → 执行」两拍：[`drain_events`] 只把菜单/点击收进待办队列
//! （不碰任何实体），[`apply_pending`] 在 worker 泵的**实体租借结束之后**
//! 才执行窗口操作。gpui 禁止对同一个实体嵌套 `update`（double lease 直接
//! panic）——而托盘事件恰恰是在 `poll_worker` 的 `this.update(...)` 里被
//! 抽到的，第一版在这里直接 `window.update` 就炸了这条线。
//!
//! Linux 后端是 ksni（StatusNotifierItem / DBus，纯 Rust）；Windows 与
//! macOS 用系统原语。菜单用 muda 构建，三个平台的事件都从
//! `MenuEvent::receiver()` 统一收取。

use std::cell::RefCell;
use std::io::Cursor;
use std::sync::OnceLock;

use gpui::{App, AsyncApp, WindowHandle};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::app::OneAsrApp;
use crate::app::close::CloseSource;
use crate::i18n::{L, t};

/// 菜单项 id（`MenuEvent` 按它分发）。
const MENU_OPEN: &str = "oneasr-tray-open";
const MENU_QUIT: &str = "oneasr-tray-quit";

/// 待执行的动作。排进 [`PENDING`]，由 [`apply_pending`] 在实体租借外执行。
#[derive(Clone, Copy)]
enum TrayAction {
    Restore,
    Quit,
}

thread_local! {
    static TRAY: RefCell<Option<TrayIcon>> = const { RefCell::new(None) };
    static WINDOW: RefCell<Option<WindowHandle<OneAsrApp>>> = const { RefCell::new(None) };
    static PENDING: RefCell<Vec<TrayAction>> = const { RefCell::new(Vec::new()) };
    /// 上次设置的 tooltip，内容没变就不动托盘（Windows 的 set_tooltip 是一次
    /// Shell 调用，任务进行中阶段词会频繁变化）。
    static LAST_TOOLTIP: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// 解码好的托盘图标 RGBA（尺寸 + 像素），失败时带原因。
type IconRgba = Result<(Vec<u8>, u32, u32), String>;

/// 应用图标 RGBA（解码一次缓存）。托盘图标与 exe/窗口同源：
/// `assets/icons/app-icon-32.png`（`scripts/gen-app-icon.py` 生成）。
fn icon_rgba() -> &'static IconRgba {
    static ICON: OnceLock<IconRgba> = OnceLock::new();
    ICON.get_or_init(|| {
        let decoder = png::Decoder::new(Cursor::new(include_bytes!(
            "../../../assets/icons/app-icon-32.png"
        )));
        let mut reader = decoder
            .read_info()
            .map_err(|e| format!("png read_info failed: {e}"))?;
        let Some(frame_size) = reader.output_buffer_size() else {
            return Err("png output buffer size unknown".into());
        };
        let mut buf = vec![0u8; frame_size];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| format!("png next_frame failed: {e}"))?;
        buf.truncate(info.buffer_size());
        Ok((buf, info.width, info.height))
    })
}

/// 启动时创建托盘（主线程，`Application::run` 闭包内调用）。
///
/// 创建失败不致命：关窗行为照常，只是「隐藏到托盘」之后没有恢复入口，
/// 记进错误日志即可。托盘常驻创建（而不是首次隐藏时才建）——入口的位置
/// 要可预期，用户才敢放心关窗。
pub(crate) fn init(window: WindowHandle<OneAsrApp>) {
    WINDOW.with(|cell| *cell.borrow_mut() = Some(window));

    let (rgba, width, height) = match icon_rgba() {
        Ok(icon) => icon,
        Err(error) => {
            crate::crashlog::log_error(format!("tray icon unavailable: {error}"));
            return;
        }
    };
    let icon = match Icon::from_rgba(rgba.clone(), *width, *height) {
        Ok(icon) => icon,
        Err(error) => {
            crate::crashlog::log_error(format!("tray icon decode failed: {error}"));
            return;
        }
    };

    let open = MenuItem::with_id(MENU_OPEN, t(L::TRAY_OPEN), true, None);
    let quit = MenuItem::with_id(MENU_QUIT, t(L::TRAY_QUIT), true, None);
    let menu = match Menu::with_items(&[&open, &PredefinedMenuItem::separator(), &quit]) {
        Ok(menu) => menu,
        Err(error) => {
            crate::crashlog::log_error(format!("tray menu build failed: {error}"));
            return;
        }
    };

    let mut builder = TrayIconBuilder::new()
        .with_id("oneasr-tray")
        .with_icon(icon)
        .with_tooltip(crate::i18n::tray_tooltip(None))
        .with_menu(Box::new(menu));
    // macOS 惯例是左键弹菜单，保持默认；Windows / Linux 左键恢复窗口，
    // 菜单交给右键。
    if !cfg!(target_os = "macos") {
        builder = builder.with_menu_on_left_click(false);
    }
    match builder.build() {
        Ok(tray) => TRAY.with(|cell| *cell.borrow_mut() = Some(tray)),
        Err(error) => crate::crashlog::log_error(format!("tray creation failed: {error}")),
    }
}

/// 托盘 tooltip。内容与上次相同就跳过（见 [`LAST_TOOLTIP`]）。
pub(crate) fn set_tooltip(text: &str) {
    TRAY.with(|cell| {
        let mut tray = cell.borrow_mut();
        let Some(tray) = tray.as_mut() else {
            return;
        };
        let changed = LAST_TOOLTIP.with_borrow_mut(|last| {
            if last.as_deref() == Some(text) {
                false
            } else {
                *last = Some(text.to_owned());
                true
            }
        });
        if changed && let Err(error) = tray.set_tooltip(Some(text.to_owned())) {
            crate::crashlog::log_warn(format!("tray tooltip update failed: {error}"));
        }
    });
}

/// 第一拍：抽干托盘事件（菜单选择 / 图标点击），只入队不执行——见模块
/// 注释「排水 → 执行」。主线程每 80ms 调一次（worker 泵内）。
pub(crate) fn drain_events() {
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        match event.id().as_ref() {
            MENU_OPEN => PENDING.with_borrow_mut(|v| v.push(TrayAction::Restore)),
            MENU_QUIT => PENDING.with_borrow_mut(|v| v.push(TrayAction::Quit)),
            _ => {}
        }
    }
    while let Ok(event) = TrayIconEvent::receiver().try_recv() {
        // menu_on_left_click(false) 时左键单击以 Click 事件到达：恢复窗口。
        // 右键弹菜单是托盘自身行为，不产生需要处理的事件。
        if let TrayIconEvent::Click { button, .. } = event
            && button == tray_icon::MouseButton::Left
        {
            PENDING.with_borrow_mut(|v| v.push(TrayAction::Restore));
        }
    }
}

/// 第二拍：在实体租借外执行待办动作。worker 泵的 `this.update(...)` 返回
/// 之后、同一个 80ms 拍内调用，`AsyncApp` 上做 `window.update` 是合法的。
pub(crate) fn apply_pending(cx: &mut AsyncApp) {
    let actions = PENDING.take();
    if actions.is_empty() {
        return;
    }
    let Some(window) = WINDOW.with_borrow(|cell| cell.as_ref().map(|h| *h)) else {
        return;
    };
    for action in actions {
        let result = window.update(cx, |app, window, cx| match action {
            TrayAction::Restore => app.show_main_window(window, cx),
            TrayAction::Quit => app.request_close(window, CloseSource::TrayQuit, cx),
        });
        // 执行失败必须留痕：这条链路没有任何可见反馈，点托盘没反应的报告
        // 要能从 oneasr-error.log 里看出来（窗口/实体已不在属正常竞争）。
        if let Err(e) = result {
            crate::crashlog::log_warn(format!("tray action dropped: {e:#}"));
            break;
        }
    }
}

/// 显示主窗口（macOS dock 重开走这里；托盘动作见 [`apply_pending`]）。
pub(crate) fn restore_main_window(cx: &mut App) {
    let Some(window) = WINDOW.with_borrow(|cell| cell.as_ref().map(|h| *h)) else {
        return;
    };
    let _ = window.update(cx, |app, window, cx| app.show_main_window(window, cx));
}
