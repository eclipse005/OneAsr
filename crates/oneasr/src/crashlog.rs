//! User-visible error log under the install / portable root.
//!
//! Path: `{app_root}/oneasr-error.log` (same folder as `oneasr.exe` layout root —
//! the directory that contains `bin/ffmpeg`, else the exe directory).
//!
//! If that directory is not writable (e.g. installed to `/opt/OneAsr` or
//! `C:\Program Files`, where model downloads fail with `拒绝访问 (os error 5)`
//! too), entries fall back to the **data directory** — the same single resolver
//! the models and settings use (`oneasr_core::paths`) — and, when that is the
//! app root itself (portable layout) or cannot be located at all, to the OS temp
//! dir. A log is never worth rejecting, unlike the multi-GB weights the
//! data-directory policy refuses to put in `%TEMP%`.
//!
//! **Privacy**: the user's home prefix is rewritten to `~` before writing
//! (users paste this file into public issues); media file names are kept and
//! the session banner says so.
//!
//! Captures:
//! - Rust panics (process may still exit; user can open the log after restart)
//! - Explicit app errors via [`log_error`] / [`log_warn`]
//!
//! GPU driver hard-crashes may leave no Rust stack — Event Viewer still needed then.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use oneasr_core::model::resolve_app_root_dir;

/// Max size before rotating to `oneasr-error.log.old`.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// File name in both the primary and the fallback location.
const LOG_FILE_NAME: &str = "oneasr-error.log";

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// `{app_root}/oneasr-error.log`.
///
/// Resolved once per process: `resolve_app_root_dir` walks the filesystem
/// (up to ~16 stat calls) and would otherwise run on every log line.
pub fn log_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| resolve_app_root_dir().join(LOG_FILE_NAME))
        .clone()
}

/// Fallback when `{app_root}` is not writable: the data directory
/// (`%LOCALAPPDATA%\OneAsr` / `$XDG_DATA_HOME/oneasr` / …) — the **one** place
/// that decides where user data lives — and, when that is the app root itself
/// (portable layout: its failure is the app root's failure) or cannot be
/// located at all, the OS temp dir. A log is never worth rejecting, unlike the
/// multi-GB weights the data-directory policy refuses to put in `%TEMP%`.
fn fallback_log_path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let primary = log_path();
        let base = oneasr_core::paths::log_fallback_base().unwrap_or_else(std::env::temp_dir);
        let data_path = base.join(LOG_FILE_NAME);
        if data_path != primary {
            return data_path;
        }
        std::env::temp_dir().join(LOG_FILE_NAME)
    })
    .clone()
}

/// 安装目录那份日志此刻是否真的写得进去。
///
/// 探测的正是 [`write_entry`] 需要的那几件事（父目录建得出来、文件能以追加
/// 方式打开），所以"横幅里写的路径"与"条目实际落到的路径"用的是**同一个判据**，
/// 不另写一套。探测会顺手创建空文件——`write_entry` 本来也会。
fn primary_accepts_writes(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new().create(true).append(true).open(path)?;
    Ok(())
}

/// 下一条日志的落点：安装目录写得进去就用它，否则退 [`fallback_log_path`]
/// （数据目录，再不行才临时目录）。
///
/// 会话横幅用它：安装目录只读时（本次数据目录重构的主场景）条目落在数据目录，
/// 横幅里的 `log:` 一行就必须指向数据目录那个文件——用户正是照这一行去找日志
/// 贴 issue 的。
fn resolved_log_path() -> PathBuf {
    let primary = log_path();
    if primary_accepts_writes(&primary).is_ok() {
        primary
    } else {
        fallback_log_path()
    }
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
        let body = format!("PANIC thread={name}\n  location: {location}\n  message: {payload}");
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
            "OneAsr {ver} started\n  exe: {exe}\n  app_root: {}\n  data_root: {}\n  os: {os}/{arch}\n  log: {}\n  note: home paths are redacted to '~'; media file names remain — re-check them before pasting this log into a public issue",
            root.display(),
            oneasr_core::paths::data_root_log_line(),
            resolved_log_path().display()
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

/// Local calendar day as `YYYY-MM-DD`, for the stats ledger.
///
/// The ledger records the day, not the clock — but it must be the *local* day
/// the user is living in, resolved **at append time** so records stay correct
/// across DST changes (recomputing later from a UTC stamp would need the offset
/// history). Kept beside [`stamp`] so both read the same OS clock.
#[cfg(windows)]
pub fn local_day_ymd() -> String {
    use windows::Win32::Foundation::SYSTEMTIME;
    use windows::Win32::System::SystemInformation::GetLocalTime;
    // SAFETY: same contract as `stamp` — writes the OS clock into a fresh POD.
    let st: SYSTEMTIME = unsafe { GetLocalTime() };
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

#[cfg(not(windows))]
pub fn local_day_ymd() -> String {
    // Dev-only path: the UTC day, derived through the ledger's own calendar
    // helpers so no extra dependency is pulled in.
    use std::time::{SystemTime, UNIX_EPOCH};
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    oneasr_core::stats::shift_days("1970-01-01", days).unwrap_or_else(|| "1970-01-01".into())
}

/// Write one entry; returns the path it actually landed on.
///
/// Primary path first — probed with [`primary_accepts_writes`], the same check
/// the session banner uses, so the banner's `log:` line and this write agree —
/// then on failure the data-directory fallback, with a WARN marker prepended
/// there naming the primary path + error, so the fallback file is
/// self-explanatory when support asks for it.
///
/// Every entry is redacted first (home prefix → `~`): the log is local and
/// never uploaded, but users paste it into public issues.
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

    let msg = oneasr_core::paths::redact_home(msg);
    let primary = log_path();
    // 落点判定只此一处（`primary_accepts_writes` 也是会话横幅用的那个判据）：
    // 探测不过就直接写兜底；探测过了仍写失败（磁盘满、权限恰好被改）也还是退兜底。
    match primary_accepts_writes(&primary) {
        Ok(()) => match write_entry(&primary, level, &msg) {
            Ok(()) => Ok(primary),
            Err(primary_err) => write_fallback(primary, primary_err, level, &msg),
        },
        Err(primary_err) => write_fallback(primary, primary_err, level, &msg),
    }
}

/// 安装目录写不了时把条目落进 [`fallback_log_path`]：每个会话先写一条 WARN
/// 标记说明退到这里的原委，兜底文件因此自己解释得清，support 拿到它不必再猜。
fn write_fallback(
    primary: PathBuf,
    primary_err: std::io::Error,
    level: &str,
    msg: &str,
) -> std::io::Result<PathBuf> {
    let fallback = fallback_log_path();
    // Same place twice: retrying cannot succeed, and the WARN marker
    // would be written into the file it is complaining about.
    if fallback == primary {
        return Err(primary_err);
    }
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

    /// Windows uses a local calendar timestamp with millisecond precision.
    #[cfg(windows)]
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

    /// Non-Windows builds use the documented Unix-seconds timestamp format.
    #[cfg(not(windows))]
    #[test]
    fn epoch_stamp_matches_log_format() {
        let s = stamp();
        let (seconds, milliseconds) = s.split_once('.').expect("missing milliseconds");
        assert!(!seconds.is_empty(), "missing seconds in {s:?}");
        assert!(
            seconds.bytes().all(|b| b.is_ascii_digit()),
            "non-digit seconds in {s:?}"
        );
        assert_eq!(milliseconds.len(), 3, "milliseconds precision in {s:?}");
        assert!(
            milliseconds.bytes().all(|b| b.is_ascii_digit()),
            "non-digit milliseconds in {s:?}"
        );
    }

    /// Entries append (headers accumulate) and missing parents are created —
    /// the fallback path relies on both.
    #[test]
    fn write_entry_appends_and_creates_parents() {
        let dir = std::env::temp_dir().join(format!("oneasr-crashlog-test-{}", std::process::id()));
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

    /// 探测判据必须与真正写日志一致：父目录可建 → 通过且真写得进去；
    /// 父路径是个文件（不是目录）→ 必须判为不可写，别把横幅指向一个假文件。
    #[test]
    fn primary_probe_matches_write_entry() {
        let dir =
            std::env::temp_dir().join(format!("oneasr-crashlog-probe-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("nested").join("probe.log");
        // 父目录还不存在：探测顺手建出来，并判定可写。
        super::primary_accepts_writes(&path).expect("nested parent must be creatable");
        // 探测说行，"落点判定"就该真的写得进去（横幅与实际写入不分家）。
        super::write_entry(&path, "INFO", "probe").unwrap();
        assert!(fs::read_to_string(&path).unwrap().contains("probe"));

        // 父路径是个普通文件：连父目录都建不出来，必须失败。
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"x").unwrap();
        assert!(
            super::primary_accepts_writes(&blocker.join("under-file.log")).is_err(),
            "a file cannot host a log directory"
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
