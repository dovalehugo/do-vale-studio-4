//! Shared Windows real-time playback loop for Integration 6 validation targets.

#![cfg(target_os = "windows")]
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dvs_decoder::DecoderSession;
use dvs_gpu::{
    FenceTimeline, FrameFenceValues, GpuBootstrap, GpuContext, SharedNv12TextureDesc,
    SurfaceWindowTarget, WindowsD3d11SharedNv12Producer, WindowsD3d11WgpuInteropBridge,
};
use dvs_media::{MediaTimestamp, VideoFrameMetadata};
use dvs_playback::{
    FrameSchedulePlan, FrameScheduler, MediaTimeUs, PlaybackClock, PlaybackMetrics,
    ScheduleDecision, SchedulerConfig, media_duration_between,
};
use dvs_render::{Nv12Renderer, Nv12RendererConfig, RenderSurface};
use winit::dpi::PhysicalSize;
use winit::window::Window;

pub const DEFAULT_FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
pub const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";

/// Timing tolerances for automated hardware validation.
pub struct TimingTolerances {
    /// Minimum acceptable monotonic playback duration as a fraction of expected media duration.
    pub min_duration_ratio: f64,
    /// Maximum acceptable monotonic playback duration as a fraction of expected media duration.
    pub max_duration_ratio: f64,
    /// Maximum sustained presentation rate before rejecting decode-as-fast-as-possible behavior.
    pub max_sustained_fps: f64,
    /// Maximum allowed late drops for a 90-frame fixture pass.
    pub max_late_drops: u64,
}

impl Default for TimingTolerances {
    fn default() -> Self {
        Self {
            min_duration_ratio: 0.85,
            max_duration_ratio: 1.50,
            max_sustained_fps: 45.0,
            max_late_drops: 3,
        }
    }
}

pub struct PreparedFrame {
    pub metadata: VideoFrameMetadata,
    pub values: FrameFenceValues,
    pub schedule_plan: FrameSchedulePlan,
}

pub struct PlaybackPipeline {
    pub session: DecoderSession,
    pub bridge: WindowsD3d11WgpuInteropBridge,
    pub renderer: Nv12Renderer,
    pub timeline: FenceTimeline,
    pub clock: PlaybackClock,
    pub scheduler: FrameScheduler,
    pub metrics: PlaybackMetrics,
    pub prepared: Option<PreparedFrame>,
    pub eof: bool,
    pub first_pts: Option<MediaTimestamp>,
    pub last_pts: Option<MediaTimestamp>,
    pub time_base_summary: Option<String>,
    pub playing: bool,
}

impl PlaybackPipeline {
    /// Opens the decoder, bridge, and renderer using the first decoded frame for dimensions.
    ///
    /// The first frame is prepared but playback does not start until [`Self::start_playback`].
    pub fn bootstrap(
        fixture: &Path,
        gpu: &GpuContext,
        surface: &RenderSurface,
    ) -> Result<Self, String> {
        let wgpu_luid = gpu
            .adapter_identity()
            .dxgi_luid()
            .ok_or_else(|| "wgpu adapter LUID unavailable".to_string())?;

        let mut session =
            DecoderSession::open(fixture, wgpu_luid).map_err(|e| format!("DecoderSession: {e}"))?;

        let (d3d11_device, d3d11_context) = {
            let hw = session
                .d3d11_hardware()
                .map_err(|e| format!("d3d11_hardware: {e}"))?;
            (hw.device().clone(), hw.context().clone())
        };
        let external_context_lock = session
            .external_context_lock()
            .map_err(|e| format!("external_context_lock: {e}"))?;

        let first = session
            .decode_next_d3d11()
            .map_err(|e| format!("first decode: {e}"))?
            .ok_or_else(|| "fixture EOF before first frame".to_string())?;

        let first_metadata = first.metadata();
        let timestamp = first_metadata
            .timestamp()
            .ok_or_else(|| "first frame missing PTS".to_string())?;
        let time_base = timestamp.time_base();
        let time_base_summary = format!("{}/{}", time_base.numerator(), time_base.denominator());

        let allocation = first_metadata.dimensions().allocation();
        let desc = SharedNv12TextureDesc::new(allocation.width(), allocation.height())
            .map_err(|e| format!("SharedNv12TextureDesc: {e}"))?;
        let producer = WindowsD3d11SharedNv12Producer::new_with_external_lock(
            &d3d11_device,
            &d3d11_context,
            wgpu_luid,
            desc,
            Some(external_context_lock),
        )
        .map_err(|e| format!("producer: {e}"))?;

        let mut bridge = WindowsD3d11WgpuInteropBridge::new(gpu, producer)
            .map_err(|e| format!("bridge: {e}"))?;

        let encoding = surface.output_encoding();
        let renderer = Nv12Renderer::new(
            gpu.device(),
            Nv12RendererConfig {
                target_format: encoding.format,
            },
        )
        .map_err(|e| format!("Nv12Renderer: {e}"))?;

        let timeline = FenceTimeline::new();
        let values = timeline.current().map_err(|e| format!("timeline: {e}"))?;
        let (first_frame_metadata, surface_ref) = first.into_parts();
        bridge
            .prepare_frame(gpu, surface_ref, values)
            .map_err(|e| format!("prepare first: {e}"))?;

        let mut pipeline = Self {
            session,
            bridge,
            renderer,
            timeline,
            clock: PlaybackClock::new(),
            scheduler: FrameScheduler::new(SchedulerConfig::ntsc_30000_over_1001_default()),
            metrics: PlaybackMetrics::new(),
            prepared: None,
            eof: false,
            first_pts: Some(timestamp),
            last_pts: Some(timestamp),
            time_base_summary: Some(time_base_summary),
            playing: false,
        };

        pipeline.metrics.record_decoded();
        let schedule_plan = pipeline
            .scheduler
            .plan_frame(Some(timestamp), first_frame_metadata.frame_id())
            .map_err(|e| format!("plan first frame: {e}"))?;
        pipeline.prepared = Some(PreparedFrame {
            metadata: first_frame_metadata,
            values,
            schedule_plan,
        });
        Ok(pipeline)
    }

    /// Starts PTS-driven playback. The first prepared frame is due immediately.
    pub fn start_playback(&mut self) -> Result<(), String> {
        if self.playing {
            return Ok(());
        }
        let timestamp = self
            .first_pts
            .ok_or_else(|| "missing first PTS".to_string())?;
        self.clock
            .start(timestamp)
            .map_err(|e| format!("clock start: {e}"))?;
        self.playing = true;
        self.metrics.record_scheduled();
        Ok(())
    }

    /// Draws the currently prepared frame without advancing playback (ready-state preview).
    pub fn draw_prepared_preview(
        &mut self,
        gpu: &GpuContext,
        surface: &RenderSurface,
    ) -> Result<(), String> {
        let prepared = self
            .prepared
            .as_ref()
            .ok_or_else(|| "no prepared frame".to_string())?;
        let video = self
            .bridge
            .prepared_frame()
            .map_err(|e| format!("prepared_frame: {e}"))?;
        let (surface_texture, target_view) = surface
            .acquire_frame(gpu)
            .map_err(|e| format!("acquire_frame: {e}"))?;
        let config = surface.configuration();
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dvs-playback-preview"),
            });
        self.renderer
            .encode_frame(
                gpu.device(),
                gpu.queue(),
                &mut encoder,
                video,
                prepared.metadata,
                &target_view,
                config.width,
                config.height,
            )
            .map_err(|e| format!("encode_frame: {e}"))?;
        gpu.queue().submit(Some(encoder.finish()));
        surface_texture.present();
        Ok(())
    }

    pub fn next_wait_deadline(&mut self) -> Option<Instant> {
        if !self.playing {
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
    ) -> TickResult {
        if !self.playing {
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
            match self
                .scheduler
                .evaluate_plan(elapsed, prepared.schedule_plan)
            {
                ScheduleDecision::WaitUntil { .. } => {
                    self.metrics.record_early_wait();
                    return TickResult::Waiting;
                }
                ScheduleDecision::PresentNow { lateness } => {
                    return self.present_prepared(gpu, surface, lateness);
                }
                ScheduleDecision::DropLate { .. } | ScheduleDecision::RejectTimestamp(_) => {
                    return TickResult::Fatal("prepared frame became undeliverable".to_string());
                }
            }
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
    ) -> TickResult {
        let prepared = match self.prepared.take() {
            Some(value) => value,
            None => return TickResult::Waiting,
        };

        let video = match self.bridge.prepared_frame() {
            Ok(frame) => frame,
            Err(error) => return TickResult::Fatal(format!("prepared_frame: {error}")),
        };

        let (surface_texture, target_view) = match surface.acquire_frame(gpu) {
            Ok(value) => value,
            Err(error) => {
                self.prepared = Some(prepared);
                return TickResult::SurfaceRetry(format!("acquire_frame: {error}"));
            }
        };

        let config = surface.configuration();
        let mut encoder = gpu
            .device()
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("dvs-playback-realtime"),
            });

        if let Err(error) = self.renderer.encode_frame(
            gpu.device(),
            gpu.queue(),
            &mut encoder,
            video,
            prepared.metadata,
            &target_view,
            config.width,
            config.height,
        ) {
            self.prepared = Some(prepared);
            return TickResult::Fatal(format!("encode_frame: {error}"));
        }

        gpu.queue().submit(Some(encoder.finish()));
        if let Err(error) = self
            .bridge
            .signal_consumed_after_submit(gpu, prepared.values)
        {
            return TickResult::Fatal(format!("signal_consumed: {error}"));
        }
        if let Err(error) = self.timeline.advance() {
            return TickResult::Fatal(format!("timeline advance: {error}"));
        }

        self.metrics
            .record_presented(prepared.metadata.frame_id(), lateness);
        if let Some(ts) = prepared.metadata.timestamp() {
            self.last_pts = Some(ts);
        }
        surface_texture.present();
        TickResult::Presented
    }

    fn decode_prepare_next(&mut self, gpu: &GpuContext) -> TickResult {
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
                Err(error) => return TickResult::Fatal(format!("decode: {error}")),
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
                        Err(error) => return TickResult::Fatal(format!("timeline: {error}")),
                    };
                    let (metadata, surface_ref) = decoded.into_parts();
                    if let Err(error) = self.bridge.prepare_frame(gpu, surface_ref, values) {
                        return TickResult::Fatal(format!("prepare_frame: {error}"));
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

    pub fn release_prepared_on_exit(&mut self, gpu: &GpuContext) {
        if let Some(prepared) = self.prepared.take() {
            let _ = self
                .bridge
                .discard_prepared_after_submit(gpu, prepared.values);
        }
    }
}

pub enum TickResult {
    Idle,
    Waiting,
    Presented,
    Finished,
    SurfaceRetry(String),
    Fatal(String),
}

pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}

pub fn fixture_path() -> PathBuf {
    std::env::var_os("DVS_DECODER_FIXTURE")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().join(DEFAULT_FIXTURE_REL))
}

pub fn require_fixture(path: &Path) -> Result<(), String> {
    if path.is_file() {
        return Ok(());
    }
    Err(format!(
        "fixture not found at {} — place the 4K HEVC fixture or set DVS_DECODER_FIXTURE. See {SETUP_DOC}.",
        path.display()
    ))
}

pub async fn initialize_gpu(window: Arc<Window>) -> Result<(GpuContext, RenderSurface), String> {
    let gpu = GpuBootstrap::initialize(window.clone() as Arc<dyn SurfaceWindowTarget>)
        .await
        .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;
    let size = window.inner_size();
    let surface = RenderSurface::configure(&gpu, size.width.max(1), size.height.max(1))
        .map_err(|e| format!("RenderSurface::configure failed: {e}"))?;
    Ok((gpu, surface))
}

pub fn validate_timing(
    metrics: &PlaybackMetrics,
    expected_media_us: Option<MediaTimeUs>,
    tolerances: &TimingTolerances,
) -> Result<(), String> {
    let presented = metrics.frames_presented();
    let wall_us = metrics
        .monotonic_wall_duration_us()
        .map(|value| value.0)
        .unwrap_or(0) as f64;
    let expected_us = expected_media_us.map(|value| value.0 as f64).unwrap_or(0.0);

    if presented == 0 {
        return Err("no frames presented".to_string());
    }

    if metrics.frames_dropped_late() > tolerances.max_late_drops {
        return Err(format!(
            "too many late drops: {} > {}",
            metrics.frames_dropped_late(),
            tolerances.max_late_drops
        ));
    }

    if expected_us > 0.0 {
        let ratio = wall_us / expected_us;
        if ratio < tolerances.min_duration_ratio {
            return Err(format!(
                "playback too fast: wall/media ratio {ratio:.3} < {}",
                tolerances.min_duration_ratio
            ));
        }
        if ratio > tolerances.max_duration_ratio {
            return Err(format!(
                "playback too slow or stalled: wall/media ratio {ratio:.3} > {}",
                tolerances.max_duration_ratio
            ));
        }
        let sustained_fps = presented as f64 / (wall_us / 1_000_000.0);
        if sustained_fps > tolerances.max_sustained_fps {
            return Err(format!(
                "decode-as-fast-as-possible suspected: {sustained_fps:.2} FPS > {}",
                tolerances.max_sustained_fps
            ));
        }
    }

    Ok(())
}

pub fn print_metrics_summary(pipeline: &PlaybackPipeline) {
    let metrics = &pipeline.metrics;
    println!("=== Integration 6 playback summary ===");
    if let Some(tb) = &pipeline.time_base_summary {
        println!("fixture_time_base: {tb}");
    }
    if let Some(first) = pipeline.first_pts {
        println!("first_pts: {}", first.pts());
    }
    if let Some(last) = pipeline.last_pts {
        println!("last_pts: {}", last.pts());
    }
    if let Some(duration) = metrics.playback_media_duration_us() {
        println!("expected_media_duration_us: {}", duration.0);
    }
    if let Some(wall) = metrics.monotonic_wall_duration_us() {
        println!("measured_monotonic_duration_us: {}", wall.0);
    }
    println!("decoded: {}", metrics.frames_decoded());
    println!("presented: {}", metrics.frames_presented());
    println!("late_drops: {}", metrics.frames_dropped_late());
    println!("early_waits: {}", metrics.early_wait_count());
    println!("max_lateness_us: {}", metrics.max_lateness_us());
    println!("average_lateness_us: {}", metrics.average_lateness_us());
    if let (Some(first), Some(last)) = (
        metrics.first_presented_frame_id(),
        metrics.last_presented_frame_id(),
    ) {
        println!("frame_id_range: {}..={}", first.value(), last.value());
    }
    println!("eof_reached: {}", metrics.eof_reached());
    println!(
        "surface_reconfigurations: {}",
        metrics.surface_reconfigurations()
    );
    println!("clock_state: {:?}", pipeline.clock.state());
}
