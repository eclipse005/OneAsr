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
#[derive(Debug, Clone)]
pub(crate) struct TranscriptCardView {
    pub file: String,
    pub lines: usize,
    pub chars: usize,
    pub dropped_timecodes: bool,
    /// `chars / audio seconds`; `None` when the media length isn't known yet.
    pub density: Option<f64>,
    /// Media length label for the density line (`26:31`).
    pub audio_label: String,
    pub from_file: bool,
    pub note: Option<String>,
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
            .top(px(26.0))
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
                    this.transcript_hover_enter(&id_hover, cx);
                } else {
                    this.transcript_hover_leave(&id_hover, cx);
                }
            }))
            .child(
                div()
                    .text_xs()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(TEXT)
                    .child(view.file),
            )
            .child(div().mt_1().text_xs().text_color(MUTED).child(expand(
                t(L::TRANSCRIPT_SUMMARY),
                &[
                    ("lines", &view.lines.to_string()),
                    ("chars", &view.chars.to_string()),
                ],
            )))
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
            // to be here, before start.
            .when_some(view.density, |el, cps| {
                let ok = (0.2..=12.0).contains(&cps);
                let text = if ok {
                    expand(
                        t(L::TRANSCRIPT_DENSITY_OK),
                        &[("cps", &format!("{cps:.1}")), ("len", &view.audio_label)],
                    )
                } else {
                    expand(
                        t(L::TRANSCRIPT_DENSITY_WARN),
                        &[("cps", &format!("{cps:.1}")), ("len", &view.audio_label)],
                    )
                };
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
            .child(div().mt_2().h(px(1.0)).w_full().bg(LINE))
            .child(
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
                                let tip = t(L::TRANSCRIPT_BREAK_TIP).to_string();
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
                                true,
                                cx.listener(move |this, _, _, cx| this.smart_break_transcript(cx)),
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
                                    let tip = t(L::TRANSCRIPT_REREAD_TIP).to_string();
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
                                    true,
                                    cx.listener(move |this, _, _, cx| this.reread_transcript(cx)),
                                )),
                        )
                    })
                    .child(
                        div()
                            .id(SharedString::from(format!("tr-remove-{id}")))
                            .tooltip({
                                let tip = t(L::TRANSCRIPT_REMOVE_TIP).to_string();
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
                                true,
                                cx.listener(move |this, _, _, cx| this.remove_transcript(cx)),
                            )),
                    ),
            ),
    )
}
