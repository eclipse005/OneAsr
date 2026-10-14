//! TXT 导出：**纯句末标点断句**。
//!
//! TXT 不带时间轴，所以它**不吃对齐结果**。这一条是硬规则，与任务同时选了
//! 哪些格式无关：SRT / ASS 走打轴那条路，TXT 仍然只由文字按句末符号断行产出，
//! 不是「字幕去掉时间戳」。
//!
//! 于是只勾 TXT 的任务连对齐模型都不用校验——打轴产物在这条路上没有任何
//! 消费者。断行只看标点：不看时长、不做长度兜底、不跑排版 DP。
//!
//! # 拼段与断句
//!
//! ASR 是按段返回文字的，一句话可能跨段。所以多段文字先按本 crate 唯一的
//! 空格规则（[`crate::sentence_boundary::join_words`]）拼成一段，再整体断句
//! （[`body_from_parts`]）——不能逐段断句，那会把 `["你好", "世界。"]` 劈成
//! 两行。单段文本直接断句走 [`body`]。
//!
//! # 已知边界
//!
//! **不做英文缩写表。** `Mr. Smith` 里的点后面是空白，会被当成句末断成两行。
//! 这是有意的取舍：`etc.` 本身就常居句尾，半吊子缩写保护会把真正的句末粘起来，
//! 多断一句比少断一句好。
//!
//! 断行也不吃 `beautify`：美化会改标点与空格，是成稿字幕的排版步骤（见
//! [`crate::subtitle::polish`]），TXT 只负责把识别出来的句子一行一行放好。

use crate::sentence_boundary::join_words;
use crate::text_script::{TextScript, convert};

/// 一行一个句子。
///
/// 断行只认句末符号：CJK 的 `。！？．` 与拉丁的 `!?` 落点即断；ASCII 句点只在
/// 后面是空白或文本结尾时才算断句，否则 `3.5` 会被劈成两行。句末符号后面跟着
/// 的收尾引号 / 括号与连续终止符都留在本行，所以 `他说：“走。”然后离开。` 断出
/// 的第一行是 `他说：“走。”`，不会剩一个孤立的 `”`。原文已有的换行同样断行
/// ——那是 ASR 自己给的边界，不该被合回去。空行丢掉，其余原样保留，终止符留在
/// 行尾。
pub fn split_lines(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        i += 1;
        if c == '\n' || c == '\r' {
            push_line(&mut lines, &mut current);
            continue;
        }
        current.push(c);
        if is_terminator(c, &chars, i) {
            // 收尾引号 / 括号和连续终止符都归本行：句号在引号里时，断点在下
            // 引号之后，而不是把 `”` 甩到下一行。
            while i < chars.len()
                && !matches!(chars[i], '\n' | '\r')
                && (is_terminator_char(chars[i]) || is_trailing_mark(chars[i]))
            {
                current.push(chars[i]);
                i += 1;
            }
            push_line(&mut lines, &mut current);
        }
    }
    push_line(&mut lines, &mut current);
    lines
}

/// 句末符号本身；ASCII 句点的后继检查在 [`is_terminator`] 里。
fn is_terminator_char(c: char) -> bool {
    matches!(c, '。' | '！' | '？' | '．' | '!' | '?')
}

/// 句末符号。ASCII 句点带一个后继检查：小数点不算断句（点后要有空白或到
/// 文本结尾）；缩写不在保护范围内，`Mr. Smith` 会被断开（模块头有说明）。
/// 检查后继时先跳过收尾字符，`He said "stop." Then` 才能断在下引号之后。
fn is_terminator(c: char, chars: &[char], i: usize) -> bool {
    if is_terminator_char(c) {
        return true;
    }
    if c != '.' {
        return false;
    }
    let mut j = i;
    while j < chars.len() && is_trailing_mark(chars[j]) {
        j += 1;
    }
    chars.get(j).is_none_or(|next| next.is_whitespace())
}

/// 句末符号之后允许跟着的收尾字符：引号、括号的右半边（中英文与全角）。
/// 它们不是句子的开头，断行必须落在它们之后。
fn is_trailing_mark(c: char) -> bool {
    matches!(
        c,
        '"' | '\''
            | '”'
            | '’'
            | '）'
            | ')'
            | '］'
            | ']'
            | '｝'
            | '}'
            | '》'
            | '〉'
            | '』'
            | '」'
            | '】'
            | '〕'
    )
}

fn push_line(lines: &mut Vec<String>, current: &mut String) {
    let line = current.trim();
    if !line.is_empty() {
        lines.push(line.to_string());
    }
    current.clear();
}

/// 整段文字 → TXT 正文（每行一句，行尾带换行）。
pub fn body(text: &str) -> String {
    let mut out = String::new();
    for line in split_lines(text) {
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// 多段文字 → TXT 正文：先按 crate 内唯一的空格规则拼接，再整体断句。
///
/// 拼接交给 [`join_words`]（字幕渲染用的是同一份规则），所以英文段边界
/// `["Hello", "world."]` 得到 `Hello world.` 而不是 `Helloworld.`，中文段
/// 之间也不会凭空多出空格。拼接后只断一次句：跨段的半句话
/// （`["你好", "世界。"]`）本来就是一句，逐段断会把它们劈开。
///
/// 不做简繁转换——那是 [`body_from_chunks`] 的事，这条 API 给已经拿到
/// 成品文字、不打算动字形的调用方。
pub fn body_from_parts<'a>(parts: impl Iterator<Item = &'a str>) -> String {
    body(&join_words(parts))
}

/// 逐段转写结果 → TXT 正文。
///
/// 简繁转换先于拼接断句：它只改字形，不影响断行位置，也不影响 [`join_words`]
/// 的空格判断（转换表不跨段）。各段拼成一段后统一断句，段与段之间不插空行
/// ——段落边界对纯文本没有意义。
pub fn body_from_chunks<'a>(chunks: impl Iterator<Item = &'a str>, script: TextScript) -> String {
    let converted: Vec<String> = chunks.map(|chunk| convert(chunk, script)).collect();
    body_from_parts(converted.iter().map(String::as_str))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 中文一句话一行，终止符留在行尾。
    #[test]
    fn splits_on_cjk_terminators() {
        assert_eq!(
            split_lines("今天讨论预算。下周发布！可以吗？好。"),
            ["今天讨论预算。", "下周发布！", "可以吗？", "好。"]
        );
    }

    /// 中英混排：两种符号都断。
    #[test]
    fn splits_mixed_scripts() {
        assert_eq!(
            split_lines("这是 first point。Then this one! 还有吗？"),
            ["这是 first point。", "Then this one!", "还有吗？"]
        );
    }

    /// 小数点不是句末：`3.5` 不会被劈开。
    #[test]
    fn keeps_decimals_intact() {
        assert_eq!(split_lines("增长了3.5个百分点。"), ["增长了3.5个百分点。"]);
    }

    /// 英文句子靠「点 + 空白」断开。
    #[test]
    fn splits_latin_on_spaced_period() {
        assert_eq!(
            split_lines("We shipped it. Then we stopped. Done"),
            ["We shipped it.", "Then we stopped.", "Done"]
        );
    }

    /// 末尾没有终止符也要收尾，不能把最后一句丢掉。
    #[test]
    fn keeps_trailing_fragment() {
        assert_eq!(split_lines("没有句号的一段"), ["没有句号的一段"]);
    }

    /// ASR 自带的换行是它给的边界，原样断行、不合回去。
    #[test]
    fn keeps_model_newlines_as_boundaries() {
        assert_eq!(split_lines("第一行\n第二行"), ["第一行", "第二行"]);
    }

    /// 空行与纯空白行丢掉，不留空串。
    #[test]
    fn drops_blank_lines() {
        assert_eq!(
            split_lines("有内容。\n\n   \n还有。"),
            ["有内容。", "还有。"]
        );
    }

    /// 空输入不产生任何行。
    #[test]
    fn empty_input_yields_no_lines() {
        assert!(split_lines("").is_empty());
        assert!(split_lines("  \n \r\n ").is_empty());
    }

    /// 句号后面的下引号留在同一行，不会变成下一行开头的孤立 `”`。
    #[test]
    fn keeps_closing_marks_with_the_terminator() {
        assert_eq!(
            split_lines("他说：“走。”然后离开。"),
            ["他说：“走。”", "然后离开。"]
        );
    }

    /// 整句被引号包住时，行的两端是成对的引号。
    #[test]
    fn closing_mark_does_not_start_an_orphan_line() {
        assert_eq!(split_lines("“你好。”"), ["“你好。”"]);
        assert_eq!(split_lines("（就到这里。）"), ["（就到这里。）"]);
        assert_eq!(split_lines("《书名》之后。"), ["《书名》之后。"]);
    }

    /// 连续终止符吸收进当前行，不把 `！` / `!` 单独劈出来。
    #[test]
    fn absorbs_consecutive_terminators() {
        assert_eq!(split_lines("什么？！"), ["什么？！"]);
        assert_eq!(split_lines("Really?!"), ["Really?!"]);
        assert_eq!(split_lines("Wow!!!"), ["Wow!!!"]);
    }

    /// ASCII 句点的后继检查跳过收尾字符：`stop.` 后面的 `"` 属于本行。
    #[test]
    fn ascii_period_looks_past_the_closing_mark() {
        assert_eq!(
            split_lines("He said \"stop.\" Then we left."),
            ["He said \"stop.\"", "Then we left."]
        );
        assert_eq!(split_lines("He said \"stop.\""), ["He said \"stop.\""]);
    }

    /// 收尾字符集覆盖中英文引号与括号，连续收尾字符也一起吸收。
    #[test]
    fn trailing_marks_cover_cjk_and_ascii_closers() {
        assert_eq!(
            split_lines("“（真的？）”然后走了。"),
            ["“（真的？）”", "然后走了。"]
        );
        assert_eq!(
            split_lines("他喊「停！」然后跑了。"),
            ["他喊「停！」", "然后跑了。"]
        );
        assert_eq!(
            split_lines("She left (for good.) We stayed."),
            ["She left (for good.)", "We stayed."]
        );
        assert_eq!(
            split_lines("这个【很重要！】记住。"),
            ["这个【很重要！】", "记住。"]
        );
    }

    /// 缩写是已知边界而不是 bug：`Mr. Smith` 的点后是空白，会被断句。
    ///
    /// 这条测试是给文档当哨兵的：真要做缩写保护，得先改掉它和模块头的
    /// 「已知边界」，说明取舍变了。
    #[test]
    fn known_boundary_abbreviations_split() {
        assert_eq!(split_lines("Mr. Smith arrived."), ["Mr.", "Smith arrived."]);
        assert_eq!(
            split_lines("买了苹果 etc. 就走了。"),
            ["买了苹果 etc.", "就走了。"]
        );
    }

    /// 每行都带换行符，段与段之间不插空行。
    #[test]
    fn body_terminates_every_line() {
        assert_eq!(body("一句。二句。"), "一句。\n二句。\n");
    }

    /// 多段转写结果直接首尾相接。
    #[test]
    fn body_from_chunks_concatenates_without_blank_lines() {
        assert_eq!(
            body_from_chunks(["甲。", "乙。"].into_iter(), TextScript::Original),
            "甲。\n乙。\n"
        );
    }

    /// 跨段的一整句只在拼接后断一次，不被段边界劈成两行。
    #[test]
    fn body_from_chunks_splits_after_joining_across_chunks() {
        assert_eq!(
            body_from_chunks(["你好", "世界。"].into_iter(), TextScript::Original),
            "你好世界。\n"
        );
    }

    /// 英文段的拼接走字幕那份空格规则，不把两个词粘成一个。
    #[test]
    fn body_from_parts_keeps_latin_spacing_at_chunk_boundaries() {
        assert_eq!(
            body_from_parts(["Hello", "world."].into_iter()),
            "Hello world.\n"
        );
    }

    /// 三条英文句子各占一行：这是 `body_from_parts` 的接口契约；`render` 接线
    /// 的回归反例在 `oneasr-cli` 的
    /// `rendered_txt_body_keeps_english_sentences_separate`。
    #[test]
    fn body_from_parts_keeps_english_sentences_separate() {
        assert_eq!(
            body_from_parts(["Hello world.", "Second sentence.", "Third one."].into_iter()),
            "Hello world.\nSecond sentence.\nThird one.\n"
        );
    }

    /// 简繁转换在断句之前生效，断行位置不受影响。
    #[test]
    fn body_from_chunks_applies_script_conversion() {
        let got = body_from_chunks(["学习。"].into_iter(), TextScript::Traditional);
        assert_eq!(got.lines().count(), 1, "转换不应增加行数：{got}");
        assert!(got.contains('學'), "应转成繁体：{got}");
    }
}
