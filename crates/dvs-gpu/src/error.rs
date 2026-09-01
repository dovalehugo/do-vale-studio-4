//! Typed GPU initialization and fence-timeline errors.

use thiserror::Error;

/// Error returned by GPU context initialization and fence timeline operations.
#[derive(Debug, Error)]
pub enum GpuError {
    /// Surface creation from a window/display target failed.
    #[error("failed to create wgpu surface")]
    SurfaceCreation(#[from] wgpu::CreateSurfaceError),

    /// No adapter compatible with the surface and required backend was found.
    #[error("no compatible GPU adapter found for the surface")]
    NoCompatibleAdapter(#[from] wgpu::RequestAdapterError),

    /// The selected adapter uses an unsupported backend for the initial Windows slice.
    #[error("unsupported GPU backend for the initial Windows slice")]
    UnsupportedBackend,

    /// A CPU, software, or Microsoft Basic Render Driver adapter was rejected.
    #[error("CPU or fallback adapter rejected")]
    CpuOrFallbackAdapterRejected,

    /// A required device feature is missing on the selected adapter.
    #[error("required GPU feature missing: TEXTURE_FORMAT_NV12")]
    RequiredFeatureMissing,

    /// Device or queue creation failed.
    #[error("failed to create GPU device")]
    DeviceRequest(#[from] wgpu::RequestDeviceError),

    /// Surface configuration used an invalid size.
    #[error("invalid surface size")]
    InvalidSurfaceSize,

    /// Fence timeline values would overflow `u64`.
    #[error("fence timeline exhausted")]
    TimelineExhausted,
}
