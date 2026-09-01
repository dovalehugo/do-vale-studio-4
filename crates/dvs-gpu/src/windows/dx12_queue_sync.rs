//! wgpu raw/present queue fence synchronization for the interop bridge.

#![allow(unsafe_code)]

use wgpu::hal::api::Dx12;
use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

use crate::context::GpuContext;
use crate::error::GpuError;

/// GPU-side `Wait(ready)` on wgpu's exact raw/present queue.
pub(crate) fn wait_ready_on_wgpu_queue(
    context: &GpuContext,
    fence: &ID3D12Fence,
    ready: u64,
) -> Result<(), GpuError> {
    // SAFETY: `context.device()` owns the wgpu DX12 HAL device and raw present queue.
    let hal_device =
        unsafe { context.device().as_hal::<Dx12>() }.ok_or(GpuError::HalDx12DeviceUnavailable)?;

    // SAFETY: `fence` is the cached shared D3D12 fence opened once at bridge creation.
    unsafe {
        hal_device
            .raw_queue()
            .Wait(fence, ready)
            .map_err(GpuError::WgpuRawQueueWaitFailed)?;
    }

    Ok(())
}

/// GPU-side `Signal(consumed)` on wgpu's exact raw/present queue.
///
/// Must be called only after `GpuContext::queue().submit(...)` for the frame.
pub(crate) fn signal_consumed_on_wgpu_queue(
    context: &GpuContext,
    fence: &ID3D12Fence,
    consumed: u64,
) -> Result<(), GpuError> {
    // SAFETY: `context.device()` owns the wgpu DX12 HAL device and raw present queue.
    let hal_device =
        unsafe { context.device().as_hal::<Dx12>() }.ok_or(GpuError::HalDx12DeviceUnavailable)?;

    // SAFETY: `fence` is the cached shared D3D12 fence opened once at bridge creation.
    unsafe {
        hal_device
            .raw_queue()
            .Signal(fence, consumed)
            .map_err(GpuError::WgpuRawQueueSignalFailed)?;
    }

    Ok(())
}
