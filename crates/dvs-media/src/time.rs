//! Rational time base and media timestamps.

use crate::error::MetadataError;

/// A positive rational time base for timestamp interpretation.
///
/// Stored as integer numerator and denominator only; no floating-point representation.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct TimeBase {
    numerator: u32,
    denominator: u32,
}

impl TimeBase {
    /// Creates a time base with both numerator and denominator greater than zero.
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, MetadataError> {
        if numerator == 0 || denominator == 0 {
            return Err(MetadataError::InvalidTimeBase);
        }
        Ok(Self {
            numerator,
            denominator,
        })
    }

    /// Returns the time base numerator.
    pub fn numerator(self) -> u32 {
        self.numerator
    }

    /// Returns the time base denominator.
    pub fn denominator(self) -> u32 {
        self.denominator
    }
}

/// A presentation timestamp expressed in a rational time base.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MediaTimestamp {
    pts: i64,
    time_base: TimeBase,
}

impl MediaTimestamp {
    /// Creates a timestamp from a PTS value and its time base.
    pub const fn new(pts: i64, time_base: TimeBase) -> Self {
        Self { pts, time_base }
    }

    /// Returns the presentation timestamp in time-base units.
    pub const fn pts(self) -> i64 {
        self.pts
    }

    /// Returns the rational time base for this timestamp.
    pub const fn time_base(self) -> TimeBase {
        self.time_base
    }

    /// Converts the timestamp to seconds as `f64`.
    ///
    /// This is a convenience conversion for display or logging. It is not used
    /// for frame-accurate scheduling or synchronization.
    pub fn as_seconds_f64(self) -> f64 {
        self.pts as f64 * self.time_base.numerator() as f64 / self.time_base.denominator() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_time_base_accepted() {
        let tb = TimeBase::new(1, 60_000).expect("time base");
        assert_eq!(tb.numerator(), 1);
        assert_eq!(tb.denominator(), 60_000);
    }

    #[test]
    fn zero_numerator_rejected() {
        let err = TimeBase::new(0, 60_000).unwrap_err();
        assert_eq!(err, MetadataError::InvalidTimeBase);
    }

    #[test]
    fn zero_denominator_rejected() {
        let err = TimeBase::new(1, 0).unwrap_err();
        assert_eq!(err, MetadataError::InvalidTimeBase);
    }
}
