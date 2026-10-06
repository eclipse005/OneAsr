//! Sentence boundary: word stream → micro chunks → boundaries → cue spans.
//!
//! [`build_source_sentences_from_words`] is the single entry point. Each stage
//! lives in its own module, in pipeline order:
//!
//! ```text
//! beautify → digit glue → micro chunks → hard boundaries (rules / Punkt)
//!          → subtitle-length DP layout → watchability merge → cue spans
//! ```
//!
//! Per-language budgets that drive the DP live in `profile`; the
//! short/standard/loose presets they are keyed by live in `preset`.

use crate::subtitle::beautify::beautify_words_for_subtitle;
use serde::{Deserialize, Serialize};

mod assembly;
mod boundary_rules;
mod digit_glue;
mod line_break;
mod preset;
mod profile;
mod punkt_map;
mod semantic;
mod subtitle_layout;
#[cfg(test)]
mod tests;
mod types;
mod util;
mod vad_align;
mod watchability_merge;

use assembly::{
    build_boundaries_from_split_points, build_micro_chunks, build_sentences_from_word_spans,
};
use preset::subtitle_length_preset_from_id;
use semantic::{build_split_points_from_hard_boundaries, split_points_to_spans};
use subtitle_layout::build_subtitle_layout_split_points;
use types::SourceSentenceStep2;
use util::{from_core_words, to_core_words};
use watchability_merge::merge_watchability_spans;

pub use assembly::{source_sentences_to_srt, source_sentences_to_txt};
pub use types::{
    BoundaryDecisionKind, SentenceBoundaryRequest, SourceSentence,
    SourceSentenceStep2 as SourceSentences,
};
// The karaoke renderer has to place a hard space exactly where the cue text has
// a space, so it reads the same decision `join_words` is made of. Exposed
// crate-wide rather than public: it is an answer about *these* words, not a
// promise to anyone outside.
pub(crate) use util::{join_words, spacing_pieces};
// Test-only: the karaoke renderer's fixture builds the token list the aligners
// would hand over, which means asking the same question about the same scripts
// that the joiner asks. Nothing outside a test needs the answer.
pub use line_break::break_long_lines;
#[cfg(test)]
pub(crate) use util::is_no_space_script;

/// Word token with timestamps (same shape as VoxTrans `WordTokenDto`).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WordTokenDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

/// 归一化词序列：beautify（只删不加）、数字粘合。
///
/// 两条入口（自动断句 / 文稿分行）都走它，所以「文稿模式下词是怎么被清理的」
/// 和识别任务完全一致——文稿只决定**边界**，不决定词。
fn normalize_for_grouping(request: &SentenceBoundaryRequest) -> Vec<WordTokenDto> {
    digit_glue::unglue_fused_ja_copula(digit_glue::glue_asr_split_digits(from_core_words(
        beautify_words_for_subtitle(to_core_words(request.words.clone())),
    )))
}

pub fn build_source_sentences_from_words(
    request: SentenceBoundaryRequest,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }

    let normalized_words = normalize_for_grouping(&request);
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }

    let vad_index = vad_align::SpeechSegmentIndex::new(request.vad_speech_segments.clone());
    let profile = profile::profile_for_lang(&request.source_lang);
    let preset = subtitle_length_preset_from_id(&request.subtitle_length_preset);

    let micro_chunks = build_micro_chunks(&normalized_words, &vad_index);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }

    let hard_split_points = build_split_points_from_hard_boundaries(&normalized_words, &*profile);
    let semantic_spans = split_points_to_spans(normalized_words.len(), &hard_split_points);
    let split_points = merge_split_points(
        hard_split_points,
        build_subtitle_layout_split_points(
            &normalized_words,
            &semantic_spans,
            &*profile,
            preset,
            &vad_index,
        ),
    );
    let spans = split_points_to_spans(normalized_words.len(), &split_points);
    if spans.is_empty() {
        return Err("failed to build sentence spans".to_string());
    }
    let spans = merge_watchability_spans(&normalized_words, &spans, &*profile, preset);
    let split_points = split_points_from_spans(&spans, &split_points);

    let translation_sentences = build_sentences_from_word_spans(&normalized_words, &spans);
    let boundaries = build_boundaries_from_split_points(&micro_chunks, &split_points);

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        micro_chunk_total: micro_chunks.len(),
        boundary_total: boundaries.len(),
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries,
        translation_sentences,
        words: normalized_words,
    })
}

/// 文稿的**一行 = 一条字幕**：边界由文稿给，不再跑排版 DP。
///
/// 这是 `build_source_sentences_from_words` 的另一半形态。两条入口共享归一化、
/// 装配和 micro chunk；只有「span 从哪来」不同——自动模式从切分点来，文稿模式
/// 从文稿的行来。
///
/// 不跑排版 DP 是有意的：用户已经分好行了，再让一个代价函数去重排，等于把他
/// 刚做的决定悄悄抹掉。仍然过一遍 [`crate::sentence_boundary::line_break`] 的
/// 兜底是调用方的事（文稿界面上的「智能断句」提前做掉了）。
pub fn build_sentences_from_transcript(
    request: SentenceBoundaryRequest,
    transcript_text: &str,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }
    let normalized_words = normalize_for_grouping(&request);
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }
    let vad_index = vad_align::SpeechSegmentIndex::new(request.vad_speech_segments.clone());
    let micro_chunks = build_micro_chunks(&normalized_words, &vad_index);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }

    let spans = spans_from_transcript_lines(&normalized_words, transcript_text);
    if spans.is_empty() {
        return Err("文稿的行和词对不上".to_string());
    }
    let translation_sentences = build_sentences_from_word_spans(&normalized_words, &spans);
    if translation_sentences.is_empty() {
        return Err("文稿的行和词对不上".to_string());
    }

    Ok(SourceSentenceStep2 {
        task_id: request.task_id,
        media_path: request.media_path,
        source_lang: request.source_lang,
        micro_chunk_total: micro_chunks.len(),
        // 文稿模式的边界就是文稿的行，不存在「切分点」这一步。
        boundary_total: 0,
        sentence_total: translation_sentences.len(),
        micro_chunks,
        boundaries: Vec::new(),
        translation_sentences,
        words: normalized_words,
    })
}

/// 文稿的行 → 词的区间。**每个词恰好落进一个区间，不重不漏。**
///
/// 对齐器的 token 流是「文稿去掉空白之后的字符序列」——CJK 逐字、拉丁按词、
/// 标点跟在词里。所以按**非空白字符数**分配就行：一个词跨过了行尾，就整个算给
/// 前一行（宁可那一行长一点，也不把一个词劈成两半，两半各带半个时间戳）。
///
/// 文稿没覆盖到的词（文本被改过、或标点被规范化掉）并进最后一条，**绝不丢**——
/// 丢掉的词不会报错，只会让字幕无声地少几个字，那是最难查的一类错。
fn spans_from_transcript_lines(words: &[WordTokenDto], text: &str) -> Vec<(usize, usize)> {
    let weight = |w: &str| w.chars().filter(|c| !c.is_whitespace()).count();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut cursor = 0usize;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let want = weight(line);
        if want == 0 {
            continue;
        }
        let start = cursor;
        let mut got = 0usize;
        while cursor < words.len() {
            let w = words[cursor].word.trim();
            if w.is_empty() {
                cursor += 1;
                continue;
            }
            got += weight(w);
            cursor += 1;
            if got >= want {
                break;
            }
        }
        if cursor > start {
            spans.push((start, cursor - 1));
        }
    }
    if let Some(last) = spans.last_mut()
        && cursor < words.len()
    {
        last.1 = words.len() - 1;
    }
    spans
}

fn merge_split_points(
    mut base: Vec<(usize, types::SplitReason)>,
    extra: Vec<(usize, types::SplitReason)>,
) -> Vec<(usize, types::SplitReason)> {
    base.extend(extra);
    base.sort_by_key(|(index, reason)| (*index, split_reason_priority(*reason)));
    base.dedup_by_key(|(index, _)| *index);
    base
}

fn split_reason_priority(reason: types::SplitReason) -> u8 {
    match reason {
        types::SplitReason::TerminalPunctuation => 1,
        types::SplitReason::SubtitleLayout => 2,
    }
}

fn split_points_from_spans(
    spans: &[(usize, usize)],
    original: &[(usize, types::SplitReason)],
) -> Vec<(usize, types::SplitReason)> {
    if spans.len() < 2 {
        return Vec::new();
    }
    let mut original_by_end = std::collections::HashMap::<usize, types::SplitReason>::new();
    for (end, reason) in original.iter().copied() {
        original_by_end.entry(end).or_insert(reason);
    }
    spans
        .iter()
        .take(spans.len() - 1)
        .map(|(_, end)| {
            (
                *end,
                original_by_end
                    .get(end)
                    .copied()
                    .unwrap_or(types::SplitReason::SubtitleLayout),
            )
        })
        .collect()
}
