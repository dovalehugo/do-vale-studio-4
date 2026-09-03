//! Platform-independent media value types.
//!
//! `dvs-media` provides validated metadata contracts for decoded video frames
//! and a pure [`MediaAsset`] source record. Types here contain no FFmpeg
//! identifiers, no GPU handles, no Windows/COM types, and no raw pixel buffers.
//! Allocation dimensions may include decoder alignment padding; visible
//! dimensions describe the displayable crop.

#![forbid(unsafe_code)]

mod asset;
mod color;
mod dimensions;
mod error;
mod metadata;
mod pixel_format;
mod time;

pub use asset::{MediaAsset, MediaAssetError};
pub use color::{ColorMatrix, ColorPrimaries, ColorRange, TransferCharacteristic, VideoColorInfo};
pub use dimensions::{Extent2D, VideoDimensions, VisibleRect};
pub use error::MetadataError;
pub use metadata::{FrameId, VideoFrameMetadata};
pub use pixel_format::VideoPixelFormat;
pub use time::{MediaTimestamp, TimeBase};

#[cfg(test)]
mod send_sync {
    use super::*;
    use std::sync::{Arc, Mutex};

    const fn assert_send_sync<T: Send + Sync>() {}

    const _: () = {
        assert_send_sync::<FrameId>();
        assert_send_sync::<Extent2D>();
        assert_send_sync::<VisibleRect>();
        assert_send_sync::<VideoDimensions>();
        assert_send_sync::<VideoPixelFormat>();
        assert_send_sync::<ColorRange>();
        assert_send_sync::<ColorMatrix>();
        assert_send_sync::<ColorPrimaries>();
        assert_send_sync::<TransferCharacteristic>();
        assert_send_sync::<VideoColorInfo>();
        assert_send_sync::<TimeBase>();
        assert_send_sync::<MediaTimestamp>();
        assert_send_sync::<VideoFrameMetadata>();
        assert_send_sync::<MetadataError>();
        assert_send_sync::<MediaAsset>();
        assert_send_sync::<MediaAssetError>();
    };

    #[test]
    fn public_value_types_are_send_and_sync() {
        fn assert_values<T: Send + Sync>(value: T) {
            let _ = Arc::new(Mutex::new(value));
        }

        let allocation = Extent2D::new(1, 1).expect("extent");
        let visible = VisibleRect::new(0, 0, 1, 1).expect("visible");
        let dimensions = VideoDimensions::new(allocation, visible).expect("dimensions");
        let time_base = TimeBase::new(1, 1).expect("time base");

        assert_values(FrameId::new(0));
        assert_values(allocation);
        assert_values(visible);
        assert_values(dimensions);
        assert_values(VideoPixelFormat::Nv12);
        assert_values(VideoColorInfo::bt709_limited());
        assert_values(time_base);
        assert_values(MediaTimestamp::new(0, time_base));
        assert_values(VideoFrameMetadata::new(
            FrameId::new(0),
            None,
            dimensions,
            VideoPixelFormat::Nv12,
            VideoColorInfo::bt709_limited(),
        ));
        assert_values(MetadataError::ZeroWidth);
        assert_values(
            MediaAsset::new(dvs_core::MediaAssetId::new(1).expect("id"), "clip.mp4")
                .expect("asset"),
        );
        assert_values(MediaAssetError::EmptySourcePath);
    }
}
