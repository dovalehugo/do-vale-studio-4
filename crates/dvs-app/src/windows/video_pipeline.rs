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
    AspectFitRect, Nv12Renderer, Nv12RendererConfig, Nv12RendererResourceStats, PhysicalRenderRect,
    RenderSurface,
};
use wgpu::{CommandEncoder, LoadOp, TextureView};
use winit::dpi::PhysicalSize;

use crate::error::AppError;

pub(crate) struct PreparedFrame {
    metadata: VideoFrameMetadata,
    values: FrameFenceValues,
    schedule_plan: FrameSchedulePlan,
}

/// Playback step evaluated without acquiring the presentation surface.
pub enum PlaybackStep {
    Idle,
    Waiting,
    Present { lateness: MediaTimeUs },
    Finished,
    Fatal(AppError),
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
    diagnostic_notes: Vec<PipelineDiagnosticNote>,
}

#[derive(Clone)]
pub(crate) struct PipelineDiagnosticNote {
    pub kind: String,
    pub extra: String,
}

enum DecodePrepareResult {
    Prepared,
    Eof,
    Failed(AppError),
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
            diagnostic_notes: Vec::new(),
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

    /// Encodes the current display frame into a destination rectangle on a shared encoder.
    pub fn encode_display_in_rect(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        target_width: u32,
        target_height: u32,
        destination: PhysicalRenderRect,
    ) -> Result<AspectFitRect, AppError> {
        let (metadata, video) = if let Some(prepared) = self.prepared.as_ref() {
            (
                prepared.metadata,
                self.bridge.prepared_frame().map_err(AppError::Gpu)?,
            )
        } else if self.eof {
            let metadata = *self
                .held_display_metadata
                .as_ref()
                .ok_or(AppError::InvalidState)?;
            (
                metadata,
                self.bridge
                    .consumed_display_frame()
                    .map_err(AppError::Gpu)?,
            )
        } else {
            return Err(AppError::InvalidState);
        };

        self.renderer
            .encode_frame_in_rect(
                gpu.device(),
                gpu.queue(),
                encoder,
                video,
                metadata,
                target,
                target_width,
                target_height,
                destination,
                LoadOp::Load,
            )
            .map_err(AppError::Render)
    }

    /// Encodes the prepared playback frame into a destination rectangle on a shared encoder.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_prepared_in_rect(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut CommandEncoder,
        target: &TextureView,
        target_width: u32,
        target_height: u32,
        destination: PhysicalRenderRect,
        prepared: &PreparedFrame,
    ) -> Result<AspectFitRect, AppError> {
        let video = self.bridge.prepared_frame().map_err(AppError::Gpu)?;
        self.renderer
            .encode_frame_in_rect(
                gpu.device(),
                gpu.queue(),
                encoder,
                video,
                prepared.metadata,
                target,
                target_width,
                target_height,
                destination,
                LoadOp::Load,
            )
            .map_err(AppError::Render)
    }

    /// Finalizes a presented frame after queue submission.
    pub fn finalize_present(
        &mut self,
        gpu: &GpuContext,
        prepared: PreparedFrame,
        lateness: MediaTimeUs,
    ) -> Result<(), AppError> {
        self.bridge
            .signal_consumed_after_submit(gpu, prepared.values)
            .map_err(AppError::Gpu)?;
        self.timeline.advance().map_err(AppError::Gpu)?;
        self.metrics
            .record_presented(prepared.metadata.frame_id(), lateness);
        self.held_display_metadata = Some(prepared.metadata);
        if let Some(ts) = prepared.metadata.timestamp() {
            self.last_pts = Some(ts);
        }
        Ok(())
    }

    /// Returns whether a prepared frame is waiting for its PTS target.
    pub fn is_waiting_for_prepared_present(&self) -> bool {
        let Some(prepared) = self.prepared.as_ref() else {
            return false;
        };
        let Some(elapsed) = self.clock.elapsed_host_us() else {
            return false;
        };
        matches!(
            self.scheduler
                .evaluate_plan(elapsed, prepared.schedule_plan),
            ScheduleDecision::WaitUntil { .. }
        )
    }

    /// Evaluates the next playback step without rendering.
    pub fn evaluate_playback_step(
        &mut self,
        gpu: &GpuContext,
        window_size: PhysicalSize<u32>,
    ) -> PlaybackStep {
        if !self.playback_started {
            return PlaybackStep::Idle;
        }
        if window_size.width == 0 || window_size.height == 0 {
            return PlaybackStep::Waiting;
        }

        if let Some(prepared) = self.prepared.as_ref() {
            let elapsed = match self.clock.elapsed_host_us() {
                Some(value) => value,
                None => return PlaybackStep::Waiting,
            };
            match self
                .scheduler
                .evaluate_plan(elapsed, prepared.schedule_plan)
            {
                ScheduleDecision::WaitUntil { .. } => {
                    self.metrics.record_early_wait();
                    PlaybackStep::Waiting
                }
                ScheduleDecision::PresentNow { lateness } => PlaybackStep::Present { lateness },
                ScheduleDecision::DropLate { .. } | ScheduleDecision::RejectTimestamp(_) => {
                    PlaybackStep::Waiting
                }
            }
        } else if self.eof {
            PlaybackStep::Finished
        } else {
            match self.decode_prepare_next(gpu) {
                DecodePrepareResult::Prepared => PlaybackStep::Waiting,
                DecodePrepareResult::Eof => PlaybackStep::Finished,
                DecodePrepareResult::Failed(error) => PlaybackStep::Fatal(error),
            }
        }
    }

    /// Takes the currently prepared frame for presentation.
    pub(crate) fn take_prepared_for_present(&mut self) -> Option<PreparedFrame> {
        self.prepared.take()
    }

    /// Restores a prepared frame when surface acquisition fails.
    pub(crate) fn restore_prepared(&mut self, prepared: PreparedFrame) {
        self.prepared = Some(prepared);
    }

    /// Returns the monotonic instant when the prepared frame becomes due, if waiting.
    pub fn playback_wakeup_deadline(&self) -> Option<Instant> {
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

    /// Returns whether the prepared frame's PTS deadline has been reached.
    pub fn is_playback_frame_due(&self) -> bool {
        match self.playback_wakeup_deadline() {
            Some(deadline) => Instant::now() >= deadline,
            None => !self.is_waiting_for_prepared_present(),
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

    pub(crate) fn diagnostic_media_position_us(&self) -> Option<i64> {
        self.clock.current_media_position().map(|value| value.0)
    }

    pub(crate) fn diagnostic_prepared(&self) -> (bool, Option<u64>, Option<i64>) {
        match self.prepared.as_ref() {
            Some(prepared) => (
                true,
                Some(prepared.metadata.frame_id().value()),
                prepared.metadata.timestamp().map(|ts| ts.pts()),
            ),
            None => (false, None, None),
        }
    }

    pub(crate) fn diagnostic_bridge_state(&self) -> String {
        if self.prepared.is_some() {
            if self.bridge.prepared_frame().is_ok() {
                "prepared".to_string()
            } else {
                "prepared_metadata_only".to_string()
            }
        } else if self.eof {
            if self.bridge.consumed_display_frame().is_ok() {
                "consumed_display".to_string()
            } else {
                "eof_no_display".to_string()
            }
        } else {
            "idle".to_string()
        }
    }

    pub(crate) fn diagnostic_clock_state(&self) -> dvs_playback::PlaybackState {
        self.clock.state()
    }

    pub(crate) fn take_diagnostic_notes(&mut self) -> Vec<PipelineDiagnosticNote> {
        std::mem::take(&mut self.diagnostic_notes)
    }

    fn extend_diagnostic_notes(&mut self, notes: Vec<PipelineDiagnosticNote>) {
        for note in notes {
            if self.diagnostic_notes.len() >= 32 {
                break;
            }
            self.diagnostic_notes.push(note);
        }
    }

    fn decode_prepare_next(&mut self, gpu: &GpuContext) -> DecodePrepareResult {
        if self.eof {
            self.decode_calls_after_eof += 1;
            return DecodePrepareResult::Eof;
        }

        loop {
            let mut loop_notes = Vec::new();
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
                    return DecodePrepareResult::Eof;
                }
                Err(error) => return DecodePrepareResult::Failed(AppError::Decoder(error)),
            };

            self.metrics.record_decoded();
            let (frame_id, pts, timestamp, frame_id_value) = {
                let metadata = decoded.metadata();
                (
                    metadata.frame_id().value(),
                    metadata.timestamp().map(|ts| ts.pts()),
                    metadata.timestamp(),
                    metadata.frame_id(),
                )
            };
            loop_notes.push(PipelineDiagnosticNote {
                kind: "frame_decoded".to_string(),
                extra: format!("frame_id={frame_id} pts={pts:?}"),
            });
            let elapsed = self.clock.elapsed_host_us().unwrap_or(MediaTimeUs::ZERO);

            let schedule_plan = match self.scheduler.plan_frame(timestamp, frame_id_value) {
                Ok(plan) => plan,
                Err(_) => {
                    self.metrics.record_rejected_timestamp();
                    self.extend_diagnostic_notes(loop_notes);
                    continue;
                }
            };
            let decision = self.scheduler.evaluate_plan(elapsed, schedule_plan);
            self.metrics.record_scheduled();

            match decision {
                ScheduleDecision::RejectTimestamp(_) => {
                    self.metrics.record_rejected_timestamp();
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_classified".to_string(),
                        extra: format!("Reject frame_id={frame_id}"),
                    });
                    self.extend_diagnostic_notes(loop_notes);
                    continue;
                }
                ScheduleDecision::DropLate { lateness } => {
                    self.metrics.record_dropped_late();
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_classified".to_string(),
                        extra: format!("DropLate frame_id={frame_id} lateness={lateness:?}"),
                    });
                    self.extend_diagnostic_notes(loop_notes);
                    continue;
                }
                ScheduleDecision::WaitUntil { target } => {
                    let values = match self.timeline.current() {
                        Ok(value) => value,
                        Err(error) => return DecodePrepareResult::Failed(AppError::Gpu(error)),
                    };
                    let (metadata, surface_ref) = decoded.into_parts();
                    if let Err(error) = self.bridge.prepare_frame(gpu, surface_ref, values) {
                        return DecodePrepareResult::Failed(AppError::Gpu(error));
                    }
                    let prepared_id = metadata.frame_id().value();
                    let prepared_pts = metadata.timestamp().map(|ts| ts.pts());
                    self.prepared = Some(PreparedFrame {
                        metadata,
                        values,
                        schedule_plan,
                    });
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_prepared".to_string(),
                        extra: format!("frame_id={prepared_id} pts={prepared_pts:?}"),
                    });
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_classified".to_string(),
                        extra: format!("WaitUntil frame_id={prepared_id} target={target:?}"),
                    });
                    self.extend_diagnostic_notes(loop_notes);
                    return DecodePrepareResult::Prepared;
                }
                ScheduleDecision::PresentNow { lateness } => {
                    let values = match self.timeline.current() {
                        Ok(value) => value,
                        Err(error) => return DecodePrepareResult::Failed(AppError::Gpu(error)),
                    };
                    let (metadata, surface_ref) = decoded.into_parts();
                    if let Err(error) = self.bridge.prepare_frame(gpu, surface_ref, values) {
                        return DecodePrepareResult::Failed(AppError::Gpu(error));
                    }
                    let prepared_id = metadata.frame_id().value();
                    let prepared_pts = metadata.timestamp().map(|ts| ts.pts());
                    self.prepared = Some(PreparedFrame {
                        metadata,
                        values,
                        schedule_plan,
                    });
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_prepared".to_string(),
                        extra: format!("frame_id={prepared_id} pts={prepared_pts:?}"),
                    });
                    loop_notes.push(PipelineDiagnosticNote {
                        kind: "frame_classified".to_string(),
                        extra: format!("PresentNow frame_id={prepared_id} lateness={lateness:?}"),
                    });
                    self.extend_diagnostic_notes(loop_notes);
                    return DecodePrepareResult::Prepared;
                }
            }
        }
    }
}
