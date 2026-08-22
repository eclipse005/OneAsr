//! Shared UI building blocks: download rows, buttons, icons, tooltips, popovers.

use gpui::{
    deferred, div, hsla, linear, percentage, point, prelude::*, px, size, svg, Animation,
    AnimationExt as _, App, BoxShadow, ClickEvent, Context, MouseButton, SharedString,
    Transformation, Window, WindowControlArea,
};
use std::f32::consts::TAU;
use std::time::Duration;
use oneasr_core::{DownloadProgress, DownloadState, ModelKind, TaskTiming};

// ─── download rows ──────────────────────────────────────────────────

/// Compact download progress bar under a model path field.
pub fn model_download_row(
    id: &'static str,
    cancel_id: &'static str,
    ready: bool,
    busy: bool,
    progress: Option<&DownloadProgress>,
    kind: ModelKind,
    on_download: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    download_action_row(
        id,
        cancel_id,
        ready,
        busy,
        progress,
        ActionCopy {
            ready_status: "已就绪",
            idle_status: "未下载",
            busy_btn: "下载中…",
            idle_btn: "下载模型",
            ready_btn: "重新下载",
            kind,
        },
        on_download,
        on_cancel,
    )
}

/// CUDA / native runtime components → `{exe}/dll/`, button「安装组件」.
pub fn component_install_row(
    id: &'static str,
    cancel_id: &'static str,
    ready: bool,
    busy: bool,
    progress: Option<&DownloadProgress>,
    on_install: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    download_action_row(
        id,
        cancel_id,
        ready,
        busy,
        progress,
        ActionCopy {
            ready_status: "已安装",
            idle_status: "未安装",
            busy_btn: "安装中…",
            idle_btn: "安装组件",
            ready_btn: "重新安装",
            kind: ModelKind::CudaRuntime,
        },
        on_install,
        on_cancel,
    )
}

struct ActionCopy {
    ready_status: &'static str,
    idle_status: &'static str,
    busy_btn: &'static str,
    idle_btn: &'static str,
    ready_btn: &'static str,
    kind: ModelKind,
}

fn download_action_row(
    id: &'static str,
    cancel_id: &'static str,
    ready: bool,
    busy: bool,
    progress: Option<&DownloadProgress>,
    copy: ActionCopy,
    on_action: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_cancel: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    // Short status only — no filesystem path (model path is already above).
    let status_text = if let Some(p) = progress {
        p.label_for_kind(copy.kind)
    } else if ready {
        copy.ready_status.to_string()
    } else {
        copy.idle_status.to_string()
    };
    let status_color = if ready && !busy {
        crate::ACCENT
    } else if busy {
        crate::WARN
    } else if progress.is_some_and(|p| p.state == DownloadState::Failed) {
        crate::DANGER
    } else {
        crate::MUTED
    };
    let busy_label = copy.busy_btn;
    let action_label = if ready { copy.ready_btn } else { copy.idle_btn };

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_2()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(status_color)
                .whitespace_normal()
                .line_clamp(2)
                .child(status_text),
        )
        .child(
            div().flex().items_center().gap_1p5().children({
                let mut kids: Vec<gpui::AnyElement> = Vec::new();
                if busy {
                    kids.push(
                        div()
                            .id(cancel_id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::LINE)
                            .bg(crate::BG)
                            .text_xs()
                            .text_color(crate::MUTED)
                            .cursor_pointer()
                            .hover(|s| {
                                s.bg(crate::DANGER_SOFT)
                                    .text_color(crate::DANGER)
                                    .border_color(crate::DANGER)
                            })
                            .on_click(on_cancel)
                            .child("取消")
                            .into_any_element(),
                    );
                    kids.push(
                        div()
                            .id(id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::LINE)
                            .bg(crate::BG)
                            .text_xs()
                            .text_color(crate::MUTED)
                            .opacity(0.7)
                            .child(busy_label)
                            .into_any_element(),
                    );
                } else {
                    kids.push(
                        div()
                            .id(id)
                            .flex_shrink_0()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .border_1()
                            .border_color(crate::ACCENT)
                            .bg(crate::ACCENT_SOFT)
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(crate::ACCENT)
                            .cursor_pointer()
                            .hover(|s| s.bg(crate::ACCENT).text_color(gpui::rgb(0xffffff)))
                            .on_click(on_action)
                            .child(action_label)
                            .into_any_element(),
                    );
                }
                kids
            }),
        )
}

// ─── titlebar / toolbar ─────────────────────────────────────────────

/// Brand mark: teal tile + SVG waveform → subtitle (matches app-icon.ico).
/// On hover, the waveform does a light left-right wiggle (repeat while hovered).
pub fn app_logo(hovered: bool) -> impl IntoElement {
    let icon = svg()
        .size(px(22.))
        .path("icons/logo.svg")
        .text_color(gpui::rgb(0xffffff));

    // Distinct element ids so GPUI remounts cleanly when hover starts/stops.
    let icon_el = if hovered {
        icon.with_animation(
            "logo-wiggle",
            Animation::new(Duration::from_millis(480))
                .repeat()
                .with_easing(linear),
            |svg, delta| {
                // ~3 half-swings per cycle → lively but not frantic.
                let phase = delta * TAU * 3.0;
                let wiggle = phase.sin();
                // ±12° rotation (fraction of a full turn).
                let turn = (12.0 / 360.0) * wiggle;
                // Tiny scale pulse so it feels springy, not just rotating.
                let pulse = 1.0 + 0.06 * phase.cos().abs();
                svg.with_transformation(
                    Transformation::rotate(percentage(turn)).with_scaling(size(pulse, pulse)),
                )
            },
        )
        .into_any_element()
    } else {
        icon.into_any_element()
    };

    div()
        .id(if hovered {
            "app-logo-hot"
        } else {
            "app-logo"
        })
        .size(px(36.))
        .rounded_xl()
        .bg(crate::LOGO)
        .shadow(vec![BoxShadow {
            color: hsla(
                174. / 360.,
                0.55,
                0.28,
                if hovered { 0.42 } else { 0.28 },
            ),
            offset: point(px(0.), px(if hovered { 2. } else { 1. })),
            blur_radius: px(if hovered { 10. } else { 6. }),
            spread_radius: px(0.),
        }])
        .flex()
        .items_center()
        .justify_center()
        .child(icon_el)
}

/// One caption button (min / max / close) for the custom title bar.
/// Hit routing is via `WindowControlArea` only — no `on_click` (see `render_titlebar`).
/// `close` gets the Windows-style red hover with a white glyph.
pub fn caption_btn(
    id: &'static str,
    icon: &'static str,
    area: WindowControlArea,
    close: bool,
) -> impl IntoElement {
    let group: SharedString = format!("{id}-hover").into();
    let icon_el = svg()
        .size(px(14.))
        .path(icon)
        .text_color(crate::MUTED)
        .when(close, |el| {
            el.group_hover(group.clone(), |s| s.text_color(crate::PANEL))
        });
    div()
        .id(id)
        .w(px(46.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .window_control_area(area)
        .when(close, |el| el.group(group))
        .hover(move |s| {
            if close {
                s.bg(crate::DANGER)
            } else {
                s.bg(crate::LINE_SOFT)
            }
        })
        .child(icon_el)
}

#[derive(Clone, Copy)]
pub enum BtnKind {
    /// Sole solid CTA (全部开始 / 空状态添加).
    Primary,
    /// Outlined secondary (添加 / 后端选项).
    Secondary,
    /// Low-emphasis destructive/utility (清空) — text only.
    Quiet,
}

/// Settings gear.
/// - **Spin**: only while any model / CUDA DLL download is in flight (open or closed).
/// - **Tint**: settings drawer open, or download running (so closed-panel DL is still visible).
pub fn settings_gear_btn(
    open: bool,
    downloading: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let active = open || downloading;
    let gear = svg()
        .size(px(18.))
        .path("icons/gear.svg")
        .text_color(if active { crate::ACCENT } else { crate::MUTED });

    // Distinct element ids so GPUI remounts when download starts/stops (reliable spin on/off).
    let gear_el = if downloading {
        gear.with_animation(
            "settings-gear-spin",
            Animation::new(Duration::from_secs(4))
                .repeat()
                .with_easing(linear),
            |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
        )
        .into_any_element()
    } else {
        gear.into_any_element()
    };

    div()
        .id(if downloading {
            "settings-gear-dl"
        } else {
            "settings-gear"
        })
        .size(px(36.))
        .rounded_lg()
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .bg(if open {
            crate::ACCENT_SOFT
        } else {
            crate::PANEL
        })
        .border_1()
        .border_color(if open {
            crate::ACCENT_SOFT
        } else {
            crate::LINE
        })
        .cursor_pointer()
        .hover(|s| {
            if open {
                s.bg(crate::ACCENT_SOFT).border_color(crate::ACCENT)
            } else {
                s.bg(crate::BG).border_color(crate::ACCENT)
            }
        })
        .on_click(on_click)
        .child(gear_el)
}

/// Primary CTA. When disabled: not clickable + tooltip explains why.
pub fn btn_cta(
    label: &str,
    enabled: bool,
    disabled_tip: &'static str,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("cta-{label}-{enabled}"));
    let tip: SharedString = disabled_tip.into();
    let mut el = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(crate::ACCENT)
        .text_color(crate::PANEL)
        .border_1()
        .border_color(crate::ACCENT)
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .opacity(if enabled { 1.0 } else { 0.42 })
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| s.opacity(0.92))
            .on_click(on_click);
    } else {
        el = el.tooltip(move |_, cx| {
            cx.new(|_| NameTooltip { text: tip.clone() }).into()
        });
    }
    el
}

pub fn btn(
    label: &str,
    kind: BtnKind,
    enabled: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!("btn-{label}-{}-{enabled}", kind as u8));
    let (bg, fg, border, opacity) = match (kind, enabled) {
        (BtnKind::Primary, true) => (crate::ACCENT, crate::PANEL, crate::ACCENT, 1.0),
        (BtnKind::Primary, false) => (crate::ACCENT, crate::PANEL, crate::ACCENT, 0.42),
        (BtnKind::Secondary, true) => (crate::PANEL, crate::TEXT, crate::LINE, 1.0),
        (BtnKind::Secondary, false) => (crate::PANEL, crate::MUTED_SOFT, crate::LINE, 1.0),
        (BtnKind::Quiet, true) => (crate::PANEL, crate::MUTED, crate::PANEL, 1.0),
        (BtnKind::Quiet, false) => (crate::PANEL, crate::MUTED_SOFT, crate::PANEL, 1.0),
    };
    let mut el = div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(bg)
        .text_color(fg)
        .border_1()
        .border_color(border)
        .font_weight(match kind {
            BtnKind::Primary => gpui::FontWeight::SEMIBOLD,
            BtnKind::Secondary | BtnKind::Quiet => gpui::FontWeight::NORMAL,
        })
        .opacity(opacity)
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                BtnKind::Primary => s.opacity(0.92),
                BtnKind::Secondary => s.border_color(crate::ACCENT),
                BtnKind::Quiet => s.text_color(crate::DANGER).bg(crate::DANGER_SOFT),
            })
            .on_click(on_click);
    }
    el
}


// ─── icon / button helpers ──────────────────────────────────────────

/// Lightweight hover tooltip for truncated filenames.
pub struct NameTooltip {
    pub text: SharedString,
}

impl crate::Render for NameTooltip {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(crate::TEXT)
            .text_color(crate::PANEL)
            .text_xs()
            .max_w(px(420.))
            .child(self.text.clone())
    }
}

#[derive(Clone, Copy)]
pub enum IconKind {
    Play,
    Trash,
    /// Open containing folder for completed SRT.
    Folder,
}

/// Compact icon action — SVG only (no emoji). Always visible; disabled = gray.
///
/// Note: GPUI SVG needs an explicit `.text_color()` (currentColor); parent
/// cascade alone often leaves stroke/fill invisible.
pub fn icon_btn(
    kind: IconKind,
    tip: &'static str,
    enabled: bool,
    row_hovered: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let id = SharedString::from(format!(
        "ico-{tip}-{enabled}-{}-{row_hovered}",
        kind as u8
    ));
    let tip_s: SharedString = tip.into();
    let (fg, bg, border) = if !enabled {
        (crate::MUTED_SOFT, crate::BG, crate::LINE_SOFT)
    } else if row_hovered {
        match kind {
            IconKind::Play => (crate::ACCENT, crate::ACCENT_SOFT, crate::ACCENT_SOFT),
            IconKind::Trash => (crate::MUTED, crate::PANEL, crate::LINE),
            IconKind::Folder => (crate::ACCENT, crate::ACCENT_SOFT, crate::ACCENT_SOFT),
        }
    } else {
        match kind {
            IconKind::Folder => (crate::ACCENT, crate::ACCENT_SOFT, crate::ACCENT_SOFT),
            IconKind::Play => (crate::MUTED, crate::PANEL, crate::LINE),
            IconKind::Trash => (crate::MUTED, crate::PANEL, crate::LINE),
        }
    };
    let mut el = div()
        .id(id)
        .size(px(32.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .bg(bg)
        .border_1()
        .border_color(border)
        .text_color(fg)
        .opacity(if enabled { 1.0 } else { 0.55 })
        .child(
            svg()
                .size(px(17.))
                .path(icon_svg_path(kind))
                .text_color(fg),
        )
        .tooltip(move |_, cx| {
            cx.new(|_| NameTooltip {
                text: tip_s.clone(),
            })
            .into()
        });
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                IconKind::Play => s.bg(crate::ACCENT_SOFT).border_color(crate::ACCENT),
                IconKind::Trash => s.bg(crate::DANGER_SOFT).border_color(crate::DANGER),
                // Keep the bg light so the ACCENT icon stays readable (unlike a
                // solid-ACCENT fill, which would eat the icon of the same color).
                IconKind::Folder => s.bg(crate::ACCENT_SOFT).border_color(crate::ACCENT),
            })
            .on_click(on_click);
    }
    el
}

pub fn icon_svg_path(kind: IconKind) -> &'static str {
    match kind {
        IconKind::Play => "icons/play.svg",
        IconKind::Trash => "icons/trash.svg",
        IconKind::Folder => "icons/folder.svg",
    }
}

// ─── popover / menu helpers ─────────────────────────────────────────

/// Full-window click-outside scrim under open floating menus (`MENU_DISMISS_Z`).
/// Mounted on the app root so toolbar / list / status bar are all outside-click targets.
pub fn popover_dismiss_layer(cx: &mut Context<crate::OneAsrApp>) -> impl IntoElement {
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
    .with_priority(crate::MENU_DISMISS_Z)
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
    target: crate::LangSelectTarget,
    lang_id: &'static str,
    cx: &mut Context<crate::OneAsrApp>,
) -> impl IntoElement {
    div()
        .id(option_id)
        .px_2p5()
        .py_1p5()
        .cursor_pointer()
        .bg(if active {
            crate::ACCENT_SOFT
        } else {
            crate::PANEL
        })
        .hover(|s| s.bg(if active { crate::ACCENT_SOFT } else { crate::BG }))
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
                            crate::ACCENT
                        } else {
                            crate::TEXT
                        })
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .child(label.to_string()),
                )
                .when(active, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(crate::ACCENT)
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
                        .text_color(if is_hot { crate::TEXT } else { crate::MUTED })
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
                        .text_color(if is_hot { crate::ACCENT } else { crate::MUTED })
                        .whitespace_nowrap()
                        .child(crate::format_process_ms(s.elapsed_ms)),
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
            .w(px(crate::TIMING_POP_W))
            .opacity(opacity)
            .mt(y_offset)
            .rounded_lg()
            .border_1()
            .border_color(crate::LINE)
            .bg(crate::PANEL)
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
                            .text_color(crate::TEXT)
                            .child("处理耗时"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(crate::ACCENT)
                            .child(total),
                    ),
            )
            .child(
                div()
                    .h(px(1.))
                    .w_full()
                    .bg(crate::LINE_SOFT)
                    .mb_1p5(),
            )
            .child(div().flex().flex_col().gap_1().children(rows)),
    )
    .with_priority(crate::MENU_Z)
}

/// Deferred floating language panel (`MENU_Z`, occludes dismiss scrim).
pub fn floating_lang_menu(
    menu_id: SharedString,
    current: &str,
    option_id_prefix: &str,
    target: crate::LangSelectTarget,
    layout: crate::LangMenuLayout,
    cx: &mut Context<crate::OneAsrApp>,
) -> impl IntoElement {
    let opts: Vec<gpui::AnyElement> = crate::SOURCE_LANGUAGES
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
        crate::LangMenuLayout::Chip => (px(30.), Some(px(crate::LANG_MENU_W))),
        crate::LangMenuLayout::FullWidth => (px(38.), None),
    };

    deferred(
        div()
            .id(menu_id)
            .absolute()
            .top(top)
            .right_0()
            .when_some(chip_width, |el, w| el.w(w))
            .when(matches!(layout, crate::LangMenuLayout::FullWidth), |el| {
                el.left_0()
            })
            .max_h(px(crate::LANG_MENU_MAX_H))
            .overflow_y_scroll()
            .rounded_lg()
            .border_1()
            .border_color(crate::LINE)
            .bg(crate::PANEL)
            .shadow(popover_menu_shadow())
            .py_1()
            .occlude()
            .children(opts),
    )
    .with_priority(crate::MENU_Z)
}
