//! winit event-loop application for the production editor shell.

use std::sync::Arc;
use std::time::Instant;

use dvs_render::AspectFitRect;
use dvs_ui::UiIntent;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, KeyEvent, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

use crate::config::{AppConfig, RunMode, event_loop_allows_any_thread};
use crate::display::can_render_display_frame;
use crate::egui_shell::{EguiEditorShell, collect_intents, space_blocked_by_egui};
use crate::error::AppError;
use crate::event_loop_schedule::{ControlFlowAction, EventLoopSchedule};
use crate::loop_metrics::{LoopMetrics, RedrawRequestSource};
use crate::metrics_summary::format_metrics_summary;
use crate::repaint::egui_delay_requests_immediate_redraw;
use crate::resize_diagnostic::{
    DiagnosticSnapshot, ResizeDiagnostic, control_flow_action_label, start_cause_label,
};
use crate::shutdown::release_prepared_bridge_frame;
use crate::state::AppState;
use crate::view_model::build_editor_view_model;
use crate::windows::gpu_surface::initialize_gpu;
use crate::windows::video_pipeline::{PlaybackStep, VideoPipeline};

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
    if let Some(mut diag) = app.diagnostic.take() {
        let _ = diag.finish();
    }
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

/// Active-playback resize validation for the smoke test harness.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum SmokePlayingResizePhase {
    AwaitFrames,
    Resize1000x700,
    AwaitPresent1,
    Resize1600x600,
    AwaitPresent2,
    Done,
}

const SMOKE_PLAYING_RESIZE_FRAME_THRESHOLD: u64 = 30;
const SMOKE_MIN_WALL_MEDIA_RATIO: f64 = 0.85;
const SMOKE_MAX_WALL_MEDIA_RATIO: f64 = 1.50;
const SMOKE_MAX_LATE_DROPS: u64 = 3;
const SMOKE_MAX_SURFACE_RECONFIGURATIONS: u64 = 12;
/// Integration 6 realtime playback records ~640k early waits with its redraw-on-wait loop.
/// A healthy editor shell must stay orders of magnitude below that.
const SMOKE_MAX_EARLY_WAITS: u64 = 10_000;
const SMOKE_MAX_SCHEDULER_EVALUATIONS: u64 = 12_000;

/// Application/window handler composition root.
///
/// # Drop order
///
/// Rust drops struct fields in declaration order. `pipeline` and `egui_shell` are
/// declared before `surface` and `gpu` so decode/bridge teardown and egui renderer
/// destruction run while the presentation device remains available.
struct VideoWindowApp {
    config: AppConfig,
    state: AppState,
    state_log: Vec<AppState>,
    window: Option<Arc<Window>>,
    pipeline: Option<VideoPipeline>,
    egui_shell: Option<EguiEditorShell>,
    surface: Option<dvs_render::RenderSurface>,
    gpu: Option<dvs_gpu::GpuContext>,
    fatal_error: Option<AppError>,
    smoke_post_eof: Option<SmokePostEofPhase>,
    smoke_playing_resize: Option<SmokePlayingResizePhase>,
    smoke_aspect_fits: Vec<(u32, u32, AspectFitRect)>,
    smoke_playing_aspect_fits: Vec<(u32, u32, AspectFitRect)>,
    smoke_started_at: Option<Instant>,
    shutdown_resources_released: bool,
    schedule: EventLoopSchedule,
    loop_metrics: Option<LoopMetrics>,
    diagnostic: Option<ResizeDiagnostic>,
}

impl VideoWindowApp {
    fn new(config: AppConfig) -> Self {
        let diagnose_resize = config.diagnose_resize();
        Self {
            config,
            state: AppState::Initializing,
            state_log: vec![AppState::Initializing],
            window: None,
            pipeline: None,
            egui_shell: None,
            surface: None,
            gpu: None,
            fatal_error: None,
            smoke_post_eof: None,
            smoke_playing_resize: None,
            smoke_aspect_fits: Vec::with_capacity(2),
            smoke_playing_aspect_fits: Vec::with_capacity(2),
            smoke_started_at: None,
            shutdown_resources_released: false,
            schedule: EventLoopSchedule::default(),
            loop_metrics: None,
            diagnostic: Self::open_diagnostic(diagnose_resize),
        }
    }

    fn open_diagnostic(enabled: bool) -> Option<ResizeDiagnostic> {
        if !enabled {
            return None;
        }
        match ResizeDiagnostic::open() {
            Ok(diag) => Some(diag),
            Err(error) => {
                eprintln!("failed to open resize diagnostic log: {error}");
                None
            }
        }
    }

    fn build_diagnostic_snapshot(
        app: &VideoWindowApp,
        diag: &ResizeDiagnostic,
        kind: &str,
        start_cause: Option<String>,
        extra: &str,
    ) -> DiagnosticSnapshot {
        let now = Instant::now();
        let window_size = app.window_size();
        let scale_factor = app
            .window
            .as_ref()
            .map(|window| window.scale_factor())
            .unwrap_or(1.0);
        let (surface_width, surface_height, surface_configured) =
            app.surface.as_ref().map_or((0, 0, false), |surface| {
                let config = surface.configuration();
                (
                    config.width,
                    config.height,
                    config.width > 0 && config.height > 0,
                )
            });
        let (
            schedule_playback_deadline_ms,
            schedule_egui_deadline_ms,
            schedule_playback_due,
            schedule_ui_redraw_due,
            schedule_surface_redraw_due,
            schedule_redraw_requested,
            schedule_egui_coalesce_outstanding,
        ) = app.schedule.diagnostic_fields(now);
        let selected_control_flow = control_flow_action_label(app.schedule.control_flow(now));

        let mut snapshot = DiagnosticSnapshot {
            kind: kind.to_string(),
            start_cause,
            app_state: app.state,
            window_width: window_size.width,
            window_height: window_size.height,
            surface_width,
            surface_height,
            scale_factor,
            surface_configured,
            redraw_outstanding: schedule_redraw_requested || schedule_egui_coalesce_outstanding,
            schedule_playback_deadline_ms,
            schedule_egui_deadline_ms,
            schedule_playback_due,
            schedule_ui_redraw_due,
            schedule_surface_redraw_due,
            schedule_redraw_requested,
            schedule_egui_coalesce_outstanding,
            selected_control_flow,
            last_scheduler_decision: diag.last_scheduler_decision().to_string(),
            last_surface_acquire: diag.last_surface_acquire().to_string(),
            display_only_redraw_count: diag.display_only_redraw_count(),
            last_submit_ms: diag.last_submit_ms(),
            last_present_ms: diag.last_present_ms(),
            process_work_depth: diag.process_work_depth(),
            extra: extra.to_string(),
            ..DiagnosticSnapshot::default()
        };

        if let Some(pipeline) = app.pipeline.as_ref() {
            let (prepared_present, prepared_frame_id, prepared_pts) =
                pipeline.diagnostic_prepared();
            let metrics = pipeline.metrics();
            snapshot.prepared_present = prepared_present;
            snapshot.prepared_frame_id = prepared_frame_id;
            snapshot.prepared_pts = prepared_pts;
            snapshot.bridge_state = pipeline.diagnostic_bridge_state();
            snapshot.playback_clock_state = pipeline.diagnostic_clock_state();
            snapshot.media_position_us = pipeline.diagnostic_media_position_us();
            snapshot.frames_decoded = metrics.frames_decoded();
            snapshot.frames_presented = metrics.frames_presented();
            snapshot.frames_dropped_late = metrics.frames_dropped_late();
            snapshot.frames_rejected = metrics.frames_rejected_timestamp();
            snapshot.eof = pipeline.eof();
        }

        snapshot
    }

    fn diag_log(&mut self, kind: &str, extra: &str) {
        let Some(mut diag) = self.diagnostic.take() else {
            return;
        };
        let snapshot = Self::build_diagnostic_snapshot(self, &diag, kind, None, extra);
        let _ = diag.log_snapshot(snapshot);
        self.diagnostic = Some(diag);
    }

    fn diag_log_start(&mut self, kind: &str, cause: &StartCause, extra: &str) {
        let Some(mut diag) = self.diagnostic.take() else {
            return;
        };
        let snapshot = Self::build_diagnostic_snapshot(
            self,
            &diag,
            kind,
            Some(start_cause_label(cause)),
            extra,
        );
        let _ = diag.log_snapshot(snapshot);
        self.diagnostic = Some(diag);
    }

    fn diag_f8_snapshot(&mut self) {
        let Some(mut diag) = self.diagnostic.take() else {
            return;
        };
        let snapshot =
            Self::build_diagnostic_snapshot(self, &diag, "stall_snapshot_f8", None, "F8");
        let _ = diag.write_stall_snapshot_f8(snapshot);
        self.diagnostic = Some(diag);
    }

    const fn smoke_loop_metrics_enabled(&self) -> bool {
        matches!(self.config.run_mode(), RunMode::SmokeTest { .. })
    }

    fn transition_to(&mut self, next: AppState) {
        if self.state != next {
            let from = self.state;
            self.state = next;
            self.state_log.push(next);
            if self.diagnostic.is_some() {
                self.diag_log("state_transition", &format!("{from:?}->{next:?}"));
            }
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

    fn request_redraw(&mut self, source: RedrawRequestSource) {
        let coalesce = matches!(
            source,
            RedrawRequestSource::EguiEventResponse | RedrawRequestSource::EguiZeroRepaintDelay
        );
        if !self.schedule.request_redraw(coalesce) {
            if let Some(mut diag) = self.diagnostic.take() {
                diag.record_redraw_coalesced();
                self.diagnostic = Some(diag);
            }
            if self.diagnostic.is_some() {
                self.diag_log("redraw_coalesced", &format!("{source:?}"));
            }
            return;
        }
        if self.smoke_loop_metrics_enabled()
            && let Some(metrics) = &mut self.loop_metrics
        {
            metrics.record_request_redraw(source);
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        self.diag_log("redraw_requested", &format!("{source:?}"));
    }

    fn record_egui_repaint_delay(&mut self, delay: Option<std::time::Duration>) {
        let Some(delay) = delay else {
            self.schedule.clear_egui_deadline();
            return;
        };
        if self.smoke_loop_metrics_enabled()
            && let Some(metrics) = &mut self.loop_metrics
        {
            metrics.record_egui_repaint_delay(delay);
        }
        let now = Instant::now();
        if egui_delay_requests_immediate_redraw(delay) {
            self.schedule.note_egui_repaint_delay(delay, now);
            self.request_redraw(RedrawRequestSource::EguiZeroRepaintDelay);
        } else {
            self.schedule.note_egui_repaint_delay(delay, now);
        }
    }

    /// Returns whether a successful surface resize should schedule a redraw.
    const fn should_redraw_after_resize(state: AppState) -> bool {
        matches!(state, AppState::Ready | AppState::Playing | AppState::Ended)
    }

    fn sync_playback_schedule(&mut self, now: Instant) {
        if self.state == AppState::Playing {
            if let Some(pipeline) = self.pipeline.as_ref() {
                self.schedule.sync_playback(
                    pipeline.playback_wakeup_deadline(),
                    pipeline.is_playback_frame_due(),
                    now,
                );
            }
        } else {
            self.schedule.clear_playback();
        }
    }

    fn finish_frame_schedule(&mut self) {
        let now = Instant::now();
        self.sync_playback_schedule(now);
        self.schedule.refresh(now);
    }

    fn apply_control_flow(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        self.sync_playback_schedule(now);
        self.schedule.refresh(now);

        match self.schedule.control_flow(now) {
            ControlFlowAction::WaitUntil(deadline) => {
                if self.smoke_loop_metrics_enabled()
                    && let Some(metrics) = &mut self.loop_metrics
                {
                    metrics.record_control_flow_wait_until(deadline, now);
                }
                event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            }
            ControlFlowAction::Wait => {
                if self.smoke_loop_metrics_enabled()
                    && let Some(metrics) = &mut self.loop_metrics
                {
                    metrics.record_control_flow_wait();
                }
                event_loop.set_control_flow(ControlFlow::Wait);
            }
        }
        self.diag_log(
            "about_to_wait_control_flow",
            &control_flow_action_label(self.schedule.control_flow(now)),
        );
    }

    fn handle_surface_retry(&mut self, event_loop: &ActiveEventLoop) {
        self.schedule.mark_surface_redraw_due();
        self.request_redraw(RedrawRequestSource::SurfaceRetry);
        self.apply_control_flow(event_loop);
    }

    /// Composes the current held/prepared display frame without advancing playback.
    fn compose_playing_display(&mut self, event_loop: &ActiveEventLoop) -> Result<bool, AppError> {
        if self.state != AppState::Playing {
            self.schedule.clear_surface_redraw_due();
            return Ok(false);
        }
        let size = self.window_size();
        if size.width == 0 || size.height == 0 {
            return Ok(false);
        }

        match self.compose_frame(event_loop, true, None) {
            Ok(_) => {
                if let Some(diag) = &mut self.diagnostic {
                    diag.record_display_only_redraw();
                }
                self.diag_log("display_only_redraw", "compose_playing_display");
                Ok(true)
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
                self.schedule.mark_surface_redraw_due();
                self.request_redraw(RedrawRequestSource::SurfaceRetry);
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    fn process_scheduled_work(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(mut diag) = self.diagnostic.take() {
            diag.process_work_enter();
            self.diagnostic = Some(diag);
        }
        if self.diagnostic.is_some() {
            self.diag_log("process_scheduled_work_enter", "-");
        }

        let now = Instant::now();
        let playback_due_before = self.schedule.playback_due();
        let ui_due_before = self.schedule.ui_redraw_due();
        self.sync_playback_schedule(now);
        self.schedule.refresh(now);
        let log_playback_promoted = !playback_due_before && self.schedule.playback_due();
        let log_ui_promoted = !ui_due_before && self.schedule.ui_redraw_due();
        if log_playback_promoted {
            if let Some(diag) = &mut self.diagnostic {
                diag.record_deadline_promoted("playback");
            }
            self.diag_log("deadline_promoted", "playback");
        }
        if log_ui_promoted {
            if let Some(diag) = &mut self.diagnostic {
                diag.record_deadline_promoted("egui");
            }
            self.diag_log("deadline_promoted", "egui");
        }

        const MAX_ROUNDS: usize = 4;
        for _ in 0..MAX_ROUNDS {
            if !self
                .schedule
                .has_immediate_work(self.state == AppState::Playing)
            {
                break;
            }

            if self.state == AppState::Playing && self.schedule.consume_playback_due() {
                self.drive_playback(event_loop);
                continue;
            }

            if self.schedule.surface_redraw_due() {
                if self.state == AppState::Playing {
                    match self.compose_playing_display(event_loop) {
                        Ok(true) => self.schedule.clear_surface_redraw_due(),
                        Ok(false) => {}
                        Err(AppError::Render(dvs_render::RenderError::SurfaceOutOfMemory)) => {
                            self.exit_with_fatal(
                                AppError::Fatal("surface out of memory".to_string()),
                                event_loop,
                            );
                            return;
                        }
                        Err(error) => {
                            self.exit_with_fatal(error, event_loop);
                            return;
                        }
                    }
                } else if can_render_display_frame(self.state) {
                    self.render_display_frame(event_loop);
                    self.schedule.clear_surface_redraw_due();
                } else {
                    self.schedule.clear_surface_redraw_due();
                }
                continue;
            }

            if self.schedule.ui_redraw_due() || self.schedule.redraw_requested() {
                if self.state == AppState::Playing {
                    if self
                        .pipeline
                        .as_ref()
                        .is_some_and(VideoPipeline::is_waiting_for_prepared_present)
                    {
                        let _ = self.compose_playing_display(event_loop);
                    } else {
                        self.drive_playback(event_loop);
                    }
                } else if can_render_display_frame(self.state) {
                    self.render_display_frame(event_loop);
                }
                self.schedule.clear_ui_redraw_due();
                self.schedule.clear_redraw_request();
                continue;
            }

            break;
        }

        self.apply_control_flow(event_loop);
        if let Some(mut diag) = self.diagnostic.take() {
            diag.process_work_exit();
            self.diagnostic = Some(diag);
        }
        if self.diagnostic.is_some() {
            self.diag_log("process_scheduled_work_exit", "-");
        }
    }

    fn window_size(&self) -> PhysicalSize<u32> {
        self.window
            .as_ref()
            .map(|window| window.inner_size())
            .unwrap_or_default()
    }

    fn handle_ui_intents(&mut self, event_loop: &ActiveEventLoop, intents: Vec<UiIntent>) {
        for intent in intents {
            match intent {
                UiIntent::StartPlayback => {
                    if let Err(error) = self.start_playback_once(event_loop) {
                        self.exit_with_fatal(error, event_loop);
                    }
                }
                UiIntent::CloseRequested => self.begin_shutdown(event_loop),
            }
        }
    }

    fn compose_frame(
        &mut self,
        event_loop: &ActiveEventLoop,
        encode_video: bool,
        playback_present: Option<dvs_playback::MediaTimeUs>,
    ) -> Result<Option<AspectFitRect>, AppError> {
        let window = self.window.as_ref().ok_or(AppError::InvalidState)?.clone();
        let window_size = window.inner_size();
        if window_size.width == 0 || window_size.height == 0 {
            return Ok(None);
        }

        let record_loop_metrics = self.smoke_loop_metrics_enabled();
        let model = build_editor_view_model(&self.config, self.state, self.pipeline.as_ref());

        let surface = self.surface.as_ref().ok_or(AppError::InvalidState)?;

        let (ui_output, egui_repaint_delay, monitor, target_width, target_height) = {
            let shell = self.egui_shell.as_mut().ok_or(AppError::InvalidState)?;
            shell.begin_frame(&window);
            let ui_output = shell.show_editor(&model);
            let _platform_output = shell.end_frame(&window);
            let egui_repaint_delay = shell.last_repaint_delay();

            let surface_config = surface.configuration();
            let target_width = surface_config.width;
            let target_height = surface_config.height;

            let monitor = shell
                .program_monitor_physical(target_width, target_height)
                .ok_or_else(|| AppError::Fatal("program monitor rect is empty".to_string()))?;

            (
                ui_output,
                egui_repaint_delay,
                monitor,
                target_width,
                target_height,
            )
        };

        let prepared = {
            let pipeline = self.pipeline.as_mut().ok_or(AppError::InvalidState)?;
            if playback_present.is_some() {
                pipeline.take_prepared_for_present()
            } else {
                None
            }
        };

        let acquire = {
            let gpu = self.gpu.as_ref().ok_or(AppError::InvalidState)?;
            match surface.acquire_frame(gpu) {
                Ok(value) => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_surface_acquire("ok".to_string());
                    }
                    Ok(value)
                }
                Err(error) => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_surface_acquire(format!("{error}"));
                    }
                    Err(error)
                }
            }
        };
        let (surface_texture, target_view) = match acquire {
            Ok(value) => {
                self.diag_log("surface_acquired", "ok");
                value
            }
            Err(error) => {
                self.diag_log("surface_acquire_error", &format!("{error}"));
                if let Some(prepared) = prepared
                    && let Some(pipeline) = self.pipeline.as_mut()
                {
                    pipeline.restore_prepared(prepared);
                }
                return Err(AppError::Render(error));
            }
        };

        let (fit, bridge_consumed_extra) = {
            let gpu = self.gpu.as_ref().ok_or(AppError::InvalidState)?;
            let mut encoder = EguiEditorShell::create_frame_encoder(gpu);
            EguiEditorShell::encode_background_clear(&mut encoder, &target_view);

            let mut fit = None;
            if encode_video {
                let pipeline = self.pipeline.as_mut().ok_or(AppError::InvalidState)?;
                fit = Some(match (prepared.as_ref(), playback_present) {
                    (Some(prepared_frame), Some(_)) => pipeline.encode_prepared_in_rect(
                        gpu,
                        &mut encoder,
                        &target_view,
                        target_width,
                        target_height,
                        monitor,
                        prepared_frame,
                    )?,
                    _ => pipeline.encode_display_in_rect(
                        gpu,
                        &mut encoder,
                        &target_view,
                        target_width,
                        target_height,
                        monitor,
                    )?,
                });
            }

            {
                let shell = self.egui_shell.as_mut().ok_or(AppError::InvalidState)?;
                shell.encode_ui(
                    gpu,
                    &mut encoder,
                    &target_view,
                    target_width,
                    target_height,
                    wgpu::LoadOp::Load,
                )?;
            }

            gpu.queue().submit(Some(encoder.finish()));
            if record_loop_metrics && let Some(metrics) = &mut self.loop_metrics {
                metrics.record_queue_submission();
            }
            if let Some(diag) = &mut self.diagnostic {
                diag.record_queue_submit();
            }

            let bridge_consumed_extra = match (prepared, playback_present) {
                (Some(prepared), Some(lateness)) => {
                    let pipeline = self.pipeline.as_mut().ok_or(AppError::InvalidState)?;
                    pipeline.finalize_present(gpu, prepared, lateness)?;
                    let frame_id = pipeline
                        .metrics()
                        .last_presented_frame_id()
                        .map(|id| id.value());
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision("present_finalize");
                    }
                    Some(format!("frame_id={frame_id:?}"))
                }
                (Some(prepared), None) => {
                    if let Some(pipeline) = self.pipeline.as_mut() {
                        pipeline.restore_prepared(prepared);
                    }
                    None
                }
                (None, _) => None,
            };

            Ok::<_, AppError>((fit, bridge_consumed_extra))
        }?;
        self.diag_log("queue_submitted", "-");
        if let Some(extra) = bridge_consumed_extra {
            self.diag_log("bridge_consumed", &extra);
        }

        surface_texture.present();
        if record_loop_metrics && let Some(metrics) = &mut self.loop_metrics {
            metrics.record_surface_present();
        }
        if let Some(diag) = &mut self.diagnostic {
            diag.record_surface_present();
        }
        self.diag_log("surface_presented", "-");
        self.handle_ui_intents(event_loop, collect_intents(&ui_output));
        if playback_present.is_some() {
            self.tick_smoke_playing_resize(event_loop);
            if let (Some(fit), Some(phase)) = (fit, self.smoke_playing_resize) {
                self.advance_smoke_playing_present(event_loop, window_size, fit, phase);
            }
        }
        self.record_egui_repaint_delay(egui_repaint_delay);
        self.finish_frame_schedule();
        Ok(fit)
    }

    fn handle_eof(&mut self, event_loop: &ActiveEventLoop) {
        self.diag_log("eof_reached", "-");
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
            self.request_redraw(RedrawRequestSource::PostEof);
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
            self.apply_control_flow(event_loop);
            self.request_redraw(RedrawRequestSource::PostEof);
        }
    }

    fn drive_playback(&mut self, event_loop: &ActiveEventLoop) {
        if self.state != AppState::Playing {
            return;
        }
        let size = self.window_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        loop {
            let early_before = self
                .pipeline
                .as_ref()
                .map(|pipeline| pipeline.metrics().early_wait_count())
                .unwrap_or(0);
            let step = self
                .pipeline
                .as_mut()
                .map(|pipeline| pipeline.evaluate_playback_step(self.gpu.as_ref().unwrap(), size))
                .unwrap_or(PlaybackStep::Idle);
            if self.diagnostic.is_some() {
                let notes = self
                    .pipeline
                    .as_mut()
                    .map(|pipeline| pipeline.take_diagnostic_notes())
                    .unwrap_or_default();
                for note in notes {
                    self.diag_log(&note.kind, &note.extra);
                }
            }
            if self.smoke_loop_metrics_enabled()
                && let (Some(metrics), Some(pipeline)) =
                    (&mut self.loop_metrics, self.pipeline.as_ref())
            {
                let early_wait = pipeline.metrics().early_wait_count() > early_before;
                metrics.record_scheduler_evaluation(early_wait);
            }

            match step {
                PlaybackStep::Idle => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision("Idle");
                    }
                    self.diag_log("scheduler_evaluated", "Idle");
                    break;
                }
                PlaybackStep::Waiting => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision("Waiting");
                    }
                    self.diag_log("scheduler_evaluated", "Waiting");
                    break;
                }
                PlaybackStep::Present { lateness } => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision(format!("Present lateness={lateness:?}"));
                    }
                    self.diag_log("scheduler_evaluated", &format!("Present {lateness:?}"));
                    match self.compose_frame(event_loop, true, Some(lateness)) {
                        Ok(_) => continue,
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
                            self.handle_surface_retry(event_loop);
                            break;
                        }
                        Err(AppError::Render(dvs_render::RenderError::SurfaceOutOfMemory)) => {
                            self.exit_with_fatal(
                                AppError::Fatal("surface out of memory".to_string()),
                                event_loop,
                            );
                            break;
                        }
                        Err(error) => {
                            self.exit_with_fatal(error, event_loop);
                            break;
                        }
                    }
                }
                PlaybackStep::Finished => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision("Finished");
                    }
                    self.diag_log("scheduler_evaluated", "Finished");
                    self.handle_eof(event_loop);
                    break;
                }
                PlaybackStep::Fatal(error) => {
                    if let Some(diag) = &mut self.diagnostic {
                        diag.set_scheduler_decision(format!("Fatal {error}"));
                    }
                    self.diag_log("scheduler_fatal", &format!("{error}"));
                    self.exit_with_fatal(error, event_loop);
                    break;
                }
            }
        }
        self.finish_frame_schedule();
    }

    fn render_display_frame(&mut self, event_loop: &ActiveEventLoop) {
        if !can_render_display_frame(self.state) {
            return;
        }
        let size = self.window_size();
        if size.width == 0 || size.height == 0 {
            return;
        }

        match self.compose_frame(event_loop, true, None) {
            Ok(fit) => {
                if let (Some(fit), Some(phase)) = (fit, self.smoke_post_eof)
                    && matches!(
                        phase,
                        SmokePostEofPhase::AwaitRedraw1 | SmokePostEofPhase::AwaitRedraw2
                    )
                {
                    self.smoke_aspect_fits.push((size.width, size.height, fit));
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
                self.handle_surface_retry(event_loop);
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
        self.handle_smoke_post_eof_resize(size);
        self.handle_smoke_playing_resize(size);
    }

    fn handle_smoke_post_eof_resize(&mut self, size: PhysicalSize<u32>) {
        let Some(phase) = self.smoke_post_eof else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        match phase {
            SmokePostEofPhase::Resize1000x700 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::AwaitRedraw1);
                self.request_redraw(RedrawRequestSource::PostEof);
            }
            SmokePostEofPhase::Resize1600x600 => {
                self.smoke_post_eof = Some(SmokePostEofPhase::AwaitRedraw2);
                self.request_redraw(RedrawRequestSource::PostEof);
            }
            _ => {}
        }
    }

    fn handle_smoke_playing_resize(&mut self, size: PhysicalSize<u32>) {
        if !self.config.smoke_post_eof_resize() {
            return;
        }
        let Some(phase) = self.smoke_playing_resize else {
            return;
        };
        if size.width == 0 || size.height == 0 {
            return;
        }
        match phase {
            SmokePlayingResizePhase::Resize1000x700 if size.width == 1000 && size.height == 700 => {
                self.smoke_playing_resize = Some(SmokePlayingResizePhase::AwaitPresent1);
                self.request_redraw(RedrawRequestSource::SmokePlayingResize);
            }
            SmokePlayingResizePhase::Resize1600x600 if size.width == 1600 && size.height == 600 => {
                self.smoke_playing_resize = Some(SmokePlayingResizePhase::AwaitPresent2);
                self.request_redraw(RedrawRequestSource::SmokePlayingResize);
            }
            _ => {}
        }
    }

    fn tick_smoke_playing_resize(&mut self, event_loop: &ActiveEventLoop) {
        if !self.config.smoke_post_eof_resize() {
            return;
        }
        if self.state != AppState::Playing {
            return;
        }
        let Some(phase) = self.smoke_playing_resize else {
            return;
        };
        if phase != SmokePlayingResizePhase::AwaitFrames {
            return;
        }
        let Some(pipeline) = &self.pipeline else {
            return;
        };
        if pipeline.metrics().frames_presented() < SMOKE_PLAYING_RESIZE_FRAME_THRESHOLD {
            return;
        }
        self.smoke_playing_resize = Some(SmokePlayingResizePhase::Resize1000x700);
        if let Some(window) = &self.window {
            let _ = window.request_inner_size(PhysicalSize::new(1000, 700));
        }
        self.request_redraw(RedrawRequestSource::SmokePlayingResize);
        let _ = event_loop;
    }

    fn advance_smoke_playing_present(
        &mut self,
        event_loop: &ActiveEventLoop,
        size: PhysicalSize<u32>,
        fit: AspectFitRect,
        phase: SmokePlayingResizePhase,
    ) {
        if !self.config.smoke_post_eof_resize() {
            return;
        }
        match phase {
            SmokePlayingResizePhase::AwaitPresent1 => {
                self.smoke_playing_aspect_fits
                    .push((size.width, size.height, fit));
                self.smoke_playing_resize = Some(SmokePlayingResizePhase::Resize1600x600);
                if let Some(window) = &self.window {
                    let _ = window.request_inner_size(PhysicalSize::new(1600, 600));
                }
            }
            SmokePlayingResizePhase::AwaitPresent2 => {
                self.smoke_playing_aspect_fits
                    .push((size.width, size.height, fit));
                self.smoke_playing_resize = Some(SmokePlayingResizePhase::Done);
            }
            _ => return,
        }
        self.request_redraw(RedrawRequestSource::SmokePlayingResize);
        let _ = event_loop;
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
        if let Some(shell) = &self.egui_shell
            && let Some(monitor) =
                shell.program_monitor_physical(self.window_size().width, self.window_size().height)
        {
            println!(
                "program_monitor_physical: x={} y={} w={} h={}",
                monitor.x, monitor.y, monitor.width, monitor.height
            );
        }
        for (index, (width, height, fit)) in self.smoke_playing_aspect_fits.iter().enumerate() {
            println!(
                "playing_resize_{index}: window={width}x{height} viewport=({},{} {}x{}) scissor=({},{} {}x{}) state=Playing",
                fit.x, fit.y, fit.width, fit.height, fit.x, fit.y, fit.width, fit.height
            );
        }
        if let Some(started) = self.smoke_started_at {
            let process_secs = started.elapsed().as_secs_f64();
            println!("smoke_process_duration_s: {process_secs:.3}");
            if let Some(media) = pipeline.metrics().playback_media_duration_us() {
                let media_secs = media.0 as f64 / 1_000_000.0;
                if media_secs > 0.0 {
                    let wall_secs = pipeline
                        .metrics()
                        .monotonic_wall_duration_us()
                        .map(|value| value.0 as f64 / 1_000_000.0)
                        .unwrap_or(0.0);
                    println!("playback_wall_duration_s: {wall_secs:.3}");
                    println!("playback_wall_media_ratio: {:.3}", wall_secs / media_secs);
                    if wall_secs > 0.0 {
                        println!(
                            "achieved_presented_fps: {:.2}",
                            pipeline.metrics().frames_presented() as f64 / wall_secs
                        );
                    }
                }
            }
        }
        for (index, (width, height, fit)) in self.smoke_aspect_fits.iter().enumerate() {
            println!(
                "post_eof_resize_{index}: window={width}x{height} viewport=({},{} {}x{}) scissor=({},{} {}x{})",
                fit.x, fit.y, fit.width, fit.height, fit.x, fit.y, fit.width, fit.height
            );
        }
        if let Some(loop_metrics) = &self.loop_metrics {
            loop_metrics.print_summary();
        }
        self.validate_smoke_outcome();
    }

    fn validate_aspect_fit_inside_window(
        label: &str,
        window_width: u32,
        window_height: u32,
        fit: AspectFitRect,
    ) {
        assert!(
            fit.width > 0 && fit.height > 0,
            "{label}: aspect-fit viewport must be non-empty"
        );
        assert!(
            fit.x + fit.width <= window_width && fit.y + fit.height <= window_height,
            "{label}: aspect-fit viewport ({},{},{}x{}) exceeds window {window_width}x{window_height}",
            fit.x,
            fit.y,
            fit.width,
            fit.height
        );
    }

    fn validate_smoke_outcome(&self) {
        if !matches!(
            self.config.run_mode(),
            RunMode::SmokeTest {
                post_eof_resize: true,
            }
        ) {
            return;
        }

        let pipeline = self.pipeline.as_ref().expect("smoke pipeline");
        assert!(pipeline.eof(), "smoke test must reach EOF");
        assert!(
            pipeline.playback_started(),
            "smoke test must start playback"
        );
        assert!(
            pipeline.metrics().frames_presented() > 0,
            "smoke test must present frames"
        );
        assert!(
            self.state_log.contains(&AppState::Playing),
            "smoke test must include Playing state"
        );
        assert!(
            matches!(
                self.smoke_playing_resize,
                Some(SmokePlayingResizePhase::Done) | None
            ),
            "active-playback resize phases must complete"
        );
        assert_eq!(
            self.smoke_playing_aspect_fits.len(),
            2,
            "expected two active-playback resized presents"
        );
        for (index, (width, height, fit)) in self.smoke_playing_aspect_fits.iter().enumerate() {
            Self::validate_aspect_fit_inside_window(
                &format!("playing_resize_{index}"),
                *width,
                *height,
                *fit,
            );
        }
        assert_eq!(
            self.smoke_aspect_fits.len(),
            2,
            "expected two post-EOF resized presents"
        );
        for (index, (width, height, fit)) in self.smoke_aspect_fits.iter().enumerate() {
            Self::validate_aspect_fit_inside_window(
                &format!("post_eof_resize_{index}"),
                *width,
                *height,
                *fit,
            );
        }

        let metrics = pipeline.metrics();
        assert!(
            metrics.frames_dropped_late() <= SMOKE_MAX_LATE_DROPS,
            "too many late drops: {} > {}",
            metrics.frames_dropped_late(),
            SMOKE_MAX_LATE_DROPS
        );
        assert!(
            metrics.surface_reconfigurations() <= SMOKE_MAX_SURFACE_RECONFIGURATIONS,
            "too many surface reconfigurations: {} > {}",
            metrics.surface_reconfigurations(),
            SMOKE_MAX_SURFACE_RECONFIGURATIONS
        );

        let expected_media = metrics
            .playback_media_duration_us()
            .expect("playback media duration");
        let wall = metrics
            .monotonic_wall_duration_us()
            .expect("monotonic wall duration");
        let ratio = wall.0 as f64 / expected_media.0 as f64;
        assert!(
            (SMOKE_MIN_WALL_MEDIA_RATIO..=SMOKE_MAX_WALL_MEDIA_RATIO).contains(&ratio),
            "playback wall/media ratio {ratio:.3} outside [{SMOKE_MIN_WALL_MEDIA_RATIO}, {SMOKE_MAX_WALL_MEDIA_RATIO}]"
        );

        let wall_secs = wall.0 as f64 / 1_000_000.0;
        if wall_secs > 0.0 {
            let sustained_fps = metrics.frames_presented() as f64 / wall_secs;
            assert!(
                sustained_fps <= 45.0,
                "decode-as-fast-as-possible suspected: {sustained_fps:.2} FPS"
            );
        }

        assert!(
            metrics.early_wait_count() <= SMOKE_MAX_EARLY_WAITS,
            "too many early waits: {} > {}",
            metrics.early_wait_count(),
            SMOKE_MAX_EARLY_WAITS
        );

        let loop_metrics = self.loop_metrics.as_ref().expect("smoke loop metrics");
        assert!(
            loop_metrics.scheduler_evaluations <= SMOKE_MAX_SCHEDULER_EVALUATIONS,
            "too many scheduler evaluations: {} > {}",
            loop_metrics.scheduler_evaluations,
            SMOKE_MAX_SCHEDULER_EVALUATIONS
        );
        assert_eq!(
            loop_metrics.control_flow_wait_until_expired, 0,
            "expired WaitUntil deadlines must not be selected"
        );
        assert!(
            loop_metrics.surface_presents >= metrics.frames_presented(),
            "every presented frame must submit at least one surface present"
        );

        let (texture_handles, fence_handles) = pipeline.bridge_handle_open_counts();
        assert_eq!(
            (texture_handles, fence_handles),
            (1, 1),
            "bridge handles must remain singleton"
        );
    }

    fn start_playback_once(&mut self, event_loop: &ActiveEventLoop) -> Result<(), AppError> {
        let next = self.state.start_playback()?;
        let pipeline = self.pipeline.as_mut().ok_or(AppError::InvalidState)?;
        pipeline.start_playback()?;
        self.transition_to(next);
        self.diag_log("playback_started", "-");
        self.drive_playback(event_loop);
        self.apply_control_flow(event_loop);
        Ok(())
    }
}

impl ApplicationHandler for VideoWindowApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.diag_log("application_resumed", "-");
        if self.window.is_some() || self.fatal_error.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title(self.config.window_title())
                .with_inner_size(LogicalSize::new(1280.0, 800.0))
                .with_min_inner_size(LogicalSize::new(
                    dvs_ui::MIN_WINDOW_WIDTH,
                    dvs_ui::MIN_WINDOW_HEIGHT,
                )),
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
                let shell = EguiEditorShell::new(&window, &gpu, surface.output_encoding().format);
                self.window = Some(window.clone());
                self.pipeline = Some(pipeline);
                self.egui_shell = Some(shell);
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
                    self.smoke_started_at = Some(Instant::now());
                    self.loop_metrics = Some(LoopMetrics::default());
                    self.smoke_playing_resize = if self.config.smoke_post_eof_resize() {
                        Some(SmokePlayingResizePhase::AwaitFrames)
                    } else {
                        None
                    };
                    if let Err(error) = self.start_playback_once(event_loop) {
                        self.exit_with_fatal(error, event_loop);
                        return;
                    }
                }

                self.request_redraw(RedrawRequestSource::Initialization);
                self.diag_log("initialization_complete", "-");
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
        if self.smoke_loop_metrics_enabled()
            && let Some(metrics) = &mut self.loop_metrics
        {
            metrics.record_window_event();
        }

        if let (Some(window), Some(shell)) = (&self.window, &mut self.egui_shell)
            && shell.on_window_event(window, &event).repaint
        {
            self.request_redraw(RedrawRequestSource::EguiEventResponse);
        }

        match event {
            WindowEvent::CloseRequested => {
                self.diag_log("close_requested", "-");
                self.begin_shutdown(event_loop);
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
                self.diag_log("keyboard_escape", "-");
                self.begin_shutdown(event_loop);
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key: Key::Named(NamedKey::F8),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } if self.diagnostic.is_some() => {
                self.diag_f8_snapshot();
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
                if self.state == AppState::Ready
                    && !self.egui_shell.as_ref().is_some_and(space_blocked_by_egui)
                    && let Err(error) = self.start_playback_once(event_loop)
                {
                    self.exit_with_fatal(error, event_loop);
                }
            }
            WindowEvent::Resized(size) => {
                self.diag_log("window_resized", &format!("{}x{}", size.width, size.height));
                if let (Some(gpu), Some(surface), Some(pipeline)) =
                    (&self.gpu, &mut self.surface, &mut self.pipeline)
                    && size.width > 0
                    && size.height > 0
                    && surface.resize(gpu, size.width, size.height).is_ok()
                {
                    pipeline.record_surface_reconfiguration();
                    self.diag_log(
                        "surface_reconfigured",
                        &format!("{}x{}", size.width, size.height),
                    );
                    self.handle_smoke_resize(size);
                    if Self::should_redraw_after_resize(self.state) {
                        self.schedule.mark_surface_redraw_due();
                        self.request_redraw(RedrawRequestSource::Resize);
                        self.process_scheduled_work(event_loop);
                    }
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.diag_log("scale_factor_changed", &format!("{scale_factor:.3}"));
            }
            WindowEvent::RedrawRequested => {
                self.diag_log("redraw_requested_event", "-");
                self.schedule.on_redraw_requested_event();
                if self.smoke_loop_metrics_enabled()
                    && let Some(metrics) = &mut self.loop_metrics
                {
                    metrics.record_redraw_requested_event();
                }
                self.process_scheduled_work(event_loop);
            }
            _ => {}
        }
    }

    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.diag_log_start("new_events", &cause, "-");
        match cause {
            StartCause::ResumeTimeReached { .. } | StartCause::WaitCancelled { .. } => {
                self.process_scheduled_work(event_loop);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.diag_log("about_to_wait", "-");
        if self.smoke_loop_metrics_enabled()
            && let Some(metrics) = &mut self.loop_metrics
        {
            metrics.record_about_to_wait();
        }
        self.process_scheduled_work(event_loop);
    }
}
