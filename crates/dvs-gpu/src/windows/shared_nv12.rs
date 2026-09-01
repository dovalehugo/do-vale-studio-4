//! Shareable NV12 D3D11 producer resources and per-frame GPU copy.

#![allow(unsafe_code)]
#![allow(dead_code)] // crate-private 3C bridge accessors retained until Integration 3C

use windows::Win32::Foundation::GENERIC_ALL;
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_FENCE_FLAG_SHARED, D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX,
    D3D11_RESOURCE_MISC_SHARED_NTHANDLE, D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, ID3D11Device,
    ID3D11Device5, ID3D11DeviceContext, ID3D11DeviceContext4, ID3D11Fence, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{
    DXGI_ERROR_WAIT_TIMEOUT, DXGI_SHARED_RESOURCE_READ, DXGI_SHARED_RESOURCE_WRITE,
    IDXGIKeyedMutex, IDXGIResource1,
};
use windows::core::{Interface, PCWSTR};

/// `DXGI_ERROR_WAIT_ABANDONED` (0x887A0026) — not exported by windows 0.58 without extra features.
const DXGI_ERROR_WAIT_ABANDONED_CODE: i32 = -2005270490;

use crate::error::GpuError;
use crate::fence_timeline::FrameFenceValues;
use crate::luid::{DxgiAdapterLuid, validate_same_adapter};

use super::d3d11_device::extract_d3d11_adapter_luid;
use super::d3d11_surface::{D3d11DecodedSurfaceRef, SharedNv12TextureDesc};
use super::owned_handle::OwnedNtHandle;

/// D3D11 producer for a single shared NV12 texture and shared fence.
///
/// Creates shareable resources once and performs keyed-mutex guarded GPU copies each frame.
/// D3D12/wgpu import is performed by Integration 3C.
pub struct WindowsD3d11SharedNv12Producer {
    device5: ID3D11Device5,
    context4: ID3D11DeviceContext4,
    shareable_texture: ID3D11Texture2D,
    keyed_mutex: IDXGIKeyedMutex,
    texture_handle: OwnedNtHandle,
    fence: ID3D11Fence,
    fence_handle: OwnedNtHandle,
    desc: SharedNv12TextureDesc,
    adapter_luid: DxgiAdapterLuid,
}

impl WindowsD3d11SharedNv12Producer {
    /// Creates producer resources on the decoder D3D11 device after validating adapter LUID.
    pub fn new(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        expected_wgpu_luid: DxgiAdapterLuid,
        desc: SharedNv12TextureDesc,
    ) -> Result<Self, GpuError> {
        let actual_luid = extract_d3d11_adapter_luid(device)?;
        validate_same_adapter(expected_wgpu_luid, actual_luid)?;

        let device5: ID3D11Device5 =
            device
                .cast()
                .map_err(|_| GpuError::D3d11InterfaceUnavailable {
                    interface_name: "ID3D11Device5",
                })?;

        let context4: ID3D11DeviceContext4 =
            context
                .cast()
                .map_err(|_| GpuError::D3d11InterfaceUnavailable {
                    interface_name: "ID3D11DeviceContext4",
                })?;

        let shareable_texture = create_shareable_nv12_texture(device, desc)?;
        let keyed_mutex = query_keyed_mutex(&shareable_texture)?;
        let texture_handle = create_texture_shared_handle(&shareable_texture)?;

        let (fence, fence_handle) = create_shared_fence(&device5)?;

        Ok(Self {
            device5,
            context4,
            shareable_texture,
            keyed_mutex,
            texture_handle,
            fence,
            fence_handle,
            desc,
            adapter_luid: actual_luid,
        })
    }

    /// Returns the validated D3D11 adapter LUID for this producer device.
    pub fn adapter_luid(&self) -> DxgiAdapterLuid {
        self.adapter_luid
    }

    /// Returns the shareable texture allocation descriptor.
    pub fn desc(&self) -> SharedNv12TextureDesc {
        self.desc
    }

    pub(crate) fn texture_shared_handle(&self) -> &OwnedNtHandle {
        &self.texture_handle
    }

    pub(crate) fn fence_shared_handle(&self) -> &OwnedNtHandle {
        &self.fence_handle
    }

    pub(crate) fn shareable_texture(&self) -> &ID3D11Texture2D {
        &self.shareable_texture
    }

    pub(crate) fn fence(&self) -> &ID3D11Fence {
        &self.fence
    }

    /// Performs the D3D11 producer half of one frame on the shared NV12 texture.
    pub fn produce_frame(
        &mut self,
        frame: D3d11DecodedSurfaceRef<'_>,
        fence_values: FrameFenceValues,
    ) -> Result<(), GpuError> {
        let source_subresource =
            frame.validate_for_copy(self.desc.allocation_width(), self.desc.allocation_height())?;

        if let Some(wait_value) = fence_values.wait_consumed() {
            // SAFETY: `context4` and `fence` are live COM objects owned by this producer.
            unsafe {
                self.context4
                    .Wait(&self.fence, wait_value)
                    .map_err(GpuError::D3d11FenceWaitFailed)?;
            }
        }

        acquire_keyed_mutex(&self.keyed_mutex)?;

        let copy_result = copy_decoder_to_shareable(
            &self.context4,
            frame.texture(),
            source_subresource,
            &self.shareable_texture,
        );

        // SAFETY: Mutex was acquired successfully; release must follow every acquire.
        let release_result = unsafe { self.keyed_mutex.ReleaseSync(0) };

        match copy_result {
            Err(copy_err) => {
                if let Err(release_err) = release_result {
                    return Err(map_keyed_mutex_release_error(release_err));
                }
                return Err(copy_err);
            }
            Ok(()) => {
                release_result.map_err(map_keyed_mutex_release_error)?;
            }
        }

        // SAFETY: `context4` and `fence` are live COM objects owned by this producer.
        unsafe {
            self.context4
                .Signal(&self.fence, fence_values.ready())
                .map_err(GpuError::D3d11FenceSignalFailed)?;
        }

        Ok(())
    }
}

fn create_shareable_nv12_texture(
    device: &ID3D11Device,
    desc: SharedNv12TextureDesc,
) -> Result<ID3D11Texture2D, GpuError> {
    let create_desc = D3D11_TEXTURE2D_DESC {
        Width: desc.allocation_width(),
        Height: desc.allocation_height(),
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: (D3D11_RESOURCE_MISC_SHARED_NTHANDLE | D3D11_RESOURCE_MISC_SHARED_KEYEDMUTEX).0
            as u32,
    };

    let mut shareable: Option<ID3D11Texture2D> = None;
    // SAFETY: `device` is valid; `create_desc` matches validated NV12 allocation dimensions.
    unsafe {
        device
            .CreateTexture2D(&create_desc, None, Some(&mut shareable))
            .map_err(GpuError::SharedNv12TextureCreationFailed)?;
    }

    shareable.ok_or(GpuError::SharedNv12TextureCreationFailed(
        windows::core::Error::from_hresult(windows::core::HRESULT(-1)),
    ))
}

fn query_keyed_mutex(texture: &ID3D11Texture2D) -> Result<IDXGIKeyedMutex, GpuError> {
    texture.cast().map_err(GpuError::KeyedMutexUnavailable)
}

fn create_texture_shared_handle(texture: &ID3D11Texture2D) -> Result<OwnedNtHandle, GpuError> {
    let dxgi_resource: IDXGIResource1 = texture
        .cast()
        .map_err(GpuError::SharedTextureHandleCreationFailed)?;

    let access = (DXGI_SHARED_RESOURCE_READ | DXGI_SHARED_RESOURCE_WRITE).0;
    // SAFETY: `dxgi_resource` wraps the shareable texture created on this device.
    let handle = unsafe {
        dxgi_resource
            .CreateSharedHandle(None, access, PCWSTR::null())
            .map_err(GpuError::SharedTextureHandleCreationFailed)?
    };

    OwnedNtHandle::new_texture(handle)
}

fn create_shared_fence(device5: &ID3D11Device5) -> Result<(ID3D11Fence, OwnedNtHandle), GpuError> {
    let mut fence: Option<ID3D11Fence> = None;
    // SAFETY: `device5` is a valid D3D11.5 device obtained from the decoder device.
    unsafe {
        device5
            .CreateFence(0, D3D11_FENCE_FLAG_SHARED, &mut fence)
            .map_err(GpuError::SharedFenceCreationFailed)?;
    }

    let fence = fence.ok_or(GpuError::SharedFenceCreationFailed(
        windows::core::Error::from_hresult(windows::core::HRESULT(-1)),
    ))?;

    // SAFETY: `fence` is a valid shared D3D11 fence owned by this producer.
    let handle = unsafe {
        fence
            .CreateSharedHandle(None, GENERIC_ALL.0, PCWSTR::null())
            .map_err(GpuError::SharedFenceHandleCreationFailed)?
    };

    Ok((fence, OwnedNtHandle::new_fence(handle)?))
}

fn acquire_keyed_mutex(mutex: &IDXGIKeyedMutex) -> Result<(), GpuError> {
    // SAFETY: `mutex` is the keyed mutex on the shareable texture owned by this producer.
    match unsafe { mutex.AcquireSync(0, 5000) } {
        Ok(()) => Ok(()),
        Err(source) => {
            if source.code() == DXGI_ERROR_WAIT_TIMEOUT {
                Err(GpuError::KeyedMutexAcquireTimeout)
            } else if source.code().0 == DXGI_ERROR_WAIT_ABANDONED_CODE {
                Err(GpuError::KeyedMutexAbandoned)
            } else {
                Err(GpuError::KeyedMutexAcquireFailed(source))
            }
        }
    }
}

fn map_keyed_mutex_release_error(source: windows::core::Error) -> GpuError {
    GpuError::KeyedMutexReleaseFailed(source)
}

fn copy_decoder_to_shareable(
    context: &ID3D11DeviceContext4,
    source: &ID3D11Texture2D,
    source_subresource: u32,
    destination: &ID3D11Texture2D,
) -> Result<(), GpuError> {
    // SAFETY: Source/destination textures and subresource indices were validated before copy.
    unsafe {
        context.CopySubresourceRegion(destination, 0, 0, 0, 0, source, source_subresource, None);
        context.Flush();
    }
    Ok(())
}
