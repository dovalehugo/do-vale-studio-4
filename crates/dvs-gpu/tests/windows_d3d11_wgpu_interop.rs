//! Windows hardware integration test for D3D11 producer + wgpu DX12 consumer interop.

#![cfg(target_os = "windows")]

use std::sync::Arc;

use dvs_gpu::{
    D3d11DecodedSurfaceRef, FenceTimeline, GpuBootstrap, GpuContext, GpuVideoPixelFormat,
    SharedNv12TextureDesc, SurfaceWindowTarget, WindowsD3d11SharedNv12Producer,
    WindowsD3d11WgpuInteropBridge,
};
use wgpu::{TextureAspect, TextureFormat, TextureViewDescriptor};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2176;

struct InteropTestApp {
    finished: Option<Result<InteropTestContext, String>>,
}

struct InteropTestContext {
    _window: Arc<Window>,
    gpu: GpuContext,
    d3d11_device: ID3D11Device,
    d3d11_context: ID3D11DeviceContext,
    source: ID3D11Texture2D,
}

impl ApplicationHandler for InteropTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("dvs-gpu interop test")
                .with_inner_size(winit::dpi::LogicalSize::new(640, 360)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.finished = Some(Err(format!("create_window failed: {err}")));
                event_loop.exit();
                return;
            }
        };

        self.finished = Some(initialize_test_context(window));
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

fn initialize_test_context(window: Arc<Window>) -> Result<InteropTestContext, String> {
    let gpu = pollster::block_on(GpuBootstrap::initialize(
        window.clone() as Arc<dyn SurfaceWindowTarget>
    ))
    .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;

    let (d3d11_device, d3d11_context) = create_hardware_d3d11_device()?;
    let source = create_test_nv12_texture(&d3d11_device);

    Ok(InteropTestContext {
        _window: window,
        gpu,
        d3d11_device,
        d3d11_context,
        source,
    })
}

fn create_hardware_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext), String> {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;
    // SAFETY: Creates a local hardware D3D11 device for isolated integration testing only.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
        .map_err(|e| format!("D3D11CreateDevice failed: {e}"))?;
    }
    Ok((device.ok_or("null device")?, context.ok_or("null context")?))
}

fn create_test_nv12_texture(device: &ID3D11Device) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: WIDTH,
        Height: HEIGHT,
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
        MiscFlags: 0,
    };
    let mut texture = None;
    // SAFETY: `device` is valid; `desc` describes a compatible NV12 test source texture.
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .expect("CreateTexture2D source");
    }
    texture.expect("source texture")
}

fn create_nv12_plane_views(frame: &dvs_gpu::GpuVideoFrame) {
    let texture = frame.texture();
    let _y = texture.create_view(&TextureViewDescriptor {
        label: Some("interop-test-y"),
        format: Some(TextureFormat::R8Unorm),
        aspect: TextureAspect::Plane0,
        ..Default::default()
    });
    let _uv = texture.create_view(&TextureViewDescriptor {
        label: Some("interop-test-uv"),
        format: Some(TextureFormat::Rg8Unorm),
        aspect: TextureAspect::Plane1,
        ..Default::default()
    });
}

fn submit_empty_queue_work(gpu: &GpuContext) {
    let encoder = gpu
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("interop-test-empty-submit"),
        });
    gpu.queue().submit(Some(encoder.finish()));
}

fn run_one_frame(
    bridge: &mut WindowsD3d11WgpuInteropBridge,
    gpu: &GpuContext,
    source: &ID3D11Texture2D,
    timeline: &mut FenceTimeline,
) {
    let values = timeline.current().expect("timeline values");
    let frame = D3d11DecodedSurfaceRef::new(source, 0).expect("decoded surface");
    let video = bridge
        .prepare_frame(gpu, frame, values)
        .expect("prepare_frame");
    assert_eq!(video.pixel_format(), GpuVideoPixelFormat::Nv12);
    assert_eq!(video.allocation_width(), WIDTH);
    assert_eq!(video.allocation_height(), HEIGHT);
    create_nv12_plane_views(video);
    submit_empty_queue_work(gpu);
    bridge
        .signal_consumed_after_submit(gpu, values)
        .expect("signal_consumed_after_submit");
    timeline.advance().expect("advance timeline");
}

fn run_interop_hardware_test(ctx: InteropTestContext) {
    let wgpu_luid = ctx.gpu.adapter_identity().dxgi_luid().expect("wgpu LUID");

    let desc = SharedNv12TextureDesc::new(WIDTH, HEIGHT).expect("desc");
    let producer =
        WindowsD3d11SharedNv12Producer::new(&ctx.d3d11_device, &ctx.d3d11_context, wgpu_luid, desc)
            .expect("producer");

    let mut bridge = WindowsD3d11WgpuInteropBridge::new(&ctx.gpu, producer).expect("bridge");
    assert_eq!(bridge.shared_handle_open_counts(), (1, 1));

    let mut timeline = FenceTimeline::new();
    run_one_frame(&mut bridge, &ctx.gpu, &ctx.source, &mut timeline);
    run_one_frame(&mut bridge, &ctx.gpu, &ctx.source, &mut timeline);
}

#[test]
#[ignore = "requires Windows hardware D3D11 + wgpu DX12 interop"]
fn windows_d3d11_wgpu_interop_hardware() {
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("EventLoop");
    let mut app = InteropTestApp { finished: None };
    event_loop.run_app(&mut app).expect("run_app");

    let ctx = app
        .finished
        .expect("handler finished")
        .expect("interop context");
    run_interop_hardware_test(ctx);
}
