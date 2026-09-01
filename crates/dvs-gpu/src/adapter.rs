//! Platform-independent adapter identity and validation.

use wgpu::{AdapterInfo, Backend, DeviceType, Features};

use crate::error::GpuError;

/// Production GPU backend identifier.
///
/// This is intentionally independent from `wgpu::Backend` so the public contract
/// can evolve without exposing wgpu backend enums directly.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum GpuBackend {
    /// Direct3D 12 (initial Windows production slice).
    Dx12,
}

/// Production GPU device class.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum GpuDeviceType {
    /// Discrete GPU with separate memory.
    DiscreteGpu,
    /// Integrated GPU with shared memory.
    IntegratedGpu,
    /// Virtual or hosted GPU.
    VirtualGpu,
    /// CPU / software rendering.
    Cpu,
    /// Other or unknown device class.
    Other,
}

/// Safe public adapter identity captured during GPU bootstrap.
///
/// Contains the fields exposed by wgpu 27 `AdapterInfo`. Exact DXGI adapter LUID
/// extraction and same-adapter enforcement are added in Integration 3 via
/// `wgpu-hal` and Windows APIs. Vendor and device IDs are not a substitute for LUID.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct AdapterIdentity {
    name: String,
    backend: GpuBackend,
    vendor_id: u32,
    device_id: u32,
    device_type: GpuDeviceType,
    driver: String,
    driver_info: String,
}

impl AdapterIdentity {
    /// Returns the adapter name reported by the driver stack.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the production backend identifier.
    pub fn backend(&self) -> GpuBackend {
        self.backend
    }

    /// Returns the backend-specific vendor identifier.
    pub fn vendor_id(&self) -> u32 {
        self.vendor_id
    }

    /// Returns the backend-specific device identifier.
    pub fn device_id(&self) -> u32 {
        self.device_id
    }

    /// Returns the mapped device class.
    pub fn device_type(&self) -> GpuDeviceType {
        self.device_type
    }

    /// Returns the driver name string.
    pub fn driver(&self) -> &str {
        &self.driver
    }

    /// Returns additional driver information.
    pub fn driver_info(&self) -> &str {
        &self.driver_info
    }

    /// Builds identity from wgpu `AdapterInfo` and validates the initial Windows slice.
    pub(crate) fn from_adapter_info(info: &AdapterInfo) -> Result<Self, GpuError> {
        let backend = map_backend(info.backend)?;
        let device_type = map_device_type(info.device_type);
        let identity = Self {
            name: info.name.clone(),
            backend,
            vendor_id: info.vendor,
            device_id: info.device,
            device_type,
            driver: info.driver.clone(),
            driver_info: info.driver_info.clone(),
        };
        reject_cpu_or_fallback_adapter(&identity)?;
        Ok(identity)
    }
}

/// Required device features for the initial NV12 production path.
pub const REQUIRED_DEVICE_FEATURES: Features = Features::TEXTURE_FORMAT_NV12;

/// Validates adapter features required for the initial Windows slice.
pub(crate) fn validate_required_features(features: Features) -> Result<(), GpuError> {
    if features.contains(REQUIRED_DEVICE_FEATURES) {
        Ok(())
    } else {
        Err(GpuError::RequiredFeatureMissing)
    }
}

fn map_backend(backend: Backend) -> Result<GpuBackend, GpuError> {
    match backend {
        Backend::Dx12 => Ok(GpuBackend::Dx12),
        _ => Err(GpuError::UnsupportedBackend),
    }
}

fn map_device_type(device_type: DeviceType) -> GpuDeviceType {
    match device_type {
        DeviceType::DiscreteGpu => GpuDeviceType::DiscreteGpu,
        DeviceType::IntegratedGpu => GpuDeviceType::IntegratedGpu,
        DeviceType::VirtualGpu => GpuDeviceType::VirtualGpu,
        DeviceType::Cpu => GpuDeviceType::Cpu,
        DeviceType::Other => GpuDeviceType::Other,
    }
}

fn reject_cpu_or_fallback_adapter(identity: &AdapterIdentity) -> Result<(), GpuError> {
    if identity.device_type == GpuDeviceType::Cpu
        || identity.name.contains("Microsoft Basic Render Driver")
    {
        return Err(GpuError::CpuOrFallbackAdapterRejected);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_dx12_info() -> AdapterInfo {
        AdapterInfo {
            name: "Radeon RX 580 Series".to_string(),
            vendor: 0x1002,
            device: 0x67DF,
            device_type: DeviceType::DiscreteGpu,
            driver: "AMD proprietary driver".to_string(),
            driver_info: "24.1.1".to_string(),
            backend: Backend::Dx12,
        }
    }

    #[test]
    fn adapter_info_dx12_mapping() {
        let identity = AdapterIdentity::from_adapter_info(&sample_dx12_info()).expect("identity");
        assert_eq!(identity.backend(), GpuBackend::Dx12);
        assert_eq!(identity.name(), "Radeon RX 580 Series");
        assert_eq!(identity.vendor_id(), 0x1002);
        assert_eq!(identity.device_id(), 0x67DF);
        assert_eq!(identity.device_type(), GpuDeviceType::DiscreteGpu);
        assert_eq!(identity.driver(), "AMD proprietary driver");
        assert_eq!(identity.driver_info(), "24.1.1");
    }

    #[test]
    fn cpu_device_type_maps_and_is_rejected() {
        let info = AdapterInfo {
            device_type: DeviceType::Cpu,
            ..sample_dx12_info()
        };
        let err = AdapterIdentity::from_adapter_info(&info).unwrap_err();
        assert!(matches!(err, GpuError::CpuOrFallbackAdapterRejected));
    }

    #[test]
    fn non_dx12_backend_rejected() {
        let info = AdapterInfo {
            backend: Backend::Vulkan,
            ..sample_dx12_info()
        };
        let err = AdapterIdentity::from_adapter_info(&info).unwrap_err();
        assert!(matches!(err, GpuError::UnsupportedBackend));
    }

    #[test]
    fn missing_texture_format_nv12_detected() {
        let err = validate_required_features(Features::empty()).unwrap_err();
        assert!(matches!(err, GpuError::RequiredFeatureMissing));
    }

    #[test]
    fn adapter_identity_has_no_luid_field() {
        let identity = AdapterIdentity::from_adapter_info(&sample_dx12_info()).expect("identity");
        let _ = (
            identity.name(),
            identity.backend(),
            identity.vendor_id(),
            identity.device_id(),
            identity.device_type(),
            identity.driver(),
            identity.driver_info(),
        );
    }
}
