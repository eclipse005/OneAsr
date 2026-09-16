//! ModelScope HTTP plumbing: client setup, Range resume, retry backoff.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use super::catalog::ModelDefinition;

use super::download::{FileDownloadCtx, READ_IDLE_TIMEOUT};

/// Delete an untrusted `.part` and remove its bytes from progress accounting
/// (they were counted by `initial_bytes`/`add_written`).
pub(super) fn discard_part(ctx: &mut FileDownloadCtx<'_>, part_path: &Path, part_bytes: u64, expected: u64) {
    let _ = std::fs::remove_file(part_path);
    ctx.subtract(part_bytes.min(expected));
}

pub(super) fn retry_backoff(attempt: u32) -> Duration {
    Duration::from_secs(1 << attempt.saturating_sub(1).min(3))
}

/// Sleep in small slices so cancellation is honoured promptly.
/// Returns `false` when cancelled.
pub(super) fn sleep_cancellable(cancel: &AtomicBool, total: Duration) -> bool {
    let deadline = Instant::now() + total;
    while Instant::now() < deadline {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    !cancel.load(Ordering::Relaxed)
}

pub(super) fn trace_download(msg: &str) {
    if std::env::var_os("ONEASR_PIPELINE_TRACE").is_some() {
        eprintln!("[download] {msg}");
    }
}

pub(super) fn content_length_total(response: &reqwest::blocking::Response, part_bytes: u64) -> Option<u64> {
    // Content-Range: bytes start-end/total
    if let Some(cr) = response.headers().get(reqwest::header::CONTENT_RANGE)
        && let Ok(s) = cr.to_str()
        && let Some(total) = s.rsplit('/').next()
        && let Ok(n) = total.parse::<u64>() {
        return Some(n);
    }
    response
        .content_length()
        .map(|len| part_bytes.saturating_add(len))
}

/// Start offset declared by a `Content-Range: bytes start-end/total` header.
pub(super) fn content_range_start(response: &reqwest::blocking::Response) -> Option<u64> {
    let raw = response.headers().get(reqwest::header::CONTENT_RANGE)?;
    let text = raw.to_str().ok()?;
    // Skip the unit token ("bytes " / "bytes="), then parse "start-end/total".
    let range = text.split_once(' ').map(|(_, rest)| rest).unwrap_or(text);
    range.split('-').next()?.trim().parse().ok()
}

pub(super) fn initial_bytes(definition: &ModelDefinition) -> (u64, u64) {
    let mut downloaded = 0_u64;
    let mut total = 0_u64;
    for file in &definition.download_files {
        let target = definition.model_dir.join(&file.file_name);
        let part = definition
            .model_dir
            .join(format!("{}.part", file.file_name.replace('/', "_")));
        let have = if target.is_file() {
            std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0)
        } else {
            std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0)
        };
        downloaded = downloaded.saturating_add(have.min(file.expected_size));
        total = total.saturating_add(file.expected_size.max(have));
    }
    (downloaded, total)
}

/// Process-wide reqwest client (connection reuse across sequential downloads).
/// Build failure is sticky for the process and returned as [`DownloadOutcome::Failed`].
pub(super) fn download_client() -> Result<&'static reqwest::blocking::Client, String> {
    static CLIENT: OnceLock<Result<reqwest::blocking::Client, String>> = OnceLock::new();
    match CLIENT.get_or_init(|| {
        reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // Blocking `Response::read` applies this timeout to each read call,
            // giving stalled connections a bounded wait without capping the
            // total duration of a large download.
            .timeout(READ_IDLE_TIMEOUT)
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) OneAsr/0.1")
            .build()
            .map_err(|e| e.to_string())
    }) {
        Ok(client) => Ok(client),
        Err(e) => Err(e.clone()),
    }
}

pub(super) fn modelscope_request(
    client: &reqwest::blocking::Client,
    url: &str,
    part_bytes: u64,
) -> reqwest::blocking::RequestBuilder {
    let req = client
        .get(url)
        .header(reqwest::header::ACCEPT, "*/*")
        .header(reqwest::header::REFERER, "https://modelscope.cn/");
    if part_bytes > 0 {
        req.header(reqwest::header::RANGE, format!("bytes={part_bytes}-"))
    } else {
        req
    }
}

pub(super) fn is_success(status: reqwest::StatusCode) -> bool {
    status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT
}
