//! Readiness probes: are the weight files in place, and can we write to the install directory?

use std::io::Write;
use std::path::Path;

use super::catalog::{model_definition, ModelId};

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
