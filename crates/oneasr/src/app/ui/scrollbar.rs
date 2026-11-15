//! The list's scrollbar — gpui does not draw one, so this is it.
//!
//! gpui 的滚动容器（`overflow_y_scroll` 的 div、`uniform_list`）都只**维护**滚动
//! 状态、不画滚动条：`UniformListScrollHandle` 只把 `base_handle`（offset /
//! max_offset / bounds / set_offset）和 `last_item_size`（视口高 / 内容高）暴露
//! 出来，让应用自己画。Zed 自己也是这么画的。没有它，几百行之后用户只剩滚轮
//! 一条路——这正是这次补的。
//!
//! 交互按「不打扰」设计：
//! - 指针不靠右侧时**完全不显示**（大家用滚轮，滚动条只是兜底与位置提示）；
//! - 进右侧 14px 热区才淡入，离开淡出；
//! - 按住可以拖；点轨道空白处直接跳到那个位置（抓取点按 thumb 正中对齐）。
//!
//! 热区始终挂着（哪怕正在隐藏）：只在可见时才挂元素的话，指针永远进不来，
//! 滚动条就再也出不来了。热区是 `HitboxBehavior::Normal`，鼠标事件会穿透到
//! 下面的行，所以它压不住行右端的删除按钮。
//!
//! 拖拽的移动 / 松开不挂在这条窄带上，而是挂在整个列表上——见
//! [`OneAsrApp::scrollbar_press`] 的注释（指针捕获的语义）。

use crate::app::prelude::*;

impl OneAsrApp {
    /// 滚动条当前的淡入淡出进度（0 = 完全隐藏，1 = 完全显示）。
    pub(crate) fn scrollbar_progress(&self) -> f32 {
        let t = (self.scrollbar_t0.elapsed().as_secs_f32() / SCROLLBAR_FADE_SECS).min(1.0);
        let e = ease_out_cubic(t);
        self.scrollbar_from + (self.scrollbar_to - self.scrollbar_from) * e
    }

    /// 显示目标 = 指针在热区里，或者在拖拽（拖出热区也不该闪掉）。
    fn sync_scrollbar_target(&mut self) {
        let show = self.scrollbar_hover || self.scrollbar_grab.is_some();
        let target = if show { 1.0 } else { 0.0 };
        if (self.scrollbar_to - target).abs() < f32::EPSILON {
            return;
        }
        self.scrollbar_from = self.scrollbar_progress();
        self.scrollbar_to = target;
        self.scrollbar_t0 = Instant::now();
    }

    fn set_scrollbar_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.scrollbar_hover == hovered {
            return;
        }
        self.scrollbar_hover = hovered;
        self.sync_scrollbar_target();
        cx.notify();
    }

    /// 内容高度与视口高度（px），直接问 `uniform_list` 的句柄——它每帧在
    /// prepaint 里量好（`last_item_size`），我们不再自己推算。首帧还没有值，
    /// 返回 `None`：那时候画不准，不画比画错好。
    fn scrollbar_metrics(&self) -> Option<(f32, f32)> {
        let handle = self.list_scroll.0.borrow();
        let size = handle.last_item_size?;
        let viewport_h = f32::from(size.item.height);
        let content_h = f32::from(size.contents.height);
        if viewport_h <= 0.0 {
            return None;
        }
        Some((content_h, viewport_h))
    }

    /// 当前滚动偏移（gpui 约定：向下为负）。
    fn list_offset_y(&self) -> f32 {
        f32::from(self.list_scroll.0.borrow().base_handle.offset().y)
    }

    /// 列表视口在窗口里的位置（指针坐标 → 列表局部坐标要用）。
    fn list_viewport_bounds(&self) -> Bounds<Pixels> {
        self.list_scroll.0.borrow().base_handle.bounds()
    }

    /// 右侧滚动条。内容装得下时返回 `None`（没什么可滚的，别画一条死的）。
    pub(super) fn render_list_scrollbar(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let (content_h, viewport_h) = self.scrollbar_metrics()?;
        let offset_y = self.list_offset_y();
        let (top, thumb_h) =
            thumb_geometry(viewport_h, content_h, offset_y, SCROLLBAR_MIN_THUMB_H)?;
        let opacity = self.scrollbar_progress();
        let dragging = self.scrollbar_grab.is_some();

        Some(
            div()
                .id("list-scrollbar")
                .absolute()
                .top_0()
                .bottom_0()
                .right_0()
                .w(px(SCROLLBAR_HIT_W))
                .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                    this.set_scrollbar_hover(*hovered, cx);
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| this.scrollbar_press(ev, cx)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(top))
                        .right(px(SCROLLBAR_MARGIN))
                        .w(px(SCROLLBAR_THUMB_W))
                        .h(px(thumb_h))
                        .rounded_full()
                        .bg(if dragging { MUTED } else { MUTED_SOFT })
                        // 淡入淡出只作用在 thumb 上：热区本身必须一直可命中。
                        .opacity(opacity),
                )
                .into_any_element(),
        )
    }

    /// 热区按下：按在 thumb 上就抓住它，按在空白处就把 thumb 中心挪过来再抓。
    ///
    /// **移动与松开挂在列表上，不挂在这条 14px 热区上**：`on_mouse_move` 只在
    /// 指针压住该 hitbox 时才派发（`vendor/gpui/src/elements/div.rs` 的
    /// `Interactivity::on_mouse_move` 里那句 `hitbox.is_hovered`），快速拖动时
    /// 指针稍微偏出窄带、哪怕还在列表里，事件就断了——表现成「拖一下卡住，
    /// 移回来才恢复」。系统滚动条的语义是「按下就抓住」，所以拖拽的移动/松开
    /// 交给整块列表（856px 宽）接，指针滑出去也照样跟手。
    fn scrollbar_press(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some((content_h, viewport_h)) = self.scrollbar_metrics() else {
            return;
        };
        let bounds = self.list_viewport_bounds();
        let local_y = f32::from(ev.position.y) - f32::from(bounds.top());
        let Some((top, thumb_h)) = thumb_geometry(
            viewport_h,
            content_h,
            self.list_offset_y(),
            SCROLLBAR_MIN_THUMB_H,
        ) else {
            return;
        };
        let grab = if local_y >= top && local_y <= top + thumb_h {
            local_y - top
        } else {
            // 点空白：thumb 跳到指针正下方，抓取点落在它的正中。
            thumb_h / 2.0
        };
        self.scrollbar_grab = Some(grab);
        self.sync_scrollbar_target();
        self.scrollbar_scroll_to(local_y - grab, viewport_h, content_h);
        cx.notify();
    }

    /// 拖拽中：把抓取点钉回指针下面。
    pub(super) fn scrollbar_drag_to(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some(grab) = self.scrollbar_grab else {
            return;
        };
        let Some((content_h, viewport_h)) = self.scrollbar_metrics() else {
            return;
        };
        let bounds = self.list_viewport_bounds();
        let local_y = f32::from(ev.position.y) - f32::from(bounds.top());
        self.scrollbar_scroll_to(local_y - grab, viewport_h, content_h);
        cx.notify();
    }

    pub(super) fn scrollbar_release(&mut self, cx: &mut Context<Self>) {
        if self.scrollbar_grab.take().is_some() {
            self.sync_scrollbar_target();
            cx.notify();
        }
    }

    /// thumb 顶部放到 `thumb_top` 处（px），换算成列表的滚动偏移。
    fn scrollbar_scroll_to(&mut self, thumb_top: f32, viewport_h: f32, content_h: f32) {
        let Some((_, thumb_h)) = thumb_geometry(
            viewport_h,
            content_h,
            self.list_offset_y(),
            SCROLLBAR_MIN_THUMB_H,
        ) else {
            return;
        };
        let offset_y = offset_for_thumb_top(thumb_top, viewport_h, content_h, thumb_h);
        self.list_scroll
            .0
            .borrow()
            .base_handle
            .set_offset(point(Pixels::ZERO, px(offset_y)));
    }
}

// ── 几何（纯函数，单测见下）───────────────────────────────────────────────

/// thumb 的 `(顶部 y, 高度)`，单位 px，坐标是列表视口内的局部坐标。
///
/// 高度按「视口 / 内容」的比例给，最短 `min_h`（内容再长也留一个按得住的东西）。
/// `offset_y` 用的是 gpui 的约定：向下滚动为负。内容装得下时返回 `None`
/// ——没有可滚的距离就没有滚动条。
pub(crate) fn thumb_geometry(
    viewport_h: f32,
    content_h: f32,
    offset_y: f32,
    min_h: f32,
) -> Option<(f32, f32)> {
    if viewport_h <= 0.0 || content_h <= viewport_h {
        return None;
    }
    let thumb_h = (viewport_h * viewport_h / content_h).clamp(min_h.min(viewport_h), viewport_h);
    let travel = viewport_h - thumb_h;
    let scrollable = content_h - viewport_h;
    let progress = (-offset_y / scrollable).clamp(0.0, 1.0);
    Some((travel * progress, thumb_h))
}

/// [`thumb_geometry`] 的反函数：thumb 顶部在 `thumb_top` 时，列表该有的滚动偏移。
pub(crate) fn offset_for_thumb_top(
    thumb_top: f32,
    viewport_h: f32,
    content_h: f32,
    thumb_h: f32,
) -> f32 {
    let travel = viewport_h - thumb_h;
    let scrollable = (content_h - viewport_h).max(0.0);
    if travel <= 0.0 || scrollable <= 0.0 {
        return 0.0;
    }
    let progress = (thumb_top / travel).clamp(0.0, 1.0);
    -(progress * scrollable)
}

#[cfg(test)]
mod tests {
    use super::{offset_for_thumb_top, thumb_geometry};

    const MIN: f32 = 28.0;

    #[test]
    fn nothing_to_scroll_means_no_scrollbar() {
        assert_eq!(thumb_geometry(800.0, 800.0, 0.0, MIN), None);
        assert_eq!(thumb_geometry(800.0, 400.0, 0.0, MIN), None);
        assert_eq!(thumb_geometry(0.0, 5000.0, 0.0, MIN), None);
    }

    #[test]
    fn the_thumb_shrinks_with_the_content() {
        // 内容两屏：thumb 占一半视口，顶部对齐。
        assert_eq!(thumb_geometry(800.0, 1600.0, 0.0, MIN), Some((0.0, 400.0)));
        // 滚到底：thumb 停在轨道底部。
        let (top, h) = thumb_geometry(800.0, 1600.0, -800.0, MIN).unwrap();
        assert_eq!(h, 400.0);
        assert_eq!(top, 400.0);
    }

    #[test]
    fn a_tiny_thumb_still_keeps_the_minimum_height() {
        // 两百屏内容：按比例算出来不到 5px，夹到 28px。
        let (_, h) = thumb_geometry(800.0, 160_000.0, 0.0, MIN).unwrap();
        assert_eq!(h, MIN);
    }

    #[test]
    fn overscroll_is_clamped_instead_of_escaping_the_track() {
        let (top, h) = thumb_geometry(800.0, 1600.0, 5_000.0, MIN).unwrap();
        assert_eq!(top, 0.0);
        let (top, _) = thumb_geometry(800.0, 1600.0, -5_000.0, MIN).unwrap();
        assert_eq!(top, 800.0 - h);
    }

    #[test]
    fn thumb_position_round_trips_through_the_offset() {
        let (viewport, content) = (800.0, 1600.0);
        let (top, h) = thumb_geometry(viewport, content, -800.0, MIN).unwrap();
        assert_eq!(offset_for_thumb_top(top, viewport, content, h), -800.0);
        let (top, h) = thumb_geometry(viewport, content, -400.0, MIN).unwrap();
        assert_eq!(offset_for_thumb_top(top, viewport, content, h), -400.0);
    }

    #[test]
    fn dragging_below_the_track_clamps_to_the_end() {
        // 抓取点被拖到轨道底下：偏移停在最底部，不会溢出。
        let offset = offset_for_thumb_top(9_999.0, 800.0, 1600.0, 400.0);
        assert_eq!(offset, -800.0);
        let offset = offset_for_thumb_top(-40.0, 800.0, 1600.0, 400.0);
        assert_eq!(offset, 0.0);
    }
}
