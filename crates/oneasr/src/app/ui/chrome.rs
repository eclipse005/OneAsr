//! Window chrome: the custom title bar and the tool bar above the list.

use crate::app::prelude::*;

#[cfg(target_os = "linux")]
use gpui::MouseButton;

/// Version shown in the titlebar, e.g. `v0.1.9`.
///
/// Single source is the Cargo package version (same one `crashlog` reports at
/// startup). Never hand-write a version string here — it will drift from the
/// build that actually shipped.
const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Whether the self-drawn min / max / close buttons are drawn at all.
///
/// Windows-only: GPUI's macOS backend implements `on_hit_test_window_control` as a
/// no-op (`vendor/gpui/src/platform/mac/window.rs`), and these buttons deliberately
/// carry no `on_click` (see below), so on macOS they can only be dead decoration.
/// There the system traffic lights (top-left) do the job instead.
const SHOW_CAPTION_BUTTONS: bool = !cfg!(target_os = "macos");

/// Left padding of the title bar.
///
/// macOS floats the native traffic lights above the content view (transparent
/// title bar), so without an inset the self-drawn `OneAsr v1.0.0` label sits under
/// them and reads as a truncated version number. 78px ≈ 13px edge + three 14pt
/// buttons + two 6pt gaps, plus a little slack — worth a one-time eyeball check on
/// real hardware (anything within ±8px is fine).
const TITLEBAR_PAD_LEFT: Pixels = if cfg!(target_os = "macos") { px(78.) } else { px(12.) };

impl OneAsrApp {
    /// Custom title bar. GPUI never sets `WS_CAPTION`, so the "native" caption is only
    /// a DWM fallback — missing or dead on some Win10 machines (the user report:
    /// click grays out, no drag / min / max / close). Drawing our own + routing hits
    /// through `WindowControlArea` works on Windows.
    ///
    /// Linux's Wayland and X11 backends do not implement the non-client hit test
    /// behind `WindowControlArea`, so the Linux build uses ordinary mouse events
    /// for the same actions.
    ///
    /// macOS: `WindowControlArea` is ignored by the platform backend, so the caption
    /// buttons are not drawn (see `SHOW_CAPTION_BUTTONS`) and the left inset keeps the
    /// window title clear of the native traffic lights. Dragging still works — that
    /// comes from the transparent title bar itself, not from `WindowControlArea::Drag`.
    pub(super) fn render_titlebar(&mut self) -> impl IntoElement {
        // Drag strip: everything left of the buttons moves the window. Wayland and
        // X11 need an explicit mouse-down handler because their backends ignore
        // `WindowControlArea::Drag`.
        let drag = div()
            .id("titlebar-drag")
            .flex_1()
            .h_full()
            .min_w_0()
            .flex()
            .items_center()
            .pl(TITLEBAR_PAD_LEFT)
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
            );
        #[cfg(target_os = "linux")]
        let drag = drag.on_mouse_down(MouseButton::Left, |_, window, _| {
            window.start_window_move();
        });

        div()
            .h(px(32.))
            .w_full()
            .flex()
            .items_center()
            .bg(PANEL)
            .border_b_1()
            .border_color(LINE)
            .child(drag)
            // 系统红绿灯在 macOS 上负责最小化 / 全屏 / 关闭，自绘按钮只在别处出现。
            .when(SHOW_CAPTION_BUTTONS, |el| {
                el.child(caption_btn(
                    "titlebar-min",
                    "icons/win-min.svg",
                    WindowControlArea::Min,
                    false,
                    CaptionAction::Minimize,
                ))
                .child(caption_btn(
                    "titlebar-max",
                    "icons/win-max.svg",
                    WindowControlArea::Max,
                    false,
                    CaptionAction::Maximize,
                ))
                .child(caption_btn(
                    "titlebar-close",
                    "icons/win-close.svg",
                    WindowControlArea::Close,
                    true,
                    CaptionAction::Close,
                ))
            })
    }

    pub(super) fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

}
