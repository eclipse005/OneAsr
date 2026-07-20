pub(super) fn gap_ms(left_end_sec: f64, right_start_sec: f64) -> u64 {
    let gap = right_start_sec - left_end_sec;
    if !gap.is_finite() || gap <= 0.0 {
        return 0;
    }
    (gap * 1000.0).round() as u64
}

pub(super) fn seconds_to_ms(seconds: f64) -> u64 {
    if !seconds.is_finite() || seconds <= 0.0 {
        return 0;
    }
    (seconds * 1000.0).round() as u64
}
