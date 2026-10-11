//! 关窗行为：所有关窗入口（原生红叉 / Alt+F4、Linux 自绘红叉、托盘「退出」）
//! 收敛到 [`OneAsrApp::request_close`]。行为由来源 + 一个开关（`close_to_tray`）
//! 决定，没有弹窗：
//!
//! - **红叉**：开关开 = 隐藏到托盘（任务继续）；关 = 直接退出。
//! - **托盘「退出」**：永远真退出——菜单项写的是「退出」，而开关开着时红叉
//!   不退出，它就是唯一的退出入口；对藏在托盘里的窗口再「隐藏」一遍正是
//!   「点退出没反应」的来源。
//!
//! gpui 的 `should_close` 回调永远返回 `false`（窗口不被默认销毁），真正的
//! 退出只发生在 [`OneAsrApp::exit_app`] 的 `cx.quit()`。

use crate::app::prelude::*;

/// 关窗请求的来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseSource {
    /// 原生标题栏红叉 / Alt+F4 / Linux 自绘红叉（经 `RequestClose` 动作）。
    WindowButton,
    /// 托盘菜单「退出 OneAsr」。
    TrayQuit,
}

impl OneAsrApp {
    /// 统一关窗入口。见模块注释：这里只做决定，退出只在 [`Self::exit_app`]。
    pub(crate) fn request_close(
        &mut self,
        window: &mut Window,
        source: CloseSource,
        cx: &mut Context<Self>,
    ) {
        match source {
            CloseSource::TrayQuit => self.exit_app(cx),
            CloseSource::WindowButton => {
                if self.settings.close_to_tray {
                    self.minimize_to_tray(window, cx);
                } else {
                    self.exit_app(cx);
                }
            }
        }
    }

    /// 最小化到托盘：隐藏窗口，进程与任务照常运行。
    pub(crate) fn minimize_to_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_visible(false);
        cx.notify();
    }

    /// 从托盘 / dock 唤回主窗口。
    pub(crate) fn show_main_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_visible(true);
        window.activate_window();
        cx.notify();
    }

    /// 全进程退出。唯一调用 gpui 退出的地方——关窗入口都不直接 quit。
    fn exit_app(&mut self, cx: &mut Context<Self>) {
        cx.quit();
    }

    /// 托盘 tooltip 跟随任务阶段。内容没变时 [`crate::tray::set_tooltip`]
    /// 自己跳过，这里每拍调用无妨。
    pub(crate) fn refresh_tray_tooltip(&self) {
        let stage = if self.busy {
            self.active_stage
                .as_ref()
                .map(|(_, stage)| stage.to_string())
        } else {
            None
        };
        crate::tray::set_tooltip(&crate::i18n::tray_tooltip(stage.as_deref()));
    }
}
