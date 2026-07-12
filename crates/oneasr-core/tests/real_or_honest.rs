//! Real ASR integration when model + ffmpeg + sample WAV are available.
//! Missing environment → honest skip (never writes a fake SRT as success).
//!
//! Optional env:
//! - `ONEASR_MODEL_DIR` — model weights directory
//! - `ONEASR_SAMPLE_WAV` — sample audio path

use std::path::PathBuf;
use std::time::Instant;

use oneasr_core::{
    ffmpeg_available, planned_output_path, process_media_file, resolve_app_root, Settings,
};

#[test]
fn real_asr_writes_install_output_srt_or_honest_skip() {
    let app_root = match resolve_app_root() {
        Some(p) => p,
        None => {
            eprintln!("SKIP: resolve_app_root() failed (bin/ffmpeg not found)");
            return;
        }
    };

    if !ffmpeg_available() {
        eprintln!("SKIP: bundled ffmpeg not available");
        return;
    }

    let model = std::env::var_os("ONEASR_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(r"D:\MOSS-Transcribe-Diarize\pretrained\moss-transcribe-diarize")
        });
    if !model.join("config.json").is_file() {
        eprintln!("SKIP: model dir missing config.json ({})", model.display());
        return;
    }

    let sample = std::env::var_os("ONEASR_SAMPLE_WAV")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"D:\MOSS-Transcribe-Diarize\ja.wav"));
    if !sample.is_file() {
        eprintln!("SKIP: sample wav missing ({})", sample.display());
        return;
    }

    let settings = Settings {
        model_dir: model,
        backend: "auto".into(),
        max_new_tokens: 512,
        export_show_speaker: true,
        ..Default::default()
    };

    let expected = planned_output_path(&app_root, &sample);
    let t0 = Instant::now();
    match process_media_file(&sample, "sample.wav", &settings, &app_root) {
        Ok(out) => {
            assert_eq!(
                out, expected,
                "process_media_file must return install output path"
            );
            assert!(out.is_file(), "SRT must exist: {}", out.display());
            let body = std::fs::read_to_string(&out).expect("read srt");
            assert!(!body.trim().is_empty(), "SRT must not be empty");
            assert!(!body.contains("[stub]"), "no stub SRT");
            eprintln!(
                "OK: wrote {} ({} bytes) in {:?}",
                out.display(),
                body.len(),
                t0.elapsed()
            );
        }
        Err(e) => {
            // Real failure is allowed to fail the test when env is fully present.
            panic!("real ASR failed: {e}");
        }
    }
}
