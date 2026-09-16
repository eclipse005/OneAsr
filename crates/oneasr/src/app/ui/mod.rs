//! View layer: interface copy, layout metrics, and the window's rendering.
//!
//! Sibling modules hold the rendering halves of `OneAsrApp`; they live under
//! `app` so they can read its private state without widening visibility.

use crate::DismissMenus;
use crate::app::prelude::*;

pub(crate) mod chrome;
pub(crate) mod empty_wave;
pub(crate) mod metrics;
pub(crate) mod settings_drawer;
pub(crate) mod settings_sections;
pub(crate) mod stats_grid;
pub(crate) mod status_bar;
pub(crate) mod stats_panel;
pub(crate) mod task_list;
pub(crate) mod task_row;
pub(crate) mod text;
pub(crate) mod util;

impl Render for OneAsrApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Drop select state that can no longer paint (locked / deleted / drawer closed).
        self.sync_lang_select_state();
        self.tick_timing_popover();

        // Drive drawer / row-hover fades at display refresh.
        if self.animations_active() {
            window.request_animation_frame();
        }

        let drawer_p = self.settings_progress();
        let menu_open = self.any_menu_open();
        let stats_open = self.stats_open;

        div()
            .id("oneasr-root")
            .track_focus(&self.focus_handle)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(BG)
            .text_color(TEXT)
            // Primary + explicit CJK fallbacks (stripped Win10 / deleted 雅黑).
            .font(self.ui_font.font.clone())
            .on_action(cx.listener(|this, _: &DismissMenus, _, cx| {
                this.dismiss_menus(cx);
            }))
            .child(self.render_titlebar())
            .child(self.render_toolbar(cx))
            .child(
                // Relative shell: list fills; settings drawer overlays from the right.
                div()
                    .flex_1()
                    .relative()
                    .min_h_0()
                    .min_w_0()
                    .overflow_hidden()
                    .child(
                        div()
                            .id("task-drop-zone")
                            .size_full()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .bg(PANEL)
                            .border_2()
                            .border_color(LINE)
                            .overflow_hidden()
                            .can_drop(|drag, _, _| drag.is::<ExternalPaths>())
                            .drag_over::<ExternalPaths>(|style, _, _, _| {
                                style
                                    .border_color(ACCENT)
                                    .border_2()
                                    .border_dashed()
                                    .bg(ACCENT_SOFT)
                            })
                            .on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                                this.add_paths(paths.paths().to_vec(), cx);
                            }))
                            .child(self.render_list(cx)),
                    )
                    // Keep mounted while animating closed (p > 0).
                    // `occlude`: block hits + scroll to the list underneath (otherwise
                    // overflow_y_scroll on both layers double-notifies every wheel tick —
                    // a likely amplifier for the hard-to-repro settings scroll crash).
                    .when(drawer_p > 0.001, |el| {
                        let slide = (1.0 - drawer_p) * SETTINGS_W;
                        let fade = 0.25 + 0.75 * drawer_p;
                        el.child(
                            div()
                                .id("settings-drawer")
                                .absolute()
                                .top_0()
                                .bottom_0()
                                .right(px(-slide))
                                .w(px(SETTINGS_W))
                                .opacity(fade)
                                .border_l_1()
                                .border_color(LINE)
                                .bg(PANEL)
                                .occlude()
                                .shadow(vec![
                                    BoxShadow {
                                        color: hsla(0.0, 0.0, 0.0, 0.06 * drawer_p),
                                        blur_radius: px(4.0),
                                        spread_radius: px(0.0),
                                        offset: point(px(-1.0), px(0.0)),
                                    },
                                    BoxShadow {
                                        color: hsla(0.0, 0.0, 0.0, 0.12 * drawer_p),
                                        blur_radius: px(28.0),
                                        spread_radius: px(-2.0),
                                        offset: point(px(-10.0), px(0.0)),
                                    },
                                ])
                                .child(self.render_settings(cx)),
                        )
                    }),
            )
            .child(self.render_status_bar(cx))
            .when(stats_open, |el| el.child(self.render_stats_panel(cx)))
            .when(menu_open || stats_open, |el| {
                el.child(popover_dismiss_layer(cx))
            })
    }
}
