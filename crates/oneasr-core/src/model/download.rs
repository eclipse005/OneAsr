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
use std::time::Instant;

use super::catalog::{model_definition, ModelDefinition, ModelId};
use super::path::{resolve_dll_dir, resolve_exe_dir};

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

    /// Human status; CUDA uses “组件” wording, models use “下载”.
    pub fn label_for_kind(&self, kind: crate::model::ModelKind) -> String {
        use crate::model::ModelKind;
        let component = matches!(kind, ModelKind::CudaRuntime);
        match self.state {
            DownloadState::Idle => {
                if component {
                    "未安装".into()
                } else {
                    "未下载".into()
                }
            }
            DownloadState::Downloading => {
                let pct = self.percent();
                let speed = format_speed(self.speed_bytes_per_sec);
                if component {
                    format!("安装中 {pct:.0}% · {speed}")
                } else {
                    format!("下载中 {pct:.0}% · {speed}")
                }
            }
            DownloadState::Completed => {
                if component {
                    "已安装".into()
                } else {
                    "已就绪".into()
                }
            }
            DownloadState::Failed => {
                if self.message.is_empty() {
                    if component {
                        "安装失败".into()
                    } else {
                        "下载失败".into()
                    }
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

/// Whether all catalog files are present with non-truncated sizes.
///
/// Uses each file's catalog `expected_size` as a low watermark (50%): rejects
/// empty / partial copies while staying tolerant of rounded CUDA size estimates.
pub fn is_model_ready(id: ModelId) -> bool {
    let def = model_definition(id);
    def.download_files.iter().all(|file| {
        let path = def.model_dir.join(&file.file_name);
        file_meets_catalog_size(&path, file.expected_size)
    })
}

/// CUDA runtime DLLs for the GPU backend.
///
/// Searches common layout roots so GPU works for:
/// - install: `{exe}/dll/`
/// - `cargo run -p oneasr`: `{target/release}/dll/`
/// - `cargo run --example …`: walk up from `target/*/examples/` to `target/*/dll/`
/// - project root: `{app_root}/dll/` when it contains `bin/ffmpeg`
pub fn is_cuda_runtime_ready() -> bool {
    cuda_runtime_search_dirs()
        .into_iter()
        .any(|dir| cuda_runtime_ready_in(&dir))
}

/// First directory that has a complete CUDA runtime set (for `SetDllDirectory`).
pub fn resolve_cuda_runtime_dir() -> Option<PathBuf> {
    cuda_runtime_search_dirs()
        .into_iter()
        .find(|dir| cuda_runtime_ready_in(dir))
}

fn cuda_runtime_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let push_unique = |dirs: &mut Vec<PathBuf>, p: PathBuf| {
        if !dirs.iter().any(|d| d == &p) {
            dirs.push(p);
        }
    };
    push_unique(&mut dirs, resolve_dll_dir());
    // Walk up from the exe (covers target/release/examples → target/release).
    let mut cur = resolve_exe_dir();
    for _ in 0..6 {
        push_unique(&mut dirs, cur.join("dll"));
        if !cur.pop() {
            break;
        }
    }
    if let Some(root) = crate::media::resolve_app_root() {
        push_unique(&mut dirs, root.join("dll"));
    }
    dirs
}

fn cuda_runtime_ready_in(dir: &Path) -> bool {
    let def = model_definition(ModelId::CudaRuntime);
    def.download_files.iter().all(|file| {
        file_meets_catalog_size(&dir.join(&file.file_name), file.expected_size)
    })
}

/// File exists and is large enough vs catalog expected size (not empty/truncated).
fn file_meets_catalog_size(path: &Path, expected_size: u64) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => {
            let min = (expected_size / 2).max(1);
            meta.len() >= min
        }
        _ => false,
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

    let (mut downloaded_bytes, mut total_bytes) = initial_bytes(&definition);

    let emit_downloading =
        |downloaded: u64, total: u64, speed: u64, message: &str, on_progress: &mut dyn FnMut(DownloadProgress)| {
            on_progress(DownloadProgress {
                state: DownloadState::Downloading,
                model_id: id,
                model_dir: model_dir.clone(),
                downloaded_bytes: downloaded,
                total_bytes: total,
                speed_bytes_per_sec: speed,
                message: message.to_string(),
            });
        };

    emit_downloading(downloaded_bytes, total_bytes, 0, "starting", &mut on_progress);

    let client = match build_client() {
        Ok(c) => c,
        Err(e) => {
            return DownloadOutcome::Failed {
                message: e,
                downloaded_bytes,
                total_bytes,
            };
        }
    };

    let mut last_speed_mark = Instant::now();
    let mut last_speed_bytes = downloaded_bytes;

    for file in &definition.download_files {
        if cancel.load(Ordering::Relaxed) {
            return DownloadOutcome::Cancelled {
                downloaded_bytes,
                total_bytes,
            };
        }

        let target = definition.model_dir.join(&file.file_name);
        if target.is_file() {
            continue;
        }

        if let Some(parent) = target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return DownloadOutcome::Failed {
                    message: e.to_string(),
                    downloaded_bytes,
                    total_bytes,
                };
            }
        }

        let part_path = definition
            .model_dir
            .join(format!("{}.part", file.file_name.replace('/', "_")));
        let part_bytes = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

        let (mut response, restarted) =
            match start_modelscope_download(&client, &file.url, &part_path, part_bytes) {
                Ok(v) => v,
                Err(e) => {
                    return DownloadOutcome::Failed {
                        message: format!("{}: {e}", file.file_name),
                        downloaded_bytes,
                        total_bytes,
                    };
                }
            };
        if restarted {
            downloaded_bytes = downloaded_bytes.saturating_sub(part_bytes);
        }
        if !is_success(response.status()) {
            return DownloadOutcome::Failed {
                message: format!("下载失败 {}: HTTP {}", file.file_name, response.status()),
                downloaded_bytes,
                total_bytes,
            };
        }

        // Prefer Content-Length / Content-Range for accurate remaining size.
        if let Some(remote_total) = content_length_total(&response, part_bytes) {
            // Reconcile: bytes already accounted + remaining for this file.
            let other = total_bytes.saturating_sub(file.expected_size);
            total_bytes = other.saturating_add(remote_total.max(file.expected_size));
        }

        let mut output = match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&part_path)
        {
            Ok(f) => f,
            Err(e) => {
                return DownloadOutcome::Failed {
                    message: e.to_string(),
                    downloaded_bytes,
                    total_bytes,
                };
            }
        };

        let mut buf = [0_u8; 64 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                // Keep `.part` so the next download resumes (same as failure path).
                drop(output);
                return DownloadOutcome::Cancelled {
                    downloaded_bytes,
                    total_bytes,
                };
            }
            let n = match response.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    return DownloadOutcome::Failed {
                        message: e.to_string(),
                        downloaded_bytes,
                        total_bytes,
                    };
                }
            };
            if let Err(e) = output.write_all(&buf[..n]) {
                return DownloadOutcome::Failed {
                    message: e.to_string(),
                    downloaded_bytes,
                    total_bytes,
                };
            }
            downloaded_bytes = downloaded_bytes.saturating_add(n as u64);
            // Keep total ahead of downloaded so percent never looks stuck >100 from clamp alone.
            if downloaded_bytes > total_bytes {
                total_bytes = downloaded_bytes;
            }
            let elapsed = last_speed_mark.elapsed().as_secs_f64();
            if elapsed >= 0.35 {
                let speed = ((downloaded_bytes.saturating_sub(last_speed_bytes) as f64) / elapsed)
                    .round() as u64;
                last_speed_mark = Instant::now();
                last_speed_bytes = downloaded_bytes;
                emit_downloading(
                    downloaded_bytes,
                    total_bytes,
                    speed,
                    &file.file_name,
                    &mut on_progress,
                );
            }
        }
        drop(output);
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::fs::rename(&part_path, &target) {
            return DownloadOutcome::Failed {
                message: e.to_string(),
                downloaded_bytes,
                total_bytes,
            };
        }
    }

    let bad: Vec<_> = definition
        .download_files
        .iter()
        .filter(|f| {
            !file_meets_catalog_size(&definition.model_dir.join(&f.file_name), f.expected_size)
        })
        .map(|f| f.file_name.clone())
        .collect();
    if !bad.is_empty() {
        return DownloadOutcome::Failed {
            message: format!("文件缺失或损坏: {}", bad.join(", ")),
            downloaded_bytes,
            total_bytes,
        };
    }

    DownloadOutcome::Completed {
        model_dir: definition.model_dir,
    }
}

fn content_length_total(response: &reqwest::blocking::Response, part_bytes: u64) -> Option<u64> {
    // Content-Range: bytes start-end/total
    if let Some(cr) = response.headers().get(reqwest::header::CONTENT_RANGE) {
        if let Ok(s) = cr.to_str() {
            if let Some(total) = s.rsplit('/').next() {
                if let Ok(n) = total.parse::<u64>() {
                    return Some(n);
                }
            }
        }
    }
    response
        .content_length()
        .map(|len| part_bytes.saturating_add(len))
}

fn initial_bytes(definition: &ModelDefinition) -> (u64, u64) {
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

fn build_client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) OneAsr/0.1")
        .build()
        .map_err(|e| e.to_string())
}

fn start_modelscope_download(
    client: &reqwest::blocking::Client,
    url: &str,
    part_path: &Path,
    part_bytes: u64,
) -> Result<(reqwest::blocking::Response, bool), String> {
    let mut response = modelscope_request(client, url, part_bytes)
        .send()
        .map_err(|e| e.to_string())?;
    let mut restarted = false;
    if response.status() == reqwest::StatusCode::OK && part_bytes > 0 {
        let _ = std::fs::remove_file(part_path);
        restarted = true;
        response = modelscope_request(client, url, 0)
            .send()
            .map_err(|e| e.to_string())?;
    }
    Ok((response, restarted))
}

fn modelscope_request(
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

fn is_success(status: reqwest::StatusCode) -> bool {
    status.is_success() || status == reqwest::StatusCode::PARTIAL_CONTENT
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
    fn catalog_size_rejects_empty_and_truncated() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr-ready-test-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.dll");

        assert!(!file_meets_catalog_size(&path, 1000));

        std::fs::File::create(&path).unwrap();
        assert!(!file_meets_catalog_size(&path, 1000));

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 100]).unwrap();
        drop(f);
        // 100 < 50% of 1000
        assert!(!file_meets_catalog_size(&path, 1000));

        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(&[0u8; 500]).unwrap();
        drop(f);
        assert!(file_meets_catalog_size(&path, 1000));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
