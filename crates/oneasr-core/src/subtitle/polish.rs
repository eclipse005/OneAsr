//! 成稿字幕美化。开关打开之后，在写出 SRT / TXT / ASS 之前改每一条字幕的文字。
//!
//! 不看任务语言。文稿对齐不必选语言，识别选了语言也不能代表这一句是中文，
//! 所以每一句自己判断：
//!
//! * 句里没有汉字、假名、中文标点、全角字母数字时，原样返回。纯英文的
//!   `Hello, world.` 不会被改成没有句号逗号的样子。
//! * 有这些字时，才做 Netflix 简体那套标点（句号逗号改成一个空格、问叹号
//!   收成全角且不叠用、省略号收成 `…`、引号收成全角）和全角半角折叠。
//! * 汉字（以及假名）和英文单词、阿拉伯数字贴在一起时，中间补一个空格。
//!   旁边如果是标点（冒号、问号、书名号）就不补，已经有的也去掉。
//!   字母和数字自己贴在一起（`MP3`、`3.14`、`4:12`）不拆开。行首行尾不补空格。
//!
//! 不插入换行，也不合并、拆开字幕条。断句仍由现有的字幕长度和文稿分行决定。

use crate::sentence_boundary::is_no_space_script;

#[derive(Debug, Clone, Copy)]
pub(crate) struct MappedChar {
    pub ch: char,
    /// 这个输出字符来自输入的第几个字符（按 `chars()` 计，不含被丢掉的 `\r`）。
    pub src: usize,
}

pub(crate) fn polish_cue_text(text: &str) -> String {
    polish_mapped(text).into_iter().map(|m| m.ch).collect()
}

pub(crate) fn polish_mapped(text: &str) -> Vec<MappedChar> {
    let chars: Vec<(usize, char)> = text
        .chars()
        .enumerate()
        .filter(|(_, ch)| *ch != '\r')
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    for i in 0..=chars.len() {
        if i == chars.len() || chars[i].1 == '\n' {
            out.extend(polish_line(&chars[start..i]));
            if i < chars.len() {
                out.push(MappedChar {
                    ch: '\n',
                    src: chars[i].0,
                });
            }
            start = i + 1;
        }
    }
    out
}

fn polish_line(line: &[(usize, char)]) -> Vec<MappedChar> {
    if !needs_polish(line.iter().map(|(_, ch)| *ch)) {
        return line
            .iter()
            .map(|&(src, ch)| MappedChar { ch, src })
            .collect();
    }
    let folded: Vec<MappedChar> = line
        .iter()
        .map(|&(src, ch)| MappedChar {
            ch: fold_width(ch),
            src,
        })
        .collect();
    let punct = apply_punct(&folded);
    let spaced = insert_cjk_gaps(&punct);
    let collapsed = collapse_spaces(spaced);
    // 数字、英文旁边已经是标点时，不再留空格。`话题：2026` 的冒号不是「内容」。
    let stuck = drop_spaces_beside_punct(collapsed);
    let trimmed = trim_spaces(stuck);
    let tight = tighten_quotes(trimmed);
    strip_trailing_enumeration(tight)
}

/// 这一句要不要走中文成稿规则。纯拉丁文本返回 false。
fn needs_polish(chars: impl Iterator<Item = char>) -> bool {
    chars.into_iter().any(|ch| {
        is_no_space_script(ch)
            || is_cjk_punct(ch)
            || is_fullwidth_ascii(ch)
            || matches!(ch, '\u{3000}')
    })
}

fn is_cjk_punct(ch: char) -> bool {
    matches!(
        ch,
        '。' | '，'
            | '、'
            | '？'
            | '！'
            | '：'
            | '；'
            | '…'
            | '⋯'
            | '‥'
            | '．'
            | '﹒'
            | '「'
            | '」'
            | '『'
            | '』'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '（'
            | '）'
    )
}

fn is_fullwidth_ascii(ch: char) -> bool {
    matches!(ch, '\u{FF01}'..='\u{FF5E}')
}

fn fold_width(ch: char) -> char {
    match ch {
        '０'..='９' => fold_fullwidth(ch, '０', '0'),
        'Ａ'..='Ｚ' => fold_fullwidth(ch, 'Ａ', 'A'),
        'ａ'..='ｚ' => fold_fullwidth(ch, 'ａ', 'a'),
        '\u{3000}' | '\u{00A0}' => ' ',
        '＂' => '"',
        '＇' => '\'',
        _ => ch,
    }
}

/// 全角 ASCII 与半角一一对应。对不齐时留下原字符，不在成稿路径上 panic。
fn fold_fullwidth(ch: char, wide_zero: char, ascii_zero: char) -> char {
    char::from_u32(u32::from(ascii_zero) + (u32::from(ch) - u32::from(wide_zero))).unwrap_or(ch)
}

fn apply_punct(input: &[MappedChar]) -> Vec<MappedChar> {
    let mut out = Vec::new();
    let mut i = 0;
    let mut double_open = false;
    let mut single_open = false;
    while i < input.len() {
        let ch = input[i].ch;
        if is_dotish(ch) {
            let start = i;
            i += 1;
            while i < input.len() && is_dotish(input[i].ch) {
                i += 1;
            }
            out.push(map_dot_run(input, start, i));
            continue;
        }
        if is_sentence_mark(ch) {
            // 网址查询串里的 `?` 必须保持半角，否则 `a.co?x=1` 就不再是链接。
            if matches!(ch, '?' | '？') && is_url_query(input, i) {
                out.push(MappedChar {
                    ch: '?',
                    src: input[i].src,
                });
                i += 1;
                continue;
            }
            let start = i;
            let mut any_question = false;
            while i < input.len() && is_sentence_mark(input[i].ch) {
                any_question |= matches!(input[i].ch, '?' | '？');
                i += 1;
            }
            out.push(MappedChar {
                ch: if any_question { '？' } else { '！' },
                src: input[start].src,
            });
            continue;
        }
        let src = input[i].src;
        let mapped = match ch {
            // 千分位不论全角半角都留下，并收成半角逗号。句中的逗号才换成空格。
            '，' | ',' if both_neighbors_digits(input, i) => Some(','),
            '，' | ',' => Some(' '),
            // 时刻保持半角冒号。网址、盘符（一边是字母或 `/`）也不改成全角，
            // 否则 `https://` 会被拆开。其余冒号按简体字幕收成全角。
            ':' | '：' if keep_colon(input, i) => Some(':'),
            ':' | '：' => Some('：'),
            '"' => {
                double_open = !double_open;
                Some(if double_open { '“' } else { '”' })
            }
            '“' => {
                double_open = true;
                Some('“')
            }
            '”' => {
                double_open = false;
                Some('”')
            }
            '\'' if is_apostrophe(input, i) => Some('\''),
            '\'' => {
                single_open = !single_open;
                Some(if single_open { '‘' } else { '’' })
            }
            '‘' => {
                single_open = true;
                Some('‘')
            }
            '’' => {
                single_open = false;
                Some('’')
            }
            _ => None,
        };
        if let Some(ch) = mapped {
            out.push(MappedChar { ch, src });
        } else {
            out.push(input[i]);
        }
        i += 1;
    }
    out
}

fn is_dotish(ch: char) -> bool {
    matches!(ch, '.' | '。' | '．' | '﹒' | '…' | '⋯' | '‥')
}

fn map_dot_run(input: &[MappedChar], start: usize, end: usize) -> MappedChar {
    let src = input[start].src;
    let run = &input[start..end];
    if run.len() == 1 {
        let ch = run[0].ch;
        if matches!(ch, '…' | '⋯' | '‥') {
            return MappedChar { ch: '…', src };
        }
        if both_neighbors_digits(input, start) {
            return MappedChar { ch: '.', src };
        }
        // `U.S.` / `Mr.` / `café.`：点的左边是拉丁字母就留下，后面若紧挨汉字再由空格规则补空格。
        if matches!(ch, '.' | '．') && neighbor_char(input, start, -1).is_some_and(is_latin_letter)
        {
            return MappedChar { ch: '.', src };
        }
        return MappedChar { ch: ' ', src };
    }
    MappedChar { ch: '…', src }
}

fn both_neighbors_digits(input: &[MappedChar], index: usize) -> bool {
    neighbor_char(input, index, -1).is_some_and(|c| c.is_ascii_digit())
        && neighbor_char(input, index, 1).is_some_and(|c| c.is_ascii_digit())
}

/// 时刻，或者冒号贴着字母、斜杠、另一个冒号（网址、盘符）。
fn keep_colon(input: &[MappedChar], index: usize) -> bool {
    if both_neighbors_digits(input, index) {
        return true;
    }
    let sticks = |ch: char| ch == '/' || ch == ':' || ch == '：' || is_latin_letter(ch);
    neighbor_char(input, index, -1).is_some_and(sticks)
        || neighbor_char(input, index, 1).is_some_and(sticks)
}

fn neighbor_char(input: &[MappedChar], index: usize, delta: isize) -> Option<char> {
    let pos = index as isize + delta;
    if pos < 0 {
        return None;
    }
    input.get(pos as usize).map(|m| m.ch)
}

fn is_sentence_mark(ch: char) -> bool {
    matches!(ch, '?' | '!' | '？' | '！')
}

/// `a.co?x=1`：问号前的同一段里有 `.` 或 `/`，后面接着字母、数字或 `=`。
fn is_url_query(input: &[MappedChar], index: usize) -> bool {
    let next_ok = neighbor_char(input, index, 1)
        .is_some_and(|ch| is_latin_letter(ch) || ch.is_ascii_digit() || ch == '=');
    if !next_ok {
        return false;
    }
    let mut j = index;
    while j > 0 {
        let ch = input[j - 1].ch;
        if ch.is_whitespace() || is_no_space_script(ch) {
            break;
        }
        if ch == '.' || ch == '/' {
            return true;
        }
        j -= 1;
    }
    false
}

/// 拉丁字母，含重音。不用 `is_alphabetic`：汉字在 Unicode 里也是字母。
fn is_latin_letter(ch: char) -> bool {
    if is_no_space_script(ch) {
        return false;
    }
    ch.is_ascii_alphabetic()
        || matches!(
            ch,
            '\u{00C0}'..='\u{00D6}'
                | '\u{00D8}'..='\u{00F6}'
                | '\u{00F8}'..='\u{00FF}'
                | '\u{0100}'..='\u{024F}'
        )
}

fn is_apostrophe(input: &[MappedChar], index: usize) -> bool {
    let prev = neighbor_char(input, index, -1);
    let next = neighbor_char(input, index, 1);
    let prev_letter = prev.is_some_and(is_latin_letter);
    let next_letter = next.is_some_and(is_latin_letter);
    let prev_digit = prev.is_some_and(|ch| ch.is_ascii_digit());
    let next_digit = next.is_some_and(|ch| ch.is_ascii_digit());
    // `it's`、`café's`、`90's`：两侧都是字母或数字，且至少有一个字母。
    if (prev_letter || prev_digit) && (next_letter || next_digit) && (prev_letter || next_letter) {
        return true;
    }
    // 词尾所有格 `students'`，以及年份省略 `'99`。
    if prev_letter && next.is_none_or(|ch| ch.is_whitespace() || is_no_space_script(ch)) {
        return true;
    }
    next_digit && prev.is_none_or(|ch| !is_latin_letter(ch))
}

/// 汉字 / 假名和英文单词、阿拉伯数字之间补一个空格。字母与数字之间不补。
fn insert_cjk_gaps(input: &[MappedChar]) -> Vec<MappedChar> {
    let mut out = Vec::new();
    for (i, item) in input.iter().enumerate() {
        if i > 0 && needs_gap(input[i - 1].ch, item.ch) {
            out.push(MappedChar {
                ch: ' ',
                src: item.src,
            });
        }
        out.push(*item);
    }
    out
}

fn needs_gap(left: char, right: char) -> bool {
    if left.is_whitespace() || right.is_whitespace() {
        return false;
    }
    let left_cjk = is_no_space_script(left);
    let right_cjk = is_no_space_script(right);
    let left_west = is_western(left);
    let right_west = is_western(right);
    (left_cjk && right_west)
        || (left_west && right_cjk)
        || (left == '.' && right_cjk)
        || (right == '.' && left_cjk)
        || (left == '\'' && right_cjk)
}

fn is_western(ch: char) -> bool {
    is_latin_letter(ch) || ch.is_ascii_digit() || ch == '%'
}

/// 空格的一侧是标点、另一侧是英文或数字时，这个空格不该出现。
/// 汉字和数字之间的空格两侧都是内容，留着。
fn drop_spaces_beside_punct(input: Vec<MappedChar>) -> Vec<MappedChar> {
    let mut out = Vec::new();
    for (i, item) in input.iter().enumerate() {
        if item.ch == ' ' {
            let prev = out.last().map(|m: &MappedChar| m.ch);
            let next = input.get(i + 1).map(|m| m.ch);
            let punct_then_word = prev.is_some_and(is_sticky_punct) && next.is_some_and(is_western);
            let word_then_punct = prev.is_some_and(is_western) && next.is_some_and(is_sticky_punct);
            if punct_then_word || word_then_punct {
                continue;
            }
        }
        out.push(*item);
    }
    out
}

fn is_sticky_punct(ch: char) -> bool {
    matches!(
        ch,
        '：' | ':'
            | '；'
            | ';'
            | '、'
            | '？'
            | '！'
            | '?'
            | '!'
            | '…'
            | '“'
            | '”'
            | '‘'
            | '’'
            | '"'
            | '\''
            | '（'
            | '）'
            | '('
            | ')'
            | '「'
            | '」'
            | '『'
            | '』'
            | '《'
            | '》'
            | '【'
            | '】'
    )
}

fn collapse_spaces(input: Vec<MappedChar>) -> Vec<MappedChar> {
    let mut out = Vec::new();
    for item in input {
        if item.ch == ' ' && out.last().is_some_and(|prev: &MappedChar| prev.ch == ' ') {
            continue;
        }
        out.push(item);
    }
    out
}

fn trim_spaces(mut input: Vec<MappedChar>) -> Vec<MappedChar> {
    while input.first().is_some_and(|item| item.ch == ' ') {
        input.remove(0);
    }
    while input.last().is_some_and(|item| item.ch == ' ') {
        input.pop();
    }
    input
}

fn tighten_quotes(input: Vec<MappedChar>) -> Vec<MappedChar> {
    let mut out = Vec::new();
    for (i, item) in input.iter().enumerate() {
        if item.ch == ' ' {
            let prev = out.last().map(|m: &MappedChar| m.ch);
            let next = input.get(i + 1).map(|m| m.ch);
            if prev.is_some_and(is_open_quote) || next.is_some_and(is_close_quote) {
                continue;
            }
            if prev == Some('：') && next.is_some_and(is_open_quote) {
                continue;
            }
        }
        out.push(*item);
    }
    out
}

fn is_open_quote(ch: char) -> bool {
    matches!(ch, '“' | '‘')
}

fn is_close_quote(ch: char) -> bool {
    matches!(ch, '”' | '’')
}

fn strip_trailing_enumeration(mut input: Vec<MappedChar>) -> Vec<MappedChar> {
    loop {
        while input.last().is_some_and(|item| item.ch == ' ') {
            input.pop();
        }
        let mut i = input.len();
        while i > 0 && is_closer(input[i - 1].ch) {
            i -= 1;
        }
        if i > 0 && input[i - 1].ch == '、' {
            input.remove(i - 1);
            continue;
        }
        break;
    }
    input
}

fn is_closer(ch: char) -> bool {
    matches!(ch, '”' | '’' | '）' | ')' | '】' | '」' | '』' | '》')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polish(text: &str) -> String {
        polish_cue_text(text)
    }

    #[test]
    fn english_only_cues_stay_byte_for_byte() {
        for text in ["Hello, world.", "It's 3.14.", "U.S.A. 2024", "OK!"] {
            assert_eq!(polish(text), text, "{text}");
        }
    }

    #[test]
    fn chinese_periods_and_commas_become_one_space() {
        assert_eq!(polish("你好，世界。"), "你好 世界");
        assert_eq!(polish("你好,世界."), "你好 世界");
        assert_eq!(polish("结束。"), "结束");
        assert_eq!(polish("第一行。\n第二行，还行"), "第一行\n第二行 还行");
    }

    #[test]
    fn numbers_and_abbreviations_keep_their_internal_marks() {
        assert_eq!(polish("售价3.14元。"), "售价 3.14 元");
        assert_eq!(polish("共1,500人"), "共 1,500 人");
        assert_eq!(polish("现在4:12分"), "现在 4:12 分");
        assert_eq!(polish("采用U.S.标准"), "采用 U.S. 标准");
        assert_eq!(polish("共１，５００人"), "共 1,500 人");
        assert_eq!(polish("见https://a.co。"), "见 https://a.co");
        assert_eq!(polish("见https://a.co?x=1。"), "见 https://a.co?x=1");
        assert_eq!(polish("日期:1976年"), "日期：1976 年");
        // 冒号、问号是标点，不给后面的数字或英文补空格。汉字那边仍然补。
        assert_eq!(polish("话题：2026年刚刚开始"), "话题：2026 年刚刚开始");
        assert_eq!(polish("话题： 2026年"), "话题：2026 年");
        assert_eq!(polish("大家好 2026 年"), "大家好 2026 年");
        assert_eq!(polish("吗？OK"), "吗？OK");
    }

    #[test]
    fn english_words_and_digits_gain_a_space_on_each_side_that_has_chinese() {
        assert_eq!(polish("使用Whisper模型"), "使用 Whisper 模型");
        assert_eq!(polish("共123人"), "共 123 人");
        assert_eq!(polish("第3章"), "第 3 章");
        assert_eq!(polish("123人"), "123 人");
        assert_eq!(polish("共123"), "共 123");
        // 字母和数字贴在一起时不拆开，空格加在整段和汉字之间。
        assert_eq!(polish("MP3播放"), "MP3 播放");
        assert_eq!(polish("已经是Whisper 模型"), "已经是 Whisper 模型");
        assert_eq!(polish("café模型"), "café 模型");
        assert_eq!(polish("使用état"), "使用 état");
        assert_eq!(polish("90's歌曲"), "90's 歌曲");
        assert_eq!(polish("回到'99年"), "回到'99 年");
    }

    #[test]
    fn fullwidth_letters_and_digits_fold_then_space() {
        assert_eq!(polish("共１２３人"), "共 123 人");
        assert_eq!(polish("用Ｗｈｉｓｐｅｒ模型"), "用 Whisper 模型");
    }

    #[test]
    fn marks_ellipsis_quotes_and_trailing_enumeration() {
        assert_eq!(polish("什么?!"), "什么？");
        assert_eq!(polish("什么??"), "什么？");
        assert_eq!(polish("真的!!"), "真的！");
        assert_eq!(polish("等等..."), "等等…");
        assert_eq!(polish("等等……"), "等等…");
        assert_eq!(polish("苹果、香蕉、"), "苹果、香蕉");
        assert_eq!(polish("我问:\"你有什么?\""), "我问：“你有什么？”");
        assert_eq!(polish("“ 你好 ”"), "“你好”");
    }

    #[test]
    fn polishing_twice_changes_nothing() {
        for text in [
            "你好，世界。",
            "使用Whisper模型，共123人。",
            "Hello, world.",
            "第一行。\n第二行",
            "售价3.14元。",
        ] {
            let once = polish(text);
            assert_eq!(polish(&once), once, "{text}");
        }
    }

    #[test]
    fn inserted_spaces_point_at_the_following_character() {
        let mapped = polish_mapped("共123人");
        let text: String = mapped.iter().map(|m| m.ch).collect();
        assert_eq!(text, "共 123 人");
        let space = mapped.iter().find(|m| m.ch == ' ').expect("space");
        // 「共123人」里空格插在「1」前面，src 指向「1」，扫光才不会被切开。
        assert_eq!(space.src, 1);
    }
}
