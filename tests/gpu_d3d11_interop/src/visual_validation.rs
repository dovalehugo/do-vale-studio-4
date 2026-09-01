//! Human visual validation mode (`--visual`).
//!
//! Renders real decoded HEVC frames in a 1280×720 window until the user exits.
//! Not a performance benchmark.

use std::path::Path;
use std::sync::Arc;

use windows::Win32::Graphics::Direct3D12::ID3D12Fence;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::render_path::RenderPathBundle;
use crate::wgpu_hal_interop::{self, WgpuDx12Context};

const FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";

enum VisualState {
    Uninitialized,
    Ready {
        probe: crate::ProbeResult,
        context: WgpuDx12Context,
        render: RenderPathBundle,
        cached_fence: ID3D12Fence,
        timeline: crate::multi_frame::ContinuousFramebufferTimeline,
    },
    Failed(String),
}

struct VisualValidationApp {
    fixture: std::path::PathBuf,
    state: VisualState,
    banner_printed: bool,
}

impl VisualValidationApp {
    fn new(fixture: std::path::PathBuf) -> Self {
        Self {
            fixture,
            state: VisualState::Uninitialized,
            banner_printed: false,
        }
    }

    fn print_banner_once(&mut self) {
        if self.banner_printed {
            return;
        }
        self.banner_printed = true;
        println!();
        println!("==================================================");
        println!("VISUAL VALIDATION MODE");
        println!("==================================================");
        println!();
        println!("Real HEVC fixture:");
        println!("{FIXTURE_REL}");
        println!();
        println!("Window:");
        println!("1280x720");
        println!();
        println!("Inspect:");
        println!();
        println!("- real moving video content");
        println!("- correct orientation");
        println!("- correct aspect ratio");
        println!("- normal colors");
        println!("- no green/purple corruption");
        println!("- correct chroma");
        println!("- no garbage/padded strip at bottom");
        println!("- smooth changing frames");
        println!();
        println!("Press ESC or close the window to exit.");
        println!();
        println!("==================================================");
        println!();
    }

    fn resize_surface(context: &mut WgpuDx12Context, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        context.surface_config.width = width;
        context.surface_config.height = height;
        context
            .surface
            .configure(&context.device, &context.surface_config);
    }

    fn process_one_frame(&mut self) -> Result<(), String> {
        let VisualState::Ready {
            probe,
            context,
            render,
            cached_fence,
            timeline,
        } = &mut self.state
        else {
            return Ok(());
        };

        let mut frames_decoded = 0u32;
        let mut gpu_copies = 0u32;
        let mut frames_rendered = 0u32;
        let mut present_calls = 0u32;
        let mut decode_ms = 0.0;
        let mut copy_ms = 0.0;
        let mut sync_ms = 0.0;
        let mut render_ms = 0.0;

        match crate::multi_frame::process_one_real_frame(
            probe,
            context,
            render,
            cached_fence,
            timeline,
            &mut frames_decoded,
            &mut gpu_copies,
            &mut frames_rendered,
            &mut present_calls,
            &mut decode_ms,
            &mut copy_ms,
            &mut sync_ms,
            &mut render_ms,
        ) {
            Ok(()) => Ok(()),
            Err(err) if err.contains("EOF") => {
                crate::restart_fixture_decode(probe)?;
                crate::multi_frame::process_one_real_frame(
                    probe,
                    context,
                    render,
                    cached_fence,
                    timeline,
                    &mut frames_decoded,
                    &mut gpu_copies,
                    &mut frames_rendered,
                    &mut present_calls,
                    &mut decode_ms,
                    &mut copy_ms,
                    &mut sync_ms,
                    &mut render_ms,
                )
            }
            Err(err) => Err(err),
        }
    }

    fn initialize_pipeline(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Do Vale Studio 4 — GPU Experiment 2 Visual Validation")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32)),
                )
                .map_err(|e| format!("create_window failed: {e}"))?,
        );

        let context = pollster::block_on(wgpu_hal_interop::init_wgpu_context_with_window(window))?;

        let probe = crate::probe_format_and_open_decoder(&self.fixture)?;
        let wgpu_interop = wgpu_hal_interop::import_shared_d3d12_nv12_into_wgpu(
            context,
            probe._shared_nt_handle.handle(),
            probe.shared_fence_sync.fence_handle(),
            &probe.d3d12_open.adapter_name,
            probe.shared_fence_sync.info.synchronization_valid,
        );

        if !wgpu_interop.info.interop_valid {
            return Err(wgpu_interop
                .info
                .error
                .unwrap_or_else(|| "wgpu interop failed".to_string()));
        }

        let context = wgpu_interop
            ._context
            .as_ref()
            .ok_or_else(|| "wgpu context missing after interop".to_string())?;

        let cached_fence = wgpu_interop
            .cached_wgpu_fence
            .as_ref()
            .ok_or_else(|| "cached wgpu ID3D12Fence missing".to_string())?
            .clone();

        let render = crate::render_path::run_render_path_steps_34_to_36(&wgpu_interop, context)?;

        let context = wgpu_interop
            ._context
            .ok_or_else(|| "wgpu context missing after interop".to_string())?;

        self.state = VisualState::Ready {
            probe,
            context,
            render,
            cached_fence,
            timeline: crate::multi_frame::ContinuousFramebufferTimeline::new(),
        };

        Ok(())
    }
}

impl ApplicationHandler for VisualValidationApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !matches!(self.state, VisualState::Uninitialized) {
            return;
        }

        match self.initialize_pipeline(event_loop) {
            Ok(()) => {
                self.print_banner_once();
                event_loop.set_control_flow(ControlFlow::Poll);
                if let VisualState::Ready { context, .. } = &self.state {
                    context._window.request_redraw();
                }
            }
            Err(err) => {
                self.state = VisualState::Failed(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let VisualState::Ready { context, .. } = &mut self.state {
                    Self::resize_surface(context, size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.process_one_frame() {
                    eprintln!("frame error: {err}");
                    self.state = VisualState::Failed(err);
                    event_loop.exit();
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed()
                    && matches!(event.logical_key, Key::Named(NamedKey::Escape))
                {
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let VisualState::Ready { context, .. } = &self.state {
            context._window.request_redraw();
        }
    }
}

pub fn run_visual_validation(fixture: &Path) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("EventLoop::new failed: {e}"))?;
    let mut app = VisualValidationApp::new(fixture.to_path_buf());
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("EventLoop::run_app failed: {e}"))?;

    if let VisualState::Failed(err) = app.state {
        return Err(err);
    }

    Ok(())
}
