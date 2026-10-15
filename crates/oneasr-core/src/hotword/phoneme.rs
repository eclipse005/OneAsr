//! 音素处理：文本 → 带语言/边界/位置属性的音素序列。
//!
//! 对齐 Python 参考实现（上游 `asr-hotword`）的 `get_phoneme_info` 路径
//! （生产链路）。`get_phoneme_seq` / `normalize_text` 等**非热路径**函数不在此
//! 实现：热词表只写正确写法，纠错后处理那半边（模糊音权重编辑距离、黑名单、
//! 文本替换）在这里没有消费者——取舍见 [`crate::hotword`] 的模块头。
//!
//! **索引语义**：本模块所有 `char_start` / `char_end` 均为 **字符（Unicode 码点）
//! 索引**，与 Python `str` 的切片语义一致，而非 Rust `&str` 的字节偏移。

use std::borrow::Cow;

use pinyin::ToPinyin;
use unicode_general_category::{GeneralCategory, get_general_category};

/// 音素的语言类型，对应 Python 的 `Literal['zh', 'en', 'num', 'other']`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Lang {
    Zh,
    En,
    Num,
    Other,
}

impl Lang {
    /// 对应 Python 中 `lang` 字段的字符串值。
    pub fn as_str(self) -> &'static str {
        match self {
            Lang::Zh => "zh",
            Lang::En => "en",
            Lang::Num => "num",
            Lang::Other => "other",
        }
    }
}

/// 带语言属性的音素，对应 Python 的 `@dataclass(frozen=True) Phoneme`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phoneme {
    /// 音素值（如 `'b'`、`'ing'`、`'python'`、`'4'`）。
    pub value: String,
    pub lang: Lang,
    /// 是否是字边界起始位置（声母 / 零声母）。
    pub is_word_start: bool,
    /// 是否是字边界结束位置（声调）。
    pub is_word_end: bool,
    /// 该音素在原文本中的起始 **字符** 索引。
    pub char_start: usize,
    /// 该音素在原文本中的结束 **字符** 索引（不含）。
    pub char_end: usize,
}

impl Phoneme {
    pub fn new(
        value: impl Into<String>,
        lang: Lang,
        is_word_start: bool,
        is_word_end: bool,
        char_start: usize,
        char_end: usize,
    ) -> Self {
        Self {
            value: value.into(),
            lang,
            is_word_start,
            is_word_end,
            char_start,
            char_end,
        }
    }

    /// 是否是声调，对应 Python 的 `is_tone` 属性。
    ///
    /// Python 用 `value.isdigit()`。本项目音素值中仅声调为数字，且 `is_tone`
    /// 只在 `lang == zh` 的分支被消费，故 ASCII 判定与 Python 等价。
    pub fn is_tone(&self) -> bool {
        !self.value.is_empty() && self.value.bytes().all(|b| b.is_ascii_digit())
    }

    /// 对应 Python `Phoneme.info` 的前 5 元组：
    /// `(value, lang, is_word_start, is_word_end, is_tone)`。
    pub fn info5(&self) -> PhonemeInfo5<'_> {
        PhonemeInfo5 {
            value: &self.value,
            lang: self.lang,
            is_word_start: self.is_word_start,
            is_word_end: self.is_word_end,
            is_tone: self.is_tone(),
        }
    }
}

/// `Phoneme.info` 的 5 元组视图（借用版，避免热路径反复分配）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhonemeInfo5<'a> {
    pub value: &'a str,
    pub lang: Lang,
    pub is_word_start: bool,
    pub is_word_end: bool,
    pub is_tone: bool,
}

/// `Phoneme.info` 的完整 7 元组视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhonemeInfo7<'a> {
    pub value: &'a str,
    pub lang: Lang,
    pub is_word_start: bool,
    pub is_word_end: bool,
    pub is_tone: bool,
    pub char_start: usize,
    pub char_end: usize,
}

impl Phoneme {
    pub fn info7(&self) -> PhonemeInfo7<'_> {
        PhonemeInfo7 {
            value: &self.value,
            lang: self.lang,
            is_word_start: self.is_word_start,
            is_word_end: self.is_word_end,
            is_tone: self.is_tone(),
            char_start: self.char_start,
            char_end: self.char_end,
        }
    }
}

// ---------------------------------------------------------------- 拼音切分

/// 声母表（**最长匹配优先**，故 zh/ch/sh 必须排在 z/c/s 之前）。
///
/// pypinyin 把 `y` `w` 也当作声母（`用` → ini=`y` fin=`ong`），零声母音节
/// （`儿` `啊` `哦`）匹配不到任何声母，得到 `("", 完整音节)`。
const INITIALS: &[&str] = &[
    "zh", "ch", "sh", "b", "p", "m", "f", "d", "t", "n", "l", "g", "k", "h", "j", "q", "x", "r",
    "z", "c", "s", "y", "w",
];

/// pypinyin 非严格模式用 `v` 表示 `ü`，`pinyin` crate 用 `ü`。
///
/// `ü` 不会出现在声母里，所以先切分、后替换即可，且常见情况下零分配。
fn replace_uv(s: &str) -> Cow<'_, str> {
    if s.contains('ü') {
        Cow::Owned(s.replace('ü', "v"))
    } else {
        Cow::Borrowed(s)
    }
}

/// 将无调音节切分为 `(声母, 韵母)`。
///
/// 声母为 `&'static str`（取自 [`INITIALS`] 表），韵母在无 `ü` 时借用入参。
///
/// 实测：对 hot.txt 全部 1444 个汉字，与 pypinyin 的
/// `Style.INITIALS(strict=False)` / `Style.FINALS(strict=False)` **零差异**。
pub fn split_pinyin(plain: &str) -> (&'static str, Cow<'_, str>) {
    for ini in INITIALS {
        if let Some(rest) = plain.strip_prefix(ini) {
            return (ini, replace_uv(rest));
        }
    }
    ("", replace_uv(plain))
}

/// 单个汉字的声母 / 韵母 / 声调三元组（中性声调为 `'5'`）。
///
/// 对应 pypinyin 的 `INITIALS(strict=False)`、`FINALS(strict=False)`、
/// `TONE3(neutral_tone_with_five=True)`。无拼音时返回 `None`。
pub fn char_to_phonemes(c: char) -> Option<(String, String, char)> {
    let p = c.to_pinyin()?;
    let (ini, fin) = split_pinyin(p.plain());
    // `with_tone_num_end` 无数字表示轻声；pypinyin 在
    // neutral_tone_with_five=True 时给 '5'，故此处映射为 '5'。
    let tone = p
        .with_tone_num_end()
        .chars()
        .last()
        .filter(|ch| ch.is_ascii_digit())
        .unwrap_or('5');
    Some((ini.to_string(), fin.into_owned(), tone))
}

// ---------------------------------------------------------------- 带调音节解析

/// 单码点的带调符号 → `(基准字母, 声调)`。对应 pypinyin `phonetic_symbol.py`。
///
/// `ḿ`(U+1E3F) 是单码点，而 `m̄`/`m̀`/`ê̄`/`ê̌` 等是两个码点，另见 [`TONE_TWO`]。
const TONE_ONE: &[(char, &str, u8)] = &[
    ('ā', "a", 1),
    ('á', "a", 2),
    ('ǎ', "a", 3),
    ('à', "a", 4),
    ('ē', "e", 1),
    ('é', "e", 2),
    ('ě', "e", 3),
    ('è', "e", 4),
    ('ō', "o", 1),
    ('ó', "o", 2),
    ('ǒ', "o", 3),
    ('ò', "o", 4),
    ('ī', "i", 1),
    ('í', "i", 2),
    ('ǐ', "i", 3),
    ('ì', "i", 4),
    ('ū', "u", 1),
    ('ú', "u", 2),
    ('ǔ', "u", 3),
    ('ù', "u", 4),
    // üe：pypinyin 非严格模式用 v 表示 ü
    ('ü', "v", 0),
    ('ǖ', "v", 1),
    ('ǘ', "v", 2),
    ('ǚ', "v", 3),
    ('ǜ', "v", 4),
    ('ń', "n", 2),
    ('ň', "n", 3),
    ('ǹ', "n", 4),
    ('ḿ', "m", 2),
];

/// 两码点的带调符号 `(基字符, 组合符)` → `(基准字母, 声调)`。
const TONE_TWO: &[((char, char), &str, u8)] = &[
    (('m', '\u{304}'), "m", 1), // m̄
    (('m', '\u{300}'), "m", 4), // m̀
    (('ê', '\u{304}'), "ê", 1), // ê̄
    (('ê', '\u{301}'), "ê", 2), // ế
    (('ê', '\u{30c}'), "ê", 3), // ê̌
    (('ê', '\u{300}'), "ê", 4), // ề
];

/// 把一个**带调**音节（如 `háng`）拆成 `(无声调音节, 声调字符)`。
///
/// 对应 pypinyin 的 `replace_symbol_to_number` + `RE_NUMBER.sub('')`，
/// 声调取 `TONE3(neutral_tone_with_five=True)` 的末位数字；无调号即轻声 `'5'`。
pub fn syllable_to_phonemes(s: &str) -> (String, char) {
    let mut plain = String::with_capacity(s.len());
    let mut tone = '5';
    let mut it = s.chars().peekable();

    while let Some(c) = it.next() {
        // 先试两码点组合（`m̄` / `ê̌` 之类），单码点分支把它们拆开了
        if let Some(&next) = it.peek()
            && let Some(&(_, base, t)) =
                TONE_TWO.iter().find(|((a, b), _, _)| *a == c && *b == next)
        {
            plain.push_str(base);
            if t != 0 {
                tone = char::from_digit(t as u32, 10).unwrap_or('5');
            }
            it.next();
            continue;
        }
        match TONE_ONE.iter().find(|(k, _, _)| *k == c) {
            Some(&(_, base, t)) => {
                plain.push_str(base);
                if t != 0 {
                    tone = char::from_digit(t as u32, 10).unwrap_or('5');
                }
            }
            None => plain.push(c),
        }
    }

    (plain, tone)
}

// ---------------------------------------------------------------- 字符分类

/// 是否属于 CJK 统一表意文字（对应 Python 的 `'\u4e00' <= c <= '\u9fff'`）。
#[inline]
fn is_cjk(c: char) -> bool {
    ('\u{4e00}'..='\u{9fff}').contains(&c)
}

/// 复刻 Python `'a' <= c.lower() <= 'z'` 的判定。
///
/// 用完整的 Unicode 小写映射（而非 `to_ascii_lowercase`），以覆盖 U+212A
/// KELVIN SIGN 等 `.lower()` 后落入 a-z 的字符。
#[inline]
fn is_ascii_alpha(c: char) -> bool {
    let mut it = c.to_lowercase();
    match (it.next(), it.next()) {
        (Some(x), None) => x.is_ascii_lowercase(),
        _ => false,
    }
}

/// 对应 `unicodedata.category(c).startswith('L')`。
#[inline]
fn is_unicode_letter(c: char) -> bool {
    matches!(
        get_general_category(c),
        GeneralCategory::UppercaseLetter
            | GeneralCategory::LowercaseLetter
            | GeneralCategory::TitlecaseLetter
            | GeneralCategory::ModifierLetter
            | GeneralCategory::OtherLetter
    )
}

// ---------------------------------------------------------------- 主流程

/// 将文本转换为带位置信息的音素序列（生产链路入口）。
///
/// 对应 Python `get_phoneme_info(text, ascii_split_char=True)`。
pub fn get_phoneme_info(text: &str, ascii_split_char: bool) -> Vec<Phoneme> {
    let chars: Vec<char> = text.chars().collect();
    let mut seq: Vec<Phoneme> = Vec::with_capacity(chars.len() * 2);
    let mut pos = 0usize;

    while pos < chars.len() {
        let c = chars[pos];

        if is_cjk(c) {
            pos = process_zh(&chars, pos, &mut seq);
        } else if is_ascii_alpha(c) || c.is_ascii_digit() {
            pos = process_en_num(&chars, pos, &mut seq, ascii_split_char);
        } else {
            // 非中文、非 ASCII 的字母（希腊字母等）保留为独立音素；
            // 标点 (P*)、分隔符 (Z*)、符号 (Sm/So) 等跳过。
            if is_unicode_letter(c) {
                let lower: String = c.to_lowercase().collect();
                seq.push(Phoneme::new(lower, Lang::Other, true, true, pos, pos + 1));
            }
            pos += 1;
        }
    }

    seq
}

/// 处理连续的中文片段，返回扫描后的新位置。
///
/// 每个汉字拆为 `[声母, 韵母, 声调]` 三元组（声母可缺省）。
///
/// 读音来源与 Python `_process_zh` 完全一致：它把整段中文交给
/// `pypinyin.pinyin()`，后者先按 [`crate::hotword::phrases`] 做最大正向分词，
/// 命中词条的字符用**词组读音**（多音字消歧就在这一步），未命中的退回逐字读音。
fn process_zh(chars: &[char], pos: usize, seq: &mut Vec<Phoneme>) -> usize {
    let mut scan = pos + 1;
    while scan < chars.len() && is_cjk(chars[scan]) {
        scan += 1;
    }
    let zh_end = scan;
    let frag = &chars[pos..zh_end];

    for (ws, we) in cut_zh(frag) {
        let word: String = frag[ws..we].iter().collect();
        let idx0 = pos + ws;

        match phrase_syllables(&word) {
            Some(syllables) => {
                // 词组读音：每个字都取词条里烘焙好的带调音节
                for (k, syl) in syllables.iter().enumerate() {
                    let idx = idx0 + k;
                    if idx >= pos + we {
                        break; // 资产异常时防御，不让索引越界
                    }
                    let (plain, tone) = syllable_to_phonemes(syl);
                    let (ini, fin) = split_pinyin(&plain);
                    emit_zh(seq, idx, ini, fin.as_ref(), tone);
                }
            }
            None => {
                for (k, c) in word.chars().enumerate() {
                    match char_to_phonemes(c) {
                        Some((ini, fin, tone)) => {
                            emit_zh(seq, idx0 + k, &ini, &fin, tone);
                        }
                        // 拼音表中无此字：退化为整字音素（对应 Python 的 `if not items` 兜底）
                        None => seq.push(Phoneme::new(
                            c.to_string(),
                            Lang::Zh,
                            true,
                            true,
                            idx0 + k,
                            idx0 + k + 1,
                        )),
                    }
                }
            }
        }
    }

    zh_end
}

/// 中文片段的最大正向分词（短语词典 + 最大正向匹配）。
fn cut_zh(frag: &[char]) -> Vec<(usize, usize)> {
    crate::hotword::phrases::dict().cut(frag)
}

/// 词组读音查询：命中短语词典就用词条读音（多音字消歧），否则退回逐字。
fn phrase_syllables(word: &str) -> Option<Vec<&'static str>> {
    crate::hotword::phrases::dict()
        .get(word)
        .map(|it| it.collect())
}

/// 把一个汉字的 `(声母, 韵母, 声调)` 展开成音素序列。
#[inline]
fn emit_zh(seq: &mut Vec<Phoneme>, idx: usize, ini: &str, fin: &str, tone: char) {
    let has_initial = !ini.is_empty();
    if has_initial {
        seq.push(Phoneme::new(ini, Lang::Zh, true, false, idx, idx + 1));
    }
    if !fin.is_empty() {
        // Python: is_word_start = not init —— 有声母时字首在声母上，
        // 零声母时（儿/啊/哦）字首落到韵母上。
        seq.push(Phoneme::new(
            fin,
            Lang::Zh,
            !has_initial,
            false,
            idx,
            idx + 1,
        ));
    }
    seq.push(Phoneme::new(
        tone.to_string(),
        Lang::Zh,
        false,
        true,
        idx,
        idx + 1,
    ));
}

// ---------------------------------------------------------------- 英文音素化

/// 26 个英文字母的「字母名」音素（ARPAbet 风格），按下标 `a..=z` 索引。
///
/// 英文缩写的识别错误，本质是**字母名被听错**，而非词内音素混淆：
/// ```text
/// USB    说话人念 "U-S-B"          -> y uw eh s b iy
/// USBB   ASR 听成 "U-S-B-B"        -> y uw eh s b iy b iy
/// ```
/// 这张表是英文字母的客观读音，不是从语料统计出来的「哪些字母会听错」。
/// 因此换语料不需要重做，且不会向提示词注入任何东西。
pub const EN_LETTER_TABLE: [&[&str]; 26] = [
    &["ey"],                      // a
    &["b", "iy"],                 // b
    &["s", "iy"],                 // c
    &["d", "iy"],                 // d
    &["iy"],                      // e
    &["eh", "f"],                 // f
    &["jh", "iy"],                // g
    &["ey", "ch"],                // h
    &["ay"],                      // i
    &["jh", "ey"],                // j
    &["k", "ey"],                 // k
    &["eh", "l"],                 // l
    &["eh", "m"],                 // m
    &["eh", "n"],                 // n
    &["ow"],                      // o
    &["p", "iy"],                 // p
    &["k", "y", "uw"],            // q
    &["aa", "r"],                 // r
    &["eh", "s"],                 // s
    &["t", "iy"],                 // t
    &["y", "uw"],                 // u
    &["v", "iy"],                 // v
    &["d", "ah", "b", "y", "uw"], // w
    &["eh", "k", "s"],            // x
    &["w", "ay"],                 // y
    &["z", "iy"],                 // z
];

/// 构词符号的读音。
///
/// 默认分词器会把 `P*` / `Sm` / `So` 等类别全部跳过，导致带符号的英文热词坍缩：
/// `A+` 只剩一个音素 `a`，于是匹配英文里每一个独立冠词——实测在 4 段真实
/// 转录上造成 261 处误伤。符号本身有标准英文读音，纳入音素序列即可。
pub const EN_SYMBOL_PHONEMES: &[(char, &[&str])] = &[
    ('+', &["p", "l", "ah", "s"]),  // plus
    ('#', &["sh", "aa", "r", "p"]), // sharp
    ('&', &["ae", "n", "d"]),       // and
];

/// 该字符是否为可吸收的构词符号。
#[inline]
fn en_symbol(c: char) -> Option<&'static [&'static str]> {
    EN_SYMBOL_PHONEMES
        .iter()
        .find(|(k, _)| *k == c)
        .map(|(_, v)| *v)
}

/// 单个字符展开出的音素数量。
#[inline]
fn en_char_phoneme_count(c: char) -> usize {
    let lower = c.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        EN_LETTER_TABLE[lower as usize - b'a' as usize].len()
    } else if let Some(s) = en_symbol(c) {
        s.len()
    } else {
        1
    }
}

/// 单个字符的音素序列展开；数字等无读音表的字符原样保留。
fn emit_en_char(
    seq: &mut Vec<Phoneme>,
    c: char,
    lang: Lang,
    word_start: bool,
    word_end: bool,
    char_idx: usize,
) {
    let lower = c.to_ascii_lowercase();
    let items: &[&str] = if lower.is_ascii_lowercase() {
        EN_LETTER_TABLE[lower as usize - b'a' as usize]
    } else if let Some(s) = en_symbol(c) {
        s
    } else {
        // 数字等非字母字符保持原样
        seq.push(Phoneme::new(
            c.to_string(),
            lang,
            word_start,
            word_end,
            char_idx,
            char_idx + 1,
        ));
        return;
    };

    let n = items.len();
    for (k, ph) in items.iter().enumerate() {
        seq.push(Phoneme::new(
            *ph,
            lang,
            word_start && k == 0,
            word_end && k == n - 1,
            char_idx,
            char_idx + 1,
        ));
    }
}

/// 处理英文/数字片段，支持驼峰与字母/数字边界拆分；英文按字母名展开为音素。
fn process_en_num(chars: &[char], pos: usize, seq: &mut Vec<Phoneme>, split_char: bool) -> usize {
    let start_pos = pos;
    let mut p = pos;
    let mut has_letter = false;

    while p < chars.len() {
        let c = chars[p];
        if is_ascii_alpha(c) || c.is_ascii_digit() {
            has_letter = has_letter || is_ascii_alpha(c);
            if p > start_pos {
                let prev = chars[p - 1];
                // 驼峰 aA / 字母数字 a1 / 数字字母 1a 三类边界
                if (prev.is_lowercase() && c.is_uppercase())
                    || (prev.is_alphabetic() && c.is_ascii_digit())
                    || (prev.is_ascii_digit() && c.is_alphabetic())
                {
                    break;
                }
            }
            p += 1;
        } else if en_symbol(c).is_some() && p > start_pos && has_letter {
            // 仅吸收紧跟在**字母**后的构词符号。
            // 要求已有字母，是为了不把 "1+1" 这类纯数字算式改成英文 token。
            p += 1;
        } else {
            break;
        }
    }

    let end_pos = p;
    let token: String = chars[start_pos..end_pos]
        .iter()
        .flat_map(|c| c.to_lowercase())
        .collect();
    let lang = if !token.is_empty() && token.bytes().all(|b| b.is_ascii_digit()) {
        Lang::Num
    } else {
        Lang::En
    };

    if split_char {
        let t: Vec<char> = token.chars().collect();
        if lang == Lang::En {
            // 展开出的多个音素共享同一来源字符；字边界只打在组首/组尾，
            // 否则 DP 的边界约束会失效并产生跨词误命中。
            let total: usize = t.iter().map(|&c| en_char_phoneme_count(c)).sum();
            let mut i = 0usize;
            for (off, &c) in t.iter().enumerate() {
                let cnt = en_char_phoneme_count(c);
                emit_en_char(seq, c, lang, i == 0, i + cnt == total, start_pos + off);
                i += cnt;
            }
        } else {
            let n = t.len();
            for (i, &c) in t.iter().enumerate() {
                seq.push(Phoneme::new(
                    c.to_string(),
                    lang,
                    i == 0,
                    i == n - 1,
                    start_pos + i,
                    start_pos + i + 1,
                ));
            }
        }
    } else {
        seq.push(Phoneme::new(token, lang, true, true, start_pos, end_pos));
    }

    end_pos
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dump(text: &str) -> Vec<(String, Lang, bool, bool, bool, usize, usize)> {
        get_phoneme_info(text, true)
            .into_iter()
            .map(|p| {
                let is_tone = p.is_tone();
                (
                    p.value,
                    p.lang,
                    p.is_word_start,
                    p.is_word_end,
                    is_tone,
                    p.char_start,
                    p.char_end,
                )
            })
            .collect()
    }

    #[test]
    fn zh_syllable_splits_into_three() {
        // 撒 -> s + a + 1
        let r = dump("撒");
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].0, "s");
        assert_eq!(r[1].0, "a");
        assert_eq!(r[2].0, "1");
        assert!(r[2].4, "last phoneme is the tone");
        assert!(r[0].2 && !r[0].3, "initial marks word start");
    }

    #[test]
    fn zero_initial_marks_word_start_on_final() {
        // 用 -> y + ong + 4，y 作声母
        let r = dump("用");
        assert_eq!(r[0].0, "y");
        assert_eq!(r[1].0, "ong");
        // 儿 -> 零声母，韵母即 "er"，承载 is_word_start
        let r = dump("儿");
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].0, "er");
        assert!(r[0].2, "zero-initial final carries word start");
    }

    #[test]
    fn neutral_tone_maps_to_five() {
        // pypinyin neutral_tone_with_five=True: de5 / zi5
        let r = dump("的");
        assert_eq!(r.last().unwrap().0, "5");
        let r = dump("子");
        assert_eq!(r.last().unwrap().0, "5");
    }

    #[test]
    fn camel_case_and_digit_boundaries_split() {
        // iPhone15Pro -> i | phone | 15 | pro，四段各自按字母名展开
        let r = dump("iPhone15Pro");
        let vals: Vec<&str> = r.iter().map(|x| x.0.as_str()).collect();
        assert_eq!(
            vals,
            vec![
                "ay", "p", "iy", "ey", "ch", "ow", "eh", "n", "iy", // i  phone
                "1", "5", // 15（数字不参与字母展开）
                "p", "iy", "aa", "r", "ow", // pro
            ]
        );
        // 数字段仍是 num 语言，其余是 en
        assert_eq!(r[9].1, Lang::Num);
        assert_eq!(r[10].1, Lang::Num);
        assert!(r[..9].iter().all(|x| x.1 == Lang::En));
        assert!(r[11..].iter().all(|x| x.1 == Lang::En));
    }

    #[test]
    fn positions_are_char_indices() {
        let r = dump("中文abc");
        // 中文 -> 6 个音素，每个 2 字符跨度；abc -> 5 个（a:1 b:2 c:2）
        assert_eq!(r.len(), 11);
        let en: Vec<_> = r.iter().filter(|x| x.1 == Lang::En).collect();
        // a 展开 1 个、b 和 c 各展开 2 个，且共享各自的来源字符
        let pairs: Vec<(usize, usize)> = en.iter().map(|x| (x.5, x.6)).collect();
        assert_eq!(pairs, vec![(2, 3), (3, 4), (3, 4), (4, 5), (4, 5)]);
        // 同一来源字符的所有音素跨度都恰好是 1
        assert!(en.iter().all(|x| x.6 - x.5 == 1));
    }

    #[test]
    fn en_letters_expand_to_letter_names() {
        // USB -> U S B 的字母名
        let vals: Vec<String> = get_phoneme_info("USB", true)
            .into_iter()
            .map(|p| p.value)
            .collect();
        assert_eq!(vals, ["y", "uw", "eh", "s", "b", "iy"]);
    }

    #[test]
    fn en_word_markers_sit_on_group_edges_only() {
        // 展开出的多个音素共享来源字符，字边界只打在组首/组尾
        let seq = get_phoneme_info("USB", true);
        assert_eq!(seq.iter().filter(|p| p.is_word_start).count(), 1);
        assert_eq!(seq.iter().filter(|p| p.is_word_end).count(), 1);
        for ch in ["USB".find('U'), "USB".find('B')] {
            let group: Vec<_> = seq.iter().filter(|p| p.char_start == ch.unwrap()).collect();
            assert!(!group.is_empty());
            assert!(group.iter().all(|p| p.char_end - p.char_start == 1));
        }
    }

    #[test]
    fn word_markers_do_not_bleed_across_tokens() {
        // USB zone -> 两个词各自一套边界
        let seq = get_phoneme_info("USB zone", true);
        assert_eq!(seq.iter().filter(|p| p.is_word_start).count(), 2);
        assert_eq!(seq.iter().filter(|p| p.is_word_end).count(), 2);
    }

    #[test]
    fn word_symbol_is_absorbed_after_letters() {
        // A+ 曾坍缩成单字母 a，于是匹配英文里每一个独立冠词
        let vals: Vec<String> = get_phoneme_info("A+", true)
            .into_iter()
            .map(|p| p.value)
            .collect();
        assert_eq!(vals, ["ey", "p", "l", "ah", "s"]);
        assert!(vals.len() > get_phoneme_info("a", true).len());
    }

    #[test]
    fn digits_are_not_polluted_by_word_symbols() {
        // 1+1 仍是数字 token
        let seq = get_phoneme_info("1+1", true);
        assert!(seq.iter().all(|p| p.lang == Lang::Num));
        assert_eq!(seq.len(), 2);
    }

    #[test]
    fn zh_path_untouched_by_en_changes() {
        let seq = get_phoneme_info("撒贝宁", true);
        assert_eq!(seq.len(), 9, "中文每字 3 个音素");
        assert!(seq.iter().all(|p| p.lang == Lang::Zh));
    }

    #[test]
    fn split_pinyin_roundtrip() {
        let cases: &[(&str, &str, &str)] = &[
            ("zhong", "zh", "ong"),
            ("er", "", "er"),
            ("lüe", "l", "ve"), // ü -> v
            ("nü", "n", "v"),
            ("nv", "n", "v"),
            ("yue", "y", "ue"),
            ("a", "", "a"), // 零声母
        ];
        for (plain, exp_ini, exp_fin) in cases {
            let (ini, fin) = split_pinyin(plain);
            assert_eq!(ini, *exp_ini, "声母: {plain}");
            assert_eq!(fin.as_ref(), *exp_fin, "韵母: {plain}");
        }
    }

    #[test]
    fn ascii_split_disabled_keeps_token_whole() {
        let r = get_phoneme_info("abc", false);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].value, "abc");
        assert!(r[0].is_word_start && r[0].is_word_end);
    }

    #[test]
    fn punctuation_skipped_greek_kept() {
        let r = dump("α，中");
        // α 保留为 other，中 -> zh/ong/1
        assert_eq!(r[0].0, "α");
        assert_eq!(r[0].1, Lang::Other);
        assert_eq!(r[1].0, "zh");
        assert_eq!(r[2].0, "ong");
    }
}
