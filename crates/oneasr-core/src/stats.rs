//! Local usage ledger behind the stats panel — numbers only, append-only.
//!
//! Two rules that must not drift, because the panel's credibility rests on them:
//!
//! 1. **Numbers only.** No filenames, no paths, no subtitle text. The product's
//!    whole promise is that your media never leaves this machine; a ledger that
//!    remembers *what* you processed works against that promise.
//! 2. **Saved time is 1:1** — `media − processing`, with no efficiency factor
//!    borrowed from anywhere. A factor would need a footnote and could be
//!    argued with; a subtraction neither needs nor can.
//!
//! Storage is append-only JSON Lines at `{app_root}/stats.jsonl`, so a torn
//! write costs one record instead of the file, and the ledger stays readable
//! in Notepad. At roughly 100 bytes per task the file reaches ~4 MiB after
//! ~40,000 tasks, so no rotation is warranted.
//!
//! The caller supplies the local calendar day: `oneasr-core` has no clock
//! dependency, and reading the OS local date at *append* time is what makes
//! each record's day correct across DST changes (recomputing later from a
//! UTC stamp would need the offset history).

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Ledger filename, next to `settings.json`.
pub const LEDGER_FILE: &str = "stats.jsonl";

/// Schema version written by this build. Readers must keep tolerating records
/// that predate the field (they parse as `0`), and writers must bump this only
/// when a change cannot be expressed as an optional field.
pub const LEDGER_VERSION: u32 = 1;

/// One finished task. Written whether it succeeded or failed — failures count
/// toward the task tally but never toward saved time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsRecord {
    /// Ledger schema version. Absent in the first shipped ledger (reads as 0),
    /// which is exactly why it exists: a future required field can branch on it
    /// instead of silently dropping the user's history.
    #[serde(default)]
    pub v: u32,
    /// Local calendar day, `YYYY-MM-DD`.
    pub day: String,
    /// Media duration in seconds. `None` when the probe found nothing usable —
    /// such a task still counts as a task, it just cannot contribute to saved
    /// time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_sec: Option<f64>,
    /// Wall-clock processing time for the whole run.
    pub process_ms: u64,
    /// Source language short id (`zh`, `en`, …).
    pub lang: String,
    /// Whether vocal separation ran for this task.
    #[serde(default)]
    pub sep: bool,
    pub ok: bool,
    /// Sentence count of the exported subtitle. SRT cues and TXT lines both
    /// derive from the same sentence list, so one number covers either format
    /// (and both at once — counted once per task, not per file).
    /// `0` when it could not be counted; the ledger tolerates gaps.
    #[serde(default)]
    pub cues: u32,
}

/// Aggregated view for the panel. Built from the whole ledger; the panel shows
/// cumulative totals and never recomputes them per frame (see `App::stats`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsSummary {
    pub tasks_ok: usize,
    pub tasks_err: usize,
    /// Successful tasks only — a failure produced no deliverable.
    pub media_sec: f64,
    /// Successful tasks only, so `media − process` stays meaningful.
    pub process_ms: u64,
    /// `(lang, count)` for successful tasks, most used first then alphabetical.
    pub langs: Vec<(String, usize)>,
    /// Tasks that ran vocal separation.
    pub sep_tasks: usize,
    /// Media seconds per local day (successful tasks), for the year grid.
    pub per_day: BTreeMap<String, f64>,
    /// Successful task count per local day, for the grid's hover card.
    pub per_day_tasks: BTreeMap<String, usize>,
    /// Failed task count per local day. The grid is about minutes, but a day
    /// that only failed must not read as "no tasks" — that would be the panel
    /// telling a softer story than the ledger holds.
    pub per_day_err: BTreeMap<String, usize>,
    /// Longest single piece of media processed successfully.
    pub longest_media_sec: Option<f64>,
    /// Best `media / process` ratio seen on a successful task.
    pub fastest_speed: Option<f64>,
    /// Exported sentence lines across successful tasks.
    pub cues: u64,
    /// Earliest day in the ledger, failures included. The year grid renders
    /// nothing before it, so "no data yet" never reads as "you did nothing".
    pub first_day: Option<String>,
}

impl StatsSummary {
    /// Total tasks, success and failure alike.
    pub fn tasks_total(&self) -> usize {
        self.tasks_ok + self.tasks_err
    }

    /// Seconds saved, 1:1. `None` until something measurable has run — the
    /// panel shows nothing rather than a misleading zero.
    pub fn saved_sec(&self) -> Option<f64> {
        (self.media_sec > 0.0).then(|| (self.media_sec - self.process_ms as f64 / 1000.0).max(0.0))
    }

    /// Average × realtime over successful tasks.
    pub fn avg_speed(&self) -> Option<f64> {
        let secs = self.process_ms as f64 / 1000.0;
        (self.media_sec > 0.0 && secs > 0.0).then(|| self.media_sec / secs)
    }

    /// True when there is nothing worth showing yet.
    pub fn is_empty(&self) -> bool {
        self.tasks_total() == 0
    }
}

/// `{root}/stats.jsonl`.
pub fn ledger_path(root: &Path) -> PathBuf {
    root.join(LEDGER_FILE)
}

/// Fallback home when `{app_root}` is not writable — a portable copy dropped in
/// `C:\Program Files` would otherwise accrue nothing, and a stats panel that
/// silently stays at zero is worse than no panel. Same shape as the crash log's
/// fallback (`%LOCALAPPDATA%\OneAsr\`, else the temp dir).
fn fallback_ledger_path() -> PathBuf {
    let base = match std::env::var_os("LOCALAPPDATA") {
        Some(v) if !v.is_empty() => PathBuf::from(v),
        _ => std::env::temp_dir(),
    };
    base.join("OneAsr").join(LEDGER_FILE)
}

/// Append one record. Creates the directory if needed, and falls back to
/// [`fallback_ledger_path`] when the app folder is read-only.
///
/// A single `write_all` of one line keeps records atomic against concurrent
/// readers; the caller is expected to treat failure as non-fatal (a stats
/// ledger must never break a transcription run).
pub fn append(root: &Path, rec: &StatsRecord) -> std::io::Result<()> {
    let mut line = serde_json::to_string(rec).map_err(std::io::Error::other)?;
    line.push('\n');
    append_at(&ledger_path(root), &line)
        .or_else(|primary| append_at(&fallback_ledger_path(), &line).map_err(|_| primary))
}

fn append_at(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())
}

/// Read every record, from the app folder and from the fallback location.
///
/// Both are read (not just the first that exists): an install that started
/// read-only and later became writable — or the reverse — has real records in
/// each, and a ledger that forgets half of them would be lying.
pub fn load(root: &Path) -> Vec<StatsRecord> {
    load_from(&[ledger_path(root), fallback_ledger_path()])
}

/// Read and concatenate ledger files in order. Unreadable or malformed lines
/// are skipped, not fatal: one bad line must not cost the user their history.
fn load_from(paths: &[PathBuf]) -> Vec<StatsRecord> {
    let mut out = Vec::new();
    for path in paths {
        out.extend(read_ledger(path));
    }
    out
}

fn read_ledger(path: &Path) -> Vec<StatsRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str::<StatsRecord>(l).ok())
        .collect()
}

/// Fold records into the panel's numbers.
pub fn summarize(records: &[StatsRecord]) -> StatsSummary {
    let mut s = StatsSummary::default();
    let mut lang_counts: BTreeMap<String, usize> = BTreeMap::new();

    for r in records {
        // Day span covers every record, failures included: the grid must know
        // when the ledger began even if that first task failed.
        if s.first_day.as_ref().is_none_or(|d| &r.day < d) {
            s.first_day = Some(r.day.clone());
        }
        if r.ok {
            s.tasks_ok += 1;
        } else {
            s.tasks_err += 1;
            *s.per_day_err.entry(r.day.clone()).or_insert(0) += 1;
            continue;
        }
        // Everything below is success-only: failures contributed no deliverable,
        // so counting their runtime as "spent" would understate what was saved.
        s.process_ms = s.process_ms.saturating_add(r.process_ms);
        s.cues += r.cues as u64;
        if r.sep {
            s.sep_tasks += 1;
        }
        *lang_counts.entry(r.lang.clone()).or_insert(0) += 1;
        *s.per_day_tasks.entry(r.day.clone()).or_insert(0) += 1;

        let Some(media) = r.media_sec.filter(|v| v.is_finite() && *v > 0.0) else {
            continue;
        };
        s.media_sec += media;
        *s.per_day.entry(r.day.clone()).or_insert(0.0) += media;

        if s.longest_media_sec.is_none_or(|m| media > m) {
            s.longest_media_sec = Some(media);
        }
        let secs = r.process_ms as f64 / 1000.0;
        if secs > 0.0 {
            let speed = media / secs;
            if s.fastest_speed.is_none_or(|f| speed > f) {
                s.fastest_speed = Some(speed);
            }
        }
    }

    // Most used first; ties broken by language id so the order is stable.
    let mut langs: Vec<(String, usize)> = lang_counts.into_iter().collect();
    langs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    s.langs = langs;
    s
}

// ─── calendar (no dependency; proleptic Gregorian) ───────────────────

/// Days since 1970-01-01. Howard Hinnant's `days_from_civil`.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = (m + 9) % 12; // Mar = 0
    let doy = (153 * mp + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`].
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn parse_ymd(day: &str) -> Option<(i64, i64, i64)> {
    let mut it = day.split('-');
    let y = it.next()?.parse().ok()?;
    let m = it.next()?.parse().ok()?;
    let d = it.next()?.parse().ok()?;
    if it.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
}

fn format_ymd(y: i64, m: i64, d: i64) -> String {
    format!("{y:04}-{m:02}-{d:02}")
}

/// Monday of the week containing `day` (weeks start on Monday).
pub fn monday_of(day: &str) -> Option<String> {
    let (y, m, d) = parse_ymd(day)?;
    let days = days_from_civil(y, m, d);
    // 1970-01-01 was a Thursday, so +3 maps Monday to 0.
    let dow_mon0 = (days + 3).rem_euclid(7);
    let (y2, m2, d2) = civil_from_days(days - dow_mon0);
    Some(format_ymd(y2, m2, d2))
}

/// `day` moved by `delta` days (negative goes back).
pub fn shift_days(day: &str, delta: i64) -> Option<String> {
    let (y, m, d) = parse_ymd(day)?;
    let (y2, m2, d2) = civil_from_days(days_from_civil(y, m, d) + delta);
    Some(format_ymd(y2, m2, d2))
}

/// Month number `1..=12` encoded in a `YYYY-MM-DD` string.
pub fn month_of(day: &str) -> Option<u32> {
    parse_ymd(day).map(|(_, m, _)| m as u32)
}

/// Days in a month, proleptic Gregorian. Feeds the year grid's month blocks:
/// each month fills 7-day columns in order, so 31-day months span 5 columns
/// (the last one partial) and February usually exactly 4.
pub fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Whole days from `from` to `to` (negative when `to` is earlier).
///
/// Used by the year grid to turn a hovered day back into its column/row.
pub fn days_between(from: &str, to: &str) -> Option<i64> {
    let (fy, fm, fd) = parse_ymd(from)?;
    let (ty, tm, td) = parse_ymd(to)?;
    Some(days_from_civil(ty, tm, td) - days_from_civil(fy, fm, fd))
}

// ─── formatting ─────────────────────────────────────────────────────

/// `2026-09-04` → `9 月 4 日`, for the grid's hover card.
pub fn format_day_cn(day: &str) -> String {
    match parse_ymd(day) {
        Some((_, m, d)) => format!("{m} 月 {d} 日"),
        None => day.to_string(),
    }
}

/// Compact Chinese span: `3 小时 20 分` / `45 分` / `30 秒`.
///
/// Minutes are the smallest unit the panel ever shows — seconds would imply a
/// precision the underlying measurements do not have.
pub fn format_span_secs(secs: f64) -> String {
    if !secs.is_finite() || secs <= 0.0 {
        return "0 分".into();
    }
    // Branch on the *rounded seconds*, not on rounded minutes: 45 s rounds to
    // 1 min, which would report "1 分" for something under a minute.
    let total_sec = secs.round() as u64;
    if total_sec < 60 {
        return format!("{} 秒", total_sec.max(1));
    }
    let total_min = (total_sec + 30) / 60;
    let h = total_min / 60;
    let m = total_min % 60;
    if h == 0 {
        format!("{m} 分")
    } else if m == 0 {
        format!("{h} 小时")
    } else {
        format!("{h} 小时 {m} 分")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(day: &str, media: Option<f64>, ms: u64, ok: bool) -> StatsRecord {
        StatsRecord {
            v: LEDGER_VERSION,
            day: day.into(),
            media_sec: media,
            process_ms: ms,
            lang: "zh".into(),
            sep: false,
            ok,
            cues: 0,
        }
    }

    #[test]
    fn civil_calendar_matches_known_weekdays() {
        // Monday = 0. Anchors chosen so a weekday slip cannot pass.
        let dow = |y, m, d| (days_from_civil(y, m, d) + 3).rem_euclid(7);
        assert_eq!(dow(1970, 1, 1), 3, "1970-01-01 was a Thursday");
        assert_eq!(dow(2000, 1, 1), 5, "2000-01-01 was a Saturday");
        assert_eq!(dow(2026, 9, 13), 6, "2026-09-13 is a Sunday");
        assert_eq!(dow(2026, 7, 1), 2, "2026-07-01 is a Wednesday");
        assert_eq!(dow(2026, 8, 1), 5, "2026-08-01 is a Saturday");
    }

    #[test]
    fn civil_conversion_round_trips() {
        for day in ["1970-01-01", "2024-02-29", "2026-09-13", "2100-12-31"] {
            let (y, m, d) = parse_ymd(day).unwrap();
            let back = civil_from_days(days_from_civil(y, m, d));
            assert_eq!(back, (y, m, d), "round trip failed for {day}");
        }
    }

    #[test]
    fn monday_of_uses_monday_as_week_start() {
        // Sunday belongs to the week that started the previous Monday.
        assert_eq!(monday_of("2026-09-13").unwrap(), "2026-09-07");
        assert_eq!(monday_of("2026-09-07").unwrap(), "2026-09-07");
        assert_eq!(monday_of("2026-09-08").unwrap(), "2026-09-07");
        // Saturday 2026-08-01 → Monday 2026-07-27 (crosses the month).
        assert_eq!(monday_of("2026-08-01").unwrap(), "2026-07-27");
    }

    #[test]
    fn shift_days_crosses_month_and_year() {
        assert_eq!(shift_days("2026-09-13", -7).unwrap(), "2026-09-06");
        assert_eq!(shift_days("2026-01-01", -1).unwrap(), "2025-12-31");
        assert_eq!(shift_days("2024-02-28", 1).unwrap(), "2024-02-29");
        assert_eq!(shift_days("2026-09-13", 0).unwrap(), "2026-09-13");
    }

    #[test]
    fn days_between_is_signed_and_grid_cell_math_holds() {
        assert_eq!(days_between("2026-09-07", "2026-09-13"), Some(6));
        assert_eq!(days_between("2026-09-13", "2026-09-07"), Some(-6));
        assert_eq!(days_between("2026-09-13", "2026-09-13"), Some(0));
        assert_eq!(days_between("bad", "2026-09-13"), None);

        // The year grid derives column/row from a day's offset within its week.
        let week = monday_of("2026-09-13").unwrap();
        assert_eq!(week, "2026-09-07");
        let off = days_between(&week, "2026-09-13").unwrap();
        assert_eq!((off / 7, off % 7), (0, 6), "Sunday is row 6 of its own week");
        let off2 = days_between(&week, "2026-09-21").unwrap();
        assert_eq!((off2 / 7, off2 % 7), (2, 0));
    }

    #[test]
    fn malformed_days_are_rejected_not_guessed() {
        for bad in ["", "2026", "2026-09", "2026-09-13-1", "x-y-z", "2026-13-01"] {
            assert!(parse_ymd(bad).is_none(), "{bad:?} should not parse");
        }
        assert!(monday_of("nope").is_none());
        assert!(shift_days("nope", 1).is_none());
    }

    #[test]
    fn failures_count_as_tasks_but_never_as_saved_time() {
        let s = summarize(&[
            rec("2026-09-13", Some(600.0), 60_000, true),
            rec("2026-09-13", Some(3600.0), 300_000, false),
        ]);
        assert_eq!(s.tasks_ok, 1);
        assert_eq!(s.tasks_err, 1);
        assert_eq!(s.tasks_total(), 2);
        // 600s of media, 60s of machine time — the failure is invisible here.
        assert_eq!(s.media_sec, 600.0);
        assert_eq!(s.process_ms, 60_000);
        assert!((s.saved_sec().unwrap() - 540.0).abs() < 1e-6);
    }

    #[test]
    fn unknown_duration_counts_as_task_but_not_as_time() {
        let s = summarize(&[rec("2026-09-13", None, 12_000, true)]);
        assert_eq!(s.tasks_ok, 1, "still a completed task");
        assert_eq!(s.media_sec, 0.0);
        assert!(s.saved_sec().is_none(), "nothing measurable → no claim");
        assert!(s.avg_speed().is_none());
        assert!(s.per_day.is_empty());
    }

    #[test]
    fn cues_count_once_per_task_and_first_day_spans_failures() {
        let mut a = rec("2026-09-12", Some(600.0), 60_000, true);
        a.cues = 120;
        // A failed task that ran earlier still marks where the ledger began,
        // but contributes no lines.
        let mut b = rec("2026-09-10", Some(600.0), 60_000, false);
        b.cues = 999;
        let s = summarize(&[a, b]);
        assert_eq!(s.cues, 120, "failed task produced no deliverable lines");
        assert_eq!(s.first_day.as_deref(), Some("2026-09-10"));
    }

    #[test]
    fn days_in_month_handles_ordinary_leap_and_century_years() {
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(2024, 2), 29, "divisible by 4");
        assert_eq!(days_in_month(2000, 2), 29, "divisible by 400");
        assert_eq!(days_in_month(1900, 2), 28, "divisible by 100 but not 400");
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
        assert_eq!(days_in_month(2026, 13), 0, "month 13 does not exist");
    }

    #[test]
    fn empty_ledger_makes_no_claims() {
        let s = summarize(&[]);
        assert!(s.is_empty());
        assert!(s.saved_sec().is_none());
        assert!(s.avg_speed().is_none());
        assert_eq!(s.tasks_total(), 0);
    }

    #[test]
    fn saved_time_is_one_to_one_with_no_fudge_factor() {
        // 2 h of media in 10 min of machine time → 1 h 50 m saved, not a
        // multiple of anything.
        let s = summarize(&[rec("2026-09-13", Some(7200.0), 600_000, true)]);
        assert!((s.saved_sec().unwrap() - 6600.0).abs() < 1e-6);
        assert!((s.avg_speed().unwrap() - 12.0).abs() < 1e-6);
    }

    #[test]
    fn per_day_and_records_accumulate_across_days() {
        let s = summarize(&[
            rec("2026-09-12", Some(600.0), 60_000, true),
            rec("2026-09-13", Some(1200.0), 60_000, true),
            rec("2026-09-13", Some(600.0), 60_000, true),
            rec("2026-09-13", Some(60.0), 6000, false),
        ]);
        assert_eq!(s.per_day.get("2026-09-12").copied(), Some(600.0));
        assert_eq!(s.per_day.get("2026-09-13").copied(), Some(1800.0));
        assert_eq!(s.per_day_tasks.get("2026-09-13").copied(), Some(2));
        assert_eq!(
            s.per_day_err.get("2026-09-13").copied(),
            Some(1),
            "the failed run belongs to its day, even with no minutes to add"
        );
        assert_eq!(s.per_day_err.get("2026-09-12"), None);
        assert_eq!(s.longest_media_sec, Some(1200.0));
    }

    #[test]
    fn day_labels_read_in_chinese() {
        assert_eq!(format_day_cn("2026-09-04"), "9 月 4 日");
        assert_eq!(format_day_cn("2026-12-31"), "12 月 31 日");
        assert_eq!(format_day_cn("nope"), "nope");
    }

    #[test]
    fn langs_are_ranked_then_stable() {
        let mut a = rec("2026-09-13", Some(60.0), 6000, true);
        a.lang = "zh".into();
        let mut b = rec("2026-09-13", Some(60.0), 6000, true);
        b.lang = "en".into();
        let mut c = rec("2026-09-13", Some(60.0), 6000, true);
        c.lang = "en".into();
        let s = summarize(&[a, b, c]);
        assert_eq!(s.langs, vec![("en".into(), 2), ("zh".into(), 1)]);
    }

    #[test]
    fn ledger_round_trips_and_skips_corrupt_lines() {
        let dir = std::env::temp_dir().join(format!("oneasr-stats-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        let a = rec("2026-09-12", Some(600.0), 60_000, true);
        let mut b = rec("2026-09-13", None, 5000, false);
        b.sep = true;
        append(&dir, &a).unwrap();
        append(&dir, &b).unwrap();

        // Simulate a torn write / hand-edited junk.
        {
            use std::io::Write as _;
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(ledger_path(&dir))
                .unwrap();
            writeln!(f, "{{ not json").unwrap();
        }

        // Read the app-folder ledger on its own: `load` also merges the
        // per-machine fallback, which is real user data and not ours to assert
        // about in a test.
        let loaded = read_ledger(&ledger_path(&dir));
        assert_eq!(loaded.len(), 2, "corrupt line must be skipped, not fatal");
        assert_eq!(loaded[0], a);
        assert_eq!(loaded[1], b);

        let s = summarize(&loaded);
        assert_eq!(s.tasks_total(), 2);
        assert_eq!(s.sep_tasks, 0, "the sep flag belonged to a failed task");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_on_missing_ledger_is_empty_not_an_error() {
        let dir = std::env::temp_dir().join(format!("oneasr-stats-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(read_ledger(&ledger_path(&dir)).is_empty());
        assert!(load_from(&[ledger_path(&dir)]).is_empty());
    }

    #[test]
    fn v1_records_carry_the_schema_version() {
        let line = serde_json::to_string(&rec("2026-09-13", Some(60.0), 6000, true)).unwrap();
        assert!(
            line.contains(r#""v":1"#),
            "new records must be stamped: {line}"
        );
    }

    #[test]
    fn records_from_before_the_version_field_still_parse() {
        // Written by the first shipped ledger: no `v`, and no optional fields.
        let legacy = r#"{"day":"2026-09-10","process_ms":60000,"lang":"zh","ok":true}"#;
        let parsed: StatsRecord = serde_json::from_str(legacy).expect("legacy line still parses");
        assert_eq!(parsed.v, 0);
        assert_eq!(parsed.day, "2026-09-10");
        assert_eq!(parsed.media_sec, None);
        assert!(!parsed.sep);
        assert_eq!(parsed.cues, 0);
    }

    #[test]
    fn spans_read_naturally_in_chinese() {
        assert_eq!(format_span_secs(45.0), "45 秒");
        assert_eq!(format_span_secs(600.0), "10 分");
        assert_eq!(format_span_secs(3600.0), "1 小时");
        assert_eq!(format_span_secs(1200.0), "20 分");
        assert_eq!(format_span_secs(7200.0 + 1500.0), "2 小时 25 分");
        assert_eq!(format_span_secs(0.0), "0 分");
        assert_eq!(format_span_secs(f64::NAN), "0 分");
    }
}
