//! Ledger storage (append-only JSON Lines) and the fold that produces the
//! panel's numbers.

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::{StatsRecord, StatsSummary};

/// Ledger filename, next to `settings.json`.
pub const LEDGER_FILE: &str = "stats.jsonl";

/// Schema version written by this build. Readers must keep tolerating records
/// that predate the field (they parse as `0`), and writers must bump this only
/// when a change cannot be expressed as an optional field.
pub const LEDGER_VERSION: u32 = 1;

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
/// `fallback_ledger_path` when the app folder is read-only.
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
}
