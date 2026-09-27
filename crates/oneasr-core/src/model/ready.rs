//! 就绪探测的**零件**：单个文件够不够大、以及某个目录收不收写。
//!
//! 「一个模型目录算不算就绪」只有一处判定 ——
//! [`crate::asr::model_check`] 的 `check_*_model_dir`（带过期策略的缓存），
//! 这里只提供它和下载流程共用的两个基础谓词。别在这里再造第二套判据。

use std::io::Write;
use std::path::Path;

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
