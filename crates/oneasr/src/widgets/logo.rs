//! The OneAsr mark in the title bar.

use gpui::{
    div, hsla, linear, point, prelude::*, px, radians, size, svg, Animation,
    AnimationExt as _, BoxShadow,
    Transformation,
};
use std::f32::consts::TAU;
use std::time::Duration;

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
                // `radians`, NOT `percentage`: the swing crosses negative, and
                // `percentage()` debug-asserts on < 0 — hovering the logo in a
                // debug build panicked the app within ~0.3s. Radians are free.
                svg.with_transformation(
                    Transformation::rotate(radians(turn * TAU)).with_scaling(size(pulse, pulse)),
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
        .bg(crate::theme::LOGO)
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
