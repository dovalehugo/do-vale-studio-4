//! Video pixel format identifiers for production GPU paths.

/// Describes the pixel layout of a decoded video frame.
///
/// Variants identify formats planned or validated for GPU ingestion. They do not
/// imply CPU storage, buffer layout, or FFmpeg pixel format identifiers.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum VideoPixelFormat {
    /// 8-bit 4:2:0 semi-planar NV12.
    Nv12,
    /// 10-bit 4:2:0 semi-planar P010.
    P010,
}

impl VideoPixelFormat {
    /// Returns the effective bit depth per luma/chroma sample.
    pub const fn bit_depth(self) -> u8 {
        match self {
            Self::Nv12 => 8,
            Self::P010 => 10,
        }
    }

    /// Returns `true` when the format stores planes in separate memory regions.
    pub const fn is_multiplanar(self) -> bool {
        match self {
            Self::Nv12 | Self::P010 => true,
        }
    }
}
