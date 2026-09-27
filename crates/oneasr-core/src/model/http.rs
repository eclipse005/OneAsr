//! ModelScope HTTP plumbing: client setup, Range resume, retry backoff.

use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::catalog::ModelDefinition;

use super::download::{FileDownloadCtx, READ_IDLE_TIMEOUT};

/// Delete an untrusted `.part` and remove its bytes from progress accounting
/// (they were counted by `initial_bytes`/`add_written`).
pub(super) fn discard_part(
    ctx: &mut FileDownloadCtx<'_>,
    part_path: &Path,
    part_bytes: u64,
    expected: u64,
) {
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
    // Same switch as the pipeline trace (`crate::diagnostics::env_flag`): a
    // variable read twice with two different rules is a bug waiting to happen.
    if crate::diagnostics::pipeline_trace() {
        eprintln!("[download] {msg}");
    }
}

/// `Mozilla/5.0 (<platform token>) OneAsr/<real version>`.
///
/// The version comes from `CARGO_PKG_VERSION` (跟踪 `Cargo.toml`，不再手写成
/// 常量), and the platform token from `std::env::consts::OS`/`ARCH` — there are
/// Windows, macOS (arm64) and Linux builds, and a hardcoded `Windows NT 10.0;
/// Win64; x64` would misdescribe two of them to the hub.
fn user_agent() -> String {
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "Windows NT 10.0; Win64; x64",
        ("macos", "aarch64") => "Macintosh; Apple Silicon Mac OS X",
        ("macos", _) => "Macintosh; Intel Mac OS X",
        ("linux", "aarch64") => "X11; Linux aarch64",
        ("linux", _) => "X11; Linux x86_64",
        (other, arch) => return format!("OneAsr/{} ({other} {arch})", env!("CARGO_PKG_VERSION")),
    };
    format!(
        "Mozilla/5.0 ({platform}) OneAsr/{}",
        env!("CARGO_PKG_VERSION")
    )
}

pub(super) fn content_length_total(
    response: &reqwest::blocking::Response,
    part_bytes: u64,
) -> Option<u64> {
    // Content-Range: bytes start-end/total
    if let Some(cr) = response.headers().get(reqwest::header::CONTENT_RANGE)
        && let Ok(s) = cr.to_str()
        && let Some(total) = s.rsplit('/').next()
        && let Ok(n) = total.parse::<u64>()
    {
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

/// Bytes already on disk for a resumable job.
///
/// `model_dir` is the caller's destination (never re-derived): a `.part` left
/// in the old install layout must not count towards a job writing elsewhere.
pub(super) fn initial_bytes(definition: &ModelDefinition, model_dir: &Path) -> (u64, u64) {
    let mut downloaded = 0_u64;
    let mut total = 0_u64;
    for file in &definition.download_files {
        let target = model_dir.join(&file.file_name);
        let part = model_dir.join(format!("{}.part", file.file_name.replace('/', "_")));
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
            .user_agent(user_agent())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_carries_the_real_version_and_this_platform() {
        let ua = user_agent();
        assert!(
            ua.contains(concat!("OneAsr/", env!("CARGO_PKG_VERSION"))),
            "user agent must track Cargo.toml: {ua}"
        );
        assert!(
            ua.contains(std::env::consts::OS) || ua.contains("Windows NT"),
            "user agent must describe this OS: {ua}"
        );
        assert!(
            !ua.contains("OneAsr/0.1"),
            "the hardcoded 0.1 is gone: {ua}"
        );
    }

    /// The resume counter must describe the directory the job writes to and
    /// nothing else — bytes left behind in another directory (the old install
    /// layout, a user's previous folder) must not be counted into this job.
    #[test]
    fn initial_bytes_counts_only_the_given_directory() {
        let definition = crate::model::model_definition(crate::model::ModelId::HtdemucsFt);
        let file = &definition.download_files[0];
        let base =
            std::env::temp_dir().join(format!("oneasr-initial-bytes-{}", std::process::id()));
        let given = base.join("given");
        let other = base.join("other");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&given).unwrap();
        std::fs::create_dir_all(&other).unwrap();

        // 另一个目录里有一个"完整"的文件 + 一个大的 .part：都不该算进来。
        std::fs::write(other.join(&file.file_name), vec![0u8; 64]).unwrap();
        std::fs::write(
            other.join(format!("{}.part", file.file_name.replace('/', "_"))),
            vec![0u8; 64],
        )
        .unwrap();
        // 给定目录里只有 8 字节的 .part：这才是"已下载"。
        std::fs::write(
            given.join(format!("{}.part", file.file_name.replace('/', "_"))),
            vec![0u8; 8],
        )
        .unwrap();

        let (downloaded, total) = initial_bytes(&definition, &given);
        let full_total: u64 = definition
            .download_files
            .iter()
            .map(|f| f.expected_size)
            .sum();
        assert_eq!(downloaded, 8, "only the given directory counts");
        assert_eq!(total, full_total, "a .part never inflates the total");
        let _ = std::fs::remove_dir_all(&base);
    }
}
