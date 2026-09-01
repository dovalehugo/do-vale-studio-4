//! Opaque imported NV12 GPU video frame metadata.

use wgpu::Texture;

/// Pixel format of an imported [`GpuVideoFrame`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum GpuVideoPixelFormat {
    /// 4:2:0 NV12 allocation imported from the shared D3D11/D3D12 bridge.
    Nv12,
}

/// Opaque imported NV12 GPU video frame.
///
/// Constructed only by the Windows interop bridge. Represents the single shared
/// NV12 allocation for Integration 3C. Does not expose HAL resources, handles, or
/// fence values.
pub struct GpuVideoFrame {
    allocation_width: u32,
    allocation_height: u32,
    texture: Texture,
}

impl GpuVideoFrame {
    pub(crate) fn new(allocation_width: u32, allocation_height: u32, texture: Texture) -> Self {
        Self {
            allocation_width,
            allocation_height,
            texture,
        }
    }

    /// Returns the NV12 allocation width in pixels.
    pub fn allocation_width(&self) -> u32 {
        self.allocation_width
    }

    /// Returns the NV12 allocation height in pixels.
    pub fn allocation_height(&self) -> u32 {
        self.allocation_height
    }

    /// Returns the imported NV12 pixel format.
    pub fn pixel_format(&self) -> GpuVideoPixelFormat {
        GpuVideoPixelFormat::Nv12
    }

    /// Returns the imported wgpu texture for downstream render consumers.
    pub fn texture(&self) -> &Texture {
        &self.texture
    }
}
