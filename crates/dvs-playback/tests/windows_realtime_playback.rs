mod support;

use std::sync::Arc;

use support::playback_runtime::{
    PlaybackPipeline, TickResult, TimingTolerances, fixture_path, initialize_gpu,
    print_metrics_summary, require_fixture, validate_timing, workspace_root,
};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

struct RealtimeTestApp {
    window: Option<Arc<Window>>,
    gpu: Option<dvs_gpu::GpuContext>,
    surface: Option<dvs_render::RenderSurface>,
    pipeline: Option<PlaybackPipeline>,
    init_error: Option<String>,
}

impl RealtimeTestApp {
    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pipeline) = &mut self.pipeline
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
                    event_loop.exit();
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

impl ApplicationHandler for RealtimeTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.init_error.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("dvs-playback realtime hardware test")
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
            Ok((gpu, surface, mut pipeline)) => {
                if let Err(err) = pipeline.start_playback() {
                    self.init_error = Some(err);
                    event_loop.exit();
                    return;
                }
                self.window = Some(window);
                self.gpu = Some(gpu);
                self.surface = Some(surface);
                self.pipeline = Some(pipeline);
                self.drive_playback(event_loop);
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
                event_loop.exit();
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
                self.drive_playback(event_loop);
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

#[test]
#[ignore = "requires Windows GPU, FFmpeg dev libraries, and docs/fixtures/test_4k_hevc_8bit30.mp4"]
fn windows_realtime_playback_fixture() {
    let fixture = fixture_path();
    require_fixture(&fixture).expect("fixture setup");

    eprintln!("workspace_root: {}", workspace_root().display());
    eprintln!(
        "timing_tolerances: min_ratio=0.85 max_ratio=1.50 max_sustained_fps=45 max_late_drops=3"
    );

    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut app = RealtimeTestApp {
        window: None,
        gpu: None,
        surface: None,
        pipeline: None,
        init_error: None,
    };
    event_loop.run_app(&mut app).expect("event loop run");

    if let Some(err) = app.init_error {
        panic!("realtime playback failed: {err}");
    }

    let pipeline = app.pipeline.expect("pipeline");
    print_metrics_summary(&pipeline);

    let expected_media = pipeline.metrics.playback_media_duration_us();
    validate_timing(
        &pipeline.metrics,
        expected_media,
        &TimingTolerances::default(),
    )
    .expect("timing validation");

    assert!(pipeline.metrics.eof_reached());
    assert!(pipeline.metrics.frames_presented() > 0);
    assert_eq!(pipeline.bridge.shared_handle_open_counts(), (1, 1));
}
