//! 文稿卡片：鼠标停在文稿芯片上弹出的那张。
//!
//! **它不是编辑器，也刻意不做预览**：卡片要显示的是用户自己的文件，而程序一个字
//! 都不改它（见 [`crate::app::StagedTranscript`]），整段铺回来只是让人重读一遍自己
//! 刚读过的东西。芯片上已经有文件名、行数、字数。
//!
//! 机制完全照「用时」卡片：悬停延时打开、移开有宽限期、卡片自己挂 `on_hover` 所以
//! 鼠标移上去不会关——里面因此放得下按钮。差别只一个：**点击芯片会把卡片粘住**
//! （`pinned`）。纯悬停的卡片上按按钮是难受的，粘住之后它就是一个随时可点的面板。
//!
//! 卡片只做三件只有它做得成的事：
//!
//! * 呈现**解析之后**的摘要——SRT 的序号与时间轴已剥、每个 cue 已合成一行；
//! * 密度校验。文稿与音频对不上时强制对齐**不会报错**，它给一个看起来正常但慢慢
//!   漂移的时间轴，所以这个数必须在点开始之前出现；
//! * 智能断句：只拆过长的行，**只作用在暂存的那一份上**，原文件不动。
//!
//! 改文字走「用顺手的编辑器改文件 → 回来点重新读取」。这条路径不是妥协，它就是
//! 「程序永不写回原文件」这条不变式的直接后果。

use std::path::PathBuf;
use std::thread;

use crate::app::OneAsrApp;
use crate::app::StagedTranscript;
use crate::app::prelude::*;

impl OneAsrApp {
    /// 芯片还是 `[+ 文稿]` 时点它：给这条任务选一份文稿。
    ///
    /// 芯片的另一半（有文稿时点它）是打开卡片；这条是「还没有」，所以是一次
    /// 选文件——和添加任务走的是同一个文件对话框。
    pub(crate) fn pick_transcript_for(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let id = id.to_string();
        let tx = self.tx.clone();
        let title = t(L::DLG_PICK_TRANSCRIPT).to_string();
        thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(&title)
                .add_filter("Transcript", &["txt", "md", "srt"])
                .pick_file();
            let _ = tx.send(WorkerMsg::TranscriptPicked { id, path: picked });
        });
        cx.notify();
    }

    /// 挂上文稿：读、暂存到任务上，并把卡片打开让用户立刻看见摘要。
    pub(crate) fn handle_transcript_picked(
        &mut self,
        id: &str,
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        self.picking = false;
        let Some(path) = path else {
            cx.notify();
            return;
        };
        let path = path.canonicalize().unwrap_or(path);
        match oneasr_core::transcript::read_transcript(&path) {
            Ok(parsed) => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                    task.transcript = Some(StagedTranscript::from_core(&parsed, Some(path)));
                }
                self.transcript_card = Some(id.to_string());
                self.transcript_card_pinned = true;
            }
            Err(e) => {
                self.flash_hint(expand(t(L::TRANSCRIPT_UNREADABLE), &[("e", &e)]), cx);
            }
        }
        cx.notify();
    }

    /// 智能断句：只拆过长的行、一个字都不动，结果只落在暂存的那一份上。
    ///
    /// 断完之后**不预览**——切出来的行数当场变，字幕文件里就是结果。
    pub(crate) fn smart_break_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.transcript_card.clone() else {
            return;
        };
        let lang = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.language.clone())
            .unwrap_or_else(|| self.settings.language.clone());
        let preset = self.settings.subtitle_length_preset.clone();
        let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) else {
            return;
        };
        let Some(st) = task.transcript.as_mut() else {
            return;
        };
        st.text = oneasr_core::sentence_boundary::break_long_lines(&st.text, &lang, &preset);
        st.revision += 1;
        let lines = st.line_count();
        self.transcript_card_note =
            Some(expand(t(L::TRANSCRIPT_BROKE), &[("n", &lines.to_string())]));
        cx.notify();
    }

    /// 重新读取：丢掉暂存上的改动，回到文件里的样子（文件本身一直没被写过）。
    pub(crate) fn reread_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.transcript_card.clone() else {
            return;
        };
        let path = self
            .tasks
            .iter()
            .find(|t| t.id == id)
            .and_then(|t| t.transcript.as_ref())
            .and_then(|st| st.path.clone());
        let Some(path) = path else {
            self.transcript_card_note = Some(t(L::TRANSCRIPT_NO_FILE).to_string());
            cx.notify();
            return;
        };
        match oneasr_core::transcript::read_transcript(&path) {
            Ok(parsed) => {
                let mut fresh = StagedTranscript::from_core(&parsed, Some(path.clone()));
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                    // 换文稿 = 换输入，之前那份产物不再对应当前输入。
                    fresh.revision = task.transcript.as_ref().map_or(0, |st| st.revision) + 1;
                    task.transcript = Some(fresh);
                }
                self.transcript_card_note = Some(t(L::TRANSCRIPT_REREAD).to_string());
            }
            Err(e) => {
                self.transcript_card_note = Some(expand(t(L::TRANSCRIPT_UNREADABLE), &[("e", &e)]));
            }
        }
        cx.notify();
    }

    /// 移除文稿：这条任务退回转录模式。已写出的产物不自动删。
    pub(crate) fn remove_transcript(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.transcript_card.clone() else {
            return;
        };
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.transcript = None;
        }
        self.close_transcript_card();
        self.flash_hint(t(L::TRANSCRIPT_REMOVED), cx);
        cx.notify();
    }
}

/// 填 `{name}` 占位。卡片上的文案都带占位（行数、字数、密度），而
/// `L::…` 是编译期定下的两种语言，所以填值只能在这一处发生。
pub(crate) fn expand(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}
