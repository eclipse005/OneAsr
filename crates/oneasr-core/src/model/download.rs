//! ModelScope download into `{app}/models/{name}/`.
//!
//! **Contract (single authority for terminal state)**
//! - `on_progress` is **only** for in-flight `Downloading` ticks (bytes/speed).
//! - Terminal outcome is **only** the returned [`DownloadOutcome`] — never double-emitted.
//! - Callers must not invent a second Failed/Completed event from `Err`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::catalog::{model_definition, ModelDownloadFile, ModelId};

use super::http::{
    content_length_total, content_range_start, discard_part, download_client, initial_bytes,
    is_success, modelscope_request, retry_backoff, sleep_cancellable, trace_download,
};
use super::ready::file_meets_ready_threshold;
#[cfg(test)]
use super::ready::probe_writable;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadState {
    Idle,
    Downloading,
    Completed,
    Failed,
    Cancelled,
}

/// In-flight progress only. Terminal results use [`DownloadOutcome`].
#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub state: DownloadState,
    pub model_id: ModelId,
    /// Install-layout destination (`{app}/models/{name}`).
    pub model_dir: PathBuf,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub message: String,
}

impl DownloadProgress {
    pub fn percent(&self) -> f32 {
        if self.total_bytes == 0 {
            return 0.0;
        }
        ((self.downloaded_bytes as f64 / self.total_bytes as f64) * 100.0).clamp(0.0, 100.0) as f32
    }

    pub fn label(&self) -> String {
        self.label_for_kind(self.model_id.kind())
    }

    /// Human status for a model download.
    pub fn label_for_kind(&self, _kind: crate::model::ModelKind) -> String {
        match self.state {
            DownloadState::Idle => "未下载".into(),
            DownloadState::Downloading => {
                let pct = self.percent();
                let speed = format_speed(self.speed_bytes_per_sec);
                format!("下载中 {pct:.0}% · {speed}")
            }
            DownloadState::Completed => "已就绪".into(),
            DownloadState::Failed => {
                if self.message.is_empty() {
                    "下载失败".into()
                } else {
                    format!("失败: {}", truncate(&self.message, 48))
                }
            }
            DownloadState::Cancelled => "已取消".into(),
        }
    }

    /// Build a UI snapshot for a terminal outcome (single place for Failed/Cancelled/Completed).
    pub fn from_outcome(id: ModelId, model_dir: PathBuf, outcome: &DownloadOutcome) -> Self {
        match outcome {
            DownloadOutcome::Completed { .. } => Self {
                state: DownloadState::Completed,
                model_id: id,
                model_dir,
                downloaded_bytes: 0,
                total_bytes: 0,
                speed_bytes_per_sec: 0,
                message: "done".into(),
            },
            DownloadOutcome::Cancelled {
                downloaded_bytes,
                total_bytes,
            } => Self {
                state: DownloadState::Cancelled,
                model_id: id,
                model_dir,
                downloaded_bytes: *downloaded_bytes,
                total_bytes: *total_bytes,
                speed_bytes_per_sec: 0,
                message: "cancelled".into(),
            },
            DownloadOutcome::Failed {
                message,
                downloaded_bytes,
                total_bytes,
            } => Self {
                state: DownloadState::Failed,
                model_id: id,
                model_dir,
                downloaded_bytes: *downloaded_bytes,
                total_bytes: *total_bytes,
                speed_bytes_per_sec: 0,
                message: message.clone(),
            },
        }
    }
}

/// Sole terminal result of a download job.
#[derive(Debug, Clone)]
pub enum DownloadOutcome {
    Completed {
        model_dir: PathBuf,
    },
    Cancelled {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
    Failed {
        message: String,
        downloaded_bytes: u64,
        total_bytes: u64,
    },
}

/// Cancel handle for one download job (share with UI).
#[derive(Clone)]
pub struct DownloadHandle {
    pub model_id: ModelId,
    pub model_dir: PathBuf,
    cancel: Arc<AtomicBool>,
}

impl DownloadHandle {
    pub fn new(model_id: ModelId) -> Self {
        let def = model_definition(model_id);
        Self {
            model_id,
            model_dir: def.model_dir,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Download (or resume) into the install-layout dir for `id`.
///
/// Progress callback receives **Downloading only**. Terminal state is `DownloadOutcome`.
pub fn download_model(
    handle: &DownloadHandle,
    mut on_progress: impl FnMut(DownloadProgress),
) -> DownloadOutcome {
    let id = handle.model_id;
    let definition = model_definition(id);
    let model_dir = definition.model_dir.clone();
    let cancel = Arc::clone(&handle.cancel);

    if let Err(e) = std::fs::create_dir_all(&model_dir) {
        return DownloadOutcome::Failed {
            message: e.to_string(),
            downloaded_bytes: 0,
            total_bytes: 0,
        };
    }

    let (downloaded_bytes, total_bytes) = initial_bytes(&definition);

    let client = match download_client() {
        Ok(c) => c,
        Err(e) => {
            return DownloadOutcome::Failed {
                message: e,
                downloaded_bytes,
                total_bytes,
            };
        }
    };

    let mut ctx = FileDownloadCtx {
        client,
        cancel: &cancel,
        model_id: id,
        model_dir: &model_dir,
        on_progress: &mut on_progress,
        downloaded_bytes,
        total_bytes,
        last_speed_mark: Instant::now(),
        last_speed_bytes: downloaded_bytes,
    };
    ctx.emit(0, "starting");

    for file in &definition.download_files {
        if ctx.cancelled() {
            return ctx.cancelled_outcome();
        }

        let target = model_dir.join(&file.file_name);
        let part_path = model_dir.join(format!("{}.part", file.file_name.replace('/', "_")));

        // A product file that does not match the pinned size is a truncated or
        // stale leftover: drop it so `.part` is rebuilt from scratch.
        if target.is_file() {
            if file_meets_ready_threshold(&target, file.expected_size) {
                continue;
            }
            let _ = std::fs::remove_file(&target);
        }

        if let Some(parent) = target.parent()
            && let Err(e) = std::fs::create_dir_all(parent) {
            return ctx.failed(e.to_string());
        }

        let mut attempt = 1_u32;
        loop {
            match download_one_file(&mut ctx, file, &target, &part_path) {
                Ok(()) => break,
                Err(FileDownloadError::Cancelled) => return ctx.cancelled_outcome(),
                Err(FileDownloadError::Permanent(message)) => {
                    return ctx.failed(format!("{}: {message}", file.file_name));
                }
                Err(FileDownloadError::Transient(message)) if attempt < MAX_FILE_ATTEMPTS => {
                    trace_download(&format!(
                        "{} transient failure (attempt {attempt}/{MAX_FILE_ATTEMPTS}): {message}",
                        file.file_name
                    ));
                    if !sleep_cancellable(ctx.cancel, retry_backoff(attempt)) {
                        return ctx.cancelled_outcome();
                    }
                    attempt += 1;
                }
                Err(FileDownloadError::Transient(message)) => {
                    return ctx.failed(format!(
                        "{}: {message}（已尝试 {MAX_FILE_ATTEMPTS} 次，可再次点击下载续传）",
                        file.file_name
                    ));
                }
            }
        }
    }

    let (downloaded_bytes, total_bytes) = (ctx.downloaded_bytes, ctx.total_bytes);
    let bad: Vec<String> = definition
        .download_files
        .iter()
        .filter_map(|f| {
            let path = definition.model_dir.join(&f.file_name);
            if file_meets_ready_threshold(&path, f.expected_size) {
                return None;
            }
            let actual = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            Some(format!(
                "{}（{actual}/{} 字节）",
                f.file_name, f.expected_size
            ))
        })
        .collect();
    if !bad.is_empty() {
        return DownloadOutcome::Failed {
            message: format!("文件缺失或损坏: {}", bad.join("、")),
            downloaded_bytes,
            total_bytes,
        };
    }

    DownloadOutcome::Completed {
        model_dir: definition.model_dir,
    }
}

/// Shared mutable download state for the current file + progress emission.
pub(super) struct FileDownloadCtx<'a> {
    client: &'a reqwest::blocking::Client,
    cancel: &'a AtomicBool,
    model_id: ModelId,
    model_dir: &'a Path,
    on_progress: &'a mut dyn FnMut(DownloadProgress),
    downloaded_bytes: u64,
    total_bytes: u64,
    last_speed_mark: Instant,
    last_speed_bytes: u64,
}

impl FileDownloadCtx<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn cancelled_outcome(&self) -> DownloadOutcome {
        DownloadOutcome::Cancelled {
            downloaded_bytes: self.downloaded_bytes,
            total_bytes: self.total_bytes,
        }
    }

    fn failed(&self, message: impl Into<String>) -> DownloadOutcome {
        DownloadOutcome::Failed {
            message: message.into(),
            downloaded_bytes: self.downloaded_bytes,
            total_bytes: self.total_bytes,
        }
    }

    fn emit(&mut self, speed: u64, message: &str) {
        (self.on_progress)(DownloadProgress {
            state: DownloadState::Downloading,
            model_id: self.model_id,
            model_dir: self.model_dir.to_path_buf(),
            downloaded_bytes: self.downloaded_bytes,
            total_bytes: self.total_bytes,
            speed_bytes_per_sec: speed,
            message: message.to_string(),
        });
    }

    /// Account `n` bytes written and emit a throttled progress tick.
    fn add_written(&mut self, n: u64, file_name: &str) {
        self.downloaded_bytes = self.downloaded_bytes.saturating_add(n);
        // Keep total ahead of downloaded so percent never exceeds 100.
        if self.downloaded_bytes > self.total_bytes {
            self.total_bytes = self.downloaded_bytes;
        }
        let elapsed = self.last_speed_mark.elapsed().as_secs_f64();
        if elapsed >= 0.35 {
            let speed = ((self.downloaded_bytes.saturating_sub(self.last_speed_bytes) as f64)
                / elapsed)
                .round() as u64;
            self.last_speed_mark = Instant::now();
            self.last_speed_bytes = self.downloaded_bytes;
            self.emit(speed, file_name);
        }
    }

    /// Discard bytes that no longer count (server ignored Range and restarted).
    pub(super) fn subtract(&mut self, n: u64) {
        self.downloaded_bytes = self.downloaded_bytes.saturating_sub(n);
        self.last_speed_bytes = self.last_speed_bytes.saturating_sub(n);
    }

    /// Reconcile the UI total with the server-declared full size for this file.
    fn reconcile_total(&mut self, file: &ModelDownloadFile, remote_total: u64) {
        let other = self.total_bytes.saturating_sub(file.expected_size);
        self.total_bytes = other.saturating_add(remote_total.max(file.expected_size));
    }
}

#[derive(Debug)]
pub(super) enum FileDownloadError {
    Cancelled,
    /// Worth retrying (network hiccup, timeout, short body, 5xx).
    Transient(String),
    /// Retrying cannot help (4xx, disk error).
    Permanent(String),
}

/// Attempts per file before surfacing the failure (resume makes retries cheap).
const MAX_FILE_ATTEMPTS: u32 = 3;

/// Per-read idle timeout: each `Response::read` call gets a fresh window, so a
/// stalled connection fails within a minute while large, flowing downloads run
/// to completion.
pub(super) const READ_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

/// One HTTP attempt at downloading `file` into `part_path` (resume-aware),
/// renaming to `target` only after both the exact size and SHA-256 match.
fn download_one_file(
    ctx: &mut FileDownloadCtx<'_>,
    file: &ModelDownloadFile,
    target: &Path,
    part_path: &Path,
) -> Result<(), FileDownloadError> {
    let expected = file.expected_size;
    let mut part_bytes = std::fs::metadata(part_path).map(|m| m.len()).unwrap_or(0);

    // Stale oversized part (e.g. from a previous catalog revision): restart.
    if part_bytes > expected {
        discard_part(ctx, part_path, part_bytes, expected);
        part_bytes = 0;
    }

    // A complete `.part` from a previous run needs no network round-trip, but
    // it must pass SHA-256 before it may become the product file.
    if part_bytes == expected {
        match sha256_matches(part_path, file.sha256) {
            Ok(true) => return finish_part_file(part_path, target),
            Ok(false) => {
                discard_part(ctx, part_path, part_bytes, expected);
                part_bytes = 0;
            }
            Err(e) => return Err(FileDownloadError::Transient(e.to_string())),
        }
    }

    let mut response = modelscope_request(ctx.client, &file.url, part_bytes)
        .send()
        .map_err(|e| FileDownloadError::Transient(e.to_string()))?;

    if response.status() == reqwest::StatusCode::OK && part_bytes > 0 {
        // Server ignored `Range` and served the whole file: discard the part
        // (and its progress accounting) and request a clean copy.
        discard_part(ctx, part_path, part_bytes, expected);
        part_bytes = 0;
        response = modelscope_request(ctx.client, &file.url, 0)
            .send()
            .map_err(|e| FileDownloadError::Transient(e.to_string()))?;
    }

    if response.status() == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
        // Resume offset past the remote end: the part cannot be trusted
        // (a valid complete part was already finished above), so restart.
        discard_part(ctx, part_path, part_bytes, expected);
        return Err(FileDownloadError::Transient(
            "服务器拒绝断点续传（HTTP 416），已重置".into(),
        ));
    }

    if !is_success(response.status()) {
        let status = response.status();
        let message = format!("HTTP {status}");
        // 4xx usually means a bad URL/permission, but timeout/rate-limit
        // responses are worth another attempt.
        let permanent = status.is_client_error()
            && status != reqwest::StatusCode::REQUEST_TIMEOUT
            && status != reqwest::StatusCode::TOO_MANY_REQUESTS;
        return Err(if permanent {
            FileDownloadError::Permanent(message)
        } else {
            FileDownloadError::Transient(message)
        });
    }

    if part_bytes > 0 {
        // Guard against a server that ignores Range: a resume must continue
        // exactly where `.part` ends, otherwise two offsets would be spliced.
        match content_range_start(&response) {
            Some(start) if start == part_bytes => {}
            _ => {
                discard_part(ctx, part_path, part_bytes, expected);
                return Err(FileDownloadError::Transient(
                    "服务器返回了错误的续传偏移，已重置".into(),
                ));
            }
        }
    }

    if let Some(remote_total) = content_length_total(&response, part_bytes) {
        ctx.reconcile_total(file, remote_total);
    }

    let mut output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(part_path)
        .map_err(|e| FileDownloadError::Permanent(e.to_string()))?;

    let mut buf = [0_u8; 64 * 1024];
    loop {
        if ctx.cancelled() {
            // Keep `.part` so the next download resumes.
            return Err(FileDownloadError::Cancelled);
        }
        let n = match response.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            // Keep `.part`: the retry resumes from the bytes already written.
            Err(e) => return Err(FileDownloadError::Transient(e.to_string())),
        };
        output
            .write_all(&buf[..n])
            .map_err(|e| FileDownloadError::Permanent(e.to_string()))?;
        ctx.add_written(n as u64, &file.file_name);
    }
    drop(output);

    let have = std::fs::metadata(part_path).map(|m| m.len()).unwrap_or(0);
    if have != expected {
        return Err(FileDownloadError::Transient(format!(
            "文件不完整（{have}/{expected} 字节）"
        )));
    }
    // The pinned revision is immutable, so content is verified before rename;
    // a mismatch is dropped and re-downloaded (retry budget applies).
    match sha256_matches(part_path, file.sha256) {
        Ok(true) => finish_part_file(part_path, target),
        Ok(false) => {
            discard_part(ctx, part_path, have, expected);
            Err(FileDownloadError::Transient(
                "SHA-256 校验失败，已删除并重新下载".into(),
            ))
        }
        Err(e) => Err(FileDownloadError::Transient(e.to_string())),
    }
}

/// Compare `path`'s SHA-256 against `expected_hex` (case-insensitive).
fn sha256_matches(path: &Path, expected_hex: &str) -> std::io::Result<bool> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 1024 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Ok(hex.eq_ignore_ascii_case(expected_hex))
}

fn finish_part_file(part_path: &Path, target: &Path) -> Result<(), FileDownloadError> {
    if let Some(parent) = target.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::rename(part_path, target).map_err(|e| FileDownloadError::Permanent(e.to_string()))
}

fn format_speed(bps: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    let x = bps as f64;
    if x >= MB {
        format!("{:.1} MB/s", x / MB)
    } else if x >= KB {
        format!("{:.0} KB/s", x / KB)
    } else if bps > 0 {
        format!("{bps} B/s")
    } else {
        "…".into()
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn catalog_size_rejects_empty_truncated_and_oversized() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr-ready-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.bin");

        assert!(!file_meets_ready_threshold(&path, 1000));

        std::fs::File::create(&path).unwrap();
        assert!(!file_meets_ready_threshold(&path, 1000));

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        drop(f);
        assert!(!file_meets_ready_threshold(&path, 1000));

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 500]).unwrap();
        drop(f);
        assert!(!file_meets_ready_threshold(&path, 1000));

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 1000]).unwrap();
        drop(f);
        assert!(file_meets_ready_threshold(&path, 1000));

        // Oversized files are not "ready" either (pinned revision is exact).
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 1001]).unwrap();
        drop(f);
        assert!(!file_meets_ready_threshold(&path, 1000));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sha256_matches_lowercase_and_uppercase() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr-sha-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("abc.bin");
        std::fs::write(&path, b"abc").unwrap();

        let known = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(sha256_matches(&path, known).unwrap());
        assert!(sha256_matches(&path, &known.to_uppercase()).unwrap());
        assert!(!sha256_matches(&path, &format!("0{known}")).unwrap());

        std::fs::write(&path, b"abd").unwrap();
        assert!(!sha256_matches(&path, known).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sleep_cancellable_honours_cancel_flag() {
        let cancelled = AtomicBool::new(true);
        assert!(!sleep_cancellable(&cancelled, Duration::from_millis(200)));

        let running = AtomicBool::new(false);
        assert!(sleep_cancellable(&running, Duration::from_millis(10)));
    }

    #[test]
    fn probe_writable_creates_dir_and_cleans_up() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr-probe-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        probe_writable(&dir).expect("fresh temp dir must be writable");
        assert!(dir.is_dir());

        // The probe file must not linger after the check.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .filter(|n| n.to_string_lossy().starts_with(".oneasr-write-probe"))
            .collect();
        assert!(leftovers.is_empty(), "probe files left behind: {leftovers:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
