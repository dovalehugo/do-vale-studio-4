//! Windows-only HAL boundary modules for DXGI and D3D interop.

#[cfg(target_os = "windows")]
mod d3d11_device;
#[cfg(target_os = "windows")]
mod d3d11_surface;
#[cfg(target_os = "windows")]
mod dxgi_luid;
#[cfg(target_os = "windows")]
mod owned_handle;
#[cfg(target_os = "windows")]
mod shared_nv12;

#[cfg(target_os = "windows")]
pub(crate) use dxgi_luid::extract_dxgi_adapter_luid;

#[cfg(target_os = "windows")]
pub use d3d11_surface::{D3d11DecodedSurfaceRef, SharedNv12TextureDesc};
#[cfg(target_os = "windows")]
pub use shared_nv12::WindowsD3d11SharedNv12Producer;
