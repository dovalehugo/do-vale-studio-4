//! Typed GPU initialization and fence-timeline errors.

use thiserror::Error;

use crate::luid::DxgiAdapterLuid;

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

    /// The wgpu device does not expose a DX12 HAL backend device.
    #[error("DX12 HAL device unavailable from wgpu device")]
    HalDx12DeviceUnavailable,

    /// DXGI adapter LUID could not be read from the DX12 device.
    #[error("DXGI adapter LUID unavailable")]
    DxgiAdapterLuidUnavailable,

    /// Adapter LUID values do not match the required physical adapter.
    #[error("adapter LUID mismatch: expected {expected}, actual {actual}")]
    AdapterLuidMismatch {
        /// Expected LUID from wgpu bootstrap.
        expected: DxgiAdapterLuid,
        /// Actual LUID from the comparison source.
        actual: DxgiAdapterLuid,
    },

    /// Shared NV12 allocation dimensions are invalid.
    #[error("invalid shared NV12 texture dimensions")]
    InvalidSharedTextureDimensions,

    /// NV12 allocation width and height must both be even.
    #[error("NV12 allocation dimensions must be even")]
    Nv12DimensionsMustBeEven,

    /// A required D3D11 interface is unavailable.
    #[cfg(target_os = "windows")]
    #[error("D3D11 interface unavailable: {interface_name}")]
    D3d11InterfaceUnavailable { interface_name: &'static str },

    /// DXGI adapter query failed on the D3D11 device.
    #[cfg(target_os = "windows")]
    #[error("D3D11 adapter query failed")]
    D3d11AdapterQueryFailed(#[source] windows::core::Error),

    /// Decoder source texture format is not NV12.
    #[error("decoder texture format is not DXGI_FORMAT_NV12")]
    InvalidDecoderTextureFormat,

    /// Decoder source texture dimensions do not match the shared allocation.
    #[error("decoder texture dimensions do not match shared allocation")]
    DecoderTextureDimensionsMismatch,

    /// Decoder texture array slice is out of bounds.
    #[error("decoder texture array slice is out of bounds")]
    DecoderTextureArraySliceOutOfBounds,

    /// Decoder texture layout is unsupported for copy.
    #[error("decoder texture layout is unsupported")]
    DecoderTextureUnsupportedLayout,

    /// Shareable NV12 texture creation failed.
    #[cfg(target_os = "windows")]
    #[error("shared NV12 texture creation failed")]
    SharedNv12TextureCreationFailed(#[source] windows::core::Error),

    /// Shared texture NT handle creation failed.
    #[cfg(target_os = "windows")]
    #[error("shared texture NT handle creation failed")]
    SharedTextureHandleCreationFailed(#[source] windows::core::Error),

    /// Shared D3D11 fence creation failed.
    #[cfg(target_os = "windows")]
    #[error("shared D3D11 fence creation failed")]
    SharedFenceCreationFailed(#[source] windows::core::Error),

    /// Shared fence NT handle creation failed.
    #[cfg(target_os = "windows")]
    #[error("shared fence NT handle creation failed")]
    SharedFenceHandleCreationFailed(#[source] windows::core::Error),

    /// Keyed mutex is unavailable on the shareable texture.
    #[cfg(target_os = "windows")]
    #[error("IDXGIKeyedMutex unavailable on shareable texture")]
    KeyedMutexUnavailable(#[source] windows::core::Error),

    /// Keyed mutex acquire timed out.
    #[cfg(target_os = "windows")]
    #[error("IDXGIKeyedMutex::AcquireSync timed out")]
    KeyedMutexAcquireTimeout,

    /// Keyed mutex was abandoned during acquire.
    #[cfg(target_os = "windows")]
    #[error("IDXGIKeyedMutex::AcquireSync abandoned")]
    KeyedMutexAbandoned,

    /// Keyed mutex acquire failed.
    #[cfg(target_os = "windows")]
    #[error("IDXGIKeyedMutex::AcquireSync failed")]
    KeyedMutexAcquireFailed(#[source] windows::core::Error),

    /// Keyed mutex release failed.
    #[cfg(target_os = "windows")]
    #[error("IDXGIKeyedMutex::ReleaseSync failed")]
    KeyedMutexReleaseFailed(#[source] windows::core::Error),

    /// D3D11 fence wait failed.
    #[cfg(target_os = "windows")]
    #[error("ID3D11DeviceContext4::Wait failed")]
    D3d11FenceWaitFailed(#[source] windows::core::Error),

    /// D3D11 fence signal failed.
    #[cfg(target_os = "windows")]
    #[error("ID3D11DeviceContext4::Signal failed")]
    D3d11FenceSignalFailed(#[source] windows::core::Error),

    /// GPU copy submission failed.
    #[cfg(target_os = "windows")]
    #[error("D3D11 CopySubresourceRegion failed")]
    D3d11CopySubresourceFailed(#[source] windows::core::Error),

    /// `GpuContext` does not contain a captured DXGI adapter LUID.
    #[error("GpuContext is missing DXGI adapter LUID")]
    MissingContextDxgiLuid,

    /// D3D12 `OpenSharedHandle` failed for the shared NV12 texture.
    #[cfg(target_os = "windows")]
    #[error("D3D12 texture OpenSharedHandle failed")]
    D3d12TextureOpenFailed(#[source] windows::core::Error),

    /// D3D12 `OpenSharedHandle` failed for the shared fence.
    #[cfg(target_os = "windows")]
    #[error("D3D12 fence OpenSharedHandle failed")]
    D3d12FenceOpenFailed(#[source] windows::core::Error),

    /// Imported D3D12 resource descriptor does not match the producer allocation.
    #[error("imported D3D12 resource descriptor mismatch")]
    ImportedResourceDescriptorMismatch,

    /// wgpu-hal failed to wrap the imported D3D12 resource.
    #[error("wgpu-hal DX12 texture wrap failed")]
    HalTextureWrapFailed,

    /// wgpu failed to create an external texture from the HAL wrapper.
    #[error("wgpu external texture creation failed")]
    WgpuExternalTextureCreationFailed,

    /// wgpu raw queue `Wait` failed.
    #[cfg(target_os = "windows")]
    #[error("wgpu raw queue Wait failed")]
    WgpuRawQueueWaitFailed(#[source] windows::core::Error),

    /// wgpu raw queue `Signal` failed.
    #[cfg(target_os = "windows")]
    #[error("wgpu raw queue Signal failed")]
    WgpuRawQueueSignalFailed(#[source] windows::core::Error),

    /// A frame is already prepared and awaiting consumed signal.
    #[error("interop bridge already has a prepared frame")]
    InteropFrameAlreadyPrepared,

    /// No prepared frame is available to signal consumed.
    #[error("interop bridge has no prepared frame")]
    InteropNoPreparedFrame,

    /// Consumed signal values do not match the prepared frame.
    #[error("interop fence values mismatch")]
    InteropFenceValuesMismatch,

    /// Interop bridge is poisoned after a synchronization failure.
    #[error("interop bridge is poisoned")]
    InteropBridgePoisoned,

    /// A shared handle was opened more than once per bridge lifetime.
    #[error("shared handle opened more than once")]
    SharedHandleOpenedMoreThanOnce,
}
