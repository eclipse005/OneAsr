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
//! (users paste this file into public issues); media file names are kept.
//! 正常启动与成功任务的过程信息不会写入错误日志。
//!
//! Captures:
//! - Rust panics (process may still exit; user can open the log after restart)
//! - Explicit app errors via [`log_error`] / [`log_warn`]
//! - 任务临时记录只会在失败或进程中断后并入错误日志
//!
//! GPU driver hard-crashes may leave no Rust stack — Event Viewer still needed then.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use oneasr_core::model::resolve_app_root_dir;

/// Max size before rotating to `oneasr-error.log.old`.
const MAX_LOG_BYTES: u64 = 2 * 1024 * 1024;

/// File name in both the primary and the fallback location.
const LOG_FILE_NAME: &str = "oneasr-error.log";
const TASK_JOURNAL_PREFIX: &str = "oneasr-task-";
const TASK_JOURNAL_SUFFIX: &str = ".pending";

static WRITE_LOCK: Mutex<()> = Mutex::new(());
static SESSION_CONTEXT: OnceLock<Mutex<SessionContext>> = OnceLock::new();

#[derive(Default)]
struct SessionContext {
    text: String,
    included_bytes: usize,
}

fn session_context() -> &'static Mutex<SessionContext> {
    SESSION_CONTEXT.get_or_init(|| Mutex::new(SessionContext::default()))
}

fn lock_session_context() -> std::sync::MutexGuard<'static, SessionContext> {
    session_context()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
}

/// 先把上下文留在内存里，等出现问题时再写入错误日志。
pub fn set_session_context(section: &str, details: impl AsRef<str>) {
    let mut context = lock_session_context();
    if !context.text.is_empty() {
        context.text.push('\n');
    }
    context.text.push_str(section);
    context.text.push_str(":\n");
    context.text.push_str(details.as_ref());
    context.text.push('\n');
}

fn append_problem(level: &str, msg: &str) -> std::io::Result<PathBuf> {
    let mut session = lock_session_context();
    let context = session.text[session.included_bytes..].to_string();
    let body = if context.is_empty() {
        msg.to_string()
    } else {
        format!("diagnostic context:\n{context}\n{msg}")
    };
    let result = append_raw(level, &body);
    if result.is_ok() {
        session.included_bytes = session.text.len();
    }
    result
}

/// 当前 ASR 任务的临时记录。成功时删除，失败时并入错误日志；进程异常退出后，
/// 下次启动会把未完成且已解锁的记录并入错误日志。
pub struct TaskJournal {
    path: PathBuf,
    file: Option<File>,
}

impl TaskJournal {
    pub fn begin(task_id: &str, details: &str) -> std::io::Result<Self> {
        let dir = oneasr_core::paths::data_dir();
        let path = dir.join(format!(
            "{TASK_JOURNAL_PREFIX}{}{TASK_JOURNAL_SUFFIX}",
            std::process::id()
        ));
        Self::begin_at(path, task_id, details)
    }

    fn begin_at(path: PathBuf, task_id: &str, details: &str) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)?;
        // PID 文件名避免进程间重名；文件锁可区分已退出的旧任务和仍在运行的实例。
        file.try_lock()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        file.set_len(0)?;
        file.seek(SeekFrom::Start(0))?;

        let mut journal = Self {
            path,
            file: Some(file),
        };
        let header = format!(
            "OneAsr {} task journal\n  pid: {}\n  task: {task_id}\n{details}",
            env!("CARGO_PKG_VERSION"),
            std::process::id()
        );
        if let Err(error) = journal.record(&header) {
            journal.remove_file();
            return Err(error);
        }
        Ok(journal)
    }

    pub fn record(&mut self, msg: impl AsRef<str>) -> std::io::Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| std::io::Error::other("task journal is already closed"))?;
        let msg = oneasr_core::paths::redact_home(msg.as_ref());
        writeln!(file, "---- [{}] ----", stamp())?;
        for line in msg.lines() {
            writeln!(file, "{line}")?;
        }
        writeln!(file)?;
        file.sync_data()
    }

    pub fn complete(mut self) {
        // 若写入成功标记后清理被中断，恢复时只删除记录，不报成任务崩溃。
        let _ = self.record("task completed successfully");
        self.remove_file();
    }

    pub fn promote_failure(mut self, summary: &str) -> std::io::Result<()> {
        self.record(summary)?;
        let mut contents = String::new();
        if let Some(file) = self.file.as_mut() {
            file.flush()?;
            file.seek(SeekFrom::Start(0))?;
            file.read_to_string(&mut contents)?;
        }
        let result = append_problem(
            "ERROR",
            &format!("task failed; promoted task journal:\n{contents}"),
        );
        match result {
            Ok(_) => {
                self.remove_file();
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn remove_file(self) {
        let Self { path, file } = self;
        drop(file);
        let _ = fs::remove_file(path);
    }
}

fn recover_interrupted_task_journals() {
    let dir = oneasr_core::paths::data_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(TASK_JOURNAL_PREFIX) || !name.ends_with(TASK_JOURNAL_SUFFIX) {
            continue;
        }
        let Ok(mut file) = OpenOptions::new().read(true).write(true).open(&path) else {
            continue;
        };
        // 仍在运行的 OneAsr 会持有文件锁；只有已解锁的记录才按中断任务处理。
        if file.try_lock().is_err() {
            continue;
        }
        let mut contents = String::new();
        if file.read_to_string(&mut contents).is_err() {
            continue;
        }
        let completed = contents.contains("task completed successfully");
        let recorded = contents.trim().is_empty()
            || completed
            || append_problem(
                "ERROR",
                &format!(
                    "previous process ended while an ASR task was active\n  journal: {}\n{contents}",
                    path.display()
                ),
            )
            .is_ok();
        drop(file);
        if recorded {
            let _ = fs::remove_file(path);
        }
    }
}

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
        // 强制抓取回溯，Release 下即使没有设置 `RUST_BACKTRACE` 也尽量保留调用栈。
        // 只在 panic 时执行，额外开销不会进入正常任务路径。
        let backtrace = std::backtrace::Backtrace::force_capture();
        let body = format!(
            "PANIC thread={name}\n  location: {location}\n  message: {payload}\n  backtrace:\n{backtrace}"
        );
        let _ = append_problem("PANIC", &body);
        previous(info);
    }));
}

/// 会话信息先保存在内存中，只有出现问题时才写入日志。
pub fn log_session_start() {
    let ver = env!("CARGO_PKG_VERSION");
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "<unknown>".into());
    let root = resolve_app_root_dir();
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    {
        let mut context = lock_session_context();
        context.text.clear();
        context.included_bytes = 0;
    }
    set_session_context(
        "session",
        format!(
            "OneAsr {ver}\n  pid: {}\n  exe: {exe}\n  app_root: {}\n  data_root: {}\n  os: {os}/{arch}",
            std::process::id(),
            root.display(),
            oneasr_core::paths::data_root_log_line()
        ),
    );
    recover_interrupted_task_journals();
}

/// Font probe result (after GPUI text system is up).
pub fn log_font_plan(report: &str) {
    set_session_context("font plan", report);
}

pub fn log_error(msg: impl AsRef<str>) {
    let _ = append_problem("ERROR", msg.as_ref());
}

pub fn log_warn(msg: impl AsRef<str>) {
    let _ = append_problem("WARN", msg.as_ref());
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
    use super::{TaskJournal, stamp, write_entry};
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

    /// 成功任务的临时过程记录应立即清理，不留在用户可见日志目录。
    #[test]
    fn successful_task_journal_is_removed() {
        let dir =
            std::env::temp_dir().join(format!("oneasr-task-journal-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("oneasr-task-test.pending");
        let journal = TaskJournal::begin_at(path.clone(), "task-test", "file: input.wav")
            .expect("temporary task journal should open");

        assert!(path.exists(), "journal should exist while the task runs");
        journal.complete();

        assert!(!path.exists(), "successful task journal should be deleted");
        let _ = fs::remove_dir_all(&dir);
    }
}
