//! Timecode formatting helpers.

/// Formats microseconds as `MM:SS.mmm`.
pub fn format_timecode(micros: i64) -> String {
    let total_ms = (micros / 1000).max(0);
    let ms = total_ms % 1000;
    let total_s = total_ms / 1000;
    let s = total_s % 60;
    let m = total_s / 60;
    format!("{m:02}:{s:02}.{ms:03}")
}

/// Formats a current/duration pair for the transport row.
pub fn format_timecode_range(current_us: Option<i64>, duration_us: Option<i64>) -> String {
    let current = current_us
        .map(format_timecode)
        .unwrap_or_else(|| "--:--.---".to_string());
    let duration = duration_us
        .map(format_timecode)
        .unwrap_or_else(|| "--:--.---".to_string());
    format!("{current} / {duration}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_zero() {
        assert_eq!(format_timecode(0), "00:00.000");
    }

    #[test]
    fn formats_one_second() {
        assert_eq!(format_timecode(1_000_000), "00:01.000");
    }

    #[test]
    fn formats_negative_as_zero() {
        assert_eq!(format_timecode(-500), "00:00.000");
    }

    #[test]
    fn range_with_missing_values() {
        assert_eq!(
            format_timecode_range(None, Some(5_000_000)),
            "--:--.--- / 00:05.000"
        );
    }
}
