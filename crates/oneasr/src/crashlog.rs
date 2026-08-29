//! User-visible error log under the install / portable root.
//!
//! Path: `{app_root}/oneasr-error.log` (same folder as `oneasr.exe` layout root —
//! the directory that contains `bin/ffmpeg`, else the exe directory).
//!
//! If that directory is not writable (e.g. copied into `C:\Program Files`, where
//! model downloads fail with `拒绝访问 (os error 5)` too), entries fall back to
//! `%LOCALAPPDATA%\OneAsr\oneasr-error.log` so a diagnosis is still possible.
//!
//! Captures:
//! - Rust panics (process may still exit; user can open the log after restart)
//! - Explicit app errors via [`log_error`] / [`log_warn`]
//!
//! GPU driver hard-crashes may leave no Rust stack — Event Viewer still needed then.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oneasr_core::model::resolve_app_root_dir;

/// Max size before rotating to `oneasr-error.log.old`.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// `{app_root}/oneasr-error.log`
pub fn log_path() -> PathBuf {
    resolve_app_root_dir().join("oneasr-error.log")
}

/// Fallback when `{app_root}` is not writable: `%LOCALAPPDATA%\OneAsr\`, else
/// `%TEMP%\OneAsr\` (dev / stripped environments without the env var).
fn fallback_log_path() -> PathBuf {
    let base = match std::env::var_os("LOCALAPPDATA") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => std::env::temp_dir(),
    };
    base.join("OneAsr").join("oneasr-error.log")
}

/// Install panic hook early in `main`. Chains the previous hook (debug console).
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".into());
        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".into()
        };
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("<unnamed>");
        let body = format!(
            "PANIC thread={name}\n  location: {location}\n  message: {payload}"
        );
        let _ = append_raw("PANIC", &body);
        previous(info);
    }));
}

/// Session banner so users know the file is live after a crash report request.
pub fn log_session_start() {
    let ver = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".into());
    let root = resolve_app_root_dir();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let _ = append_raw(
        "INFO",
        &format!(
            "OneAsr {ver} started\n  exe: {exe}\n  app_root: {}\n  os: {os}/{arch}\n  log: {}",
            root.display(),
            log_path().display()
        ),
    );
}

/// Font probe result (after GPUI text system is up).
pub fn log_font_plan(report: &str) {
    let _ = append_raw("INFO", &format!("font plan\n{report}"));
}

pub fn log_error(msg: impl AsRef<str>) {
    let _ = append_raw("ERROR", msg.as_ref());
}

pub fn log_warn(msg: impl AsRef<str>) {
    let _ = append_raw("WARN", msg.as_ref());
}

pub fn log_info(msg: impl AsRef<str>) {
    let _ = append_raw("INFO", msg.as_ref());
}

/// Local wall-clock stamp formatted as `YYYY-MM-DD HH:MM:SS.mmm`.
///
/// Uses the OS local time so log timestamps line up with what the user sees in
/// Explorer mtime / Event Viewer. `GetLocalTime` is always available on the
/// Win10+ targets OneAsr supports.
#[cfg(windows)]
fn stamp() -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: `GetLocalTime` reads the OS clock into a fresh `SYSTEMTIME` (POD).
    // No preconditions, no out-params held across the call.
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond, st.wMilliseconds
    )
}

#[cfg(not(windows))]
fn stamp() -> String {
    // Non-Windows builds are dev-only; UTC epoch seconds is fine.
    use std::time::{SystemTime, UNIX_EPOCH};
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

/// Write one entry; returns the path it actually landed on.
///
/// Primary path first; on failure, retry the `%LOCALAPPDATA%` fallback and
/// prepend a WARN marker there naming the primary path + error, so the fallback
/// file is self-explanatory when support asks for it.
fn append_raw(level: &str, msg: &str) -> std::io::Result<PathBuf> {
    // Poison only happens if a writer panicked mid-line; keep logging through it
    // (a crash log that goes silent is worse than a slightly racy one) but warn
    // once on stderr so it isn't completely invisible.
    let _guard = match WRITE_LOCK.lock() {
        Ok(g) => g,
        Err(e) => {
            static WARNED: std::sync::Once = std::sync::Once::new();
            WARNED.call_once(|| {
                eprintln!("oneasr crashlog: WRITE_LOCK poisoned ({e}); continuing");
            });
            e.into_inner()
        }
    };

    let primary = log_path();
    match write_entry(&primary, level, msg) {
        Ok(()) => Ok(primary),
        Err(primary_err) => {
            let fallback = fallback_log_path();
            // One marker per session, not one per entry — the fallback file
            // would otherwise drown in repeated WARN headers.
            static MARKED: std::sync::Once = std::sync::Once::new();
            MARKED.call_once(|| {
                let marker = format!(
                    "primary log unavailable, falling back\n  primary: {}\n  error: {primary_err}",
                    primary.display()
                );
                let _ = write_entry(&fallback, "WARN", &marker);
            });
            write_entry(&fallback, level, msg)?;
            Ok(fallback)
        }
    }
}

fn write_entry(path: &Path, level: &str, msg: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    rotate_if_needed(path)?;

    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "---- [{}] {} ----", stamp(), level)?;
    for line in msg.lines() {
        writeln!(f, "{line}")?;
    }
    writeln!(f)?;
    f.flush()?;
    Ok(())
}

fn rotate_if_needed(path: &std::path::Path) -> std::io::Result<()> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(()),
    };
    if meta.len() < MAX_LOG_BYTES {
        return Ok(());
    }
    let old = path.with_extension("log.old");
    let _ = fs::remove_file(&old);
    let _ = fs::rename(path, &old);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{stamp, write_entry};
    use std::fs;

    /// stamp() must match the log-line format other tooling may parse.
    /// We don't assert the actual time — only the shape — so this stays
    /// stable across time zones / clock values.
    #[test]
    fn stamp_matches_log_format() {
        let s = stamp();
        assert_eq!(s.len(), 23, "stamp() produced {s:?}");
        let bytes = s.as_bytes();
        let expect = b"0000-00-00 00:00:00.000";
        for (i, &b) in bytes.iter().enumerate() {
            let want = expect[i];
            if want == b'0' {
                assert!(b.is_ascii_digit(), "pos {i}: digit expected in {s:?}");
            } else {
                assert_eq!(b, want, "pos {i}: separator mismatch in {s:?}");
            }
        }
    }

    /// Entries append (headers accumulate) and missing parents are created —
    /// the fallback path relies on both.
    #[test]
    fn write_entry_appends_and_creates_parents() {
        let dir = std::env::temp_dir().join(format!(
            "oneasr-crashlog-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("test.log");

        write_entry(&path, "INFO", "first").unwrap();
        write_entry(&path, "ERROR", "second\nline2").unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text.matches("---- [").count(), 2, "{text}");
        assert!(text.contains("first"), "{text}");
        assert!(text.contains("line2"), "{text}");

        let _ = fs::remove_dir_all(&dir);
    }
}
