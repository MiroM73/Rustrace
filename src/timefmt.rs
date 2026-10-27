//! Durations for display: the time span of selected rows and the Δ time column.

/// A duration in a unit that fits its size: `0.120 s`, `12.5 s`, `3 min 05 s`, `1 h 02 min`,
/// `2 d 03 h`. Negative durations get a minus sign.
pub fn format_duration(ms: i64) -> String {
    let sign = if ms < 0 { "−" } else { "" };
    let ms = ms.unsigned_abs();
    let s = ms / 1000;
    let text = if ms < 10_000 {
        format!("{}.{:03} s", s, ms % 1000)
    } else if ms < 60_000 {
        format!("{}.{} s", s, ms % 1000 / 100)
    } else if s < 3600 {
        format!("{} min {:02} s", s / 60, s % 60)
    } else if s < 86_400 {
        format!("{} h {:02} min", s / 3600, s % 3600 / 60)
    } else {
        format!("{} d {:02} h", s / 86_400, s % 86_400 / 3600)
    };
    format!("{sign}{text}")
}

/// A time difference with its sign: `+0.120 s`, `−3 min 05 s`.
pub fn format_delta(ms: i64) -> String {
    if ms < 0 {
        format_duration(ms)
    } else {
        format!("+{}", format_duration(ms))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(format_duration(0), "0.000 s");
        assert_eq!(format_duration(120), "0.120 s");
        assert_eq!(format_duration(9_999), "9.999 s");
        assert_eq!(format_duration(12_560), "12.5 s");
        assert_eq!(format_duration(59_999), "59.9 s");
        assert_eq!(format_duration(185_000), "3 min 05 s");
        assert_eq!(format_duration(3_599_999), "59 min 59 s");
        assert_eq!(format_duration(3_720_000), "1 h 02 min");
        assert_eq!(format_duration(2 * 86_400_000 + 3 * 3_600_000), "2 d 03 h");
        assert_eq!(format_duration(-185_000), "−3 min 05 s");
        assert_eq!(format_delta(120), "+0.120 s");
        assert_eq!(format_delta(0), "+0.000 s");
        assert_eq!(format_delta(-12_500), "−12.5 s");
        assert_eq!(format_delta(3_720_000), "+1 h 02 min");
    }
}
