//! GPU context foundation for Do Vale Studio 4.
//!
//! Integration 2 provides safe DX12 bootstrap, public adapter identity, typed errors,
//! and monotonic fence value generation. Integration 3A adds exact DXGI adapter LUID
//! extraction through an audited Windows HAL boundary.

#![deny(unsafe_code)]

mod adapter;
mod context;
mod error;
mod fence_timeline;
mod luid;
#[cfg(target_os = "windows")]
mod windows;

pub use adapter::{AdapterIdentity, GpuBackend, GpuDeviceType, REQUIRED_DEVICE_FEATURES};
pub use context::{GpuBootstrap, GpuContext, SurfaceWindowTarget};
pub use error::GpuError;
pub use fence_timeline::{FenceTimeline, FrameFenceValues};
pub use luid::{DxgiAdapterLuid, validate_same_adapter};

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
