//! The ten sections of the settings drawer.
//!
//! Each builder renders one `section(..)` card. They take [`SettingsFormState`]
//! by reference so the drawer's snapshot is read, never re-derived, and return
//! `impl IntoElement + use<>` — precise capturing is required because the
//! drawer chains all ten into one element, and the default (edition 2024)
//! capture set would make each one conservatively borrow `self` and `cx`.

use crate::app::prelude::*;
use crate::app::ui::settings_drawer::SettingsFormState;

/// One settings card: rounded, bordered, padded. All ten sections go through
/// this so their chrome cannot drift apart.
pub(super) fn settings_section(body: AnyElement) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .rounded_lg()
        .border_1()
        .border_color(LINE_SOFT)
        .bg(BG)
        .px_3()
        .py_2p5()
        .child(body)
}

impl OneAsrApp {
/// 默认语言 + 字幕长度
pub(super) fn render_settings_language(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body = {

        let open = self.settings_lang_open;
        let cur_label = source_language_by_id(&form.language)
            .map(|l| l.label)
            .unwrap_or("中文普通话");

        // 语言 | 字幕长度 并排平分
        div()
            .flex()
            .items_start()
            .gap_2p5()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT)
                            .child("默认语言"),
                    )
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .child(
                                div()
                                    .id("settings-lang-trigger")
                                    .w_full()
                                    .px_2p5()
                                    .py_1p5()
                                    .rounded_md()
                                    .border_1()
                                    .border_color(if open {
                                        ACCENT
                                    } else {
                                        LINE
                                    })
                                    .bg(if open { ACCENT_SOFT } else { BG })
                                    .cursor_pointer()
                                    .hover(|s| {
                                        s.bg(ACCENT_SOFT).border_color(ACCENT)
                                    })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.toggle_settings_lang(cx);
                                    }))
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .text_color(TEXT)
                                                    .whitespace_nowrap()
                                                    .child(cur_label.to_string()),
                                            )
                                            .child(
                                                div()
                                                    .text_xs()
                                                    .text_color(MUTED)
                                                    .child(if open {
                                                        "▴"
                                                    } else {
                                                        "▾"
                                                    }),
                                            ),
                                    ),
                            )
                            .when(open, |el| {
                                el.child(floating_lang_menu(
                                    "settings-lang-menu".into(),
                                    &form.language,
                                    "settings-lang-opt",
                                    LangSelectTarget::Settings,
                                    LangMenuLayout::FullWidth,
                                    cx,
                                ))
                            }),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT)
                            .child("字幕长度"),
                    )
                    .child(
                        div().flex().gap_1p5().children(
                            [
                                ("short", "短"),
                                ("standard", "标准"),
                                ("loose", "宽松"),
                            ]
                            .into_iter()
                            .map(|(id, label)| {
                                let active = form.length_preset == id;
                                btn(
                                    label,
                                    if active {
                                        BtnKind::Primary
                                    } else {
                                        BtnKind::Secondary
                                    },
                                    true,
                                    cx.listener(move |this, _, _, cx| {
                                        if this.settings.subtitle_length_preset
                                            == id
                                        {
                                            return;
                                        }
                                        this.settings.subtitle_length_preset =
                                            id.into();
                                        this.mark_settings_dirty(cx);
                                    }),
                                )
                            }),
                        ),
                    ),
            )
            .into_any_element()

    };
    settings_section(body)
}

/// 分段时长
pub(super) fn render_settings_chunk_duration(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body = {

        // 分段时长：30–180s 预设（默认 60）；短尾 <15s 运行时合并
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT)
                            .child("分段时长"),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED)
                            .child(format!(
                                "{} 秒 · {}–{}",
                                form.chunk_target, CHUNK_TARGET_MIN_SEC, CHUNK_TARGET_MAX_SEC
                            )),
                    ),
            )
            .child(
                div().flex().gap_1p5().children(
                    CHUNK_TARGET_PRESETS.iter().copied().map(|(sec, label)| {
                        let active = form.chunk_target == sec;
                        btn(
                            label,
                            if active {
                                BtnKind::Primary
                            } else {
                                BtnKind::Secondary
                            },
                            true,
                            cx.listener(move |this, _, _, cx| {
                                if this.settings.chunk_target_seconds == sec {
                                    return;
                                }
                                this.settings.chunk_target_seconds = sec;
                                this.mark_settings_dirty(cx);
                            }),
                        )
                    }),
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(
                        div()
                            .text_xs()
                            .text_color(MUTED)
                            .child("建议 4GB 显存使用 60 秒分段时长"),
                    ),
            )
            .into_any_element()

    };
    settings_section(body)
}

/// 输出格式 + 中文输出
pub(super) fn render_settings_output(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .items_start()
                                    .gap_2p5()
                                    .child(
                                        div()
                                            // 中文输出 takes its natural width (3 pills) and
                                            // may shrink; 输出格式 absorbs the remaining space.
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::MEDIUM)
                                                            .text_color(TEXT)
                                                            .whitespace_nowrap()
                                                            .child("输出格式"),
                                                    ),
                                            )
                                            .child(
                                                div()
                                                    .flex()
                                                    .gap_1p5()
                                                    .child(btn(
                                                        "SRT",
                                                        if form.output_srt {
                                                            BtnKind::Primary
                                                        } else {
                                                            BtnKind::Secondary
                                                        },
                                                        true,
                                                        cx.listener(|this, _, _, cx| {
                                                            if !this.settings.output_txt {
                                                                this.flash_hint(
                                                                    "至少保留一种输出格式",
                                                                    cx,
                                                                );
                                                                return;
                                                            }
                                                            this.settings.output_srt =
                                                                !this.settings.output_srt;
                                                            this.mark_settings_dirty(cx);
                                                        }),
                                                    ))
                                                    .child(btn(
                                                        "TXT",
                                                        if form.output_txt {
                                                            BtnKind::Primary
                                                        } else {
                                                            BtnKind::Secondary
                                                        },
                                                        true,
                                                        cx.listener(|this, _, _, cx| {
                                                            if !this.settings.output_srt {
                                                                this.flash_hint(
                                                                    "至少保留一种输出格式",
                                                                    cx,
                                                                );
                                                                return;
                                                            }
                                                            this.settings.output_txt =
                                                                !this.settings.output_txt;
                                                            this.mark_settings_dirty(cx);
                                                        }),
                                                    )),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .flex()
                                            .flex_col()
                                            .gap_1()
                                            .child(
                                                div()
                                                    .flex()
                                                    .items_center()
                                                    .justify_between()
                                                    .child(
                                                        div()
                                                            .text_sm()
                                                            .font_weight(gpui::FontWeight::MEDIUM)
                                                            .text_color(TEXT)
                                                            .whitespace_nowrap()
                                                            .child("中文输出"),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_xs()
                                                            .text_color(MUTED)
                                                            .whitespace_nowrap()
                                                            .child("仅中文/粤语"),
                                                    ),
                                            )
                                            .child(
                                                div().flex().gap_1().children(TextScript::ALL.map(
                                                    |script| {
                                                        let active = form.text_script == script;
                                                        btn(
                                                            script.label(),
                                                            if active {
                                                                BtnKind::Primary
                                                            } else {
                                                                BtnKind::Secondary
                                                            },
                                                            true,
                                                            cx.listener(move |this, _, _, cx| {
                                                                if this.settings.text_script == script.id() {
                                                                    return;
                                                                }
                                                                this.settings.text_script =
                                                                    script.id().into();
                                                                this.mark_settings_dirty(cx);
                                                            }),
                                                        )
                                                    },
                                                )),
                                            ),
                                    )
                                    .into_any_element();
    settings_section(body)
}

/// 字幕输出位置
pub(super) fn render_settings_output_location(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .text_color(TEXT)
                                            .child("字幕输出位置"),
                                    )
                                    .child(
                                        div().flex().gap_1p5().children(
                                            [(true, "视频同目录"), (false, "指定目录")]
                                                .into_iter()
                                                .map(|(next, label)| {
                                                    let active = form.save_next == next;
                                                    btn(
                                                        label,
                                                        if active {
                                                            BtnKind::Primary
                                                        } else {
                                                            BtnKind::Secondary
                                                        },
                                                        true,
                                                        cx.listener(move |this, _, _, cx| {
                                                            if this.settings.save_next_to_source == next {
                                                                return;
                                                            }
                                                            this.settings.save_next_to_source = next;
                                                            this.mark_settings_dirty(cx);
                                                        }),
                                                    )
                                                }),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .id("output-dir")
                                            .flex()
                                            .items_center()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(LINE)
                                            .bg(PANEL)
                                            .overflow_hidden()
                                            .opacity(if form.save_next { 0.45 } else { 1.0 })
                                            .hover(|s| s.border_color(ACCENT))
                                            .child(
                                                div()
                                                    .id("output-dir-path")
                                                    .flex_1()
                                                    .min_w_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .text_xs()
                                                    .text_color(TEXT)
                                                    .whitespace_normal()
                                                    .line_clamp(2)
                                                    .child(form.output_dir.clone())
                                                    .tooltip({
                                                        let tip = form.output_dir_tip.clone();
                                                        move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: tip.clone().into(),
                                                        })
                                                        .into()
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id("output-dir-browse")
                                                    .flex_shrink_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .border_l_1()
                                                    .border_color(LINE_SOFT)
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(ACCENT_SOFT))
                                                    .child(
                                                        svg()
                                                            .size(px(15.))
                                                            .path("icons/folder.svg")
                                                            .text_color(MUTED),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.pick_output_dir(cx)
                                                    })),
                                            ),
                                    )
                                    .into_any_element();
    settings_section(body)
}

/// 语音识别模型
pub(super) fn render_settings_asr_model(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .child("语音识别模型"),
                                            )
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(if form.asr_ready { ACCENT } else { DANGER }),
                                            ),
                                    )
                                    .child(
                                        div().flex().gap_1p5().children(
                                            ModelId::ASR_CHOICES.into_iter().map(|id| {
                                                let active = form.asr_id == id;
                                                let can_switch = !form.asr_size_locked || active;
                                                btn(
                                                    id.short_label(),
                                                    if active {
                                                        BtnKind::Primary
                                                    } else {
                                                        BtnKind::Secondary
                                                    },
                                                    can_switch,
                                                    cx.listener(move |this, _, _, cx| {
                                                        if this.settings.selected_asr_id() == id {
                                                            return;
                                                        }
                                                        if this.download_kind_busy(ModelKind::Asr) {
                                                            this.flash_hint(
                                                                "ASR 下载进行中，请稍后再切换尺寸",
                                                                cx,
                                                            );
                                                            return;
                                                        }
                                                        this.settings.select_asr_model(id);
                                                        this.clear_stale_asr_progress();
                                                        this.refresh_model_probe();
                                                        this.mark_settings_dirty(cx);
                                                    }),
                                                )
                                            }),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .id("model-dir")
                                            .flex()
                                            .items_center()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(LINE)
                                            .bg(PANEL)
                                            .overflow_hidden()
                                            .hover(|s| s.border_color(ACCENT))
                                            .child(
                                                div()
                                                    .id("model-dir-path")
                                                    .flex_1()
                                                    .min_w_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .text_xs()
                                                    .text_color(TEXT)
                                                    .whitespace_normal()
                                                    .line_clamp(2)
                                                    .child(form.model.clone())
                                                    .tooltip({
                                                        let tip = form.model_tip.clone();
                                                        move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: tip.clone().into(),
                                                        })
                                                        .into()
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id("model-dir-browse")
                                                    .flex_shrink_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .border_l_1()
                                                    .border_color(LINE_SOFT)
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(ACCENT_SOFT))
                                                    .child(
                                                        svg()
                                                            .size(px(15.))
                                                            .path("icons/folder.svg")
                                                            .text_color(MUTED),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.pick_model_dir(cx)
                                                    })),
                                            ),
                                    )
                                    .child(model_download_row(
                                        ComponentRow {
                                            id: "asr-dl-btn",
                                            cancel_id: "asr-dl-cancel",
                                            ready: form.asr_ready,
                                            busy: form.asr_dl_busy,
                                            progress: form.asr_dl.as_ref(),
                                        },
                                        ModelKind::Asr,
                                        cx.listener(move |this, _, _, cx| {
                                            let id = this.settings.selected_asr_id();
                                            this.start_model_download(id, cx);
                                        }),
                                        cx.listener(move |this, _, _, cx| {
                                            let id = this.settings.selected_asr_id();
                                            this.cancel_model_download(id, cx);
                                        }),
                                    ))
                                    .into_any_element();
    settings_section(body)
}

/// 对齐模型
pub(super) fn render_settings_aligner(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .child("对齐模型"),
                                            )
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(if form.align_ready { ACCENT } else { DANGER }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .id("aligner-dir")
                                            .flex()
                                            .items_center()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(LINE)
                                            .bg(PANEL)
                                            .overflow_hidden()
                                            .hover(|s| s.border_color(ACCENT))
                                            .child(
                                                div()
                                                    .id("aligner-dir-path")
                                                    .flex_1()
                                                    .min_w_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .text_xs()
                                                    .text_color(TEXT)
                                                    .whitespace_normal()
                                                    .line_clamp(2)
                                                    .child(form.aligner.clone())
                                                    .tooltip({
                                                        let tip = form.aligner_tip.clone();
                                                        move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: tip.clone().into(),
                                                        })
                                                        .into()
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id("aligner-dir-browse")
                                                    .flex_shrink_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .border_l_1()
                                                    .border_color(LINE_SOFT)
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(ACCENT_SOFT))
                                                    .child(
                                                        svg()
                                                            .size(px(15.))
                                                            .path("icons/folder.svg")
                                                            .text_color(MUTED),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.pick_aligner_dir(cx)
                                                    })),
                                            ),
                                    )
                                    .child(model_download_row(
                                        ComponentRow {
                                            id: "align-dl-btn",
                                            cancel_id: "align-dl-cancel",
                                            ready: form.align_ready,
                                            busy: form.align_dl_busy,
                                            progress: form.align_dl.as_ref(),
                                        },
                                        ModelKind::Align,
                                        cx.listener(|this, _, _, cx| {
                                            this.start_model_download(ModelId::QwenAlign06B, cx);
                                        }),
                                        cx.listener(|this, _, _, cx| {
                                            this.cancel_model_download(ModelId::QwenAlign06B, cx);
                                        }),
                                    ))
                                    .into_any_element();
    settings_section(body)
}

/// 人声分离（HTDemucs）
pub(super) fn render_settings_demucs(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .child("人声分离"),
                                            )
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(if form.demucs_ready { ACCENT } else { DANGER }),
                                            ),
                                    )
                                    // 开关 = 新任务默认值（任务行里可单独覆盖）
                                    .child(
                                        div().flex().gap_1p5().children(
                                            [(false, "关闭"), (true, "开启")]
                                                .into_iter()
                                                .map(|(on, label)| {
                                                    let active = form.vocal_sep == on;
                                                    btn(
                                                        label,
                                                        if active {
                                                            BtnKind::Primary
                                                        } else {
                                                            BtnKind::Secondary
                                                        },
                                                        true,
                                                        cx.listener(move |this, _, _, cx| {
                                                            if this.settings.vocal_separation == on {
                                                                return;
                                                            }
                                                            if on && !this.demucs_ready {
                                                                this.flash_hint(
                                                                    "请先下载人声分离模型",
                                                                    cx,
                                                                );
                                                                return;
                                                            }
                                                            this.settings.vocal_separation = on;
                                                            this.mark_settings_dirty(cx);
                                                        }),
                                                    )
                                                }),
                                        ),
                                    )
                                    .child(
                                        div()
                                            .id("demucs-dir")
                                            .flex()
                                            .items_center()
                                            .rounded_md()
                                            .border_1()
                                            .border_color(LINE)
                                            .bg(PANEL)
                                            .overflow_hidden()
                                            .hover(|s| s.border_color(ACCENT))
                                            .child(
                                                div()
                                                    .id("demucs-dir-path")
                                                    .flex_1()
                                                    .min_w_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .text_xs()
                                                    .text_color(TEXT)
                                                    .whitespace_normal()
                                                    .line_clamp(2)
                                                    .child(form.demucs_dir.clone())
                                                    .tooltip({
                                                        let tip = form.demucs_dir_tip.clone();
                                                        move |_, cx| {
                                                        cx.new(|_| NameTooltip {
                                                            text: tip.clone().into(),
                                                        })
                                                        .into()
                                                        }
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .id("demucs-dir-browse")
                                                    .flex_shrink_0()
                                                    .px_2p5()
                                                    .py_1p5()
                                                    .border_l_1()
                                                    .border_color(LINE_SOFT)
                                                    .cursor_pointer()
                                                    .hover(|s| s.bg(ACCENT_SOFT))
                                                    .child(
                                                        svg()
                                                            .size(px(15.))
                                                            .path("icons/folder.svg")
                                                            .text_color(MUTED),
                                                    )
                                                    .on_click(cx.listener(|this, _, _, cx| {
                                                        this.pick_demucs_dir(cx)
                                                    })),
                                            ),
                                    )
                                    .child(model_download_row(
                                        ComponentRow {
                                            id: "demucs-dl-btn",
                                            cancel_id: "demucs-dl-cancel",
                                            ready: form.demucs_ready,
                                            busy: form.demucs_dl_busy,
                                            progress: form.demucs_dl.as_ref(),
                                        },
                                        ModelKind::Demucs,
                                        cx.listener(|this, _, _, cx| {
                                            this.start_model_download(ModelId::HtdemucsFt, cx);
                                        }),
                                        cx.listener(|this, _, _, cx| {
                                            this.cancel_model_download(ModelId::HtdemucsFt, cx);
                                        }),
                                    ))
                                    .into_any_element();
    settings_section(body)
}

/// 推理后端（auto / cuda）
pub(super) fn render_settings_backend(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .text_sm()
                                            .font_weight(gpui::FontWeight::MEDIUM)
                                            .child("推理后端"),
                                    )
                                    .child(
                                        div().flex().gap_1p5().children(
                                            [
                                                ("auto", "自动"),
                                                ("cuda", "GPU"),
                                                ("cpu", "CPU"),
                                            ]
                                            .into_iter()
                                            .map(|(id, label)| {
                                                let active = form.backend == id;
                                                btn(
                                                    label,
                                                    if active {
                                                        BtnKind::Primary
                                                    } else {
                                                        BtnKind::Secondary
                                                    },
                                                    true,
                                                    cx.listener(move |this, _, _, cx| {
                                                        if this.settings.backend == id {
                                                            return;
                                                        }
                                                        if id == "cuda" && !this.cuda_ready {
                                                            this.flash_hint(
                                                                "请先下载 CUDA 运行库，再选择 GPU",
                                                                cx,
                                                            );
                                                            return;
                                                        }
                                                        this.settings.backend = id.into();
                                                        this.refresh_model_probe();
                                                        this.mark_settings_dirty(cx);
                                                    }),
                                                )
                                            }),
                                        ),
                                    )
                                    .into_any_element();
    settings_section(body)
}

/// CUDA 运行库
pub(super) fn render_settings_cuda(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .child("CUDA 运行库"),
                                            )
                                            .child(
                                                div()
                                                    .size(px(8.))
                                                    .rounded_full()
                                                    .bg(if form.cuda_ready { ACCENT } else { DANGER }),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(MUTED)
                                            .child("需要 N 卡（NVIDIA 显卡），显存 4GB 起"),
                                    )
                                    .child(component_install_row(
                                        ComponentRow {
                                            id: "cuda-dl-btn",
                                            cancel_id: "cuda-dl-cancel",
                                            ready: form.cuda_ready,
                                            busy: form.cuda_dl_busy,
                                            progress: form.cuda_dl.as_ref(),
                                        },
                                        cx.listener(|this, _, _, cx| {
                                            this.start_model_download(ModelId::CudaRuntime, cx);
                                        }),
                                        cx.listener(|this, _, _, cx| {
                                            this.cancel_model_download(ModelId::CudaRuntime, cx);
                                        }),
                                    ))
                                    .into_any_element();
    settings_section(body)
}

/// 提示音与运行提醒
pub(super) fn render_settings_sound(
    &mut self,
    form: &SettingsFormState,
    cx: &mut Context<Self>,
) -> impl IntoElement + use<> {
    let body =
        div()
                                    .flex()
                                    .flex_col()
                                    .gap_1p5()
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .justify_between()
                                            .child(
                                                div()
                                                    .text_sm()
                                                    .font_weight(gpui::FontWeight::MEDIUM)
                                                    .text_color(TEXT)
                                                    .child("提示音"),
                                            )
                                            .child(
                                                div().flex().gap_1p5().children(
                                                    [(false, "关闭"), (true, "开启")]
                                                        .into_iter()
                                                        .map(|(on, label)| {
                                                            let active = form.sound == on;
                                                            pill(
                                                                if on {
                                                                    "sound-on"
                                                                } else {
                                                                    "sound-off"
                                                                },
                                                                label,
                                                                active,
                                                                cx.listener(move |this, _, _, cx| {
                                                                    if this.settings.sound == on {
                                                                        return;
                                                                    }
                                                                    this.settings.sound = on;
                                                                    // Audible the moment it
                                                                    // comes back on.
                                                                    this.play_ui(sfx::Sfx::Click);
                                                                    this.mark_settings_dirty(cx);
                                                                }),
                                                            )
                                                        }),
                                                ),
                                            ),
                                    )
                                    .into_any_element();
    settings_section(body)
}
}
