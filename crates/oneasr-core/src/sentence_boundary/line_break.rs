//! 无时间的行切分 —— 给「手里只有文字、还没有时间戳」的场合用。
//!
//! 这是「智能断句」的实现：用户在文稿卡片上点一下，我们把过长的行重新断成
//! 字幕该有的样子。**它和转写路径跑的是同一套断句逻辑** —— 同一个语义句界
//! （`semantic`）、同一套语法守卫（`boundary_rules`）、同一个排版 DP
//! （`subtitle_layout`）、同一份长度预算（`SpanBudget`）。
//!
//! 唯一的差别是输入少两项：打轴之前没有时间戳，也没有 VAD。这两项在边界规则里
//! 本来就是显式的「未知」（见 `subtitle_layout::timing`），传 `None` 时它们既不
//! 奖励也不惩罚一个切点，也绝不会因为「看起来粘在一起」而压制切点。所以 DP 自然
//! 退化成纯「文本 + 长度」优化器，策略的其余部分一字不改。
//!
//! 早先这里是**另一套更弱的实现**：一个只看长度的贪心 first-fit，只实现了 DP 的
//! 强制档，于是「超预算就切」而完全不看标点——中文实测 92% 的断点落在句中。
//!
//! 两条性质，各有一条测试钉死：
//!
//! * **只动换行**：非空白字符序列前后逐字相同。切开的行按 `join_words` 重新拼，
//!   而它只增删空格——所以「你的字一个不动」成立。
//! * **幂等**：再跑一次不动任何东西。生成时还会再兜底一次，幂等保证第二遍是空操作。

use super::preset::{SubtitleLengthPreset, subtitle_length_preset_from_id};
use super::profile::{LanguageProfile, profile_for_lang};
use super::semantic::{build_split_points_from_hard_boundaries, split_points_to_spans};
use super::subtitle_layout::build_subtitle_layout_split_points_text_only;
use super::util::{is_no_space_script, join_words};

/// How many times [`break_long_lines`] re-runs itself looking for a fixed point.
///
/// Two is enough in practice: pass 1 does the segmentation, pass 2 confirms
/// (or corrects) it, pass 3 confirms pass 2. The bound exists so a pathological
/// input can't spin.
const MAX_STABILIZE_PASSES: usize = 3;

/// 把文稿里过长的行切开，返回切好的整段文本。
///
/// 空行原样保留：段落结构是作者写的，不是我们该顺手抹平的。`preset_id` 用的是
/// 断句那套 id（`short` / `standard` / `loose`），未知值按 `standard`。
///
/// 跑到**不动点**为止。排版 DP 是对整段做的全局最优，它输出的某一小段单独再
/// 优化时未必得到同一个切点——这是全局优化器的固有性质，不是缺陷。但这个函数
/// 必须幂等：生成路径会在文稿已经断过一次之后再兜底跑一遍（见本模块开头的两条
/// 性质），所以「跑第二遍什么都不动」是硬要求。跑到达不到不动点就停。
pub fn break_long_lines(text: &str, lang: &str, preset_id: &str) -> String {
    let mut current = one_pass(text, lang, preset_id);
    for _ in 1..MAX_STABILIZE_PASSES {
        let next = one_pass(&current, lang, preset_id);
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn one_pass(text: &str, lang: &str, preset_id: &str) -> String {
    let profile = profile_for_lang(lang);
    let preset = subtitle_length_preset_from_id(preset_id);
    let mut out: Vec<String> = Vec::new();
    for line in text.split('\n') {
        let units = layout_units(line);
        if units.is_empty() {
            out.push(String::new());
            continue;
        }
        out.extend(split_line(line, &units, preset, profile.as_ref()));
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
///
/// 切点来自转写路径的同两个阶段：先按句末标点划出语义句界，再让排版 DP 在仍然
/// 超预算的句子里挑切点（语法好切点优先，找不到就宁可整句保留）。没有时间戳，
/// 所以 DP 不知道哪里有停顿——那正是「智能断句」与转写结果可能不同的地方，
/// 也正是它唯一该不同的地方。
fn split_line(
    line: &str,
    units: &[String],
    preset: SubtitleLengthPreset,
    profile: &dyn LanguageProfile,
) -> Vec<String> {
    let limit = f64::from(profile.source_limit(preset));
    let total: f64 = units.iter().map(|u| profile.token_units(u)).sum();
    if total <= limit {
        return vec![line.to_string()];
    }
    let views: Vec<&str> = units.iter().map(String::as_str).collect();

    // 语义句界与排版切点是**并集**，与转写路径完全一致（`build_source_sentences_
    // from_words` 做的是同一件事）：句末标点自己就是切点，DP 只负责在仍然超预算
    // 的句子里再加刀。只取 DP 那一份的话，「三句短句拼成一行」会因为每句都在
    // 预算内而一个切点都不产生，整行原样返回。
    let semantic = build_split_points_from_hard_boundaries(&views[..], profile);
    let semantic_spans = split_points_to_spans(units.len(), &semantic);
    let layout =
        build_subtitle_layout_split_points_text_only(&views[..], &semantic_spans, profile, preset);
    let cuts = super::merge_split_points(semantic, layout);

    // 一个都没切 → 原样返回。
    if cuts.is_empty() {
        return vec![line.to_string()];
    }

    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    for (last_of_left, _) in cuts {
        // 切点下标是**左段最后一个单元**（`dp_split_span` 给的是 `start + k - 1`，
        // `build_split_points_from_hard_boundaries` 给的是句末那个词），与
        // `split_points_to_spans` 对切点的期望一致。
        let end = last_of_left + 1;
        if end > start && end <= units.len() {
            out.push(join_words(units[start..end].iter().map(String::as_str)));
            start = end;
        }
    }
    if start < units.len() {
        out.push(join_words(units[start..].iter().map(String::as_str)));
    }
    // 防御：DP 理论上不会给出空行，但空行进字幕文件是难查的错。
    out.retain(|piece| !piece.trim().is_empty());
    if out.is_empty() {
        vec![line.to_string()]
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sentence_boundary::preset::SubtitleLengthPreset;
    use crate::sentence_boundary::profile::profile_for_lang;
    use crate::sentence_boundary::subtitle_layout::build_subtitle_layout_split_points_text_only;

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
        // 远超标��（30+ 字 ≫ 22 + 2 宽限带）走强制档：每段都必须在硬上限之内。
        // 这条只断言**强制档**；软目标与宽限带的行为由下面两条分别钉住——它们合
        // 起来才是排版 DP 的完整三档策略。早先这里只断言 ≤22，等于把「只有最粗暴
        // 那一档」写成了不变式，正是本次要拆掉的东西。
        for l in &lines {
            assert!(l.chars().count() <= 22, "强制档不该留下超硬上限的行：{l:?}");
        }
    }

    /// 本次修复的核心断言：断点落在句末标点上，而不是长度预算上。
    ///
    /// 旧实现是「数字符、超 22 就切」，于是中文实测 92% 的断点落在句中——一句话
    /// 被从中间劈开，两半各自以标点结尾的完整句子反被拆散。现在切点先由语义句界
    /// （与转写路径**同一个** `build_split_points_from_hard_boundaries`）给出。
    #[test]
    fn a_long_cjk_line_prefers_sentence_boundaries_over_the_budget() {
        // 单行装了三句，每句都短于 22 字预算，合起来远超。
        let text = "第一句话在这里说完了。第二句话也不长。第三句话同样不长但是凑够字数了。";
        let out = break_long_lines(text, "zh", "standard");
        assert_only_newlines_changed(text, &out);
        let lines: Vec<&str> = out.split('\n').collect();
        assert_eq!(lines.len(), 3, "一句一行，不该被预算劈开：{lines:?}");
        for l in &lines {
            assert!(l.trim_end().ends_with('。'), "断点必须落在句末标点：{l:?}");
        }
        assert_eq!(lines[0], "第一句话在这里说完了。");
        assert_eq!(lines[1], "第二句话也不长。");
    }

    /// 强制档仍然生效：远远超预算、且整段找不到任何好切点时必须切干净。
    ///
    /// 与上一条互为对照——上一条说「有标点就听标点的」，这条说「没标点就按硬
    /// 上限切」。两条都在，才是被保留下来的完整三档策略。
    #[test]
    fn a_far_overlong_run_with_no_boundary_still_gets_cut() {
        // 一整段没有句末标点、也没有空格可切。
        let text: String = "文".repeat(120);
        let out = break_long_lines(&text, "zh", "standard");
        assert_only_newlines_changed(&text, &out);
        for l in out.split('\n') {
            assert!(l.chars().count() <= 22, "无好切点时必须强制切：{l:?}");
        }
    }

    /// 「没有时间戳」绝不能被读成「时间上粘在一起」。
    ///
    /// 这是本次改造里最容易复发、后果最重的坑：`is_time_glued_content` 在间距为 0
    /// 时会把两侧都是实词的边界判成粘连，而连带地把代价抬到最差、把这个切点从
    /// 候选里挤掉。若无时间戳被写成 `start == end == 0`，中文整行就一个切点都不
    /// 会剩——而这**不会报错**，只会让断句静默退化成"整段不切"。
    #[test]
    fn an_untimed_unit_is_never_treated_as_glued() {
        let text =
            "这是一段没有任何时间戳但是必须被切开的中文长句用来验证未计时单元不会被误判为粘连";
        let units = layout_units(text);
        let views: Vec<&str> = units.iter().map(String::as_str).collect();
        let profile = profile_for_lang("zh");
        let cuts = build_subtitle_layout_split_points_text_only(
            &views[..],
            &[(0, views.len() - 1)],
            profile.as_ref(),
            SubtitleLengthPreset::Standard,
        );
        assert!(
            !cuts.is_empty(),
            "未计时输入也必须能切：若全被判成粘连，这里会是空的"
        );
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
