//! The task list: the scrolling skeleton, the empty state, and the scrollbar host.
//!
//! A single row is built by [`super::task_row::task_row_view`]; this file only
//! decides which rows exist and where the viewport is.
//!
//! ## 为什么是 `uniform_list`
//!
//! 一千多行的批量列表如果把每一行都建成元素，代价不在函数调用上：一行是
//! 一棵约三十个节点的树，各自的文字要 shape、各自的 hitbox 要进命中树、还要过
//! 一遍 taffy。而滚动每滚一格都会 `cx.notify()`（见 `vendor/gpui/src/elements/div.rs`
//! 的 `paint_scroll_listener`），于是这一整套每格重来一遍——行数越多越钝。
//!
//! 所以用 gpui 自带的 [`gpui::uniform_list`]：它只把**可见区间**交给渲染闭包，
//! 行高由它自己量第一行（`measure_item`），内容高度、滚动 clamp、`scroll_to_item`
//! 也都在它内部。这就是 Zed 自己的列表用法，不要手写窗口化——
//! 试过一版「padding 垫空白 + 自己算窗口」的写法：滚动容器的 `pt`/`pb` 会让它
//! 的高度在视口高与垫片高之间来回翻（实测 438 → 87089 → 438），一帧画 7 行、
//! 下一帧画 1194 行，滚动时一闪一闪，滚一下顶部还是大片空白。
//!
//! 代价只有一个：`uniform_list` 的渲染闭包只拿得到 `&mut Window` + `&mut App`，
//! 拿不到 `&mut Context<OneAsrApp>`。所以行内所有监听器走
//! `window.listener_for(&entity, …)`（见 `task_row`），语言 / 热词组菜单同理。
//!
//! 行高必须一致：`uniform_list` 量第一行，然后按它排所有行。行内文字全是
//! `whitespace_nowrap` + 省略/上限宽度，高度只由内边距与图标决定，天然一致。

use crate::app::prelude::*;
use crate::app::ui::task_row::{RowCtx, task_row_view};

impl OneAsrApp {
    pub(super) fn render_list(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        self.drain_row_anims();

        if self.tasks.is_empty() {
            let title = empty_state_title(ui_lang());
            let not_ready = self.model_status == ModelStatus::NotReady;
            let subtitle = empty_state_subtitle(ui_lang(), not_ready);
            self.tick_empty_wave();
            let wave = self.render_empty_wave(cx);
            return vec![
                div()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .min_w_0()
                    // Full-width interactive wave sits above the copy block.
                    .child(wave)
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_3()
                            .px_6()
                            .mt_4()
                            .child(
                                div()
                                    .text_base()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(MUTED)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(MUTED_SOFT)
                                    .text_center()
                                    .max_w(px(320.))
                                    .child(subtitle),
                            )
                            .child(
                                div()
                                    .mt_2()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .child(btn(
                                        t(L::ADD_FILES),
                                        BtnKind::Primary,
                                        true,
                                        cx.listener(|this, _, _, cx| this.add_files_dialog(cx)),
                                    ))
                                    .when(not_ready, |el| {
                                        el.child(btn(
                                            t(L::OPEN_SETTINGS),
                                            BtnKind::Secondary,
                                            true,
                                            cx.listener(|this, _, _, cx| {
                                                if !this.settings_page_open() {
                                                    this.toggle_settings(cx);
                                                }
                                            }),
                                        ))
                                    }),
                            ),
                    )
                    .into_any_element(),
            ];
        }

        // 行数据快照：`uniform_list` 的渲染闭包只拿得到 `&mut Window` + `&mut App`
        // （没有 `&self`），所以数据在这里一次备好、move 进去。
        //
        // 贵的那一半是**行元素**（每行约三十个节点 + 文字 shape + hitbox），
        // 它仍然是按可见区间懒构造的：`uniform_list` 只把视口内的区间喂给闭包。
        let rows = self.snapshot_task_rows(Instant::now());
        let count = rows.len();
        let hover_id = self.hover_row.clone();
        let lang_menu = self.lang_menu.clone();
        let hotword_menu = self.hotword_menu.clone();
        let active_stage = self.active_stage.clone();
        let timing_popover = self.timing_popover.clone();
        let timing_visible = self.timing_popover_visible();
        let timing_progress = self.timing_popover_progress();
        // 热词组（id, 显示名）快照：改名后名字随之变，任务绑定不变。
        let hotword_groups = self.hotwords.menu_options(ui_lang());

        // Row-invariant state, captured once for the whole repaint.
        let row_ctx = RowCtx {
            hover_id,
            lang_menu,
            hotword_menu,
            hotword_groups,
            active_stage,
            timing_popover,
            timing_visible,
            timing_progress,
        };

        // 滚动条浮在列表上（不占布局），只在指针靠右侧时淡入。
        let scrollbar = self.render_list_scrollbar(cx);

        let scroll = self.list_scroll.clone();
        let entity = cx.entity();
        let list = uniform_list("task-list", count, move |range, window, _cx| {
            // `count` 就是 `rows.len()`，区间也由 uniform_list 保证在表内。
            range
                .map(|ix| task_row_view(ix, &rows[ix], &row_ctx, &entity, window))
                .collect::<Vec<_>>()
        })
        .flex_1()
        .min_h_0()
        .min_w_0()
        // 滚动条拖拽的**移动与松开**挂在这里（整块列表），不挂在 14px 热区上：
        // 指针捕获的语义是「按下就抓住」，拖出窄带、拖到行上都要继续跟手。
        // 见 `scrollbar.rs` 的 `scrollbar_press`。
        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
            this.scrollbar_drag_to(ev, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.scrollbar_release(cx)),
        )
        // 在列表外（工具栏 / 状态栏）松手也要收尾，否则抓取状态会一直留着。
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.scrollbar_release(cx)),
        )
        .track_scroll(scroll);

        // 列表与滚动条是**同一个 flex 容器里的兄弟**，滚动条绝对定位
        // （外面的 `task-drop-zone` 已经是 flex_col + relative）。
        let mut out = Vec::with_capacity(2);
        out.push(list.into_any_element());
        out.extend(scrollbar);
        out
    }
}
