//! Readiness probes: are the weight files / CUDA runtime in place, and can we write to the install directory?

use std::io::Write;
use std::path::{Path, PathBuf};

use super::catalog::{model_definition, ModelId};
use super::path::{resolve_dll_dir, resolve_exe_dir};

/// Whether all catalog files are present at their exact pinned sizes.
///
/// Hashing multi-GB weights on every readiness probe would be far too slow, so
/// readiness is size-based; SHA-256 is verified once at download time.
pub fn is_model_ready(id: ModelId) -> bool {
    let def = model_definition(id);
    def.download_files.iter().all(|file| {
        let path = def.model_dir.join(&file.file_name);
        file_meets_ready_threshold(&path, file.expected_size)
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
        file_meets_ready_threshold(&dir.join(&file.file_name), file.expected_size)
    })
}

/// File exists at exactly `expected_size` bytes (size read from the pinned
/// revision). Returns `false` for missing, zero-byte, truncated, or oversized
/// files. SHA-256 is checked separately when downloading.
pub fn file_meets_ready_threshold(path: &Path, expected_size: u64) -> bool {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() => meta.len() == expected_size,
        _ => false,
    }
}

/// Probe whether `dir` accepts file writes (create → write → remove a temp file).
///
/// Mirrors the real download flow (`create_dir_all` then open `.part` for append),
/// so a failure surfaces the same `io::Error` a download would hit. Used to log
/// the environment when a download starts — with a bare OS error like
/// `拒绝访问 (os error 5)` this is the only record of *where* it broke.
pub fn probe_writable(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let probe = dir.join(format!(".oneasr-write-probe-{}", std::process::id()));
    let mut f = std::fs::File::create(&probe)?;
    f.write_all(b"ok")?;
    f.flush()?;
    drop(f);
    std::fs::remove_file(&probe)
}
