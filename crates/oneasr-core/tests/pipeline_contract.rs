//! Contract tests against the shipped `asr` source (no self-include footguns).

#[test]
fn process_source_uses_real_asr_and_install_output() {
    let src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/asr.rs"
    ));
    assert!(
        src.contains("AsrInference"),
        "pipeline must use AsrInference"
    );
    assert!(
        src.contains("transcribe_wav"),
        "pipeline must call transcribe_wav"
    );
    assert!(
        src.contains("TranscriptEngine"),
        "pipeline must use TranscriptEngine for parse/export"
    );
    assert!(
        src.contains("output_srt_path"),
        "pipeline must use output_srt_path"
    );
    assert!(
        src.contains("app_root.join(\"output\")")
            || src.contains("output_srt_path(app_root")
            || src.contains("output_srt_path(app_root,"),
        "pipeline must write under install output/"
    );
    // Banned success templates (must not appear as string literals for written content).
    assert!(
        !src.contains("format!(\"[stub]"),
        "must not format stub SRT body"
    );
    // Chinese convert-only stub success line used in older code:
    let banned = ["仅", "完成", "转码"].concat();
    // Only fail if the old full stub phrase is present as a write template.
    assert!(
        !src.contains("仅完成转码"),
        "must not contain convert-only stub success phrase: {banned}"
    );
}
