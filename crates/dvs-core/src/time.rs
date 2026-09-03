//! Timeline time types in integer microseconds.
//!
//! These values describe editorial positions on the project timeline. They are
//! independent of `dvs-media` playback timestamps (`MediaTimestamp`,
//! `TimeBase`, `FrameId`).

use crate::error::EditorError;

/// Absolute position on the project timeline, in microseconds.
///
/// Zero is valid. Negative positions are not representable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelinePosition(u64);

impl TimelinePosition {
    /// Creates a timeline position from a non-negative microsecond value.
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Returns the position in microseconds.
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Adds a duration with overflow checking.
    pub const fn checked_add(self, duration: TimelineDuration) -> Result<Self, EditorError> {
        match self.0.checked_add(duration.as_micros()) {
            Some(sum) => Ok(Self(sum)),
            None => Err(EditorError::TimeOverflow),
        }
    }

    /// Subtracts a duration with underflow checking.
    pub const fn checked_sub(self, duration: TimelineDuration) -> Result<Self, EditorError> {
        match self.0.checked_sub(duration.as_micros()) {
            Some(diff) => Ok(Self(diff)),
            None => Err(EditorError::InvalidRange),
        }
    }

    /// Distance to a later position as a duration.
    pub const fn checked_duration_until(self, end: Self) -> Result<TimelineDuration, EditorError> {
        if end.0 <= self.0 {
            return Err(EditorError::InvalidRange);
        }
        TimelineDuration::from_micros(end.0 - self.0)
    }
}

/// Strictly positive duration on the project timeline, in microseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimelineDuration(u64);

impl TimelineDuration {
    /// Creates a duration from microseconds. Zero is rejected.
    pub const fn from_micros(micros: u64) -> Result<Self, EditorError> {
        if micros == 0 {
            Err(EditorError::ZeroDuration)
        } else {
            Ok(Self(micros))
        }
    }

    /// Returns the duration in microseconds.
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Adds two durations with overflow checking.
    pub const fn checked_add(self, other: Self) -> Result<Self, EditorError> {
        match self.0.checked_add(other.0) {
            Some(sum) => Ok(Self(sum)),
            None => Err(EditorError::TimeOverflow),
        }
    }
}

/// Offset into a media source, in microseconds.
///
/// Zero is valid. This is editorial media time, not a playback PTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceOffset(u64);

impl SourceOffset {
    /// Creates a source offset from microseconds.
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Returns the offset in microseconds.
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// Advances the offset by a duration with overflow checking.
    pub const fn checked_add(self, duration: TimelineDuration) -> Result<Self, EditorError> {
        match self.0.checked_add(duration.as_micros()) {
            Some(sum) => Ok(Self(sum)),
            None => Err(EditorError::TimeOverflow),
        }
    }
}

/// Half-open timeline span `[start, start + duration)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimelineRange {
    start: TimelinePosition,
    duration: TimelineDuration,
}

impl TimelineRange {
    /// Creates a range from a start position and positive duration.
    pub const fn new(start: TimelinePosition, duration: TimelineDuration) -> Self {
        Self { start, duration }
    }

    /// Returns the inclusive start.
    pub const fn start(self) -> TimelinePosition {
        self.start
    }

    /// Returns the duration.
    pub const fn duration(self) -> TimelineDuration {
        self.duration
    }

    /// Returns the exclusive end position.
    pub const fn end(self) -> Result<TimelinePosition, EditorError> {
        self.start.checked_add(self.duration)
    }

    /// Returns true when the ranges share any timeline sample.
    ///
    /// Adjacent ranges (`a.end == b.start`) do not overlap.
    pub fn overlaps(self, other: Self) -> Result<bool, EditorError> {
        let a_end = self.end()?;
        let b_end = other.end()?;
        Ok(self.start < b_end && other.start < a_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_position_is_valid() {
        assert_eq!(TimelinePosition::from_micros(0).as_micros(), 0);
    }

    #[test]
    fn zero_duration_is_rejected() {
        assert_eq!(
            TimelineDuration::from_micros(0),
            Err(EditorError::ZeroDuration)
        );
    }

    #[test]
    fn valid_addition() {
        let start = TimelinePosition::from_micros(1_000);
        let duration = TimelineDuration::from_micros(500).expect("duration");
        assert_eq!(start.checked_add(duration).expect("sum").as_micros(), 1_500);
    }

    #[test]
    fn addition_overflow_is_rejected() {
        let start = TimelinePosition::from_micros(u64::MAX);
        let duration = TimelineDuration::from_micros(1).expect("duration");
        assert_eq!(start.checked_add(duration), Err(EditorError::TimeOverflow));
    }

    #[test]
    fn range_end_and_adjacency() {
        let a = TimelineRange::new(
            TimelinePosition::from_micros(0),
            TimelineDuration::from_micros(10).expect("d"),
        );
        let b = TimelineRange::new(
            TimelinePosition::from_micros(10),
            TimelineDuration::from_micros(5).expect("d"),
        );
        assert_eq!(a.end().expect("end").as_micros(), 10);
        assert!(!a.overlaps(b).expect("overlap"));
        let c = TimelineRange::new(
            TimelinePosition::from_micros(9),
            TimelineDuration::from_micros(2).expect("d"),
        );
        assert!(a.overlaps(c).expect("overlap"));
    }
}
