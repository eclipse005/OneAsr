//! Media prep via bundled tools under `{app_root}/bin/`.
//!
//! Layout (install and dev share the same convention; a Windows build only
//! adds the `.exe` suffix):
//!
//! ```text
//! OneAsr/                 ← app root
//!   oneasr[.exe]          ← (or target/debug/oneasr in dev)
//!   bin/
//!     ffmpeg[.exe]
//! ```
//!
//! Not user-configurable: after install, tools live in the install directory's
//! `bin/`. We only *locate* that directory relative to the running executable
//! (walking up for `cargo run` from `target/debug`). When `bin/` holds no
//! usable binary — source checkouts, distro packages — the pipeline falls back
//! to an `ffmpeg` on `PATH`; see [`ffmpeg_source`].
//!
//! **Pipeline audio contract**: ASR/Aligner/VAD all consume **16 kHz mono PCM
//! s16le WAV**. Once converted, further work (duration, slice) stays in-process
//! via `hound` — no ffmpeg round-trips for PCM.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;

use std::time::{Duration, Instant, SystemTime};

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
    #[error("ffmpeg not found: expected {0}, or install ffmpeg on PATH")]
    FfmpegMissing(String),
    #[error("bundled ffmpeg is not executable: {0} (chmod +x, or install ffmpeg on PATH)")]
    FfmpegNotExecutable(PathBuf),
    #[error("ffmpeg failed: {0}")]
    FfmpegFailed(String),
    #[error("wav error: {0}")]
    Wav(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Where the pipeline found ffmpeg.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FfmpegSource {
    /// `{app_root}/bin/ffmpeg[.exe]` — the build shipped with the package.
    Bundled(PathBuf),
    /// `ffmpeg[.exe]` resolved from `PATH` (system install).
    System(PathBuf),
}

impl FfmpegSource {
    pub fn path(&self) -> &Path {
        match self {
            Self::Bundled(path) | Self::System(path) => path,
        }
    }
}

impl std::fmt::Display for FfmpegSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Bundled(path) => write!(f, "bundled ({})", path.display()),
            Self::System(path) => write!(f, "system ({})", path.display()),
        }
    }
}

/// App root = directory that contains `bin/ffmpeg(.exe)`.
///
/// Search:
/// 1. `current_exe().parent()` and ancestors (covers install + `target/debug`)
/// 2. `current_dir()` and ancestors (covers tests / odd cwd)
pub fn resolve_app_root() -> Option<PathBuf> {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
        && let Some(root) = app_root_from(dir)
    {
        return Some(root);
    }
    if let Ok(cwd) = std::env::current_dir()
        && let Some(root) = app_root_from(&cwd)
    {
        return Some(root);
    }
    None
}

/// Walk up from `start`, at most 8 levels, for the directory that holds
/// `bin/ffmpeg[.exe]`.
///
/// Split out of [`resolve_app_root`] on purpose: that one reads
/// process-global state (`current_exe` / `current_dir`), so it cannot be
/// exercised from a parallel test run — which is exactly how the walk
/// went untested and only its Windows-only environment side effect got
/// asserted. Taking the start directory as an argument makes the walk
/// itself testable on every platform.
fn app_root_from(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    for _ in 0..8 {
        if dir.join("bin").join(FFMPEG_NAME).is_file() {
            return Some(dir);
        }
        if !dir.pop() {
            break;
        }
    }
    None
}

fn resolve_bin_dir() -> Option<PathBuf> {
    resolve_app_root().map(|root| root.join("bin"))
}

/// The ffmpeg the pipeline will run, or `None` when neither the bundled binary
/// nor a system one on `PATH` is usable (startup environment report).
pub fn ffmpeg_source() -> Option<FfmpegSource> {
    locate_ffmpeg().ok()
}

/// Bundled `{app_root}/bin/ffmpeg[.exe]` first — the build the package was
/// tested with — then the first executable `ffmpeg[.exe]` on `PATH`.
fn locate_ffmpeg() -> Result<FfmpegSource, MediaError> {
    let bundled = resolve_bin_dir().map(|dir| dir.join(FFMPEG_NAME));
    let mut not_executable = None;

    if let Some(path) = &bundled
        && path.is_file()
    {
        if ensure_executable(path) {
            return Ok(FfmpegSource::Bundled(path.clone()));
        }
        not_executable = Some(path.clone());
    }

    if let Some(system) = std::env::var_os("PATH").and_then(|var| find_ffmpeg_in_path(&var)) {
        return Ok(FfmpegSource::System(system));
    }

    Err(match not_executable {
        Some(path) => MediaError::FfmpegNotExecutable(path),
        None => MediaError::FfmpegMissing(
            bundled
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| format!("<app>/bin/{FFMPEG_NAME}")),
        ),
    })
}

fn resolve_ffmpeg() -> Result<PathBuf, MediaError> {
    locate_ffmpeg().map(|source| source.path().to_path_buf())
}

/// First `ffmpeg[.exe]` on a `PATH`-style list. Unix requires the execute bit,
/// so a stray data file named `ffmpeg` cannot win.
fn find_ffmpeg_in_path(path_var: &std::ffi::OsStr) -> Option<PathBuf> {
    std::env::split_paths(path_var)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(FFMPEG_NAME))
        .find(|candidate| is_executable_file(candidate))
}

fn is_executable_file(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// Windows resolves by filename alone; Unix needs the execute bit, which a
/// portable archive round-trip can drop. Repair it in place when the install
/// directory is writable.
fn ensure_executable(path: &Path) -> bool {
    if is_executable_file(path) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(path)
            && meta.is_file()
        {
            let mut perms = meta.permissions();
            perms.set_mode(perms.mode() | 0o755);
            return std::fs::set_permissions(path, perms).is_ok();
        }
    }
    false
}

fn ffmpeg_command(ffmpeg: &Path) -> Command {
    // `mut` is only needed by the Windows-only `creation_flags` call below.
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut cmd = Command::new(ffmpeg);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Upper bound for a metadata probe (headers only, never full decode).
const PROBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Run a command with a wall-clock budget, returning `(success, stderr)`.
///
/// Returns `None` when the child could not be spawned or was killed after
/// `timeout`. Stderr is drained on a helper thread so a chatty child cannot
/// deadlock on a full pipe while we wait.
fn run_capture_stderr(cmd: &mut Command, timeout: Duration) -> Option<(bool, String)> {
    use std::process::Stdio;
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    let mut child = cmd.spawn().ok()?;
    let mut stderr_pipe = child.stderr.take();
    let reader = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(pipe) = stderr_pipe.as_mut() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf
    });

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(_) => {
                let _ = child.kill();
                break None;
            }
        }
    };
    let stderr = reader.join().unwrap_or_default();
    status.map(|s| (s.success(), stderr))
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

// ─── 媒体类型判定 ───────────────────────────────────────────────────

/// 视频容器扩展名 —— 界面图标与 CLI `probe` 共用这一份表。
///
/// 判定只在 core 做一次：`oneasr-cli probe` 靠它给 `kind`，GUI 的
/// `is_video_format` 也转调 [`is_video_ext`]（同一个问题不再有第二个答案）。
pub const VIDEO_EXTS: &[&str] = &[
    "mp4", "mkv", "mov", "webm", "avi", "flv", "ts", "m4v", "mpeg", "mpg", "wmv", "3gp",
];

/// 扩展名（大小写不限、可带前导点）是不是视频容器。
pub fn is_video_ext(ext: &str) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    VIDEO_EXTS.iter().any(|known| *known == ext)
}

/// 音频扩展名 —— 与仓库实际支持的输入一致（CLI `transcribe` / `align` 收的
/// 音视频、GUI 添加对话框的「Media」过滤器、文稿配对里的媒体清单，过去三处各写
/// 一份，现在都从这一张表回答）。
///
/// 表里没有的（含没有扩展名的文件）是 [`MediaKind::Other`]，**不再默认按音频报**：
/// `probe` 把 `.txt` 说成音频，壳就会给它挂上音频图标——那是壳在替后端猜。
/// ffmpeg 能解的容器远不止这些；这张表回答的是「OneAsr 认得的输入」，不是
/// 「ffmpeg 认得的输入」。
pub const AUDIO_EXTS: &[&str] = &["wav", "mp3", "flac", "m4a", "aac", "ogg", "opus", "wma"];

/// 扩展名（大小写不限、可带前导点）是不是音频。
pub fn is_audio_ext(ext: &str) -> bool {
    let ext = ext.trim_start_matches('.').to_ascii_lowercase();
    AUDIO_EXTS.iter().any(|known| *known == ext)
}

/// 全仓库唯一的「哪些扩展名算音视频」清单：[`AUDIO_EXTS`] 在前、[`VIDEO_EXTS`]
/// 在后，各自保持声明顺序（顺序稳定，壳拿它拼文件过滤器）。
///
/// 返回值全小写、不带点；同一个扩展名若同时写进两张表，只留第一次出现的那个。
/// `status --json` 的 `media_exts` 就是它——壳不必再维护第二份正则。
pub fn supported_exts() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::with_capacity(AUDIO_EXTS.len() + VIDEO_EXTS.len());
    for ext in AUDIO_EXTS.iter().chain(VIDEO_EXTS.iter()).copied() {
        // 规范写法是契约（单测逐条钉住）：表里只许出现小写、不带点的拼法。
        let normalized = ext.trim_start_matches('.').to_ascii_lowercase();
        debug_assert!(
            ext == normalized.as_str(),
            "扩展名表里只许写小写、不带点的规范写法: {ext}"
        );
        if !out.contains(&ext) {
            out.push(ext);
        }
    }
    out
}

/// 媒体类型：落在 [`AUDIO_EXTS`] 的算音频、落在 [`VIDEO_EXTS`] 的算视频，
/// 其余（含没有扩展名）是 [`MediaKind::Other`]。
///
/// **不再拿音频当兜底**：判不出来时按音频报，`probe` 就会把 `.txt` 说成音频，
/// 壳于是在列表里给它挂音频图标——那是壳/后端替对方猜。判不出来就说判不出来，
/// 由调用方决定是拒绝还是照传（转写本身不看扩展名，时长探测对容器不敏感）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Audio,
    Video,
    /// 扩展名不在 [`AUDIO_EXTS`] / [`VIDEO_EXTS`] 里（含没有扩展名）。
    Other,
}

impl MediaKind {
    /// 机器可读值（`probe --json` 的 `kind` 字段）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
            Self::Other => "other",
        }
    }

    /// 人读标签（双语）。
    pub fn label(self, lang: crate::i18n::UiLang) -> &'static str {
        use crate::i18n::UiLang;
        match (self, lang) {
            (Self::Audio, UiLang::Zh) => "音频",
            (Self::Audio, UiLang::En) => "audio",
            (Self::Video, UiLang::Zh) => "视频",
            (Self::Video, UiLang::En) => "video",
            (Self::Other, UiLang::Zh) => "其他",
            (Self::Other, UiLang::En) => "other",
        }
    }
}

/// 按扩展名判定媒体类型（见 [`MediaKind`]）：两张表都不在 → `Other`。
pub fn media_kind(path: &Path) -> MediaKind {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return MediaKind::Other;
    };
    if is_video_ext(ext) {
        MediaKind::Video
    } else if is_audio_ext(ext) {
        MediaKind::Audio
    } else {
        MediaKind::Other
    }
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
    // ffmpeg exits non-zero without an output target; duration still lands on
    // stderr. The timeout matters more: a corrupt container must not pin a
    // probe worker (and with it the whole import queue) forever.
    let (_success, stderr) = run_capture_stderr(&mut cmd, PROBE_TIMEOUT)?;
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

/// Extract the audio track to a PCM s16le **stereo** WAV at 44.1 kHz — the
/// sample rate HTDemucs inference requires. ffmpeg downmixes multi-channel
/// sources (mono is duplicated) and resamples; the vocals WAV is later
/// transcoded to the 16 kHz mono master.
pub fn extract_audio_wav(input: &Path, out_wav: &Path) -> Result<PathBuf, MediaError> {
    if let Some(parent) = out_wav.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let ffmpeg = resolve_ffmpeg()?;
    let mut cmd = ffmpeg_command(&ffmpeg);
    cmd.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "2", "-ar", "44100", "-c:a", "pcm_s16le"])
        .arg(out_wav);
    let output = cmd.output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MediaError::FfmpegFailed(stderr.chars().take(500).collect()));
    }
    Ok(out_wav.to_path_buf())
}

/// Slice `[start, end)` seconds from a pipeline PCM WAV into `out_wav`.
///
/// Uses in-process sample I/O (no ffmpeg). Input must be target PCM
/// ([`is_target_pcm_wav`]); that is always true after [`convert_to_16k_mono_wav`].
pub fn slice_wav(
    input: &Path,
    start: f32,
    end: f32,
    out_wav: &Path,
) -> Result<PathBuf, MediaError> {
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

    let mut writer = hound::WavWriter::create(out_wav, spec)
        .map_err(|e| MediaError::Wav(format!("create {}: {e}", out_wav.display())))?;

    if n == 0 {
        writer
            .finalize()
            .map_err(|e| MediaError::Wav(format!("finalize: {e}")))?;
        return Ok(out_wav.to_path_buf());
    }

    // hound::seek is in PCM frames from the data chunk, not file bytes.
    reader
        .seek(start_i)
        .map_err(|e| MediaError::Wav(format!("seek: {e}")))?;
    let byte_len = (n as usize).saturating_mul(2);
    let mut buf = vec![0u8; byte_len];
    reader
        .into_inner()
        .read_exact(&mut buf)
        .map_err(|e| MediaError::Wav(format!("read samples: {e}")))?;
    for chunk in buf.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
        writer
            .write_sample(sample)
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

/// Queue a duration probe on the process-wide pool (≤ `PROBE_WORKERS` concurrent).
///
/// Prefer this over unbounded `thread::spawn` when importing many files.
pub fn probe_duration_async(path: PathBuf, on_done: impl FnOnce(Option<f64>) + Send + 'static) {
    let _ = probe_sender().send((path, Box::new(on_done)));
}

/// Unique suffix for atomic-write temp files (PID + timestamp + random),
/// preventing collisions across rapid restarts that reuse the same PID on Windows.
fn atomic_tmp_suffix() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(1);
    let ts = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u32)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "{:08x}.{:08x}.{:06x}",
        std::process::id(),
        ts.wrapping_add(seq),
        seq
    )
}

/// Write `data` to `path` via temp file + rename (no torn product files on crash).
pub fn write_atomic(path: &Path, data: impl AsRef<[u8]>) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "out".into());
    let tmp = parent.join(format!(".{file_name}.{}.tmp", atomic_tmp_suffix()));
    std::fs::write(&tmp, data.as_ref())?;
    // `fs::rename` maps to MoveFileExW(MOVEFILE_REPLACE_EXISTING) on Windows,
    // so it atomically replaces an existing destination. Deleting first would
    // open a window where a crash/AV lock loses the previous good file.
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

    /// 扩展名 → 类型：音频 / 视频各认各的，判不出来是 `Other`，不是音频。
    ///
    /// 断骨锚点：把 `media_kind` 的 `Other` 分支改回 `MediaKind::Audio`
    /// （老行为：非视频一律按音频），`.txt` 与没有扩展名那两条立刻变红。
    #[test]
    fn media_kind_maps_audio_video_and_unknown_to_other() {
        assert_eq!(media_kind(Path::new("a.wav")), MediaKind::Audio);
        assert_eq!(media_kind(Path::new("a.MP3")), MediaKind::Audio);
        assert_eq!(media_kind(Path::new("a.mp4")), MediaKind::Video);
        assert_eq!(media_kind(Path::new("CLIP.MKV")), MediaKind::Video);
        assert_eq!(media_kind(Path::new("notes.txt")), MediaKind::Other);
        assert_eq!(media_kind(Path::new("plain")), MediaKind::Other);
        assert_eq!(media_kind(Path::new("archive.tar.gz")), MediaKind::Other);
        // 机器可读值就是壳读的那个字符串。
        assert_eq!(MediaKind::Other.as_str(), "other");
        assert_eq!(MediaKind::Audio.as_str(), "audio");
        assert_eq!(MediaKind::Video.as_str(), "video");
    }

    /// 两张表拼出来的清单：全小写、不带点、无重复，且覆盖 wav 与 mp4。
    ///
    /// 断骨锚点：把 `supported_exts` 的去重删掉、或往任一张表里塞一个大写 / 带点
    /// 的写法，下面任一条断言变红。
    #[test]
    fn supported_exts_is_normalized_deduplicated_and_covers_the_product_set() {
        let exts = supported_exts();
        assert!(exts.contains(&"wav"), "音频代表项必须在: {exts:?}");
        assert!(exts.contains(&"mp4"), "视频代表项必须在: {exts:?}");
        assert!(!exts.contains(&"txt"), "非媒体扩展名不许混进来: {exts:?}");
        for ext in &exts {
            assert_eq!(*ext, ext.to_ascii_lowercase(), "必须全小写: {ext}");
            assert!(!ext.starts_with('.'), "必须不带点: {ext}");
            assert!(!ext.trim().is_empty(), "空扩展名没有意义");
        }
        let mut seen = std::collections::HashSet::new();
        for ext in &exts {
            assert!(seen.insert(*ext), "重复的扩展名: {ext}");
        }
        // 顺序稳定且可预期：音频表在前、视频表在后，各自保持声明顺序。
        let expected: Vec<&str> = AUDIO_EXTS
            .iter()
            .chain(VIDEO_EXTS.iter())
            .copied()
            .collect();
        let mut unique = Vec::new();
        for ext in expected {
            if !unique.contains(&ext) {
                unique.push(ext);
            }
        }
        assert_eq!(exts, unique, "顺序必须稳定（音频在前、视频在后）");
    }

    #[test]
    fn app_root_walks_up_to_the_dir_holding_bin_ffmpeg() {
        // 自建临时安装树，不依赖仓库里那个被 .gitignore 的
        // bin/ffmpeg（CI 的 checkout 里根本没有，只有 Windows 本地有）。
        let root = std::env::temp_dir().join(format!("oneasr_app_root_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // 应用被埋在几层目录里（安装目录 / target/debug 那种形状）
        let deep = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();

        // 树里还没有 bin/ffmpeg：temp 树里的任何目录都不该被当成 app root。
        // 只断言这一点 —— 再往上（/tmp、/）是否恰好装着 ffmpeg
        // 取决于机器，断言那个又变回环境检查。
        let found = app_root_from(&deep);
        assert!(
            !found.as_deref().is_some_and(|p| p.starts_with(&root)),
            "没有 bin/ffmpeg 的目录不该被当成 app root: {found:?}"
        );

        // 装上 ffmpeg 之后：从根命中、从三层深的目录向上命中，
        // 结果是同一个根。
        std::fs::create_dir_all(root.join("bin")).unwrap();
        std::fs::write(root.join("bin").join(FFMPEG_NAME), b"stub").unwrap();
        assert_eq!(app_root_from(&root), Some(root.clone()));
        assert_eq!(app_root_from(&deep), Some(root.clone()));
        // 目录名中间了一个不存在的中间层也不影响结果
        assert_eq!(app_root_from(&root.join("a").join("b")), Some(root.clone()));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_lookup_accepts_executable_ffmpeg() {
        let dir = std::env::temp_dir().join(format!("oneasr_path_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join(FFMPEG_NAME);
        std::fs::write(&fake, b"stub").unwrap();
        assert!(ensure_executable(&fake));

        assert_eq!(find_ffmpeg_in_path(dir.as_os_str()), Some(fake));
        // Empty entries (leading/trailing separators) must be skipped, not
        // resolved against the current directory.
        assert_eq!(find_ffmpeg_in_path(std::ffi::OsStr::new("")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn path_lookup_skips_non_executable_and_repairs_it() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oneasr_noexec_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fake = dir.join(FFMPEG_NAME);
        std::fs::write(&fake, b"stub").unwrap();
        std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o644)).unwrap();

        assert_eq!(find_ffmpeg_in_path(dir.as_os_str()), None);
        assert!(ensure_executable(&fake));
        assert_eq!(find_ffmpeg_in_path(dir.as_os_str()), Some(fake));
        let _ = std::fs::remove_dir_all(&dir);
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
        // 1.0 s ramp so the slice region is identifiable (not just duration).
        {
            let mut w = hound::WavWriter::create(&src, spec).unwrap();
            for i in 0..TARGET_SAMPLE_RATE {
                w.write_sample(i as i16).unwrap();
            }
            w.finalize().unwrap();
        }
        assert!(is_target_pcm_wav(&src));
        assert!((wav_duration_sec(&src).unwrap() - 1.0).abs() < 0.01);

        slice_wav(&src, 0.25, 0.75, &out).unwrap();
        assert!(is_target_pcm_wav(&out));
        let d = wav_duration_sec(&out).unwrap();
        assert!((d - 0.5).abs() < 0.02, "got {d}");

        let samples: Vec<i16> = hound::WavReader::open(&out)
            .unwrap()
            .samples::<i16>()
            .map(|s| s.unwrap())
            .collect();
        let start_i = ((0.25f64) * f64::from(TARGET_SAMPLE_RATE)).round() as i16;
        assert_eq!(samples.first().copied(), Some(start_i));
        assert_eq!(
            samples.last().copied(),
            Some(start_i + (samples.len() as i16) - 1)
        );

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
