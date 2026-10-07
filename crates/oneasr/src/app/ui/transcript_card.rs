//! The transcript card: what pops out when the pointer rests on the chip.
//!
//! Anchored to the chip's **right** edge and growing leftward, because the chip
//! sits in the right-hand control cluster. Rise + fade, same as the 用时 card;
//! it also carries its own `on_hover` so the pointer can travel onto it and
//! press a button.
//!
//! No preview of the transcript itself — see [`crate::app::transcript_ui`].

use crate::app::prelude::*;
use crate::app::transcript_ui::expand;

/// Row data the card needs, so the closure below doesn't capture `self`.
#[derive(Debug, Clone, Default)]
pub(crate) struct TranscriptCardView {
    /// 这一行挂了文稿没有。`false` 时卡片只讲「挂一份会怎样」，其余一概没有——
    /// 没有文稿就没有行数、字数、语速，也就没有按钮可按。
    pub has_transcript: bool,
    pub file: String,
    pub lines: usize,
    pub chars: usize,
    pub dropped_timecodes: bool,
    /// `units / audio seconds`, in the transcript's own unit (see
    /// [`oneasr_core::transcript::SpeechRate`]); `None` when the media length
    /// isn't known yet.
    pub rate: Option<oneasr_core::transcript::SpeechRate>,
    /// Media length label for the density line (`26:31`).
    pub audio_label: String,
    pub from_file: bool,
    pub note: Option<String>,
    /// 排队或处理中时，文稿操作按钮置灰并禁用。
    pub actions_enabled: bool,
}

pub(crate) fn transcript_card(
    id: String,
    progress: f32,
    view: TranscriptCardView,
    cx: &mut Context<OneAsrApp>,
) -> impl IntoElement {
    let p = progress.clamp(0.0, 1.0);
    let id_hover = id.clone();

    deferred(
        div()
            .id(SharedString::from(format!("tr-card-{id}")))
            .absolute()
            // `top: 100%` —— 锚在芯片列**底边之下**，而不是一个写死的像素。
            //
            // 写死像素（像「用时」卡片那样的 26px）就赌芯片比它矮：赌输了卡片会
            // 盖住芯片，光标在两者之间来回，命中判定每一帧翻一次，卡片就一闪一闪。
            // 百分比跟着实际高度走，永远不重叠。下面再留 4px 让指针横穿得过。
            .top(relative(1.0))
            .mt(px(4.0))
            .right_0()
            .w(px(TRANSCRIPT_CARD_W))
            .opacity(p)
            .rounded_md()
            .border_1()
            .border_color(LINE)
            .bg(PANEL)
            .shadow(popover_menu_shadow())
            .px_3()
            .py_2p5()
            .occlude()
            // Keeps the card open while the pointer is on it — that is what
            // makes the buttons reachable, and why a purely hover-driven card
            // would need the pin.
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.transcript_card_hover_enter(&id_hover, cx);
                } else {
                    this.transcript_card_hover_leave(&id_hover, cx);
                }
            }))
            // 「还没挂文稿」时只有这一句话。没有提示框来抢这个位置——提示框浮在
            // 光标附近，盖住芯片就会打断悬停，卡片一闪一闪。
            .when(!view.has_transcript, |el| {
                el.child(
                    div()
                        .text_xs()
                        .text_color(MUTED)
                        .child(t(L::TRANSCRIPT_TIP_NONE)),
                )
            })
            .when(view.has_transcript, |el| {
                el.child(
                    div()
                        .text_xs()
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(TEXT)
                        .child(view.file.clone()),
                )
            })
            .when(view.has_transcript, |el| {
                el.child(div().mt_1().text_xs().text_color(MUTED).child(expand(
                    t(L::TRANSCRIPT_SUMMARY),
                    &[
                        ("lines", &view.lines.to_string()),
                        ("chars", &view.chars.to_string()),
                    ],
                )))
            })
            // The one thing the program does to your file's content: drop the
            // timings. Say it, once, here.
            .when(view.dropped_timecodes, |el| {
                el.child(
                    div()
                        .mt_1()
                        .text_xs()
                        .text_color(MUTED)
                        .child(t(L::TRANSCRIPT_TIMINGS_DROPPED)),
                )
            })
            // The guard against silent degradation: when the transcript and the
            // audio are not the same thing, forced alignment does not error — it
            // hands back a timeline that looks fine and drifts. This number has
            // to be here, before start. The unit is the transcript's own script
            // (字/秒 for CJK, 词/秒 for spaced writing) — one ruler for both
            // would call every English transcript a mismatch.
            .when_some(view.rate, |el, rate| {
                let ok = rate.plausible();
                let template = match (rate.spaced, ok) {
                    (true, true) => t(L::TRANSCRIPT_RATE_WORDS_OK),
                    (true, false) => t(L::TRANSCRIPT_RATE_WORDS_WARN),
                    (false, true) => t(L::TRANSCRIPT_RATE_CHARS_OK),
                    (false, false) => t(L::TRANSCRIPT_RATE_CHARS_WARN),
                };
                let text = expand(
                    template,
                    &[
                        ("rate", &format!("{:.1}", rate.per_second)),
                        ("len", &view.audio_label),
                    ],
                );
                el.child(
                    div()
                        .mt_1()
                        .text_xs()
                        .text_color(if ok { MUTED } else { DANGER })
                        .child(text),
                )
            })
            .when_some(view.note, |el, note| {
                el.child(div().mt_1().text_xs().text_color(ACCENT).child(note))
            })
            .when(view.has_transcript, |el| {
                el.child(div().mt_2().h(px(1.0)).w_full().bg(LINE)).child(
                    div()
                        .mt_2()
                        .flex()
                        .gap_1p5()
                        .child(
                            // `btn` takes no tooltip, so the wrapper carries it —
                            // "不改一个字、原文件不会被改动" is the whole promise of
                            // this button and it deserves to be readable.
                            div()
                                .id(SharedString::from(format!("tr-break-{id}")))
                                .tooltip({
                                    let tip = if view.actions_enabled {
                                        t(L::TRANSCRIPT_BREAK_TIP)
                                    } else {
                                        t(L::TRANSCRIPT_ACTIONS_LOCKED)
                                    }
                                    .to_string();
                                    move |_, cx| {
                                        cx.new(|_| NameTooltip {
                                            text: tip.clone().into(),
                                        })
                                        .into()
                                    }
                                })
                                .child(btn(
                                    t(L::TRANSCRIPT_BTN_BREAK),
                                    BtnKind::Secondary,
                                    view.actions_enabled,
                                    cx.listener(move |this, _, _, cx| {
                                        this.smart_break_transcript(cx)
                                    }),
                                )),
                        )
                        // Re-read is the round trip for "I edited the file in my own
                        // editor". Only meaningful when the transcript came from a
                        // file at all.
                        .when(view.from_file, |el| {
                            el.child(
                                div()
                                    .id(SharedString::from(format!("tr-reread-{id}")))
                                    .tooltip({
                                        let tip = if view.actions_enabled {
                                            t(L::TRANSCRIPT_REREAD_TIP)
                                        } else {
                                            t(L::TRANSCRIPT_ACTIONS_LOCKED)
                                        }
                                        .to_string();
                                        move |_, cx| {
                                            cx.new(|_| NameTooltip {
                                                text: tip.clone().into(),
                                            })
                                            .into()
                                        }
                                    })
                                    .child(btn(
                                        t(L::TRANSCRIPT_BTN_REREAD),
                                        BtnKind::Secondary,
                                        view.actions_enabled,
                                        cx.listener(move |this, _, _, cx| {
                                            this.reread_transcript(cx)
                                        }),
                                    )),
                            )
                        })
                        .child(
                            div()
                                .id(SharedString::from(format!("tr-remove-{id}")))
                                .tooltip({
                                    let tip = if view.actions_enabled {
                                        t(L::TRANSCRIPT_REMOVE_TIP)
                                    } else {
                                        t(L::TRANSCRIPT_ACTIONS_LOCKED)
                                    }
                                    .to_string();
                                    move |_, cx| {
                                        cx.new(|_| NameTooltip {
                                            text: tip.clone().into(),
                                        })
                                        .into()
                                    }
                                })
                                .child(btn(
                                    t(L::TRANSCRIPT_BTN_REMOVE),
                                    BtnKind::Secondary,
                                    view.actions_enabled,
                                    cx.listener(move |this, _, _, cx| this.remove_transcript(cx)),
                                )),
                        ),
                )
            }),
    )
    // 和「用时」卡片同一个层级（`MENU_Z`）：浮层之间要有一处说了算的地方，
    // 否则谁后画谁在上面，全看行在列表里的顺序。
    .with_priority(MENU_Z)
}
