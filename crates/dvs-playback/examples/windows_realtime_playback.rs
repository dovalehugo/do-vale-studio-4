//! Manual motion validation: PTS-timed continuous playback of the HEVC fixture.
//!
//! Run: `cargo run -p dvs-playback --example windows_realtime_playback --release`
//!
//! Human motion validation is required before Integration 6 can be marked COMPLETE.

#![cfg(target_os = "windows")]

use std::sync::Arc;

#[path = "../tests/support/playback_runtime.rs"]
mod playback_runtime;

use playback_runtime::{
    PlaybackPipeline, TickResult, fixture_path, initialize_gpu, print_metrics_summary,
    require_fixture,
};
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

struct VisualApp {
    window: Option<Arc<Window>>,
    gpu: Option<dvs_gpu::GpuContext>,
    surface: Option<dvs_render::RenderSurface>,
    pipeline: Option<PlaybackPipeline>,
    init_error: Option<String>,
}

impl VisualApp {
    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pipeline) = &mut self.pipeline
            && pipeline.playing
            && let Some(deadline) = pipeline.next_wait_deadline()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn drive_playback(&mut self, event_loop: &ActiveEventLoop) {
        let Some(pipeline) = self.pipeline.as_mut() else {
            return;
        };
        if !pipeline.playing {
            return;
        }
        let Some(gpu) = self.gpu.as_ref() else {
            return;
        };
        let Some(surface) = self.surface.as_ref() else {
            return;
        };
        let size = self
            .window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or_default();

        loop {
            match pipeline.tick(gpu, surface, size) {
                TickResult::Idle | TickResult::Waiting => {
                    self.update_control_flow(event_loop);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                    break;
                }
                TickResult::Presented => continue,
                TickResult::SurfaceRetry(message) => {
                    eprintln!("surface retry: {message}");
                    self.update_control_flow(event_loop);
                    break;
                }
                TickResult::Finished => {
                    print_metrics_summary(pipeline);
                    self.update_control_flow(event_loop);
                    break;
                }
                TickResult::Fatal(message) => {
                    self.init_error = Some(message);
                    event_loop.exit();
                    break;
                }
            }
        }
    }
}

impl ApplicationHandler for VisualApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.init_error.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("Do Vale Studio 4 — Integration 6 Realtime Playback")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.init_error = Some(format!("create_window failed: {err}"));
                event_loop.exit();
                return;
            }
        };

        let fixture = fixture_path();
        if let Err(err) = require_fixture(&fixture) {
            self.init_error = Some(err);
            event_loop.exit();
            return;
        }

        let window_for_gpu = window.clone();
        let init_result = pollster::block_on(async {
            let (gpu, surface) = initialize_gpu(window_for_gpu).await?;
            let pipeline = PlaybackPipeline::bootstrap(&fixture, &gpu, &surface)?;
            Ok::<_, String>((gpu, surface, pipeline))
        });

        match init_result {
            Ok((gpu, surface, pipeline)) => {
                self.window = Some(window.clone());
                self.gpu = Some(gpu);
                self.surface = Some(surface);
                self.pipeline = Some(pipeline);
                println!("Press SPACE to start playback");
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
                if let (Some(gpu), Some(pipeline)) = (&self.gpu, &mut self.pipeline) {
                    pipeline.release_prepared_on_exit(gpu);
                }
                if let Some(pipeline) = &self.pipeline
                    && pipeline.playing
                    && pipeline.metrics.eof_reached()
                {
                    print_metrics_summary(pipeline);
                }
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
                if let (Some(gpu), Some(pipeline)) = (&self.gpu, &mut self.pipeline) {
                    pipeline.release_prepared_on_exit(gpu);
                }
                event_loop.exit();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Space),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if let Some(pipeline) = self.pipeline.as_mut()
                    && !pipeline.playing
                {
                    if let Err(err) = pipeline.start_playback() {
                        self.init_error = Some(err);
                        event_loop.exit();
                        return;
                    }
                    self.drive_playback(event_loop);
                }
            }
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(surface), Some(pipeline)) =
                    (&self.gpu, &mut self.surface, &mut self.pipeline)
                    && size.width > 0
                    && size.height > 0
                    && surface.resize(gpu, size.width, size.height).is_ok()
                {
                    pipeline.metrics.record_surface_reconfiguration();
                }
            }
            WindowEvent::RedrawRequested => {
                if let (Some(gpu), Some(surface), Some(pipeline)) =
                    (&self.gpu, &self.surface, &mut self.pipeline)
                {
                    if pipeline.playing {
                        self.drive_playback(event_loop);
                    } else if let Err(err) = pipeline.draw_prepared_preview(gpu, surface) {
                        eprintln!("preview render error: {err}");
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self
            .pipeline
            .as_ref()
            .is_some_and(|pipeline| pipeline.playing)
        {
            self.drive_playback(event_loop);
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().expect("event loop");
    let mut app = VisualApp {
        window: None,
        gpu: None,
        surface: None,
        pipeline: None,
        init_error: None,
    };
    event_loop.run_app(&mut app).expect("event loop run");

    if let Some(err) = app.init_error {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
