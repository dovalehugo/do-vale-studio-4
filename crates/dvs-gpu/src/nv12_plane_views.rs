//! NV12 plane view creation for imported wgpu textures.

use wgpu::{Device, TextureAspect, TextureFormat, TextureView, TextureViewDescriptor};

use crate::error::GpuError;
use crate::gpu_video_frame::{GpuVideoFrame, GpuVideoPixelFormat};

/// Y and interleaved UV plane views for an imported NV12 texture.
pub struct GpuNv12PlaneViews {
    y: TextureView,
    uv: TextureView,
}

impl GpuNv12PlaneViews {
    /// Returns the luma (Y) plane view (`R8Unorm`, `TextureAspect::Plane0`).
    pub fn y_view(&self) -> &TextureView {
        &self.y
    }

    /// Returns the chroma (UV) plane view (`Rg8Unorm`, `TextureAspect::Plane1`).
    pub fn uv_view(&self) -> &TextureView {
        &self.uv
    }
}

/// Creates validated NV12 plane views from an imported [`GpuVideoFrame`].
pub fn create_nv12_plane_views(
    _device: &Device,
    frame: &GpuVideoFrame,
) -> Result<GpuNv12PlaneViews, GpuError> {
    if frame.pixel_format() != GpuVideoPixelFormat::Nv12 {
        return Err(GpuError::InvalidDecoderTextureFormat);
    }

    let allocation_width = frame.allocation_width();
    let allocation_height = frame.allocation_height();
    if allocation_width == 0
        || allocation_height == 0
        || !allocation_width.is_multiple_of(2)
        || !allocation_height.is_multiple_of(2)
    {
        return Err(GpuError::Nv12DimensionsMustBeEven);
    }

    let texture = frame.texture();
    let y = texture.create_view(&TextureViewDescriptor {
        label: Some("dvs-gpu-nv12-y"),
        format: Some(TextureFormat::R8Unorm),
        aspect: TextureAspect::Plane0,
        ..Default::default()
    });
    let uv = texture.create_view(&TextureViewDescriptor {
        label: Some("dvs-gpu-nv12-uv"),
        format: Some(TextureFormat::Rg8Unorm),
        aspect: TextureAspect::Plane1,
        ..Default::default()
    });

    Ok(GpuNv12PlaneViews { y, uv })
}
