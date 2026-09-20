//! Writing the product output (SRT / TXT / words JSON) and the small text helpers the export stage needs.

use super::AsrError;
use std::path::{Path, PathBuf};

use crate::diagnostics::trace_log;
use crate::media::write_atomic;
use crate::paths::output_path;
use crate::subtitle::alignment::align_text_to_timestamps;
use crate::subtitle::segmenter::WordToken;

/// Write one export file into `target_dir`; on failure fall back to the
/// configured output dir (same policy as the original SRT-only write).
pub(super) fn write_export_file(
    target_dir: &Path,
    fallback_dir: &Path,
    stem: &str,
    ext: &str,
    body: &str,
) -> Result<PathBuf, AsrError> {
    let mut path = output_path(target_dir, stem, ext);
    if let Err(e) = write_atomic(&path, body) {
        if target_dir == fallback_dir {
            return Err(e.into());
        }
        eprintln!(
            "warning: .{ext} write failed in {} ({e}); saved to fallback {} instead",
            target_dir.display(),
            fallback_dir.display()
        );
        trace_log(format!(
            "{ext} fallback: {} → {}",
            target_dir.display(),
            fallback_dir.display()
        ));
        path = output_path(fallback_dir, stem, ext);
        write_atomic(&path, body)?;
    }
    Ok(path)
}

/// Word/char tokens after ForcedAligner + punct restore + normalize (pre-sentence-boundary).
pub(super) fn write_words_json(
    path: &Path,
    stem: &str,
    media_name: &str,
    lang: &str,
    words: &[WordToken],
) -> Result<(), AsrError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let items: Vec<serde_json::Value> = words
        .iter()
        .map(|w| {
            serde_json::json!({
                "text": w.word,
                "start": w.start,
                "end": w.end,
            })
        })
        .collect();
    let body = serde_json::json!({
        "id": stem,
        "media": media_name,
        "lang": lang,
        "unit": "aligner_token",
        "note": "ForcedAligner tokens after punctuation restore + normalize_word_tokens; not sentence cues",
        "word_count": items.len(),
        "words": items,
    });
    let text = serde_json::to_string_pretty(&body)
        .map_err(|e| AsrError::Other(format!("words json: {e}")))?;
    write_atomic(path, &text).map_err(AsrError::Io)
}

/// Chunks shorter than this have no usable mel frames to align against.
pub(super) const MIN_ALIGN_SEC: f32 = 0.1;

/// The Qwen aligner strips punctuation before tokenizing, so a chunk whose
/// text is punctuation-only yields zero words → zero `<timestamp>` slots →
/// the timestamp gather would run on an empty set of slots. Detect such
/// chunks and skip the model call.
pub(super) fn has_alignable_word(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

pub(super) fn attach_transcript_punctuation(
    transcript_text: &str,
    aligned_words: &[WordToken],
) -> Vec<WordToken> {
    if transcript_text.trim().is_empty() || aligned_words.is_empty() {
        return aligned_words.to_vec();
    }
    let mapped = align_text_to_timestamps(transcript_text, aligned_words);
    if mapped.len() == aligned_words.len() {
        mapped
    } else {
        aligned_words.to_vec()
    }
}
