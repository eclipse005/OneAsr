//! Headless full pipeline (same path as the GUI worker).
//!
//! Prefer the dedicated binary when possible:
//! ```powershell
//! cargo run -p oneasr-core --release --bin oneasr-cli --features cuda -- `
//!   transcribe --input "C:\path\to\video.mp4" --app-root "D:\OneAsr" `
//!   --language zh --chunk-seconds 60 --backend cuda
//! ```
//!
//! This example remains for backward-compatible scripts:
//! ```powershell
//! cargo run -p oneasr-core --release --example run_pipeline --features cuda -- `
//!   --input "C:\path\to\video.mp4" --app-root "D:\OneAsr" --chunk-seconds 60 --backend cuda
//! ```

use std::env;
use std::path::PathBuf;
use std::time::Instant;

use oneasr_core::{
    init_native_library_path, process_media_file_with_progress, StageClock, StageUpdate, Settings,
};

fn usage() -> ! {
    eprintln!(
        "Usage: run_pipeline --input <media> [--app-root <dir>] [--language zh] \
         [--chunk-seconds 60] [--backend cuda|cpu|auto] [--max-new-tokens N] [--output <srt>]\n\
         Prefer: cargo run -p oneasr-core --release --bin oneasr-cli -- transcribe ..."
    );
    std::process::exit(2);
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == name)
        .map(|w| w[1].clone())
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let input = arg_value(&args, "--input").unwrap_or_else(|| usage());
    let app_root = arg_value(&args, "--app-root").unwrap_or_else(|| {
        if PathBuf::from(r"D:\OneAsr\bin\ffmpeg.exe").is_file() {
            r"D:\OneAsr".into()
        } else {
            usage()
        }
    });
    let language = arg_value(&args, "--language").unwrap_or_else(|| "zh".into());
    let chunk_seconds: u32 = arg_value(&args, "--chunk-seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(60);
    let backend = arg_value(&args, "--backend").unwrap_or_else(|| "cuda".into());
    let max_new_tokens = arg_value(&args, "--max-new-tokens").and_then(|s| s.parse().ok());
    let output_copy = arg_value(&args, "--output").map(PathBuf::from);

    let input_path = PathBuf::from(&input);
    if !input_path.is_file() {
        eprintln!("input not found: {}", input_path.display());
        std::process::exit(1);
    }
    let app_root_path = PathBuf::from(&app_root);
    if !app_root_path.join("bin").join("ffmpeg.exe").is_file()
        && !app_root_path.join("bin").join("ffmpeg").is_file()
    {
        eprintln!("ffmpeg not under {}", app_root_path.join("bin").display());
        std::process::exit(1);
    }

    if let Some(dll) = oneasr_core::resolve_cuda_runtime_dir() {
        register_dll_dir(&dll);
    } else {
        register_dll_dir(&app_root_path.join("dll"));
    }
    init_native_library_path();

    let mut settings = Settings::default();
    settings.language = language;
    settings.chunk_target_seconds = chunk_seconds.clamp(30, 180);
    settings.backend = backend;
    if let Some(n) = max_new_tokens {
        settings.max_new_tokens = n;
    }
    let asr_06 = app_root_path.join("models").join("Qwen3-ASR-0.6B");
    let align = app_root_path.join("models").join("Qwen3-ForcedAligner-0.6B");
    if asr_06.is_dir() {
        settings.asr_model_dir = asr_06;
    }
    if align.is_dir() {
        settings.aligner_model_dir = align;
    }
    settings.normalize();

    eprintln!("=== OneAsr headless pipeline ===");
    eprintln!("input:   {}", input_path.display());
    eprintln!("app:     {}", app_root_path.display());
    eprintln!("asr:     {}", settings.asr_model_dir.display());
    eprintln!("align:   {}", settings.aligner_model_dir.display());
    eprintln!(
        "lang={} chunk={}s backend={} max_new_tokens={}",
        settings.language, settings.chunk_target_seconds, settings.backend, settings.max_new_tokens
    );

    let media_name = input_path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| input_path.display().to_string());

    let mut clock = StageClock::new();
    let t0 = Instant::now();
    let result = process_media_file_with_progress(
        &input_path,
        &media_name,
        &settings,
        &app_root_path,
        |update: StageUpdate| {
            clock.note(&update);
            eprintln!("[stage] {}", update.label());
        },
    );
    let timing = clock.finish();
    let wall = t0.elapsed();

    match result {
        Ok(srt) => {
            eprintln!("OK srt={}", srt.display());
            if let Some(dst) = output_copy {
                if let Some(parent) = dst.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(e) = std::fs::copy(&srt, &dst) {
                    eprintln!("copy srt failed: {e}");
                    std::process::exit(1);
                }
                eprintln!("copied → {}", dst.display());
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
            std::process::exit(1);
        }
    }
}

fn register_dll_dir(dll_dir: &std::path::Path) {
    let _ = std::fs::create_dir_all(dll_dir);
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = dll_dir
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        unsafe extern "system" {
            fn SetDllDirectoryW(path: *const u16) -> i32;
        }
        unsafe {
            let _ = SetDllDirectoryW(wide.as_ptr());
        }
        eprintln!("dll_dir: {}", dll_dir.display());
    }
}
