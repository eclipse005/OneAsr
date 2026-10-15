//! 最小文本输入框（本仓库唯一一处文本编辑）。
//!
//! gpui 自带的是 `EntityInputHandler` + `ElementInputHandler` 那一套底层接口，没有现成
//! 的输入控件，所以这里自己拼一个：文字用 `TextSystem::shape_line` 整形后自绘，光标
//! 位置由 `LineLayout` 的 `x_for_index` 算出来。
//!
//! 文字提交走**平台输入处理器**（WM_CHAR / IME 走 `replace_text_in_range` /
//! `replace_and_mark_text_in_range`），所以 `on_key_down` 只处理非文字键
//! （退格 / 方向 / Home/End / Enter / Ctrl 组合）。两条路都插一遍就会双写。
//!
//! # 高度与滚动
//!
//! 内容短于 `min_rows` 时框至少那么高，长于 `max_rows` 时**封顶**：
//!
//! - 只整形**可见窗**内的行（`[first_row, first_row + 可见行数)`）：一千条词的词表
//!   每帧也只 shape 十来行。不封顶地「按内容长高」会让每敲一个字都重新 layout 全文；
//! - 容器 `overflow_hidden`：字形、光标、选区都不可能画到框外；
//! - 可见窗跟着**光标**走（`top_row` / `scroll_x_for` 是纯函数），敲字、方向键、
//!   程序化换文都不会把光标留在看不见的地方；
//! - 滚轮可以在**不动光标**的前提下翻页（`scroll_row`）。两个来源的优先级写死在
//!   `top_row` 里：光标出窗就把窗口拉回来，光标没动就保持用户翻到的位置。
//!
//! 支持：鼠标点选 / **拖选**、Shift + 方向键 / Home / End 扩选、Ctrl+A 全选、
//! Ctrl+C/X/V、**Ctrl+Z 撤销 / Ctrl+Y（或 Ctrl+Shift+Z）重做**、滚轮翻页。
//!
//! 撤销按**用户能感知的一步**合并：连续打字与一次输入法组合算一步，粘贴 / 删除 /
//! 换行 / 导航各算一步（规则在 [`EditHistory`]，纯状态、可单测）。换编辑对象
//! （[`TextField::set_text`]）会清空历史——撤销不该把上一个组的词写回这一个组。
//!
//! 明确不做：双击选词、撤销栈跨编辑对象、横向滚动条、富文本。折行按**字符宽度**
//! 贪心装箱，但**优先在空格后断**（拉丁词不从中间劈开，CJK 无空格时退回按字符断）。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AbsoluteLength, App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler,
    FocusHandle, Focusable, Font, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    Pixels, Point, Render, ScrollWheelEvent, ShapedLine, SharedString, UTF16Selection, Window,
    canvas, div, fill, phi, point, prelude::*, px, rems, size,
};

use crate::theme::{ACCENT, ACCENT_SOFT, LINE, MUTED_SOFT, PANEL, TEXT};

/// 输入框的排版模式。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// 单行：不换行，Enter 不插入任何字符。
    Single,
    /// 词表（热词）：**一行一个词**，Enter 换行；粘贴的换行原样保留。
    ///
    /// 逗号**仍然是**合法分隔符——`parse_word_list` 把 `,` / `，` / 换行当同一种
    /// 分隔符，所以从别处粘来一串逗号分隔的词照样直接能用。这里不做格式转换：
    /// **展示 = 存储 = 用户输入**，用户打成什么样就是什么样。两种写法解析出来是同一
    /// 份词表，所以「老配置是逗号串」这件事根本不需要迁移。
    ///
    /// 为什么不是「一行一个词」的强制格式，而是「随用户」：200 条词挤成一条逗号长串
    /// 时，眼睛要横着扫一整行才找得到某个词；一行一个词能竖着扫，排查重复和改拼写
    /// 都快得多。而**存**什么完全不动——这条框只是让「敲起来」这件事更舒服。
    List,
}

/// 用户编辑后的回调（拿到最新文本）。`set_text` 不触发它，避免回写回环。
type ChangeFn = Rc<dyn Fn(&str, &mut App)>;

/// 一步编辑的快照：文本 + 光标 + 选区锚点。
///
/// 全文快照而不是「差量 + 反向操作」：词表这个量级（几十 KB）复制一份远比维护一套
/// 反向操作便宜，也不会因为某个编辑路径忘了记录而出现「撤销撤出半个字」。
#[derive(Clone, PartialEq, Eq, Debug)]
struct Snapshot {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

/// 撤销栈上限。词表级文本（顶多几十 KB）每条快照一个 `String`，64 步足够，
/// 也把「按住 Ctrl+Z 的人」挡在无限增长之外。
const UNDO_LIMIT: usize = 64;

/// 撤销 / 重做历史。**只管栈**：压什么、能不能合并（`coalesce`）由调用方声明，
/// 应用快照由调用方做——所以它是纯状态，能脱离 gpui 单测。
#[derive(Default)]
struct EditHistory {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// 上一步是「可以继续合并」的编辑（连续打字 / 输入法组合的中间态）。
    coalescing: bool,
}

impl EditHistory {
    /// 一次编辑**开始前**调用：把 `current` 压进撤销栈，并清掉重做栈。
    ///
    /// `coalesce` 为真且上一步也在合并中时**不压**：连续打字只留最开始那一刻，
    /// 否则 Ctrl+Z 一次退一个字，等于没有撤销。
    fn checkpoint(&mut self, current: Snapshot, coalesce: bool) {
        if coalesce && self.coalescing && !self.undo.is_empty() {
            return;
        }
        if self.undo.len() >= UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.undo.push(current);
        self.redo.clear();
        self.coalescing = coalesce;
    }

    /// 撤销一步：把 `current` 交给重做栈，返回要恢复的快照。
    fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let prev = self.undo.pop()?;
        self.redo.push(current);
        self.coalescing = false;
        Some(prev)
    }

    /// 重做一步：与 [`Self::undo`] 对称。
    fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let next = self.redo.pop()?;
        self.undo.push(current);
        self.coalescing = false;
        Some(next)
    }

    /// 打断合并：导航 / 点选 / 换文之后，下一步编辑另起一条。
    ///
    /// 少了这一句，打字 → 挪光标 → 再打字会并成一步，撤销时把中间那次导航也一起
    /// 吞掉（光标跳回去，用户不知道发生了什么）。
    fn break_run(&mut self) {
        self.coalescing = false;
    }

    /// 换文档（`set_text`）：历史整份丢掉。
    fn reset(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.coalescing = false;
    }
}

/// 一帧的整形结果：画字、光标、鼠标命中都靠它。
struct Geometry {
    /// 文本区左上角（画布 bounds，已排除父级内边距）。
    origin: Point<Pixels>,
    /// **可见窗**内的整形行（下标 0 对应整段行表里的 `first_row` 行）。
    lines: Vec<ShapedLine>,
    /// 可见窗首行在**整段**行表里的行号。
    first_row: usize,
    /// 每行起始字节偏移，**整段**行表（可见窗外的行也在，用来把字节映射到行号）。
    line_starts: Vec<usize>,
    line_height: Pixels,
    /// 水平偏移：单行框里光标跑到右边框外时把内容往左推（`Single` 之外恒为 0）。
    scroll_x: Pixels,
    /// 这一帧画的光标（字节）。**用来判断下一帧「光标动没动」**：窗口是跟光标走
    /// 还是保持滚轮翻到的位置，全看这一个比较（见 `top_row`）。
    cursor: usize,
}

/// 单行框横向滚动时，光标右侧至少留出的余量。
const CARET_MARGIN: f32 = 4.0;

/// 输入框的高度约束（行数）：内容少时至少 `min` 行，多时最多 `max` 行。
///
/// 收成一个值而不是两个松散的 `usize`，是因为这两条边**必须一起传**：只给最小值
/// 就成了「无限长高」（一千条词的词表每帧都要 layout 全文），只给最大值又没了
/// 空框的呼吸。调用方只该表达「这个框最多长到几行」。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rows {
    min: usize,
    max: usize,
}

impl Rows {
    /// 单行框（不换行，也不滚动）。
    pub const SINGLE: Self = Self { min: 1, max: 1 };

    /// 随内容长高，到 `max` 行封顶（超出部分框内滚动）。
    pub const fn up_to(min: usize, max: usize) -> Self {
        Self { min, max }
    }

    /// 归一化后的 `(最少行数, 最多行数)`：都至少 1，且 `max >= min`。
    fn bounds(self) -> (usize, usize) {
        let min = self.min.max(1);
        (min, self.max.max(min))
    }
}

pub struct TextField {
    /// 稳定元素 id（同一窗内唯一）。
    id: SharedString,
    focus_handle: FocusHandle,
    text: String,
    /// 光标字节偏移。
    cursor: usize,
    /// 选区锚点（字节）；`None` = 无选区。
    anchor: Option<usize>,
    /// 输入法正在组合的区间（字节），组合期间替换它、不追加。
    marked: Option<(usize, usize)>,
    placeholder: SharedString,
    kind: FieldKind,
    /// 最少显示行数（内容更多时自动长高，到 `max_rows` 封顶）。
    min_rows: usize,
    /// 最多显示行数（超出的部分靠内部滚动看）。`Single` 恒为 1。
    max_rows: usize,
    /// 顶行（滚轮翻页用）。真正的可见窗每帧由 `top_row` 从它和光标一起推出来，
    /// paint 再把夹取后的结果写回来——所以滚轮下一次是从「这一帧真的在哪」继续，
    /// 而不是从一个过期的意图继续。
    scroll_row: Rc<Cell<usize>>,
    /// 撤销 / 重做（见 [`EditHistory`]）。
    history: EditHistory,
    /// 左侧按钮是不是在本框内按下的、且还没松手——只有这种拖动才扩选。
    ///
    /// 不加这个判断的话，在别处按住鼠标拖过本框也会改变选区。
    dragging: bool,
    /// 上一帧的整形结果，鼠标命中 / IME 光标定位用。每帧由 paint 刷新。
    geometry: Rc<RefCell<Option<Geometry>>>,
    on_change: ChangeFn,
}

impl TextField {
    pub fn new(
        id: impl Into<SharedString>,
        text: impl Into<String>,
        placeholder: impl Into<SharedString>,
        kind: FieldKind,
        rows: Rows,
        on_change: impl Fn(&str, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        let (min_rows, max_rows) = rows.bounds();
        Self {
            id: id.into(),
            focus_handle: cx.focus_handle(),
            text: text.into(),
            cursor: 0,
            anchor: None,
            marked: None,
            placeholder: placeholder.into(),
            kind,
            min_rows,
            max_rows,
            scroll_row: Rc::new(Cell::new(0)),
            history: EditHistory::default(),
            dragging: false,
            geometry: Rc::new(RefCell::new(None)),
            on_change: Rc::new(on_change),
        }
    }

    /// 程序化换文（切换编辑对象时用）。**不触发** `on_change`，也不动光标，
    /// 并清空撤销历史——撤销不该把上一个组的词写回这一个组。
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        let len = self.text.len();
        self.cursor = self.cursor.min(len);
        self.anchor = None;
        self.marked = None;
        self.history.reset();
        self.dragging = false;
        cx.notify();
    }

    pub fn focus(&mut self, window: &mut Window) {
        window.focus(&self.focus_handle);
    }

    // ── 编辑 ──────────────────────────────────────────────────────────

    fn selection(&self) -> Range<usize> {
        match self.anchor {
            Some(a) => a.min(self.cursor)..a.max(self.cursor),
            None => self.cursor..self.cursor,
        }
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            text: self.text.clone(),
            cursor: self.cursor,
            anchor: self.anchor,
        }
    }

    /// 恢复一步快照。光标/锚点都夹回文本长度内（历史与文本理论上一致，夹一下是
    /// 为了任何一条编辑路径写坏了也不会 panic）。
    fn apply_snapshot(&mut self, snap: Snapshot) {
        self.text = snap.text;
        let len = self.text.len();
        self.cursor = snap.cursor.min(len);
        self.anchor = snap.anchor.map(|a| a.min(len));
        self.marked = None;
    }

    fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
        self.history.break_run();
    }

    /// 用 `new` 替换 `range`，光标落到替换文本末尾——**不**碰撤销栈。
    /// 编辑路径请走 [`Self::edit`]。
    fn replace_range(&mut self, range: Range<usize>, new: &str) {
        let new = sanitize(new, self.kind);
        let len = self.text.len();
        let start = range.start.min(len);
        let end = range.end.clamp(start, len);
        self.text.replace_range(start..end, &new);
        self.cursor = start + new.len();
        self.anchor = None;
    }

    /// 一次用户编辑：压快照 → 改文本。
    ///
    /// `coalesce` = 这一步与上一步属于同一串输入（连续打字 / 输入法组合），撤销时
    /// 合成一步；删除、粘贴、换行传 `false`。
    fn edit(&mut self, range: Range<usize>, new: &str, coalesce: bool) {
        self.history.checkpoint(self.snapshot(), coalesce);
        self.replace_range(range, new);
    }

    /// 撤销 / 重做。返回是否真的改了文本（调用方据此决定要不要 `on_change`）。
    fn undo_edit(&mut self) -> bool {
        match self.history.undo(self.snapshot()) {
            Some(prev) => {
                self.apply_snapshot(prev);
                true
            }
            None => false,
        }
    }

    fn redo_edit(&mut self) -> bool {
        match self.history.redo(self.snapshot()) {
            Some(next) => {
                self.apply_snapshot(next);
                true
            }
            None => false,
        }
    }

    /// 鼠标按下：聚焦 + 按坐标落光标，并进入拖动（按住不放就是拖选）。
    ///
    /// Shift + 点击 = 「从原锚点选到这里」：锚点不动，光标跳过去。
    fn begin_drag(&mut self, ev: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        self.dragging = true;
        if let Some(byte) = self.byte_at(ev.position) {
            if ev.modifiers.shift {
                if self.anchor.is_none() {
                    self.anchor = Some(self.cursor);
                }
            } else {
                // 普通点击：锚点落在同一处（退化选区），之后拖动就从这里开始扩。
                self.anchor = Some(byte);
            }
            self.cursor = byte;
            self.history.break_run();
            cx.notify();
        }
    }

    /// 拖动中：锚点不动、光标跟着指针走——这就是「拖选」。
    fn drag_to(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || !ev.dragging() {
            return;
        }
        if let Some(byte) = self.byte_at(ev.position)
            && byte != self.cursor
        {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
            self.cursor = byte;
            cx.notify();
        }
    }

    /// 松开左键（含在框外松开）：拖动结束。选区留着。
    fn end_drag(&mut self) {
        self.dragging = false;
    }

    /// 退格。返回**是否真的改了文本**——在开头处按退格什么都不会发生，而调用方
    /// 据此决定要不要回调 `on_change`（否则空框里按一下就把抽屉标成「未保存」）。
    fn delete_backward(&mut self) -> bool {
        let range = backward_delete_range(&self.text, self.cursor, self.anchor);
        match range {
            Some(range) => {
                self.edit(range, "", false);
                true
            }
            None => false,
        }
    }

    /// 删除键。同样返回是否真的改了文本。
    fn delete_forward(&mut self) -> bool {
        let range = forward_delete_range(&self.text, self.cursor, self.anchor);
        match range {
            Some(range) => {
                self.edit(range, "", false);
                true
            }
            None => false,
        }
    }

    /// 移动光标。`keep_selection`（Shift）为真则保留锚点（从原位置扩选），
    /// 否则收起选区——顺带打断撤销合并，让「打字→挪光标→再打字」是两步。
    fn move_cursor(&mut self, to: usize, keep_selection: bool) {
        if keep_selection {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = to.min(self.text.len());
        self.history.break_run();
    }

    /// 光标所在行的字节区间（不含行尾 `\n`）。
    fn current_line_range(&self) -> Range<usize> {
        let start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        start..end
    }

    fn notify_changed(&mut self, cx: &mut Context<Self>) {
        let cb = self.on_change.clone();
        cb(&self.text, cx);
        cx.notify();
    }

    /// 撤销 / 重做一步，并把新文本回传（`on_change`）；没有变化就什么都不做。
    fn apply_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        let changed = if redo {
            self.redo_edit()
        } else {
            self.undo_edit()
        };
        if changed {
            self.notify_changed(cx);
        }
    }

    fn handle_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        // Ctrl / Cmd 组合走命令，不落到下面的编辑键。
        if m.control || m.platform {
            match key {
                "a" => {
                    self.select_all();
                    cx.notify();
                }
                "c" => {
                    let sel = self.selection();
                    if !sel.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            self.text[sel.clone()].to_string(),
                        ));
                    }
                }
                "x" => {
                    let sel = self.selection();
                    if !sel.is_empty() {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            self.text[sel.clone()].to_string(),
                        ));
                        self.edit(sel, "", false);
                        self.notify_changed(cx);
                    }
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard()
                        && let Some(text) = item.text()
                    {
                        let sel = self.selection();
                        self.edit(sel, &text, false);
                        self.notify_changed(cx);
                    }
                }
                // Ctrl+Z 撤销；Ctrl+Y 与 Ctrl+Shift+Z 都认作重做——前者是 Windows
                // 习惯、后者是 macOS / 浏览器的，两个都收比让用户猜更好。
                "z" if m.shift => self.apply_history(true, cx),
                "z" => self.apply_history(false, cx),
                "y" => self.apply_history(true, cx),
                _ => {}
            }
            return;
        }

        let keep = m.shift;
        let moved = match key {
            "backspace" => {
                if self.delete_backward() {
                    self.notify_changed(cx);
                }
                return;
            }
            "delete" => {
                if self.delete_forward() {
                    self.notify_changed(cx);
                }
                return;
            }
            "enter" if self.kind == FieldKind::List => {
                // 一行一个词：Enter 直接换行。用户粘进来的逗号串照样能用
                // （`parse_word_list` 把逗号与换行当同一种分隔符），所以这里
                // 不做任何「帮用户整理格式」的猜测——那等于替他改词表。
                let sel = self.selection();
                self.edit(sel, "\n", false);
                self.notify_changed(cx);
                return;
            }
            "left" => {
                let to = prev_char_boundary(&self.text, self.selection().start);
                self.move_cursor(to, keep);
                true
            }
            "right" => {
                let to = next_char_boundary(&self.text, self.selection().end);
                self.move_cursor(to, keep);
                true
            }
            "home" => {
                let to = self.current_line_range().start;
                self.move_cursor(to, keep);
                true
            }
            "end" => {
                let to = self.current_line_range().end;
                self.move_cursor(to, keep);
                true
            }
            _ => false,
        };
        if moved {
            cx.notify();
        }
        // 未识别的键（含 Escape / Tab）不消费，让它们照常冒泡给上层。
        let _ = window;
    }

    /// 滚轮纵向滚动（只有内容超过可见行数时才有得滚）。
    ///
    /// 写的是**期望顶行**，真正夹取留给下一帧的 `top_row`——那里同时看得见光标和
    /// 可用高度，是唯一能判断「这个意图合不合法」的地方。光标不在窗外的滚轮动作
    /// 不会改变光标，所以窗口停在新位置（见模块头「高度与滚动」）。
    ///
    /// 滚到头（或压根没得滚）时**不消费事件**：让抽屉接着滚，用户不会在词表框里
    /// 「滚不动了也出不去」。
    fn scroll_by_wheel(
        &mut self,
        ev: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.kind != FieldKind::List {
            return;
        }
        let (_, line_height) = field_metrics(window);
        let dy = ev.delta.pixel_delta(line_height).y;
        // 触控板给小数值像素，按行四舍五入；不足半行不动（免得抖）。
        let rows = (f32::from(dy) / f32::from(line_height)).round() as i64;
        if rows == 0 {
            return;
        }
        let current = self.scroll_row.get() as i64;
        let wanted = (current + rows).max(0) as usize;
        // 夹到「最后一行刚好露出来」的位置。不夹的话在底部会永远「变了没」——
        // 滚轮被一直消费，抽屉就滚不动了。
        let limit = {
            let geom = self.geometry.borrow();
            match geom.as_ref() {
                Some(g) => max_top_row(g.line_starts.len(), g.lines.len()),
                None => 0,
            }
        };
        let next = wanted.min(limit);
        if next == self.scroll_row.get() {
            return; // 已经在头 / 尾：交给外层容器
        }
        self.scroll_row.set(next);
        cx.stop_propagation();
        cx.notify();
    }

    /// 窗口坐标 → 文本字节偏移。几何没算出来（首帧）时返回 `None`。
    fn byte_at(&self, position: Point<Pixels>) -> Option<usize> {
        let geom = self.geometry.borrow();
        let geom = geom.as_ref()?;
        let rel_y = (position.y - geom.origin.y).max(px(0.0));
        let line_ix = (f32::from(rel_y) / f32::from(geom.line_height)).floor() as usize;
        let line_ix = line_ix.min(geom.lines.len().saturating_sub(1));
        let line = geom.lines.get(line_ix)?;
        // 水平方向要补上滚动偏移：画的时候内容往左推了 `scroll_x`。
        let rel_x = position.x - geom.origin.x + geom.scroll_x;
        let within = line.closest_index_for_x(rel_x).min(line.len());
        let row = geom.first_row + line_ix;
        Some(geom.line_starts.get(row).copied()? + within)
    }

    fn render_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let kind = self.kind;
        let placeholder = self.placeholder.clone();
        let empty = self.text.is_empty();
        let geometry = self.geometry.clone();
        let scroll_row = self.scroll_row.clone();
        let max_rows = self.max_rows;
        let weak = cx.entity().downgrade();
        let focus_handle = self.focus_handle.clone();
        let (font_size, line_height) = field_metrics(window);
        let text = self.text.clone();
        let cursor = self.cursor;
        let anchor = self.anchor;
        let marked = self.marked;
        // 高度取**上一帧**数出来的可见行数（内容变长时下一帧才长高，见 paint_field
        // 里那一句 request_animation_frame）；首帧还没有几何，用最少行数兜底。
        // 可见行数已被 `max_rows` 封顶，所以长词表也不会把框拉长。
        let rows_now = self
            .geometry
            .borrow()
            .as_ref()
            .map(|g| g.lines.len())
            .unwrap_or(0)
            .max(self.min_rows);
        let content_h = px(f32::from(line_height) * rows_now as f32);

        div()
            .id(self.id.clone())
            .w_full()
            .min_w_0()
            .px_2p5()
            .py_1p5()
            .rounded_md()
            .border_1()
            .border_color(LINE)
            .bg(PANEL)
            .text_sm()
            .text_color(TEXT)
            .cursor_text()
            // 画出边框的东西一律裁掉：字形只受祖先的 content mask 约束，字段自己不
            // 设这句的话，超宽 / 超高的内容会画到框外来。
            .overflow_hidden()
            .track_focus(&self.focus_handle)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev, window, cx| {
                    this.focus(window);
                    this.begin_drag(ev, window, cx);
                }),
            )
            .on_mouse_move(cx.listener(|this, ev, _window, cx| this.drag_to(ev, cx)))
            // 松开（含在框外松开，比如拖出边界）都要结束拖动，否则下一次经过本框
            // 的移动会被当成还在拖。
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_drag()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_drag()),
            )
            .on_key_down(cx.listener(|this, ev, window, cx| this.handle_key(ev, window, cx)))
            .on_scroll_wheel(
                cx.listener(|this, ev, window, cx| this.scroll_by_wheel(ev, window, cx)),
            )
            .child(
                canvas::<()>(
                    |_bounds, _window, _cx| {},
                    move |bounds, _, window, cx| {
                        paint_field(
                            bounds,
                            font_size,
                            line_height,
                            kind,
                            max_rows,
                            &text,
                            cursor,
                            anchor,
                            marked,
                            empty,
                            &placeholder,
                            &geometry,
                            &scroll_row,
                            &focus_handle,
                            &weak,
                            window,
                            cx,
                        );
                    },
                )
                .w_full()
                .h(content_h),
            )
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.render_input(window, cx)
    }
}

// ── 绘制（自由函数，避免闭包里再借 `self`）──────────────────────────────

#[allow(clippy::too_many_arguments)]
fn paint_field(
    bounds: Bounds<Pixels>,
    font_size: Pixels,
    line_height: Pixels,
    kind: FieldKind,
    max_rows: usize,
    text: &str,
    cursor: usize,
    anchor: Option<usize>,
    marked: Option<(usize, usize)>,
    empty: bool,
    placeholder: &SharedString,
    geometry: &Rc<RefCell<Option<Geometry>>>,
    scroll_row: &Rc<Cell<usize>>,
    focus_handle: &FocusHandle,
    weak: &gpui::WeakEntity<TextField>,
    window: &mut Window,
    cx: &mut App,
) {
    // 整形用的 style：family / color 继承自祖先（父 div 设了字体与颜色），
    // 字号与行高钉在这里，和上面算高度用的是同一组常量。
    let mut style = window.text_style();
    style.font_size = rems(0.875).into();
    style.line_height = phi();

    // 折行：按字符宽度贪心地切视觉行；`Single` 不换行（整段一行）。
    let font_id = window.text_system().resolve_font(&Font {
        family: style.font_family.clone(),
        features: style.font_features.clone(),
        fallbacks: style.font_fallbacks.clone(),
        weight: style.font_weight,
        style: style.font_style,
    });
    let max_w = match kind {
        FieldKind::Single => f32::INFINITY,
        FieldKind::List => f32::from(bounds.size.width),
    };
    let mut advances: HashMap<char, f32> = HashMap::new();
    let rows = wrap_rows_with(text, max_w, |ch| {
        *advances.entry(ch).or_insert_with(|| {
            window
                .text_system()
                .advance(font_id, font_size, ch)
                .map(|s| f32::from(s.width))
                .unwrap_or(f32::from(font_size))
        })
    });
    // 折行只产出字节区间，不 shape：**整段**行表很便宜，整形只做可见窗（见下）。
    let line_starts: Vec<usize> = rows.iter().map(|row| row.start).collect();
    let total_rows = rows.len();
    // 可见行数：内容短就按内容，长过 `max_rows` 就封顶——框的高度（render_input
    // 里那个 content_h）用的也是它，两处必须同源。
    let visible_rows = total_rows.min(max_rows);

    // 可见窗的顶行：光标动过（打字 / 方向键 / 点选 / 换文）就跟着光标走；只有滚轮
    // 动过就保持用户翻到的位置。判据是「上一帧画的光标和这一帧一样吗」——
    // 见 `top_row`。
    let (prev_cursor, prev_rows) = {
        let g = geometry.borrow();
        match g.as_ref() {
            Some(g) => (Some(g.cursor), Some(g.lines.len())),
            None => (None, None),
        }
    };
    let cursor_row = row_of(&line_starts, cursor);
    let follow_caret = prev_cursor != Some(cursor);
    let first_row = top_row(
        scroll_row.get(),
        cursor_row,
        total_rows,
        visible_rows,
        follow_caret,
    );

    // 只整形可见窗内的行：一千条词的词表每帧也只 shape 十来行。
    let last_row = (first_row + visible_rows).min(total_rows);
    let lines: Vec<ShapedLine> = rows[first_row..last_row]
        .iter()
        .map(|row| {
            let slice = &text[row.clone()];
            window.text_system().shape_line(
                slice.to_string().into(),
                font_size,
                &[style.to_run(slice.len())],
                None,
            )
        })
        .collect();

    // 水平偏移：单行框里光标顶到右边框时把内容往左推，光标始终看得见。按宽度折行
    // 的模式（`List`）每个视觉行本来就装得下，恒为 0。
    let scroll_x = px(match kind {
        FieldKind::Single => {
            let line = lines.first();
            let caret_x = line
                .map(|l| f32::from(l.x_for_index(cursor.min(l.len()))))
                .unwrap_or(0.0);
            let line_w = line.map(|l| f32::from(l.width)).unwrap_or(0.0);
            scroll_x_for(caret_x, line_w, f32::from(bounds.size.width), CARET_MARGIN)
        }
        FieldKind::List => 0.0,
    });

    let geom = Geometry {
        origin: bounds.origin,
        lines,
        first_row,
        line_starts,
        line_height,
        scroll_x,
        cursor,
    };

    // 选区底色（**先于文字**画：铺在字下面。画在字上面会把文字盖掉，看起来就像
    // 「全选了但看不出来」）。
    let mut sel = match anchor {
        Some(a) => a.min(cursor)..a.max(cursor),
        None => cursor..cursor,
    };
    if let Some((s, e)) = marked
        && e > s
    {
        sel = s..e;
    }
    if !sel.is_empty() {
        for rect in selection_rects(&geom, text, sel) {
            window.paint_quad(fill(rect, ACCENT_SOFT));
        }
    }

    // 画字；空文本且未聚焦时画占位符。
    if empty && !focus_handle.is_focused(window) {
        let mut ph = style;
        ph.color = MUTED_SOFT.into();
        let shaped = window.text_system().shape_line(
            placeholder.clone(),
            font_size,
            &[ph.to_run(placeholder.len())],
            None,
        );
        let _ = shaped.paint(geom.origin, line_height, window, cx);
    } else {
        for (ix, line) in geom.lines.iter().enumerate() {
            let origin = point(
                geom.origin.x - geom.scroll_x,
                geom.origin.y + line_height * ix as f32,
            );
            let _ = line.paint(origin, line_height, window, cx);
        }
    }

    // 光标（仅在聚焦时画）。
    if focus_handle.is_focused(window)
        && let Some(p) = caret_point(&geom, cursor)
    {
        let caret = Bounds::new(p, size(px(1.5), line_height));
        window.paint_quad(fill(caret, ACCENT));
    }

    // 注册平台输入处理器（IME / 字符提交都走它）。
    if let Some(entity) = weak.upgrade() {
        window.handle_input(focus_handle, ElementInputHandler::new(bounds, entity), cx);
    }

    // 几何只在这里刷新，供鼠标命中 / IME 光标定位（下一帧的输入事件用），并让
    // 滚轮下回从「这一帧真的在哪」继续。本帧已经把要用的都取在前面了。
    let painted_rows = geom.lines.len();
    *geometry.borrow_mut() = Some(geom);
    scroll_row.set(first_row);

    // 行数变了 → 需要一帧把高度改过来（高度取的是上一帧数出来的行数）。
    // 只在真的变了才请求：否则会变成每帧自转的动画。
    if prev_rows != Some(painted_rows) {
        window.request_animation_frame();
    }
}

/// 光标点（窗口坐标）。
///
/// 行号从**整段**行表推出来，但只有落进可见窗才有坐标可给：光标被滚轮留在窗外
/// 的那一帧返回 `None`（不画），下一帧窗口就把它拉回来了（见 `top_row`）。
fn caret_point(geom: &Geometry, byte: usize) -> Option<Point<Pixels>> {
    let row = row_of(&geom.line_starts, byte);
    let ix = row.checked_sub(geom.first_row)?;
    let line = geom.lines.get(ix)?;
    let start = geom.line_starts[row];
    let within = byte.saturating_sub(start).min(line.len());
    Some(point(
        geom.origin.x + line.x_for_index(within) - geom.scroll_x,
        geom.origin.y + geom.line_height * ix as f32,
    ))
}

/// 字节 → 行号（该字节落在**整段**行表的哪一行）。
///
/// 行表按起始字节递增且逐段覆盖全文（末尾换行会留下一个空行），所以一次
/// `partition_point` 就够。`byte == text.len()` 落在末行。
fn row_of(line_starts: &[usize], byte: usize) -> usize {
    line_starts
        .partition_point(|&start| start <= byte)
        .saturating_sub(1)
        .min(line_starts.len().saturating_sub(1))
}

/// 可见窗的顶行。两条规则，按「光标这一帧动没动」分工：
///
/// - `follow = true`（打字 / 方向键 / 点选 / 程序化换文）：把光标拉进窗里，**最小
///   移动**——在窗口下沿外就贴下沿，上沿外就贴上沿；已经在窗里就保持 `wanted`
///   （否则每次敲字视图都会跳）。
/// - `follow = false`（只有滚轮动过）：只把 `wanted` 夹到合法区间。**这一条不能
///   省**：光标停在窗口下方时，若还按「拉进窗里」处理，往上翻页会被立刻拽回去，
///   滚轮等于失效。
fn top_row(
    wanted: usize,
    cursor_row: usize,
    total_rows: usize,
    height_rows: usize,
    follow: bool,
) -> usize {
    if height_rows == 0 || total_rows <= height_rows {
        return 0;
    }
    let max_top = max_top_row(total_rows, height_rows);
    let wanted = wanted.min(max_top);
    if !follow {
        return wanted;
    }
    if cursor_row < wanted {
        cursor_row
    } else if cursor_row >= wanted + height_rows {
        (cursor_row + 1 - height_rows).min(max_top)
    } else {
        wanted
    }
}

/// 能滚到的最大顶行（最后一行刚好露出来时，顶行就在这）。
///
/// 滚轮与 `top_row` 用的是同一个上限：两边各写一份，迟早有一边先漂。
fn max_top_row(total_rows: usize, height_rows: usize) -> usize {
    total_rows.saturating_sub(height_rows)
}

/// 单行框的水平偏移：把光标拉进可视宽度（右侧留 `margin`），内容比框短就是 0。
///
/// 偏移永远由光标推出来，不存状态：横向没有滚轮，光标就是唯一的位置来源。
fn scroll_x_for(caret_x: f32, line_w: f32, viewport_w: f32, margin: f32) -> f32 {
    let overflow = (line_w - viewport_w).max(0.0);
    (caret_x + margin - viewport_w).max(0.0).min(overflow)
}

/// 可见窗内、与选区相交的每一行各画一段底色。
///
/// 逐行画（而不是拿选区两端凑一个矩形）有三个必须的理由：
/// - 跨行选区：两端坐标画不出中间那些整行；
/// - `Ctrl+A`：起点在第 0 字节，而窗口可能停在文末（光标在末尾），起点压根不在窗内
///   ——旧实现拿 `caret_point` 取两端，一见 `None` 就整段不画，于是「全选了但看不
///   出来」；
/// - 拖到行尾时，行尾那截空白也该带上（不然看着像少选了几个字）。
///
/// 行与字节的边界算术在 [`selection_row_spans`] 里（纯函数、可单测），这里只把
/// 结果换算成坐标。
fn selection_rects(geom: &Geometry, text: &str, sel: Range<usize>) -> Vec<Bounds<Pixels>> {
    let mut out = Vec::new();
    for (row, span, whole_line) in selection_row_spans(&geom.line_starts, text, sel) {
        let ix = match row.checked_sub(geom.first_row) {
            Some(ix) if ix < geom.lines.len() => ix,
            _ => continue, // 窗外的行：不画，滚到了自然会画
        };
        let line = &geom.lines[ix];
        let row_start = geom.line_starts[row];
        let x_at =
            |byte: usize| geom.origin.x + line.x_for_index(byte.min(line.len())) - geom.scroll_x;
        let x1 = x_at(span.start - row_start);
        let x2 = if whole_line {
            geom.origin.x + line.width - geom.scroll_x
        } else {
            x_at(span.end - row_start)
        };
        let y = geom.origin.y + geom.line_height * ix as f32;
        // 空行被选中时宽度是 0：给一小段，否则「选中了一个空行」根本看不见。
        let w = if x2 > x1 { x2 - x1 } else { px(6.) };
        out.push(Bounds::new(point(x1, y), size(w, geom.line_height)));
    }
    out
}

/// 选区与每一行相交的部分：`(行号, 行内字节区间, 该行是否整行入选)`。
///
/// 拆出来是为了能单测——`\n` 不属于任何一行、空行、以及「最后一行到文末」这几个
/// 边界都在这里。注意行内区间是按**整段文本**的字节下标算的，画的时候才减去行首。
fn selection_row_spans(
    line_starts: &[usize],
    text: &str,
    sel: Range<usize>,
) -> Vec<(usize, Range<usize>, bool)> {
    let mut out = Vec::new();
    for (row, &start) in line_starts.iter().enumerate() {
        // 下一行的起点就是本行终点；最后一行到文末。行表把硬换行的 `\n` 排除在行外，
        // 所以行尾那个 `\n` 要减掉。
        let end = line_starts.get(row + 1).copied().unwrap_or(text.len());
        let line_end = if text[start..end].ends_with('\n') {
            end - 1
        } else {
            end
        };
        let s = sel.start.max(start);
        let e = sel.end.min(line_end);
        // 整行都在选区里（含行尾换行）时，行尾那段空白也算选中。
        let whole_line = sel.start <= start && sel.end >= end;
        if e <= s && !whole_line {
            continue;
        }
        out.push((row, s..e.max(s), whole_line));
    }
    out
}

// ── EntityInputHandler ─────────────────────────────────────────────────

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        adjusted_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = byte_range(&self.text, &range_utf16);
        *adjusted_range = Some(utf16_range(&self.text, range.clone()));
        Some(self.text[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let sel = self.selection();
        Some(UTF16Selection {
            range: utf16_range(&self.text, sel.clone()),
            reversed: self.anchor.is_some_and(|a| a > self.cursor),
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked.map(|(s, e)| utf16_range(&self.text, s..e))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // 组合期间提交：替换掉 marked 区间，否则替换选区。
        let marked = self.marked.take();
        let composing = marked.is_some();
        let range = match marked {
            Some((s, e)) => s..e,
            None => match range_utf16 {
                Some(r) => byte_range(&self.text, &r),
                None => self.selection(),
            },
        };
        self.edit(range, text, true);
        // 一次输入法组合到此结束：下一次输入另起一步撤销。合并的位置很关键——
        // 放在这里（而不是每个字符都断）才能让「打了一串拼音」是一步。
        if composing {
            self.history.break_run();
        }
        self.notify_changed(cx);
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _new_selected_range: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = match self.marked {
            Some((s, e)) => s..e,
            None => match range_utf16 {
                Some(r) => byte_range(&self.text, &r),
                None => self.selection(),
            },
        };
        let start = range.start.min(self.text.len());
        let sanitized = sanitize(new_text, self.kind);
        self.text.replace_range(range.clone(), &sanitized);
        self.cursor = start + sanitized.len();
        self.anchor = None;
        self.marked = if sanitized.is_empty() {
            None
        } else {
            Some((start, start + sanitized.len()))
        };
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let byte = byte_range(&self.text, &range_utf16).start;
        let geom = self.geometry.borrow();
        let Some(geom) = geom.as_ref() else {
            return Some(element_bounds);
        };
        // 坐标一律走同一份几何（含滚动偏移），否则 IME 的候选框会飘在框外。
        let p = caret_point(geom, byte)?;
        Some(Bounds::new(p, size(px(1.5), geom.line_height)))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        self.byte_at(point)
            .map(|byte| utf16_range(&self.text, 0..byte).end)
    }
}

// ── 纯函数（可单测）────────────────────────────────────────────────────

/// 折行：把文本切成若干视觉行，返回每行的字节区间。
///
/// - `\n` 是硬换行（该字符本身不属于任何一行）；
/// - 其余按 `advance` 给的字符宽度贪心装箱，**优先在空格后断**：拉丁词不该从中间
///   劈开（`Machine Translation` 断成 `Machine ` / `Translation`，而不是 `Machine Translatio` / `n`），
///   断点前那截空格留在上一行；
/// - 一行里没有空格可断（CJK 常态，或一个超长单词）时退回按字符断；
/// - 单个字符本身就超宽时仍然独占一行（否则会死循环）。
///
/// 纯函数：宽度由调用方以 `advance` 注入，所以折行规则可以脱离 GPUI 单测。
fn wrap_rows_with(
    text: &str,
    max_w: f32,
    mut advance: impl FnMut(char) -> f32,
) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let mut start = 0usize;
    let mut width = 0.0f32;
    // 最近一次「可以在其后断行」的位置：`(行内断点后偏移, 断点前的累计宽度)`。
    let mut break_after: Option<(usize, f32)> = None;

    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            rows.push(start..i);
            start = i + 1;
            width = 0.0;
            break_after = None;
            continue;
        }
        let w = advance(ch);
        if i > start && width + w > max_w {
            match break_after.filter(|(at, _)| *at > start) {
                // 在有空格的那一处断：上一行到断点为止，宽度减去断点前的部分。
                Some((at, w_at)) => {
                    rows.push(start..at);
                    start = at;
                    width -= w_at;
                }
                // 没有断点（CJK / 超长单词）：在该字符前断。
                None => {
                    rows.push(start..i);
                    start = i;
                    width = 0.0;
                }
            }
            break_after = None;
        }
        width += w;
        if ch == ' ' {
            // 断在空格**之后**：空格留在上一行行尾，下一行不从空格开头。
            break_after = Some((i + ch.len_utf8(), width));
        }
    }
    rows.push(start..text.len());
    rows
}

/// 统一换行，并按模式归一：
/// - [`FieldKind::List`]：只把 `\r\n` / `\r` 归一成 `\n`，**不做别的**——用户打的
///   逗号、换行、空格一律原样保留（`parse_word_list` 认全部这些分隔符）；
/// - [`FieldKind::Single`]：换行 → 空格（单行框不出现换行）。
fn sanitize(text: &str, kind: FieldKind) -> String {
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    match kind {
        FieldKind::List => normalized,
        FieldKind::Single => normalized.replace('\n', " "),
    }
}

/// 退格要删掉的字节区间；`None` = 无内容可删（开头处，或空文本）。
///
/// 纯函数：先有选区就删选区，否则删光标前一个字符。抽出来是为了能脱离
/// [`TextField`] 单测——它需要一个 `FocusHandle`，单测里造不出来。
fn backward_delete_range(text: &str, cursor: usize, anchor: Option<usize>) -> Option<Range<usize>> {
    let sel = match anchor {
        Some(a) => a.min(cursor)..a.max(cursor),
        None => cursor..cursor,
    };
    if !sel.is_empty() {
        return Some(sel);
    }
    let prev = prev_char_boundary(text, cursor);
    (prev != cursor).then_some(prev..cursor)
}

/// 删除键要删掉的字节区间；`None` = 无内容可删（末尾处，或空文本）。
fn forward_delete_range(text: &str, cursor: usize, anchor: Option<usize>) -> Option<Range<usize>> {
    let sel = match anchor {
        Some(a) => a.min(cursor)..a.max(cursor),
        None => cursor..cursor,
    };
    if !sel.is_empty() {
        return Some(sel);
    }
    let next = next_char_boundary(text, cursor);
    (next != cursor).then_some(cursor..next)
}

fn prev_char_boundary(s: &str, at: usize) -> usize {
    s[..at.min(s.len())]
        .char_indices()
        .next_back()
        .map(|(i, _)| i)
        .unwrap_or(0)
}

fn next_char_boundary(s: &str, at: usize) -> usize {
    let at = at.min(s.len());
    if at >= s.len() {
        return s.len();
    }
    at + s[at..].chars().next().map(char::len_utf8).unwrap_or(1)
}

/// UTF-16 区间 → UTF-8 字节区间。
fn byte_range(s: &str, range: &Range<usize>) -> Range<usize> {
    byte_for_utf16(s, range.start)..byte_for_utf16(s, range.end)
}

/// UTF-8 字节区间 → UTF-16 区间。
fn utf16_range(s: &str, range: Range<usize>) -> Range<usize> {
    utf16_for_byte(s, range.start)..utf16_for_byte(s, range.end)
}

fn byte_for_utf16(s: &str, utf16_ix: usize) -> usize {
    let mut count = 0usize;
    for (b, ch) in s.char_indices() {
        if count >= utf16_ix {
            return b;
        }
        count += ch.len_utf16();
    }
    s.len()
}

fn utf16_for_byte(s: &str, byte: usize) -> usize {
    s[..byte.min(s.len())].chars().map(|c| c.len_utf16()).sum()
}

/// 字号 / 行高（与 `text_sm` + 默认 `phi()` 行高一致）。
fn field_metrics(window: &Window) -> (Pixels, Pixels) {
    let base = AbsoluteLength::from(rems(0.875));
    (
        base.to_pixels(window.rem_size()),
        phi().to_pixels(base, window.rem_size()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每字符宽度 1 的折行，便于断言。
    fn wrap(text: &str, max_w: f32) -> Vec<Range<usize>> {
        wrap_rows_with(text, max_w, |_| 1.0)
    }

    #[test]
    fn wrap_keeps_empty_text_as_one_row() {
        assert_eq!(wrap("", 10.0), vec![0..0]);
    }

    #[test]
    fn hard_newlines_start_a_new_row_and_are_excluded() {
        assert_eq!(wrap("a\nb", 100.0), vec![0..1, 2..3]);
        // 末尾换行产生一个空行（与编辑器一致：光标能落在那）。
        assert_eq!(wrap("a\n", 100.0), vec![0..1, 2..2]);
        assert_eq!(wrap("甲\n乙\n", 100.0), vec![0..3, 4..7, 8..8]);
    }

    /// 宽度不够就在字符前断行；单个字符超宽时仍独占一行（不死循环）。
    #[test]
    fn overlong_rows_break_before_the_overflowing_char() {
        // 每字符宽 1、行宽 3 → 每行 3 个字符。
        assert_eq!(wrap("abcd", 3.0), vec![0..3, 3..4]);
        assert_eq!(wrap("abcdef", 3.0), vec![0..3, 3..6]);
        // 窄到放不下任何一个字符时，仍然一个字符一行。
        assert_eq!(wrap("ab", 0.0), vec![0..1, 1..2]);
    }

    /// `Single` 不换行（宽度无穷大 → 整段一行）。
    #[test]
    fn a_single_row_field_never_wraps() {
        // 7 个汉字 = 21 字节。
        assert_eq!(wrap("一句话很长很长", f32::INFINITY), vec![0..21]);
    }

    /// 拉丁词不从中间劈开：优先在空格后断（断点前的空格留在上一行）。
    #[test]
    fn wrap_prefers_breaking_at_a_space() {
        // 每字符宽 1、行宽 5："ab cd ef" → "ab " + "cd ef"
        assert_eq!(wrap("ab cd ef", 5.0), vec![0..3, 3..8]);
        // 一个词自己就超宽时仍退回按字符断（不硬撑成一行）
        assert_eq!(wrap("abcdefgh", 3.0), vec![0..3, 3..6, 6..8]);
        // 空格太靠前也不留超宽行：断点之后的部分照样重新装箱
        assert_eq!(wrap("a bbbbbbbb", 4.0), vec![0..2, 2..6, 6..10]);
    }

    // ── 撤销历史（纯状态，可单测） ──────────────────────────────────────

    fn snap(text: &str, cursor: usize) -> Snapshot {
        Snapshot {
            text: text.into(),
            cursor,
            anchor: None,
        }
    }

    /// 连续打字合成一步——这正是「Ctrl+Z 一次退一个字」与「退一步回到词首」的区别。
    ///
    /// 断骨验证：把 `checkpoint` 里 `coalesce && self.coalescing` 那一支去掉，
    /// 第一次断言立刻变红（撤销栈里会多出两条）。
    #[test]
    fn typing_run_is_one_undo_step() {
        let mut h = EditHistory::default();
        h.checkpoint(snap("", 0), true); // 输入第一个词
        h.checkpoint(snap("甲", 3), true); // 第二个
        h.checkpoint(snap("甲乙", 6), true); // 第三个
        let prev = h.undo(snap("甲乙丙", 9)).expect("应当能撤销");
        assert_eq!(prev.text, "", "三次连续输入应当并成一步");
        assert!(h.undo(snap("", 0)).is_none(), "再撤就没有了");
    }

    /// 非合并编辑（删除 / 粘贴 / 换行）各算一步。
    #[test]
    fn separate_edits_each_get_their_own_step() {
        let mut h = EditHistory::default();
        h.checkpoint(snap("甲", 3), true); // 打字
        h.checkpoint(snap("甲乙", 6), false); // 删除
        assert_eq!(h.undo(snap("甲", 3)).unwrap().text, "甲乙");
        assert_eq!(h.undo(snap("", 0)).unwrap().text, "甲");
    }

    /// 撤销后能重做回来；重做栈在有新编辑时清空（否则会「重做出一个分叉」）。
    #[test]
    fn redo_tracks_undo_and_is_dropped_on_a_new_edit() {
        let mut h = EditHistory::default();
        h.checkpoint(snap("甲", 3), false);
        let prev = h.undo(snap("甲乙", 6)).unwrap();
        assert_eq!(prev.text, "甲");
        assert_eq!(h.redo(snap("甲", 3)).unwrap().text, "甲乙");

        let _ = h.undo(snap("甲乙", 6)).unwrap();
        h.checkpoint(snap("甲", 3), false);
        assert!(h.redo(snap("甲改了", 6)).is_none(), "新编辑必须清掉重做栈");
    }

    /// 导航（挪光标 / 点选）打断合并：打字 → 挪光标 → 再打字 = 两步。
    ///
    /// 少了这条，撤销会把中间那次导航一起吞掉：光标跳回去，用户不知道发生了什么。
    #[test]
    fn moving_the_caret_breaks_the_run() {
        let mut h = EditHistory::default();
        h.checkpoint(snap("甲", 3), true);
        h.break_run();
        h.checkpoint(snap("甲乙", 6), true);
        assert_eq!(
            h.undo(snap("甲乙丙", 9)).unwrap().text,
            "甲乙",
            "第二次输入要独立成一步"
        );
        assert_eq!(h.undo(snap("甲乙", 6)).unwrap().text, "甲");
    }

    /// 换文档（`set_text`）清空历史——撤销不该把别组的词写进当前编辑对象。
    #[test]
    fn reset_drops_everything() {
        let mut h = EditHistory::default();
        h.checkpoint(snap("甲", 3), true);
        h.checkpoint(snap("甲乙", 6), false);
        h.reset();
        assert!(h.undo(snap("别组的词", 12)).is_none());
        assert!(h.redo(snap("别组的词", 12)).is_none());
    }

    /// 撤销栈有上限：第 65 步不会把内存吃掉，最早那步被丢掉。
    #[test]
    fn history_is_capped() {
        let mut h = EditHistory::default();
        for i in 0..(UNDO_LIMIT + 5) {
            h.checkpoint(snap(&format!("第{i}步"), i), false);
        }
        let mut steps = 0usize;
        let mut cur = snap("末", 0);
        while let Some(prev) = h.undo(cur) {
            cur = prev;
            steps += 1;
        }
        assert_eq!(steps, UNDO_LIMIT, "撤销步数应当被 UNDO_LIMIT 封顶");
    }

    // ── 可见窗（高度封顶 + 内部滚动）的纯函数 ────────────────────────────

    /// 字节 → 行号：`byte == len` 落在末行（含末尾换行留下的那个空行）。
    #[test]
    fn row_of_maps_bytes_to_the_row_that_covers_them() {
        // "a\nbb" → 行 [0..1, 2..4]
        let starts = [0usize, 2];
        assert_eq!(row_of(&starts, 0), 0);
        assert_eq!(row_of(&starts, 1), 0, "换行符本身算上一行");
        assert_eq!(row_of(&starts, 2), 1);
        assert_eq!(row_of(&starts, 4), 1, "文末");
        // "a\n" → 行 [0..1, 2..2]：文末那个空行也要落得上去
        assert_eq!(row_of(&[0usize, 2], 2), 1);
        // 单行（含空文本）
        assert_eq!(row_of(&[0], 0), 0);
        assert_eq!(row_of(&[0], 99), 0, "越界字节夹到末行，不能 panic");
    }

    /// 可见窗的顶行：光标动就跟随（最小移动），只有滚轮动就只夹取。
    ///
    /// 断骨验证：把 `follow = false` 那一支也写成「拉进窗里」，最后一条断言
    /// （滚轮往上翻、光标在下方）立刻变红——那正是「滚轮翻不动」的形状。
    #[test]
    fn top_row_follows_the_caret_and_keeps_manual_scroll() {
        const TOTAL: usize = 100;
        const VIEW: usize = 10;

        // 光标在窗内：窗口不动（否则每敲一个字视图就跳）
        assert_eq!(top_row(0, 0, TOTAL, VIEW, true), 0);
        assert_eq!(top_row(0, 9, TOTAL, VIEW, true), 0);
        // 光标刚出下沿：贴下沿（最小移动）
        assert_eq!(top_row(0, 10, TOTAL, VIEW, true), 1);
        // 光标跑到上沿外：贴上沿
        assert_eq!(top_row(50, 3, TOTAL, VIEW, true), 3);
        // 光标到末尾：贴下沿且不越界
        assert_eq!(top_row(0, 99, TOTAL, VIEW, true), TOTAL - VIEW);
        // 只有滚轮动过：夹到合法区间，**不**被光标拽回去
        assert_eq!(top_row(30, 0, TOTAL, VIEW, false), 30);
        assert_eq!(top_row(500, 0, TOTAL, VIEW, false), TOTAL - VIEW);
        // 内容不足一屏：永远贴顶
        assert_eq!(top_row(3, 0, 4, VIEW, false), 0);
        assert_eq!(top_row(3, 2, 4, VIEW, true), 0);
    }

    /// 单行框的横向偏移：把光标推回可视宽度内，光标在左边时立刻回 0。
    #[test]
    fn scroll_x_keeps_the_caret_visible_within_the_line() {
        // 内容比框短：不动
        assert_eq!(scroll_x_for(50.0, 80.0, 100.0, 4.0), 0.0);
        // 光标还在框内：不动
        assert_eq!(scroll_x_for(90.0, 200.0, 100.0, 4.0), 0.0);
        // 光标顶出右边：刚好推到「光标 + 余量」贴住右边界
        assert_eq!(scroll_x_for(150.0, 300.0, 100.0, 4.0), 54.0);
        // 光标回到左边：偏移消失（内容跟着回去）
        assert_eq!(scroll_x_for(10.0, 300.0, 100.0, 4.0), 0.0);
        // 推到尽头也不超过内容宽度
        assert_eq!(scroll_x_for(299.0, 300.0, 100.0, 4.0), 200.0);
    }

    // ── 选区分行（纯函数：行号与字节区间） ──────────────────────────────

    /// 选区按行切开：整行为一段、行尾 `\n` 不算进行内、空行也要能被选中。
    #[test]
    fn selection_spans_are_cut_per_line() {
        // "ab\ncd" → 行 [0..2, 3..5]
        let starts = [0usize, 3];
        // 单行中段
        assert_eq!(
            selection_row_spans(&starts, "ab\ncd", 0..1),
            vec![(0, 0..1, false)]
        );
        // 跨行（第 0 行后半 + 第 1 行前半）
        assert_eq!(
            selection_row_spans(&starts, "ab\ncd", 1..4),
            vec![(0, 1..2, false), (1, 3..4, false)]
        );
        // 从文首选到文末：两行都是「整行」，行尾的 `\n` 不占行内区间
        assert_eq!(
            selection_row_spans(&starts, "ab\ncd", 0..5),
            vec![(0, 0..2, true), (1, 3..5, true)]
        );
        // 空行被选中：区间是空的，但 `whole_line` 为真（画的时候给一小段）
        assert_eq!(
            selection_row_spans(&[0usize, 2, 3], "a\n\nb", 0..3),
            vec![(0, 0..1, true), (1, 2..2, true)]
        );
    }

    /// `Ctrl+A` 那种「整段入选」：每一行都要在结果里——旧实现（拿选区两端坐标
    /// 凑一个矩形）在「窗口停在文末」时一条都画不出来，就是「全选了但看不出」。
    #[test]
    fn select_all_covers_every_row() {
        let text = "甲\n乙\n丙";
        let starts = [0usize, 4, 8];
        let spans = selection_row_spans(&starts, text, 0..text.len());
        assert_eq!(spans.len(), 3, "三行都要有：{spans:?}");
        assert!(spans.iter().all(|(_, _, whole)| *whole));
    }

    #[test]
    fn sanitize_normalizes_newlines_per_mode() {
        // 词表框：只统一行尾，**不碰分隔符**——用户打的逗号串原样留着
        // （parse_word_list 认逗号也认换行，所以两种写法等价）。
        assert_eq!(sanitize("a\r\nb", FieldKind::List), "a\nb");
        assert_eq!(sanitize("a\rb", FieldKind::List), "a\nb");
        assert_eq!(sanitize("a,b\nc", FieldKind::List), "a,b\nc", "逗号不动");
        assert_eq!(
            sanitize("API,JSON\nWorkflow", FieldKind::List),
            "API,JSON\nWorkflow"
        );
        // 单行框：换行 → 空格（单行框不出现换行）。
        assert_eq!(sanitize("a\nb", FieldKind::Single), "a b");
        assert_eq!(sanitize("a\r\nb", FieldKind::Single), "a b");
    }

    #[test]
    fn utf16_and_byte_ranges_round_trip() {
        // 「中」是 3 字节 / 1 UTF-16 单元，「𝕏」是 4 字节 / 2 UTF-16 单元。
        let s = "中𝕏文";
        assert_eq!(byte_range(s, &(0..1)), 0..3);
        assert_eq!(byte_range(s, &(1..3)), 3..7);
        assert_eq!(byte_range(s, &(3..4)), 7..10);
        assert_eq!(utf16_range(s, 0..3), 0..1);
        assert_eq!(utf16_range(s, 3..7), 1..3);
        assert_eq!(utf16_range(s, 7..10), 3..4);
    }

    #[test]
    fn char_boundary_steps_handle_multibyte() {
        let s = "中𝕏";
        assert_eq!(next_char_boundary(s, 0), 3);
        assert_eq!(next_char_boundary(s, 3), 7);
        assert_eq!(next_char_boundary(s, 7), 7, "末尾不动");
        assert_eq!(prev_char_boundary(s, 7), 3);
        assert_eq!(prev_char_boundary(s, 3), 0);
        assert_eq!(prev_char_boundary(s, 0), 0, "开头不动");
    }

    /// 退格 / 删除在无内容可删时必须返回 `None` ——调用方据此**不**触发
    /// `on_change`。否则在空的词表框里按一下退格，抽屉会凭空冒出「未保存」标记。
    ///
    /// 断骨验证：让 `delete_backward` / `delete_forward` 无条件返回 `true`（旧的
    /// 无条件 `notify_changed` 行为）后，本测试立刻变红。
    #[test]
    fn deleting_at_the_edges_yields_no_range() {
        assert_eq!(backward_delete_range("", 0, None), None, "空文本");
        assert_eq!(forward_delete_range("", 0, None), None);

        // 「中𝕏」共 7 字节：光标在开头（0）退不动，在末尾（7）删不动。
        assert_eq!(backward_delete_range("中𝕏", 0, None), None, "开头退不动");
        assert_eq!(forward_delete_range("中𝕏", 7, None), None, "末尾删不动");

        // 中间位置当然删得动，且落在字符边界上（不是半个多字节字符）。
        assert_eq!(
            backward_delete_range("中𝕏", 7, None).map(|r| (r.start, r.end)),
            Some((3, 7)),
            "退格删掉整个「𝕏」"
        );
        assert_eq!(
            forward_delete_range("中𝕏", 0, None).map(|r| (r.start, r.end)),
            Some((0, 3)),
            "删除删掉整个「中」"
        );
        assert_eq!(
            forward_delete_range("中𝕏", 3, None).map(|r| (r.start, r.end)),
            Some((3, 7))
        );

        // 有选区时删整个选区，哪怕光标正贴着边界。
        assert_eq!(backward_delete_range("abcdef", 2, Some(5)), Some(2..5));
        assert_eq!(forward_delete_range("abcdef", 5, Some(2)), Some(2..5));
        // 退化选区（anchor == cursor）不算选区，退回单字符规则。
        assert_eq!(backward_delete_range("abc", 1, Some(1)), Some(0..1));
    }
}
