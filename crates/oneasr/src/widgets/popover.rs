//! Deferred popovers: dismiss layer, language menu, timing breakdown.

use gpui::{
    App, BoxShadow, Context, MouseButton, SharedString, Window, deferred, div, hsla, point,
    prelude::*, px,
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
        .hover(|s| {
            s.bg(if active {
                crate::theme::ACCENT_SOFT
            } else {
                crate::theme::BG
            })
        })
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
                    el.child(div().text_xs().text_color(crate::theme::ACCENT).child("✓"))
                }),
        )
}

/// Processing-time breakdown card under the **用时** chip (`MENU_Z`).
///
/// Parent must be a `relative` wrapper around the chip so `left_0` aligns to it.
pub fn timing_breakdown_popover(
    menu_id: SharedString,
    timing: &TaskTiming,
    // `6.5 倍速` chip for the header line, already formatted by
    // `realtime_factor_label`; `None` hides it.
    rtfx: Option<SharedString>,
    progress: f32,
    on_hover: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let p = progress.clamp(0.0, 1.0);
    let opacity = p;
    // Rise from slightly below (dialog-ish, not full modal).
    let y_offset = px(6.0 * (1.0 - p));
    let total = timing.total_label();
    let max_stage = timing
        .stages
        .iter()
        .map(|s| s.elapsed_ms)
        .max()
        .unwrap_or(1);

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
                        .text_color(if is_hot {
                            crate::theme::TEXT
                        } else {
                            crate::theme::MUTED
                        })
                        .font_weight(if is_hot {
                            gpui::FontWeight::MEDIUM
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(s.stage.label(oneasr_core::i18n::ui_lang()).to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(if is_hot {
                            crate::theme::ACCENT
                        } else {
                            crate::theme::MUTED
                        })
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
                            .child(crate::i18n::t(crate::i18n::L::TIMING_TITLE)),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            // Rate chip, left of the total it is derived from:
                            // media length ÷ this wall clock. The plate stays
                            // neutral on purpose — the accent-topped chips in the
                            // app (完成 / 用时) are things you can act on, and this
                            // one must not read as clickable.
                            .when_some(rtfx, |el, rtfx| {
                                el.child(
                                    div()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_md()
                                        .bg(crate::theme::MEDIA_PLATE)
                                        .text_xs()
                                        .text_color(crate::theme::MUTED)
                                        .whitespace_nowrap()
                                        .child(rtfx),
                                )
                            })
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(crate::theme::ACCENT)
                                    .child(total),
                            ),
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
        crate::app::LangMenuLayout::Chip => {
            (px(30.), Some(px(crate::app::ui::metrics::LANG_MENU_W)))
        }
        crate::app::LangMenuLayout::FullWidth => (px(38.), None),
    };

    deferred(
        div()
            .id(menu_id)
            .absolute()
            .top(top)
            .right_0()
            .when_some(chip_width, |el, w| el.w(w))
            .when(
                matches!(layout, crate::app::LangMenuLayout::FullWidth),
                |el| el.left_0(),
            )
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

/// One row inside the 热词组 menu: label + ✓ when it is the current pick.
fn hotword_menu_option(
    option_id: SharedString,
    label: &str,
    active: bool,
    task_id: String,
    group: Option<String>,
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
        .hover(|s| {
            s.bg(if active {
                crate::theme::ACCENT_SOFT
            } else {
                crate::theme::BG
            })
        })
        .on_click(cx.listener(move |this, _, _, cx| {
            this.pick_hotword_group(&task_id, group.clone(), cx);
        }))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap_2()
                .child(
                    div()
                        // 面板宽度是固定的（`HOTWORD_MENU_W`），而组名由用户起——
                        // 一条 `whitespace_nowrap` 的长名字会直接顶出面板右沿。所以
                        // 标签必须先吃掉剩余宽度再省略：`flex_1` 给它弹性、`min_w_0`
                        // 允许它缩到内容宽度以下，`overflow_hidden + text_ellipsis`
                        // 才收得住。少了 `flex_1`，标签仍按内容宽排版，后两个属性
                        // 都无从生效——这是「省略号不出现」的典型原因。
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_xs()
                        .text_color(if active {
                            crate::theme::ACCENT
                        } else {
                            crate::theme::TEXT
                        })
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(label.to_string()),
                )
                .when(active, |el| {
                    el.child(div().text_xs().text_color(crate::theme::ACCENT).child("✓"))
                }),
        )
}

/// Floating 热词组 menu under a task row's chip (`MENU_Z`): 无 + every group,
/// plus a footer entry that opens the hotword panel. The group list is read
/// live from settings, so renaming / adding / deleting a group shows up here
/// immediately — the row keeps binding the stable id.
pub fn floating_hotword_menu(
    menu_id: SharedString,
    option_id_prefix: &str,
    current: Option<&str>,
    task_id: String,
    groups: &[(String, String)],
    cx: &mut Context<crate::app::OneAsrApp>,
) -> impl IntoElement {
    let mut opts: Vec<gpui::AnyElement> = Vec::with_capacity(groups.len() + 2);
    opts.push(
        hotword_menu_option(
            SharedString::from(format!("{option_id_prefix}-none")),
            crate::i18n::t(crate::i18n::L::HOTWORD_NONE),
            current.is_none(),
            task_id.clone(),
            None,
            cx,
        )
        .into_any_element(),
    );
    for (id, label) in groups {
        let active = current == Some(id.as_str());
        opts.push(
            hotword_menu_option(
                SharedString::from(format!("{option_id_prefix}-{id}")),
                label,
                active,
                task_id.clone(),
                Some(id.clone()),
                cx,
            )
            .into_any_element(),
        );
    }
    // 刻意**不**在底部放「管理热词组…」：工具栏那颗「热词」chip 就是它的正门，
    // 就在列表正上方一排。放在这里等于在一张只该列「选哪个」的菜单里混进一个
    // 「去别处改」的动作——菜单一旦开始导航，就不再是菜单了。

    deferred(
        div()
            .id(menu_id)
            .absolute()
            .top(px(30.))
            .right_0()
            .w(px(crate::app::ui::metrics::HOTWORD_MENU_W))
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
