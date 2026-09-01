//! Typed rendering errors for the production NV12 path.

use thiserror::Error;

/// Error returned by the production NV12 renderer and supporting helpers.
#[derive(Debug, Error)]
pub enum RenderError {
    /// The frame pixel format is not NV12.
    #[error("unsupported pixel format for NV12 renderer")]
    UnsupportedPixelFormat,

    /// Allocation or visible crop metadata is invalid for NV12 rendering.
    #[error("invalid allocation or visible crop: {detail}")]
    InvalidCrop { detail: &'static str },

    /// NV12 chroma alignment requirements are not met by the visible crop.
    #[error("NV12 chroma alignment violation: {detail}")]
    Nv12ChromaAlignment { detail: &'static str },

    /// Color matrix metadata is unsupported or unspecified.
    #[error("unsupported color matrix: {0:?}")]
    UnsupportedColorMatrix(dvs_media::ColorMatrix),

    /// Color range metadata is unsupported or unspecified.
    #[error("unsupported color range: {0:?}")]
    UnsupportedColorRange(dvs_media::ColorRange),

    /// Transfer characteristic is unsupported for the SDR renderer path.
    #[error("unsupported transfer characteristic: {0:?}")]
    UnsupportedTransfer(dvs_media::TransferCharacteristic),

    /// HDR transfer paths are rejected for the SDR milestone.
    #[error("HDR transfer rejected for SDR renderer")]
    HdrTransferRejected,

    /// No supported non-HDR surface format is available.
    #[error("unsupported output surface format")]
    UnsupportedOutputFormat,

    /// Plane view creation failed.
    #[error("NV12 plane view creation failed: {0}")]
    PlaneViewCreation(#[from] dvs_gpu::GpuError),

    /// Target dimensions are zero.
    #[error("invalid target dimensions")]
    InvalidTargetDimensions,

    /// wgpu surface is lost.
    #[error("surface lost")]
    SurfaceLost,

    /// wgpu surface is outdated and must be reconfigured.
    #[error("surface outdated")]
    SurfaceOutdated,

    /// wgpu surface acquisition timed out.
    #[error("surface timeout")]
    SurfaceTimeout,

    /// wgpu surface is out of memory.
    #[error("surface out of memory")]
    SurfaceOutOfMemory,

    /// Shader module compilation failed.
    #[error("shader compilation failed")]
    ShaderCompilationFailed,

    /// Render pipeline creation failed.
    #[error("render pipeline creation failed")]
    PipelineCreationFailed,

    /// Bind group creation failed because plane views are incompatible.
    #[error("bind group creation failed")]
    BindGroupCreationFailed,
}

impl From<wgpu::SurfaceError> for RenderError {
    fn from(error: wgpu::SurfaceError) -> Self {
        match error {
            wgpu::SurfaceError::Lost => Self::SurfaceLost,
            wgpu::SurfaceError::Outdated => Self::SurfaceOutdated,
            wgpu::SurfaceError::Timeout => Self::SurfaceTimeout,
            wgpu::SurfaceError::OutOfMemory => Self::SurfaceOutOfMemory,
            wgpu::SurfaceError::Other => Self::SurfaceLost,
        }
    }
}
