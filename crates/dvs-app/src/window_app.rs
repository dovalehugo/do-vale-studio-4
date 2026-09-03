//! winit event-loop application for the production video window.

use std::sync::Arc;

use dvs_render::AspectFitRect;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

use crate::config::{AppConfig, RunMode, event_loop_allows_any_thread};
use crate::display::can_render_display_frame;
use crate::egui_overlay::EguiStaticOverlay;
use crate::error::AppError;
use crate::metrics_summary::format_metrics_summary;
use crate::shutdown::release_prepared_bridge_frame;
use crate::state::AppState;
use crate::windows::gpu_surface::initialize_gpu;
use crate::windows::video_pipeline::{TickResult, VideoPipeline};

fn create_event_loop(config: &AppConfig) -> Result<EventLoop<()>, AppError> {
    if event_loop_allows_any_thread(config.run_mode()) {
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .map_err(|err| AppError::Window(err.to_string()))
    } else {
        EventLoop::new().map_err(|err| AppError::Window(err.to_string()))
    }
}

/// Runs the production Windows video application.
pub fn run_windows_app(config: AppConfig) -> Result<(), AppError> {
    let event_loop = create_event_loop(&config)?;
    let mut app = VideoWindowApp::new(config);
    event_loop
        .run_app(&mut app)
        .map_err(|err| AppError::Window(err.to_string()))?;
    if let Some(err) = app.fatal_error {
        return Err(err);
    }
    Ok(())
}

/// Post-EOF resize validation steps for the smoke test harness.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SmokePostEofPhase {
    Resize1000x700,
    AwaitRedraw1,
    Resize1600x600,
    AwaitRedraw2,
    Complete,
}

/// Application/window handler composition root.
///
/// # Drop order
///
/// Rust drops struct fields in declaration order. `pipeline` is declared before
/// `egui`, `surface`, and `gpu` so decode/bridge teardown runs while the presentation
/// device remains available if explicit shutdown did not already release prepared
/// resources. `egui` drops before `surface`/`gpu` so the egui-wgpu renderer releases
/// GPU resources while the device is still alive. `gpu` holds
/// `Arc<dyn SurfaceWindowTarget>` and therefore keeps the window target alive after
/// the optional `window` field is dropped.
struct VideoWindowApp {
    config: AppConfig,
    state: AppState,
    state_log: Vec<AppState>,
    window: Option<Arc<Window>>,
    pipeline: Option<VideoPipeline>,
    egui: Option<EguiStaticOverlay>,
    surface: Option<dvs_render::RenderSurface>,
    gpu: Option<dvs_gpu::GpuContext>,
    fatal_error: Option<AppError>,
    smoke_post_eof: Option<SmokePostEofPhase>,
    smoke_aspect_fits: Vec<(u32, u32, AspectFitRect)>,
    shutdown_resources_released: bool,
}

impl VideoWindowApp {
    fn new(config: AppConfig) -> Self {
        Self {
            config,
            state: AppState::Initializing,
            state_log: vec![AppState::Initializing],
            window: None,
            pipeline: None,
            egui: None,
            surface: None,
            gpu: None,
            fatal_error: None,
            smoke_post_eof: None,
            smoke_aspect_fits: Vec::with_capacity(2),
            shutdown_resources_released: false,
        }
    }

    fn transition_to(&mut self, next: AppState) {
        if self.state != next {
            self.state = next;
            self.state_log.push(next);
        }
    }

    fn set_fatal(&mut self, error: AppError) {
        self.transition_to(AppState::Fatal);
        self.fatal_error = Some(error);
    }

    fn release_prepared_if_needed(&mut self) {
        if self.shutdown_resources_released {
            return;
        }
        let _ = release_prepared_bridge_frame(self.gpu.as_ref(), self.pipeline.as_mut());
        self.shutdown_resources_released = true;
    }

    fn begin_shutdown(&mut self, event_loop: &ActiveEventLoop) {
        self.release_prepared_if_needed();
        self.transition_to(self.state.begin_close());
        event_loop.exit();
    }

    fn exit_with_fatal(&mut self, error: AppError, event_loop: &ActiveEventLoop) {
        self.set_fatal(error);
        self.release_prepared_if_needed();
        event_loop.exit();
    }

    fn request_redraw_if_window_live(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn update_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pipeline) = &mut self.pipeline
            && self.state == AppState::Playing
            && let Some(deadline) = pipeline.next_wait_deadline()
        {
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn handle_eof(&mut self, event_loop: &ActiveEventLoop) {
        match self.state.eof_reached() {
            Ok(next) => self.transition_to(next),
            Err(error) => {
                self.exit_with_fatal(error, event_loop);
                return;
            }
        }

        if self.config.smoke_post_eof_resize() {
            self.smoke_post_eof = Some(SmokePostEofPhase::Resize1000x700);
            if let Some(window) = &self.window {
                let _ = window.request_inner_size(PhysicalSize::new(1000, 700));
            }
            self.request_redraw_if_window_live();
            return;
        }

        if matches!(
            self.config.run_mode(),
            RunMode::SmokeTest {
                post_eof_resize: false,
            }
        ) {
            self.print_smoke_summary();
            self.begin_shutdown(event_loop);
        } else {
            self.update_control_flow(event_loop);
            self.request_redraw_if_window_live();
        }
    }

    fn drive_playback(&mut self, event_loop: &ActiveEventLoop) {
        if self.pipeline.is_none() || self.state != AppState::Playing {
            return;
        }
        if self.gpu.is_none() || self.surface.is_none() {
            return;
        }
        let size = self
            .window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or_default();

        loop {
            let tick = {
                let gpu = self.gpu.as_ref().expect("gpu checked");
                let surface = self.surface.as_ref().expect("surface checked");
                let overlay = self.egui.as_mut();
                let pipeline = self.pipeline.as_mut().expect("pipeline checked");
                pipeline.tick(gpu, surface, size, overlay)
            };
            match tick {
                TickResult::Idle | TickResult::Waiting => {
                    self.update_control_flow(event_loop);
                    self.request_redraw_if_window_live();
                    break;
                }
                TickResult::Presented => continue,
                TickResult::SurfaceRetry(error) => {
                    eprintln!("surface retry: {error}");
                    self.update_control_flow(event_loop);
                    break;
                }
                TickResult::Finished => {
                    self.handle_eof(event_loop);
                    break;
                }
                TickResult::Fatal(error) => {
                    self.exit_with_fatal(error, event_loop);
                    break;
                }
            }
        }
    }

    fn render_display_frame(&mut self, event_loop: &ActiveEventLoop) {
        if !can_render_display_frame(self.state) {
            return;
        }
        if self.gpu.is_none() || self.surface.is_none() || self.pipeline.is_none() {
            return;
        }

        let window_size = self
            .window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or_default();
        if window_size.width == 0 || window_size.height == 0 {
            return;
        }

        let render_result = {
            let gpu = self.gpu.as_ref().expect("gpu checked");
            let surface = self.surface.as_ref().expect("surface checked");
            let overlay = self.egui.as_mut();
            let pipeline = self.pipeline.as_mut().expect("pipeline checked");
            pipeline.render_current_display_frame(gpu, surface, overlay)
        };

        match render_result {
            Ok(fit) => {
                if matches!(
                    self.smoke_post_eof,
                    Some(SmokePostEofPhase::AwaitRedraw1 | SmokePostEofPhase::AwaitRedraw2)
                ) {
                    self.smoke_aspect_fits
                        .push((window_size.width, window_size.height, fit));
                    self.advance_smoke_post_eof(event_loop);
                }
            }
            Err(AppError::Render(error))
                if matches!(
                    error,
                    dvs_render::RenderError::SurfaceLost
                        | dvs_render::RenderError::SurfaceOutdated
                        | dvs_render::RenderError::SurfaceTimeout
                ) =>
            {
                if !matches!(error, dvs_render::RenderError::SurfaceTimeout) {
                    eprintln!("surface retry: {error}");
                }
            }
            Err(AppError::Render(dvs_render::RenderError::SurfaceOutOfMemory)) => {
                self.exit_with_fatal(
                    AppError::Fatal("surface out of memory".to_string()),
                    event_loop,
                );
            }
            Err(error) => {
                self.exit_with_fatal(error, event_loop);
            }
        }
    }

    fn advance_smoke_post_eof(&mut self, event_loop: &ActiveEventLoop) {
        let Some(phase) = self.smoke_post_eof else {
            return;
        };
        match phase {
            SmokePostEofPhase::AwaitRedraw1 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::Resize1600x600);
                if let Some(window) = &self.window {
                    let _ = window.request_inner_size(PhysicalSize::new(1600, 600));
                }
            }
            SmokePostEofPhase::AwaitRedraw2 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::Complete);
                self.print_smoke_summary();
                self.begin_shutdown(event_loop);
            }
            _ => {}
        }
    }

    fn handle_smoke_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(phase) = self.smoke_post_eof else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        match phase {
            SmokePostEofPhase::Resize1000x700 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::AwaitRedraw1);
                self.request_redraw_if_window_live();
            }
            SmokePostEofPhase::Resize1600x600 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::AwaitRedraw2);
                self.request_redraw_if_window_live();
            }
            _ => {}
        }
    }

    fn print_smoke_summary(&self) {
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        let Some(gpu) = &self.gpu else {
            return;
        };
        let states = self
            .state_log
            .iter()
            .map(|state| format!("{state:?}"))
            .collect::<Vec<_>>()
            .join(" -> ");
        println!("state_transitions: {states}");
        if let Some(luid) = gpu.adapter_identity().dxgi_luid() {
            println!("adapter_luid: {luid:?}");
        }
        println!(
            "{}",
            format_metrics_summary(pipeline.metrics(), pipeline.time_base_summary())
        );
        if let Some(first) = pipeline.first_pts() {
            println!("first_pts: {}", first.pts());
        }
        if let Some(last) = pipeline.last_pts() {
            println!("last_pts: {}", last.pts());
        }
        println!("playback_started: {}", pipeline.playback_started());
        println!("eof: {}", pipeline.eof());
        println!(
            "held_display_frame: {} (frame_id={})",
            pipeline.has_held_display_frame(),
            pipeline
                .held_display_metadata()
                .map(|metadata| metadata.frame_id().value())
                .unwrap_or(u64::MAX)
        );
        println!(
            "decode_calls_after_eof: {}",
            pipeline.decode_calls_after_eof()
        );
        let (texture_handles, fence_handles) = pipeline.bridge_handle_open_counts();
        println!("bridge_handle_opens: texture={texture_handles}, fence={fence_handles}");
        let stats = pipeline.renderer_resource_stats();
        println!(
            "renderer_resources: shader_modules={}, render_pipelines={}, bind_groups={}, samplers={}",
            stats.shader_modules, stats.render_pipelines, stats.bind_groups, stats.samplers
        );
        for (index, (width, height, fit)) in self.smoke_aspect_fits.iter().enumerate() {
            println!(
                "post_eof_resize_{index}: window={width}x{height} viewport=({},{} {}x{}) scissor=({},{} {}x{})",
                fit.x, fit.y, fit.width, fit.height, fit.x, fit.y, fit.width, fit.height
            );
        }
    }

    fn start_playback_once(&mut self, event_loop: &ActiveEventLoop) -> Result<(), AppError> {
        let next = self.state.start_playback()?;
        let pipeline = self.pipeline.as_mut().ok_or(AppError::InvalidState)?;
        pipeline.start_playback()?;
        self.transition_to(next);
        self.drive_playback(event_loop);
        Ok(())
    }
}

impl ApplicationHandler for VideoWindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || self.fatal_error.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title(self.config.window_title())
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.exit_with_fatal(AppError::Window(err.to_string()), event_loop);
                return;
            }
        };

        let input = self.config.input().to_path_buf();
        let window_for_gpu = window.clone();
        let init_result = pollster::block_on(async {
            let (gpu, surface) = initialize_gpu(window_for_gpu).await?;
            let pipeline = VideoPipeline::bootstrap(&input, &gpu, &surface)?;
            Ok::<_, AppError>((gpu, surface, pipeline))
        });

        match init_result {
            Ok((gpu, surface, pipeline)) => {
                let egui = EguiStaticOverlay::new(
                    window.clone(),
                    gpu.device(),
                    surface.configuration().format,
                );
                self.window = Some(window.clone());
                self.pipeline = Some(pipeline);
                self.egui = Some(egui);
                self.surface = Some(surface);
                self.gpu = Some(gpu);
                match self.state.ready() {
                    Ok(next) => self.transition_to(next),
                    Err(error) => {
                        self.exit_with_fatal(error, event_loop);
                        return;
                    }
                }

                if matches!(self.config.run_mode(), RunMode::SmokeTest { .. }) {
                    if let Err(error) = self.start_playback_once(event_loop) {
                        self.exit_with_fatal(error, event_loop);
                        return;
                    }
                } else {
                    println!("Press SPACE to start playback");
                }

                window.request_redraw();
            }
            Err(error) => {
                self.exit_with_fatal(error, event_loop);
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Accumulate egui input for the next stable Integration 7 render opportunity.
        // Intentionally ignore EventResponse::{repaint, consumed} — no egui-driven redraw.
        if let Some(egui) = self.egui.as_mut() {
            let _ = egui.on_window_event(&event);
        }

        match event {
            WindowEvent::CloseRequested => self.begin_shutdown(event_loop),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => self.begin_shutdown(event_loop),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::Space),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                if self.state == AppState::Ready
                    && let Err(error) = self.start_playback_once(event_loop)
                {
                    self.exit_with_fatal(error, event_loop);
                }
            }
            WindowEvent::Resized(size) => {
                if let (Some(gpu), Some(surface), Some(pipeline)) =
                    (&self.gpu, &mut self.surface, &mut self.pipeline)
                    && size.width > 0
                    && size.height > 0
                    && surface.resize(gpu, size.width, size.height).is_ok()
                {
                    pipeline.record_surface_reconfiguration();
                    self.handle_smoke_resize(size);
                    if matches!(self.state, AppState::Ready | AppState::Ended) {
                        self.request_redraw_if_window_live();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if self.state == AppState::Playing {
                    self.drive_playback(event_loop);
                } else if can_render_display_frame(self.state) {
                    self.render_display_frame(event_loop);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.state == AppState::Playing {
            self.drive_playback(event_loop);
        }
    }
}
