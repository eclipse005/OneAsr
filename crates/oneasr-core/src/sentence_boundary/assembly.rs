//! Assembling finished source sentences into exported text (SRT / TXT).

use crate::sentence_boundary::WordTokenDto;
use crate::subtitle::srt::{SrtCue, to_srt_from_cues};

use super::util::join_words;
use super::util::{gap_ms, seconds_to_ms};
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

/// Plain-text transcript: one line per finished cue, with layout line breaks
/// folded back together using the normal CJK / latin spacing rules.
pub fn source_sentences_to_txt(step2: &SourceSentenceStep2) -> String {
    let mut out = String::new();
    for sentence in &step2.translation_sentences {
        let line = join_words(sentence.text.lines());
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
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

#[cfg(test)]
mod txt_tests {
    use super::*;
    use crate::sentence_boundary::SourceSentence;

    fn step2(texts: &[&str]) -> SourceSentenceStep2 {
        SourceSentenceStep2 {
            task_id: "t".into(),
            media_path: "m.mp4".into(),
            source_lang: "zh".into(),
            micro_chunk_total: 0,
            boundary_total: 0,
            sentence_total: texts.len(),
            micro_chunks: Vec::new(),
            boundaries: Vec::new(),
            translation_sentences: texts
                .iter()
                .enumerate()
                .map(|(i, text)| SourceSentence {
                    sentence_id: i + 1,
                    start_ms: (i as u64) * 1000,
                    end_ms: (i as u64) * 1000 + 900,
                    text: (*text).to_string(),
                    word_start: 0,
                    word_end: 1,
                    chunk_start: 0,
                    chunk_end: 1,
                })
                .collect(),
            words: Vec::new(),
        }
    }

    #[test]
    fn one_line_per_cue_without_timestamps() {
        let txt = source_sentences_to_txt(&step2(&["第一句。", "第二句。"]));
        assert_eq!(txt, "第一句。\n第二句。\n");
        assert!(!txt.contains("-->"));
        assert!(!txt.contains("00:00"));
    }

    #[test]
    fn layout_line_breaks_are_folded_back() {
        // CJK lines join without a space, latin lines keep one (same rule as
        // cue assembly), so the plain-text export reads like prose.
        let cjk = source_sentences_to_txt(&step2(&["我在这\n里等你。"]));
        assert_eq!(cjk, "我在这里等你。\n");

        let latin = source_sentences_to_txt(&step2(&["hello\nworld"]));
        assert_eq!(latin, "hello world\n");
    }

    #[test]
    fn empty_cues_are_skipped() {
        let txt = source_sentences_to_txt(&step2(&["", "  ", "有内容。"]));
        assert_eq!(txt, "有内容。\n");
    }

    #[test]
    fn no_cues_yields_empty_body() {
        assert!(source_sentences_to_txt(&step2(&[])).is_empty());
    }
}
