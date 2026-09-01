//! Windows-only HAL boundary modules for DXGI and D3D interop.

#[cfg(target_os = "windows")]
mod dxgi_luid;

#[cfg(target_os = "windows")]
pub(crate) use dxgi_luid::extract_dxgi_adapter_luid;
