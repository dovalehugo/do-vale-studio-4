//! Rational timestamp conversion for playback scheduling.

use dvs_media::{MediaTimestamp, TimeBase};

use crate::error::PlaybackError;

/// Microseconds of monotonic media time relative to the first presented PTS.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct MediaTimeUs(pub i64);

impl MediaTimeUs {
    /// Zero media time.
    pub const ZERO: Self = Self(0);

    /// Returns the raw microsecond value.
    pub const fn as_i64(self) -> i64 {
        self.0
    }

    /// Saturating addition.
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// Saturating subtraction.
    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }
}

/// Converts a [`MediaTimestamp`] to microseconds using checked integer arithmetic.
pub fn timestamp_to_microseconds(timestamp: MediaTimestamp) -> Result<MediaTimeUs, PlaybackError> {
    let pts = timestamp.pts();
    let time_base = timestamp.time_base();
    let num = i64::from(time_base.numerator());
    let den = i64::from(time_base.denominator());
    if num <= 0 || den <= 0 {
        return Err(PlaybackError::InvalidTimeBase);
    }

    let scaled = pts
        .checked_mul(1_000_000)
        .and_then(|value| value.checked_mul(num))
        .ok_or(PlaybackError::TimestampOverflow)?;
    let micros = scaled
        .checked_div(den)
        .ok_or(PlaybackError::InvalidTimeBase)?;
    Ok(MediaTimeUs(micros))
}

/// Normalizes a timestamp relative to an anchor, returning elapsed media microseconds.
pub fn normalize_timestamp(
    timestamp: MediaTimestamp,
    anchor: MediaTimestamp,
) -> Result<MediaTimeUs, PlaybackError> {
    if timestamp.time_base() != anchor.time_base() {
        return Err(PlaybackError::InconsistentTimeBase);
    }

    let value = timestamp_to_microseconds(timestamp)?;
    let anchor_value = timestamp_to_microseconds(anchor)?;
    let normalized = value
        .0
        .checked_sub(anchor_value.0)
        .ok_or(PlaybackError::NegativeNormalizedTimestamp)?;
    if normalized < 0 {
        return Err(PlaybackError::NegativeNormalizedTimestamp);
    }
    Ok(MediaTimeUs(normalized))
}

/// Returns the media duration between two timestamps with the same time base.
pub fn media_duration_between(
    start: MediaTimestamp,
    end: MediaTimestamp,
) -> Result<MediaTimeUs, PlaybackError> {
    normalize_timestamp(end, start)
}

/// Builds a [`TimeBase`] for the 30000/1001 NTSC frame rate cadence.
pub fn ntsc_30000_over_1001_time_base() -> Result<TimeBase, PlaybackError> {
    TimeBase::new(1, 30_000).map_err(|_| PlaybackError::InvalidTimeBase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_media::TimeBase;

    #[test]
    fn timestamp_to_microseconds_uses_checked_math() {
        let tb = TimeBase::new(1, 60_000).expect("time base");
        let ts = MediaTimestamp::new(3_600_000, tb);
        let us = timestamp_to_microseconds(ts).expect("micros");
        assert_eq!(us, MediaTimeUs(60_000_000));
    }

    #[test]
    fn normalize_timestamp_subtracts_anchor() {
        let tb = TimeBase::new(1, 30_000).expect("time base");
        let anchor = MediaTimestamp::new(0, tb);
        let frame = MediaTimestamp::new(1001, tb);
        let normalized = normalize_timestamp(frame, anchor).expect("normalized");
        assert_eq!(normalized, MediaTimeUs(33_366));
    }

    #[test]
    fn inconsistent_time_base_rejected() {
        let a = MediaTimestamp::new(0, TimeBase::new(1, 60_000).expect("tb"));
        let b = MediaTimestamp::new(0, TimeBase::new(1, 30_000).expect("tb"));
        let err = normalize_timestamp(b, a).unwrap_err();
        assert_eq!(err, PlaybackError::InconsistentTimeBase);
    }
}
