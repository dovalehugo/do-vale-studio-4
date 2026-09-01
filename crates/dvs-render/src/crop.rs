//! Visible crop normalization for NV12 plane sampling.

use dvs_media::{VideoFrameMetadata, VideoPixelFormat};

use crate::error::RenderError;

/// Normalized UV bounds for the visible crop within an NV12 allocation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nv12CropUv {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
}

/// Validates NV12 metadata and returns normalized UV bounds for shader sampling.
pub fn normalized_visible_uv(metadata: &VideoFrameMetadata) -> Result<Nv12CropUv, RenderError> {
    if metadata.pixel_format() != VideoPixelFormat::Nv12 {
        return Err(RenderError::UnsupportedPixelFormat);
    }

    let dimensions = metadata.dimensions();
    let allocation = dimensions.allocation();
    let visible = dimensions.visible();

    let alloc_w = allocation.width();
    let alloc_h = allocation.height();
    if alloc_w == 0 || alloc_h == 0 {
        return Err(RenderError::InvalidCrop {
            detail: "allocation dimensions must be non-zero",
        });
    }

    if !alloc_w.is_multiple_of(2) || !alloc_h.is_multiple_of(2) {
        return Err(RenderError::Nv12ChromaAlignment {
            detail: "allocation dimensions must be even",
        });
    }

    if !visible.x().is_multiple_of(2)
        || !visible.y().is_multiple_of(2)
        || !visible.width().is_multiple_of(2)
        || !visible.height().is_multiple_of(2)
    {
        return Err(RenderError::Nv12ChromaAlignment {
            detail: "visible crop origin and size must be even for NV12",
        });
    }

    let u_min = visible.x() as f32 / alloc_w as f32;
    let u_max = (visible.x() + visible.width()) as f32 / alloc_w as f32;
    let v_min = visible.y() as f32 / alloc_h as f32;
    let v_max = (visible.y() + visible.height()) as f32 / alloc_h as f32;

    if !(u_min < u_max && v_min < v_max) {
        return Err(RenderError::InvalidCrop {
            detail: "visible crop produces invalid UV bounds",
        });
    }

    Ok(Nv12CropUv {
        uv_min: [u_min, v_min],
        uv_max: [u_max, v_max],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_media::{Extent2D, FrameId, VideoColorInfo, VideoDimensions, VisibleRect};

    fn metadata_with_visible(
        alloc_w: u32,
        alloc_h: u32,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    ) -> VideoFrameMetadata {
        let allocation = Extent2D::new(alloc_w, alloc_h).expect("allocation");
        let visible = VisibleRect::new(x, y, w, h).expect("visible");
        let dimensions = VideoDimensions::new(allocation, visible).expect("dimensions");
        VideoFrameMetadata::new(
            FrameId::new(0),
            None,
            dimensions,
            VideoPixelFormat::Nv12,
            VideoColorInfo::bt709_limited(),
        )
    }

    #[test]
    fn experiment_fixture_crop_normalizes_padding() {
        let metadata = metadata_with_visible(3840, 2176, 0, 0, 3840, 2160);
        let crop = normalized_visible_uv(&metadata).expect("crop");
        assert!((crop.uv_max[1] - 2160.0 / 2176.0).abs() < 1e-6);
        assert_eq!(crop.uv_min, [0.0, 0.0]);
    }

    #[test]
    fn non_zero_crop_origin_supported() {
        let metadata = metadata_with_visible(1920, 1088, 2, 4, 1916, 1080);
        let crop = normalized_visible_uv(&metadata).expect("crop");
        assert!((crop.uv_min[0] - 2.0 / 1920.0).abs() < 1e-6);
        assert!((crop.uv_min[1] - 4.0 / 1088.0).abs() < 1e-6);
    }

    #[test]
    fn odd_visible_width_rejected() {
        let metadata = metadata_with_visible(1920, 1080, 0, 0, 1919, 1080);
        let err = normalized_visible_uv(&metadata).unwrap_err();
        assert!(matches!(err, RenderError::Nv12ChromaAlignment { .. }));
    }
}
