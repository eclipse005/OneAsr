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
//! Env: `ONEASR_PIPELINE_TRACE=1` prints per-chunk ASR/align detail.

use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use oneasr_core::media::slice_wav;
use oneasr_core::{
    ffmpeg_source, process_media_file_with_export, resolve_app_root, ProcessExportOptions,
    StageClock, StageUpdate, Settings, ModelId,
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

    // Default subcommand: bare flags act as `transcribe`.
    let (cmd, rest) = if args[1].starts_with('-') {
        ("transcribe", &args[1..])
    } else {
        (args[1].as_str(), &args[2..])
    };

    let result = match cmd {
        "transcribe" | "run" | "pipeline" => cmd_transcribe(rest),
        "asr-chunk" | "chunk" => cmd_asr_chunk(rest),
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            print_help();
            Err(2)
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code as u8),
    }
}

fn print_help() {
    eprintln!(
        "\
oneasr-cli — headless Qwen ASR + ForcedAligner pipeline

Usage:
  oneasr-cli transcribe --input <media> [options]
  oneasr-cli asr-chunk  --wav <16k.wav> --start <sec> --end <sec> [options]

Commands:
  transcribe   Full pipeline → {{app-root}}/output/{{stem}}.srt by default  (alias: run, pipeline)
  asr-chunk    ASR only for one time range (hallucination / length debug)

transcribe options:
  --input <path>           Media file (required)
  --app-root <dir>         App root with bin/ffmpeg, models/  (default: this exe's install dir)
  --language <code>        zh|en|yue|ja|ko|...  (default: zh)
  --chunk-seconds <30-180> VAD chunk target (default: 60)
  --backend <gpu|cpu|auto> Default: auto (GPU if a driver is present, else CPU)
  --max-new-tokens <n>     ASR decode ceiling (default: settings / 2048)
  --output <path>          Copy the primary result (SRT, else TXT) to this path
  --txt                    Also write {{stem}}.txt (one transcript line per cue)
  --no-srt                 Suppress the .srt file (requires --txt)
  --script <simplified|traditional>
                           Chinese output script for zh / yue (default: simplified)
  --vocal-separation       Run HTDemucs vocal separation before ASR
  --demucs-model-dir <dir> Directory holding htdemucs_ft_vocals.safetensors
  --words-json <path>      Write ForcedAligner word/char tokens + timestamps (JSON)

asr-chunk options:
  --wav <path>             16 kHz mono wav (required)
  --start <sec>  --end <sec>
  --language <code>
  --app-root <dir>
  --max-new-tokens <n>
  --out <path>             Write ASR text to file
  --backend <gpu|cpu|auto>

Env:
  ONEASR_PIPELINE_TRACE=1  Per-chunk ASR/align logs

Examples:
  oneasr-cli transcribe --input video.mp4 --app-root D:\\OneAsr --chunk-seconds 120 --backend auto
  oneasr-cli asr-chunk --wav runs\\x\\input_16k.wav --start 722 --end 843 --language zh
"
    );
}

// ─── transcribe ───────────────────────────────────────────────────

fn cmd_transcribe(args: &[String]) -> Result<(), i32> {
    if flag(args, "--help") || flag(args, "-h") {
        print_help();
        return Ok(());
    }
    let input = require_arg(args, "--input")?;
    let app_root = parse_app_root(args)?;
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg(args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let output_copy = arg(args, "--output").map(PathBuf::from);
    let words_json = arg(args, "--words-json").map(PathBuf::from);
    let want_txt = flag(args, "--txt");
    let no_srt = flag(args, "--no-srt");
    let script = arg(args, "--script");
    let vocal_separation = flag(args, "--vocal-separation");
    let demucs_model_dir = arg(args, "--demucs-model-dir").map(PathBuf::from);

    let input_path = PathBuf::from(&input);
    if !input_path.is_file() {
        eprintln!("input not found: {}", input_path.display());
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
    settings.output_srt = !no_srt;
    if no_srt && !want_txt {
        eprintln!("warning: --no-srt 需要配合 --txt；已保留 SRT 输出");
    }
    if let Some(s) = script {
        settings.text_script = s;
    }
    settings.vocal_separation = vocal_separation;
    apply_app_root_paths(&mut settings, &app_root);
    if let Some(dir) = demucs_model_dir {
        settings.demucs_model_dir = dir;
    }
    settings.normalize();
    if let Err(e) = settings.can_start() {
        eprintln!("{e}");
        return Err(1);
    }

    eprintln!("=== OneAsr CLI · transcribe ===");
    eprintln!("input:   {}", input_path.display());
    eprintln!("app:     {}", app_root.display());
    eprintln!("asr:     {}", settings.asr_model_dir.display());
    eprintln!("align:   {}", settings.aligner_model_dir.display());
    eprintln!("output:  {}", settings.resolved_output_dir().display());
    eprintln!(
        "lang={} chunk={}s backend={} max_new_tokens={} srt={} txt={} script={} vocal_sep={}",
        settings.language,
        settings.chunk_target_seconds,
        settings.backend,
        settings.max_new_tokens,
        settings.output_srt,
        settings.output_txt,
        settings.text_script_choice().label(),
        settings.vocal_separation,
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
        &app_root,
        |update: StageUpdate| {
            clock.note(&update);
            if let Some(w) = &update.warning {
                eprintln!("[warn] {w}");
            }
            eprintln!("[stage] {}", update.label());
        },
        export,
    );
    let timing = clock.finish();
    let wall = t0.elapsed();

    match result {
        Ok(primary) => {
            eprintln!("OK output={}", primary.display());
            if let Some(dst) = output_copy {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                std::fs::copy(&primary, &dst).map_err(|e| {
                    eprintln!("copy output failed: {e}");
                    1
                })?;
                eprintln!("copied → {}", dst.display());
            }
            if let Some(wj) = words_json {
                eprintln!("words → {}", wj.display());
            }
            eprintln!(
                "wall={:.1}s total_ms={} stages={}",
                wall.as_secs_f64(),
                timing.total_ms,
                timing.stages.len()
            );
            for s in &timing.stages {
                eprintln!(
                    "  {:12} {:>8}",
                    s.stage.label(),
                    oneasr_core::format_process_ms(s.elapsed_ms)
                );
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("FAIL after {:.1}s: {e}", wall.as_secs_f64());
            for s in &timing.stages {
                eprintln!(
                    "  {:12} {:>8}",
                    s.stage.label(),
                    oneasr_core::format_process_ms(s.elapsed_ms)
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
    let start: f32 = require_arg(args, "--start")?
        .parse()
        .map_err(|_| {
            eprintln!("--start must be a number (seconds)");
            2
        })?;
    let end: f32 = require_arg(args, "--end")?.parse().map_err(|_| {
        eprintln!("--end must be a number (seconds)");
        2
    })?;
    if end <= start {
        eprintln!("--end must be > --start");
        return Err(2);
    }
    let language = arg(args, "--language").unwrap_or_else(|| "zh".into());
    let out = arg(args, "--out").map(PathBuf::from);
    let backend_s = arg(args, "--backend").unwrap_or_else(|| "auto".into());
    let max_new_tokens = arg(args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let app_root = parse_app_root(args).unwrap_or_else(|_| default_app_root());

    if !wav.is_file() {
        eprintln!("wav not found: {}", wav.display());
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
    apply_app_root_paths(&mut settings, &app_root);
    settings.normalize();

    let tmp = env::temp_dir().join(format!(
        "oneasr_dump_{}_{}.wav",
        (start * 1000.0) as u32,
        (end * 1000.0) as u32
    ));
    slice_wav(&wav, start, end, &tmp).map_err(|e| {
        eprintln!("slice_wav: {e}");
        1
    })?;
    eprintln!(
        "slice {:.3}-{:.3} ({:.1}s) → {}",
        start,
        end,
        end - start,
        tmp.display()
    );

    let backend = match settings.backend.to_ascii_lowercase().as_str() {
        "cpu" => AsrBackend::Cpu,
        "gpu" => AsrBackend::Gpu,
        _ => AsrBackend::Auto,
    };
    let asr = AsrInference::load(&settings.asr_model_dir, backend).map_err(|e| {
        eprintln!("load ASR: {e}");
        1
    })?;
    let lang = oneasr_core::lang::to_qwen_language_label(&settings.language);
    let opts = TranscribeOptions::default()
        .with_max_new_tokens(settings.max_new_tokens)
        .with_language(lang);
    let path_str = tmp.to_str().ok_or_else(|| {
        eprintln!("wav path is not valid UTF-8");
        1
    })?;
    let t0 = Instant::now();
    let report = asr.transcribe(path_str, opts).map_err(|e| {
        eprintln!("transcribe: {e}");
        1
    })?;
    let text = report.text.trim();
    let chars = text.chars().count();
    eprintln!(
        "chars={chars} raw_chars={} elapsed={:.1}s",
        report.raw_output.chars().count(),
        t0.elapsed().as_secs_f64()
    );
    eprintln!("--- ASR TEXT BEGIN ---");
    println!("{text}");
    eprintln!("--- ASR TEXT END ---");

    if let Some(p) = out {
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&p, text.as_bytes()).map_err(|e| {
            eprintln!("write: {e}");
            1
        })?;
        eprintln!("wrote {}", p.display());
    }

    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

// ─── shared ───────────────────────────────────────────────────────

/// Bind model dirs + SRT output folder to the CLI `--app-root` layout.
///
/// SRT path is driven by `settings.output_dir` (not the pipeline `app_root`
/// argument alone), so headless runs must pin it here.
fn apply_app_root_paths(settings: &mut Settings, app_root: &Path) {
    let models = app_root.join("models");
    // Bind whichever catalog ASR size is actually installed under --app-root
    // (0.6B preferred); otherwise keep the default path so the model check
    // reports the missing directory.
    if let Some(id) = ModelId::ASR_CHOICES
        .into_iter()
        .find(|id| models.join(id.as_str()).is_dir())
    {
        settings.asr_model = id.as_str().into();
        settings.asr_model_dir = models.join(id.as_str());
    }
    let align = models.join("Qwen3-ForcedAligner-0.6B-hf");
    if align.is_dir() {
        settings.aligner_model_dir = align;
    }
    let demucs = models.join("htdemucs_ft");
    if demucs.is_dir() {
        settings.demucs_model_dir = demucs;
    }
    settings.output_dir = app_root.join("output");
    // Headless runs keep writing to {app_root}/output (GUI's "next to source"
    // default would be surprising for batch scripts).
    settings.save_next_to_source = false;
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
            eprintln!("ffmpeg: no binary under {} — using {source}", bin.display());
            Ok(())
        }
        None => {
            eprintln!(
                "ffmpeg not found: put a binary in {} or install ffmpeg on PATH",
                bin.display()
            );
            Err(1)
        }
    }
}

fn default_app_root() -> PathBuf {
    resolve_app_root().unwrap_or_else(|| {
        env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    })
}

fn parse_app_root(args: &[String]) -> Result<PathBuf, i32> {
    if let Some(s) = arg(args, "--app-root") {
        let p = PathBuf::from(s);
        if !p.is_dir() {
            eprintln!("--app-root not a directory: {}", p.display());
            return Err(1);
        }
        return Ok(p);
    }
    Ok(default_app_root())
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
}

fn flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

fn require_arg(args: &[String], name: &str) -> Result<String, i32> {
    arg(args, name).ok_or_else(|| {
        eprintln!("missing required {name}");
        2
    })
}

fn is_help(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "help")
}


