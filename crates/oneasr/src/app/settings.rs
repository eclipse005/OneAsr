//! Settings persistence and the path pickers behind the drawer's fields.

use crate::app::prelude::*;
use crate::app::transcript_ui::expand;

impl OneAsrApp {
    /// Fast FS check for status bar + settings dots. Does not touch GPU / weights.
    /// Call after path / download / backend changes — not on every scroll paint.
    ///
    /// **唯一的就绪缓存失效入口**：这个函数只在"路径 / 下载 / 后端刚变过"时被
    /// 调用，正是磁盘内容可能已经变了的时刻（下载落盘、用户换目录、删掉文件）。
    /// 抽屉与状态栏每帧只读下面这几个布尔值，不重复打文件系统；真正的
    /// "一个目录算不算就绪"判据只有 `check_*_model_dir`（见 `model_check`）。
    pub(crate) fn refresh_model_probe(&mut self) {
        oneasr_core::invalidate_all_model_checks();
        self.asr_ready = check_asr_model_dir(&self.settings.asr_model_dir).is_ok();
        self.align_ready =
            oneasr_core::check_aligner_model_dir(&self.settings.aligner_model_dir).is_ok();
        self.demucs_ready =
            oneasr_core::check_demucs_model_dir(&self.settings.resolved_demucs_model_dir()).is_ok();
        self.model_status = if self.settings.can_start().is_ok() {
            ModelStatus::Ready
        } else {
            ModelStatus::NotReady
        };
    }

    /// Re-probe the configured model folders after a path change.
    pub(crate) fn reset_model_config(&mut self, cx: &mut Context<Self>) {
        self.refresh_model_probe();
        cx.notify();
    }

    pub(crate) fn mark_settings_dirty(&mut self, cx: &mut Context<Self>) {
        if !self.settings_dirty {
            self.settings_dirty = true;
            cx.notify();
        }
    }

    /// Restore every setting to its initial value — staged like any other
    /// edit, never written on its own.
    ///
    /// 保存设置 persists the reset; closing the drawer discards it, exactly
    /// like a manual edit. Model paths fall back to the install layout, which
    /// is what a factory reset means here.
    pub(crate) fn reset_settings(&mut self, cx: &mut Context<Self>) {
        // 欢迎卡不算「设置」：恢复出厂不该让下次启动再自动弹一遍欢迎卡。
        let welcome_done = self.settings.welcome_done;
        self.settings = Settings::default();
        self.settings.welcome_done = welcome_done;
        self.mark_settings_dirty(cx);
        // Paths moved — re-probe so the status dots follow the reset.
        self.refresh_model_probe();
        cx.notify();
    }

    pub(crate) fn is_settings_dirty(&self, _cx: &Context<Self>) -> bool {
        self.settings_dirty
    }

    /// 界面语言图标按钮：在 跟随系统 → 中文 → English 间循环。
    ///
    /// 与抽屉里其他设置**同一套暂存语义**：点击只改草稿（图标跟着变、
    /// 出现「未保存」），点「保存设置」才切换全界面文案并落盘
    /// （见 [`Self::save_settings`]），关抽屉放弃更改则随快照回滚。
    pub(crate) fn cycle_ui_language(&mut self, cx: &mut Context<Self>) {
        let next = match self.settings.ui_language.as_str() {
            oneasr_core::i18n::UI_LANGUAGE_ZH => oneasr_core::i18n::UI_LANGUAGE_EN,
            oneasr_core::i18n::UI_LANGUAGE_EN => oneasr_core::i18n::UI_LANGUAGE_SYSTEM,
            _ => oneasr_core::i18n::UI_LANGUAGE_ZH,
        };
        self.settings.ui_language = next.into();
        self.mark_settings_dirty(cx);
        self.play_ui(sfx::Sfx::Click);
        self.flash_hint(t(L::UI_LANGUAGE_APPLY_ON_SAVE), cx);
    }

    /// Write `settings.json`, re-check model files.
    ///
    /// The one and only commit point: the drawer's 保存设置 button calls
    /// this, and nothing else does — closing the drawer discards instead.
    pub(crate) fn save_settings(&mut self, cx: &mut Context<Self>) {
        match self.settings.save() {
            Ok(_) => {
                self.settings_dirty = false;
                self.settings_snapshot = Some(self.settings.clone());
                // 界面语言随本次保存生效（抽屉里选的档位是暂存的）。
                oneasr_core::i18n::set_ui_lang(self.settings.resolved_ui_language());
                self.reset_model_config(cx);
                // Only a real commit earns the tap.
                self.play_ui(sfx::Sfx::Click);
                self.flash_hint(
                    match self.model_status {
                        ModelStatus::Ready => t(L::SETTINGS_SAVED_READY),
                        ModelStatus::NotReady => t(L::SETTINGS_SAVED_NOT_READY),
                    },
                    cx,
                );
            }
            Err(e) => {
                crashlog::log_error(format!("settings save failed: {e}"));
                self.flash_hint(crate::i18n::save_failed(&e), cx);
            }
        }
    }

    pub(crate) fn add_files_dialog(&mut self, cx: &mut Context<Self>) {
        if self.picking {
            return;
        }
        self.picking = true;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title(t(L::DLG_ADD_MEDIA))
                // 「全部」必须排在第一个并且**默认选中**：rfd 一次只能选当前
                // 过滤器匹配的类型，而「音频和它的文稿一起挑」正是这个功能要的
                // 动作——先切过滤器选媒体、再切回来选文稿，那不叫一步。
                .add_filter(
                    "All supported",
                    &[
                        // media
                        "wav", "mp3", "m4a", "flac", "ogg", "opus", "mp4", "mkv", "mov", "webm",
                        "avi", "m4v", "aac", "wma", // transcript
                        "txt", "md", "srt",
                    ],
                )
                .add_filter(
                    "Media",
                    &[
                        "wav", "mp3", "m4a", "flac", "ogg", "opus", "mp4", "mkv", "mov", "webm",
                        "avi", "m4v", "aac", "wma",
                    ],
                )
                .add_filter("Transcript", &["txt", "md", "srt"])
                .pick_files();
            match files {
                Some(paths) => {
                    let _ = tx.send(WorkerMsg::FilesPicked(paths));
                }
                None => {
                    let _ = tx.send(WorkerMsg::PickCancelled);
                }
            }
        });
        cx.notify();
    }

    pub(crate) fn pick_model_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.asr_model_dir.clone();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title(t(L::DLG_ASR_DIR));
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::ModelDirPicked(dir));
            }
        });
        cx.notify();
    }

    pub(crate) fn pick_aligner_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.aligner_model_dir.clone();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title(t(L::DLG_ALIGNER_DIR));
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::AlignerDirPicked(dir));
            }
        });
        cx.notify();
    }

    pub(crate) fn pick_demucs_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.resolved_demucs_model_dir();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title(t(L::DLG_DEMUCS_DIR));
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::DemucsDirPicked(dir));
            }
        });
        cx.notify();
    }

    pub(crate) fn pick_output_dir(&mut self, cx: &mut Context<Self>) {
        let tx = self.tx.clone();
        let start = self.settings.resolved_output_dir();
        thread::spawn(move || {
            let mut dlg = rfd::FileDialog::new().set_title(t(L::DLG_OUTPUT_DIR));
            if start.is_dir() {
                dlg = dlg.set_directory(&start);
            }
            if let Some(dir) = dlg.pick_folder() {
                let _ = tx.send(WorkerMsg::OutputDirPicked(dir));
            }
        });
        cx.notify();
    }

    pub(crate) fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        // 单媒体 + 单文稿直接配对；多文件时文稿按文件名配给同名媒体。配不上的
        // **要报出来**——用户挑了一堆文件，最后靠数行数才知道哪份稿子没生效。
        let pairing = crate::app::transcript::pair_picked(&paths);
        let default_lang = self.settings.language.clone();
        let default_sep = self.settings.vocal_separation;
        // 读在循环外：媒体行要挂的那份文稿，先解析一次；读不了的当场报，任务照建
        // （不因为一份坏文稿丢掉整个添加动作）。
        let mut staged: Vec<(PathBuf, Option<StagedTranscript>)> = Vec::new();
        for (text_path, _) in &pairing.transcripts {
            let read = match oneasr_core::transcript::read_transcript(text_path) {
                Ok(parsed) => Some(StagedTranscript::from_core(
                    &parsed,
                    Some(text_path.clone()),
                )),
                Err(e) => {
                    self.flash_hint(expand(t(L::TRANSCRIPT_UNREADABLE), &[("e", &e)]), cx);
                    None
                }
            };
            staged.push((text_path.clone(), read));
        }
        // 每条媒体配到哪份文稿：按**规范化后**的媒体路径索引，循环里一次查完。
        let mut by_media: Vec<(PathBuf, Option<StagedTranscript>)> = Vec::new();
        for ((_, media), (_, parsed)) in pairing.transcripts.iter().zip(staged) {
            by_media.push((media.canonicalize().unwrap_or(media.clone()), parsed));
        }
        let crate::app::transcript::Pairing {
            media,
            transcripts,
            unpaired_texts,
        } = pairing;

        let before = self.tasks.len();
        for path in media {
            let path = path.canonicalize().unwrap_or(path);
            if !accept_input_path(&path) {
                // Silent skip in the UI is intentional ("no reaction"), but a
                // dropped file that never appears must be explainable later.
                crashlog::log_info(format!("input rejected (unsupported): {}", path.display()));
                continue;
            }
            if self.tasks.iter().any(|t| t.path == path) {
                continue;
            }
            let mut task = Task::from_path(&path, default_lang.clone(), default_sep);
            task.transcript = by_media
                .iter()
                .find(|(media, _)| *media == path)
                .and_then(|(_, st)| st.clone());
            let id = task.id.clone();
            let p = task.path.clone();
            let tx = self.tx.clone();
            // Bounded process-wide probe pool (not one thread per file).
            probe_duration_async(p, move |dur| {
                let _ = tx.send(WorkerMsg::Probed {
                    id,
                    duration_sec: dur,
                });
            });
            let anim_id = task.id.clone();
            self.tasks.push(task);
            // Wave: fade the new row in (symmetric with delete exit).
            self.entering.entry(anim_id).or_insert_with(Instant::now);
        }
        // Feedback = list itself (no toast) — plus one tap for the whole drop.
        // Deliberately NOT one per file: dropping 20 files must not stutter.
        if self.tasks.len() > before {
            self.play_ui(sfx::Sfx::Click);
        }
        // 已配对的文稿只提示数量；未配对的文稿不会建出任务，名字必须保留，
        // 否则用户不知道哪份文件没有匹配成功。
        if !transcripts.is_empty() {
            self.flash_hint(
                expand(
                    t(L::TRANSCRIPT_PAIRED),
                    &[("n", &transcripts.len().to_string())],
                ),
                cx,
            );
        }
        if !unpaired_texts.is_empty() {
            let names: Vec<String> = unpaired_texts
                .iter()
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| p.display().to_string())
                })
                .collect();
            self.flash_hint(
                expand(
                    t(L::TRANSCRIPT_UNPAIRED),
                    &[
                        ("n", &unpaired_texts.len().to_string()),
                        ("files", &names.join("、")),
                    ],
                ),
                cx,
            );
        }
        cx.notify();
    }
}
