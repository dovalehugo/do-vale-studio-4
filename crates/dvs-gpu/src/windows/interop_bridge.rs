//! Windows D3D11 producer + wgpu DX12 consumer interop bridge.

use windows::Win32::Graphics::Direct3D12::ID3D12Fence;

use crate::context::GpuContext;
use crate::error::GpuError;
use crate::fence_timeline::FrameFenceValues;
use crate::gpu_video_frame::GpuVideoFrame;
use crate::luid::validate_same_adapter;

use super::d3d11_surface::D3d11DecodedSurfaceRef;
use super::dx12_import::import_shared_nv12_from_producer;
use super::dx12_queue_sync::{signal_consumed_on_wgpu_queue, wait_ready_on_wgpu_queue};
use super::shared_nv12::WindowsD3d11SharedNv12Producer;

/// Per-frame bridge state for the single shared NV12 texture.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BridgeFrameState {
    Idle,
    AwaitingConsumed(FrameFenceValues),
    Poisoned,
}

/// Owns the D3D11 producer, imported D3D12 resources, and wgpu NV12 texture.
///
/// Field order is deliberate: `producer` is dropped last so NT shared handles remain
/// valid for the lifetime of the opened D3D12 objects and wgpu texture wrapper.
pub struct WindowsD3d11WgpuInteropBridge {
    producer: WindowsD3d11SharedNv12Producer,
    cached_fence: ID3D12Fence,
    video_frame: GpuVideoFrame,
    frame_state: BridgeFrameState,
    texture_open_count: u32,
    fence_open_count: u32,
}

impl WindowsD3d11WgpuInteropBridge {
    /// Creates the bridge by opening producer NT handles once and importing into wgpu DX12.
    pub fn new(
        context: &GpuContext,
        producer: WindowsD3d11SharedNv12Producer,
    ) -> Result<Self, GpuError> {
        let context_luid = context
            .adapter_identity()
            .dxgi_luid()
            .ok_or(GpuError::MissingContextDxgiLuid)?;

        validate_same_adapter(context_luid, producer.adapter_luid())?;

        let desc = producer.desc();
        let imported = import_shared_nv12_from_producer(context.device(), &producer, desc)?;

        if imported.texture_open_count != 1 || imported.fence_open_count != 1 {
            return Err(GpuError::SharedHandleOpenedMoreThanOnce);
        }

        let video_frame = GpuVideoFrame::new(
            desc.allocation_width(),
            desc.allocation_height(),
            imported.texture,
        );

        Ok(Self {
            producer,
            cached_fence: imported.cached_fence,
            video_frame,
            frame_state: BridgeFrameState::Idle,
            texture_open_count: imported.texture_open_count,
            fence_open_count: imported.fence_open_count,
        })
    }

    /// Returns how many times texture and fence shared handles were opened (must be 1 each).
    pub fn shared_handle_open_counts(&self) -> (u32, u32) {
        (self.texture_open_count, self.fence_open_count)
    }

    /// Producer half + wgpu `Wait(ready)` for one frame.
    ///
    /// Returns a borrowed [`GpuVideoFrame`] valid until [`Self::signal_consumed_after_submit`].
    pub fn prepare_frame(
        &mut self,
        context: &GpuContext,
        source: D3d11DecodedSurfaceRef<'_>,
        values: FrameFenceValues,
    ) -> Result<&GpuVideoFrame, GpuError> {
        match self.frame_state {
            BridgeFrameState::Idle => {}
            BridgeFrameState::Poisoned => return Err(GpuError::InteropBridgePoisoned),
            BridgeFrameState::AwaitingConsumed(_) => {
                return Err(GpuError::InteropFrameAlreadyPrepared);
            }
        }

        if let Err(error) = self.producer.produce_frame(source, values) {
            self.frame_state = BridgeFrameState::Poisoned;
            return Err(error);
        }

        if let Err(error) = wait_ready_on_wgpu_queue(context, &self.cached_fence, values.ready()) {
            self.frame_state = BridgeFrameState::Poisoned;
            return Err(error);
        }

        self.frame_state = BridgeFrameState::AwaitingConsumed(values);
        Ok(&self.video_frame)
    }

    /// Returns the imported frame prepared by [`Self::prepare_frame`] while awaiting
    /// [`Self::signal_consumed_after_submit`].
    pub fn prepared_frame(&self) -> Result<&GpuVideoFrame, GpuError> {
        match self.frame_state {
            BridgeFrameState::AwaitingConsumed(_) => Ok(&self.video_frame),
            BridgeFrameState::Idle => Err(GpuError::InteropNoPreparedFrame),
            BridgeFrameState::Poisoned => Err(GpuError::InteropBridgePoisoned),
        }
    }

    /// Releases a prepared frame without rendering by submitting an empty queue batch
    /// and signalling `consumed`.
    ///
    /// Use when presentation cannot proceed but the bridge slot must be freed in-order.
    pub fn discard_prepared_after_submit(
        &mut self,
        context: &GpuContext,
        values: FrameFenceValues,
    ) -> Result<(), GpuError> {
        context.queue().submit(std::iter::empty());
        self.signal_consumed_after_submit(context, values)
    }

    /// Signals `consumed` on wgpu's raw queue after the frame's GPU work was submitted.
    ///
    /// Callers must invoke [`GpuContext::queue`].`submit(...)` before this method.
    pub fn signal_consumed_after_submit(
        &mut self,
        context: &GpuContext,
        values: FrameFenceValues,
    ) -> Result<(), GpuError> {
        match self.frame_state {
            BridgeFrameState::AwaitingConsumed(prepared) => {
                if prepared != values {
                    return Err(GpuError::InteropFenceValuesMismatch);
                }
            }
            BridgeFrameState::Idle => return Err(GpuError::InteropNoPreparedFrame),
            BridgeFrameState::Poisoned => return Err(GpuError::InteropBridgePoisoned),
        }

        if let Err(error) =
            signal_consumed_on_wgpu_queue(context, &self.cached_fence, values.consumed())
        {
            self.frame_state = BridgeFrameState::Poisoned;
            return Err(error);
        }

        self.frame_state = BridgeFrameState::Idle;
        Ok(())
    }
}
