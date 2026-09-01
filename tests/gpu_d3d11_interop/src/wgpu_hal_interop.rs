//! Step 33 — import the real shared D3D12 NV12 resource into wgpu-hal DX12.

use std::sync::Arc;

use wgpu::hal::api::Dx12;
use wgpu::hal::dx12::Device as HalDx12Device;
use wgpu::{
    Backends, Device, DeviceDescriptor, ExperimentalFeatures, Features, Instance,
    InstanceDescriptor, Limits, Queue, RequestAdapterOptions, Surface, SurfaceConfiguration,
    TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Graphics::Direct3D12::{ID3D12Fence, ID3D12Resource};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

const DECODER_ALLOC_WIDTH: u32 = 3840;
const DECODER_ALLOC_HEIGHT: u32 = 2176;
const FENCE_SIGNAL_VALUE: u64 = 1;
const WGPU_VERSION: &str = "27";

pub struct WgpuDx12Context {
    pub adapter_name: String,
    pub adapter_backend: String,
    pub(crate) _window: Arc<Window>,
    pub surface: Surface<'static>,
    pub surface_config: SurfaceConfiguration,
    pub device: Device,
    pub queue: Queue,
}

pub struct WgpuHalInteropInfo {
    pub wgpu_version: String,
    pub wgpu_hal_version: String,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub texture_open_result: String,
    pub imported_resource_pointer: usize,
    pub wgpu_wrapped_resource_pointer: usize,
    pub fence_open_result: String,
    pub wgpu_queue_wait_result: String,
    pub create_texture_from_hal_result: String,
    pub mechanism: String,
    pub interop_valid: bool,
    pub step_status: String,
    pub error: Option<String>,
}

pub struct WgpuHalInteropBundle {
    pub info: WgpuHalInteropInfo,
    pub _context: Option<WgpuDx12Context>,
    pub _texture: Option<wgpu::Texture>,
    /// Opened once during Step 33; reused for all subsequent Wait calls.
    pub cached_wgpu_fence: Option<ID3D12Fence>,
    /// Count of ID3D12Device::OpenSharedHandle calls for the fence (must be 1 on success).
    pub fence_open_shared_handle_calls: u32,
}

struct InitWgpuApp {
    finished: Option<Result<WgpuDx12Context, String>>,
}

impl ApplicationHandler for InitWgpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Do Vale Studio 4 — GPU Experiment 2 Visual Validation")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.finished = Some(Err(format!(
                    "winit ActiveEventLoop::create_window failed: {err}"
                )));
                event_loop.exit();
                return;
            }
        };

        self.finished = Some(pollster::block_on(init_wgpu_context_with_window(window)));
        event_loop.exit();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}

fn format_hresult(operation: &str, error: windows::core::Error) -> String {
    format!(
        "{operation} failed: {error} (HRESULT=0x{:08X})",
        error.code().0 as u32
    )
}

fn failed_bundle(
    error: String,
    adapter_name: String,
    adapter_backend: String,
) -> WgpuHalInteropBundle {
    WgpuHalInteropBundle {
        info: WgpuHalInteropInfo {
            wgpu_version: WGPU_VERSION.to_string(),
            wgpu_hal_version: format!("{WGPU_VERSION}.x (wgpu::hal re-export)"),
            adapter_name,
            adapter_backend,
            texture_open_result: "FAILED".to_string(),
            imported_resource_pointer: 0,
            wgpu_wrapped_resource_pointer: 0,
            fence_open_result: "skipped".to_string(),
            wgpu_queue_wait_result: "skipped".to_string(),
            create_texture_from_hal_result: "skipped".to_string(),
            mechanism:
                "pre-init wgpu DX12 + OpenSharedHandle + texture_from_raw + create_texture_from_hal"
                    .to_string(),
            interop_valid: false,
            step_status: "STEP 33 / 40: FAILED".to_string(),
            error: Some(error),
        },
        _context: None,
        _texture: None,
        cached_wgpu_fence: None,
        fence_open_shared_handle_calls: 0,
    }
}

/// On the tested Windows 10 + RX 580 configuration, selecting the wgpu DX12 adapter
/// before FFmpeg/D3D11VA device creation was required for consistent RX 580 selection
/// (empirical observation; not established as a universal DXGI requirement).
pub fn init_wgpu_dx12_context() -> Result<WgpuDx12Context, String> {
    let event_loop = EventLoop::new().map_err(|e| format!("winit EventLoop::new failed: {e}"))?;
    let mut app = InitWgpuApp { finished: None };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("winit EventLoop::run_app failed: {e}"))?;
    match app.finished {
        Some(Ok(context)) => Ok(context),
        Some(Err(err)) => Err(err),
        None => Err("wgpu init handler did not produce a result".to_string()),
    }
}

pub(crate) async fn init_wgpu_context_with_window(
    window: Arc<Window>,
) -> Result<WgpuDx12Context, String> {
    let instance = Instance::new(&InstanceDescriptor {
        backends: Backends::DX12,
        ..Default::default()
    });

    let surface = instance
        .create_surface(window.clone())
        .map_err(|e| format!("wgpu create_surface failed: {e}"))?;

    let adapter = instance
        .request_adapter(&RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| format!("wgpu request_adapter(DX12+surface) failed: {e}"))?;

    let adapter_info = adapter.get_info();
    if adapter_info.backend != wgpu::Backend::Dx12 {
        return Err(format!(
            "wgpu selected non-DX12 backend {:?}",
            adapter_info.backend
        ));
    }
    if adapter_info.name.contains("Microsoft Basic Render Driver") {
        return Err(
            "wgpu selected Microsoft Basic Render Driver during pre-FFmpeg init".to_string(),
        );
    }
    if !adapter.features().contains(Features::TEXTURE_FORMAT_NV12) {
        return Err("wgpu DX12 adapter does not support Features::TEXTURE_FORMAT_NV12".to_string());
    }

    let (device, queue) = adapter
        .request_device(&DeviceDescriptor {
            label: Some("gpu-d3d11-interop-step33"),
            required_features: Features::TEXTURE_FORMAT_NV12,
            required_limits: Limits::default(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: Default::default(),
            trace: Default::default(),
        })
        .await
        .map_err(|e| format!("wgpu request_device failed: {e}"))?;

    let caps = surface.get_capabilities(&adapter);
    let surface_format = caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb() || matches!(f, TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm))
        .unwrap_or(caps.formats[0]);
    let size = window.inner_size();
    let surface_config = SurfaceConfiguration {
        usage: TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };
    surface.configure(&device, &surface_config);

    Ok(WgpuDx12Context {
        adapter_name: adapter_info.name,
        adapter_backend: format!("{:?}", adapter_info.backend),
        _window: window,
        surface,
        surface_config,
        device,
        queue,
    })
}

pub fn import_shared_d3d12_nv12_into_wgpu(
    context: WgpuDx12Context,
    texture_handle: HANDLE,
    fence_handle: Option<HANDLE>,
    expected_adapter_name: &str,
    fence_sync_valid: bool,
) -> WgpuHalInteropBundle {
    let adapter_name = context.adapter_name.clone();
    let adapter_backend = context.adapter_backend.clone();

    if !is_handle_valid(texture_handle) {
        return failed_bundle(
            "shared texture NT HANDLE is not valid".to_string(),
            adapter_name,
            adapter_backend,
        );
    }

    if !fence_sync_valid {
        return failed_bundle(
            "cross-API fence synchronization (step 32) is not valid".to_string(),
            adapter_name,
            adapter_backend,
        );
    }

    if adapter_name != expected_adapter_name {
        eprintln!(
            "warning: pre-init wgpu adapter {:?} differs from decode-path adapter {:?}",
            adapter_name, expected_adapter_name
        );
    }

    if adapter_name.contains("Microsoft Basic Render Driver") {
        return failed_bundle(
            "wgpu context is on Microsoft Basic Render Driver; shared NT handles require the decode GPU"
                .to_string(),
            adapter_name,
            adapter_backend,
        );
    }

    let device = &context.device;

    let hal_device = match unsafe { device.as_hal::<Dx12>() } {
        Some(hal_device) => hal_device,
        None => {
            return failed_bundle(
                "device.as_hal::<Dx12>() returned None".to_string(),
                adapter_name,
                adapter_backend,
            );
        }
    };

    let mut imported_resource: Option<ID3D12Resource> = None;
    let texture_open_result = match unsafe {
        hal_device
            .raw_device()
            .OpenSharedHandle(texture_handle, &mut imported_resource)
    } {
        Ok(()) => {
            if imported_resource.is_none() {
                "FAILED — OpenSharedHandle returned null resource".to_string()
            } else {
                "OK".to_string()
            }
        }
        Err(e) => format_hresult("wgpu D3D12 OpenSharedHandle<ID3D12Resource>", e),
    };
    if !texture_open_result.starts_with("OK") {
        return failed_bundle(texture_open_result, adapter_name, adapter_backend);
    }
    let imported_resource = imported_resource.unwrap();
    let imported_resource_pointer = windows::core::Interface::as_raw(&imported_resource) as usize;

    let (fence_open_result, wgpu_queue_wait_result, cached_wgpu_fence, fence_open_calls) =
        match fence_handle.filter(|&handle| is_handle_valid(handle)) {
            Some(fence_handle) => {
                let mut wgpu_fence: Option<ID3D12Fence> = None;
                match unsafe {
                    hal_device
                        .raw_device()
                        .OpenSharedHandle(fence_handle, &mut wgpu_fence)
                } {
                    Ok(()) => {
                        let fence = match wgpu_fence {
                            Some(fence) => fence,
                            None => {
                                return failed_bundle(
                                    "wgpu D3D12 OpenSharedHandle<ID3D12Fence> returned null"
                                        .to_string(),
                                    adapter_name,
                                    adapter_backend,
                                );
                            }
                        };
                        match unsafe { hal_device.raw_queue().Wait(&fence, FENCE_SIGNAL_VALUE) } {
                            Ok(()) => (
                                "OK — OpenSharedHandle once, fence retained".to_string(),
                                format!("OK — Wait(value={FENCE_SIGNAL_VALUE})"),
                                Some(fence),
                                1u32,
                            ),
                            Err(e) => (
                                "OK — OpenSharedHandle once".to_string(),
                                format_hresult("wgpu D3D12 CommandQueue::Wait", e),
                                Some(fence),
                                1u32,
                            ),
                        }
                    }
                    Err(e) => (
                        format_hresult("wgpu D3D12 OpenSharedHandle<ID3D12Fence>", e),
                        "skipped".to_string(),
                        None,
                        0u32,
                    ),
                }
            }
            None => (
                "skipped — fence HANDLE unavailable".to_string(),
                "skipped".to_string(),
                None,
                0u32,
            ),
        };

    if !wgpu_queue_wait_result.starts_with("OK") {
        return failed_bundle(wgpu_queue_wait_result, adapter_name, adapter_backend);
    }

    if cached_wgpu_fence.is_none() {
        return failed_bundle(
            "cached wgpu ID3D12Fence was not retained after OpenSharedHandle".to_string(),
            adapter_name,
            adapter_backend,
        );
    }

    let texture_desc = TextureDescriptor {
        label: Some("decoded-hevc-nv12-external"),
        size: wgpu::Extent3d {
            width: DECODER_ALLOC_WIDTH,
            height: DECODER_ALLOC_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::NV12,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };

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

    let wgpu_texture =
        unsafe { device.create_texture_from_hal::<Dx12>(hal_texture, &texture_desc) };

    let wgpu_wrapped_resource_pointer = unsafe {
        wgpu_texture
            .as_hal::<Dx12>()
            .map(|hal_tex| windows::core::Interface::as_raw(hal_tex.raw_resource()) as usize)
            .unwrap_or(0)
    };

    WgpuHalInteropBundle {
        info: WgpuHalInteropInfo {
            wgpu_version: WGPU_VERSION.to_string(),
            wgpu_hal_version: format!("{WGPU_VERSION}.x (wgpu::hal re-export)"),
            adapter_name,
            adapter_backend,
            texture_open_result,
            imported_resource_pointer,
            wgpu_wrapped_resource_pointer,
            fence_open_result,
            wgpu_queue_wait_result,
            create_texture_from_hal_result: "OK".to_string(),
            mechanism:
                "pre-init wgpu DX12 + OpenSharedHandle + texture_from_raw + create_texture_from_hal"
                    .to_string(),
            interop_valid: true,
            step_status: "STEP 33 / 40: PASS".to_string(),
            error: None,
        },
        _context: Some(context),
        _texture: Some(wgpu_texture),
        cached_wgpu_fence,
        fence_open_shared_handle_calls: fence_open_calls,
    }
}

/// GPU-side Wait on the cached wgpu ID3D12Fence (no OpenSharedHandle).
pub fn wait_cached_wgpu_fence(
    context: &WgpuDx12Context,
    fence: &ID3D12Fence,
    value: u64,
) -> Result<(), String> {
    let hal_device = unsafe { context.device.as_hal::<Dx12>() }
        .ok_or_else(|| "device.as_hal::<Dx12>() returned None".to_string())?;
    unsafe {
        hal_device
            .raw_queue()
            .Wait(fence, value)
            .map_err(|e| format_hresult("wgpu D3D12 CommandQueue::Wait (cached fence)", e))?;
    }
    Ok(())
}

/// GPU-side Signal on wgpu-hal's exact raw/present queue (no probe D3D12 queue).
pub fn signal_cached_wgpu_fence(
    context: &WgpuDx12Context,
    fence: &ID3D12Fence,
    value: u64,
) -> Result<(), String> {
    let hal_device = unsafe { context.device.as_hal::<Dx12>() }
        .ok_or_else(|| "device.as_hal::<Dx12>() returned None".to_string())?;
    unsafe {
        hal_device
            .raw_queue()
            .Signal(fence, value)
            .map_err(|e| format_hresult("wgpu D3D12 CommandQueue::Signal (cached fence)", e))?;
    }
    Ok(())
}

fn is_handle_valid(handle: HANDLE) -> bool {
    !handle.is_invalid() && handle != windows::Win32::Foundation::INVALID_HANDLE_VALUE
}

pub fn print_wgpu_hal_interop(bundle: &WgpuHalInteropBundle) {
    let info = &bundle.info;
    println!("=== wgpu-hal DX12 external resource interop ===");
    println!("wgpu version:           {}", info.wgpu_version);
    println!("wgpu-hal version:       {}", info.wgpu_hal_version);
    println!("wgpu adapter:           {}", info.adapter_name);
    println!("wgpu adapter backend:   {}", info.adapter_backend);
    println!("texture OpenSharedHandle: {}", info.texture_open_result);
    println!(
        "imported ID3D12Resource: 0x{:x}",
        info.imported_resource_pointer
    );
    println!(
        "wgpu-wrapped ID3D12Resource: 0x{:x}",
        info.wgpu_wrapped_resource_pointer
    );
    println!("fence OpenSharedHandle: {}", info.fence_open_result);
    println!("wgpu queue Wait:        {}", info.wgpu_queue_wait_result);
    println!(
        "create_texture_from_hal: {}",
        info.create_texture_from_hal_result
    );
    println!("mechanism:              {}", info.mechanism);
    println!(
        "interop valid:          {}",
        if info.interop_valid { "yes" } else { "no" }
    );
    if let Some(err) = &info.error {
        println!("error:                  {err}");
    }
    println!();
    println!("{}", info.step_status);
}

pub fn print_cached_fence_info(bundle: &WgpuHalInteropBundle) {
    println!(
        "cached ID3D12Fence:      {}",
        if bundle.cached_wgpu_fence.is_some() {
            "retained (opened once)"
        } else {
            "MISSING"
        }
    );
    println!(
        "fence OpenSharedHandle calls (init): {}",
        bundle.fence_open_shared_handle_calls
    );
}
