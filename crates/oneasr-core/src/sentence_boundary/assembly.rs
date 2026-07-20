use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::srt::{SrtCue, to_srt_from_cues};

use super::text::join_words;
use super::timing::{gap_ms, seconds_to_ms};
use super::types::{
    BoundaryDecision, BoundaryDecisionKind, MicroChunk, SourceSentence, SourceSentenceStep2,
    SplitReason,
};
use super::vad_align::SpeechSegmentIndex;

pub fn source_sentences_to_srt(step2: &SourceSentenceStep2) -> String {
    let cues = step2
        .translation_sentences
        .iter()
        .map(|sentence| SrtCue {
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
    spans
        .iter()
        .filter_map(|(start, end)| {
            if *start >= words.len() || *end >= words.len() || start > end {
                return None;
            }
            Some((*start, *end))
        })
        .enumerate()
        .map(|(index, (start, end))| SourceSentence {
            sentence_id: index + 1,
            start_ms: seconds_to_ms(words[start].start),
            end_ms: seconds_to_ms(words[end].end.max(words[start].start)),
            text: join_words(words[start..=end].iter().map(|word| word.word.as_str())),
            word_start: start,
            word_end: end,
            chunk_start: start + 1,
            chunk_end: end + 1,
        })
        .collect()
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
