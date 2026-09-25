//! 传统样式的滑动开关：二值设置（如提示音）一行一个。
//!
//! gpui 没有现成的 Switch；轨道 + 滑块两个 div 就够了。状态由调用方持有
//! （与 [`btn`] 同模式），点击即翻转，天然适配"暂存-保存"语义。

use gpui::{prelude::*, px, div, App, ClickEvent, Window};

/// 36×20 圆角轨道 + 16 圆形滑块。`on` 决定轨道颜色与滑块位置。
pub fn switch(
    id: &'static str,
    on: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .flex_shrink_0()
        .w(px(36.))
        .h(px(20.))
        .rounded_full()
        .bg(if on {
            crate::theme::ACCENT
        } else {
            crate::theme::SWITCH_OFF
        })
        .cursor_pointer()
        .hover(|s| {
            if on {
                s.bg(crate::theme::LOGO)
            } else {
                s.bg(crate::theme::SWITCH_OFF_HOVER)
            }
        })
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(if on { px(18.) } else { px(2.) })
                .size(px(16.))
                .rounded_full()
                .bg(crate::theme::PANEL)
                .shadow_sm(),
        )
        .on_click(on_click)
}
