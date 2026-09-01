//! Audited DXGI adapter LUID extraction from wgpu's DX12 HAL device.

#![allow(unsafe_code)]

use wgpu::Device;
use wgpu::hal::api::Dx12;

use crate::error::GpuError;
use crate::luid::DxgiAdapterLuid;

/// Extracts the exact DXGI adapter LUID from the wgpu DX12 device created at bootstrap.
pub(crate) fn extract_dxgi_adapter_luid(device: &Device) -> Result<DxgiAdapterLuid, GpuError> {
    // SAFETY: `as_hal` reads the live DX12 HAL device owned by `device`. The returned
    // reference is not retained beyond this function and is only used to query LUID.
    let hal_device =
        unsafe { device.as_hal::<Dx12>() }.ok_or(GpuError::HalDx12DeviceUnavailable)?;

    let raw_device = hal_device.raw_device();

    // SAFETY: `raw_device` is a valid `ID3D12Device` obtained from wgpu-hal for the
    // duration of the `hal_device` borrow. `GetAdapterLuid` is a read-only query.
    let luid = unsafe { raw_device.GetAdapterLuid() };

    Ok(DxgiAdapterLuid::new(luid.LowPart, luid.HighPart))
}
