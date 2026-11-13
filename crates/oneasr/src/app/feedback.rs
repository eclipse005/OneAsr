//! 「反馈问题」的动作：拼一条预填好的 issue，把完整日志放进剪贴板，再打开网页。
//!
//! 只做两件事：写剪贴板、`open_url`。不联网、不带凭证 —— 项目本身没有后端也
//! 没有遥测，用户提交之前能看到（也能改）即将发出去的每一个字。

use crate::app::prelude::*;

impl OneAsrApp {
    /// 先写剪贴板再开网页：浏览器抢焦点之后用户回来按 Ctrl+V，内容已经在里面了。
    pub(super) fn open_feedback(&mut self, cx: &mut Context<Self>) {
        let report = crate::sysinfo::system_report();
        let mut lines = report.lines();
        let os = lines
            .next()
            .unwrap_or("")
            .trim_start_matches("os: ")
            .to_string();
        let cpu = lines
            .next()
            .unwrap_or("")
            .trim_start_matches("cpu: ")
            .to_string();
        let lang = oneasr_core::i18n::ui_lang();
        let log = crate::feedback::latest_task_log(lang, &oneasr_core::paths::data_dir());
        let facts = crate::feedback::Facts {
            version: env!("CARGO_PKG_VERSION"),
            backend: &self.settings.backend,
            vulkan: Some(crate::sysinfo::vulkan_report()),
            dx12: crate::sysinfo::dx12_compiler_report(),
            asr_model: self.settings.selected_asr_id().as_str(),
            aligner: self.settings.selected_aligner_id().as_str(),
            vocal_separation: self.settings.vocal_separation,
            os: &os,
            cpu: &cpu,
        };
        let draft = crate::feedback::draft(lang, &facts, log.as_deref());
        let hint = if draft.clipboard.is_empty() {
            t(L::FEEDBACK_NO_LOG)
        } else {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(draft.clipboard.clone()));
            t(L::FEEDBACK_OPENED)
        };
        cx.open_url(&draft.url);
        self.flash_hint(hint, cx);
    }
}
