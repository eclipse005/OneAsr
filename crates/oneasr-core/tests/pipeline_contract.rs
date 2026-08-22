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
    let transcribe = src
        .find("fn stage_transcribe_all")
        .expect("stage_transcribe_all");
    let align = src.find("fn stage_align_all").expect("stage_align_all");
    let asr_load = src
        .find("AsrInference::load")
        .expect("AsrInference::load");
    let align_load = src
        .find("Qwen3ForcedAligner::load")
        .expect("Qwen3ForcedAligner::load");
    assert!(
        transcribe < asr_load && asr_load < align,
        "AsrInference must be local to stage_transcribe_all (dropped before aligner)"
    );
    assert!(
        align < align_load,
        "Qwen3ForcedAligner must load in stage_align_all after ASR has returned"
    );
}
