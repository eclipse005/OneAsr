//! Year-grid data and panel wording for the stats panel.
//!
//! Everything here is a value or a string — no elements are built — so the
//! layout rules and the phrasing can be unit-tested without standing up a GPUI
//! window. [`super::stats_panel`] turns these into `div`s.

use std::collections::BTreeMap;

use crate::app::prelude::*;

// ─── geometry ────────────────────────────────────────────────────────

/// Stats panel width, sized around the year grid: 12 month blocks span 59–60
/// columns (≈538px worst case), plus the panel's own padding.
pub(super) const STATS_PANEL_W: f32 = 580.;
/// Cell size and pitch. 7px is the smallest that still reads as a *cell*
/// rather than noise; the width is the only thing that can buy more.
pub(super) const STATS_CELL: f32 = 7.;
pub(super) const STATS_GAP: f32 = 2.;
/// Seven weekday rows, fixed.
pub(super) const STATS_GRID_H: f32 = 7. * STATS_CELL + 6. * STATS_GAP;

/// Colour for a day that saw work. Thresholds are **absolute** (15 min / 1 h /
/// 3 h): scaling them against the busiest day would make the same shade mean
/// different things on different screens, which is the one thing a heat map
/// must never do.
pub(super) fn stats_level_color(media_sec: f64) -> Rgba {
    if media_sec >= 3.0 * 3600.0 {
        STATS_L4
    } else if media_sec >= 3600.0 {
        STATS_L3
    } else if media_sec >= 15.0 * 60.0 {
        STATS_L2
    } else {
        STATS_L1
    }
}

// ─── wording ─────────────────────────────────────────────────────────

/// First day the grid may paint. A record stamped in the future (clock moved
/// back) must not widen the window: fall back to today, which paints a single
/// cell at worst.
pub(super) fn stats_range_start(summary: &StatsSummary, today: &str) -> String {
    summary
        .first_day
        .as_deref()
        .filter(|d| *d <= today)
        .unwrap_or(today)
        .to_string()
}

/// Hover-card line for one day.
///
/// Outcomes are split, and the minutes are dropped when the day has none to
/// claim (the probe found no duration): "0 分" would be a number the ledger
/// never measured.
pub(super) fn stats_hover_text(summary: &StatsSummary, day: &str, lang: UiLang) -> String {
    let media = summary.per_day.get(day).copied().unwrap_or(0.0);
    let tasks = summary.per_day_tasks.get(day).copied().unwrap_or(0);
    let errs = summary.per_day_err.get(day).copied().unwrap_or(0);
    if tasks == 0 && errs == 0 {
        let no_tasks = match lang {
            UiLang::Zh => "无任务",
            UiLang::En => "no tasks",
        };
        return format!("{} · {no_tasks}", oneasr_core::stats::format_day(lang, day));
    }
    let mut line = oneasr_core::stats::format_day(lang, day);
    if media > 0.0 {
        line.push_str(&format!(" · {}", oneasr_core::stats::format_span_secs(lang, media)));
    }
    if tasks > 0 {
        let ok = match lang {
            UiLang::Zh => format!(" · 成功 {tasks} 个"),
            UiLang::En => format!(" · {tasks} ok"),
        };
        line.push_str(&ok);
    }
    if errs > 0 {
        let failed = match lang {
            UiLang::Zh => format!(" · 失败 {errs} 个"),
            UiLang::En => format!(" · {errs} failed"),
        };
        line.push_str(&failed);
    }
    line
}

/// Caption under the grid while the record is young.
///
/// Early on the grid is mostly canvas: say where the record starts rather than
/// letting a blank year read as silence — and retire the line once the span is
/// wide enough to speak for itself.
pub(super) fn stats_early_caption(
    summary: &StatsSummary,
    today: &str,
    range_start: &str,
    lang: UiLang,
) -> Option<String> {
    summary
        .first_day
        .as_deref()
        .and_then(|d| oneasr_core::stats::days_between(d, today))
        .filter(|span| (0..84).contains(span))
        .map(|span| match lang {
            UiLang::Zh => format!(
                "记录从 {} 开始 · 已积累 {} 天",
                oneasr_core::stats::format_day(lang, range_start),
                span + 1
            ),
            UiLang::En => format!(
                "Records start {} · {} days logged",
                oneasr_core::stats::format_day(lang, range_start),
                span + 1
            ),
        })
}

// ─── grid ────────────────────────────────────────────────────────────

/// One painted cell. `col`/`row` are absolute grid coordinates, so the painter
/// needs no month arithmetic of its own.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct StatsDayCell {
    pub day: String,
    pub col: usize,
    pub row: usize,
    /// Media seconds that day; `0.0` when nothing measurable ran.
    pub media_sec: f64,
    /// False for a day outside the ledger's span. Such a cell holds its place
    /// in the grid — it is the canvas the year fills in — but it is not
    /// hoverable and never claims "nothing was done".
    pub in_range: bool,
}

/// Resolved year grid: 12 month blocks laid out left to right.
#[derive(Debug, Clone, Default, PartialEq)]
pub(super) struct StatsYearGrid {
    /// `(month, first column)` label anchors, in paint order.
    pub month_cols: Vec<(u32, usize)>,
    /// Cells in paint order (month by month, column by column, row by row).
    pub cells: Vec<StatsDayCell>,
    /// Total columns spanned. The pitch is the single source of truth for
    /// cells, labels and the hover card alike.
    pub col_cursor: usize,
    /// Position of the hovered day, when it falls inside the recorded span.
    pub hover_cell: Option<(usize, usize)>,
}

impl StatsYearGrid {
    /// Width of the painted columns; the panel sizes the grid box with it.
    pub(super) fn grid_w(&self) -> f32 {
        (self.col_cursor as f32 * (STATS_CELL + STATS_GAP) - STATS_GAP).max(0.0)
    }
}

/// Lay out one year.
///
/// Horizontal = months left to right; each month fills 7-day columns in order
/// (1–7, 8–14, …), so a month spans 4 full columns plus a partial 5th (28–31
/// days). Rows are the position within those groups, NOT fixed weekdays — day 1
/// is never guaranteed to be a Monday. The 12-month frame always paints, but a
/// cell only exists for a day this install actually lived through: before the
/// ledger began there is no "did nothing" to report, and after today there is
/// nothing to report yet.
pub(super) fn build_stats_year_grid(
    year: i64,
    range_start: &str,
    today: &str,
    per_day: &BTreeMap<String, f64>,
    hover_day: Option<&str>,
) -> StatsYearGrid {
    let mut grid = StatsYearGrid::default();
    for m in 1..=12u32 {
        let dim = oneasr_core::stats::days_in_month(year, m) as usize;
        if dim == 0 {
            continue;
        }
        let first_col = grid.col_cursor;
        // The layout guarantees every month ≥4 columns, so its label can never
        // collide with a neighbour's.
        grid.month_cols.push((m, first_col));
        for c in 0..dim.div_ceil(7) {
            for row in 0..7usize {
                let day_num = c * 7 + row + 1;
                if day_num > dim {
                    continue;
                }
                let day = format!("{year:04}-{m:02}-{day_num:02}");
                let in_range = day.as_str() >= range_start && day.as_str() <= today;
                if in_range && hover_day == Some(day.as_str()) {
                    grid.hover_cell = Some((first_col + c, row));
                }
                let media_sec = per_day.get(&day).copied().unwrap_or(0.0);
                grid.cells.push(StatsDayCell {
                    day,
                    col: first_col + c,
                    row,
                    media_sec,
                    in_range,
                });
            }
        }
        grid.col_cursor += dim.div_ceil(7);
    }
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary_with(rows: &[(&str, f64, usize, usize)]) -> StatsSummary {
        let mut s = StatsSummary::default();
        for (day, media, tasks, errs) in rows {
            if *media > 0.0 {
                s.per_day.insert((*day).to_string(), *media);
            }
            if *tasks > 0 {
                s.per_day_tasks.insert((*day).to_string(), *tasks);
            }
            if *errs > 0 {
                s.per_day_err.insert((*day).to_string(), *errs);
            }
            if s.first_day.as_deref().is_none_or(|d| *day < d) {
                s.first_day = Some((*day).to_string());
            }
        }
        s
    }

    fn cell<'a>(grid: &'a StatsYearGrid, day: &str) -> &'a StatsDayCell {
        grid.cells
            .iter()
            .find(|c| c.day == day)
            .unwrap_or_else(|| panic!("no cell for {day}"))
    }

    #[test]
    fn grid_covers_every_day_of_the_year() {
        let grid = build_stats_year_grid(2026, "2026-01-01", "2026-12-31", &BTreeMap::new(), None);
        assert_eq!(grid.cells.len(), 365);
        assert_eq!(grid.month_cols.len(), 12);
        // 31-day months take 5 columns, February 4 → 5*11 + 4 = 59.
        assert_eq!(grid.col_cursor, 59);
        assert_eq!(grid.grid_w(), 59. * 9. - 2.);
        assert_eq!(grid.month_cols[0], (1, 0));
        assert_eq!(grid.month_cols[1], (2, 5));
    }

    #[test]
    fn leap_february_takes_a_fifth_column() {
        let leap = build_stats_year_grid(2028, "2028-01-01", "2028-12-31", &BTreeMap::new(), None);
        assert_eq!(leap.cells.len(), 366);
        assert_eq!(leap.col_cursor, 60);
    }

    #[test]
    fn cells_keep_their_place_outside_the_recorded_span() {
        let grid = build_stats_year_grid(
            2026,
            "2026-03-10",
            "2026-03-20",
            &BTreeMap::new(),
            None,
        );
        // The frame still paints 365 cells; only the span is hoverable.
        assert_eq!(grid.cells.len(), 365);
        assert_eq!(grid.cells.iter().filter(|c| c.in_range).count(), 11);
        assert!(!cell(&grid, "2026-03-09").in_range);
        assert!(cell(&grid, "2026-03-10").in_range);
        assert!(cell(&grid, "2026-03-20").in_range);
        assert!(!cell(&grid, "2026-03-21").in_range);
    }

    #[test]
    fn day_within_a_month_walks_columns_then_rows() {
        let grid = build_stats_year_grid(2026, "2026-01-01", "2026-12-31", &BTreeMap::new(), None);
        // 1–7 fill column 0 top to bottom; 8 starts column 1.
        assert_eq!((cell(&grid, "2026-01-01").col, cell(&grid, "2026-01-01").row), (0, 0));
        assert_eq!((cell(&grid, "2026-01-07").col, cell(&grid, "2026-01-07").row), (0, 6));
        assert_eq!((cell(&grid, "2026-01-08").col, cell(&grid, "2026-01-08").row), (1, 0));
        // February starts where January left off (5 columns in).
        assert_eq!((cell(&grid, "2026-02-01").col, cell(&grid, "2026-02-01").row), (5, 0));
    }

    #[test]
    fn hover_resolves_to_grid_coordinates() {
        let grid = build_stats_year_grid(
            2026,
            "2026-01-01",
            "2026-12-31",
            &BTreeMap::new(),
            Some("2026-07-15"),
        );
        let c = cell(&grid, "2026-07-15");
        assert_eq!(grid.hover_cell, Some((c.col, c.row)));
    }

    #[test]
    fn hover_outside_the_span_is_ignored() {
        // Hovering a day after "today" cannot happen through the UI, but the
        // resolver must not invent a position if it ever did.
        let grid = build_stats_year_grid(
            2026,
            "2026-03-10",
            "2026-03-20",
            &BTreeMap::new(),
            Some("2026-12-31"),
        );
        assert_eq!(grid.hover_cell, None);
    }

    #[test]
    fn range_start_never_follows_a_future_stamp() {
        let s = summary_with(&[("2027-01-01", 60.0, 1, 0)]);
        assert_eq!(stats_range_start(&s, "2026-09-16"), "2026-09-16");

        let past = summary_with(&[("2026-01-01", 60.0, 1, 0)]);
        assert_eq!(stats_range_start(&past, "2026-09-16"), "2026-01-01");
    }

    #[test]
    fn hover_text_names_a_day_with_no_work() {
        let s = summary_with(&[]);
        assert_eq!(
            stats_hover_text(&s, "2026-09-16", UiLang::Zh),
            "9 月 16 日 · 无任务"
        );
        assert_eq!(
            stats_hover_text(&s, "2026-09-16", UiLang::En),
            "Sep 16 · no tasks"
        );
    }

    #[test]
    fn hover_text_splits_outcomes_and_omits_unmeasured_minutes() {
        let s = summary_with(&[("2026-09-16", 3600.0, 2, 1)]);
        assert_eq!(
            stats_hover_text(&s, "2026-09-16", UiLang::Zh),
            "9 月 16 日 · 1 小时 · 成功 2 个 · 失败 1 个"
        );
        assert_eq!(
            stats_hover_text(&s, "2026-09-16", UiLang::En),
            "Sep 16 · 1 hr · 2 ok · 1 failed"
        );

        // A failure-only day carries no duration claim.
        let failed = summary_with(&[("2026-09-16", 0.0, 0, 1)]);
        assert_eq!(
            stats_hover_text(&failed, "2026-09-16", UiLang::Zh),
            "9 月 16 日 · 失败 1 个"
        );
    }

    #[test]
    fn early_caption_retires_once_the_span_speaks_for_itself() {
        let young = summary_with(&[("2026-09-01", 60.0, 1, 0)]);
        assert_eq!(
            stats_early_caption(&young, "2026-09-16", "2026-09-01", UiLang::Zh).as_deref(),
            Some("记录从 9 月 1 日 开始 · 已积累 16 天")
        );
        assert_eq!(
            stats_early_caption(&young, "2026-09-16", "2026-09-01", UiLang::En).as_deref(),
            Some("Records start Sep 1 · 16 days logged")
        );

        let old = summary_with(&[("2026-01-01", 60.0, 1, 0)]);
        assert_eq!(
            stats_early_caption(&old, "2026-09-16", "2026-01-01", UiLang::Zh),
            None
        );
    }

    #[test]
    fn level_colour_thresholds_are_absolute() {
        let l1 = stats_level_color(60.0);
        let l2 = stats_level_color(20.0 * 60.0);
        let l3 = stats_level_color(2.0 * 3600.0);
        let l4 = stats_level_color(4.0 * 3600.0);
        assert_eq!(l1, STATS_L1);
        assert_eq!(l2, STATS_L2);
        assert_eq!(l3, STATS_L3);
        assert_eq!(l4, STATS_L4);
        // Boundaries belong to the higher band.
        assert_eq!(stats_level_color(15.0 * 60.0), STATS_L2);
        assert_eq!(stats_level_color(3600.0), STATS_L3);
        assert_eq!(stats_level_color(3.0 * 3600.0), STATS_L4);
    }
}
