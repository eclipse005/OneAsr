//! VoxTrans sentence-boundary pipeline (ported): hard punct split + DP layout.

use crate::subtitle::beautify::beautify_words_for_subtitle;

mod assembly;
mod language;
mod punkt_map;
mod semantic;
mod subtitle_layout;
mod text;
mod timing;
mod types;
mod vad_align;
mod words;

use assembly::{
    build_boundaries_from_split_points, build_micro_chunks, build_sentences_from_word_spans,
};
use semantic::{build_split_points_from_hard_boundaries, split_points_to_spans};
use subtitle_layout::build_subtitle_layout_split_points;
use types::SourceSentenceStep2;
use words::{from_core_words, to_core_words};

pub use assembly::source_sentences_to_srt;
pub use types::{BoundaryDecisionKind, SentenceBoundaryRequest, SourceSentenceStep2 as SourceSentences};

/// Word token with timestamps (same shape as VoxTrans `WordTokenDto`).
#[derive(Debug, Clone)]
pub struct WordTokenDto {
    pub start: f64,
    pub end: f64,
    pub word: String,
}

/// Build subtitle sentences from aligned words (sync; same logic as VoxTrans step2).
pub fn build_source_sentences_from_words(
    request: SentenceBoundaryRequest,
) -> Result<SourceSentenceStep2, String> {
    if request.words.is_empty() {
        return Err("words is empty".to_string());
    }

    let normalized_words = from_core_words(beautify_words_for_subtitle(to_core_words(
        request.words.clone(),
    )));
    if normalized_words.is_empty() {
        return Err("words is empty".to_string());
    }

    let vad_index = vad_align::SpeechSegmentIndex::new(request.vad_speech_segments.clone());
    let profile = language::profile_for_lang(&request.source_lang);
    let preset =
        crate::subtitle_length::subtitle_length_preset_from_id(&request.subtitle_length_preset);

    let micro_chunks = build_micro_chunks(&normalized_words, &vad_index);
    if micro_chunks.is_empty() {
        return Err("failed to build micro chunks".to_string());
    }

    let hard_split_points =
        build_split_points_from_hard_boundaries(&normalized_words, &*profile);
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
    })
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
