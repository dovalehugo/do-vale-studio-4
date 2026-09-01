//! GPU bootstrap, surface creation, and context ownership.

use std::sync::Arc;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use wgpu::{
    Adapter, Backends, Device, ExperimentalFeatures, Instance, InstanceDescriptor, Limits, Queue,
    RequestAdapterOptions, Surface, SurfaceTarget,
};

use crate::adapter::{AdapterIdentity, REQUIRED_DEVICE_FEATURES, validate_required_features};
use crate::error::GpuError;

/// Window or display target that can back a wgpu surface.
///
/// Compatible with future `Arc<winit::window::Window>` callers. Implementors must
/// remain alive for the lifetime of the created [`GpuContext`].
pub trait SurfaceWindowTarget: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static {}

impl<T> SurfaceWindowTarget for T where T: HasWindowHandle + HasDisplayHandle + Send + Sync + 'static
{}

/// Safe entry point for wgpu initialization on the initial Windows DX12 slice.
///
/// Creates a DXGI-surface-backed adapter selection path before any FFmpeg or D3D11VA
/// initialization. Headless adapter selection is not the production path.
pub struct GpuBootstrap;

impl GpuBootstrap {
    /// Creates a wgpu instance limited to the DX12 backend for the initial slice.
    pub fn create_instance() -> Instance {
        Instance::new(&InstanceDescriptor {
            backends: Backends::DX12,
            ..Default::default()
        })
    }

    /// Initializes GPU context from a surface-compatible window target.
    ///
    /// The target must outlive the returned context. In production, `dvs-app` is
    /// expected to call this before FFmpeg/D3D11VA device creation.
    pub async fn initialize(target: Arc<dyn SurfaceWindowTarget>) -> Result<GpuContext, GpuError> {
        let instance = Self::create_instance();
        let surface_target = SurfaceTargetHandle(target.clone());
        let surface = instance.create_surface(SurfaceTarget::Window(Box::new(surface_target)))?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await?;

        let adapter_info = adapter.get_info();
        let identity = AdapterIdentity::from_adapter_info(&adapter_info)?;
        validate_required_features(adapter.features())?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("dvs-gpu"),
                required_features: REQUIRED_DEVICE_FEATURES,
                required_limits: Limits::default(),
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: Default::default(),
                trace: Default::default(),
            })
            .await?;

        #[cfg(target_os = "windows")]
        let identity = {
            let luid = crate::windows::extract_dxgi_adapter_luid(&device)?;
            identity.with_dxgi_luid(luid)
        };

        Ok(GpuContext {
            _instance: instance,
            surface_target: target,
            surface,
            _adapter: adapter,
            device,
            queue,
            identity,
        })
    }
}

/// Owns the wgpu instance, surface, adapter, device, queue, and adapter identity.
///
/// Must be created before FFmpeg/D3D11VA initialization so adapter selection is
/// surface-compatible and NV12-capable. Rendering and presentation are not
/// performed by this type in Integration 2.
pub struct GpuContext {
    _instance: Instance,
    surface_target: Arc<dyn SurfaceWindowTarget>,
    surface: Surface<'static>,
    _adapter: Adapter,
    device: Device,
    queue: Queue,
    identity: AdapterIdentity,
}

impl GpuContext {
    /// Returns the validated public adapter identity.
    pub fn adapter_identity(&self) -> &AdapterIdentity {
        &self.identity
    }

    /// Returns the wgpu device for render and interop consumers.
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Returns the wgpu queue for render and interop consumers.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// Returns the window-backed surface selected during bootstrap.
    pub fn surface(&self) -> &Surface<'static> {
        &self.surface
    }

    /// Returns the surface target kept alive for the surface lifetime.
    pub fn surface_target(&self) -> &Arc<dyn SurfaceWindowTarget> {
        &self.surface_target
    }
}

struct SurfaceTargetHandle(Arc<dyn SurfaceWindowTarget>);

impl HasWindowHandle for SurfaceTargetHandle {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        self.0.window_handle()
    }
}

impl HasDisplayHandle for SurfaceTargetHandle {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        self.0.display_handle()
    }
}
