//! Where a sentence ends: Punkt when a language model is available, punctuation
//! rules otherwise.
//!
//! Both paths produce split points over the same word list, so downstream stages
//! never need to know which one ran.

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::text_rules::{ends_with_terminal_punctuation, strip_trailing_closers};

use super::boundary_rules::{
    is_ja_turn_start_after, is_japanese_spoken_end, is_opening_punctuation,
};
use super::profile::LanguageProfile;
use super::punkt_map::map_sentence_boundaries_to_word_indices;
use super::types::SplitReason;
use super::util::join_words;

/// Pre-split (hard boundaries): sentence-terminal punctuation (`. ! ? 。`).
///
/// **英文路径（Punkt 统计学习）**：当 `profile.uses_punkt_sentence_boundary()`
/// 返回 `true` 时，把所有 word 拼接成完整文本交给 Punkt 断句，再把句子边界
/// 映射回 word 索引。Punkt 会自动识别 `Mr.`/`Dr.`/`p.m.`/`U.S.` 等缩写而不
/// 在这些位置切分。单字母缩写链（`J. K.`）仍用规则特判。
///
/// **其他语言路径（规则 + 缩写表）**：每个以 `. ! ?` 结尾的 token 产生切分，
/// 除非它在 `LanguageProfile::abbreviations()` 列表中，或是单字母缩写链。
///
/// VAD silence crossings are deliberately NOT hard-split here. They are handled
/// by the DP cost function in `subtitle_layout.rs`, which only splits when a
/// span exceeds the length budget — preventing mid-sentence fragmentation on
/// short sentences that merely contain a breath pause.
///
/// A run of unit text that can be read by index.
///
/// The sentence layer is **pure text** — it never reads a timestamp — but its
/// two callers hold different containers: the transcription path has
/// `&[WordTokenDto]` (which carries timing this layer happens not to need) and
/// 智能断句 has a plain `&[&str]`. This trait is what lets both call the *same*
/// code, rather than one caller building a throwaway `Vec<&str>` for every
/// boundary it asks about (which would be quadratic on a long transcript).
pub(super) trait UnitText {
    /// Text of unit `i`, or `None` past the end.
    fn unit(&self, i: usize) -> Option<&str>;
    /// How many units there are.
    fn unit_count(&self) -> usize;
}

impl UnitText for [WordTokenDto] {
    fn unit(&self, i: usize) -> Option<&str> {
        self.get(i).map(|w| w.word.as_str())
    }
    fn unit_count(&self) -> usize {
        self.len()
    }
}

impl UnitText for [&str] {
    fn unit(&self, i: usize) -> Option<&str> {
        self.get(i).copied()
    }
    fn unit_count(&self) -> usize {
        self.len()
    }
}

/// The input is **unit text**, not timed tokens: nothing in this module reads a
/// timestamp (it never did — it only ever used `WordTokenDto` as a container).
/// Keeping it that way is what lets 智能断句 call it directly, before alignment
/// has produced any timing to speak of.
pub(super) fn build_split_points_from_hard_boundaries<U: UnitText + ?Sized>(
    units: &U,
    profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    if profile.uses_punkt_sentence_boundary() {
        build_split_points_with_punkt(units, profile)
    } else {
        build_split_points_with_rules(units, profile)
    }
}

/// 英文路径：Punkt 统计学习断句 + 句号兜底 + 单字母缩写链特判。
///
/// Punkt 在短文本或训练数据覆盖不足时可能漏切（如 "Hello world. Again."
/// 被识别为单句）。为了不退化到比规则更差，对 Punkt 没识别的**句号**
/// 位置再用规则补切。`?`/`!` 不走兜底：引号标题（`a "What Do You See?" post`）
/// 里 Punkt 正确地不切，强制补切会把中心语甩到下一句。
///
/// Punkt 已经切了的 `?`/`!` 也要再看一眼：闭引号后若不是句首（小写延续），
/// 仍然不切。
fn build_split_points_with_punkt<U: UnitText + ?Sized>(
    units: &U,
    _profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    use punkt::params::Standard;
    use punkt::{SentenceTokenizer, TrainingData};
    use std::collections::HashSet;
    use std::sync::OnceLock;

    let n = units.unit_count();
    if n == 0 {
        return Vec::new();
    }

    // 拼接完整文本（join_words 处理标点前空格等）
    let text = join_words((0..n).filter_map(|i| units.unit(i)));
    if text.trim().is_empty() {
        return Vec::new();
    }

    // 训练数据是编译时嵌入的，加载一次后复用
    static DATA: OnceLock<TrainingData> = OnceLock::new();
    let data = DATA.get_or_init(TrainingData::english);

    // Punkt 断句
    let tokenizer = SentenceTokenizer::<Standard>::new(&text, data);
    let sentences: Vec<&str> = tokenizer.collect();

    // 映射回 word 索引
    let punkt_splits: HashSet<usize> =
        map_sentence_boundaries_to_word_indices(&text, &sentences, units)
            .into_iter()
            .collect();

    // 规则兜底：Punkt 没切的句末标点位置也切
    // （但排除单字母缩写链 J. K. 的内部）
    let mut out = Vec::new();
    for index in 0..n {
        // Punkt 已经识别为切分点
        if punkt_splits.contains(&index) {
            // 单字母缩写链特判
            if is_single_letter_dotted(units.unit(index).unwrap_or_default()) {
                let continues = units.unit(index + 1).is_some_and(is_single_letter_dotted);
                if continues {
                    continue;
                }
            }
            // Punkt 切了，但 `?`/`!` 未必真的是句末：引号标题（`a "What Do
            // You See?" post`）里 Punkt 正确切了，可紧接着的中心语说明它是
            // 引用内部，仍要并回来。`Really? she asked` 同理（下一词非句首）。
            if should_suppress_question_bang_split(units, index) {
                continue;
            }
            push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
            continue;
        }

        // 规则兜底：Punkt 漏切的句末标点（`. ! ?` 都补）。`?`/`!` 必须补——
        // 漏掉一个句子边界会把两句并成一行。真正需要防的不是"补切"，而是
        // "在引号内部补切"，那由下面的 suppress 负责。
        // 只对最后一个 word 跳过（它是真正的文本末尾，不需要切）
        if index == n - 1 {
            continue;
        }
        if is_terminal_end_rule_fallback(units.unit(index).unwrap_or_default()) {
            // 单字母缩写链特判
            if is_single_letter_dotted(units.unit(index).unwrap_or_default()) {
                let continues = units.unit(index + 1).is_some_and(is_single_letter_dotted);
                if continues {
                    continue;
                }
            }
            if should_suppress_question_bang_split(units, index) {
                continue;
            }
            push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
        }
    }
    out
}

/// English `?`/`!` that is NOT a real sentence end: a quoted title/question
/// whose next content word continues the outer sentence
/// (`I post a "What Do You See?" post.`), or any `?`/`!` followed by a
/// non-sentence-start (`Really? she asked`).
///
/// The discriminator is the word AFTER the mark, not the mark itself — both
/// shapes are `X?"` and neither carries any other signal:
///
/// - `… do you see?" Then he left.`  → `Then` opens a sentence → split
/// - `… see?" ` + `do` (lowercase)   → continues the sentence → suppress
/// - `… See?" post.`                 → `post.` continues it → suppress
///
/// Closing-quote tokens are skipped when looking for the next word, so
/// `See?` + `"` + `post.` and `See?"` + `post.` behave the same.
///
/// English-only by construction — it keys off capitalisation, so callers must
/// gate it on [`LanguageProfile::uses_punkt_sentence_boundary`].
pub(super) fn should_suppress_question_bang_split<U: UnitText + ?Sized>(
    units: &U,
    index: usize,
) -> bool {
    let Some(token) = units.unit(index) else {
        return false;
    };
    if !is_question_bang_terminal(token) {
        return false;
    }
    let Some(next) = next_content_word(units, index) else {
        return false;
    };
    !looks_like_english_sentence_start(next)
}

/// Does `token` end with a question/exclamation mark (any script)?
fn is_question_bang_terminal(token: &str) -> bool {
    matches!(
        strip_trailing_closers(token.trim()).chars().last(),
        Some('!' | '?' | '！' | '？' | '‼' | '⁇' | '⁈' | '⁉')
    )
}

fn is_closer_only_token(token: &str) -> bool {
    let trimmed = token.trim();
    !trimmed.is_empty() && strip_trailing_closers(trimmed).is_empty()
}

fn next_content_word<U: UnitText + ?Sized>(units: &U, index: usize) -> Option<&str> {
    // `find_map(..).filter(..)` would stop at the first *existing* unit and then
    // reject it — it would never look past a closer-only token. The filter has
    // to sit inside the search for "next word that isn't a stray quote mark".
    ((index + 1)..units.unit_count())
        .find_map(|i| units.unit(i).filter(|token| !is_closer_only_token(token)))
}

fn looks_like_english_sentence_start(token: &str) -> bool {
    let trimmed = token.trim().trim_start_matches(|c: char| {
        is_opening_punctuation(c) || matches!(c, '"' | '\'' | '«' | '‹')
    });
    let alpha: Vec<char> = trimmed.chars().filter(|c| c.is_alphabetic()).collect();
    match alpha.first() {
        Some(c) => c.is_uppercase(),
        None => false,
    }
}

/// 规则兜底：token 是否以句末标点结尾（`. ! ?`，含 CJK 等价符）。
///
/// 这是 Punkt 漏切时的 fallback。不依赖 Punkt 已知的缩写（Punkt 应该
/// 已经识别缩写并避免切分），但当 Punkt 漏切时，我们用一个小型常见
/// 缩写黑名单 + 点分隔模式来避免误切缩写。
fn is_terminal_end_rule_fallback(token: &str) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    // 排除常见缩写（Punkt 可能漏切的）
    if is_common_abbreviation(&lower) {
        return false;
    }
    // 排除点分隔缩写（p.m. / U.S. / e.g. / i.e. / Ph.D.）
    if is_dotted_abbreviation(&lower) {
        return false;
    }
    true
}

/// 常见英文缩写黑名单（Punkt 漏切时的兜底保护）。
///
/// 这不是完整的缩写表，只是规则兜底里用来防止误切的高频缩写。
/// Punkt 负责绝大多数缩写识别，这里只补 Punkt 训练数据覆盖不到的。
const COMMON_ABBREVIATIONS: &[&str] = &[
    "mr.", "mrs.", "ms.", "dr.", "prof.", "rev.", "hon.", "sr.", "jr.", "st.", "mt.", "no.", "vs.",
    "etc.", "al.", "cf.", "fig.", "ed.", "vol.", "pp.", "dept.", "inc.", "ltd.", "co.", "corp.",
    "bros.", "llc.", "jan.", "feb.", "mar.", "apr.", "jun.", "jul.", "aug.", "sep.", "sept.",
    "oct.", "nov.", "dec.", "ave.", "blvd.", "rd.", "ln.", "ct.", "pl.", "pres.", "gov.", "sen.",
    "rep.", "capt.", "cmdr.", "col.", "gen.", "lt.", "maj.", "sgt.", "adm.", "univ.", "assn.",
    "assoc.", "esq.", "mx.", "fr.", "amb.",
];

fn is_common_abbreviation(lower: &str) -> bool {
    COMMON_ABBREVIATIONS.contains(&lower)
}

/// 判断 token 是否是点分隔缩写模式：a.b. / u.s. / p.m. / e.g. / ph.d.
/// 形如 2+ 个短字母段用点连接，每段 <= 3 字母。
fn is_dotted_abbreviation(lower: &str) -> bool {
    // 去掉末尾的句号再判断
    let s = lower.trim_end_matches('.').trim();
    if s.is_empty() {
        return false;
    }
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_alphabetic()))
}

/// 其他语言路径：规则 + 缩写表。
fn build_split_points_with_rules<U: UnitText + ?Sized>(
    units: &U,
    profile: &dyn LanguageProfile,
) -> Vec<(usize, SplitReason)> {
    let mut out = Vec::<(usize, SplitReason)>::new();
    let abbrs = profile.abbreviations();
    for index in 0..units.unit_count() {
        let here = units.unit(index).unwrap_or_default();
        let punct_end = is_terminal_end(here, abbrs);
        let spoken_end = units.unit(index + 1).is_some_and(|next| {
            let next2 = units.unit(index + 2).unwrap_or_default();
            let prev = if index == 0 {
                ""
            } else {
                units.unit(index - 1).unwrap_or_default()
            };
            is_japanese_spoken_end(prev, here, next, next2)
        });
        // はい / 皆さん start a new move even when the current line is short.
        let turn_before = units.unit(index + 1).is_some_and(|next| {
            let next2 = units.unit(index + 2).unwrap_or_default();
            is_ja_turn_start_after(here, next, next2)
        });
        if !punct_end && !spoken_end && !turn_before {
            continue;
        }
        // Single-letter dotted token (B./A./J.): only suppress the split when
        // it forms an initial chain with the next token. An isolated single-
        // letter token is a real sentence end (e.g. "step one B.").
        if punct_end && is_single_letter_dotted(here) {
            let continues = units.unit(index + 1).is_some_and(is_single_letter_dotted);
            if continues {
                continue;
            }
        }
        push_split_point(&mut out, index, SplitReason::TerminalPunctuation);
    }
    out
}

/// Does `token` end with a sentence-terminal mark that should force a split?
/// Returns false for language-specific abbreviations in `abbrs`.
fn is_terminal_end(token: &str, abbrs: &[&str]) -> bool {
    let normalized = strip_trailing_closers(token.trim());
    if normalized.is_empty() || !ends_with_terminal_punctuation(normalized) {
        return false;
    }
    let lower = normalized.to_ascii_lowercase();
    !abbrs.contains(&lower.as_str())
}

/// Is `token` a single ASCII letter followed by a dot (e.g. `B.`, `A.`)?
fn is_single_letter_dotted(token: &str) -> bool {
    let chars: Vec<char> = strip_trailing_closers(token.trim()).chars().collect();
    chars.len() == 2 && chars[0].is_ascii_alphabetic() && chars[1] == '.'
}

#[cfg(test)]
pub(super) fn build_deterministic_split_points(
    words: &[WordTokenDto],
) -> Vec<(usize, SplitReason)> {
    use super::profile::profile_for_lang;
    let units: Vec<&str> = words.iter().map(|w| w.word.as_str()).collect();
    build_split_points_from_hard_boundaries(&units[..], &*profile_for_lang("en"))
}

fn push_split_point(
    split_points: &mut Vec<(usize, SplitReason)>,
    index: usize,
    reason: SplitReason,
) {
    if split_points.last().map(|(end, _)| *end) == Some(index) {
        return;
    }
    split_points.push((index, reason));
}

pub(super) fn split_points_to_spans(
    word_total: usize,
    split_points: &[(usize, SplitReason)],
) -> Vec<(usize, usize)> {
    if word_total == 0 {
        return Vec::new();
    }

    let mut out = Vec::<(usize, usize)>::new();
    let mut cursor = 0usize;
    for (end, _) in split_points.iter().copied() {
        if end < cursor || end + 1 >= word_total {
            continue;
        }
        out.push((cursor, end));
        cursor = end + 1;
    }
    out.push((cursor, word_total - 1));
    out
}
