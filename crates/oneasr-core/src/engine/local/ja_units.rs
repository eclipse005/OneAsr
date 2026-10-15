//! CTC 日语单元分组（Qwen 对齐器口径）。
//!
//! CTC 的 `AlignOutput::words` 对无空格书写系统逐字输出——日语因此是「字级
//! 单元」（判定在 crate 的 `spans::unit_runs`，见 [`super::ctc_aligner`] 的
//! 模块头）。Qwen 对齐器走的是另一条路：它用 nagisa 的 morpheme 当对齐单元
//! （`qwen3-aligner-wgpu/src/words.rs`）。两条对齐器给同一条管线喂出两种粒度，
//! 断句层就得为日语额外背一族手写绑定（`sentence_boundary::boundary_rules`
//! 里的日语规则族）。
//!
//! 这里把 CTC 的日语粒度对齐到 Qwen 一侧：借 Qwen 对齐器 crate 公开的
//! `split_words`（同一条 `ja` 路径、同一套 clean 规则——不在本仓库写第二套
//! 日语分词，两处永远同步）把整段文本切成词表，再把这串词映射回字符单元：
//! 每个词吃掉一串单元，时间取「首单元 start / 末单元 end」，文本取被吃掉的
//! 单元拼起来的原文——标点照旧跟着它骑过的字走，可见文本与逐字时一致。
//!
//! 映射对不上（nagisa 归一化改了字形、文稿与单元序列不一致、`ja` feature
//! 未启用……）就返回 `None`，调用方原样保留逐字结果——这一步不丢时间、不丢字。
//!
//! 没落进任何词跨度的单元（行首/行尾孤立的标点，例如引号 `「」`）贴给相邻词
//! 并把它的时间并进那个词：**每个单元都有归属**，首词 start = 首单元 start、
//! 末词 end = 末单元 end，中间不漏时间。

use qwen3_aligner_wgpu::split_words;
use qwen3_aligner_wgpu::words::is_kept_char;

use crate::engine::AlignedToken;

/// 日语才分组，失败即回退；其余语言原样返回。
///
/// 调用点在 [`super::ctc_aligner`]：`AlignOutput::words` 已经是最终形态的
/// 对齐单元，这里只在日语上再动一次手——理由见模块头。
pub(super) fn apply(language: &str, text: &str, units: Vec<AlignedToken>) -> Vec<AlignedToken> {
    if crate::lang::to_lang_key(language) != "ja" {
        return units;
    }
    group_japanese(text, &units).unwrap_or(units)
}

/// `None` = 保持逐字单元不动（任何映射对不上都走这里）。
fn group_japanese(text: &str, units: &[AlignedToken]) -> Option<Vec<AlignedToken>> {
    if units.is_empty() {
        return None;
    }
    // feature 未启用时 `split_words` 返回 Err，这里自然回退。
    let words = split_words(text, Some("ja")).ok()?;
    group_with_words(&words, units)
}

/// 纯映射：把词表铺回字符单元。独立成函数是为了让「对不上 → None」这条
/// 兜底路径能在不加载模型的前提下被单测钉住（词表由测试直接给）。
fn group_with_words(words: &[String], units: &[AlignedToken]) -> Option<Vec<AlignedToken>> {
    if words.is_empty() {
        return None;
    }
    // 平铺字符流：每个字符记住它属于哪个单元。词表按 clean 口径只含实字，
    // 标点/空白在这里被跳过——跳过不丢字，它们仍留在单元的原文里。
    let chars: Vec<(char, usize)> = units
        .iter()
        .enumerate()
        .flat_map(|(i, u)| u.text.chars().map(move |c| (c, i)))
        .collect();
    let mut cursor = 0usize;
    let mut spans: Vec<(usize, usize)> = Vec::with_capacity(words.len());
    for w in words {
        let mut first: Option<usize> = None;
        let mut last = 0usize;
        for wc in w.chars() {
            while cursor < chars.len() && !is_kept_char(chars[cursor].0) {
                cursor += 1;
            }
            let (c, unit) = *chars.get(cursor)?;
            if c != wc {
                return None;
            }
            first.get_or_insert(unit);
            last = unit;
            cursor += 1;
        }
        spans.push((first?, last));
    }
    // 词表之后不许再剩实字——剩下的只能是既不属于任何词、也没巧坐进词跨度
    // 的标点（由 `assemble` 贴给相邻词）。
    if chars[cursor..].iter().any(|(c, _)| is_kept_char(*c)) {
        return None;
    }
    // 词界必须落在单元与单元之间；同一单元被两个词分摊（拉丁长串被切）时
    // 拼不出干净的归属，回退逐字。
    if spans.windows(2).any(|w| w[1].0 <= w[0].1) {
        return None;
    }
    Some(assemble(units, &spans))
}

/// 单元原文按词跨度拼回文本。空档（不属于任何词的标点单元）归前一个词，
/// 行首的空档归第一个词；时间跟着覆盖到的首/末单元走。
fn assemble(units: &[AlignedToken], spans: &[(usize, usize)]) -> Vec<AlignedToken> {
    let mut out: Vec<AlignedToken> = Vec::with_capacity(spans.len());
    for (idx, _) in spans.iter().enumerate() {
        let start = if idx == 0 { 0 } else { spans[idx - 1].1 + 1 };
        let end = if idx + 1 < spans.len() {
            spans[idx + 1].0 - 1
        } else {
            units.len() - 1
        };
        let text: String = units[start..=end].iter().map(|u| u.text.as_str()).collect();
        out.push(AlignedToken {
            text,
            start_sec: units[start].start_sec,
            end_sec: units[end].end_sec,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(text: &str, start: f64, end: f64) -> AlignedToken {
        AlignedToken {
            text: text.to_string(),
            start_sec: start,
            end_sec: end,
        }
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// 逐字单元 → Qwen 词表：文本无损、时间取首/末单元、标点骑在词尾。
    #[test]
    fn groups_character_units_into_qwen_words() {
        let units = vec![
            unit("私", 0.0, 0.4),
            unit("は", 0.4, 0.6),
            unit("学", 0.6, 0.9),
            unit("校", 0.9, 1.3),
            unit("に", 1.3, 1.5),
            unit("行", 1.5, 1.7),
            unit("き", 1.7, 1.9),
            unit("ま", 1.9, 2.1),
            unit("す。", 2.1, 2.3),
        ];
        let out = group_with_words(&words(&["私", "は", "学校", "に", "行き", "ます"]), &units)
            .expect("must group");
        let texts: Vec<&str> = out.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["私", "は", "学校", "に", "行き", "ます。"]);
        assert_eq!((out[0].start_sec, out[0].end_sec), (0.0, 0.4));
        assert_eq!((out[2].start_sec, out[2].end_sec), (0.6, 1.3));
        assert_eq!((out[5].start_sec, out[5].end_sec), (1.9, 2.3));
        assert_eq!(
            out.iter().map(|t| t.text.as_str()).collect::<String>(),
            "私は学校に行きます。"
        );
    }

    /// 行首/行尾孤立的标点（「」）贴进相邻词，文本与时间覆盖都不丢。
    #[test]
    fn orphan_punctuation_attaches_and_keeps_full_coverage() {
        let units = vec![
            unit("「", 0.0, 0.1),
            unit("は", 0.1, 0.3),
            unit("い", 0.3, 0.5),
            unit("」", 0.5, 0.6),
        ];
        let out = group_with_words(&words(&["はい"]), &units).expect("must group");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "「はい」");
        assert_eq!((out[0].start_sec, out[0].end_sec), (0.0, 0.6));
    }

    /// 多字符单元（拉丁串）整体落进一个词；文本仍然无损。
    #[test]
    fn latin_run_stays_one_unit_and_text_is_lossless() {
        let units = vec![
            unit("こ", 0.0, 0.2),
            unit("れ", 0.2, 0.4),
            unit("は", 0.4, 0.6),
            unit("OK", 0.6, 1.0),
            unit("で", 1.0, 1.2),
            unit("す", 1.2, 1.4),
        ];
        let out = group_with_words(&words(&["これ", "は", "OK", "です"]), &units).expect("group");
        let texts: Vec<&str> = out.iter().map(|t| t.text.as_str()).collect();
        assert_eq!(texts, ["これ", "は", "OK", "です"]);
        assert_eq!(
            out.iter().map(|t| t.text.as_str()).collect::<String>(),
            "これはOKです"
        );
    }

    /// 对不上就必须拒绝（调用方回退逐字），而不是硬拼。
    #[test]
    fn rejects_mismatched_words() {
        let units = vec![unit("私", 0.0, 0.4), unit("は", 0.4, 0.6)];
        // 词表更长：吃掉实字后没有下一颗。
        assert!(group_with_words(&words(&["私", "は", "学校"]), &units).is_none());
        // 同长但对不上（は vs よ）。
        assert!(group_with_words(&words(&["私", "よ"]), &units).is_none());
        // 词表短了：还剩实字「は」。
        assert!(group_with_words(&words(&["私"]), &units).is_none());
        // 空词表不是合法分组。
        assert!(group_with_words(&[], &units).is_none());
    }

    /// 非日语语言直通，一个单元都不动。
    #[test]
    fn apply_is_passthrough_for_other_languages() {
        let units = vec![unit("你", 0.0, 0.5), unit("好", 0.5, 1.0)];
        let out = apply("zh", "你好", units.clone());
        assert_eq!(out, units);
        let out = apply("English", "hello world", vec![unit("hello", 0.0, 1.0)]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello");
    }

    /// 日语分组的真实路径：词表来自 Qwen 的 `split_words`（会加载内嵌模型），
    /// 单元是 CTC 的逐字形态（标点零时长骑在「す」上）。
    #[test]
    fn apply_groups_japanese_with_qwen_word_list() {
        let source = "私は学校に行きます。";
        let mut units: Vec<AlignedToken> = Vec::new();
        let mut t = 0.0;
        for c in source.chars() {
            if c == '。' {
                units
                    .last_mut()
                    .expect("transcript starts with a letter")
                    .text
                    .push(c);
            } else {
                units.push(unit(&c.to_string(), t, t + 0.2));
                t += 0.2;
            }
        }
        let out = apply("ja", source, units.clone());
        // 真的分组了，而不是回退。
        assert!(out.len() < units.len());
        // 文本无损。
        assert_eq!(
            out.iter().map(|t| t.text.as_str()).collect::<String>(),
            source
        );
        // 每个词 clean 之后 == Qwen 对齐器的词表（同源，逐词一致）。
        let expected = split_words(source, Some("ja")).expect("embedded ja model");
        let cleaned: Vec<String> = out
            .iter()
            .map(|t| t.text.chars().filter(|c| is_kept_char(*c)).collect())
            .collect();
        assert_eq!(cleaned, expected);
    }

    /// 文稿与单元对不上时原样回退（这条路径经过真实 `split_words`）。
    #[test]
    fn apply_falls_back_to_units_when_mapping_fails() {
        let units = vec![unit("私", 0.0, 0.4), unit("は", 0.4, 0.6)];
        let out = apply("ja", "東京", units.clone());
        assert_eq!(out, units);
    }
}
