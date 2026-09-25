//! The task list: scrolling skeleton, empty state, and the per-row loop.
//!
//! A single row is built by [`super::task_row::task_row_view`]; this file only
//! decides how many and where.

use crate::app::prelude::*;
use crate::app::ui::task_row::{RowCtx, task_row_view};

impl OneAsrApp {
    pub(super) fn render_list(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        self.drain_row_anims();

        if self.tasks.is_empty() {
            let title = empty_state_title(ui_lang());
            let not_ready = self.model_status == ModelStatus::NotReady;
            let subtitle = empty_state_subtitle(ui_lang(), not_ready);
            self.tick_empty_wave();
            let wave = self.render_empty_wave(cx);
            return div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .w_full()
                .min_w_0()
                // Full-width interactive wave sits above the copy block.
                .child(wave)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_3()
                        .px_6()
                        .mt_4()
                        .child(
                            div()
                                .text_base()
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(MUTED)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(MUTED_SOFT)
                                .text_center()
                                .max_w(px(320.))
                                .child(subtitle),
                        )
                        .child(
                            div()
                                .mt_2()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(btn(
                                    t(L::ADD_FILES),
                                    BtnKind::Primary,
                                    true,
                                    cx.listener(|this, _, _, cx| this.add_files_dialog(cx)),
                                ))
                                .when(not_ready, |el| {
                                    el.child(btn(
                                        t(L::OPEN_SETTINGS),
                                        BtnKind::Secondary,
                                        true,
                                        cx.listener(|this, _, _, cx| {
                                            if !this.settings_open {
                                                this.toggle_settings(cx);
                                            }
                                        }),
                                    ))
                                }),
                        ),
                )
                .into_any_element();
        }

        // Rows stay in `tasks` order. Exit tombstones fade in place (1→0);
        // enter map fades new rows (0→1). Exiting rows are non-interactive.
        let items = self.snapshot_task_rows(Instant::now());
        let hover_id = self.hover_row.clone();
        let lang_menu = self.lang_menu.clone();
        let active_stage = self.active_stage.clone();
        let timing_popover = self.timing_popover.clone();
        let timing_visible = self.timing_popover_visible();
        let timing_progress = self.timing_popover_progress();

            // Row-invariant state, captured once for the whole repaint.
            let row_ctx = RowCtx {
                hover_id,
                lang_menu,
                active_stage,
                timing_popover,
                timing_visible,
                timing_progress,
            };

        div()
            .id("task-list")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(items.into_iter().enumerate().map(move |(ix, row)| {
                task_row_view(ix, &row, &row_ctx, cx)
            }))
            .into_any_element()
    }
}
