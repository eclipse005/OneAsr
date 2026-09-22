//! The settings drawer's skeleton — slide-in shell, scroll host, save bar.
//!
//! The ten sections themselves live in [`super::settings_sections`], which
//! reads a [`SettingsFormState`] so the probes and clones happen once.

use crate::app::prelude::*;

impl OneAsrApp {
    pub(super) fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let form = self.settings_form_state(cx);

        // Layout: title | scrollable body | pin footer (save always visible).
        div()
            .w(px(SETTINGS_W))
            .h_full()
            .bg(PANEL)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_base()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(TEXT)
                            .child("设置"),
                    )
                    .when(form.dirty, |el| {
                        el.child(
                            div()
                                .text_xs()
                                .text_color(WARN)
                                .child("未保存"),
                        )
                    }),
            )
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .px_4()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .gap_2p5()
                    .child(self.render_settings_language(&form, cx))
                    .child(self.render_settings_chunk_duration(&form, cx))
                    // 输出：格式 | 中文字形 并排平分（同「默认语言 | 字幕长度」）
                    .child(self.render_settings_output(&form, cx))
                    .child(self.render_settings_output_location(&form, cx))
                    .child(self.render_settings_asr_model(&form, cx))
                    .child(self.render_settings_aligner(&form, cx))
                    // 人声分离（可选）：HTDemucs 原生 Rust 推理，转录前压掉 BGM
                    .child(self.render_settings_demucs(&form, cx))
                    .child(self.render_settings_backend(&form, cx))
                    // One switch, no essay: taps and the run reminder together.
                    .child(self.render_settings_sound(&form, cx)),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .px_4()
                    .py_3()
                    .border_t_1()
                    .border_color(LINE_SOFT)
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div().flex_shrink_0().child(btn(
                            "重置",
                            BtnKind::Quiet,
                            true,
                            cx.listener(|this, _, _, cx| this.reset_settings(cx)),
                        )),
                    )
                    .child(
                        div().flex_shrink_0().child(btn(
                            if form.dirty { "保存设置" } else { "已保存" },
                            if form.dirty {
                                BtnKind::Primary
                            } else {
                                BtnKind::Secondary
                            },
                            form.dirty,
                            cx.listener(|this, _, _, cx| this.save_settings(cx)),
                        )),
                    ),
            )
    }
}

/// Everything the settings drawer derives once per repaint.
///
/// Collected up front (as the monolith did) and shared by reference with the
/// ten section builders, so probe / download state is read exactly once.
pub(super) struct SettingsFormState {
    pub(super) backend: String,
    pub(super) language: String,
    pub(super) length_preset: String,
    pub(super) chunk_target: u32,
    pub(super) asr_id: ModelId,
    pub(super) model: String,
    pub(super) model_tip: String,
    pub(super) aligner: String,
    pub(super) aligner_tip: String,
    pub(super) output_dir: String,
    pub(super) output_dir_tip: String,
    pub(super) save_next: bool,
    pub(super) output_srt: bool,
    pub(super) output_txt: bool,
    pub(super) text_script: TextScript,
    pub(super) vocal_sep: bool,
    pub(super) sound: bool,
    pub(super) demucs_ready: bool,
    pub(super) demucs_dl: Option<DownloadProgress>,
    pub(super) demucs_dl_busy: bool,
    pub(super) demucs_dir: String,
    pub(super) demucs_dir_tip: String,
    pub(super) dirty: bool,
    pub(super) asr_ready: bool,
    pub(super) align_ready: bool,
    pub(super) asr_dl: Option<DownloadProgress>,
    pub(super) align_dl: Option<DownloadProgress>,
    pub(super) asr_dl_busy: bool,
    pub(super) asr_size_locked: bool,
    pub(super) align_dl_busy: bool,
}

impl OneAsrApp {
    /// Snapshot the settings + probe / download state for one repaint.
    pub(super) fn settings_form_state(&self, cx: &Context<Self>) -> SettingsFormState {
        let backend = self.settings.backend.clone();
        let language = self.settings.language.clone();
        let length_preset = self.settings.subtitle_length_preset.clone();
        let chunk_target = self.settings.chunk_target_seconds_clamped();
        let asr_id = self.settings.selected_asr_id();
        let model = self.settings.asr_model_dir.display().to_string();
        let model_tip = model.clone();
        let aligner = self.settings.aligner_model_dir.display().to_string();
        let aligner_tip = aligner.clone();
        let output_dir = self.settings.resolved_output_dir().display().to_string();
        let output_dir_tip = output_dir.clone();
        let save_next = self.settings.save_next_to_source;
        let output_srt = self.settings.output_srt;
        let output_txt = self.settings.output_txt;
        let text_script = self.settings.text_script_choice();
        let vocal_sep = self.settings.vocal_separation;
        let sound = self.settings.sound;
        let demucs_ready = self.demucs_ready;
        let demucs_dl = self.progress_for(ModelId::HtdemucsFt).cloned();
        let demucs_dl_busy = self.download_busy(ModelId::HtdemucsFt);
        let demucs_dir = self
            .settings
            .resolved_demucs_model_dir()
            .display()
            .to_string();
        let demucs_dir_tip = demucs_dir.clone();
        let dirty = self.is_settings_dirty(cx);
        // Use probe cache — never re-stat model dirs on every scroll paint.
        let asr_ready = self.asr_ready;
        let align_ready = self.align_ready;
        // Progress is keyed by model id — never show another size’s snapshot here.
        let asr_dl = self.progress_for(asr_id).cloned();
        let align_dl = self.progress_for(ModelId::QwenAlign06B).cloned();
        let asr_dl_busy = self.download_busy(asr_id);
        let asr_size_locked = self.download_kind_busy(ModelKind::Asr);
        let align_dl_busy = self.download_busy(ModelId::QwenAlign06B);
        SettingsFormState {
            backend,
            language,
            length_preset,
            chunk_target,
            asr_id,
            model,
            model_tip,
            aligner,
            aligner_tip,
            output_dir,
            output_dir_tip,
            save_next,
            output_srt,
            output_txt,
            text_script,
            vocal_sep,
            sound,
            demucs_ready,
            demucs_dl,
            demucs_dl_busy,
            demucs_dir,
            demucs_dir_tip,
            dirty,
            asr_ready,
            align_ready,
            asr_dl,
            align_dl,
            asr_dl_busy,
            asr_size_locked,
            align_dl_busy,
        }
    }
}
