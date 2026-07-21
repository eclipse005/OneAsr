//! Media prep via bundled tools under `{app_root}/bin/`.
//!
//! Layout (install and dev share the same convention):
//!
//! ```text
//! OneAsr/                 ← app root
//!   oneasr.exe            ← (or target/debug/oneasr.exe in dev)
//!   bin/
//!     ffmpeg.exe
//! ```
//!
//! Not user-configurable: after install, tools always live in the install
//! directory's `bin/`. We only *locate* that directory relative to the running
//! executable (walking up for `cargo run` from `target/debug`).
//!
//! **Pipeline audio contract**: ASR/Aligner/VAD all consume **16 kHz mono PCM
//! s16le WAV**. Once converted, further work (duration, slice) stays in-process
//! via `hound` — no ffmpeg round-trips for PCM.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use thiserror::Error;

#[cfg(windows)]
const FFMPEG_NAME: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_NAME: &str = "ffmpeg";

/// ASR / VAD / aligner input format (product contract).
pub const TARGET_SAMPLE_RATE: u32 = 16_000;
pub const TARGET_CHANNELS: u16 = 1;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Concurrent duration probes (UI batch import). One process-wide pool.
const PROBE_WORKERS: usize = 2;

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("bundled ffmpeg not found (expected {{app}}/bin/ffmpeg; searched from {0})")]
    FfmpegMissing(String),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("wav error: {0}")]
    Wav(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// App root = directory that contains `bin/ffmpeg(.exe)`.
///
/// Search:
/// 1. `current_exe().parent()` and ancestors (covers install + `target/debug`)
/// 2. `current_dir()` and ancestors (covers tests / odd cwd)
pub fn resolve_app_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            starts.push(dir.to_path_buf());
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }

    for start in starts {
        let mut dir = start;
        for _ in 0..8 {
            if dir.join("bin").join(FFMPEG_NAME).is_file() {
                return Some(dir);
            }
            if !dir.pop() {
                break;
            }
        }
    }
    None
}

fn resolve_bin_dir() -> Option<PathBuf> {
    resolve_app_root().map(|root| root.join("bin"))
}

fn resolve_ffmpeg() -> Result<PathBuf, MediaError> {
    let path = resolve_bin_dir()
        .map(|d| d.join(FFMPEG_NAME))
        .ok_or_else(|| {
            let hint = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "<unknown exe>".into());
            MediaError::FfmpegMissing(hint)
        })?;
    if path.is_file() {
        Ok(path)
    } else {
        Err(MediaError::FfmpegMissing(path.display().to_string()))
    }
}

fn ffmpeg_command(ffmpeg: &Path) -> Command {
    let mut cmd = Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// True when `path` is already the pipeline PCM contract (16 kHz mono s16le).
pub fn is_target_pcm_wav(path: &Path) -> bool {
    let Ok(reader) = hound::WavReader::open(path) else {
        return false;
    };
    let spec = reader.spec();
    spec.sample_rate == TARGET_SAMPLE_RATE
        && spec.channels == TARGET_CHANNELS
        && spec.sample_format == hound::SampleFormat::Int
        && spec.bits_per_sample == 16
}

/// Duration of a WAV from header + sample count (no process spawn).
pub fn wav_duration_sec(path: &Path) -> Option<f64> {
    let reader = hound::WavReader::open(path).ok()?;
    let sr = reader.spec().sample_rate as f64;
    if sr <= 0.0 {
        return None;
    }
    Some(reader.duration() as f64 / sr)
}

/// Probe duration (seconds). PCM WAV uses in-process header read; else ffmpeg.
///
/// Caps probe/analyze so large files (GB-class MP4) don't hang for minutes
/// scanning for duration when container headers already have it.
pub fn probe_duration_sec(input: &Path) -> Option<f64> {
    if let Some(d) = wav_duration_sec(input) {
        return Some(d);
    }
    let ffmpeg = resolve_ffmpeg().ok()?;
    let mut cmd = ffmpeg_command(&ffmpeg);
    cmd.args([
        "-hide_banner",
        "-probesize",
        "5M",
        "-analyzeduration",
        "5M",
        "-i",
    ])
    .arg(input);
    let output = cmd.output().ok()?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    parse_duration_from_ffmpeg_stderr(&stderr)
}

fn parse_duration_from_ffmpeg_stderr(stderr: &str) -> Option<f64> {
    // Duration: 00:01:30.05,
    for line in stderr.lines() {
        let Some(idx) = line.find("Duration:") else {
            continue;
        };
        let rest = line[idx + "Duration:".len()..].trim();
        let token = rest.split([',', ' ']).next()?.trim();
        return parse_hms(token);
    }
    None
}

fn parse_hms(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    let h: f64 = parts[0].parse().ok()?;
    let m: f64 = parts[1].parse().ok()?;
    let sec: f64 = parts[2].parse().ok()?;
    Some(h * 3600.0 + m * 60.0 + sec)
}

/// Convert any media path to 16 kHz mono PCM WAV.
///
/// Already-correct PCM is copied (or no-op when paths match) — no ffmpeg.
pub fn convert_to_16k_mono_wav(input: &Path, out_wav: &Path) -> Result<PathBuf, MediaError> {
    if let Some(parent) = out_wav.parent() {
        std::fs::create_dir_all(parent)?;
    }

    if is_target_pcm_wav(input) {
        if same_file(input, out_wav) {
            return Ok(out_wav.to_path_buf());
        }
        std::fs::copy(input, out_wav)?;
        return Ok(out_wav.to_path_buf());
    }

    let ffmpeg = resolve_ffmpeg()?;
    let mut cmd = ffmpeg_command(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args([
            "-vn",
            "-ac",
            "1",
            "-ar",
            &TARGET_SAMPLE_RATE.to_string(),
            "-c:a",
            "pcm_s16le",
        ])
        .arg(out_wav);
    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::FfmpegFailed(stderr.chars().take(500).collect()));
    }
    Ok(out_wav.to_path_buf())
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(ca), Ok(cb)) => ca == cb,
        _ => a == b,
    }
}

/// Slice `[start, end)` seconds from a pipeline PCM WAV into `out_wav`.
///
/// Uses in-process sample I/O (no ffmpeg). Input must be target PCM
/// ([`is_target_pcm_wav`]); that is always true after [`convert_to_16k_mono_wav`].
pub fn slice_wav(input: &Path, start: f32, end: f32, out_wav: &Path) -> Result<PathBuf, MediaError> {
    if let Some(parent) = out_wav.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut reader = hound::WavReader::open(input)
        .map_err(|e| MediaError::Wav(format!("open {}: {e}", input.display())))?;
    let spec = reader.spec();
    if spec.sample_rate != TARGET_SAMPLE_RATE
        || spec.channels != TARGET_CHANNELS
        || spec.sample_format != hound::SampleFormat::Int
        || spec.bits_per_sample != 16
    {
        return Err(MediaError::Wav(format!(
            "slice expects {} Hz mono s16le, got {} Hz {}ch {}bit {:?}",
            TARGET_SAMPLE_RATE,
            spec.sample_rate,
            spec.channels,
            spec.bits_per_sample,
            spec.sample_format
        )));
    }

    let total = reader.duration();
    let start_s = start.max(0.0);
    let end_s = end.max(start_s);
    let start_i = ((start_s as f64) * f64::from(TARGET_SAMPLE_RATE))
        .round()
        .clamp(0.0, f64::from(total)) as u32;
    let end_i = ((end_s as f64) * f64::from(TARGET_SAMPLE_RATE))
        .round()
        .clamp(f64::from(start_i), f64::from(total)) as u32;
    let n = end_i.saturating_sub(start_i);

    reader
        .seek(start_i)
        .map_err(|e| MediaError::Wav(format!("seek: {e}")))?;

    let mut writer = hound::WavWriter::create(out_wav, spec)
        .map_err(|e| MediaError::Wav(format!("create {}: {e}", out_wav.display())))?;

    // Mono i16: one sample per frame.
    let mut samples = reader.samples::<i16>();
    for _ in 0..n {
        let s = samples
            .next()
            .ok_or_else(|| MediaError::Wav("unexpected end of wav while slicing".into()))?
            .map_err(|e| MediaError::Wav(format!("read sample: {e}")))?;
        writer
            .write_sample(s)
            .map_err(|e| MediaError::Wav(format!("write sample: {e}")))?;
    }
    writer
        .finalize()
        .map_err(|e| MediaError::Wav(format!("finalize: {e}")))?;

    Ok(out_wav.to_path_buf())
}

// ── Bounded duration-probe pool (UI batch import) ─────────────────────

type ProbeJob = (PathBuf, Box<dyn FnOnce(Option<f64>) + Send>);

fn probe_sender() -> Sender<ProbeJob> {
    static TX: OnceLock<Sender<ProbeJob>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ProbeJob>();
        let rx = std::sync::Arc::new(Mutex::new(rx));
        for i in 0..PROBE_WORKERS {
            let rx = std::sync::Arc::clone(&rx);
            thread::Builder::new()
                .name(format!("oneasr-probe-{i}"))
                .spawn(move || {
                    loop {
                        let job = {
                            let guard = match rx.lock() {
                                Ok(g) => g,
                                Err(_) => break,
                            };
                            guard.recv()
                        };
                        match job {
                            Ok((path, done)) => done(probe_duration_sec(&path)),
                            Err(_) => break,
                        }
                    }
                })
                .expect("spawn duration probe worker");
        }
        tx
    })
    .clone()
}

/// Queue a duration probe on the process-wide pool (≤ [`PROBE_WORKERS`] concurrent).
///
/// Prefer this over unbounded `thread::spawn` when importing many files.
pub fn probe_duration_async(path: PathBuf, on_done: impl FnOnce(Option<f64>) + Send + 'static) {
    let _ = probe_sender().send((path, Box::new(on_done)));
}

/// Write `data` to `path` via temp file + rename (no torn product files on crash).
pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".into());
    let tmp = parent.join(format!(".{file_name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, data.as_ref())?;
    // Windows: rename fails if destination exists.
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_project_bin_ffmpeg() {
        // When tests run from workspace, cwd walk should hit D:\OneAsr\bin\ffmpeg.exe
        let ff = resolve_ffmpeg().expect("project bin/ffmpeg.exe should exist for local dev");
        assert!(ff.ends_with(FFMPEG_NAME));
        assert!(ff.is_file());
    }

    #[test]
    fn app_root_has_bin_child() {
        let root = resolve_app_root().expect("app root");
        assert!(root.join("bin").join(FFMPEG_NAME).is_file());
    }

    #[test]
    fn parse_duration_line() {
        let s = "  Duration: 00:01:30.05, start: 0.000000, bitrate: 128 kb/s\n";
        assert!((parse_duration_from_ffmpeg_stderr(s).unwrap() - 90.05).abs() < 0.01);
    }

    #[test]
    fn slice_target_pcm_roundtrip() {
        let dir = std::env::temp_dir().join(format!("oneasr_slice_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("src.wav");
        let out = dir.join("slice.wav");

        let spec = hound::WavSpec {
            channels: TARGET_CHANNELS,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        // 1.0 s of silence
        {
            let mut w = hound::WavWriter::create(&src, spec).unwrap();
            for _ in 0..TARGET_SAMPLE_RATE {
                w.write_sample(0i16).unwrap();
            }
            w.finalize().unwrap();
        }
        assert!(is_target_pcm_wav(&src));
        assert!((wav_duration_sec(&src).unwrap() - 1.0).abs() < 0.01);

        slice_wav(&src, 0.25, 0.75, &out).unwrap();
        assert!(is_target_pcm_wav(&out));
        let d = wav_duration_sec(&out).unwrap();
        assert!((d - 0.5).abs() < 0.02, "got {d}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_atomic_replaces() {
        let dir = std::env::temp_dir().join(format!("oneasr_atomic_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("out.srt");
        write_atomic(&p, "a").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a");
        write_atomic(&p, "b").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "b");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn convert_skips_ffmpeg_for_target_pcm() {
        let dir = std::env::temp_dir().join(format!("oneasr_copy_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("a.wav");
        let dst = dir.join("b.wav");
        let spec = hound::WavSpec {
            channels: TARGET_CHANNELS,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        {
            let mut w = hound::WavWriter::create(&src, spec).unwrap();
            w.write_sample(42i16).unwrap();
            w.finalize().unwrap();
        }
        convert_to_16k_mono_wav(&src, &dst).unwrap();
        assert!(dst.is_file());
        assert!(is_target_pcm_wav(&dst));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
