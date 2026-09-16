//! Proleptic-Gregorian calendar arithmetic and the panel's number formatting.
//!
//! No dependency and no clock: the caller supplies the local day. Kept apart
//! from the ledger because these are the grid's rules, not the ledger's — the
//! year grid asks "how many days in this month" far more often than the ledger
//! asks anything at all.

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
    fn day_labels_read_in_chinese() {
        assert_eq!(format_day_cn("2026-09-04"), "9 月 4 日");
        assert_eq!(format_day_cn("2026-12-31"), "12 月 31 日");
        assert_eq!(format_day_cn("nope"), "nope");
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
