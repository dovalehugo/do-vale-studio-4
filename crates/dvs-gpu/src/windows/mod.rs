//! Windows-only HAL boundary modules for DXGI and D3D interop.

#[cfg(target_os = "windows")]
mod d3d11_device;
#[cfg(target_os = "windows")]
mod d3d11_lock;
#[cfg(target_os = "windows")]
mod d3d11_surface;
#[cfg(target_os = "windows")]
mod dx12_import;
#[cfg(target_os = "windows")]
mod dx12_queue_sync;
#[cfg(target_os = "windows")]
mod dxgi_luid;
#[cfg(target_os = "windows")]
mod interop_bridge;
#[cfg(target_os = "windows")]
mod owned_handle;
#[cfg(target_os = "windows")]
mod shared_nv12;

#[cfg(target_os = "windows")]
pub(crate) use dxgi_luid::extract_dxgi_adapter_luid;

#[cfg(target_os = "windows")]
pub use d3d11_lock::{
    D3d11ExternalContextLock, D3d11ExternalContextLockConfigError,
    D3d11ExternalContextLockKeepalive,
};
#[cfg(target_os = "windows")]
pub use d3d11_surface::{D3d11DecodedSurfaceRef, SharedNv12TextureDesc};
#[cfg(target_os = "windows")]
pub use interop_bridge::WindowsD3d11WgpuInteropBridge;
#[cfg(target_os = "windows")]
pub use shared_nv12::WindowsD3d11SharedNv12Producer;
