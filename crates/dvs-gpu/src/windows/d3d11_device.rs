//! D3D11 adapter LUID extraction from DXGI.

#![allow(unsafe_code)]

use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::core::Interface;

use windows::Win32::Graphics::Direct3D11::ID3D11Device;

use crate::error::GpuError;
use crate::luid::DxgiAdapterLuid;

/// Extracts the DXGI adapter LUID from a D3D11 device.
pub(crate) fn extract_d3d11_adapter_luid(
    device: &ID3D11Device,
) -> Result<DxgiAdapterLuid, GpuError> {
    let dxgi_device: IDXGIDevice = device.cast().map_err(GpuError::D3d11AdapterQueryFailed)?;

    // SAFETY: `device` is a live COM object; DXGI adapter queries are read-only metadata.
    unsafe {
        let adapter: IDXGIAdapter = dxgi_device
            .GetAdapter()
            .map_err(GpuError::D3d11AdapterQueryFailed)?;

        let desc = adapter
            .GetDesc()
            .map_err(GpuError::D3d11AdapterQueryFailed)?;

        Ok(DxgiAdapterLuid::new(
            desc.AdapterLuid.LowPart,
            desc.AdapterLuid.HighPart,
        ))
    }
}
