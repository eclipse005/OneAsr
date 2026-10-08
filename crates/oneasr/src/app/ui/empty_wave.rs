//! The empty state: the animated wave and the copy inviting a first drop.

use crate::app::OneAsrApp;
use crate::app::prelude::*;

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
                    let next = local.clamp(0.0, 1.0);
                    let needs_frame = wave_move_needs_frame(
                        next,
                        this.empty_wave_cursor_x,
                        this.empty_wave_hover,
                    );
                    this.empty_wave_cursor_x = next;
                    if !this.empty_wave_hover {
                        this.empty_wave_hover = true;
                    }
                    if needs_frame {
                        cx.notify();
                    }
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
                        let u = ((h - EMPTY_WAVE_FLAT_H) / (EMPTY_WAVE_MAX_H - EMPTY_WAVE_FLAT_H))
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

/// 一次 mousemove 要不要为它重画一帧（纯函数，便于单测）。
///
/// mousemove 是逐像素来的：同一个像素内的重复事件算出来的目标逐位相同，
/// 波形不会动，也就不必 `cx.notify()`。指针在带上时 RAF 本来就在跑
/// （见 `animations_active`），缓动照旧推进，这里省掉的只是多余的一帧。
/// 比位模式而不是 `!=`：`0.0` 与 `-0.0` 不该共用一个「没变」。
/// 首次进入（目标没动但指针刚来）仍要一帧——起伏得从零开始爬。
fn wave_move_needs_frame(next_x: f32, cursor_x: f32, was_hover: bool) -> bool {
    next_x.to_bits() != cursor_x.to_bits() || !was_hover
}

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

#[cfg(test)]
mod tests {
    use super::wave_move_needs_frame;

    #[test]
    fn a_repeated_move_inside_one_pixel_needs_no_frame() {
        // mousemove 逐像素来，同一个像素的重复事件目标逐位相同。
        assert!(!wave_move_needs_frame(0.42, 0.42, true));
        // 移到了别处 —— 要一帧，波形得跟过去。
        assert!(wave_move_needs_frame(0.43, 0.42, true));
    }

    #[test]
    fn the_first_move_needs_a_frame_even_though_the_target_did_not_move() {
        // 指针刚落到带上：目标没动，但起伏得从零开始爬。
        assert!(wave_move_needs_frame(0.5, 0.5, false));
    }

    #[test]
    fn signed_zero_is_treated_as_a_moved_target() {
        // `0.0 == -0.0`，用 `!=` 会把这次移动当成「没变」而漏掉一帧。
        assert!(wave_move_needs_frame(0.0, -0.0, true));
    }

    #[test]
    fn clamped_edges_repeat_without_a_frame() {
        // 指针横向越出带子时目标被夹到 0/1，重复事件同样不必重画。
        assert!(!wave_move_needs_frame(0.0, 0.0, true));
        assert!(!wave_move_needs_frame(1.0, 1.0, true));
    }
}
