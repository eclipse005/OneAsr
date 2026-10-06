//! 无时间的行切分 —— 给「手里只有文字、还没有时间戳」的场合用。
//!
//! 排版 DP（`subtitle_layout`）能算出最好的切点，靠的是词的时间戳：每段的时长、
//! 停顿、标点位置都是代价的一部分。文稿在打轴之前只有文字，那些代价一个都没有。
//!
//! 所以这里给一个**没有时间**的版本：**同一个长度预算、同一套「什么算一个单元」
//! 的判定**，只是切点只能贪心地找。预算与单位都问 `profile`，所以编辑器和生成器
//! 看到的宽窄是同一个数字——这正是「界面上看到的断句就是生成结果」那条不变式
//! 能成立的地方。
//!
//! 两条性质，各有一条测试钉死：
//!
//! * **只动换行**：非空白字符序列前后逐字相同。切开的行按 `join_words` 重新拼，
//!   而它只增删空格——所以「你的字一个不动」成立。
//! * **幂等**：再跑一次不动任何东西。生成时还会再兜底一次，幂等保证第二遍是空操作。

use super::preset::subtitle_length_preset_from_id;
use super::profile::{LanguageProfile, profile_for_lang};
use super::util::{is_no_space_script, join_words};

/// 把文稿里过长的行切开，返回切好的整段文本。
///
/// 空行原样保留：段落结构是作者写的，不是我们该顺手抹平的。`preset_id` 用的是
/// 断句那套 id（`short` / `standard` / `loose`），未知值按 `standard`。
pub fn break_long_lines(text: &str, lang: &str, preset_id: &str) -> String {
    let profile = profile_for_lang(lang);
    let limit = f64::from(profile.source_limit(subtitle_length_preset_from_id(preset_id)));
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let units = layout_units(line);
        if units.is_empty() {
            out.push(String::new());
            continue;
        }
        out.extend(split_line(line, &units, limit, profile.as_ref()));
    }
    out.join("\n")
}

/// 一行拆成「排版单元」：不带空格的书写系统逐字，带空格的按词。
///
/// 与两个对齐器给出的 token 粒度是同一件事——CJK 逐字、拉丁按词——所以这里的
/// 「一个单元」就是打轴之后「一个时间戳」。
fn layout_units(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut latin = String::new();
    for ch in line.chars() {
        if ch.is_whitespace() {
            push_word(&mut latin, &mut out);
        } else if is_no_space_script(ch) {
            push_word(&mut latin, &mut out);
            out.push(ch.to_string());
        } else {
            latin.push(ch);
        }
    }
    push_word(&mut latin, &mut out);
    out
}

fn push_word(word: &mut String, out: &mut Vec<String>) {
    if !word.is_empty() {
        out.push(std::mem::take(word));
    }
}

/// 切分一行。放得下就**原样返回**——不碰用户的空格；放不下才切，切出的片段按
/// `join_words` 重新拼（那是我们唯一一份「这段文字该怎么排」的知识）。
fn split_line(
    line: &str,
    units: &[String],
    limit: f64,
    profile: &dyn LanguageProfile,
) -> Vec<String> {
    let total: f64 = units.iter().map(|u| profile.token_units(u)).sum();
    if total <= limit {
        return vec![line.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    let mut acc = 0.0f64;
    for (i, unit) in units.iter().enumerate() {
        let w = profile.token_units(unit);
        // `i > start` 是防呆：单个单元就超预算时（一个超长词、一串没有空格的
        // 拉丁长句），宁可留一个超预算的整词，也不把它切成碎片。
        if i > start && acc + w > limit {
            out.push(join_words(units[start..i].iter().map(String::as_str)));
            start = i;
            acc = 0.0;
        }
        acc += w;
    }
    if start < units.len() {
        out.push(join_words(units[start..].iter().map(String::as_str)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 断行只能改换行。这条是这个功能的核心承诺：用户的字是权威的。
    fn letters(s: &str) -> String {
        s.chars().filter(|c| !c.is_whitespace()).collect()
    }

    fn assert_only_newlines_changed(before: &str, after: &str) {
        assert_eq!(letters(before), letters(after), "一个字都不许变");
    }

    #[test]
    fn a_short_line_is_returned_untouched() {
        // 包括它自己带的空格：不该被顺手规范化。
        let line = "你  好";
        assert_eq!(break_long_lines(line, "zh", "standard"), line);
    }

    #[test]
    fn a_long_cjk_line_is_cut_at_the_budget() {
        let text =
            "最近看到一些关于人工智能与语音识别在时间轴准确性上的负面评价尤其是应用最为广泛的原版";
        let out = break_long_lines(text, "zh", "standard");
        assert_only_newlines_changed(text, &out);
        let lines: Vec<&str> = out.split('\n').collect();
        assert!(lines.len() > 1, "这么长的一行必须被切开");
        // 中文 standard 预算是 22 字。
        for l in &lines {
            assert!(l.chars().count() <= 22, "还有超预算的行：{l:?}");
        }
    }

    #[test]
    fn a_long_latin_line_is_cut_at_word_boundaries() {
        let text = "this is a fairly long english sentence that has to be split somewhere sensible for subtitles";
        let out = break_long_lines(text, "en", "standard");
        assert_only_newlines_changed(text, &out);
        for l in out.split('\n') {
            assert!(
                l.split_whitespace().count() <= 16,
                "英文按词切，每行不超过 16 词：{l:?}"
            );
            assert!(!l.starts_with(' '), "切口不能留下行首空格：{l:?}");
        }
    }

    /// 幂等：生成时还会再兜底一次，第二遍必须是空操作。
    #[test]
    fn running_it_twice_changes_nothing() {
        for (lang, text) in [
            (
                "zh",
                "最近看到一些关于人工智能与语音识别在时间轴准确性上的负面评价尤其是应用最为广泛的原版",
            ),
            (
                "en",
                "this is a fairly long english sentence that has to be split somewhere sensible",
            ),
            (
                "ja",
                "最近 Challenger 府的.Print  película について 語ります 日本語 テキスト 字幕 の 分割 について",
            ),
        ] {
            let once = break_long_lines(text, lang, "short");
            let twice = break_long_lines(&once, lang, "short");
            assert_eq!(once, twice, "{lang} 断行不幂等");
        }
    }

    #[test]
    fn paragraph_structure_survives() {
        let text = "第一段。\n\n第二段。";
        assert_eq!(break_long_lines(text, "zh", "short"), text);
    }

    /// 单个单元就超预算时留整词，不切碎。
    #[test]
    fn an_unbreakable_run_is_left_whole() {
        let long_word = "a".repeat(40);
        let out = break_long_lines(&long_word, "en", "short");
        assert_eq!(out, long_word, "一个词不拆");
    }
}
