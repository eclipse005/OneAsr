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
//! 明确不做：选区拖拽、撤销栈、横向滚动条。折行按**字符**推进（CJK 没有词边界）。
//! 够用即可。

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;

use gpui::{
    AbsoluteLength, App, Bounds, ClipboardItem, Context, ElementInputHandler, EntityInputHandler,
    FocusHandle, Focusable, Font, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, Point, Render,
    ScrollWheelEvent, ShapedLine, SharedString, UTF16Selection, Window, canvas, div, fill, phi,
    point, prelude::*, px, rems, size,
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
            geometry: Rc::new(RefCell::new(None)),
            on_change: Rc::new(on_change),
        }
    }

    /// 程序化换文（切换编辑对象时用）。**不触发** `on_change`，也不动光标。
    pub fn set_text(&mut self, text: impl Into<String>, cx: &mut Context<Self>) {
        self.text = text.into();
        let len = self.text.len();
        self.cursor = self.cursor.min(len);
        self.anchor = None;
        self.marked = None;
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

    fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// 用 `new` 替换 `range`，光标落到替换文本末尾。
    fn replace_range(&mut self, range: Range<usize>, new: &str) {
        let new = sanitize(new, self.kind);
        let len = self.text.len();
        let start = range.start.min(len);
        let end = range.end.clamp(start, len);
        self.text.replace_range(start..end, &new);
        self.cursor = start + new.len();
        self.anchor = None;
    }

    /// 退格。返回**是否真的改了文本**——在开头处按退格什么都不会发生，而调用方
    /// 据此决定要不要回调 `on_change`（否则空框里按一下就把抽屉标成「未保存」）。
    fn delete_backward(&mut self) -> bool {
        let range = backward_delete_range(&self.text, self.cursor, self.anchor);
        match range {
            Some(range) => {
                self.replace_range(range, "");
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
                self.replace_range(range, "");
                true
            }
            None => false,
        }
    }

    fn move_cursor(&mut self, to: usize) {
        self.cursor = to.min(self.text.len());
        self.anchor = None;
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
                        self.replace_range(sel, "");
                        self.notify_changed(cx);
                    }
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard()
                        && let Some(text) = item.text()
                    {
                        let sel = self.selection();
                        self.replace_range(sel, &text);
                        self.notify_changed(cx);
                    }
                }
                _ => {}
            }
            return;
        }

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
                self.replace_range(sel, "\n");
                self.notify_changed(cx);
                return;
            }
            "left" => {
                let to = prev_char_boundary(&self.text, self.selection().start);
                self.move_cursor(to);
                true
            }
            "right" => {
                let to = next_char_boundary(&self.text, self.selection().end);
                self.move_cursor(to);
                true
            }
            "home" => {
                let to = self.current_line_range().start;
                self.move_cursor(to);
                true
            }
            "end" => {
                let to = self.current_line_range().end;
                self.move_cursor(to);
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

    /// 鼠标按下：聚焦 + 按坐标落光标。
    fn place_cursor(&mut self, ev: &MouseDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if let Some(byte) = self.byte_at(ev.position) {
            self.cursor = byte;
            self.anchor = None;
            cx.notify();
        }
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
                    this.place_cursor(ev, window, cx);
                }),
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

    // 选区底色。
    let mut sel = match anchor {
        Some(a) => a.min(cursor)..a.max(cursor),
        None => cursor..cursor,
    };
    if let Some((s, e)) = marked
        && e > s
    {
        sel = s..e;
    }
    if !sel.is_empty()
        && let (Some(a), Some(b)) = (caret_point(&geom, sel.start), caret_point(&geom, sel.end))
    {
        for rect in selection_rects(a, b, line_height) {
            window.paint_quad(fill(rect, ACCENT_SOFT));
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

/// 选区矩形（跨行时按行切成若干块；单行直接一段）。
fn selection_rects(a: Point<Pixels>, b: Point<Pixels>, line_height: Pixels) -> Vec<Bounds<Pixels>> {
    if (a.y - b.y).abs() < px(0.5) {
        let x = a.x.min(b.x);
        let w = (b.x - a.x).abs();
        return vec![Bounds::new(point(x, a.y), size(w, line_height))];
    }
    vec![Bounds::new(a, size(px(4.0), line_height))]
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
        let range = match self.marked.take() {
            Some((s, e)) => s..e,
            None => match range_utf16 {
                Some(r) => byte_range(&self.text, &r),
                None => self.selection(),
            },
        };
        self.replace_range(range, text);
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
/// - 其余按 `advance` 给的字符宽度贪心装箱，超宽就在该字符前断行；
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
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            rows.push(start..i);
            start = i + 1;
            width = 0.0;
            continue;
        }
        let w = advance(ch);
        if i > start && width + w > max_w {
            rows.push(start..i);
            start = i;
            width = 0.0;
        }
        width += w;
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
