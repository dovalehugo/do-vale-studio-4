//! Manual visual validation: one real decoded frame rendered continuously.
//!
//! Run: `cargo run -p dvs-render --example windows_nv12_visual --release`
//!
//! Human visual validation is required before Integration 5 can be marked COMPLETE.

#![cfg(target_os = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dvs_decoder::DecoderSession;
use dvs_gpu::{
    FenceTimeline, GpuBootstrap, GpuContext, SharedNv12TextureDesc, SurfaceWindowTarget,
    WindowsD3d11SharedNv12Producer, WindowsD3d11WgpuInteropBridge,
};
use dvs_render::{Nv12Renderer, Nv12RendererConfig, RenderSurface};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

const DEFAULT_FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";

struct VisualApp {
    window: Option<Arc<Window>>,
    gpu: Option<GpuContext>,
    surface: Option<RenderSurface>,
    renderer: Option<Nv12Renderer>,
    bridge: Option<WindowsD3d11WgpuInteropBridge>,
    prepared_metadata: Option<dvs_media::VideoFrameMetadata>,
    prepared_values: Option<dvs_gpu::FrameFenceValues>,
    init_error: Option<String>,
}

impl VisualApp {
    fn new() -> Self {
        Self {
            window: None,
            gpu: None,
            surface: None,
            renderer: None,
            bridge: None,
            prepared_metadata: None,
            prepared_values: None,
            init_error: None,
        }
    }

    fn signal_consumed_if_needed(&mut self) {
        if let (Some(gpu), Some(bridge), Some(values)) = (
            self.gpu.as_ref(),
            self.bridge.as_mut(),
            self.prepared_values,
        ) {
            let _ = bridge.signal_consumed_after_submit(gpu, values);
        }
    }

    fn draw_frame(&mut self) -> Result<(), String> {
        let gpu = self.gpu.as_ref().ok_or("gpu missing")?;
        let surface = self.surface.as_ref().ok_or("surface missing")?;
        let renderer = self.renderer.as_mut().ok_or("renderer missing")?;
        let bridge = self.bridge.as_mut().ok_or("bridge missing")?;
        let metadata = self.prepared_metadata.ok_or("prepared metadata missing")?;

        let video = bridge
            .prepared_frame()
            .map_err(|e| format!("prepared_frame: {e}"))?;

        let (surface_texture, target_view) = surface
            .acquire_frame(gpu)
            .map_err(|e| format!("acquire_frame: {e}"))?;
        let config = surface.configuration();

        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dvs-render-visual"),
            });

        renderer
            .encode_frame(
                gpu.device(),
                gpu.queue(),
                &mut encoder,
                video,
                metadata,
                &target_view,
                config.width,
                config.height,
            )
            .map_err(|e| format!("encode_frame: {e}"))?;

        gpu.queue().submit(Some(encoder.finish()));
        surface_texture.present();
        Ok(())
    }
}

impl ApplicationHandler for VisualApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.init_error.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Do Vale Studio 4 — Integration 5 NV12 Visual Validation")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.init_error = Some(format!("create_window failed: {err}"));
                event_loop.exit();
                return;
            }
        };

        match initialize_pipeline(window.clone()) {
            Ok((gpu, surface, renderer, bridge, metadata, values)) => {
                self.window = Some(window.clone());
                self.gpu = Some(gpu);
                self.surface = Some(surface);
                self.renderer = Some(renderer);
                self.bridge = Some(bridge);
                self.prepared_metadata = Some(metadata);
                self.prepared_values = Some(values);
                window.request_redraw();
            }
            Err(err) => {
                self.init_error = Some(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.signal_consumed_if_needed();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                self.signal_consumed_if_needed();
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(surface)) = (&self.gpu, &mut self.surface)
                    && size.width > 0
                    && size.height > 0
                {
                    let _ = surface.resize(gpu, size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.draw_frame() {
                    eprintln!("render error: {err}");
                    self.signal_consumed_if_needed();
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = VisualApp::new();
    event_loop.run_app(&mut app).expect("event loop run");

    if let Some(err) = app.init_error {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

fn fixture_path() -> PathBuf {
    std::env::var_os("DVS_DECODER_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join(DEFAULT_FIXTURE_REL))
}

fn require_fixture(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    Err(format!(
        "fixture not found at {} — see {SETUP_DOC}.",
        path.display()
    ))
}

fn initialize_pipeline(
    window: Arc<Window>,
) -> Result<
    (
        GpuContext,
        RenderSurface,
        Nv12Renderer,
        WindowsD3d11WgpuInteropBridge,
        dvs_media::VideoFrameMetadata,
        dvs_gpu::FrameFenceValues,
    ),
    String,
> {
    let fixture = fixture_path();
    require_fixture(&fixture)?;

    let gpu = pollster::block_on(GpuBootstrap::initialize(
        window.clone() as Arc<dyn SurfaceWindowTarget>
    ))
    .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;

    let size = window.inner_size();
    let surface = RenderSurface::configure(&gpu, size.width.max(1), size.height.max(1))
        .map_err(|e| format!("RenderSurface::configure failed: {e}"))?;

    let wgpu_luid = gpu
        .adapter_identity()
        .dxgi_luid()
        .ok_or_else(|| "wgpu adapter LUID unavailable".to_string())?;

    let mut session = DecoderSession::open(&fixture, wgpu_luid)
        .map_err(|e| format!("DecoderSession::open failed: {e}"))?;

    let (d3d11_device, d3d11_context) = {
        let hw = session
            .d3d11_hardware()
            .map_err(|e| format!("d3d11_hardware unavailable: {e}"))?;
        (hw.device().clone(), hw.context().clone())
    };
    let external_context_lock = session
        .external_context_lock()
        .map_err(|e| format!("external_context_lock failed: {e}"))?;

    let decoded = session
        .decode_next_d3d11()
        .map_err(|e| format!("decode failed: {e}"))?
        .ok_or_else(|| "decoder returned no frames".to_string())?;

    let metadata = decoded.metadata();
    let color = metadata.color();
    let allocation = metadata.dimensions().allocation();
    let visible = metadata.dimensions().visible();
    let (metadata, surface_ref) = decoded.into_parts();

    let desc = SharedNv12TextureDesc::new(allocation.width(), allocation.height())
        .map_err(|e| format!("SharedNv12TextureDesc failed: {e}"))?;
    let producer = WindowsD3d11SharedNv12Producer::new_with_external_lock(
        &d3d11_device,
        &d3d11_context,
        wgpu_luid,
        desc,
        Some(external_context_lock),
    )
    .map_err(|e| format!("producer failed: {e}"))?;

    let mut bridge = WindowsD3d11WgpuInteropBridge::new(&gpu, producer)
        .map_err(|e| format!("bridge failed: {e}"))?;

    let timeline = FenceTimeline::new();
    let values = timeline
        .current()
        .map_err(|e| format!("timeline values unavailable: {e}"))?;
    bridge
        .prepare_frame(&gpu, surface_ref, values)
        .map_err(|e| format!("prepare_frame failed: {e}"))?;

    drop(session);

    let encoding = surface.output_encoding();
    let renderer = Nv12Renderer::new(
        gpu.device(),
        Nv12RendererConfig {
            target_format: encoding.format,
        },
    )
    .map_err(|e| format!("Nv12Renderer::new failed: {e}"))?;

    eprintln!("=== Integration 5 visual validation ===");
    eprintln!("fixture: {}", fixture.display());
    eprintln!(
        "allocation: {}x{} visible: {}x{}",
        allocation.width(),
        allocation.height(),
        visible.width(),
        visible.height()
    );
    eprintln!(
        "color: matrix={:?} range={:?} transfer={:?} primaries={:?}",
        color.matrix(),
        color.range(),
        color.transfer(),
        color.primaries()
    );
    eprintln!("target: {}", encoding.summary());
    eprintln!("Press ESC or close the window to exit.");
    eprintln!("Human visual validation: PENDING — verify real video, aspect, and crop.");

    Ok((gpu, surface, renderer, bridge, metadata, values))
}
