//! Typed playback scheduling and clock errors.

use thiserror::Error;

/// Error returned by playback clock, scheduler, and timestamp conversion.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum PlaybackError {
    /// A required presentation timestamp is missing.
    #[error("frame presentation timestamp is missing")]
    MissingTimestamp,

    /// Stream time bases do not match.
    #[error("inconsistent media time base")]
    InconsistentTimeBase,

    /// Presentation timestamps are not monotonically increasing.
    #[error("non-monotonic presentation timestamp")]
    NonMonotonicTimestamp,

    /// A presentation timestamp is negative after normalization.
    #[error("negative normalized presentation timestamp")]
    NegativeNormalizedTimestamp,

    /// Timestamp conversion overflowed signed integer range.
    #[error("timestamp conversion overflow")]
    TimestampOverflow,

    /// The media time base cannot be represented.
    #[error("invalid or unrepresentable media time base")]
    InvalidTimeBase,

    /// Playback clock is not in a state that permits the requested operation.
    #[error("invalid playback clock state")]
    InvalidClockState,
}
