//! Frame identity and aggregated video frame metadata.

use crate::color::VideoColorInfo;
use crate::dimensions::VideoDimensions;
use crate::pixel_format::VideoPixelFormat;
use crate::time::MediaTimestamp;

/// Opaque monotonic identifier for a video frame within a session.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct FrameId(u64);

impl FrameId {
    /// Creates a frame identifier from its raw value.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the raw identifier value.
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Metadata describing a decoded video frame without pixel payload.
///
/// Contains dimensions, pixel format, color, and optional timing. There is no
/// GPU handle, CPU buffer, or decoded pixel data in this type.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct VideoFrameMetadata {
    frame_id: FrameId,
    timestamp: Option<MediaTimestamp>,
    dimensions: VideoDimensions,
    pixel_format: VideoPixelFormat,
    color: VideoColorInfo,
}

impl VideoFrameMetadata {
    /// Creates frame metadata from validated value types.
    pub const fn new(
        frame_id: FrameId,
        timestamp: Option<MediaTimestamp>,
        dimensions: VideoDimensions,
        pixel_format: VideoPixelFormat,
        color: VideoColorInfo,
    ) -> Self {
        Self {
            frame_id,
            timestamp,
            dimensions,
            pixel_format,
            color,
        }
    }

    /// Returns the frame identifier.
    pub const fn frame_id(self) -> FrameId {
        self.frame_id
    }

    /// Returns the optional presentation timestamp.
    pub const fn timestamp(self) -> Option<MediaTimestamp> {
        self.timestamp
    }

    /// Returns the allocation and visible dimensions.
    pub const fn dimensions(self) -> VideoDimensions {
        self.dimensions
    }

    /// Returns the pixel format identifier.
    pub const fn pixel_format(self) -> VideoPixelFormat {
        self.pixel_format
    }

    /// Returns the color metadata.
    pub const fn color(self) -> VideoColorInfo {
        self.color
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{ColorMatrix, ColorPrimaries, ColorRange, TransferCharacteristic};
    use crate::dimensions::{Extent2D, VisibleRect};
    use crate::time::TimeBase;

    #[test]
    fn video_frame_metadata_retains_all_supplied_values() {
        let allocation = Extent2D::new(3840, 2176).expect("allocation");
        let visible = VisibleRect::new(0, 0, 3840, 2160).expect("visible");
        let dimensions = VideoDimensions::new(allocation, visible).expect("dimensions");
        let time_base = TimeBase::new(1, 60_000).expect("time base");
        let timestamp = MediaTimestamp::new(3_600_000, time_base);
        let color = VideoColorInfo::new(
            ColorRange::Limited,
            ColorMatrix::Bt709,
            ColorPrimaries::Bt709,
            TransferCharacteristic::Bt709,
        );
        let metadata = VideoFrameMetadata::new(
            FrameId::new(42),
            Some(timestamp),
            dimensions,
            VideoPixelFormat::Nv12,
            color,
        );

        assert_eq!(metadata.frame_id(), FrameId::new(42));
        assert_eq!(metadata.timestamp(), Some(timestamp));
        assert_eq!(metadata.dimensions(), dimensions);
        assert_eq!(metadata.pixel_format(), VideoPixelFormat::Nv12);
        assert_eq!(metadata.color(), color);
    }
}
