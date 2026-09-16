//! The empty state: the animated wave and the copy inviting a first drop.

use crate::app::prelude::*;
use crate::app::OneAsrApp;

impl OneAsrApp {
    pub(super) fn render_empty_wave(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = self.empty_wave_clock.elapsed().as_secs_f32();
        let heights = empty_wave_heights(t, self.empty_wave_smooth_x, self.empty_wave_amp);
        let entity = cx.entity().clone();

        div()
            .id("empty-wave")
            .w_full()
            .h(px(EMPTY_WAVE_MAX_H + 24.0))
            .relative()
            .cursor_default()
            // Capture layout bounds so mouse X can be mapped 0..=1 along the strip.
            .child(
                canvas(
                    {
                        let entity = entity.clone();
                        move |bounds, _window, cx| {
                            entity.update(cx, |app, _cx| {
                                app.empty_wave_bounds = Some(bounds);
                            });
                        }
                    },
                    |_bounds, (), _window, _cx| {},
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                if let Some(b) = this.empty_wave_bounds {
                    let w = f32::from(b.size.width).max(1.0);
                    let local = (f32::from(event.position.x) - f32::from(b.left())) / w;
                    this.empty_wave_cursor_x = local.clamp(0.0, 1.0);
                    if !this.empty_wave_hover {
                        this.empty_wave_hover = true;
                    }
                    cx.notify();
                }
            }))
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                if this.empty_wave_hover != *hovered {
                    this.empty_wave_hover = *hovered;
                    cx.notify();
                }
            }))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .px_6()
                    .flex()
                    .items_center()
                    .justify_between()
                    .children((0..EMPTY_WAVE_BARS).map(move |i| {
                        let h = heights[i];
                        let u = ((h - EMPTY_WAVE_FLAT_H)
                            / (EMPTY_WAVE_MAX_H - EMPTY_WAVE_FLAT_H))
                            .clamp(0.0, 1.0);
                        let opacity = 0.40 + 0.55 * u;
                        div()
                            .w(px(2.5))
                            .h(px(h))
                            .rounded_full()
                            .bg(ACCENT)
                            .opacity(opacity)
                    })),
            )
    }
}

const EMPTY_WAVE_BARS: usize = 64;
const EMPTY_WAVE_MAX_H: f32 = 78.0;
const EMPTY_WAVE_FLAT_H: f32 = 5.0;
const EMPTY_WAVE_SIGMA: f32 = 0.09;

/// Bar heights in px. Flat when `amp≈0`; local gaussian bulge at `cx` (0..=1) otherwise.
fn empty_wave_heights(t_secs: f32, cx: f32, amp: f32) -> [f32; EMPTY_WAVE_BARS] {
    let mut out = [EMPTY_WAVE_FLAT_H; EMPTY_WAVE_BARS];
    if amp < 0.004 {
        return out;
    }
    let n = (EMPTY_WAVE_BARS - 1) as f32;
    let sigma2 = 2.0 * EMPTY_WAVE_SIGMA * EMPTY_WAVE_SIGMA;
    for (i, slot) in out.iter_mut().enumerate() {
        let x = i as f32 / n;
        let dx = x - cx;
        let env = (-(dx * dx) / sigma2).exp();
        // Gentle shimmer only under the bulge (not whole-line jitter).
        let ripple = (x * TAU * 5.0 - t_secs * 5.5).sin() * 0.18;
        let fine = (x * TAU * 11.0 + t_secs * 3.2).sin() * 0.07;
        let u = (env * (0.88 + ripple + fine)).clamp(0.0, 1.0) * amp;
        *slot = EMPTY_WAVE_FLAT_H + u * (EMPTY_WAVE_MAX_H - EMPTY_WAVE_FLAT_H);
    }
    out
}
