//! Contract tests: pipeline must use real Qwen ASR + aligner + sentence boundary.

#[test]
fn process_source_uses_qwen_asr_align_and_srt() {
    let src = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/asr.rs"));
    assert!(
        src.contains("qwen3_asr") || src.contains("AsrInference"),
        "pipeline must use Qwen AsrInference"
    );
    assert!(
        src.contains("Qwen3ForcedAligner") || src.contains("qwen_forced_aligner"),
        "pipeline must use Qwen forced aligner"
    );
    assert!(
        src.contains("build_source_sentences_from_words"),
        "pipeline must run VoxTrans-style sentence boundary"
    );
    assert!(
        src.contains("source_sentences_to_srt"),
        "pipeline must export SRT from sentences"
    );
    assert!(
        src.contains("drop(asr)") || src.contains("drop(asr)"),
        "pipeline must unload ASR before aligner"
    );
    assert!(
        !src.contains("moss_transcribe_diarize"),
        "pipeline must not depend on MOSS"
    );
    assert!(
        !src.contains("format!(\"[stub]"),
        "must not format stub SRT body"
    );
}
