//! 热词页 —— 设置抽屉的**第二页**（`DrawerPage::Hotword`）。
//!
//! 热词管理本质是一张表单（模型下载 + 组分栏 + 文本编辑），所以它不新造第三种
//! 界面，而是放进同一个右滑抽屉：共用宽度、滑入动画、开抽屉快照 / 关抽屉回滚、
//! 以及底部那条保存栏。卡片用与设置页同一套 `settings_section`，于是圆角、内边距、
//! 标题 + 灰字说明、状态点、模型卡骨架全都只有一份。
//!
//! **暂存语义**：这里的编辑只写进内存里的 `settings`（`mark_settings_dirty`），
//! 点抽屉底部的「保存设置」才落盘，关抽屉＝整份回滚。与设置页完全同一套规则，
//! 这也是它和整个 app 协调的原因。

use crate::app::prelude::*;
use crate::app::ui::settings_sections::{setting_title, settings_section};

impl OneAsrApp {
    /// 热词页打开时默认编辑哪个组：默认组，没有默认组就第一个。
    ///
    /// 「没有组」不可能发生——`HotwordStore::normalize` 保证至少留一个——所以
    /// 这里返回 `Option` 只是为了不去假设那件事。
    pub(crate) fn first_hotword_group_id(&self) -> Option<String> {
        self.hotwords
            .default_group_id()
            .or_else(|| self.hotwords.groups.first().map(|g| g.id.clone()))
    }

    /// 惰性创建这一页的两个输入框（创建一次，跨组复用）。
    ///
    /// 输入框把「编辑后的文本」通过回调送回 app：回调里写进 `hotwords` 的对应组
    /// 并 `mark_settings_dirty`。`set_text` 是程序化换文，**不**走回调，所以切组时
    /// 不会把旧组的内容写进新组。
    pub(crate) fn ensure_hotword_fields(&mut self, cx: &mut Context<Self>) {
        if self.hotword_name_field.is_some() {
            return;
        }
        let app = cx.entity().downgrade();

        let app_name = app.clone();
        let name = cx.new(|cx| {
            TextField::new(
                "hotword-name",
                "",
                t(L::HOTWORD_GROUP_NAME_PLACEHOLDER),
                FieldKind::Single,
                // 单行框：行数恒为 1（不换行），横向靠光标把内容推着走。
                Rows::SINGLE,
                move |text, cx| {
                    if let Some(app) = app_name.upgrade() {
                        let text = text.to_string();
                        app.update(cx, |this, cx| this.on_hotword_name_changed(text, cx));
                    }
                },
                cx,
            )
        });

        let words = cx.new(|cx| {
            TextField::new(
                "hotword-words",
                "",
                t(L::HOTWORD_WORDS_PLACEHOLDER),
                FieldKind::List,
                // 3 行起、**填满抽屉剩下的高度**：最大化窗口时框跟着长满，57 条的
                // 词表一屏就看得完，而不是在框下面留一大片空白（用户报的那点）。
                // 窗口小的时候退化成 3 行 + 框内滚动。
                Rows::fill(3),
                move |text, cx| {
                    if let Some(app) = app.upgrade() {
                        let text = text.to_string();
                        app.update(cx, |this, cx| this.on_hotword_words_changed(text, cx));
                    }
                },
                cx,
            )
        });

        self.hotword_name_field = Some(name);
        self.hotword_words_field = Some(words);
    }

    /// 切换这一页正在编辑的组：把两个输入框的文本换成该组的内容。
    ///
    /// 只是**换编辑对象**，不落盘、也不改变任何组的内容。
    pub(crate) fn select_hotword_edit_group(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        let (name, words) = id
            .as_deref()
            .and_then(|id| self.hotwords.groups.iter().find(|g| g.id == id))
            .map(|g| (g.name.clone(), g.words.clone()))
            .unwrap_or_default();
        self.hotword_edit_group = id;
        if let Some(field) = self.hotword_name_field.clone() {
            field.update(cx, |f, cx| f.set_text(name, cx));
        }
        if let Some(field) = self.hotword_words_field.clone() {
            field.update(cx, |f, cx| f.set_text(words, cx));
        }
        cx.notify();
    }

    /// 组名输入框改了（回调）。
    fn on_hotword_name_changed(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(id) = self.hotword_edit_group.clone() {
            self.hotwords.rename_group(&id, &text);
            self.mark_settings_dirty(cx);
        }
    }

    /// 词表输入框改了（回调）。
    fn on_hotword_words_changed(&mut self, text: String, cx: &mut Context<Self>) {
        if let Some(id) = self.hotword_edit_group.clone() {
            self.hotwords.set_group_words(&id, &text);
            self.mark_settings_dirty(cx);
        }
    }

    /// 新建一个空组并把它设为当前编辑对象。
    fn add_hotword_group_action(&mut self, cx: &mut Context<Self>) {
        let id = self.hotwords.add_group("");
        self.hotword_edit_group = None; // 让 select 走一遍同步输入框的路径
        self.select_hotword_edit_group(Some(id), cx);
        self.mark_settings_dirty(cx);
    }

    /// 删除一个组。删的是默认组时提示一次（默认置空，不替用户另挑）。
    fn delete_hotword_group_action(&mut self, id: &str, cx: &mut Context<Self>) {
        let was_default = self.hotwords.default_group.as_deref() == Some(id);
        self.hotwords.remove_group(id);
        if was_default {
            self.flash_hint(t(L::HOTWORD_DEFAULT_GROUP_DELETED), cx);
        }
        if self.hotword_edit_group.as_deref() == Some(id) {
            let next = self.first_hotword_group_id();
            self.hotword_edit_group = None;
            self.select_hotword_edit_group(next, cx);
        }
        self.mark_settings_dirty(cx);
    }

    /// 把当前组设为默认组（新任务默认绑定它）；已经是默认组则取消。
    fn toggle_default_hotword_group(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.hotword_edit_group.clone() else {
            return;
        };
        let is_default = self.hotwords.default_group.as_deref() == Some(id.as_str());
        self.hotwords
            .set_default_group(if is_default { None } else { Some(&id) });
        self.play_ui(sfx::Sfx::Click);
        self.mark_settings_dirty(cx);
    }

    /// 「新任务默认用热词」开关。
    ///
    /// 只改**新加入任务**的默认绑定（`HotwordStore::new_task_group` 是唯一出口）；
    /// 已有任务、以及每个任务自己的选择都不动。打开时若还没有默认组，就把当前正在
    /// 编辑的这一组标为默认——否则开关打开却没有任何组会被绑定，等于没打开。
    fn toggle_hotword_new_task_default(&mut self, cx: &mut Context<Self>) {
        let enabled = !self.hotwords.default_group_enabled;
        // 打开时把「当前这一组」标为默认；没有正在编辑的组（理论上不会，但别假设）
        // 就退到第一个——总之不能让开关打开却没有任何组会被绑定。
        let fallback = self
            .hotword_edit_group
            .clone()
            .or_else(|| self.first_hotword_group_id());
        self.hotwords
            .set_default_group_enabled(enabled, fallback.as_deref());
        self.play_ui(sfx::Sfx::Click);
        self.mark_settings_dirty(cx);
    }

    /// 这一页的三张卡：热词粗转模型 / 热词组 / 当前组编辑器。
    ///
    /// 由抽屉的 `render_settings` 在 `DrawerPage::Hotword` 时调用——滚动容器、
    /// 保存栏、快照回滚都在抽屉那边，这里只管卡片内容。
    pub(super) fn render_hotword_cards(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        // 先把渲染需要的数据取出来（下面要 `&mut self` 建监听器）。
        let groups = self.hotwords.groups.clone();
        let default_id = self.hotwords.default_group.clone();
        let edit_id = self.hotword_edit_group.clone();
        let ready = self.sensevoice_ready;
        let dl = self.progress_for(ModelId::SenseVoiceSmall).cloned();
        let dl_busy = self.download_busy(ModelId::SenseVoiceSmall);
        let dir = self
            .settings
            .resolved_sensevoice_model_dir()
            .display()
            .to_string();
        let dir_tip = dir.clone();
        let name_field = self.hotword_name_field.clone();
        let words_field = self.hotword_words_field.clone();
        let editable = edit_id
            .as_deref()
            .is_some_and(|id| groups.iter().any(|g| g.id == id));
        let word_count = edit_id
            .as_deref()
            .and_then(|id| groups.iter().find(|g| g.id == id))
            .map(|g| oneasr_core::hotword::count_group_words(&g.words))
            .unwrap_or(0);
        let is_default = edit_id
            .as_deref()
            .is_some_and(|id| default_id.as_deref() == Some(id));
        let can_delete = groups.len() > 1;
        // 开关状态（只看这一个布尔：`normalize` 保证「开着 ⇒ 一定有默认组」）。
        let use_by_default = self.hotwords.default_group_enabled;

        // ── 卡 1：热词模型（骨架照「人声分离」那张）──────────────────────
        let model_body = div()
            .flex()
            .flex_col()
            .gap_1p5()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-sensevoice-model",
                        t(L::SENSEVOICE_MODEL),
                        t(L::TIP_SENSEVOICE_MODEL),
                    ))
                    .child(div().size(px(8.)).rounded_full().bg(if ready {
                        ACCENT
                    } else {
                        DANGER
                    })),
            )
            .child(model_download_row(
                ComponentRow {
                    id: "sensevoice-dl-btn",
                    ready,
                    busy: dl_busy,
                    progress: dl.as_ref(),
                },
                model_path_bar(
                    "sensevoice-dir",
                    dir.clone().into(),
                    dir_tip.clone().into(),
                    1,
                    false,
                    cx.listener(|this, _, _, cx| this.flash_path_copied(cx)),
                    cx.listener(|this, _, _, cx| this.pick_sensevoice_dir(cx)),
                ),
                cx.listener(|this, _, _, cx| {
                    this.start_model_download(ModelId::SenseVoiceSmall, cx);
                }),
                cx.listener(|this, _, _, cx| {
                    this.cancel_model_download(ModelId::SenseVoiceSmall, cx);
                }),
            ))
            .into_any_element();

        // ── 卡 2：热词组（横向 chip，选中即编辑对象）────────────────────────
        // 选中/未选的配色与字重**逐项照 `pill()`**（量化 / 默认启用 那些 on-off chip
        // 同款）：未选 NORMAL 字重、白底灰边；选中 SEMIBOLD、青底白字。组名由用户起、
        // 可能重名，而 `pill`/`btn` 的元素 id 来自标签 / 需要 `&'static str`——这里只能
        // 自绘，但样式值一份不改地照抄，避免它比全 app 的按钮「重」一档。
        let chips: Vec<AnyElement> = groups
            .iter()
            .map(|group| {
                let id = group.id.clone();
                let label = oneasr_core::hotword::display_name(group, ui_lang());
                let selected = edit_id.as_deref() == Some(group.id.as_str());
                let is_default = default_id.as_deref() == Some(group.id.as_str());
                div()
                    .id(SharedString::from(format!("hwg-{id}")))
                    .px_3()
                    .py_1()
                    .rounded_lg()
                    .text_sm()
                    // 组名由用户起，长度不可预测；chip 是 `flex_wrap` 排的，一条失控
                    // 的名字会把整颗撑出抽屉右沿。上限 + 省略号兜住，选中项的完整
                    // 名字就在紧邻下方的编辑器里（组名输入框）。
                    .max_w(px(crate::app::ui::metrics::HOTWORD_GROUP_CHIP_MAX_W))
                    .overflow_hidden()
                    .bg(if selected { ACCENT } else { PANEL })
                    .text_color(if selected { PANEL } else { TEXT })
                    .border_1()
                    .border_color(if selected { ACCENT } else { LINE })
                    .font_weight(if selected {
                        gpui::FontWeight::SEMIBOLD
                    } else {
                        gpui::FontWeight::NORMAL
                    })
                    .cursor_pointer()
                    .hover(|s| s.border_color(ACCENT))
                    .tooltip({
                        let tip: SharedString = if is_default {
                            t(L::HOTWORD_DEFAULT_BADGE).into()
                        } else {
                            t(L::TIP_HOTWORD_SET_DEFAULT).into()
                        };
                        move |_, cx| cx.new(|_| NameTooltip { text: tip.clone() }).into()
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_hotword_edit_group(Some(id.clone()), cx);
                    }))
                    .child(
                        div()
                            .max_w_full()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(label),
                    )
                    .into_any_element()
            })
            .collect();

        let groups_body = div()
            .flex()
            .flex_col()
            .gap_2()
            // 不参与收缩：空间不够时该缩的是下面的词表框，不是组名与 chip。
            .flex_shrink_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(setting_title(
                        "tip-hotword-groups",
                        t(L::HOTWORD_GROUPS),
                        t(L::TIP_HOTWORD),
                    ))
                    .child(btn(
                        t(L::HOTWORD_NEW_GROUP),
                        BtnKind::Secondary,
                        true,
                        cx.listener(|this, _, _, cx| this.add_hotword_group_action(cx)),
                    )),
            )
            // 召回范围：只有中英两语的发音规则进了匹配（中文拼音、英文拼写），
            // 其他语言只能按原文比。是**常驻灰字**、不是 tooltip——填词之前就要
            // 看得见，否则用户拿日语词表跑完才从结果里猜出「这不支持」。
            .child(
                div()
                    .text_xs()
                    .text_color(MUTED)
                    .child(t(L::HOTWORD_SCOPE_HINT)),
            )
            .child(div().flex().flex_wrap().gap_1p5().children(chips))
            // 开关行：只决定「新加入的任务默认绑哪一组」，与上面那两个动作（新建 /
            // 选中某组）不是一回事，所以单独一行、上面一条细分割线。
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_2()
                    .border_t_1()
                    .border_color(LINE_SOFT)
                    .child(setting_title(
                        "tip-hotword-new-task-default",
                        t(L::HOTWORD_NEW_TASK_DEFAULT),
                        t(L::TIP_HOTWORD_NEW_TASK_DEFAULT),
                    ))
                    .child(switch(
                        "switch-hotword-new-task-default",
                        use_by_default,
                        cx.listener(|this, _, _, cx| this.toggle_hotword_new_task_default(cx)),
                    )),
            )
            .into_any_element();

        // ── 当前组编辑器（组名 + 逗号词表 + 计数 + 两个动作）──────────────
        //
        // 刻意和上面那张 chip 卡**并进同一张卡**：chip 选的就是下面要编辑的那个组，
        // 拆成两张卡等于把「选择器」和「被编辑器」隔一道框，读起来是两个不相干的
        // 设置项。合成一张之后，「上面选、下面改」是一条自然的视线。
        let editor_body = if editable {
            div()
                .flex()
                .flex_col()
                .gap_2()
                // 编辑器列本身吃掉剩下高度，好让里面的词表框（`Rows::fill`）长起来。
                //
                // 这里**不**写 `min_h_0`：自动最小高度（内容高度）才是对的——窗口矮
                // 到装不下时，该由抽屉整体滚动，而不是让内容从卡片里溢出来。
                .flex_1()
                .child(match name_field {
                    Some(field) => div().flex_shrink_0().child(field).into_any_element(),
                    None => div().into_any_element(),
                })
                .child(match words_field {
                    // 词表框是这一页唯一「可以长」的东西：给它一层能长高的父容器，
                    // 框自己（`Rows::fill`）就会把抽屉里剩下的高度吃满。
                    Some(field) => div().flex().flex_1().child(field).into_any_element(),
                    None => div().into_any_element(),
                })
                // 词条计数贴着词表框的右下角走，而不是和两个动作按钮并排：
                // 左边是「信息」（这一组有多少词），右边是「动作」，混在一行里
                // 会让人以为计数也是可点的。
                .child(
                    div()
                        .flex_shrink_0()
                        .text_xs()
                        .text_color(MUTED_SOFT)
                        .child(crate::i18n::hotword_word_count(word_count)),
                )
                .child(
                    div()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .justify_between()
                        .child(pill(
                            "hw-default",
                            if is_default {
                                t(L::HOTWORD_DEFAULT_BADGE)
                            } else {
                                t(L::HOTWORD_SET_DEFAULT)
                            },
                            is_default,
                            Some(t(L::TIP_HOTWORD_SET_DEFAULT)),
                            cx.listener(|this, _, _, cx| this.toggle_default_hotword_group(cx)),
                        ))
                        .child(btn(
                            t(L::HOTWORD_DELETE_GROUP),
                            BtnKind::Quiet,
                            can_delete,
                            cx.listener(|this, _, _, cx| {
                                if let Some(id) = this.hotword_edit_group.clone() {
                                    this.delete_hotword_group_action(&id, cx);
                                }
                            }),
                        )),
                )
                .into_any_element()
        } else {
            div()
                .text_xs()
                .text_color(MUTED)
                .child(t(L::HOTWORD_NO_SELECTION))
                .into_any_element()
        };

        // 这一页只有**两张**卡：粗转模型（一张独立的事），以及「组 + 编辑器」
        // （一个整体）。原先拆成三张，每张都自带一圈描边和内边距，读起来碎。
        //
        // 第二张 `flex_1`：**吃掉抽屉里剩下的高度**。最大化窗口时，一张固定高度的
        // 卡下面会留一大片空白——那空间该给词表框（`Rows::fill`），而不是空着。
        vec![
            settings_section(model_body).into_any_element(),
            settings_section(
                div()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .flex_1()
                    .child(groups_body)
                    .child(editor_body)
                    .into_any_element(),
            )
            .flex_1()
            .into_any_element(),
        ]
    }
}
