//! One row of the batch list.
//!
//! Deliberately a **free function**, not an `OneAsrApp` method: `render_list`
//! hands it an owned [`TaskRowView`] snapshot and a [`RowCtx`] of row-invariant
//! state, and the closure it is called from must not capture `self` — it moves
//! into `children` and only borrows `cx` for its `listener`s. A `&mut self`
//! method here would fight the borrow checker for no gain.

use crate::app::prelude::*;
use crate::app::{LangMenuLayout, LangSelectTarget, OneAsrApp, TaskRowView};

/// Row-invariant state for one repaint: captured once, not once per row.
pub(super) struct RowCtx {
    pub(super) hover_id: Option<String>,
    pub(super) lang_menu: Option<String>,
    pub(super) active_stage: Option<(String, SharedString)>,
    pub(super) timing_popover: Option<String>,
    pub(super) timing_visible: bool,
    pub(super) timing_progress: f32,
}

pub(super) fn task_row_view(
    ix: usize,
    row: &TaskRowView,
    ctx: &RowCtx,
    cx: &mut Context<OneAsrApp>,
) -> impl IntoElement + use<> {
    let interactive = row.interactive;
    let row_opacity = row.opacity;
    let id_start = row.id.clone();
    let id_del = row.id.clone();
    let id_open = row.id.clone();
    let id_lang = row.id.clone();
    let row_id = row.id.clone();
    let row_id_hover = row.id.clone();
    let id_lang_pick = row.id.clone();
    let done_with_out = row.status == TaskStatus::Done && row.has_output;
    let primary_kind = if done_with_out {
        IconKind::Folder
    } else {
        IconKind::Play
    };
    let primary_tip = if done_with_out {
        t(L::TIP_OPEN_OUTPUT)
    } else {
        t(L::TIP_START)
    };
    let primary_enabled = interactive
        && if done_with_out {
            true
        } else {
            matches!(row.status, TaskStatus::Pending | TaskStatus::Error)
        };
    let can_delete = interactive && !row.status.locks_row_actions();
    let can_edit_lang = interactive && !row.status.locks_row_actions();
    let lang_open = ctx.lang_menu.as_ref() == Some(&row.id);
    let lang_meta = source_language_by_id(&row.language);
    let lang_short = lang_meta.map(|l| l.short).unwrap_or("?");
    let lang_current = row.language.clone();
    let id_sep = row.id.clone();
    let sep_on = row.vocal_separation;
    let err = row.error.clone();
    let name = row.name.clone();
    let name_tip = row.name.clone();
    let size_l = row.size_label.clone();
    let status = row.status;
    let is_video = row.is_video;
    let is_hovered = interactive && ctx.hover_id.as_ref() == Some(&row.id);
    let qn = row.queue_rank;

    // Meta: size · duration · status text (no free-floating status circle).
    let dur = row.duration_label.clone();
    let stage_for_row = ctx.active_stage
        .as_ref()
        .filter(|(sid, _)| sid == &row.id)
        .map(|(_, s)| s.as_ref());
    let (status_label, status_color, status_bg) =
        status_pill_style(status, qn, stage_for_row, ui_lang());
    let timing_total = row
        .timing
        .as_ref()
        .filter(|t| t.has_breakdown())
        .map(|t| t.total_label());
    let timing_for_card = row
        .timing
        .as_ref()
        .filter(|t| t.has_breakdown());
    let timing_open = ctx.timing_popover.as_ref() == Some(&row.id) && ctx.timing_visible;
    let timing_pop_p = if timing_open || ctx.timing_popover.as_ref() == Some(&row.id) {
        ctx.timing_progress
    } else {
        0.0
    };
    let meta_media = format!("{size_l}  ·  {dur}");
    let accent = row_accent(status);

    let row_bg = if is_hovered {
        ROW_HOVER
    } else if ix % 2 == 1 {
        ZEBRA
    } else {
        PANEL
    };

    let lang_menu_float: Option<gpui::AnyElement> = if lang_open && can_edit_lang {
        Some(
            floating_lang_menu(
                SharedString::from(format!("lang-menu-{id_lang_pick}")),
                &lang_current,
                &format!("lang-opt-{id_lang_pick}"),
                LangSelectTarget::Task(id_lang_pick.clone()),
                LangMenuLayout::Chip,
                cx,
            )
            .into_any_element(),
        )
    } else {
        None
    };

    div()
        .id(SharedString::from(format!("task-{row_id}")))
        .flex()
        .flex_col()
        .w_full()
        .min_w_0()
        // Never shrink: the list is a scroll container, so rows
        // past one screenful must overflow (→ scrollbar), not
        // compress — every child here truncates, so Taffy's
        // min-content floor is ~0 and without this 11+ rows
        // squash into each other.
        .flex_shrink_0()
        // Allow floating language / timing menus to paint outside the row box.
        .when(!lang_open && !timing_open, |el| el.overflow_hidden())
        .border_b_1()
        .border_color(LINE_SOFT)
        .bg(row_bg)
        .opacity(row_opacity)
        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
            if !interactive {
                return;
            }
            if *hovered {
                this.hover_row = Some(row_id_hover.clone());
            } else if this.hover_row.as_ref() == Some(&row_id_hover) {
                this.hover_row = None;
            }
            cx.notify();
        }))
        .child(
            div()
                .flex()
                .w_full()
                .min_w_0()
                // Left status accent as border (full row height, no stretch API needed).
                .border_l_4()
                .border_color(accent)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .px_3()
                        .py_3()
                        .flex()
                        .items_center()
                        .gap_3()
                        .child(
                            div()
                                .flex_shrink_0()
                                .child(media_type_icon(is_video, status)),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                // Keep overflow when timing card is closed so long names clip.
                                .when(!timing_open, |el| el.overflow_hidden())
                                .pr_2()
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "name-{row_id}"
                                        )))
                                        .w_full()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .whitespace_nowrap()
                                        .child(name)
                                        .tooltip(move |_, cx| {
                                            cx.new(|_| NameTooltip {
                                                text: name_tip.clone().into(),
                                            })
                                            .into()
                                        }),
                                )
                                .child({
                                    // Meta: size · media length · [用时 chip + hover card]
                                    let id_timing = row.id.clone();
                                    let id_timing_leave = row.id.clone();
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_1p5()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(MUTED_SOFT)
                                                .whitespace_nowrap()
                                                .child(meta_media),
                                        )
                                        .when_some(
                                            timing_total.zip(timing_for_card),
                                            |el, (total_label, timing)| {
                                                let id_card = id_timing.clone();
                                                let id_card_leave =
                                                    id_timing_leave.clone();
                                                let show_card = timing_pop_p > 0.01;
                                                el.child(
                                                    div()
                                                        .text_xs()
                                                        .text_color(MUTED_SOFT)
                                                        .child("·"),
                                                )
                                                // Chip + card share one relative box so
                                                // the popover anchors under 用时, not the row.
                                                .child(
                                                    div()
                                                        .relative()
                                                        .flex_shrink_0()
                                                        .child(
                                                            div()
                                                                .id(SharedString::from(
                                                                    format!(
                                                                        "timing-chip-{id_timing}"
                                                                    ),
                                                                ))
                                                                .px_1p5()
                                                                .py_0p5()
                                                                .rounded_md()
                                                                .cursor_default()
                                                                .bg(if timing_open {
                                                                    ACCENT_SOFT
                                                                } else {
                                                                    MEDIA_PLATE
                                                                })
                                                                .hover(|s| {
                                                                    s.bg(ACCENT_SOFT)
                                                                })
                                                                .on_hover(cx.listener(
                                                                    move |this,
                                                                          hovered: &bool,
                                                                          _,
                                                                          cx| {
                                                                        if *hovered {
                                                                            this.timing_hover_enter(
                                                                                &id_timing,
                                                                                cx,
                                                                            );
                                                                        } else {
                                                                            this.timing_hover_leave(
                                                                                &id_timing_leave,
                                                                                cx,
                                                                            );
                                                                        }
                                                                    },
                                                                ))
                                                                .child(
                                                                    div()
                                                                        .text_xs()
                                                                        .text_color(
                                                                            if timing_open {
                                                                                ACCENT
                                                                            } else {
                                                                                MUTED
                                                                            },
                                                                        )
                                                                        .whitespace_nowrap()
                                                                        .child(
                                                                            crate::i18n::took_time(
                                                                                &total_label,
                                                                            ),
                                                                        ),
                                                                ),
                                                        )
                                                        .when(show_card, |wrap| {
                                                            wrap.child(
                                                                timing_breakdown_popover(
                                                                    SharedString::from(
                                                                        format!(
                                                                            "timing-pop-{id_card}"
                                                                        ),
                                                                    ),
                                                                    timing,
                                                                    row.rtfx_label
                                                                        .clone()
                                                                        .map(SharedString::from),
                                                                    timing_pop_p,
                                                                    cx.listener(
                                                                        move |this,
                                                                              hovered: &bool,
                                                                              _,
                                                                              cx| {
                                                                            if *hovered {
                                                                                this.timing_hover_enter(
                                                                                    &id_card,
                                                                                    cx,
                                                                                );
                                                                            } else {
                                                                                this.timing_hover_leave(
                                                                                    &id_card_leave,
                                                                                    cx,
                                                                                );
                                                                            }
                                                                        },
                                                                    ),
                                                                ),
                                                            )
                                                        }),
                                                )
                                            },
                                        )
                                }),
                        )
                        // Status — informational, left of the control
                        // cluster so all clickable pills (语言 / 分离 /
                        // 开始 / 删除) sit together on the right.
                        .child(
                            div()
                                .w(px(STATUS_COL_PX))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_full()
                                        .bg(status_bg)
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(status_color)
                                                .whitespace_nowrap()
                                                .child(status_label),
                                        ),
                                ),
                        )
                        // Language select — fixed column so status length never shifts it.
                        .child(
                            div()
                                .w(px(LANG_COL_PX))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    div()
                                        .relative()
                                        .child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "lang-{row_id}"
                                                )))
                                                .px_2()
                                                .py_0p5()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(if lang_open {
                                                    ACCENT
                                                } else if can_edit_lang {
                                                    LINE
                                                } else {
                                                    LINE_SOFT
                                                })
                                                .bg(if lang_open {
                                                    ACCENT_SOFT
                                                } else if can_edit_lang {
                                                    BG
                                                } else {
                                                    PANEL
                                                })
                                                .when(can_edit_lang, |el| {
                                                    el.cursor_pointer().hover(|s| {
                                                        s.bg(ACCENT_SOFT)
                                                            .border_color(ACCENT)
                                                    })
                                                })
                                                .when(can_edit_lang, |el| {
                                                    el.on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.toggle_lang_menu(
                                                                &id_lang, cx,
                                                            );
                                                        },
                                                    ))
                                                })
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1()
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .font_weight(
                                                                    gpui::FontWeight::MEDIUM,
                                                                )
                                                                .text_color(
                                                                    if can_edit_lang {
                                                                        TEXT
                                                                    } else {
                                                                        MUTED_SOFT
                                                                    },
                                                                )
                                                                .whitespace_nowrap()
                                                                .child(
                                                                    lang_short
                                                                        .to_string(),
                                                                ),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(MUTED)
                                                                .child(if lang_open {
                                                                    "▴"
                                                                } else {
                                                                    "▾"
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .children(lang_menu_float),
                                ),
                        )
                        // Vocal separation toggle — same row idiom as
                        // the language chip: compact pill, filled when
                        // on, per-task override of the settings default.
                        .child(
                            div()
                                .w(px(SEPARATE_COL_PX))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    div()
                                        .id(SharedString::from(format!(
                                            "sep-{id_sep}"
                                        )))
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if sep_on {
                                            ACCENT
                                        } else if can_edit_lang {
                                            LINE
                                        } else {
                                            LINE_SOFT
                                        })
                                        .bg(if sep_on {
                                            ACCENT
                                        } else if can_edit_lang {
                                            BG
                                        } else {
                                            PANEL
                                        })
                                        .when(can_edit_lang, |el| {
                                            el.cursor_pointer().hover(|s| {
                                                s.bg(ACCENT_SOFT)
                                                    .border_color(ACCENT)
                                            })
                                        })
                                        .when(can_edit_lang, |el| {
                                            el.on_click(cx.listener(
                                                move |this, _, _, cx| {
                                                    this.toggle_task_separation(
                                                        &id_sep, cx,
                                                    );
                                                },
                                            ))
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(
                                                    gpui::FontWeight::MEDIUM,
                                                )
                                                .whitespace_nowrap()
                                                .text_color(if sep_on {
                                                    PANEL
                                                } else if can_edit_lang {
                                                    TEXT
                                                } else {
                                                    MUTED_SOFT
                                                })
                                                .child(t(L::SEP_CHIP)),
                                        )
                                        .tooltip({
                                            let tip = if sep_on {
                                                t(L::TIP_SEP_ON)
                                            } else {
                                                t(L::TIP_SEP_OFF)
                                            };
                                            move |_, cx| {
                                                cx.new(|_| NameTooltip {
                                                    text: tip.into(),
                                                })
                                                .into()
                                            }
                                        }),
                                ),
                        )
                        // Two fixed slots: primary (开始 | 打开字幕) + 删除.
                        .child(
                            div()
                                .w(px(ACTIONS_COL_PX))
                                .flex_shrink_0()
                                .flex()
                                .gap_2()
                                .justify_end()
                                .items_center()
                                .child(icon_btn(
                                    primary_kind,
                                    primary_tip,
                                    primary_enabled,
                                    is_hovered,
                                    cx.listener(move |this, _, _, cx| {
                                        this.close_floating_overlays();
                                        if this
                                            .tasks
                                            .iter()
                                            .find(|t| t.id == id_start)
                                            .map(|t| {
                                                t.status == TaskStatus::Done
                                                    && t.output_file.is_some()
                                            })
                                            .unwrap_or(false)
                                        {
                                            this.open_task_output(&id_open, cx);
                                        } else {
                                            this.start_one(&id_start, cx);
                                        }
                                    }),
                                ))
                                .child(icon_btn(
                                    IconKind::Trash,
                                    t(L::TIP_DELETE),
                                    can_delete,
                                    is_hovered,
                                    cx.listener(move |this, _, _, cx| {
                                        this.close_floating_overlays();
                                        this.delete_task(&id_del, cx);
                                    }),
                                )),
                        ),
                ),
        )
        .when(err.is_some(), |el| {
            el.child(
                div()
                    .px_3()
                    .pb_2()
                    .pl(px(48.))
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_md()
                            .bg(DANGER_SOFT)
                            .text_xs()
                            .text_color(DANGER)
                            .child(err.unwrap_or_default()),
                    ),
            )
        })
}
