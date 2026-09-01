//! Validation errors for video metadata value types.

use thiserror::Error;

/// Error returned when constructing or validating media metadata value types.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum MetadataError {
    /// Width must be greater than zero.
    #[error("width must be greater than zero")]
    ZeroWidth,

    /// Height must be greater than zero.
    #[error("height must be greater than zero")]
    ZeroHeight,

    /// The visible rectangle is invalid (for example, zero width or height).
    #[error("visible rectangle is invalid")]
    InvalidVisibleRect,

    /// The visible rectangle extends outside the allocation extent.
    #[error("visible rectangle is outside the allocation extent")]
    VisibleRectOutOfBounds,

    /// Coordinate arithmetic overflowed while validating dimensions.
    #[error("dimension arithmetic overflow")]
    DimensionOverflow,

    /// A time base must have a positive numerator and denominator.
    #[error("time base numerator and denominator must both be greater than zero")]
    InvalidTimeBase,
}
