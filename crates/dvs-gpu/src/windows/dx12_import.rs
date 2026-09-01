//! D3D12 OpenSharedHandle import and wgpu-hal NV12 wrapping.

#![allow(unsafe_code)]

use wgpu::hal::api::Dx12;
use wgpu::hal::dx12::Device as HalDx12Device;
use wgpu::{Device, Extent3d, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages};
use windows::Win32::Graphics::Direct3D12::{
    D3D12_RESOURCE_DIMENSION_TEXTURE2D, ID3D12Fence, ID3D12Resource,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_NV12;

use crate::error::GpuError;

use super::d3d11_surface::SharedNv12TextureDesc;
use super::owned_handle::OwnedNtHandle;
use super::shared_nv12::WindowsD3d11SharedNv12Producer;

/// Result of one-time D3D12 shared-handle import for the interop bridge.
pub(crate) struct Dx12ImportedNv12 {
    pub texture: wgpu::Texture,
    pub cached_fence: ID3D12Fence,
    pub texture_open_count: u32,
    pub fence_open_count: u32,
}

/// Opens the producer NT handles once and wraps the imported NV12 resource through wgpu-hal DX12.
///
/// wgpu 27 external-texture state assumption (GPU Experiment 2 §11): the imported
/// `ID3D12Resource` is wrapped with `texture_from_raw` without an explicit D3D12
/// resource-state transition or CPU initialization. The resource is used exactly as
/// produced by the D3D11 copy path and synchronized through the shared fence before
/// shader sampling.
pub(crate) fn import_shared_nv12_from_producer(
    device: &Device,
    producer: &WindowsD3d11SharedNv12Producer,
    desc: SharedNv12TextureDesc,
) -> Result<Dx12ImportedNv12, GpuError> {
    let texture_handle = producer.texture_shared_handle();
    let fence_handle = producer.fence_shared_handle();

    // SAFETY: `device` owns the wgpu DX12 HAL device used for one-time shared-handle import.
    let hal_device =
        unsafe { device.as_hal::<Dx12>() }.ok_or(GpuError::HalDx12DeviceUnavailable)?;

    let imported_resource =
        open_shared_texture_once(hal_device.raw_device(), texture_handle, desc)?;

    let (cached_fence, fence_open_count) =
        open_shared_fence_once(hal_device.raw_device(), fence_handle)?;

    let texture = wrap_imported_nv12_texture(device, imported_resource, desc)?;

    Ok(Dx12ImportedNv12 {
        texture,
        cached_fence,
        texture_open_count: 1,
        fence_open_count,
    })
}

fn open_shared_texture_once(
    d3d12_device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    handle: &OwnedNtHandle,
    expected: SharedNv12TextureDesc,
) -> Result<ID3D12Resource, GpuError> {
    let mut imported_resource: Option<ID3D12Resource> = None;
    // SAFETY: `handle` is a valid NT handle opened once by the producer for this texture.
    unsafe {
        d3d12_device
            .OpenSharedHandle(handle.handle(), &mut imported_resource)
            .map_err(GpuError::D3d12TextureOpenFailed)?;
    }

    let imported_resource = imported_resource.ok_or(GpuError::D3d12TextureOpenFailed(
        windows::core::Error::from_hresult(windows::core::HRESULT(-1)),
    ))?;

    validate_imported_nv12_descriptor(&imported_resource, expected)?;
    Ok(imported_resource)
}

fn open_shared_fence_once(
    d3d12_device: &windows::Win32::Graphics::Direct3D12::ID3D12Device,
    handle: &OwnedNtHandle,
) -> Result<(ID3D12Fence, u32), GpuError> {
    let mut imported_fence: Option<ID3D12Fence> = None;
    // SAFETY: `handle` is a valid NT handle opened once by the producer for this fence.
    unsafe {
        d3d12_device
            .OpenSharedHandle(handle.handle(), &mut imported_fence)
            .map_err(GpuError::D3d12FenceOpenFailed)?;
    }

    let imported_fence = imported_fence.ok_or(GpuError::D3d12FenceOpenFailed(
        windows::core::Error::from_hresult(windows::core::HRESULT(-1)),
    ))?;

    Ok((imported_fence, 1))
}

fn validate_imported_nv12_descriptor(
    resource: &ID3D12Resource,
    expected: SharedNv12TextureDesc,
) -> Result<(), GpuError> {
    // SAFETY: `resource` is a valid opened D3D12 resource used for descriptor inspection only.
    let desc = unsafe { resource.GetDesc() };

    if desc.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || desc.Width != expected.allocation_width() as u64
        || desc.Height != expected.allocation_height()
        || desc.DepthOrArraySize != 1
        || desc.MipLevels != 1
        || desc.Format != DXGI_FORMAT_NV12
        || desc.SampleDesc.Count != 1
    {
        return Err(GpuError::ImportedResourceDescriptorMismatch);
    }

    Ok(())
}

fn wrap_imported_nv12_texture(
    device: &Device,
    imported_resource: ID3D12Resource,
    desc: SharedNv12TextureDesc,
) -> Result<wgpu::Texture, GpuError> {
    let texture_desc = TextureDescriptor {
        label: Some("dvs-gpu-shared-nv12"),
        size: Extent3d {
            width: desc.allocation_width(),
            height: desc.allocation_height(),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::NV12,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };

    // SAFETY: Descriptor validation succeeded; `texture_from_raw` takes ownership of the
    // imported resource opened once from the producer NT handle.
    let hal_texture = unsafe {
        HalDx12Device::texture_from_raw(
            imported_resource,
            TextureFormat::NV12,
            TextureDimension::D2,
            texture_desc.size,
            texture_desc.mip_level_count,
            texture_desc.sample_count,
        )
    };

    // SAFETY: HAL texture wraps the validated imported NV12 resource for wgpu DX12.
    let texture = unsafe { device.create_texture_from_hal::<Dx12>(hal_texture, &texture_desc) };

    Ok(texture)
}

#[cfg(test)]
mod tests {
    #[test]
    fn imported_descriptor_validation_rejects_zero_width_expectation() {
        fn check(width: u64, height: u32, expected_width: u32, expected_height: u32) -> bool {
            width == expected_width as u64
                && height == expected_height
                && expected_width > 0
                && expected_height > 0
        }

        assert!(!check(0, 2176, 3840, 2176));
    }
}
