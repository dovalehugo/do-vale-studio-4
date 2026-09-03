//! Production Windows NV12 decode → bridge → render → playback pipeline.

use std::path::Path;
use std::time::Instant;

use dvs_decoder::DecoderSession;
use dvs_gpu::{
    FenceTimeline, FrameFenceValues, GpuContext, SharedNv12TextureDesc,
    WindowsD3d11SharedNv12Producer, WindowsD3d11WgpuInteropBridge,
};
use dvs_media::{MediaTimestamp, VideoFrameMetadata};
use dvs_playback::{
    FrameSchedulePlan, FrameScheduler, MediaTimeUs, PlaybackClock, PlaybackMetrics,
    ScheduleDecision, SchedulerConfig, media_duration_between,
};
use dvs_render::{
    AspectFitRect, Nv12Renderer, Nv12RendererConfig, Nv12RendererResourceStats, RenderSurface,
    aspect_fit_rect,
};
use winit::dpi::PhysicalSize;

use crate::egui_overlay::EguiStaticOverlay;
use crate::error::AppError;

struct PreparedFrame {
    metadata: VideoFrameMetadata,
    values: FrameFenceValues,
    schedule_plan: FrameSchedulePlan,
}

/// Production video pipeline owned by the application composition root.
///
/// # Drop order
///
/// Rust drops struct fields in declaration order. `session` is declared first and
/// therefore drops before `bridge`. This is safe because the producer's
/// `D3d11ExternalContextLock` owns an independent `D3d11ExternalContextLockKeepalive`
/// with a retained FFmpeg `AVBufferRef` (Integration 4B keepalive proof:
/// `producer_lock_survives_decoder_session_drop`). No bridge destructor invokes
/// FFmpeg lock callbacks whose owner disappeared with `DecoderSession`.
pub struct VideoPipeline {
    session: DecoderSession,
    bridge: WindowsD3d11WgpuInteropBridge,
    renderer: Nv12Renderer,
    timeline: FenceTimeline,
    clock: PlaybackClock,
    scheduler: FrameScheduler,
    metrics: PlaybackMetrics,
    prepared: Option<PreparedFrame>,
    /// Metadata for the last presented frame, retained for Ended-state redraws.
    held_display_metadata: Option<VideoFrameMetadata>,
    eof: bool,
    first_pts: Option<MediaTimestamp>,
    last_pts: Option<MediaTimestamp>,
    time_base_summary: Option<String>,
    playback_started: bool,
    decode_calls_after_eof: u64,
}

/// Result of one playback tick.
pub enum TickResult {
    Idle,
    Waiting,
    Presented,
    Finished,
    SurfaceRetry(AppError),
    Fatal(AppError),
}

/// Pure translation of a scheduler decision for an already-prepared frame.
///
/// Separates DropLate recovery from RejectTimestamp fatality without GPU mocks.
#[derive(Debug, Clone, Eq, PartialEq)]
enum PreparedFrameDecisionAction {
    Wait,
    Present {
        lateness: MediaTimeUs,
    },
    DiscardLate {
        lateness_us: i64,
        plan_target_us: i64,
        frame_id: u64,
    },
    FatalReject {
        reason: String,
        plan_target_us: i64,
        frame_id: u64,
    },
}

fn prepared_frame_decision_action(
    decision: ScheduleDecision,
    plan_target_us: i64,
    frame_id: u64,
) -> PreparedFrameDecisionAction {
    match decision {
        ScheduleDecision::WaitUntil { .. } => PreparedFrameDecisionAction::Wait,
        ScheduleDecision::PresentNow { lateness } => {
            PreparedFrameDecisionAction::Present { lateness }
        }
        ScheduleDecision::DropLate { lateness } => PreparedFrameDecisionAction::DiscardLate {
            lateness_us: lateness.0,
            plan_target_us,
            frame_id,
        },
        ScheduleDecision::RejectTimestamp(error) => PreparedFrameDecisionAction::FatalReject {
            reason: format!("{error:?}"),
            plan_target_us,
            frame_id,
        },
    }
}

impl VideoPipeline {
    /// Opens the decoder, bridge, and renderer using the first decoded frame for dimensions.
    pub fn bootstrap(
        input: &Path,
        gpu: &GpuContext,
        surface: &RenderSurface,
    ) -> Result<Self, AppError> {
        let wgpu_luid = gpu
            .adapter_identity()
            .dxgi_luid()
            .ok_or_else(|| AppError::Gpu(dvs_gpu::GpuError::DxgiAdapterLuidUnavailable))?;

        let mut session = DecoderSession::open(input, wgpu_luid)?;

        let (d3d11_device, d3d11_context) = {
            let hw = session.d3d11_hardware()?;
            (hw.device().clone(), hw.context().clone())
        };
        let external_context_lock = session.external_context_lock()?;

        let first = session.decode_next_d3d11()?.ok_or_else(|| {
            AppError::Fatal("input ended before the first decodable frame".to_string())
        })?;

        let first_metadata = first.metadata();
        let timestamp = first_metadata
            .timestamp()
            .ok_or_else(|| AppError::Playback(dvs_playback::PlaybackError::MissingTimestamp))?;
        let time_base = timestamp.time_base();
        let time_base_summary = format!("{}/{}", time_base.numerator(), time_base.denominator());

        let allocation = first_metadata.dimensions().allocation();
        let desc = SharedNv12TextureDesc::new(allocation.width(), allocation.height())?;
        let producer = WindowsD3d11SharedNv12Producer::new_with_external_lock(
            &d3d11_device,
            &d3d11_context,
            wgpu_luid,
            desc,
            Some(external_context_lock),
        )?;

        let mut bridge = WindowsD3d11WgpuInteropBridge::new(gpu, producer)?;

        let encoding = surface.output_encoding();
        let renderer = Nv12Renderer::new(
            gpu.device(),
            Nv12RendererConfig {
                target_format: encoding.format,
            },
        )?;

        let timeline = FenceTimeline::new();
        let values = timeline.current()?;
        let (first_frame_metadata, surface_ref) = first.into_parts();
        bridge.prepare_frame(gpu, surface_ref, values)?;

        let mut pipeline = Self {
            session,
            bridge,
            renderer,
            timeline,
            clock: PlaybackClock::new(),
            scheduler: FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default()),
            metrics: PlaybackMetrics::new(),
            prepared: None,
            held_display_metadata: None,
            eof: false,
            first_pts: Some(timestamp),
            last_pts: Some(timestamp),
            time_base_summary: Some(time_base_summary),
            playback_started: false,
            decode_calls_after_eof: 0,
        };

        pipeline.metrics.record_decoded();
        let schedule_plan = pipeline
            .scheduler
            .plan_frame(Some(timestamp), first_frame_metadata.frame_id())?;
        pipeline.prepared = Some(PreparedFrame {
            metadata: first_frame_metadata,
            values,
            schedule_plan,
        });
        Ok(pipeline)
    }

    /// Returns whether a held display frame is available for Ended-state redraws.
    pub const fn has_held_display_frame(&self) -> bool {
        self.held_display_metadata.is_some()
    }

    /// Returns decode calls attempted after EOF (must remain zero).
    pub const fn decode_calls_after_eof(&self) -> u64 {
        self.decode_calls_after_eof
    }

    /// Returns held display metadata when EOF presentation completed.
    pub const fn held_display_metadata(&self) -> Option<&VideoFrameMetadata> {
        self.held_display_metadata.as_ref()
    }
    pub const fn has_prepared_frame(&self) -> bool {
        self.prepared.is_some()
    }

    /// Records a presentation surface reconfiguration.
    pub fn record_surface_reconfiguration(&mut self) {
        self.metrics.record_surface_reconfiguration();
    }

    pub const fn playback_started(&self) -> bool {
        self.playback_started
    }

    /// Returns whether the decoder reached EOF.
    pub const fn eof(&self) -> bool {
        self.eof
    }

    /// Returns read-only access to playback metrics.
    pub const fn metrics(&self) -> &PlaybackMetrics {
        &self.metrics
    }

    /// Returns the first decoded PTS when available.
    pub const fn first_pts(&self) -> Option<MediaTimestamp> {
        self.first_pts
    }

    /// Returns the last presented PTS when available.
    pub const fn last_pts(&self) -> Option<MediaTimestamp> {
        self.last_pts
    }

    /// Returns the source time-base summary string.
    pub fn time_base_summary(&self) -> Option<&str> {
        self.time_base_summary.as_deref()
    }

    /// Returns shared-handle open counts from the interop bridge.
    pub fn bridge_handle_open_counts(&self) -> (u32, u32) {
        self.bridge.shared_handle_open_counts()
    }

    /// Returns renderer resource counts.
    pub fn renderer_resource_stats(&self) -> Nv12RendererResourceStats {
        self.renderer.resource_stats()
    }

    /// Starts PTS-driven playback. The first prepared frame is due immediately.
    pub fn start_playback(&mut self) -> Result<(), AppError> {
        if self.playback_started {
            return Err(AppError::InvalidState);
        }
        let timestamp = self
            .first_pts
            .ok_or_else(|| AppError::Playback(dvs_playback::PlaybackError::MissingTimestamp))?;
        self.clock.start(timestamp)?;
        self.playback_started = true;
        self.metrics.record_scheduled();
        Ok(())
    }

    /// Renders the current display frame without changing playback state.
    ///
    /// Uses the prepared bridge frame in `Ready`, or the held last-presented frame in
    /// `Ended` after the bridge slot was consumed.
    ///
    /// When `overlay` is provided, the editor shell is prepared once, NV12 is encoded
    /// into the Program Monitor region, then egui is encoded with `LoadOp::Load`
    /// (Integration 8A.2).
    pub fn render_current_display_frame(
        &mut self,
        gpu: &GpuContext,
        surface: &RenderSurface,
        mut overlay: Option<&mut EguiStaticOverlay>,
    ) -> Result<AspectFitRect, AppError> {
        if surface.configuration().width == 0 || surface.configuration().height == 0 {
            return Err(AppError::Render(
                dvs_render::RenderError::InvalidTargetDimensions,
            ));
        }

        let (metadata, video) = if let Some(prepared) = self.prepared.as_ref() {
            (prepared.metadata, self.bridge.prepared_frame()?)
        } else if self.eof {
            let metadata = *self
                .held_display_metadata
                .as_ref()
                .ok_or(AppError::InvalidState)?;
            (metadata, self.bridge.consumed_display_frame()?)
        } else {
            return Err(AppError::InvalidState);
        };

        let visible = metadata.dimensions().visible();
        let config = surface.configuration();

        let (surface_texture, target_view) = surface.acquire_frame(gpu)?;

        let monitor = if let Some(overlay) = overlay.as_deref_mut() {
            overlay.prepare_editor_frame(config.width, config.height)?
        } else {
            dvs_ui::PhysicalViewport {
                x: 0,
                y: 0,
                width: config.width,
                height: config.height,
            }
        };

        let destination = dvs_render::AspectFitRect {
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
        };

        let fit = if destination.width == 0 || destination.height == 0 {
            aspect_fit_rect(
                visible.width(),
                visible.height(),
                config.width,
                config.height,
            )?
        } else {
            let local = aspect_fit_rect(
                visible.width(),
                visible.height(),
                destination.width,
                destination.height,
            )?;
            AspectFitRect {
                x: destination.x + local.x,
                y: destination.y + local.y,
                width: local.width,
                height: local.height,
            }
        };

        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dvs-app-display"),
            });
        self.renderer.encode_frame_in_region(
            gpu.device(),
            gpu.queue(),
            &mut encoder,
            video,
            metadata,
            &target_view,
            config.width,
            config.height,
            destination,
        )?;
        if let Some(overlay) = overlay {
            overlay.encode_pending_after_video(
                gpu.device(),
                gpu.queue(),
                &mut encoder,
                &target_view,
                config.width,
                config.height,
            )?;
        }
        gpu.queue().submit(Some(encoder.finish()));
        surface_texture.present();
        Ok(fit)
    }

    pub fn next_wait_deadline(&mut self) -> Option<Instant> {
        if !self.playback_started {
            return None;
        }
        let prepared = self.prepared.as_ref()?;
        let elapsed = self.clock.elapsed_host_us()?;
        match self
            .scheduler
            .evaluate_plan(elapsed, prepared.schedule_plan)
        {
            ScheduleDecision::WaitUntil { target } => {
                self.clock.host_instant_for_media_target(target).ok()
            }
            _ => None,
        }
    }

    pub fn tick(
        &mut self,
        gpu: &GpuContext,
        surface: &RenderSurface,
        window_size: PhysicalSize<u32>,
        overlay: Option<&mut EguiStaticOverlay>,
    ) -> TickResult {
        if !self.playback_started {
            return TickResult::Idle;
        }

        if window_size.width == 0 || window_size.height == 0 {
            return TickResult::Waiting;
        }

        if let Some(prepared) = self.prepared.as_ref() {
            let elapsed = match self.clock.elapsed_host_us() {
                Some(value) => value,
                None => return TickResult::Waiting,
            };
            let plan_target_us = prepared.schedule_plan.target().0;
            let frame_id = prepared.metadata.frame_id().value();
            let decision = self
                .scheduler
                .evaluate_plan(elapsed, prepared.schedule_plan);
            match prepared_frame_decision_action(decision, plan_target_us, frame_id) {
                PreparedFrameDecisionAction::Wait => {
                    self.metrics.record_early_wait();
                    return TickResult::Waiting;
                }
                PreparedFrameDecisionAction::Present { lateness } => {
                    return self.present_prepared(gpu, surface, lateness, overlay);
                }
                PreparedFrameDecisionAction::DiscardLate { .. } => {
                    return self.discard_prepared_drop_late(gpu);
                }
                PreparedFrameDecisionAction::FatalReject {
                    reason,
                    plan_target_us,
                    frame_id,
                } => {
                    return TickResult::Fatal(AppError::Fatal(format!(
                        "prepared frame became undeliverable: RejectTimestamp {reason} plan_target_us={plan_target_us} frame_id={frame_id}"
                    )));
                }
            }
        }

        if self.eof {
            return TickResult::Finished;
        }

        self.decode_prepare_next(gpu)
    }

    /// Frees a prepared bridge frame that became DropLate without presenting it.
    ///
    /// Uses the approved empty-submit + consumed-signal path, then advances the fence
    /// timeline exactly as a successful present would after consume, so the next prepare
    /// receives fresh fence values. Then attempts to prepare the next decode frame.
    fn discard_prepared_drop_late(&mut self, gpu: &GpuContext) -> TickResult {
        let prepared = match self.prepared.take() {
            Some(value) => value,
            None => return TickResult::Waiting,
        };

        self.metrics.record_dropped_late();

        if let Err(error) = self
            .bridge
            .discard_prepared_after_submit(gpu, prepared.values)
        {
            return TickResult::Fatal(AppError::Gpu(error));
        }
        if let Err(error) = self.timeline.advance() {
            return TickResult::Fatal(AppError::Gpu(error));
        }

        if self.eof {
            return TickResult::Finished;
        }
        self.decode_prepare_next(gpu)
    }

    fn present_prepared(
        &mut self,
        gpu: &GpuContext,
        surface: &RenderSurface,
        lateness: MediaTimeUs,
        mut overlay: Option<&mut EguiStaticOverlay>,
    ) -> TickResult {
        let prepared = match self.prepared.take() {
            Some(value) => value,
            None => return TickResult::Waiting,
        };

        let video = match self.bridge.prepared_frame() {
            Ok(frame) => frame,
            Err(error) => return TickResult::Fatal(AppError::Gpu(error)),
        };

        let (surface_texture, target_view) = match surface.acquire_frame(gpu) {
            Ok(value) => value,
            Err(error) => {
                self.prepared = Some(prepared);
                return TickResult::SurfaceRetry(AppError::Render(error));
            }
        };

        let config = surface.configuration();

        let monitor = match overlay.as_deref_mut() {
            Some(overlay) => match overlay.prepare_editor_frame(config.width, config.height) {
                Ok(value) => value,
                Err(error) => {
                    self.prepared = Some(prepared);
                    return TickResult::Fatal(error);
                }
            },
            None => dvs_ui::PhysicalViewport {
                x: 0,
                y: 0,
                width: config.width,
                height: config.height,
            },
        };
        let destination = dvs_render::AspectFitRect {
            x: monitor.x,
            y: monitor.y,
            width: monitor.width,
            height: monitor.height,
        };

        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dvs-app-realtime"),
            });

        if let Err(error) = self.renderer.encode_frame_in_region(
            gpu.device(),
            gpu.queue(),
            &mut encoder,
            video,
            prepared.metadata,
            &target_view,
            config.width,
            config.height,
            destination,
        ) {
            self.prepared = Some(prepared);
            return TickResult::Fatal(AppError::Render(error));
        }

        if let Some(overlay) = overlay
            && let Err(error) = overlay.encode_pending_after_video(
                gpu.device(),
                gpu.queue(),
                &mut encoder,
                &target_view,
                config.width,
                config.height,
            )
        {
            self.prepared = Some(prepared);
            return TickResult::Fatal(error);
        }

        gpu.queue().submit(Some(encoder.finish()));
        if let Err(error) = self
            .bridge
            .signal_consumed_after_submit(gpu, prepared.values)
        {
            return TickResult::Fatal(AppError::Gpu(error));
        }
        if let Err(error) = self.timeline.advance() {
            return TickResult::Fatal(AppError::Gpu(error));
        }

        self.metrics
            .record_presented(prepared.metadata.frame_id(), lateness);
        self.held_display_metadata = Some(prepared.metadata);
        if let Some(ts) = prepared.metadata.timestamp() {
            self.last_pts = Some(ts);
        }
        surface_texture.present();
        TickResult::Presented
    }

    fn decode_prepare_next(&mut self, gpu: &GpuContext) -> TickResult {
        if self.eof {
            self.decode_calls_after_eof += 1;
            return TickResult::Finished;
        }

        loop {
            let decoded = match self.session.decode_next_d3d11() {
                Ok(Some(frame)) => frame,
                Ok(None) => {
                    self.eof = true;
                    let wall = self.clock.elapsed_host_us().unwrap_or(MediaTimeUs::ZERO);
                    let media = match (self.first_pts, self.last_pts) {
                        (Some(start), Some(end)) => media_duration_between(start, end).ok(),
                        _ => None,
                    };
                    self.clock.mark_ended();
                    self.metrics.record_eof(media, wall);
                    return TickResult::Finished;
                }
                Err(error) => return TickResult::Fatal(AppError::Decoder(error)),
            };

            self.metrics.record_decoded();
            let metadata = decoded.metadata();
            let elapsed = self.clock.elapsed_host_us().unwrap_or(MediaTimeUs::ZERO);

            let schedule_plan = match self
                .scheduler
                .plan_frame(metadata.timestamp(), metadata.frame_id())
            {
                Ok(plan) => plan,
                Err(_) => {
                    self.metrics.record_rejected_timestamp();
                    continue;
                }
            };
            let decision = self.scheduler.evaluate_plan(elapsed, schedule_plan);
            self.metrics.record_scheduled();

            match decision {
                ScheduleDecision::RejectTimestamp(_) => {
                    self.metrics.record_rejected_timestamp();
                    continue;
                }
                ScheduleDecision::DropLate { .. } => {
                    self.metrics.record_dropped_late();
                    continue;
                }
                ScheduleDecision::WaitUntil { .. } | ScheduleDecision::PresentNow { .. } => {
                    let values = match self.timeline.current() {
                        Ok(value) => value,
                        Err(error) => return TickResult::Fatal(AppError::Gpu(error)),
                    };
                    let (metadata, surface_ref) = decoded.into_parts();
                    if let Err(error) = self.bridge.prepare_frame(gpu, surface_ref, values) {
                        return TickResult::Fatal(AppError::Gpu(error));
                    }
                    self.prepared = Some(PreparedFrame {
                        metadata,
                        values,
                        schedule_plan,
                    });
                    return TickResult::Waiting;
                }
            }
        }
    }

    /// Releases a prepared bridge frame using the approved GPU-ordered discard path.
    ///
    /// Held display metadata for Ended-state redraws is not affected.
    pub fn release_prepared_on_exit(&mut self, gpu: &GpuContext) -> Result<(), AppError> {
        if let Some(prepared) = self.prepared.take() {
            self.bridge
                .discard_prepared_after_submit(gpu, prepared.values)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod prepared_decision_tests {
    use super::*;
    use dvs_playback::PlaybackError;

    #[test]
    fn drop_late_maps_to_discard_not_fatal() {
        let action = prepared_frame_decision_action(
            ScheduleDecision::DropLate {
                lateness: MediaTimeUs(101_632),
            },
            7_073_733,
            212,
        );
        assert!(matches!(
            action,
            PreparedFrameDecisionAction::DiscardLate {
                lateness_us: 101_632,
                plan_target_us: 7_073_733,
                frame_id: 212,
            }
        ));
        assert!(
            !matches!(action, PreparedFrameDecisionAction::FatalReject { .. }),
            "DropLate must not map to FatalReject"
        );
        assert!(
            !matches!(action, PreparedFrameDecisionAction::Present { .. }),
            "DropLate must not present"
        );
    }

    #[test]
    fn reject_timestamp_remains_fatal_with_diagnostics() {
        let action = prepared_frame_decision_action(
            ScheduleDecision::RejectTimestamp(PlaybackError::NonMonotonicTimestamp),
            1_000,
            9,
        );
        match action {
            PreparedFrameDecisionAction::FatalReject {
                reason,
                plan_target_us,
                frame_id,
            } => {
                assert!(reason.contains("NonMonotonicTimestamp"));
                assert_eq!(plan_target_us, 1_000);
                assert_eq!(frame_id, 9);
            }
            other => panic!("expected FatalReject, got {other:?}"),
        }
    }

    #[test]
    fn present_now_still_presents() {
        let action = prepared_frame_decision_action(
            ScheduleDecision::PresentNow {
                lateness: MediaTimeUs(12),
            },
            500,
            3,
        );
        assert_eq!(
            action,
            PreparedFrameDecisionAction::Present {
                lateness: MediaTimeUs(12),
            }
        );
    }

    #[test]
    fn discard_late_action_clears_prepared_slot_contract() {
        // Documented contract of discard_prepared_drop_late: take() leaves prepared None
        // before bridge discard; the pure action never implies keeping the slot.
        let action = prepared_frame_decision_action(
            ScheduleDecision::DropLate {
                lateness: MediaTimeUs(70_000),
            },
            42,
            7,
        );
        assert!(matches!(
            action,
            PreparedFrameDecisionAction::DiscardLate { .. }
        ));
    }
}
