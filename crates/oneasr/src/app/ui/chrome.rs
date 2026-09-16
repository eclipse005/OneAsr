//! Window chrome: the custom title bar and the tool bar above the list.

use crate::app::prelude::*;

/// Version shown in the titlebar, e.g. `v0.1.9`.
///
/// Single source is the Cargo package version (same one `crashlog` reports at
/// startup). Never hand-write a version string here — it will drift from the
/// build that actually shipped.
const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));

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
    pub(super) fn render_titlebar(&mut self) -> impl IntoElement {
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
