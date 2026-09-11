//! Pipeline behaviour, exercised with in-memory engines.
//!
//! These replace the old source-string "contract" test: instead of grepping
//! `asr.rs` for `AsrInference`/`Qwen3ForcedAligner`, the pipeline runs for real
//! against fake engines and we assert what it actually does — stage order,
//! engine lifetime, export formats, script conversion and warnings.
//!
//! No weights, no GPU and no ffmpeg are involved: inputs and model dirs are
//! synthesized, and the fake separator writes the pipeline's own 16 kHz mono
//! master format so the transcode step takes its copy fast-path.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use oneasr_core::engine::AlignedToken;
use oneasr_core::engine::testing::{FakeProvider, FakeSeparation};
use oneasr_core::{ProcessExportOptions, Settings, StageUpdate, process_media_file_with_provider};

/// Per-test scratch root (`cargo test` runs tests in parallel threads).
fn scratch_root(label: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("oneasr_pipe_{}_{}_{n}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch root");
    dir
}

/// Write a 16 kHz mono PCM WAV (the pipeline's master format).
fn write_master_wav(path: &Path, seconds: f32) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).expect("create wav");
    for i in 0..(seconds * 16_000.0) as usize {
        // A quiet tone keeps the fixture realistic without needing ffmpeg.
        let v = ((i as f32 * 0.01).sin() * 200.0) as i16;
        writer.write_sample(v).expect("write sample");
    }
    writer.finalize().expect("finalize wav");
}

/// Fake model dirs that satisfy the custom-dir checks (name ≠ catalog folder).
fn install_fake_models(root: &Path, with_demucs: bool) -> (PathBuf, PathBuf, PathBuf) {
    let models = root.join("models");
    let asr = models.join("asr-test");
    let align = models.join("align-test");
    let demucs = models.join("demucs-test");
    for dir in [&asr, &align, &demucs] {
        std::fs::create_dir_all(dir).expect("create model dir");
    }
    let blob = vec![0u8; 4096];
    std::fs::write(asr.join("config.json"), &blob).unwrap();
    std::fs::write(asr.join("tokenizer.json"), &blob).unwrap();
    std::fs::write(asr.join("model.safetensors"), &blob).unwrap();
    std::fs::write(align.join("config.json"), &blob).unwrap();
    std::fs::write(align.join("model.safetensors"), &blob).unwrap();
    if with_demucs {
        std::fs::write(demucs.join("htdemucs_ft.safetensors"), &blob).unwrap();
    }
    (asr, align, demucs)
}

struct Run {
    settings: Settings,
    input: PathBuf,
    app_root: PathBuf,
}

impl Run {
    fn new(label: &str, with_demucs: bool) -> Self {
        let app_root = scratch_root(label);
        let (asr, align, demucs) = install_fake_models(&app_root, with_demucs);
        let input = app_root.join("sample.wav");
        write_master_wav(&input, 4.0);

        let settings = Settings {
            asr_model_dir: asr,
            aligner_model_dir: align,
            demucs_model_dir: demucs,
            output_dir: app_root.join("output"),
            save_next_to_source: false,
            language: "zh".into(),
            ..Settings::default()
        };
        Self {
            settings,
            input,
            app_root,
        }
    }

    fn run(&self, provider: &FakeProvider) -> (Vec<StageUpdate>, Result<PathBuf, String>) {
        let mut stages = Vec::new();
        let result = process_media_file_with_provider(
            &self.input,
            "sample.wav",
            &self.settings,
            &self.app_root,
            provider,
            |update: StageUpdate| stages.push(update),
            ProcessExportOptions::default(),
        )
        .map_err(|e| e.to_string());
        (stages, result)
    }
}

fn tokens() -> Vec<AlignedToken> {
    vec![
        AlignedToken {
            text: "你好".into(),
            start_sec: 0.20,
            end_sec: 1.10,
        },
        AlignedToken {
            text: "世界".into(),
            start_sec: 1.20,
            end_sec: 2.40,
        },
    ]
}

#[test]
fn engines_are_used_one_at_a_time() {
    let run = Run::new("one_at_a_time", false);
    let provider = FakeProvider::new("你好世界。", tokens());
    let (stages, result) = run.run(&provider);
    let srt = result.expect("pipeline should succeed");

    assert!(srt.ends_with("sample.srt"), "got {}", srt.display());
    let body = std::fs::read_to_string(&srt).unwrap();
    assert!(body.contains("你好世界。"), "SRT body: {body}");

    // The rule that used to be checked by grepping the source: ASR is dropped
    // before the aligner is created, so only one model is ever resident.
    let log = provider.log().entries();
    assert_eq!(
        log,
        vec![
            "asr.load",
            "asr.transcribe",
            "asr.drop",
            "aligner.load",
            "aligner.align",
            "aligner.drop",
        ],
        "engine lifecycle changed"
    );

    // Separation is off → the provider must not even be asked for one.
    assert_eq!(provider.separator_loads(), 0);

    // Stage stream still describes the run for the UI.
    let labels: Vec<String> = stages.iter().map(|s| s.label()).collect();
    assert!(labels.iter().any(|l| l == "转码音频"));
    assert!(labels.iter().any(|l| l == "加载识别模型"));
    assert!(labels.iter().any(|l| l == "加载对齐模型"));
}

#[test]
fn txt_only_writes_no_srt() {
    let mut run = Run::new("txt_only", false);
    run.settings.output_srt = false;
    run.settings.output_txt = true;
    let provider = FakeProvider::new("你好世界。", tokens());
    let (_stages, result) = run.run(&provider);

    let txt = result.expect("pipeline should succeed");
    assert!(txt.ends_with("sample.txt"), "got {}", txt.display());
    let body = std::fs::read_to_string(&txt).unwrap();
    assert_eq!(body, "你好世界。\n");
    assert!(
        !run.app_root.join("output/sample.srt").exists(),
        "SRT must not be written when it is switched off"
    );
}

#[test]
fn script_conversion_happens_after_alignment() {
    let mut run = Run::new("script", false);
    run.settings.text_script = "traditional".into();
    run.settings.output_txt = true;
    // Tokens must line up with the transcript so punctuation restore (the real
    // aligner strips it) reproduces the spoken sentence.
    let provider = FakeProvider::new(
        "我们测试。",
        vec![
            AlignedToken {
                text: "我们".into(),
                start_sec: 0.20,
                end_sec: 1.10,
            },
            AlignedToken {
                text: "测试".into(),
                start_sec: 1.20,
                end_sec: 2.40,
            },
        ],
    );
    let (_stages, result) = run.run(&provider);
    result.expect("pipeline should succeed");

    // Subtitle text is converted…
    let srt = std::fs::read_to_string(run.app_root.join("output/sample.srt")).unwrap();
    assert!(
        srt.contains("我們測試。"),
        "SRT must carry the converted sentence: {srt}"
    );

    // …while the engine still aligned the raw transcript.
    assert_eq!(provider.align_inputs(), vec!["我们测试。".to_string()]);
}

#[test]
fn separation_reports_progress_and_cpu_fallback() {
    let mut run = Run::new("separation", true);
    run.settings.vocal_separation = true;
    let provider = FakeProvider::new("你好世界。", tokens()).with_separation(FakeSeparation {
        progress_total: 3,
        fall_back_to_cpu: true,
        vocals_seconds: 4.0,
    });
    let (stages, result) = run.run(&provider);
    result.expect("pipeline should succeed with a fake separator");

    assert_eq!(provider.separator_loads(), 1, "separator must load once");
    let labels: Vec<String> = stages.iter().map(|s| s.label()).collect();
    assert!(
        labels.iter().any(|l| l == "人声分离 1/3") && labels.iter().any(|l| l == "人声分离 3/3"),
        "progress labels missing: {labels:?}"
    );
    assert!(
        stages
            .iter()
            .any(|s| s.warning.as_deref().is_some_and(|w| w.contains("改用 CPU"))),
        "CPU fallback must reach the UI as a warning"
    );

    // Separation runs before the master transcode.
    let sep = labels
        .iter()
        .position(|l| l.starts_with("人声分离"))
        .unwrap();
    let conv = labels.iter().position(|l| l == "转码音频").unwrap();
    assert!(sep < conv, "stage order changed: {labels:?}");
}
