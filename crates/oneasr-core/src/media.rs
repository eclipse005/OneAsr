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

use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[cfg(windows)]
const FFMPEG_NAME: &str = "ffmpeg.exe";
#[cfg(not(windows))]
const FFMPEG_NAME: &str = "ffmpeg";

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("bundled ffmpeg not found (expected {{app}}/bin/ffmpeg; searched from {0})")]
    FfmpegMissing(String),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
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

/// `{app_root}/bin`
pub fn resolve_bin_dir() -> Option<PathBuf> {
    resolve_app_root().map(|root| root.join("bin"))
}

/// `{app_root}/bin/ffmpeg(.exe)`
pub fn resolve_ffmpeg() -> Result<PathBuf, MediaError> {
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

pub fn ffmpeg_available() -> bool {
    resolve_ffmpeg().is_ok()
}

/// Probe duration (seconds) via bundled ffmpeg. Returns `None` if unreadable.
///
/// Caps probe/analyze so large files (GB-class MP4) don't hang for minutes
/// scanning for duration when container headers already have it.
pub fn probe_duration_sec(input: &Path) -> Option<f64> {
    let ffmpeg = resolve_ffmpeg().ok()?;
    // Pass Path as OsStr so non-UTF8 / long Windows paths still work.
    let mut cmd = Command::new(&ffmpeg);
    cmd.args([
        "-hide_banner",
        "-probesize",
        "5M",
        "-analyzeduration",
        "5M",
        "-i",
    ])
    .arg(input);
    // Avoid flashing a console window on Windows.
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
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

/// Convert any media path to 16 kHz mono PCM WAV using bundled ffmpeg.
pub fn convert_to_16k_mono_wav(input: &Path, out_wav: &Path) -> Result<PathBuf, MediaError> {
    let ffmpeg = resolve_ffmpeg()?;
    if let Some(parent) = out_wav.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut cmd = Command::new(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"])
        .arg(out_wav);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::FfmpegFailed(stderr.chars().take(500).collect()));
    }
    Ok(out_wav.to_path_buf())
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
        assert!(ffmpeg_available());
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
}
