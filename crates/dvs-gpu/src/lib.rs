//! GPU context foundation for Do Vale Studio 4.
//!
//! Integration 2 provides safe DX12 bootstrap, public adapter identity, typed errors,
//! and monotonic fence value generation. Integration 3A adds exact DXGI adapter LUID
//! extraction through an audited Windows HAL boundary. Integration 3B adds the D3D11
//! shared NV12 producer half of the interop bridge (Windows-only). Integration 3C adds
//! the D3D12/wgpu consumer half and bidirectional raw-queue fence synchronization.

#![deny(unsafe_code)]

mod adapter;
mod context;
mod error;
mod fence_timeline;
mod gpu_video_frame;
mod luid;
mod nv12_allocation;
#[cfg(target_os = "windows")]
mod windows;

pub use adapter::{AdapterIdentity, GpuBackend, GpuDeviceType, REQUIRED_DEVICE_FEATURES};
pub use context::{GpuBootstrap, GpuContext, SurfaceWindowTarget};
pub use error::GpuError;
pub use fence_timeline::{FenceTimeline, FrameFenceValues};
pub use gpu_video_frame::{GpuVideoFrame, GpuVideoPixelFormat};
pub use luid::{DxgiAdapterLuid, validate_same_adapter};

#[cfg(target_os = "windows")]
pub use windows::{
    D3d11DecodedSurfaceRef, SharedNv12TextureDesc, WindowsD3d11SharedNv12Producer,
    WindowsD3d11WgpuInteropBridge,
};

#[cfg(test)]
mod send_sync {
    use std::sync::{Arc, Mutex};

    use super::*;

    const fn assert_send_sync<T: Send + Sync>() {}

    const _: () = {
        assert_send_sync::<AdapterIdentity>();
        assert_send_sync::<GpuBackend>();
        assert_send_sync::<GpuDeviceType>();
        assert_send_sync::<DxgiAdapterLuid>();
        assert_send_sync::<FrameFenceValues>();
        assert_send_sync::<FenceTimeline>();
        assert_send_sync::<GpuError>();
    };

    #[test]
    fn adapter_identity_and_timeline_value_types_are_send_and_sync() {
        fn assert_values<T: Send + Sync>(value: T) {
            let _ = Arc::new(Mutex::new(value));
        }

        let info = wgpu::AdapterInfo {
            name: "test".to_string(),
            vendor: 1,
            device: 2,
            device_type: wgpu::DeviceType::DiscreteGpu,
            driver: "driver".to_string(),
            driver_info: "info".to_string(),
            backend: wgpu::Backend::Dx12,
        };
        let identity = crate::adapter::AdapterIdentity::from_adapter_info(&info).expect("identity");
        assert_values(identity);
        assert_values(DxgiAdapterLuid::new(1, 2));
        assert_values(FenceTimeline::new());
        assert_values(fence_timeline::fence_values_for_frame(0).expect("frame 0"));
    }
}

#[cfg(not(target_os = "windows"))]
mod platform_exports {
    #[test]
    fn windows_d3d11_producer_exports_are_cfg_gated() {
        let source = include_str!("lib.rs");
        assert!(source.contains("#[cfg(target_os = \"windows\")]"));
        assert!(source.contains("WindowsD3d11SharedNv12Producer"));
        assert!(source.contains("SharedNv12TextureDesc"));
        assert!(source.contains("D3d11DecodedSurfaceRef"));
    }
}
