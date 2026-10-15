//! One row of the batch list.
//!
//! Deliberately a **free function**, not an `OneAsrApp` method: `render_list`
//! hands it an owned [`TaskRowView`] snapshot and a [`RowCtx`] of row-invariant
//! state, and the closure it is called from must not capture `self` — it moves
//! into `children` and only borrows `cx` for its `listener`s. A `&mut self`
//! method here would fight the borrow checker for no gain.

use crate::app::prelude::*;
use crate::app::ui::transcript_card::transcript_card;
use crate::app::{LangMenuLayout, LangSelectTarget, OneAsrApp, TaskRowView};

/// Row-invariant state for one repaint: captured once, not once per row.
pub(super) struct RowCtx {
    pub(super) hover_id: Option<String>,
    pub(super) lang_menu: Option<String>,
    /// 任务 id whose 热词组 dropdown is open.
    pub(super) hotword_menu: Option<String>,
    /// 热词组（id, 显示名）快照，下拉选项用。
    pub(super) hotword_groups: Vec<(String, String)>,
    pub(super) active_stage: Option<(String, SharedString)>,
    pub(super) timing_popover: Option<String>,
    pub(super) timing_visible: bool,
    pub(super) timing_progress: f32,
}

/// 把一行的悬停事件落到全局悬停标记上，返回「状态有没有真的变」。
///
/// 纯函数（`task_row_view` 是自由函数，这里同样不碰 GPUI）。存在的理由：
/// 指针从 A 行直接移到 B 行时，A 的 `hover(false)` 也会来一次，而那时标记
/// 已经是 B 了——值没变就不该 `cx.notify()`，重画出来也是同样的像素。
/// 进 B 行、或把当前标记所在的行清空，都是变。
fn hover_row_changed(row_id: &str, hovered: bool, current: Option<&str>) -> bool {
    if hovered {
        current != Some(row_id)
    } else {
        current == Some(row_id)
    }
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
    // 文稿芯片用**另一个**判据，这是有意的：文稿在「排队」那一刻就已经成为
    // 本次运行的输入，所以 Queued 和 Processing 都得锁住
    // （`TaskStatus::locks_transcript_actions` 就是为此存在的）。
    //
    // 之前这个判据只喂给了**卡片里**的取消/更换按钮，芯片自己的点击、悬停
    // 高亮、光标形状一个都没接——于是「处理中」的一行里点「+ 文稿」照样弹出
    // 文件选择框，悬停也照样亮成可点。判据本来就在，缺的是把它接到芯片上。
    let can_edit_transcript = interactive && !row.status.locks_transcript_actions();
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
    let stage_for_row = ctx
        .active_stage
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
    let timing_for_card = row.timing.as_ref().filter(|t| t.has_breakdown());
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

    // 热词组 chip：语言 chip 右侧，同一套交互（点开下拉、处理中锁）。
    // 显示名每帧从 settings 解析（组改名即刻反映），绑定按稳定 id。
    let id_hw = row.id.clone();
    let id_hw_pick = row.id.clone();
    let hw_open = ctx.hotword_menu.as_ref() == Some(&row.id);
    let hw_current = row.hotword_group.clone();
    // 绑了组才值得给它一格药丸的分量（见下面 chip 的注释）。
    let hw_bound = hw_current.is_some();
    // 语言门禁：热词只接中文与英文（`lang::supports_hotwords`）。不支持的语言上
    // 这颗 chip 一律「不适用」且不可点；**绑定不清掉**——语言改回中/英它会恢复，
    // 只是这一趟不会用（管线侧同一道闸见 `stage_rough_transcribe`）。
    let hw_supported = oneasr_core::lang::supports_hotwords(&row.language);
    let can_edit_hw = can_edit_lang && hw_supported;
    // 未绑定时显示**字段名**而不是「无」：这颗控件紧跟在语种 `EN ▾` 后面，
    // 光秃秃一个「无」读不出是热词还是字幕/翻译。显示字段名之后，这一列自己
    // 讲清楚了「空 = 没绑」，而且中英文都不必为它多留宽度。
    let hw_label: SharedString = if !hw_supported {
        // 不支持的语言：显示「不适用」而不是绑定的组名——组名在这儿只会让人
        // 以为「这一行用了热词」。
        t(L::HOTWORD_NOT_APPLICABLE).into()
    } else {
        match &row.hotword_group_label {
            Some(name) => name.clone().into(),
            // 未绑定时只写「无」：这一列的位置已经说明了它是热词（左边语种、右边分离、
            // 文稿），再写「热词：」是重复；tooltip 里仍有一句「点击挑选一组」。
            None => t(L::HOTWORD_NONE).into(),
        }
    };
    let hw_tip: SharedString = if !hw_supported {
        t(L::TIP_HOTWORD_LANG_UNSUPPORTED).into()
    } else if hw_bound {
        hw_label.clone()
    } else {
        t(L::TIP_HOTWORD_UNBOUND).into()
    };
    let hw_menu_float: Option<gpui::AnyElement> = if hw_open && can_edit_hw {
        Some(
            floating_hotword_menu(
                SharedString::from(format!("hw-menu-{id_hw_pick}")),
                &format!("hw-opt-{id_hw_pick}"),
                hw_current.as_deref(),
                id_hw_pick.clone(),
                &ctx.hotword_groups,
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
            let changed = hover_row_changed(&row_id_hover, *hovered, this.hover_row.as_deref());
            if changed {
                if *hovered {
                    this.hover_row = Some(row_id_hover.clone());
                } else {
                    this.hover_row = None;
                }
                cx.notify();
            }
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
                                        // 状态药丸，跟在大小/时长后面。挪到这里是因为它在
                                        // 控制簇里那一列固定宽度（见下面控制区的注释）：宽度
                                        // 按最长阶段标签预留，空闲时却只有「待处理」三个字，
                                        // 于是左右各空 40px，整排控件里夹着一个 46px 的空洞。
                                        // 状态色由左侧色条与图标边框继续承担，信息不丢。
                                        // max_w 兜底带 N/M 计数的超长标签（英文界面尤甚），
                                        // 超出省略而不是把文件名列挤没。
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .max_w(px(STATUS_PILL_MAX_W))
                                                .overflow_hidden()
                                                .px_2()
                                                .py_0p5()
                                                .rounded_full()
                                                .bg(status_bg)
                                                .child(
                                                    div()
                                                        .text_xs()
                                                        .font_weight(
                                                            gpui::FontWeight::MEDIUM,
                                                        )
                                                        .text_color(status_color)
                                                        .whitespace_nowrap()
                                                        .text_ellipsis()
                                                        .child(status_label),
                                                ),
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
                        // 控制区：五颗操作件收进**一个** flex，用比正文更小的 gap。
                        //
                        // 之前它们是同一个 gap_3 里的平铺兄弟，加上每颗都坐在固定宽度的
                        // 列里居中——一颗 45px 宽的「EN ▾」躺在 64px 的列中央，两侧各空
                        // 十几像素，肉眼看到的是「间隔很远」，于是整排像散架而不是一个
                        // 控制区。收进一个紧凑的 flex 之后：
                        //   · 控件彼此挨紧，一眼看成一个动作组；
                        //   · 整组 flex_shrink_0 靠在最右，名字那一列吃掉全部余量；
                        //   · 各列仍是固定宽度，切换语种不会把后面的控件推来推去
                        //     （LANG_COL_PX 当初存在的理由，不能丢）。
                        //
                        // 状态药丸**不在这一组里**了：它原来占着本组最左边一列 132px 的
                        // 固定宽度，空闲时「待处理」只有 52px，列里居中 → 左右各空 40px，
                        // 加上 gap 就是 46px 的视觉间距，夹在 10~14px 的邻居中间格外刺眼；
                        // 而那一列是按最长标签「人声分离 1800/1800」预留的，英文界面下
                        // "Separating vocals 1800/1800" 反而会溢出、压到两侧控件上。
                        // 挪到左侧信息行之后，宽度随内容走，上面两个问题一起消失，
                        // 控制簇也从 470px 缩到 332px，省下的都还给了文件名。
                        .child(
                            div()
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .gap_1p5()
                        // Language select — fixed column so changing the language on one
                        // row does not shift the chips behind it.
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
                                                    // 与 icon_btn 的禁用态同一个底色
                                                    // （以前这里是 PANEL 纯白，
                                                    // 在白色行底上等于没画）。
                                                    BG
                                                })
                                                .opacity(if can_edit_lang {
                                                    1.0
                                                } else {
                                                    DISABLED_OPACITY
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
                                                                // 四颗 chip 统一**不加粗**：MEDIUM 在
                                                                // 12px 下看着像半粗体，和文件名一
                                                                // 比就跳出来。chip 是「安静的一排操作
                                                                // 件」，不该在文件旁边抢分量。
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
                        // 热词组 chip — 语言 chip 右侧，同一套交互（点开下拉、
                        // 处理中锁定）。显示名跟着组改名实时变，绑定是 id。
                        //
                        // 这一列**不设固定宽度**，只给上下限：内容是用户自己起的组名，
                        // 固定宽只有两种坏结果——比内容窄就切掉，比内容宽就被拉出
                        // 一截空白（chip 在竖向 flex 里默认 `align-items: stretch`，
                        // 容器一宽它就跟着宽，于是右边空着一大块）。
                        //
                        // **边框常驻**：曾经「未绑定就不画边框」，理由是别让整行变成
                        // 一堵药丸墙。现在未绑定时写的是「热词：无」——文字本身已经
                        // 说明了一切，边框再时有时无反倒像是渲染不一致。
                        .child(
                            div()
                                .min_w(px(HOTWORD_COL_MIN_PX))
                                .max_w(px(HOTWORD_COL_MAX_PX))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .items_center()
                                .child(
                                    div()
                                        .relative()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .id(SharedString::from(format!(
                                                    "hw-{row_id}"
                                                )))
                                                .max_w(px(HOTWORD_COL_MAX_PX))
                                                .px_2()
                                                .py_0p5()
                                                .rounded_md()
                                                .border_1()
                                                .border_color(if hw_open {
                                                    ACCENT
                                                } else if can_edit_hw {
                                                    LINE
                                                } else {
                                                    LINE_SOFT
                                                })
                                                .bg(if hw_open {
                                                    ACCENT_SOFT
                                                } else {
                                                    BG
                                                })
                                                .opacity(if can_edit_hw {
                                                    1.0
                                                } else {
                                                    DISABLED_OPACITY
                                                })
                                                .when(can_edit_hw, |el| {
                                                    el.cursor_pointer().hover(|s| {
                                                        s.bg(ACCENT_SOFT)
                                                            .border_color(ACCENT)
                                                    })
                                                })
                                                .when(can_edit_hw, |el| {
                                                    el.on_click(cx.listener(
                                                        move |this, _, _, cx| {
                                                            this.toggle_hotword_menu(
                                                                &id_hw, cx,
                                                            );
                                                        },
                                                    ))
                                                })
                                                .tooltip({
                                                    let tip = hw_tip.clone();
                                                    move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: tip.clone(),
                                                        })
                                                        .into()
                                                    }
                                                })
                                                .child(
                                                    div()
                                                        .flex()
                                                        .items_center()
                                                        .gap_1()
                                                        .min_w_0()
                                                        .child(
                                                            div()
                                                                .min_w_0()
                                                                .overflow_hidden()
                                                                .text_xs()
                                                                .whitespace_nowrap()
                                                                // 组名截断时给出省略号，而不是把字硬切一半
                                                                // （`overflow_hidden` 单用是硬切）。完整
                                                                // 名字在这个 chip 的 tooltip 和下拉里都拿得到。
                                                                .text_ellipsis()
                                                                // 未绑定时用弱一档的灰：它是个「可选」的开关，
                                                                // 不该和语言 chip 一样抢注意力。
                                                                .text_color(if hw_bound {
                                                                    if can_edit_hw {
                                                                        TEXT
                                                                    } else {
                                                                        MUTED_SOFT
                                                                    }
                                                                } else if can_edit_hw {
                                                                    MUTED
                                                                } else {
                                                                    MUTED_SOFT
                                                                })
                                                                .child(hw_label),
                                                        )
                                                        .child(
                                                            div()
                                                                .text_xs()
                                                                .text_color(MUTED)
                                                                .child(if hw_open {
                                                                    "▴"
                                                                } else {
                                                                    "▾"
                                                                }),
                                                        ),
                                                ),
                                        )
                                        .children(hw_menu_float),
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
                                            // 同 icon_btn 的禁用态（以前是 PANEL
                                            // 纯白，白底行上等于没画）。
                                            BG
                                        })
                                        .opacity(if can_edit_lang {
                                            1.0
                                        } else {
                                            DISABLED_OPACITY
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
                        // 文稿芯片 —— **一个按钮的两个状态**，位置固定在「分离」与
                        // 「▶」之间：它紧挨着它所改变的那个按钮，因果关系用相邻
                        // 表达。悬停弹出卡片（和「用时」同款），点击把卡片粘住。
                        //
                        // 「字幕比文稿旧」这个状态**不在这里加记号**：主按钮自己会
                        // 从 📁 变回 ▶，那才是该说这件事的地方。
                        .child({
                            let id_tr = row.id.clone();
                            let id_hover = id_tr.clone();
                            let id_click = id_tr.clone();
                            let has = row.has_transcript;
                            // 悬停**永远**出卡片，没有第二个浮层了。原来的提示框是
                            // 一块浮在光标附近的东西：它一旦盖住芯片就会把悬停打断，
                            // 卡片关掉、提示框又冒出来，肉眼就是一闪一闪。「+ 文稿」
                            // 那句话现在由卡片自己讲。
                            let label = if has {
                                t(L::TRANSCRIPT_CHIP).to_string()
                            } else {
                                t(L::TRANSCRIPT_ADD).to_string()
                            };
                            div()
                                .w(px(TRANSCRIPT_COL_PX))
                                .flex_shrink_0()
                                .flex()
                                .justify_center()
                                .items_center()
                                .relative()
                                .child(
                                    div()
                                        .id(SharedString::from(format!("tr-{id_tr}")))
                                        .px_2()
                                        .py_1()
                                        .rounded_md()
                                        .border_1()
                                        .border_color(if has { ACCENT } else { LINE_SOFT })
                                        .bg(if has {
                                            ACCENT_SOFT
                                        } else {
                                            BG
                                        })
                                        .opacity(if can_edit_transcript {
                                            1.0
                                        } else {
                                            DISABLED_OPACITY
                                        })
                                        // 锁定时**不挂** cursor_pointer / hover / on_click。
                                        // 这一段以前是无条件的，于是排队中、处理中的
                                        // 一行里，「+ 文稿」照样亮成可点、照样弹文件框
                                        // ——同一行里 ▶ 🗑 EN 分离 全都灰着，只有它
                                        // 还醒着，比不禁用更糟。悬停出卡片的 on_hover
                                        // 反而要留着：进出会成对，卡片是纯说明的
                                        //（它的按钮另有一道 `actions_enabled` 把关），
                                        // 而且少了 leave 事件，一张已经打开的卡片
                                        // 会永远关不掉。
                                        .when(can_edit_transcript, |el| {
                                            el.cursor_pointer().hover(|s| {
                                                s.bg(ACCENT_SOFT).border_color(ACCENT)
                                            })
                                        })
                                        .on_hover(cx.listener(
                                            move |this, hovered: &bool, _, cx| {
                                                if *hovered {
                                                    this.transcript_hover_enter(&id_hover, cx);
                                                } else {
                                                    this.transcript_hover_leave(&id_hover, cx);
                                                }
                                            },
                                        ))
                                        // 「+ 文稿」点它 = 选一份文件。有文稿之后芯片就只是**纯悬停**的入口，
                                        // 和「用时」芯片一样：点它什么也不做，卡片靠悬停出。
                                        // 取消 / 换文稿走卡片里的按钮，那才是它们的家。
                                        .when(!has && can_edit_transcript, |chip| {
                                            chip.on_click(cx.listener(
                                                move |this, _, _, cx| {
                                                    this.pick_transcript_for(&id_click, cx);
                                                },
                                            ))
                                        })
                                        .child(
                                            div()
                                                .text_xs()
                                                .whitespace_nowrap()
                                                .text_color(if has { ACCENT } else { MUTED })
                                                .child(label),
                                        ),
                                )
                                // Card is a sibling of the chip inside this
                                // relative wrapper, anchored to the chip's right
                                // edge and growing leftward — the chip sits in the
                                // right-hand control cluster, so a left-anchored
                                // card would run off the window.
                                // 渲染门用 `transcript_card_visible()`（progress > 0.01），和「用时」卡片一致。
                                // 用 `is_some()` 的话，一张 opacity 0 的卡片仍然带着
                                // 自己的盒子进命中测试，而它带 `occlude()`——整块区域
                                // 的点击会被一张看不见的卡片吃掉，表现为「点哪都没用」。
                                .when(row.transcript_card_visible, |wrap| {
                                    let id_card = row.id.clone();
                                    let progress = row.transcript_card_progress;
                                    let view = row.transcript_card.clone().unwrap_or_default();
                                    wrap.child(transcript_card(id_card, progress, view, cx))
                                })
                        })
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
                                    cx.listener(move |this, _, _, cx| {
                                        this.close_floating_overlays();
                                        this.delete_task(&id_del, cx);
                                    }),
                                )),
                        ),
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

#[cfg(test)]
mod tests {
    use super::hover_row_changed;

    #[test]
    fn entering_a_row_is_a_change() {
        assert!(hover_row_changed("a", true, None));
        // 已经在这一行上（gpui 的重复 hover(true)）——不是变化。
        assert!(!hover_row_changed("a", true, Some("a")));
        // 指针从别的行移过来。
        assert!(hover_row_changed("b", true, Some("a")));
    }

    #[test]
    fn leaving_the_hovered_row_is_a_change_and_leaving_another_is_not() {
        assert!(hover_row_changed("a", false, Some("a")));
        // 指针从 A 直接走到 B：A 的 hover(false) 到达时标记已经是 B，别再要一帧。
        assert!(!hover_row_changed("a", false, Some("b")));
        assert!(!hover_row_changed("a", false, None));
    }
}
