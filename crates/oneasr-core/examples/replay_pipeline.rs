//! Replay the sentence-boundary pipeline from a saved `--words-json` dump,
//! without re-running ASR. Trimmed port of voxtrans' `replay_pipeline.rs`
//! (source-only: OneAsr has no translation/DB layer, and the CLI's words
//! dump does not include VAD segments).
//!
//! Usage:
//!   cargo run -p oneasr-core --example replay_pipeline -- <words.json> [short|standard|loose]
//!
//! `words.json` is what `oneasr-cli --words-json <path>` writes:
//! `{ id, media, lang, words: [{text, start, end}, ...] }`.

use std::fs;
use std::path::Path;
use std::time::Instant;

use oneasr_core::sentence_boundary::{
    SentenceBoundaryRequest, WordTokenDto, build_source_sentences_from_words,
    source_sentences_to_srt,
};

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WordJson {
    id: String,
    #[serde(default)]
    media: String,
    #[serde(default)]
    lang: String,
    words: Vec<WordItem>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WordItem {
    text: String,
    start: f64,
    end: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let words_path = args.get(1).cloned().unwrap_or_else(|| {
        eprintln!("usage: replay_pipeline <words.json> [short|standard|loose]");
        std::process::exit(2);
    });
    let preset = args.get(2).map(String::as_str).unwrap_or("standard");

    if let Err(err) = run(&words_path, preset) {
        eprintln!("replay failed: {err}");
        std::process::exit(1);
    }
}

fn run(words_path: &str, preset: &str) -> Result<(), String> {
    let dump: WordJson = serde_json::from_str(
        &fs::read_to_string(words_path).map_err(|e| format!("read words json: {e}"))?,
    )
    .map_err(|e| format!("parse words json: {e}"))?;
    if dump.words.is_empty() {
        return Err("words json has no words".into());
    }

    let words: Vec<WordTokenDto> = dump
        .words
        .into_iter()
        .map(|w| WordTokenDto {
            start: w.start,
            end: w.end,
            word: w.text,
        })
        .collect();

    eprintln!(
        "replay start task={} words={} lang={:?} preset={preset}",
        dump.id,
        words.len(),
        if dump.lang.is_empty() { None } else { Some(&dump.lang) }
    );

    let request = SentenceBoundaryRequest {
        task_id: dump.id.clone(),
        media_path: dump.media.clone(),
        source_lang: dump.lang,
        subtitle_length_preset: preset.to_string(),
        // The CLI words dump does not save VAD speech segments; passes through
        // the boundary rules without VAD-gap information.
        vad_speech_segments: Vec::new(),
        words,
    };

    let step2_t = Instant::now();
    let step2 = build_source_sentences_from_words(request)?;
    eprintln!(
        "step2 done cues={} boundaries={} in {:.1}s",
        step2.sentence_total,
        step2.boundary_total,
        step2_t.elapsed().as_secs_f64()
    );

    let srt = source_sentences_to_srt(&step2);
    if srt.trim().is_empty() {
        return Err("sentence boundary produced empty SRT".into());
    }

    let dump_path = Path::new(words_path);
    let out_path = dump_path.with_file_name(format!(
        "replay_{}.srt",
        dump_path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| step2.task_id.clone())
    ));
    fs::write(&out_path, &srt).map_err(|e| format!("write srt: {e}"))?;
    eprintln!("wrote {}", out_path.display());
    Ok(())
}