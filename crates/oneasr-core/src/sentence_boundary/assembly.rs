//! Assembling finished source sentences into subtitle text (SRT).
//!
//! TXT never comes through here: it is not a subtitle without timestamps but a
//! plain sentence list built from the text ([`crate::txt`]).

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::srt::to_srt_from_cues;
use crate::timeline::Cue;

use super::types::{
    BoundaryDecision, BoundaryDecisionKind, MicroChunk, SourceSentence, SourceSentenceStep2,
    SplitReason,
};
use super::util::join_words;
use super::util::{gap_ms, seconds_to_ms};
use super::vad_align::SpeechSegmentIndex;

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| Cue {
            index: sentence.sentence_id,
            start_ms: sentence.start_ms,
            end_ms: sentence.end_ms,
            text: sentence.text.clone(),
        })
        .collect::<Vec<_>>();
    to_srt_from_cues(&cues)
}

pub(super) fn build_micro_chunks(
    words: &[WordTokenDto],
    vad_index: &SpeechSegmentIndex,
) -> Vec<MicroChunk> {
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let gap_before_ms = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| gap_ms(prev.end, word.start))
                .unwrap_or(0);
            let gap_after_ms = words
                .get(index + 1)
                .map(|next| gap_ms(word.end, next.start))
                .unwrap_or(0);
            let hard_split_before = index
                .checked_sub(1)
                .and_then(|prev| words.get(prev))
                .map(|prev| vad_index.crosses_silence(prev.end, word.start))
                .unwrap_or(false);
            let hard_split_after = words
                .get(index + 1)
                .map(|next| vad_index.crosses_silence(word.end, next.start))
                .unwrap_or(false);
            MicroChunk {
                chunk_id: index + 1,
                start_ms: seconds_to_ms(word.start),
                end_ms: seconds_to_ms(word.end.max(word.start)),
                text: word.word.clone(),
                word_start: index,
                word_end: index,
                gap_before_ms,
                gap_after_ms,
                hard_split_before,
                hard_split_after,
            }
        })
        .collect()
}

pub(super) fn build_sentences_from_word_spans(
    words: &[WordTokenDto],
    spans: &[(usize, usize)],
) -> Vec<SourceSentence> {
    let mut out: Vec<SourceSentence> = Vec::with_capacity(spans.len());
    for (start, end) in spans.iter().copied() {
        if let Some((start_ms, end_ms)) = word_span_time(words, start, end) {
            out.push(SourceSentence {
                sentence_id: out.len() + 1,
                start_ms,
                end_ms,
                text: join_words(words[start..=end].iter().map(|word| word.word.as_str())),
                word_start: start,
                word_end: end,
                chunk_start: start + 1,
                chunk_end: end + 1,
            });
        }
    }
    out
}

/// 文稿的一行，以及它分到的那段词。
///
/// `text` 是**文稿那一行的原文**，不是从词拼出来的：文稿模式的字幕文字由用户
/// 决定，行内的空格、全角空格、全角标点都原样保留（见
/// [`build_sentences_from_lines`]）。`start`/`end` 只负责把时间对上。
pub(super) struct LineSpan<'a> {
    pub start: usize,
    pub end: usize,
    pub text: &'a str,
}

/// 文稿模式：一条字幕 = 文稿的一行，**文字逐字取原文**，只有时间来自词。
///
/// 与 [`build_sentences_from_word_spans`] 的差别只有文字来源。那条路拼 `join_words`
/// ——空格按书写系统猜（汉字之间不插），所以用户自己排的字距会消失；这条路不猜，
/// 因为文稿是用户写的，机器不该改它一个字。
pub(super) fn build_sentences_from_lines(
    words: &[WordTokenDto],
    lines: &[LineSpan<'_>],
) -> Vec<SourceSentence> {
    let mut out: Vec<SourceSentence> = Vec::with_capacity(lines.len());
    for line in lines {
        let (start, end) = (line.start, line.end);
        let Some((start_ms, end_ms)) = word_span_time(words, start, end) else {
            continue;
        };
        out.push(SourceSentence {
            sentence_id: out.len() + 1,
            start_ms,
            end_ms,
            text: line.text.to_string(),
            word_start: start,
            word_end: end,
            chunk_start: start + 1,
            chunk_end: end + 1,
        });
    }
    out
}

/// 一段词的起止时间；区间越界（词流被文稿超用）时返回 `None`。
fn word_span_time(words: &[WordTokenDto], start: usize, end: usize) -> Option<(u64, u64)> {
    if start >= words.len() || end >= words.len() || start > end {
        return None;
    }
    Some((
        seconds_to_ms(words[start].start),
        seconds_to_ms(words[end].end.max(words[start].start)),
    ))
}

pub(super) fn build_boundaries_from_split_points(
    micro_chunks: &[MicroChunk],
    split_points: &[(usize, SplitReason)],
) -> Vec<BoundaryDecision> {
    if micro_chunks.len() < 2 {
        return Vec::new();
    }

    let mut split_by_end = std::collections::HashMap::<usize, SplitReason>::new();
    for (end, reason) in split_points.iter().copied() {
        split_by_end.insert(end, reason);
    }

    (0..micro_chunks.len() - 1)
        .map(|index| {
            let left = &micro_chunks[index];
            let right = &micro_chunks[index + 1];
            let split_reason = split_by_end.get(&index).copied();
            let (rule_decision, llm_decision, final_decision, confidence, reason_tag) =
                match split_reason {
                    Some(SplitReason::TerminalPunctuation) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        1.0,
                        "terminal_punctuation",
                    ),
                    Some(SplitReason::SubtitleLayout) => (
                        BoundaryDecisionKind::Split,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Split,
                        0.9,
                        "subtitle_layout",
                    ),
                    None => (
                        BoundaryDecisionKind::Merge,
                        BoundaryDecisionKind::Unknown,
                        BoundaryDecisionKind::Merge,
                        0.95,
                        "merge",
                    ),
                };
            BoundaryDecision {
                left_chunk_id: left.chunk_id,
                right_chunk_id: right.chunk_id,
                gap_ms: gap_ms(
                    (left.end_ms as f64) / 1000.0,
                    (right.start_ms as f64) / 1000.0,
                ),
                rule_decision,
                llm_decision,
                final_decision,
                confidence,
                reason_tag: reason_tag.to_string(),
            }
        })
        .collect()
}
