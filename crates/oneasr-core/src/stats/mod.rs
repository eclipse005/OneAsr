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
//!
//! The two halves live next door — `ledger` owns storage and aggregation,
//! `calendar` the day arithmetic and formatting — and are re-exported here,
//! so `stats::…` stays the single path callers need.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

mod calendar;
mod ledger;

pub use calendar::{
    days_between, days_in_month, format_day, format_span_secs, monday_of, month_of, shift_days,
};
pub use ledger::{LEDGER_FILE, LEDGER_VERSION, append, ledger_path, load, summarize};

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
