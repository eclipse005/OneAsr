//! Buttons, pills, icons and the name tooltip.

use gpui::{
    div, linear, percentage, prelude::*, px, svg, Animation,
    AnimationExt as _, App, ClickEvent, Context, SharedString,
    Transformation, Window, WindowControlArea,
};
use std::time::Duration;

/// Actions used by the self-drawn caption buttons on platforms that need
/// regular mouse handlers instead of a native non-client hit test.
#[derive(Clone, Copy)]
pub enum CaptionAction {
    Minimize,
    Maximize,
    Close,
}

/// One caption button (min / max / close) for the custom title bar.
/// `close` gets the Windows-style red hover with a white glyph.
pub fn caption_btn(
    id: &'static str,
    icon: &'static str,
    area: WindowControlArea,
    close: bool,
    action: CaptionAction,
) -> impl IntoElement {
    let group: SharedString = format!("{id}-hover").into();
    let icon_el = svg()
        .size(px(14.))
        .path(icon)
        .text_color(crate::theme::MUTED)
        .when(close, |el| {
            el.group_hover(group.clone(), |s| s.text_color(crate::theme::PANEL))
        });
    let button = div()
        .id(id)
        .w(px(46.))
        .h_full()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .window_control_area(area)
        .when(close, |el| el.group(group))
        .hover(move |s| {
            if close {
                s.bg(crate::theme::DANGER)
            } else {
                s.bg(crate::theme::LINE_SOFT)
            }
        })
        .child(icon_el);

    #[cfg(target_os = "linux")]
    let button = match action {
        CaptionAction::Minimize => button.on_click(|_, window, _| window.minimize_window()),
        CaptionAction::Maximize => button.on_click(|_, window, _| window.zoom_window()),
        CaptionAction::Close => button.on_click(|_, _, cx| cx.quit()),
    };
    #[cfg(not(target_os = "linux"))]
    let _ = action;

    button
}

#[derive(Clone, Copy)]
pub enum BtnKind {
    /// Sole solid CTA (全部开始 / 空状态添加).
    Primary,
    /// Outlined secondary (添加 / 后端选项).
    Secondary,
    /// Outlined low-emphasis utility (重置) — bordered like Secondary but
    /// muted text; the destructive tint only appears on hover.
    Quiet,
    /// Outlined destructive (清空) — red text / border at rest, fills on hover.
    Danger,
}

/// Settings gear.
/// - **Spin**: only while any model download is in flight (open or closed).
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
        .text_color(if active { crate::theme::ACCENT } else { crate::theme::MUTED });

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
            crate::theme::ACCENT_SOFT
        } else {
            crate::theme::PANEL
        })
        .border_1()
        .border_color(if open {
            crate::theme::ACCENT_SOFT
        } else {
            crate::theme::LINE
        })
        .cursor_pointer()
        .hover(|s| {
            if open {
                s.bg(crate::theme::ACCENT_SOFT).border_color(crate::theme::ACCENT)
            } else {
                s.bg(crate::theme::BG).border_color(crate::theme::ACCENT)
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
        .bg(crate::theme::ACCENT)
        .text_color(crate::theme::PANEL)
        .border_1()
        .border_color(crate::theme::ACCENT)
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
        (BtnKind::Primary, true) => (crate::theme::ACCENT, crate::theme::PANEL, crate::theme::ACCENT, 1.0),
        (BtnKind::Primary, false) => (crate::theme::ACCENT, crate::theme::PANEL, crate::theme::ACCENT, 0.42),
        (BtnKind::Secondary, true) => (crate::theme::PANEL, crate::theme::TEXT, crate::theme::LINE, 1.0),
        (BtnKind::Secondary, false) => (crate::theme::PANEL, crate::theme::MUTED_SOFT, crate::theme::LINE, 1.0),
        (BtnKind::Quiet, true) => (crate::theme::PANEL, crate::theme::MUTED, crate::theme::LINE, 1.0),
        (BtnKind::Quiet, false) => (crate::theme::PANEL, crate::theme::MUTED_SOFT, crate::theme::LINE, 1.0),
        (BtnKind::Danger, true) => (crate::theme::DANGER_SOFT, crate::theme::DANGER, crate::theme::DANGER, 1.0),
        (BtnKind::Danger, false) => (crate::theme::PANEL, crate::theme::MUTED_SOFT, crate::theme::LINE, 1.0),
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
            BtnKind::Secondary | BtnKind::Quiet | BtnKind::Danger => gpui::FontWeight::NORMAL,
        })
        .opacity(opacity)
        .child(label.to_string());
    if enabled {
        el = el
            .cursor_pointer()
            .hover(|s| match kind {
                BtnKind::Primary => s.opacity(0.92),
                BtnKind::Secondary => s.border_color(crate::theme::ACCENT),
                BtnKind::Quiet => s.text_color(crate::theme::DANGER).bg(crate::theme::DANGER_SOFT).border_color(crate::theme::DANGER),
                BtnKind::Danger => s.bg(crate::theme::DANGER).text_color(crate::theme::PANEL),
            })
            .on_click(on_click);
    }
    el
}

/// Two-state pill for settings rows.
///
/// [`btn`] derives its element id from the label, so two different rows reusing
/// the same "开启 / 关闭" wording would produce colliding ids. This takes the id
/// explicitly, which is what a settings row needs.
pub fn pill(
    id: &'static str,
    label: &'static str,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_lg()
        .text_sm()
        .bg(if active { crate::theme::ACCENT } else { crate::theme::PANEL })
        .text_color(if active { crate::theme::PANEL } else { crate::theme::TEXT })
        .border_1()
        .border_color(if active { crate::theme::ACCENT } else { crate::theme::LINE })
        .font_weight(if active {
            gpui::FontWeight::SEMIBOLD
        } else {
            gpui::FontWeight::NORMAL
        })
        .cursor_pointer()
        .hover(|s| s.border_color(crate::theme::ACCENT))
        .child(label)
        .on_click(on_click)
}

// ─── icon / button helpers ──────────────────────────────────────────

/// Lightweight hover tooltip for truncated filenames.
pub struct NameTooltip {
    pub text: SharedString,
}

impl gpui::Render for NameTooltip {
    fn render(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .px_2()
            .py_1()
            .rounded_md()
            .bg(crate::theme::TEXT)
            .text_color(crate::theme::PANEL)
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
        (crate::theme::MUTED_SOFT, crate::theme::BG, crate::theme::LINE_SOFT)
    } else if row_hovered {
        match kind {
            IconKind::Play => (crate::theme::ACCENT, crate::theme::ACCENT_SOFT, crate::theme::ACCENT_SOFT),
            IconKind::Trash => (crate::theme::MUTED, crate::theme::PANEL, crate::theme::LINE),
            IconKind::Folder => (crate::theme::ACCENT, crate::theme::ACCENT_SOFT, crate::theme::ACCENT_SOFT),
        }
    } else {
        match kind {
            IconKind::Folder => (crate::theme::ACCENT, crate::theme::ACCENT_SOFT, crate::theme::ACCENT_SOFT),
            IconKind::Play => (crate::theme::MUTED, crate::theme::PANEL, crate::theme::LINE),
            IconKind::Trash => (crate::theme::MUTED, crate::theme::PANEL, crate::theme::LINE),
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
                IconKind::Play => s.bg(crate::theme::ACCENT_SOFT).border_color(crate::theme::ACCENT),
                IconKind::Trash => s.bg(crate::theme::DANGER_SOFT).border_color(crate::theme::DANGER),
                // Keep the bg light so the ACCENT icon stays readable (unlike a
                // solid-ACCENT fill, which would eat the icon of the same color).
                IconKind::Folder => s.bg(crate::theme::ACCENT_SOFT).border_color(crate::theme::ACCENT),
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
