//! Headless OneAsr CLI — same pipeline as the GUI worker.
//!
//! ```powershell
//! cargo run -p oneasr-core --release --bin oneasr-cli -- `
//!   transcribe --input "C:\path\to\video.mp4" --app-root "D:\OneAsr" `
//!   --language zh --chunk-seconds 60 --backend auto
//!
//! cargo run -p oneasr-core --release --bin oneasr-cli -- `
//!   asr-chunk --wav "D:\OneAsr\runs\...\input_16k.wav" `
//!   --start 722.75 --end 842.75 --language zh
//! ```
//!
//! Layout: `--app-root` is the **install** directory (`bin/ffmpeg`) and keeps its
//! old meaning; `--data-root` (or `ONEASR_DATA_DIR`) pins the **data** directory
//! that owns `models/`, `output/`, `runs/` and `settings.json`. Without
//! `--data-root` the two are the same, exactly as before.
//!
//! Env: `ONEASR_PIPELINE_TRACE=1` prints per-chunk ASR/align detail.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use oneasr_core::i18n;
use oneasr_core::media::slice_wav;
use oneasr_core::model::HTDEMUCS_FT;
use oneasr_core::{
    ModelId, ProcessExportOptions, Settings, StageClock, StageUpdate, ffmpeg_source,
    process_media_file_with_export, resolve_app_root,
};
use qwen3_asr_wgpu::{AsrInference, Backend as AsrBackend, TranscribeOptions};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || is_help(&args[1]) {
        print_help();
        return if args.len() < 2 {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    // 线程池 / 线程优先级：这里**刻意不调用** `oneasr_core::init_runtime()`。
    // 它做的唯一一件事是"给 GPUI 事件循环留一个核"（为全局 rayon 池保留一个
    // 逻辑核），理由是让 UI 不被模型 CPU 算子饿死；CLI 没有事件循环要保护，
    // 一次性批处理把整台机器用满才是对的，ffmpeg 作为子进程也不需要别人让路。
    // `demote_current_thread()` 同理，那是 GUI worker 线程的优先级策略。

    // CLI 不读 settings.json（全部走参数），文案语言直接跟随系统语言——
    // 对所有子命令、所有输出生效：用法、参数校验错误、运行摘要都取自
    // `oneasr_core::i18n`，本文件里不出现硬编码文案（键值摘要的键名即 flag 名，
    // 两种语言一致，同样定义在 i18n）。
    oneasr_core::i18n::set_ui_lang(oneasr_core::i18n::detect_system_lang());

    // Default subcommand: bare flags act as `transcribe`.
    let (cmd, rest) = if args[1].starts_with('-') {
        ("transcribe", &args[1..])
    } else {
        (args[1].as_str(), &args[2..])
    };

    let result = match cmd {
        "transcribe" | "run" | "pipeline" => cmd_transcribe(rest),
        "asr-chunk" | "chunk" => cmd_asr_chunk(rest),
        "align" => cmd_align(rest),
        "render" => cmd_render(rest),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("{}", i18n::cli_unknown_command(other));
            print_help();
            Err(2)
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code as u8),
    }
}

/// `--help` 全文取自 `i18n::cli_help()`，两种语言各一份，改文案只改 i18n。
fn print_help() {
    eprintln!("{}", i18n::cli_help());
}

// ─── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let input = require_arg(args, "--input")?;
    let app_root = parse_app_root(args)?;
    // 数据目录必须在任何 `Settings::default()` / 路径解析之前钉住。
    let data_root = pin_data_root(args, &app_root)?;
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg(args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let output_copy = arg(args, "--output").map(PathBuf::from);
    let words_json = arg(args, "--words-json").map(PathBuf::from);
    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let no_srt = flag(args, "--no-srt");
    let script = arg(args, "--script");
    let vocal_separation = flag(args, "--vocal-separation");
    let demucs_model_dir = arg(args, "--demucs-model-dir").map(PathBuf::from);

    let input_path = PathBuf::from(&input);
    if !input_path.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&input_path));
        return Err(1);
    }
    ensure_ffmpeg(&app_root)?;

    let mut settings = Settings {
        language,
        ..Settings::default()
    };
    // `Settings::normalize()` below clamps the chunk target to the product range.
    settings.chunk_target_seconds = chunk_seconds;
    settings.backend = backend;
    if let Some(n) = max_new_tokens {
        settings.max_new_tokens = n;
    }
    settings.output_txt = want_txt;
    settings.output_ass = want_ass;
    settings.output_srt = !no_srt;
    if no_srt && !want_txt && !want_ass {
        eprintln!("{}", i18n::cli_no_srt_warning());
    }
    if let Some(s) = script {
        settings.text_script = s;
    }
    settings.vocal_separation = vocal_separation;
    // `--aligner ctc|qwen|<目录名>`：显式指定只认那一个，否则按 CTC→Qwen 扫描。
    let aligner_override = match arg(args, "--aligner").as_deref() {
        None => None,
        Some(raw) => match parse_aligner_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_aligner_unknown(raw));
                return Err(2);
            }
        },
    };
    apply_app_root_paths(&mut settings, &app_root, &data_root, aligner_override);
    if let Some(dir) = demucs_model_dir {
        settings.demucs_model_dir = dir;
    }
    settings.normalize();
    if let Err(e) = settings.can_start() {
        eprintln!("{e}");
        return Err(1);
    }

    eprintln!("{}", i18n::cli_transcribe_banner());
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_INPUT, &input_path));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_APP, &app_root));
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_DATA, &data_root));
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_ASR, &settings.asr_model_dir)
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_ALIGN, &settings.aligner_model_dir)
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_OUTPUT, &settings.resolved_output_dir())
    );
    eprintln!(
        "{} {}",
        i18n::cli_run_flags_decode(
            &settings.language,
            settings.chunk_target_seconds,
            &settings.backend,
            settings.max_new_tokens,
        ),
        i18n::cli_run_flags_output(
            settings.output_srt,
            settings.output_txt,
            settings.text_script_choice().label(i18n::ui_lang()),
            settings.vocal_separation,
        ),
    );

    let media_name = input_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input_path.display().to_string());

    let mut clock = StageClock::new();
    let t0 = Instant::now();
    let export = ProcessExportOptions {
        words_json: words_json.clone(),
    };
    let result = process_media_file_with_export(
        &input_path,
        &media_name,
        &settings,
        &data_root,
        |update: StageUpdate| {
            clock.note(&update);
            if let Some(w) = &update.warning {
                eprintln!("{}", i18n::cli_warn(w));
            }
            eprintln!("{}", i18n::cli_stage(&update.label(i18n::ui_lang())));
        },
        export,
    );
    let timing = clock.finish();
    let wall = t0.elapsed();

    match result {
        Ok(primary) => {
            eprintln!("{}", i18n::cli_ok_output(&primary));
            if let Some(dst) = output_copy {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(&primary, &dst).map_err(|e| {
                    eprintln!("{}", i18n::cli_copy_failed(&e.to_string()));
                    1
                })?;
                eprintln!("{}", i18n::cli_copied(&dst));
            }
            if let Some(wj) = words_json {
                eprintln!("{}", i18n::cli_words_written(&wj));
            }
            eprintln!(
                "{}",
                i18n::cli_timing(wall.as_secs_f64(), timing.total_ms, timing.stages.len())
            );
            for s in &timing.stages {
                eprintln!(
                    "{}",
                    i18n::cli_stage_row(
                        s.stage.label(i18n::ui_lang()),
                        &oneasr_core::format_process_ms(s.elapsed_ms)
                    )
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", i18n::cli_failed(wall.as_secs_f64(), &e.to_string()));
            for s in &timing.stages {
                eprintln!(
                    "{}",
                    i18n::cli_stage_row(
                        s.stage.label(i18n::ui_lang()),
                        &oneasr_core::format_process_ms(s.elapsed_ms)
                    )
                );
            }
            Err(1)
        }
    }
}

// ─── asr-chunk ────────────────────────────────────────────────────

fn cmd_asr_chunk(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let wav = PathBuf::from(require_arg(args, "--wav")?);
    let start: f32 = require_arg(args, "--start")?.parse().map_err(|_| {
        eprintln!("{}", i18n::cli_flag_not_number("--start"));
        2
    })?;
    let end: f32 = require_arg(args, "--end")?.parse().map_err(|_| {
        eprintln!("{}", i18n::cli_flag_not_number("--end"));
        2
    })?;
    if end <= start {
        eprintln!("{}", i18n::cli_end_before_start());
        return Err(2);
    }
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let out = arg(args, "--out").map(PathBuf::from);
    let backend_s = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let app_root = parse_app_root(args).unwrap_or_else(|_| default_app_root());
    let data_root = pin_data_root(args, &app_root)?;

    if !wav.is_file() {
        eprintln!("{}", i18n::cli_wav_not_found(&wav));
        return Err(1);
    }

    let mut settings = Settings {
        language,
        backend: backend_s.clone(),
        ..Settings::default()
    };
    if let Some(n) = max_new_tokens {
        settings.max_new_tokens = n;
    }
    apply_app_root_paths(&mut settings, &app_root, &data_root, None);
    settings.normalize();

    let tmp = env::temp_dir().join(format!(
        "oneasr_dump_{}_{}.wav",
        (start * 1000.0) as u32,
        (end * 1000.0) as u32
    ));
    // 拿到路径就交给守卫：下面任何一步提前返回（切片失败、路径非 UTF-8、
    // 转写失败、写 `--out` 失败）都不会把临时切片留在 %TEMP% 里。
    let tmp = TempFileGuard::new(tmp);
    slice_wav(&wav, start, end, tmp.path()).map_err(|e| {
        eprintln!("{}", i18n::cli_slice_failed(&e.to_string()));
        1
    })?;
    eprintln!(
        "{}",
        i18n::cli_slice_done(start, end, end - start, tmp.path())
    );

    let backend = match settings.backend.to_ascii_lowercase().as_str() {
        "cpu" => AsrBackend::Cpu,
        "gpu" => AsrBackend::Gpu,
        _ => AsrBackend::Auto,
    };
    let asr = AsrInference::load(&settings.asr_model_dir, backend).map_err(|e| {
        eprintln!("{}", i18n::load_asr_failed(&e.to_string()));
        1
    })?;
    let lang = oneasr_core::lang::to_qwen_language_label(&settings.language);
    let opts = TranscribeOptions::default()
        .with_max_new_tokens(settings.max_new_tokens)
        .with_language(lang);
    let path_str = tmp.path().to_str().ok_or_else(|| {
        eprintln!("{}", i18n::audio_path_not_utf8());
        1
    })?;
    let t0 = Instant::now();
    let report = asr.transcribe(path_str, opts).map_err(|e| {
        eprintln!("{}", i18n::cli_transcribe_failed(&e.to_string()));
        1
    })?;
    let text = report.text.trim();
    let chars = text.chars().count();
    eprintln!(
        "{}",
        i18n::cli_chars(
            chars,
            report.raw_output.chars().count(),
            t0.elapsed().as_secs_f64()
        )
    );
    eprintln!("{}", i18n::t(i18n::CLI_ASR_TEXT_BEGIN));
    println!("{text}");
    eprintln!("{}", i18n::t(i18n::CLI_ASR_TEXT_END));

    if let Some(p) = out {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&p, text.as_bytes()).map_err(|e| {
            eprintln!("{}", i18n::cli_write_failed(&e.to_string()));
            1
        })?;
        eprintln!("{}", i18n::cli_wrote(&p));
    }

    // 临时切片的清理交给 `TempFileGuard`（函数返回时删除，含上面每条早退路径）。
    Ok(())
}

/// `render` — Phase B on its own: present a measured timeline again, with no
/// model, no ffmpeg and no audio in sight.
///
/// This is the reason the timeline is a file. Changing the segment length, the
/// output script or the output directory used to mean re-running two models;
/// here it is a re-render in milliseconds, and the words it works from cannot
/// drift from the words the original run measured because they *are* those
/// words, on disk.
fn cmd_render(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let timeline_arg = require_arg(args, "--timeline")?;
    let timeline_path = PathBuf::from(&timeline_arg);
    if !timeline_path.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&timeline_path));
        return Err(1);
    }

    // `--preset` is checked here rather than parsed leniently downstream: a
    // typo that silently rendered the default would be indistinguishable from
    // the option having done nothing.
    let preset = match arg(args, "--preset") {
        None => None,
        Some(raw) => match raw.as_str() {
            "short" | "standard" | "loose" => Some(raw),
            other => {
                eprintln!("{}", i18n::cli_preset_unknown(other));
                return Err(2);
            }
        },
    };

    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let want_srt = !flag(args, "--no-srt");
    if !want_srt && !want_txt && !want_ass {
        eprintln!("{}", i18n::cli_no_srt_warning());
    }

    eprintln!("{}", i18n::cli_render_banner());
    let measured = oneasr_core::timeline::read_timeline(&timeline_path).map_err(|e| {
        eprintln!("{}", i18n::cli_timeline_read_failed(&e));
        1
    })?;
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_TIMELINE, &timeline_path));

    let script = match arg(args, "--script") {
        Some(raw) => oneasr_core::TextScript::from_id(&raw),
        None => oneasr_core::TextScript::Original,
    };
    // `--transcript` switches the boundary source: the transcript's own lines
    // become the cues. Re-presenting an aligned transcript therefore needs no
    // audio, no model and no re-alignment.
    let transcript = match arg(args, "--transcript") {
        None => None,
        Some(path) => {
            let path = PathBuf::from(path);
            match oneasr_core::transcript::read_transcript(&path) {
                Ok(t) => Some(t.text),
                Err(e) => {
                    eprintln!("{}", i18n::cli_transcript_read_failed(&e));
                    return Err(1);
                }
            }
        }
    };
    let rendered = oneasr_core::timeline::render(
        &measured,
        &oneasr_core::RenderOptions {
            preset,
            transcript,
            script,
            srt: want_srt,
            txt: want_txt,
            ass: want_ass,
        },
    )
    .map_err(|e| {
        eprintln!("{}", i18n::cli_render_failed(&e));
        1
    })?;

    // `clip.timeline.json` → `clip.srt`: the stem is the timeline's own, so a
    // re-render lands beside the timeline it came from unless told otherwise.
    let out_dir = arg(args, "--output")
        .map(PathBuf::from)
        .or_else(|| timeline_path.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = oneasr_core::paths::media_stem(&timeline_path);
    let stem = stem.strip_suffix(".timeline").unwrap_or(&stem);

    let mut wrote_any = false;
    for (body, ext) in [
        (&rendered.srt, "srt"),
        (&rendered.txt, "txt"),
        (&rendered.ass, "ass"),
    ] {
        let Some(body) = body.as_deref() else {
            continue;
        };
        if body.trim().is_empty() {
            eprintln!("{}", i18n::cli_render_empty(ext));
            continue;
        }
        let path = out_dir.join(format!("{stem}.{ext}"));
        oneasr_core::media::write_atomic(&path, body).map_err(|e| {
            eprintln!("{}", i18n::cli_write_failed(&e.to_string()));
            1
        })?;
        eprintln!("{}", i18n::cli_wrote(&path));
        wrote_any = true;
    }
    if !wrote_any {
        return Err(1);
    }
    Ok(())
}

/// 删除临时文件的 RAII 守卫。
///
/// `asr-chunk` 会在切片之后经历转写、写 `--out` 等多个可能提前 `?` 返回的步骤；
/// 只有成功路径才 `remove_file` 会把几 MB 的切片永久留在 `%TEMP%` 里。守卫让
/// 成功、失败、切片本身失败三种情况都清理，且不必在每个 `?` 旁边写一次。
struct TempFileGuard(PathBuf);

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

// ─── shared ───────────────────────────────────────────────────────

/// Bind model dirs + SRT output folder to the CLI's `--app-root` / `--data-root`.
///
/// SRT path is driven by `settings.output_dir` (not the pipeline `data_root`
/// argument alone), so headless runs must pin it here.
///
/// 目录扫描顺序 = 数据目录优先、安装目录兜底。
///
/// 这是**显式根**之间的选择，不是 `resolve_model_dir` 那条"显式覆盖不回头找安装
/// 目录"的规则：CLI 的 `--app-root` / `--data-root` 都是用户自己指的路，两边都查
/// 一遍最不容易"看不见已下好的模型"（老脚本把权重放在 `--app-root` 下，换到
/// `--data-root` 后权重在新家）；那里防的是程序自己猜出来的 exe 目录。
fn apply_app_root_paths(
    settings: &mut Settings,
    app_root: &Path,
    data_root: &Path,
    aligner_override: Option<ModelId>,
) {
    let roots: Vec<&Path> = if data_root == app_root {
        vec![data_root]
    } else {
        vec![data_root, app_root]
    };
    // Bind whichever catalog ASR variant is actually installed under the data
    // root (fp16 preferred over its int8 sibling); otherwise keep the default
    // path so the model check reports the missing directory.
    let candidates = ModelId::ASR_CHOICES
        .into_iter()
        .chain([ModelId::Qwen3Asr06BInt8, ModelId::Qwen3Asr17BInt8]);
    let found = roots.iter().find_map(|root| {
        let models = root.join("models");
        candidates
            .clone()
            .find(|id| models.join(id.as_str()).is_dir())
            .map(|id| (models.join(id.as_str()), id))
    });
    if let Some((dir, id)) = found {
        settings.asr_model = id.as_str().into();
        settings.asr_model_dir = dir;
    }
    // 对齐模型：`--aligner` 显式指定只认那一个；否则按 ALIGNER_CHOICES 顺序
    // （CTC 优先）扫描安装布局，都没有就保留默认路径让模型检查报缺目录。
    let align_candidates: Vec<ModelId> = match aligner_override {
        Some(id) => vec![id],
        None => ModelId::ALIGNER_CHOICES.into_iter().collect(),
    };
    let align = roots.iter().find_map(|root| {
        let models = root.join("models");
        align_candidates
            .iter()
            .find(|id| models.join(id.as_str()).is_dir())
            .map(|id| (models.join(id.as_str()), *id))
    });
    if let Some((dir, id)) = align {
        settings.aligner_model = id.as_str().into();
        settings.aligner_model_dir = dir;
    }
    let demucs = roots
        .iter()
        .map(|root| root.join("models").join(HTDEMUCS_FT))
        .find(|dir| dir.is_dir());
    if let Some(dir) = demucs {
        settings.demucs_model_dir = dir;
    }
    settings.output_dir = oneasr_core::paths::output_dir_under(data_root);
    // Headless runs keep writing to {data-root}/output (GUI's "next to source"
    // default would be surprising for batch scripts).
    settings.save_next_to_source = false;
}

/// `align` —— 音频 + 文稿 → 字幕，**整个识别阶段跳过**。
///
/// 与 `transcribe` 的差别只有一句：文字从哪来。所以这里没有识别模型的检查、
/// 没有语言模型的加载，阶段列表里也就**没有「转写」这一项**——那就是用户判断
/// 「这次是文稿匹配」的全部依据。
fn cmd_align(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let audio = PathBuf::from(require_arg(args, "--audio")?);
    if !audio.is_file() {
        eprintln!("{}", i18n::cli_input_not_found(&audio));
        return Err(1);
    }
    let text_path = PathBuf::from(require_arg(args, "--text")?);
    let transcript = oneasr_core::transcript::read_transcript(&text_path).map_err(|e| {
        eprintln!("{}", i18n::cli_transcript_read_failed(&e));
        1
    })?;

    let app_root = parse_app_root(args)?;
    // 数据目录必须在任何 `Settings::default()` / 路径解析之前钉住。
    let data_root = pin_data_root(args, &app_root)?;
    ensure_ffmpeg(&app_root)?;

    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg(args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let want_txt = flag(args, "--txt");
    let want_ass = flag(args, "--ass");
    let no_srt = flag(args, "--no-srt");
    if no_srt && !want_txt && !want_ass {
        eprintln!("{}", i18n::cli_no_srt_warning());
    }

    let mut settings = Settings {
        language,
        backend,
        vocal_separation: flag(args, "--vocal-separation"),
        save_next_to_source: false,
        ..Settings::default()
    };
    settings.chunk_target_seconds = chunk_seconds;
    settings.output_srt = !no_srt;
    settings.output_txt = want_txt;
    settings.output_ass = want_ass;
    if let Some(dir) = arg(args, "--demucs-model-dir") {
        settings.demucs_model_dir = PathBuf::from(dir);
    }
    // 文稿匹配固定用 CTC：整段一次对齐，块边界不伤词。见设计文档 §1.1。
    settings.select_aligner_model(oneasr_core::ModelId::OmniAsrCtc300M);
    settings.normalize();
    if let Err(e) = settings.can_start() {
        eprintln!("{e}");
        return Err(1);
    }

    eprintln!("{}", i18n::cli_align_banner());
    eprintln!("{}", i18n::cli_kv(i18n::CLI_KV_INPUT, &audio));
    eprintln!(
        "{} {}",
        i18n::cli_kv(i18n::CLI_KV_TRANSCRIPT, &text_path),
        i18n::cli_transcript_loaded(
            transcript.line_count(),
            transcript.char_count(),
            transcript.dropped_timecodes()
        )
    );
    eprintln!(
        "{}",
        i18n::cli_kv(i18n::CLI_KV_OUTPUT, &settings.resolved_output_dir())
    );

    let media_name = audio
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| audio.display().to_string());
    let mut clock = StageClock::new();
    let t0 = Instant::now();
    let result = oneasr_core::process_media_file_with_transcript_export(
        &audio,
        &media_name,
        &oneasr_core::asr::TranscriptInput {
            text: transcript.text,
            path: Some(text_path),
        },
        &settings,
        &data_root,
        |update: StageUpdate| {
            clock.note(&update);
            if let Some(w) = &update.warning {
                eprintln!("{}", i18n::cli_warn(w));
            }
            eprintln!("{}", i18n::cli_stage(&update.label(i18n::ui_lang())));
        },
        ProcessExportOptions { words_json: None },
    );
    let timing = clock.finish();
    match result {
        Ok(primary) => {
            eprintln!("{}", i18n::cli_ok_output(&primary));
            eprintln!(
                "{}",
                i18n::cli_timing(
                    t0.elapsed().as_secs_f64(),
                    timing.total_ms,
                    timing.stages.len()
                )
            );
            Ok(())
        }
        Err(e) => {
            eprintln!("{e}");
            Err(1)
        }
    }
}

/// `--aligner` 旗标值 → 目录 id。接受短别名 `ctc` / `qwen` 与完整目录名。
fn parse_aligner_arg(raw: &str) -> Option<ModelId> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("ctc") {
        return Some(ModelId::OmniAsrCtc300M);
    }
    if s.eq_ignore_ascii_case("qwen") {
        return Some(ModelId::QwenAlign06B);
    }
    ModelId::try_parse_aligner(s)
}

/// `--data-root`：给了就必须是目录（可以还不存在——第一次运行会在那里建
/// `models/`）；没给返回 `None`。
fn data_root_arg(args: &[String]) -> Result<Option<PathBuf>, i32> {
    let Some(s) = arg(args, "--data-root") else {
        return Ok(None);
    };
    let p = PathBuf::from(s);
    if p.exists() && !p.is_dir() {
        eprintln!("{}", i18n::cli_data_root_not_dir(&p));
        return Err(1);
    }
    Ok(Some(p))
}

/// 钉住数据目录并返回它：`--data-root` 优先，否则沿用 `--app-root`
/// （老语义：给了 `--app-root`，`models/`、`output/`、`runs/` 就都在它下面）。
///
/// 必须在任何 `Settings::default()` / 路径解析**之前**调用——数据目录在进程内
/// 只解析一次（`oneasr_core::paths`）。
fn pin_data_root(args: &[String], app_root: &Path) -> Result<PathBuf, i32> {
    let dir = match data_root_arg(args)? {
        Some(dir) => dir,
        None => app_root.to_path_buf(),
    };
    oneasr_core::paths::set_data_root_override(dir.clone());
    Ok(dir)
}

fn ensure_ffmpeg(app_root: &Path) -> Result<(), i32> {
    let bin = app_root.join("bin");
    if bin.join("ffmpeg.exe").is_file() || bin.join("ffmpeg").is_file() {
        return Ok(());
    }
    // No bundled copy — the pipeline itself falls back to `PATH`, so only fail
    // when that is empty too.
    match ffmpeg_source() {
        Some(source) => {
            eprintln!("{}", i18n::cli_ffmpeg_fallback(&bin, &source.to_string()));
            Ok(())
        }
        None => {
            eprintln!("{}", i18n::cli_ffmpeg_not_found(&bin));
            Err(1)
        }
    }
}

fn default_app_root() -> PathBuf {
    resolve_app_root().unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn parse_app_root(args: &[String]) -> Result<PathBuf, i32> {
    if let Some(s) = arg(args, "--app-root") {
        let p = PathBuf::from(s);
        if !p.is_dir() {
            eprintln!("{}", i18n::cli_app_root_not_dir(&p));
            return Err(1);
        }
        return Ok(p);
    }
    Ok(default_app_root())
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn require_arg(args: &[String], name: &str) -> Result<String, i32> {
    arg(args, name).ok_or_else(|| {
        eprintln!("{}", i18n::cli_missing_required(name));
        2
    })
}

fn is_help(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "help")
}
