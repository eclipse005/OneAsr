//! Deferred popovers: dismiss layer, language menu, timing breakdown.

use gpui::{
    deferred, div, hsla, point, prelude::*, px, App, BoxShadow, Context, MouseButton, SharedString, Window,
};
use oneasr_core::TaskTiming;

/// Full-window click-outside scrim under open floating menus (`MENU_DISMISS_Z`).
/// Mounted on the app root so toolbar / list / status bar are all outside-click targets.
pub fn popover_dismiss_layer(cx: &mut Context<crate::app::OneAsrApp>) -> impl IntoElement {
    deferred(
        div()
            .id("menu-dismiss-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .cursor_default()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.dismiss_menus(cx);
                }),
            ),
    )
    .with_priority(crate::app::ui::metrics::MENU_DISMISS_Z)
}

pub fn popover_menu_shadow() -> Vec<BoxShadow> {
    vec![
        BoxShadow {
            color: hsla(0., 0., 0., 0.08),
            offset: point(px(0.), px(2.)),
            blur_radius: px(4.),
            spread_radius: px(0.),
        },
        BoxShadow {
            color: hsla(0., 0., 0., 0.14),
            offset: point(px(0.), px(8.)),
            blur_radius: px(20.),
            spread_radius: px(0.),
        },
    ]
}

/// One row inside the language menu.
pub fn lang_menu_option(
    option_id: SharedString,
    label: &str,
    active: bool,
    target: crate::app::LangSelectTarget,
    lang_id: &'static str,
    cx: &mut Context<crate::app::OneAsrApp>,
) -> impl IntoElement {
    div()
        .id(option_id)
        .px_2p5()
        .py_1p5()
        .cursor_pointer()
        .bg(if active {
            crate::theme::ACCENT_SOFT
        } else {
            crate::theme::PANEL
        })
        .hover(|s| s.bg(if active { crate::theme::ACCENT_SOFT } else { crate::theme::BG }))
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pick_source_language(target.clone(), lang_id, cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(if active {
                            crate::theme::ACCENT
                        } else {
                            crate::theme::TEXT
                        })
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .when(active, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(crate::theme::ACCENT)
                            .child("✓"),
                    )
                }),
        )
}

/// Processing-time breakdown card under the **用时** chip (`MENU_Z`).
///
/// Parent must be a `relative` wrapper around the chip so `left_0` aligns to it.
pub fn timing_breakdown_popover(
    menu_id: SharedString,
    timing: &TaskTiming,
    progress: f32,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = progress.clamp(0.0, 1.0);
    let opacity = p;
    // Rise from slightly below (dialog-ish, not full modal).
    let y_offset = px(6.0 * (1.0 - p));
    let total = timing.total_label();
    let max_stage = timing.stages.iter().map(|s| s.elapsed_ms).max().unwrap_or(1);

    let rows: Vec<gpui::AnyElement> = timing
        .stages
        .iter()
        .map(|s| {
            let is_hot = s.elapsed_ms == max_stage && max_stage > 0;
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .w_full()
                .child(
                    div()
                        .text_xs()
                        .text_color(if is_hot { crate::theme::TEXT } else { crate::theme::MUTED })
                        .font_weight(if is_hot {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(s.stage.label().to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if is_hot { crate::theme::ACCENT } else { crate::theme::MUTED })
                        .whitespace_nowrap()
                        .child(oneasr_core::format_process_ms(s.elapsed_ms)),
                )
                .into_any_element()
        })
        .collect();

    deferred(
        div()
            .id(menu_id)
            .absolute()
            // Sibling under the 用时 chip's relative wrapper.
            .top(px(26.0))
            .left_0()
            .w(px(crate::app::ui::metrics::TIMING_POP_W))
            .opacity(opacity)
            .mt(y_offset)
            .rounded_lg()
            .border_1()
            .border_color(crate::theme::LINE)
            .bg(crate::theme::PANEL)
            .shadow(popover_menu_shadow())
            .px_3()
            .py_2p5()
            .occlude()
            .on_hover(on_hover)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .mb_1p5()
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::theme::TEXT)
                            .child("处理耗时"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::theme::ACCENT)
                            .child(total),
                    ),
            )
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .bg(crate::theme::LINE_SOFT)
                    .mb_1p5(),
            )
            .child(div().flex().flex_col().gap_1().children(rows)),
    )
    .with_priority(crate::app::ui::metrics::MENU_Z)
}

/// Deferred floating language panel (`MENU_Z`, occludes dismiss scrim).
pub fn floating_lang_menu(
    menu_id: SharedString,
    current: &str,
    option_id_prefix: &str,
    target: crate::app::LangSelectTarget,
    layout: crate::app::LangMenuLayout,
    cx: &mut Context<crate::app::OneAsrApp>,
) -> impl IntoElement {
    let opts: Vec<gpui::AnyElement> = oneasr_core::SOURCE_LANGUAGES
        .iter()
        .map(|lang| {
            let active = current == lang.id;
            lang_menu_option(
                SharedString::from(format!("{option_id_prefix}-{}", lang.id)),
                lang.label,
                active,
                target.clone(),
                lang.id,
                cx,
            )
            .into_any_element()
        })
        .collect();

    let (top, chip_width) = match layout {
        crate::app::LangMenuLayout::Chip => (px(30.), Some(px(crate::app::ui::metrics::LANG_MENU_W))),
        crate::app::LangMenuLayout::FullWidth => (px(38.), None),
    };

    deferred(
        div()
            .id(menu_id)
            .absolute()
            .top(top)
            .right_0()
            .when_some(chip_width, |el, w| el.w(w))
            .when(matches!(layout, crate::app::LangMenuLayout::FullWidth), |el| {
                el.left_0()
            })
            .max_h(px(crate::app::ui::metrics::LANG_MENU_MAX_H))
            .overflow_y_scroll()
            .rounded_lg()
            .border_1()
            .border_color(crate::theme::LINE)
            .bg(crate::theme::PANEL)
            .shadow(popover_menu_shadow())
            .py_1()
            .occlude()
            .children(opts),
    )
    .with_priority(crate::app::ui::metrics::MENU_Z)
}
