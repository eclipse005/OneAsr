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

// 这里必须用 `cfg(debug_assertions)` 而不是 `cfg(test)`：集成测试是独立 crate，
// 把 oneasr-core 当普通依赖编译，`cfg(test)` 只在 lib 自己那一侧成立，在这一侧
// 恒为假 —— 那样整份文件被剔除、一个测试也不跑。`debug_assertions` 在调试/测试
// profile 下成立，与 `engine/mod.rs` 中 `engine::testing` 的门控对齐；
// `cargo test --release` 下本文件被整段 cfg 掉（release 构建里没有测试替身）。
#![cfg(debug_assertions)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use oneasr_core::engine::AlignedToken;
use oneasr_core::engine::testing::{FakeProvider, FakeSeparation};
use oneasr_core::{ProcessExportOptions, Settings, StageUpdate, process_media_file_with_provider};
use oneasr_core::{TranscriptInput, process_media_file_with_transcript};

/// Per-test scratch root (`cargo test` runs tests in parallel threads).
///
/// 目录在 [`ScratchRoot`] 被丢弃时删掉 —— 正常返回、断言失败、panic 都一样。
/// 以前这里只建不删：每跑一次 `cargo test` 就在 `%TEMP%` 里漏下 14 个
/// `oneasr_pipe_*` 目录（每个还带 models/ output/ runs/ 子树）。本地攒了
/// 两个多月一千多个才被发现，而 CI 上看不见 —— runner 每次全新，进程一结束
/// 临时目录跟着虚拟机一起没了，那种失效只有在反复跑测试的机器上才显形。
struct ScratchRoot(PathBuf);

impl std::ops::Deref for ScratchRoot {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchRoot {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch_root(label: &str) -> ScratchRoot {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir =
        std::env::temp_dir().join(format!("oneasr_pipe_{}_{}_{n}", label, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create scratch root");
    ScratchRoot(dir)
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
    std::fs::write(align.join("tokenizer.json"), &blob).unwrap();
    std::fs::write(align.join("tokenizer_config.json"), &blob).unwrap();
    std::fs::write(align.join("model.safetensors"), &blob).unwrap();
    if with_demucs {
        std::fs::write(demucs.join("htdemucs_ft_vocals.safetensors"), &blob).unwrap();
    }
    (asr, align, demucs)
}

struct Run {
    settings: Settings,
    input: PathBuf,
    app_root: ScratchRoot,
}

impl Run {
    fn new(label: &str, with_demucs: bool) -> Self {
        let app_root = scratch_root(label);
        let (asr, align, demucs) = install_fake_models(&app_root, with_demucs);
        let input = app_root.join("sample.wav");
        write_master_wav(&input, 4.0);

        let settings = Settings {
            asr_model_dir: asr,
            // FakeAligner 输出无标点的词序列，模拟的是 Qwen 对齐器 —— 显式选中，
            // 让管线走「标点从转写贴回」的 Qwen 路径（CTC 路径 token 自带标点）。
            aligner_model: oneasr_core::model::QWEN_ALIGN_06B.into(),
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
        self.run_with_export(provider, ProcessExportOptions::default())
    }

    fn run_with_export(
        &self,
        provider: &FakeProvider,
        export: ProcessExportOptions,
    ) -> (Vec<StageUpdate>, Result<PathBuf, String>) {
        let mut stages = Vec::new();
        let result = process_media_file_with_provider(
            &self.input,
            "sample.wav",
            &self.settings,
            &self.app_root,
            provider,
            |update: StageUpdate| stages.push(update),
            export,
        )
        .map_err(|e| e.to_string());
        (stages, result)
    }

    /// The transcript path: same pipeline, the text comes from the caller and
    /// the ASR stage never runs.
    fn run_with_transcript(
        &self,
        provider: &FakeProvider,
        transcript: &str,
    ) -> (Vec<StageUpdate>, Result<PathBuf, String>) {
        let mut stages = Vec::new();
        let input = TranscriptInput {
            text: transcript.to_string(),
            path: None,
        };
        let result = process_media_file_with_transcript(
            &self.input,
            "sample.wav",
            &input,
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

/// One aligned token per character — enough structure for the subtitle DP to
/// lay out an unpunctuated sentence, without having to hand-count timings.
fn long_tokens(text: &str) -> Vec<AlignedToken> {
    text.chars()
        .enumerate()
        .map(|(i, c)| AlignedToken {
            text: c.to_string(),
            start_sec: i as f64 * 0.30,
            end_sec: i as f64 * 0.30 + 0.25,
        })
        .collect()
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
    assert!(
        !run.settings
            .output_dir
            .join("sample.timeline.json")
            .exists(),
        "GUI-style default exports must not leave an internal timeline sidecar"
    );

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
    let labels: Vec<String> = stages
        .iter()
        .map(|s| s.label(oneasr_core::i18n::UiLang::Zh))
        .collect();
    assert!(labels.iter().any(|l| l == "转码音频"));
    assert!(labels.iter().any(|l| l == "加载识别模型"));
    assert!(labels.iter().any(|l| l == "加载对齐模型"));
}

#[test]
fn timeline_sidecar_is_written_only_when_explicitly_requested() {
    let run = Run::new("timeline_opt_in", false);
    let provider = FakeProvider::new("你好世界。", tokens());
    let (_, result) = run.run_with_export(
        &provider,
        ProcessExportOptions {
            words_json: None,
            timeline_json: true,
        },
    );
    result.expect("pipeline should succeed");
    assert!(
        run.settings
            .output_dir
            .join("sample.timeline.json")
            .is_file()
    );
}

#[test]
fn transcript_matching_rejects_txt_only_before_touching_the_pipeline() {
    let mut run = Run::new("transcript_txt_only", false);
    run.settings.output_srt = false;
    run.settings.output_ass = false;
    run.settings.output_txt = true;
    let provider = FakeProvider::new("unused", tokens());
    let (stages, result) = run.run_with_transcript(&provider, "你好世界。");
    assert!(
        result.is_err(),
        "a TXT-only match has no timestamped deliverable"
    );
    assert!(
        stages.is_empty(),
        "reject before conversion or model loading"
    );
    assert!(
        provider.log().entries().is_empty(),
        "no engine should be touched"
    );
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

/// TXT 不带时间轴，所以只勾 TXT 的任务根本不碰对齐器：引擎生命周期里没有
/// 它的加载与对齐，阶段列表里也没有那两个标签。
#[test]
fn txt_only_never_touches_the_aligner() {
    let mut run = Run::new("txt_no_align", false);
    run.settings.output_srt = false;
    run.settings.output_txt = true;
    let provider = FakeProvider::new("你好世界。", tokens());
    let (stages, result) = run.run(&provider);
    result.expect("pipeline should succeed");

    assert_eq!(
        provider.log().entries(),
        vec!["asr.load", "asr.transcribe", "asr.drop"],
        "TXT-only must not load or run the aligner"
    );
    assert!(
        provider.align_inputs().is_empty(),
        "nothing should have been aligned: {:?}",
        provider.align_inputs()
    );
    for stage in [
        oneasr_core::AsrStage::LoadingAligner,
        oneasr_core::AsrStage::Aligning,
    ] {
        assert!(
            !stages.iter().any(|s| s.stage == stage),
            "{stage:?} must not be reported on a TXT-only run"
        );
    }
}

/// TXT-only 连对齐模型都不校验：把它指向一个空目录，任务照样跑完。
#[test]
fn txt_only_does_not_require_the_aligner_model() {
    let mut run = Run::new("txt_no_align_model", false);
    run.settings.output_srt = false;
    run.settings.output_txt = true;
    // A path that exists but holds none of the files `check_aligner_model_dir`
    // asks for — the check must not even be attempted.
    let bogus = run.app_root.join("models").join("aligner-absent");
    std::fs::create_dir_all(&bogus).unwrap();
    run.settings.aligner_model_dir = bogus;
    assert!(
        run.settings.can_start().is_ok(),
        "TXT-only must not require the aligner model"
    );

    let provider = FakeProvider::new("你好世界。", tokens());
    let (_stages, result) = run.run(&provider);
    let txt = result.expect("pipeline should succeed without an aligner model");
    assert_eq!(std::fs::read_to_string(&txt).unwrap(), "你好世界。\n");
}

/// CLI 的默认形状 `--txt --no-srt` 仍写 timeline 侧车，所以对齐器是需要的。
/// 缺对齐模型必须在任何昂贵阶段（转码 / VAD / ASR）之前报出来，而不是等
/// 识别跑完才在打轴阶段失败——那是明显的 UX 回退。
#[test]
fn txt_with_timeline_checks_the_aligner_before_any_stage() {
    let mut run = Run::new("txt_timeline_no_aligner", false);
    run.settings.output_srt = false;
    run.settings.output_txt = true;
    let bogus = run.app_root.join("models").join("aligner-absent");
    std::fs::create_dir_all(&bogus).unwrap();
    run.settings.aligner_model_dir = bogus;

    let provider = FakeProvider::new("你好世界。", tokens());
    let (stages, result) = run.run_with_export(
        &provider,
        ProcessExportOptions {
            words_json: None,
            timeline_json: true,
        },
    );

    let err = result.expect_err("缺对齐模型必须立即失败");
    assert!(err.contains("不完整"), "{err}");
    assert!(
        provider.log().entries().is_empty(),
        "失败必须发生在任何引擎加载之前: {:?}",
        provider.log().entries()
    );
    assert!(stages.is_empty(), "失败必须发生在任何阶段之前: {stages:?}");
}

/// 文稿路径永远要打轴，它的对齐器校验同样提前：缺模型连转码都不开始。
#[test]
fn transcript_matching_checks_the_aligner_before_any_stage() {
    let mut run = Run::new("transcript_no_aligner", false);
    let bogus = run.app_root.join("models").join("aligner-absent");
    std::fs::create_dir_all(&bogus).unwrap();
    run.settings.aligner_model_dir = bogus;

    let provider = FakeProvider::new("你好世界。", tokens());
    let (stages, result) = run.run_with_transcript(&provider, "你好世界。");

    let err = result.expect_err("缺对齐模型必须立即失败");
    assert!(err.contains("不完整"), "{err}");
    assert!(
        provider.log().entries().is_empty(),
        "失败必须发生在任何引擎加载之前: {:?}",
        provider.log().entries()
    );
    assert!(stages.is_empty(), "失败必须发生在任何阶段之前: {stages:?}");
}

/// 同一段文字：字幕按 22 字切成多条，TXT 一行到底。
///
/// 这条是 TXT 与字幕分家的正面证据 —— 两边不再共用一套断行。
#[test]
fn txt_lines_are_sentences_not_subtitle_cues() {
    let spoken = "这是一个很长的句子里面一个标点都没有所以字幕会把它切成好几段但是纯文本只要一行";
    assert!(
        spoken.chars().count() > 22,
        "样本要长过字幕行长上限才有意义"
    );

    let mut run = Run::new("txt_vs_cue", false);
    run.settings.output_srt = true;
    run.settings.output_txt = true;
    let provider = FakeProvider::new(spoken, long_tokens(spoken));
    let (_stages, result) = run.run(&provider);
    result.expect("pipeline should succeed");

    let txt = std::fs::read_to_string(run.app_root.join("output/sample.txt")).unwrap();
    assert_eq!(
        txt.lines().count(),
        1,
        "无标点的长句在 TXT 里必须只有一行：{txt}"
    );
    assert_eq!(txt, format!("{spoken}\n"));

    // 同一段文字进了字幕就是多条 cue：两边确实各走各的。
    let srt = std::fs::read_to_string(run.app_root.join("output/sample.srt")).unwrap();
    assert!(
        srt.matches("-->").count() > 1,
        "字幕应当切成多条 cue：{srt}"
    );
}

/// 同时选 TXT 与 SRT 时，TXT 仍然按句末标点断，不因为有字幕就改口径。
#[test]
fn txt_keeps_punctuation_split_alongside_srt() {
    let mut run = Run::new("txt_with_srt", false);
    run.settings.output_srt = true;
    run.settings.output_txt = true;
    let provider = FakeProvider::new("第一句。第二句。", tokens());
    let (_stages, result) = run.run(&provider);
    result.expect("pipeline should succeed");

    let txt = std::fs::read_to_string(run.app_root.join("output/sample.txt")).unwrap();
    assert_eq!(txt, "第一句。\n第二句。\n");
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
    let labels: Vec<String> = stages
        .iter()
        .map(|s| s.label(oneasr_core::i18n::UiLang::Zh))
        .collect();
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

/// 文稿路径的那一段静默，是这个功能最贵的一处体验债：一份二十分钟的文稿要对着
/// 一个不动的「打轴中」等二十几秒。分母由对齐器自己数（CTC 数编码窗口），这里
/// 用假引擎把它数成 4 颗，钉住「引擎报的进度真的变成了阶段更新」。
#[test]
fn transcript_alignment_surfaces_the_aligners_own_progress() {
    let run = Run::new("align_progress", false);
    let provider = FakeProvider::new(
        "unused",
        vec![AlignedToken {
            text: "你好".into(),
            start_sec: 0.10,
            end_sec: 1.00,
        }],
    )
    .with_align_progress(4);
    let (stages, result) = run.run_with_transcript(&provider, "你好世界。");
    let output = result.expect("transcript pipeline should succeed");
    assert!(
        output.ends_with("sample.aligned.srt"),
        "got {}",
        output.display()
    );

    let align_chunks: Vec<(usize, usize)> = stages
        .iter()
        .filter(|s| s.stage == oneasr_core::AsrStage::Aligning)
        .filter_map(|s| s.chunk)
        .collect();
    assert_eq!(
        align_chunks,
        vec![(1, 4), (2, 4), (3, 4), (4, 4)],
        "every tick must reach the UI stage, in order"
    );
}

/// 反过来也要钉住：引擎没有分母时**不要**造一个。文稿路径上模型加载、导出这些
/// 阶段本来就没有块数，凭空补一个 `1/1` 会让界面显示一条永远满着的条。
#[test]
fn a_denominator_less_stage_keeps_its_chunk_empty() {
    let run = Run::new("align_no_progress", false);
    let provider = FakeProvider::new(
        "unused",
        vec![AlignedToken {
            text: "你好".into(),
            start_sec: 0.10,
            end_sec: 1.00,
        }],
    );
    let (stages, result) = run.run_with_transcript(&provider, "你好世界。");
    result.expect("transcript pipeline should succeed");

    for stage in &stages {
        if matches!(
            stage.stage,
            oneasr_core::AsrStage::LoadingAligner | oneasr_core::AsrStage::Exporting
        ) {
            assert_eq!(
                stage.chunk, None,
                "{:?} has no denominator and must not claim one",
                stage.stage
            );
        }
    }
}

// ─── 临时目录不许留残余 ─────────────────────────────────────────────
// 断骨锚点：把 `impl Drop for ScratchRoot` 整段删掉，这两条同时变红。

#[test]
fn a_scratch_root_is_removed_when_it_goes_out_of_scope() {
    let dir = {
        let s = scratch_root("cleanup_scope");
        assert!(s.is_dir(), "夹具根应当先被建出来");
        s.0.clone()
    };
    assert!(!dir.exists(), "离开作用域后临时目录必须已被删除：{dir:?}");
}

#[test]
fn a_scratch_root_is_removed_even_when_the_test_panics() {
    // 断言失败正是最该清理的时刻：本地反复跑测试的人，正是在失败之后才去看
    // %TEMP% 里堆了什么的。靠 `Drop` 而不是靠每个测试体末尾手写一句。
    let kept = std::sync::Arc::new(std::sync::Mutex::new(PathBuf::new()));
    let sink = kept.clone();
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // 预期中的 panic，不让它污染输出
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
        let s = scratch_root("cleanup_panic");
        *sink.lock().unwrap() = s.0.clone();
        panic!("测试中途炸了");
    }));
    std::panic::set_hook(prev);

    assert!(outcome.is_err(), "应当真的 panic 了才对");
    let dir = kept.lock().unwrap().clone();
    assert!(!dir.exists(), "panic 也必须把临时目录清掉：{dir:?}");
}
