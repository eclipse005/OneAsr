//! 指南面板：首次启动自动展开一次，之后由状态栏「指南」chip 随时开关。
//!
//! 与统计面板**同一套浮层机制**（`deferred` 直接子元素做 `absolute` +
//! MENU_Z + 全窗 dismiss 层）：非模态，不挡下面的列表。gpui 的坑：`absolute`
//! 参照的是**直接父节点**（taffy 语义，不是 CSS 的「最近定位祖先」），且
//! `deferred` 对布局透明——所以面板必须是 deferred 的直接子元素，中间再包
//! 一层普通 div 就会塌进父节点的布局槽（v1.2.x 模态版卡片因此贴底截断）。

use crate::app::prelude::*;

impl OneAsrApp {
    /// 浮动指南面板，锚在状态栏上方——与统计面板同一块位置，互斥展开。
    pub(super) fn render_welcome_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let not_ready = self.model_status == ModelStatus::NotReady;
        let steps = [
            t(L::WELCOME_STEP1),
            t(L::WELCOME_STEP2),
            t(L::WELCOME_STEP3),
        ];

        deferred(
            div()
                .absolute()
                .left(px(14.))
                .bottom(px(34.))
                .w(px(WELCOME_PANEL_W))
                .flex()
                .flex_col()
                .gap_3()
                .p_4()
                .rounded_lg()
                .border_1()
                .border_color(LINE)
                .bg(PANEL)
                .shadow(popover_menu_shadow())
                .occlude()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2p5()
                        .child(app_logo(false))
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(TEXT)
                                .child(t(L::WELCOME_TITLE)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(MUTED)
                        .child(t(L::WELCOME_SUBTITLE)),
                )
                .child(
                    div().flex().flex_col().gap_2().children(
                        steps
                            .into_iter()
                            .enumerate()
                            .map(|(i, text)| welcome_step(i + 1, text)),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(MUTED_SOFT)
                        .child(t(L::WELCOME_LOCAL)),
                )
                .child(
                    div()
                        .border_t_1()
                        .border_color(LINE)
                        .pt_2()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(MUTED)
                                .child(t(L::WELCOME_EXTRAS)),
                        )
                        .child(welcome_feature(
                            t(L::WELCOME_FEAT_TRANSCRIPT),
                            t(L::WELCOME_FEAT_TRANSCRIPT_DESC),
                        ))
                        .child(welcome_feature(
                            t(L::WELCOME_FEAT_SEPARATION),
                            t(L::WELCOME_FEAT_SEPARATION_DESC),
                        ))
                        .child(welcome_feature(
                            t(L::WELCOME_FEAT_MODEL_SIZE),
                            t(L::WELCOME_FEAT_MODEL_SIZE_DESC),
                        )),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_end()
                        .gap_2()
                        .when(not_ready, |el| {
                            el.child(btn(
                                t(L::WELCOME_OK),
                                BtnKind::Quiet,
                                true,
                                cx.listener(|this, _, _, cx| this.close_welcome(cx)),
                            ))
                            .child(btn(
                                t(L::OPEN_SETTINGS),
                                BtnKind::Primary,
                                true,
                                cx.listener(|this, _, _, cx| this.close_welcome_open_settings(cx)),
                            ))
                        })
                        .when(!not_ready, |el| {
                            el.child(btn(
                                t(L::WELCOME_OK),
                                BtnKind::Secondary,
                                true,
                                cx.listener(|this, _, _, cx| this.close_welcome(cx)),
                            ))
                        }),
                ),
        )
        .with_priority(MENU_Z)
    }
}

/// 一行步骤：编号圆徽 + 说明。徽小一号（面板比模态卡紧凑）。
fn welcome_step(n: usize, text: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .size(px(18.))
                .flex_shrink_0()
                .rounded_full()
                .bg(ACCENT_SOFT)
                .text_color(ACCENT)
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .flex()
                .items_center()
                .justify_center()
                .child(n.to_string()),
        )
        // flex_1 + min_w_0：taffy 的 flex 子项默认不收缩到内容宽度以下，
        // 少了这两个，长文案不会换行而是顶出面板右缘。
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_sm()
                .text_color(TEXT)
                .child(text),
        )
}

/// 「进阶」小节的一行：固定宽的粗体标签列 + 说明，两列左对齐（zh / en 标签
/// 长短不一，靠列宽对齐而不是互相推挤）。
fn welcome_feature(label: &'static str, desc: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .child(
            div()
                .w(px(72.))
                .flex_shrink_0()
                .text_xs()
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(TEXT)
                .child(label),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(MUTED)
                .child(desc),
        )
}
