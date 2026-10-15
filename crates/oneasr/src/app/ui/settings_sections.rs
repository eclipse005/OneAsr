//! The eleven sections of the settings drawer.
//!
//! Each builder renders one `section(..)` card. They take [`SettingsFormState`]
//! by reference so the drawer's snapshot is read, never re-derived, and return
//! `impl IntoElement + use<>` — precise capturing is required because the
//! drawer chains all ten into one element, and the default (edition 2024)
//! capture set would make each one conservatively borrow `self` and `cx`.

use crate::app::prelude::*;
use crate::app::ui::settings_drawer::SettingsFormState;

/// 设置标题。悬停用现成的深色小卡片，文案按当前界面语言取。
pub(super) fn setting_title(id: &'static str, label: &str, tip: &'static str) -> impl IntoElement {
    let tip: SharedString = tip.into();
    div()
        .id(id)
        .text_sm()
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(TEXT)
        .whitespace_nowrap()
        .child(label.to_string())
        .tooltip(move |_, cx| cx.new(|_| NameTooltip { text: tip.clone() }).into())
}

/// One settings card: rounded, bordered, padded. Every section goes through
/// this so their chrome cannot drift apart. Gray cards on the white drawer. White on the gray page background.
///
/// 内边距比「刚好装下」再松一档：一屏塞十张各自贴着边的卡会读成一堵墙。卡与卡之间
/// 由抽屉的 `gap_3` 分开，卡内则给内容呼吸。
pub(super) fn settings_section(body: AnyElement) -> impl IntoElement + use<> {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .rounded_lg()
        .border_1()
        .border_color(LINE_SOFT)
        .bg(BG)
        .px_4()
        .py_3()
        .child(body)
}

/// 「对齐模型」标题右侧状态点的三态。
///
/// 纯数据，不含颜色——渲染处把它映射到 theme 令牌，好让三态能在单测里
/// 被断言，而不必去比 Rgba。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlignerDot {
    /// 需要对齐器，且所选模型已就绪（绿）。
    Ready,
    /// 需要对齐器，但所选模型还没下好（红）。
    Missing,
    /// 当前输出格式用不到对齐器（灰）。
    NotNeeded,
}

/// 当前暂存的输出格式是否需要对齐器：SRT / ASS 带时间轴，得靠打轴才写得
/// 出来；TXT 只是纯文本，没有起止时间可标。
///
/// 文稿匹配任务天然要对齐器，但它被闸门要求至少勾一个 SRT / ASS，所以
/// 「按输出格式判断」与「按任务判断」在实际可跑的组合上是一致的。
pub(crate) fn aligner_needed(output_srt: bool, output_ass: bool) -> bool {
    output_srt || output_ass
}

/// 标题右侧那颗点的三态：需要 → 就绪绿 / 未就绪红，不需要 → 灰。
pub(crate) fn aligner_dot_state(output_srt: bool, output_ass: bool, ready: bool) -> AlignerDot {
    if !aligner_needed(output_srt, output_ass) {
        AlignerDot::NotNeeded
    } else if ready {
        AlignerDot::Ready
    } else {
        AlignerDot::Missing
    }
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
            // 语言 id 非法（settings.json 被手改坏）时没有对应的 SourceLanguage，
            // 兜底标签必须走双语表——写成固定中文会把中文塞进英文界面。
            let cur_label = source_language_by_id(&form.language)
                .map(|l| l.label)
                .unwrap_or_else(|| t(L::LANGUAGE_FALLBACK));

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
                        .child(setting_title(
                            "tip-default-language",
                            t(L::DEFAULT_LANGUAGE),
                            t(L::TIP_DEFAULT_LANGUAGE),
                        ))
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
                                        .border_color(if open { ACCENT } else { LINE })
                                        .bg(if open { ACCENT_SOFT } else { BG })
                                        .cursor_pointer()
                                        .hover(|s| s.bg(ACCENT_SOFT).border_color(ACCENT))
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
                                                        .child(if open { "▴" } else { "▾" }),
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
                        .child(setting_title(
                            "tip-subtitle-length",
                            t(L::SUBTITLE_LENGTH),
                            t(L::TIP_SUBTITLE_LENGTH),
                        ))
                        .child(
                            // flex_wrap：英文档位名更宽，半栏放不下时换行而不是
                            // 被抽屉边缘裁掉。
                            div().flex().flex_wrap().gap_1p5().children(
                                [
                                    ("short", t(L::LEN_SHORT)),
                                    ("standard", t(L::LEN_STANDARD)),
                                    ("loose", t(L::LEN_LOOSE)),
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
                                            if this.settings.subtitle_length_preset == id {
                                                return;
                                            }
                                            this.settings.subtitle_length_preset = id.into();
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
        let body =
            {
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
                            .child(setting_title(
                                "tip-chunk-duration",
                                t(L::CHUNK_DURATION),
                                t(L::TIP_CHUNK_DURATION),
                            ))
                            .child(div().text_xs().text_color(MUTED).child(
                                crate::i18n::chunk_target_label(
                                    form.chunk_target,
                                    CHUNK_TARGET_MIN_SEC,
                                    CHUNK_TARGET_MAX_SEC,
                                ),
                            )),
                    )
                    .child(div().flex().gap_1p5().children(
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
                    ))
                    .child(
                        div().flex().flex_col().gap_0p5().child(
                            div()
                                .text_xs()
                                .text_color(MUTED)
                                .child(t(L::CHUNK_4GB_HINT)),
                        ),
                    )
                    .into_any_element()
            };
        settings_section(body)
    }

    /// 输出格式、中文输出，以及同一块里的字幕美化。
    pub(super) fn render_settings_output(
        &mut self,
        form: &SettingsFormState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let columns = div()
            .flex()
            .items_start()
            .gap_2p5()
            .child(
                div()
                    // 两列平分宽度（同「默认语言 | 字幕长度」。
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(div().flex().items_center().child(setting_title(
                        "tip-output-format",
                        t(L::OUTPUT_FORMAT),
                        t(L::TIP_OUTPUT_FORMAT),
                    )))
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
                                    if !this.settings.output_txt && !this.settings.output_ass {
                                        this.flash_hint(t(L::KEEP_ONE_FORMAT), cx);
                                        return;
                                    }
                                    this.settings.output_srt = !this.settings.output_srt;
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
                                    if !this.settings.output_srt && !this.settings.output_ass {
                                        this.flash_hint(t(L::KEEP_ONE_FORMAT), cx);
                                        return;
                                    }
                                    this.settings.output_txt = !this.settings.output_txt;
                                    this.mark_settings_dirty(cx);
                                }),
                            ))
                            .child(btn(
                                "ASS",
                                if form.output_ass {
                                    BtnKind::Primary
                                } else {
                                    BtnKind::Secondary
                                },
                                true,
                                cx.listener(|this, _, _, cx| {
                                    if !this.settings.output_srt && !this.settings.output_txt {
                                        this.flash_hint(t(L::KEEP_ONE_FORMAT), cx);
                                        return;
                                    }
                                    this.settings.output_ass = !this.settings.output_ass;
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
                    // 侧注（zh_yue_only）删掉：英文 "Chinese & Cantonese only"
                    // 比整栏还宽，把标题挤出抽屉；这层意思悬停 tooltip 里已有。
                    .child(setting_title(
                        "tip-chinese-output",
                        t(L::CHINESE_OUTPUT),
                        t(L::TIP_CHINESE_OUTPUT),
                    ))
                    .child(
                        // flex_wrap：英文 Original / Simplified / Traditional 比中文宽。
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1()
                            .children(TextScript::ALL.map(|script| {
                                let active = form.text_script == script;
                                btn(
                                    script.label(ui_lang()),
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
                                        this.settings.text_script = script.id().into();
                                        this.mark_settings_dirty(cx);
                                    }),
                                )
                            })),
                    ),
            );
        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(columns)
            // 卡片左右有 px_3。负边距把线拉到边框内侧，颜色比边框深一档、比文字浅。
            .child(div().h(px(1.)).mx(gpui::rems(-0.75)).bg(DIVIDER))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-subtitle-beautify",
                        t(L::SUBTITLE_BEAUTIFY),
                        t(L::TIP_SUBTITLE_BEAUTIFY),
                    ))
                    .child(switch(
                        "switch-subtitle-beautify",
                        form.subtitle_beautify,
                        cx.listener(|this, _, _, cx| {
                            this.settings.subtitle_beautify = !this.settings.subtitle_beautify;
                            this.mark_settings_dirty(cx);
                        }),
                    )),
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
        let body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .text_sm()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(TEXT)
                    .child(setting_title(
                        "tip-output-location",
                        t(L::OUTPUT_LOCATION),
                        t(L::TIP_OUTPUT_LOCATION),
                    )),
            )
            .child(
                div().flex().gap_1p5().children(
                    [(true, t(L::NEXT_TO_VIDEO)), (false, t(L::CUSTOM_DIR))]
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
            .child(model_path_bar(
                "output-dir",
                form.output_dir.clone().into(),
                form.output_dir_tip.clone().into(),
                2,
                form.save_next,
                cx.listener(|this, _, _, cx| this.flash_path_copied(cx)),
                cx.listener(|this, _, _, cx| this.pick_output_dir(cx)),
            ))
            .into_any_element();
        settings_section(body)
    }

    /// 语音识别模型
    pub(super) fn render_settings_asr_model(
        &mut self,
        form: &SettingsFormState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-asr-model",
                        t(L::ASR_MODEL),
                        t(L::TIP_ASR_MODEL),
                    ))
                    .child(div().size(px(8.)).rounded_full().bg(if form.asr_ready {
                        ACCENT
                    } else {
                        DANGER
                    })),
            )
            .child({
                // 尺寸 chip 选"档位家族"：量化开着时切尺寸
                // 仍落在该尺寸的 int8 档；「量化」是同一行里
                // 可独立点亮/取消的按钮（与 SRT/TXT 同款）。
                let mut chips: Vec<AnyElement> = ModelId::ASR_CHOICES
                    .into_iter()
                    .map(|id| {
                        let active = form.asr_base_id == id;
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
                                let target = id.with_quant(this.settings.asr_quantized());
                                if this.settings.selected_asr_id() == target {
                                    return;
                                }
                                if this.download_kind_busy(ModelKind::Asr) {
                                    this.flash_hint(t(L::ASR_DL_BUSY_RESIZE), cx);
                                    return;
                                }
                                this.settings.select_asr_model(target);
                                this.clear_stale_asr_progress();
                                this.refresh_model_probe();
                                this.mark_settings_dirty(cx);
                            }),
                        )
                        .into_any_element()
                    })
                    .collect();
                chips.push(
                    pill(
                        "asr-quant",
                        t(L::QUANT),
                        form.asr_quant,
                        Some(t(L::TIP_QUANT)),
                        cx.listener(|this, _, _, cx| {
                            if this.download_kind_busy(ModelKind::Asr) {
                                this.flash_hint(t(L::ASR_DL_BUSY), cx);
                                return;
                            }
                            this.settings
                                .set_asr_quantized(!this.settings.asr_quantized());
                            this.clear_stale_asr_progress();
                            this.refresh_model_probe();
                            this.mark_settings_dirty(cx);
                        }),
                    )
                    .into_any_element(),
                );
                div().flex().gap_1p5().children(chips)
            })
            .child(model_download_row(
                ComponentRow {
                    id: "asr-dl-btn",
                    ready: form.asr_ready,
                    busy: form.asr_dl_busy,
                    progress: form.asr_dl.as_ref(),
                },
                model_path_bar(
                    "model-dir",
                    form.model.clone().into(),
                    form.model_tip.clone().into(),
                    1,
                    false,
                    cx.listener(|this, _, _, cx| this.flash_path_copied(cx)),
                    cx.listener(|this, _, _, cx| this.pick_model_dir(cx)),
                ),
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
        let body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-aligner-model",
                        t(L::ALIGNER_MODEL),
                        t(L::TIP_ALIGNER_MODEL),
                    ))
                    .child(div().size(px(8.)).rounded_full().bg(
                        match aligner_dot_state(form.output_srt, form.output_ass, form.align_ready)
                        {
                            AlignerDot::Ready => ACCENT,
                            AlignerDot::Missing => DANGER,
                            AlignerDot::NotNeeded => MUTED,
                        },
                    )),
            )
            .child({
                // 对齐引擎选择：CTC 在前、默认 CTC（与 ASR 尺寸 chip 同款交互）。
                let chips: Vec<AnyElement> = ModelId::ALIGNER_CHOICES
                    .into_iter()
                    .map(|id| {
                        let active = form.align_base_id == id;
                        let can_switch = !form.align_locked || active;
                        btn(
                            id.short_label(),
                            if active {
                                BtnKind::Primary
                            } else {
                                BtnKind::Secondary
                            },
                            can_switch,
                            cx.listener(move |this, _, _, cx| {
                                if this.settings.selected_aligner_id() == id {
                                    return;
                                }
                                if this.download_kind_busy(ModelKind::Align) {
                                    this.flash_hint(t(L::ALIGN_DL_BUSY), cx);
                                    return;
                                }
                                this.settings.select_aligner_model(id);
                                this.refresh_model_probe();
                                this.mark_settings_dirty(cx);
                            }),
                        )
                        .into_any_element()
                    })
                    .collect();
                div().flex().gap_1p5().children(chips)
            })
            .child(model_download_row(
                ComponentRow {
                    id: "align-dl-btn",
                    ready: form.align_ready,
                    busy: form.align_dl_busy,
                    progress: form.align_dl.as_ref(),
                },
                model_path_bar(
                    "aligner-dir",
                    form.aligner.clone().into(),
                    form.aligner_tip.clone().into(),
                    1,
                    false,
                    cx.listener(|this, _, _, cx| this.flash_path_copied(cx)),
                    cx.listener(|this, _, _, cx| this.pick_aligner_dir(cx)),
                ),
                cx.listener(|this, _, _, cx| {
                    this.start_model_download(this.settings.selected_aligner_id(), cx);
                }),
                cx.listener(|this, _, _, cx| {
                    this.cancel_model_download(this.settings.selected_aligner_id(), cx);
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
        let body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-vocal-separation",
                        t(L::VOCAL_SEPARATION),
                        t(L::TIP_VOCAL_SEPARATION),
                    ))
                    .child(div().size(px(8.)).rounded_full().bg(if form.demucs_ready {
                        ACCENT
                    } else {
                        DANGER
                    })),
            )
            // 「默认启用」= 新任务默认值（任务行里可单独覆盖）；
            // 与「量化」同款：点亮即选中，再点取消。
            .child(div().flex().gap_1p5().child(pill(
                "vocal-sep-default",
                t(L::SEP_DEFAULT_ON),
                form.vocal_sep,
                Some(t(L::TIP_SEP_DEFAULT_ON)),
                cx.listener(|this, _, _, cx| {
                    let enabling = !this.settings.vocal_separation;
                    if enabling && !this.demucs_ready {
                        this.flash_hint(t(L::SEP_NEEDS_MODEL), cx);
                        return;
                    }
                    this.settings.vocal_separation = enabling;
                    this.mark_settings_dirty(cx);
                }),
            )))
            .child(model_download_row(
                ComponentRow {
                    id: "demucs-dl-btn",
                    ready: form.demucs_ready,
                    busy: form.demucs_dl_busy,
                    progress: form.demucs_dl.as_ref(),
                },
                model_path_bar(
                    "demucs-dir",
                    form.demucs_dir.clone().into(),
                    form.demucs_dir_tip.clone().into(),
                    1,
                    false,
                    cx.listener(|this, _, _, cx| this.flash_path_copied(cx)),
                    cx.listener(|this, _, _, cx| this.pick_demucs_dir(cx)),
                ),
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

    /// 推理后端（auto / gpu / cpu）
    pub(super) fn render_settings_backend(
        &mut self,
        form: &SettingsFormState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(setting_title(
                "tip-backend",
                t(L::BACKEND),
                t(L::TIP_BACKEND),
            ))
            .child(
                div().flex().gap_1p5().children(
                    [("auto", t(L::BACKEND_AUTO)), ("gpu", "GPU"), ("cpu", "CPU")]
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

    /// 提示音 + 关窗到托盘：一张卡两行开关，同为「一行一开关」的最简语义。
    /// 关窗开关：开 = 点红叉收进托盘（任务继续），关 = 直接退出。
    pub(super) fn render_settings_sound(
        &mut self,
        form: &SettingsFormState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title("tip-sound", t(L::SOUND), t(L::TIP_SOUND)))
                    .child(switch(
                        "switch-sound",
                        form.sound,
                        cx.listener(|this, _, _, cx| {
                            let enabling = !this.settings.sound;
                            this.settings.sound = enabling;
                            // Audible the moment it comes back on.
                            if enabling {
                                this.play_ui(sfx::Sfx::Click);
                            }
                            this.mark_settings_dirty(cx);
                        }),
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-close-to-tray",
                        t(L::CLOSE_TO_TRAY),
                        t(L::TIP_CLOSE_TO_TRAY),
                    ))
                    .child(switch(
                        "switch-close-to-tray",
                        self.settings.close_to_tray,
                        cx.listener(|this, _, _, cx| {
                            this.settings.close_to_tray = !this.settings.close_to_tray;
                            this.mark_settings_dirty(cx);
                        }),
                    )),
            );
        settings_section(body.into_any_element())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subtitle_outputs_need_the_aligner_but_plain_text_does_not() {
        assert!(aligner_needed(true, false), "SRT carries timestamps");
        assert!(aligner_needed(false, true), "ASS carries timestamps");
        assert!(aligner_needed(true, true));
        assert!(
            !aligner_needed(false, false),
            "TXT only: nothing to align, gray dot regardless of the model"
        );
    }

    #[test]
    fn a_required_aligner_reports_green_or_red() {
        assert_eq!(aligner_dot_state(true, false, true), AlignerDot::Ready);
        assert_eq!(aligner_dot_state(false, true, true), AlignerDot::Ready);
        assert_eq!(aligner_dot_state(true, true, false), AlignerDot::Missing);
        assert_eq!(aligner_dot_state(false, true, false), AlignerDot::Missing);
    }

    #[test]
    fn txt_only_stays_gray_even_when_the_model_is_missing() {
        assert_eq!(
            aligner_dot_state(false, false, false),
            AlignerDot::NotNeeded
        );
        assert_eq!(aligner_dot_state(false, false, true), AlignerDot::NotNeeded);
    }
}
