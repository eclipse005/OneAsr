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
    GroupLookup, HotwordRun, HotwordStore, ModelId, ProcessExportOptions, Settings, StageClock,
    StageUpdate, ffmpeg_source, process_media_file_with_export, resolve_app_root,
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
    let no_timeline = flag(args, "--no-timeline");
    let script = arg(args, "--script");
    let vocal_separation = flag(args, "--vocal-separation");
    let demucs_model_dir = arg(args, "--demucs-model-dir").map(PathBuf::from);
    let hotword_group = arg(args, "--hotword-group");
    let hotword_words = arg(args, "--hotwords");
    let sensevoice_model_dir = arg(args, "--sensevoice-model-dir").map(PathBuf::from);

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
    // `--asr 0.6b|1.7b|<目录名>`：显式指定只认那一个，否则按 0.6B→1.7B、
    // fp16 优先的顺序扫描已装目录。与 `--aligner` 同一套优先级。
    let asr_override = match arg(args, "--asr").as_deref() {
        None => None,
        Some(raw) => match parse_asr_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_asr_unknown(raw));
                return Err(2);
            }
        },
    };
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
    apply_app_root_paths(
        &mut settings,
        &app_root,
        &data_root,
        asr_override,
        aligner_override,
    );
    if let Some(dir) = demucs_model_dir {
        settings.demucs_model_dir = dir;
    }
    if let Some(dir) = sensevoice_model_dir {
        settings.sensevoice_model_dir = dir;
    }
    // 热词只在这一趟识别里有消费者，所以**只有给了旗标才碰 hotword.json**——
    // 不碰它，普通转写就不会触发老配置迁移之类的写盘副作用。
    // `--hotword-group`（从库里查组）与 `--hotwords`（即席词表）二选一。
    if hotword_group.is_some() || hotword_words.is_some() {
        let store = load_hotwords_or_warn();
        settings.hotword_run = resolve_cli_hotwords(
            &store,
            hotword_group.as_deref(),
            hotword_words.as_deref(),
            i18n::ui_lang(),
        )?;
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
    if let Some(run) = &settings.hotword_run {
        // 只报条数、不回显组名（banner 会进日志、会被贴出去，组名是用户内容）。
        eprintln!("{}", i18n::cli_hotword_run(run.word_count()));
        // 语言不支持时管线会整趟跳过（粗转不跑、context 不注入）：选了组却
        // 什么都不说等于静默无视旗标，那条警告是「旗标真的被看见了」的凭据。
        if !oneasr_core::lang::supports_hotwords(&settings.language) {
            eprintln!(
                "{}",
                i18n::cli_warn(&i18n::cli_hotwords_language_skipped(&settings.language))
            );
        }
    }
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
        // The sidecar is what makes `render` possible, so it stays on by default.
        // `--no-timeline` drops it — and with it the only reason a TXT-only run
        // would need the aligner at all (see `needs_alignment`).
        timeline_json: !no_timeline,
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
    // 与 transcribe 同一个旗标：排查幻觉时能指定型号，才对得上 GUI 那边选的。
    let asr_override = match arg(args, "--asr").as_deref() {
        None => None,
        Some(raw) => match parse_asr_arg(raw) {
            Some(id) => Some(id),
            None => {
                eprintln!("{}", i18n::cli_asr_unknown(raw));
                return Err(2);
            }
        },
    };
    apply_app_root_paths(&mut settings, &app_root, &data_root, asr_override, None);
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

/// `render` 的 TXT 出口：已渲染的句子列表 → 纯文本正文。
///
/// 单独成函数，是为了让「句子 → TXT」这一步能被单测钉住（见文件末尾的
/// `rendered_txt_body_keeps_english_sentences_separate`）：接线退回先
/// `join("")` 再断句时，英文句子会粘成一行。
fn rendered_txt_body(sentences: &[oneasr_core::sentence_boundary::SourceSentence]) -> String {
    oneasr_core::txt::body_from_parts(sentences.iter().map(|s| s.text.as_str()))
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
            ass: want_ass,
            beautify: false,
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
    // TXT 不进字幕渲染：它只由文字按句末标点断行产出（`oneasr_core::txt`），
    // 与 SRT/ASS 是否同时产出无关。句子文本先按同一套空格规则拼接再整体断句
    // ——`join("")` 会把 "Hello world." 与下一句粘成一行。
    let txt_body = want_txt.then(|| rendered_txt_body(&rendered.sentences.translation_sentences));
    for (body, ext) in [
        (&rendered.srt, "srt"),
        (&txt_body, "txt"),
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
    asr_override: Option<AsrPick>,
    aligner_override: Option<ModelId>,
) {
    let roots: Vec<&Path> = if data_root == app_root {
        vec![data_root]
    } else {
        vec![data_root, app_root]
    };
    // 显式指定优先，扫描只是兜底 —— 两者对 ASR 与对齐器是同一套规则。
    //
    // 关键在「扫不到时怎么办」：不能什么都不做就留着 `Settings::default()`
    // 里的默认模型路径，那等于**悄悄退回另一个已装的模型**跑（点名 1.7B 却
    // 跑成 0.6B，最难发现的一种）。所以显式指定时先把「要哪个」钉死，扫不到
    // 就指向那个确实不存在的目录，让模型检查报「你选的型号没装」。
    //
    // 候选怎么排由 `AsrPick` 给：短别名（尺寸级）fp16 与 int8 都试，fp16 优先；
    // 完整目录名（具体级）只认它自己。两个变体都没装才退回钉死并报缺。
    if let Some(pick) = asr_override {
        // 候选在外层：fp16 优先是**全局**规则，不能被 root 顺序截胡。反过来写
        // （root 在外）时，`--data-root` 里只装了 int8 就会盖过 `--app-root` 里的
        // fp16。同一个候选仍按 root 顺序取（data_root 优先）。
        let candidates = pick.candidates();
        let found = candidates.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        match found {
            Some((dir, id)) => {
                settings.asr_model = id.as_str().into();
                settings.asr_model_dir = dir;
            }
            None => {
                let id = pick.primary();
                settings.asr_model = id.as_str().into();
                settings.asr_model_dir = roots[0].join("models").join(id.as_str());
            }
        }
    } else {
        // 没指定：按 ASR_CHOICES 顺序（0.6B 先）再带上两个 int8 变体扫描
        // 安装布局，fp16 优先于 int8。都没有就保留默认路径让模型检查报缺目录。
        // 候选同样在外层，理由见上面那支。
        let candidates: Vec<ModelId> = ModelId::ASR_CHOICES
            .into_iter()
            .chain([ModelId::Qwen3Asr06BInt8, ModelId::Qwen3Asr17BInt8])
            .collect();
        let found = candidates.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        if let Some((dir, id)) = found {
            settings.asr_model = id.as_str().into();
            settings.asr_model_dir = dir;
        }
    }
    // 对齐模型：与 ASR 同一套优先级 —— `--aligner` 显式指定只认那一个，
    // 扫不到就指向那个不存在的目录报缺，而不是退回默认的 CTC；没指定才按
    // ALIGNER_CHOICES 顺序（CTC 优先）扫描安装布局。
    if let Some(id) = aligner_override {
        settings.aligner_model = id.as_str().into();
        settings.aligner_model_dir = roots
            .iter()
            .map(|root| root.join("models").join(id.as_str()))
            .find(|dir| dir.is_dir())
            .unwrap_or_else(|| roots[0].join("models").join(id.as_str()));
    } else {
        // 与 ASR 那支同一个形状：候选在外层，引擎优先级（CTC 先）才是全局规则，
        // 不被 root 顺序截胡；同一个候选仍按 root 顺序取。
        let found = ModelId::ALIGNER_CHOICES.iter().find_map(|id| {
            roots
                .iter()
                .map(|root| root.join("models").join(id.as_str()))
                .find(|dir| dir.is_dir())
                .map(|dir| (dir, *id))
        });
        if let Some((dir, id)) = found {
            settings.aligner_model = id.as_str().into();
            settings.aligner_model_dir = dir;
        }
    }
    let demucs = roots
        .iter()
        .map(|root| root.join("models").join(HTDEMUCS_FT))
        .find(|dir| dir.is_dir());
    if let Some(dir) = demucs {
        settings.demucs_model_dir = dir;
    }
    // 热词粗转模型（SenseVoiceSmall）：与 demucs 同一个形状——数据目录优先、
    // 安装目录兜底。都没有就保留 `Settings::default()` 解析出的落点（那会再看
    // 一次 exe 安装目录），让 `can_start` 报缺。
    let sensevoice = roots
        .iter()
        .map(|root| root.join("models").join(ModelId::SenseVoiceSmall.as_str()))
        .find(|dir| dir.is_dir());
    if let Some(dir) = sensevoice {
        settings.sensevoice_model_dir = dir;
    }
    settings.output_dir = oneasr_core::paths::output_dir_under(data_root);
    // Headless runs keep writing to {data-root}/output (GUI's "next to source"
    // default would be surprising for batch scripts).
    settings.save_next_to_source = false;
}

/// 读热词库（`{data-root}/hotword.json`）；迁移 / 自愈 / 坏文件的原因进 stderr。
///
/// 只在真的要热词时才调用：普通转写不该顺带触发迁移之类的写盘副作用。
/// 「哪些算值得说的」的判据在 core 侧（`HotwordLoadReport::is_notable`），
/// 这里只负责问——不重新枚举字段。
fn load_hotwords_or_warn() -> HotwordStore {
    let (store, report) = HotwordStore::load();
    if report.is_notable() {
        eprintln!(
            "{}",
            i18n::cli_warn(&i18n::cli_hotword_store_issue(&report.summary()))
        );
    }
    store
}

/// `--hotword-group` / `--hotwords` → 本次运行的热词参数（`settings.hotword_run`）。
///
/// 纯函数：store 与语言都从参数进，单测不读进程级语言。错误直接打屏并返回
/// 退出码 2（用法 / 取值错误），与 `--asr` 那类旗标同一口径：
/// - 两个旗标同时给：二选一；
/// - 组找不到或重名（查找规则见 [`HotwordStore::group_by_id_or_name`]）——
///   报错里把可用组全列出来，照着改就行；
/// - `--hotwords` 解析后没有词条：按用例把它当「空组」放行等于静默不跑热词，
///   比说清楚更糟（与 `can_start` 对空组的立场一致）。
///
/// 组**存在但没词**不在这里拦：留给 `settings.can_start()` 用那条现成的
/// 「热词组「X」还没有词」报错，命令行的说法与界面逐字一致。
fn resolve_cli_hotwords(
    store: &HotwordStore,
    group: Option<&str>,
    words: Option<&str>,
    lang: i18n::UiLang,
) -> Result<Option<HotwordRun>, i32> {
    match (group, words) {
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            eprintln!("{}", i18n::cli_hotwords_both_flags());
            Err(2)
        }
        (Some(query), None) => match store.group_by_id_or_name(query) {
            GroupLookup::Found(g) => Ok(store.resolve_run(Some(&g.id), lang)),
            GroupLookup::Ambiguous(ids) => {
                eprintln!("{}", i18n::cli_hotword_group_ambiguous(query, &ids));
                Err(2)
            }
            GroupLookup::NotFound => {
                eprintln!(
                    "{}",
                    i18n::cli_hotword_group_unknown(query, &store.menu_options(lang))
                );
                Err(2)
            }
        },
        (None, Some(raw)) => {
            if oneasr_core::hotword::parse_word_list(raw).is_empty() {
                eprintln!("{}", i18n::cli_hotwords_empty());
                return Err(2);
            }
            // 即席词表没有组：调参沿用库里的（`hotword.json` 缺失时就是默认值）。
            Ok(Some(HotwordRun {
                group_id: "cli".into(),
                group_name: i18n::t(i18n::CLI_HOTWORD_INLINE).to_string(),
                text: raw.to_string(),
                top_k: store.top_k,
                threshold: store.threshold,
            }))
        }
    }
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
    // 热词旗标只服务识别阶段；align 整个跳过识别（文字来自文稿），给了也只会
    // 静默失效——说出来，别让用户对着「一点变化都没有」猜。
    if arg(args, "--hotword-group").is_some() || arg(args, "--hotwords").is_some() {
        eprintln!(
            "{}",
            i18n::cli_warn(i18n::t(i18n::CLI_ALIGN_HOTWORDS_IGNORED))
        );
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
    settings.subtitle_beautify = flag(args, "--beautify");
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
        ProcessExportOptions {
            words_json: None,
            timeline_json: true,
        },
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

/// `--asr` 点名的粒度：**尺寸**（`1.7b`）还是**具体权重目录**（`Qwen3-ASR-1.7B-int8`）。
///
/// 两者扫不到时的兜底规则不一样，所以不能都压成一个 `ModelId`：
/// - 尺寸：fp16 与 int8 都算候选（fp16 优先）——同尺寸的量化版不算「换了模型」；
/// - 具体目录名：就认它自己，没装就报缺，不许换成同尺寸的另一个变体。
///
/// 之前两种写法都返回 `ModelId`，尺寸级因此被当成目录级钉死：`--asr 1.7b`
/// 只找 `Qwen3-ASR-1.7B-hf`，机器上只装了 `-int8` 就报「模型目录不存在」，
/// 明明有 1.7B 却用不了（2026-10-14 真机冒烟撞到）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AsrPick {
    Size(ModelId),
    Exact(ModelId),
}

impl AsrPick {
    /// 扫不到时要指给用户看的那一个：尺寸级取 fp16 名，报错文案里才有型号。
    fn primary(self) -> ModelId {
        match self {
            Self::Size(id) | Self::Exact(id) => id,
        }
    }

    /// 目录扫描的候选 id，按优先级排。
    fn candidates(self) -> Vec<ModelId> {
        match self {
            Self::Size(id) => vec![id.asr_base(), id.with_quant(true)],
            Self::Exact(id) => vec![id],
        }
    }
}

/// `--asr` 旗标值 → [`AsrPick`]。接受短别名 `0.6b` / `1.7b` 与完整目录名。
///
/// 与 [`parse_aligner_arg`] 的差别：对齐器没有量化变体，`ctc` / `qwen` 指的
/// 就是引擎本身，两种写法一个意思；识别模型是「尺寸 × fp16/int8」两维，
/// 短别名只点了其中一维，所以得把粒度一起带下去（见 [`AsrPick`]）。
fn parse_asr_arg(raw: &str) -> Option<AsrPick> {
    let s = raw.trim();
    if s.eq_ignore_ascii_case("0.6b") {
        return Some(AsrPick::Size(ModelId::Qwen3Asr06B));
    }
    if s.eq_ignore_ascii_case("1.7b") {
        return Some(AsrPick::Size(ModelId::Qwen3Asr17B));
    }
    ModelId::try_parse_asr(s).map(AsrPick::Exact)
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

#[cfg(test)]
mod tests {
    use super::*;
    use oneasr_core::sentence_boundary::SourceSentence;

    /// 造一棵带模型的临时数据根。**四个变体全装上** —— 少建一个，
    /// 「扫描顺序」就测不出来：只装 0.6B-hf 与 1.7B-int8 时把候选顺序反过来，
    /// 第一个命中的仍然是 0.6B-hf，顺序断言会假绿。
    fn root_with_all_asr_variants(tag: &str) -> std::path::PathBuf {
        let root = env::temp_dir().join(format!("oneasr-asr-pick-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr06BInt8,
            ModelId::Qwen3Asr17BInt8,
        ] {
            std::fs::create_dir_all(root.join("models").join(id.as_str())).unwrap();
        }
        root
    }

    #[test]
    fn an_explicit_asr_flag_beats_the_installed_scan() {
        let root = root_with_all_asr_variants("explicit");
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8)),
            None,
        );
        // 断骨锚点：把 `asr_override` 那段 match 改成无条件扫描，这条立刻红。
        assert_eq!(s.asr_model, ModelId::Qwen3Asr17BInt8.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "显式指定必须赢过扫描到的 0.6B"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_the_flag_the_installed_scan_still_runs() {
        let root = root_with_all_asr_variants("scan");
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, None);
        // 显式指定之外的兜底没被改掉：ASR_CHOICES 里 0.6B 排在 1.7B 前面，
        // 两者都装了，所以必须绑 0.6B。断骨锚点：把候选顺序反过来，这条变红。
        assert_eq!(s.asr_model, ModelId::Qwen3Asr06B.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr06B.as_str())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_flag_naming_a_size_that_is_not_installed_is_not_silently_swapped() {
        // 选了 1.7B，但两个变体都没装：宁可不绑定让模型检查报缺目录，
        // 也不能悄悄换个型号跑——那正是「跑出来的不是我要的」最难发现的一种。
        let root = root_with_all_asr_variants("missing");
        for id in [ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8] {
            std::fs::remove_dir_all(root.join("models").join(id.as_str())).unwrap();
        }
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17B.as_str()),
            "没装就该保留它自己的路径报错，而不是回退去跑已装的 0.6B"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_size_alias_falls_back_to_the_installed_int8_sibling() {
        // 只装了 1.7B 的 int8 变体（量化版换 fp16 不算「换了模型」，同一个尺寸），
        // `--asr 1.7b` 必须绑到它，而不是报「模型目录不存在」。
        // 断骨锚点：把 `candidates()` 的 Size 分支改回只给一个 id，这条立刻红。
        let root = root_with_all_asr_variants("int8-only");
        std::fs::remove_dir_all(root.join("models").join(ModelId::Qwen3Asr17B.as_str())).unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(s.asr_model, ModelId::Qwen3Asr17BInt8.as_str());
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "尺寸级指定：同尺寸的 int8 装了就该用它，而不是报缺目录"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_asr_size_alias_prefers_fp16_when_both_variants_are_installed() {
        // 两个变体都装时仍是 fp16 优先，与不带旗标的扫描顺序一致。
        // 断骨锚点：把 `candidates()` 的两个候选反过来，这条立刻红。
        let root = root_with_all_asr_variants("fp16-first");
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17B.as_str())
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// fp16 优先是**全局**规则，不被 root 顺序截胡：`--data-root` 只装了
    /// 1.7B-int8、`--app-root` 只装了 1.7B-fp16 时，不管点不点名尺寸，都该绑
    /// app_root 的 fp16。
    /// 断骨锚点：把候选扫描的两层换回 root 在外，两条断言一起变红。
    #[test]
    fn fp16_preference_is_global_across_data_and_app_roots() {
        let data = root_with_all_asr_variants("xroot-data");
        let app = root_with_all_asr_variants("xroot-app");
        // data_root 只留 1.7B-int8；app_root 只留 1.7B-fp16。
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17B,
            ModelId::Qwen3Asr06BInt8,
        ] {
            std::fs::remove_dir_all(data.join("models").join(id.as_str())).unwrap();
        }
        for id in [
            ModelId::Qwen3Asr06B,
            ModelId::Qwen3Asr17BInt8,
            ModelId::Qwen3Asr06BInt8,
        ] {
            std::fs::remove_dir_all(app.join("models").join(id.as_str())).unwrap();
        }
        let want = app.join("models").join(ModelId::Qwen3Asr17B.as_str());

        let mut explicit = Settings::default();
        apply_app_root_paths(
            &mut explicit,
            &app,
            &data,
            Some(AsrPick::Size(ModelId::Qwen3Asr17B)),
            None,
        );
        assert_eq!(
            explicit.asr_model_dir, want,
            "点名尺寸：app_root 的 fp16 必须赢过 data_root 的 int8"
        );

        let mut scanned = Settings::default();
        apply_app_root_paths(&mut scanned, &app, &data, None, None);
        assert_eq!(
            scanned.asr_model_dir, want,
            "不带旗标：扫描同样要全局 fp16 优先"
        );

        let _ = std::fs::remove_dir_all(&data);
        let _ = std::fs::remove_dir_all(&app);
    }

    #[test]
    fn an_exact_asr_directory_name_never_falls_back_to_another_variant() {
        // 点名了具体权重（int8），机器上只有 fp16：同尺寸也不换——换了就等于
        // 「跑出来的不是我点名的那份权重」，与默默换尺寸一样是骗人。
        let root = root_with_all_asr_variants("exact-no-fallback");
        std::fs::remove_dir_all(root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()))
            .unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(
            &mut s,
            &root,
            &root,
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8)),
            None,
        );
        assert_eq!(
            s.asr_model_dir,
            root.join("models").join(ModelId::Qwen3Asr17BInt8.as_str()),
            "具体目录名只认它自己：宁可报缺，也不能悄悄用同尺寸的 fp16"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_aligner_flag_naming_a_missing_engine_is_not_silently_swapped() {
        // 对齐器那边是同一个形状：点名 qwen 却没有它，不能悄悄退回 CTC 跑。
        // 两边一起修，就得两边都有用例守着。
        let root = env::temp_dir().join(format!("oneasr-align-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("models").join(ModelId::OmniAsrCtc300M.as_str()))
            .unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, Some(ModelId::QwenAlign06B));
        assert_eq!(s.aligner_model, ModelId::QwenAlign06B.as_str());
        assert_eq!(
            s.aligner_model_dir,
            root.join("models").join(ModelId::QwenAlign06B.as_str()),
            "没装就该指向它自己并报缺，而不是退回已装的 CTC"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn parse_asr_arg_keeps_the_granularity_the_user_typed() {
        // 尺寸级：别名落到 Size，候选含 fp16 + int8。
        assert_eq!(
            parse_asr_arg("1.7b"),
            Some(AsrPick::Size(ModelId::Qwen3Asr17B))
        );
        assert_eq!(
            parse_asr_arg("0.6B"),
            Some(AsrPick::Size(ModelId::Qwen3Asr06B))
        );
        assert_eq!(
            parse_asr_arg(" 1.7B "),
            Some(AsrPick::Size(ModelId::Qwen3Asr17B))
        );
        // 目录级：完整名字只认它自己，哪怕它是量化变体。
        assert_eq!(
            parse_asr_arg(ModelId::Qwen3Asr17BInt8.as_str()),
            Some(AsrPick::Exact(ModelId::Qwen3Asr17BInt8))
        );
        assert_eq!(
            ModelId::Qwen3Asr17BInt8.as_str(),
            "Qwen3-ASR-1.7B-int8",
            "完整目录名走的是目录级，夹具依赖这个字面名"
        );
        assert_eq!(
            AsrPick::Size(ModelId::Qwen3Asr17B).candidates(),
            vec![ModelId::Qwen3Asr17B, ModelId::Qwen3Asr17BInt8]
        );
        assert_eq!(
            AsrPick::Exact(ModelId::Qwen3Asr17BInt8).candidates(),
            vec![ModelId::Qwen3Asr17BInt8]
        );
        assert_eq!(parse_asr_arg("ctc"), None, "对齐器的别名不该被认成识别模型");
        assert_eq!(parse_asr_arg("nope"), None);
    }

    // ── 热词：`--hotword-group` / `--hotwords` ─────────────────────────

    fn hotword_group(id: &str, name: &str, words: &str) -> oneasr_core::HotwordGroup {
        oneasr_core::HotwordGroup {
            id: id.into(),
            name: name.into(),
            words: words.into(),
        }
    }

    fn hotword_store(groups: Vec<oneasr_core::HotwordGroup>) -> HotwordStore {
        HotwordStore {
            groups,
            default_group: None,
            default_group_enabled: false,
            top_k: 7,
            threshold: 0.42,
        }
    }

    /// `--hotword-group` 按 id 命中，并把组里的词表与库级调参带进 `HotwordRun`。
    #[test]
    fn cli_hotword_group_resolves_by_id() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("terms", "术语表", "甲词, 乙词"),
        ]);
        let run = resolve_cli_hotwords(&store, Some("terms"), None, i18n::UiLang::Zh)
            .expect("按 id 必须命中")
            .expect("给了组旗标就该有 run");
        assert_eq!(run.group_id, "terms");
        assert_eq!(run.text, "甲词, 乙词");
        assert_eq!(run.top_k, 7, "调参要沿用库里存的");
        assert_eq!(run.threshold, 0.42);
        assert!(run.is_usable());
    }

    /// 组名（含大小写不敏感）也能命中：用户记住的是 chip 上的名字，不是 id。
    ///
    /// 断骨验证：把 `HotwordStore::group_by_id_or_name` 的忽略大小写兜底删掉，
    /// `"glossary"` 一条立刻变红。
    #[test]
    fn cli_hotword_group_resolves_by_name() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("g1", "Glossary", "alpha"),
        ]);
        for query in ["Glossary", "glossary", "GLOSSARY"] {
            let run = resolve_cli_hotwords(&store, Some(query), None, i18n::UiLang::Zh)
                .expect("组名必须命中")
                .expect("run");
            assert_eq!(run.group_id, "g1", "query={query}");
        }
    }

    /// 重名：报错退出（2），不默默取一个——「编辑 A 却跑的是 B」正是要防的。
    ///
    /// 断骨验证：把 `group_by_id_or_name` 的重名分支改成取第一个，本测试变红。
    #[test]
    fn cli_hotword_group_name_collision_is_refused() {
        let store = hotword_store(vec![
            hotword_group("default", "", ""),
            hotword_group("g1", "甲", "a"),
            hotword_group("g2", "甲", "b"),
        ]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("甲"), None, i18n::UiLang::Zh).unwrap_err(),
            2
        );
    }

    /// 找不到：报错退出（2），不静默跑。
    #[test]
    fn cli_hotword_group_unknown_is_refused() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("nope"), None, i18n::UiLang::Zh).unwrap_err(),
            2
        );
    }

    /// 两个旗标二选一。
    ///
    /// 断骨验证：去掉各自独立分支（让后给的那个静默胜出），本测试变红。
    #[test]
    fn cli_hotword_flags_are_mutually_exclusive() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, Some("default"), Some("甲"), i18n::UiLang::Zh)
                .unwrap_err(),
            2
        );
    }

    /// 没给旗标：不解析、不碰热词库（`Ok(None)`，管线自然不跑粗转）。
    #[test]
    fn cli_without_hotword_flags_resolves_to_none() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        assert_eq!(
            resolve_cli_hotwords(&store, None, None, i18n::UiLang::Zh).unwrap(),
            None
        );
    }

    /// `--hotwords`：即席词表直接进 `HotwordRun`，调参沿用库里的。
    #[test]
    fn cli_inline_hotwords_carry_the_store_knobs() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        let run = resolve_cli_hotwords(&store, None, Some("窦神归来, 柳宗元"), i18n::UiLang::Zh)
            .expect("即席词表必须解析出来")
            .expect("run");
        assert_eq!(run.text, "窦神归来, 柳宗元");
        assert_eq!(run.group_id, "cli");
        assert!(
            !run.group_name.trim().is_empty(),
            "报错与 banner 要有个名字"
        );
        assert_eq!(run.word_count(), 2);
        assert_eq!(run.top_k, 7);
        assert_eq!(run.threshold, 0.42);
    }

    /// `--hotwords` 解析后没有词条：用法错误（2），不当「空组」放行。
    #[test]
    fn cli_inline_hotwords_must_contain_words() {
        let store = hotword_store(vec![hotword_group("default", "", "")]);
        for raw in ["", "   ", ",,,", "，", " , \n , "] {
            assert_eq!(
                resolve_cli_hotwords(&store, None, Some(raw), i18n::UiLang::Zh).unwrap_err(),
                2,
                "raw={raw:?}"
            );
        }
    }

    /// `apply_app_root_paths` 也认安装布局里的热词模型目录（与 demucs 同一套
    /// 数据目录优先、安装目录兜底）。
    ///
    /// 断骨验证：删掉 `apply_app_root_paths` 里 sensevoice 那一段扫描，本测试变红
    /// （`sensevoice_model_dir` 会停在 `Settings::default()` 解析的落点）。
    #[test]
    fn app_root_paths_also_find_the_sensevoice_model_dir() {
        let root = env::temp_dir().join(format!("oneasr-sensevoice-pick-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let dir = root.join("models").join(ModelId::SenseVoiceSmall.as_str());
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Settings::default();
        apply_app_root_paths(&mut s, &root, &root, None, None);
        assert_eq!(s.sensevoice_model_dir, dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn sentence(id: usize, text: &str) -> SourceSentence {
        SourceSentence {
            sentence_id: id,
            start_ms: id as u64 * 1000,
            end_ms: id as u64 * 1000 + 900,
            text: text.to_string(),
            word_start: 0,
            word_end: 1,
            chunk_start: 0,
            chunk_end: 1,
        }
    }

    /// `render` 的三条英文句子各占一行。接线若退回先 `join("")` 再断句，
    /// 三条会粘成一行，这条测试会红——它守的是 cmd_render 的接线，不只是
    /// `body_from_parts` 本身。
    #[test]
    fn rendered_txt_body_keeps_english_sentences_separate() {
        let sentences = [
            sentence(1, "Hello world."),
            sentence(2, "Second sentence."),
            sentence(3, "Third one."),
        ];
        assert_eq!(
            rendered_txt_body(&sentences),
            "Hello world.\nSecond sentence.\nThird one.\n"
        );
    }
}
