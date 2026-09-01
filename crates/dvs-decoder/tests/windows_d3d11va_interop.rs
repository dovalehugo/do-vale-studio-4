//! Windows hardware integration test: real FFmpeg D3D11VA decode → production interop bridge.

#![cfg(target_os = "windows")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use dvs_decoder::{DecodedD3d11Frame, DecoderSession};
use dvs_gpu::{
    FenceTimeline, GpuBootstrap, GpuContext, GpuVideoPixelFormat, SharedNv12TextureDesc,
    SurfaceWindowTarget, WindowsD3d11SharedNv12Producer, WindowsD3d11WgpuInteropBridge,
};
use dvs_media::VideoPixelFormat;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

const DEFAULT_FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";
const FRAMES_TO_BRIDGE: usize = 90;

struct InteropTestApp {
    finished: Option<Result<InteropTestContext, String>>,
}

struct InteropTestContext {
    _window: Arc<Window>,
    gpu: GpuContext,
}

impl ApplicationHandler for InteropTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("dvs-decoder D3D11VA interop test")
                .with_inner_size(winit::dpi::LogicalSize::new(640, 360)),
        ) {
            Ok(window) => Arc::new(window),
            Err(err) => {
                self.finished = Some(Err(format!("create_window failed: {err}")));
                event_loop.exit();
                return;
            }
        };

        self.finished = Some(initialize_gpu(window));
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

fn initialize_gpu(window: Arc<Window>) -> Result<InteropTestContext, String> {
    let gpu = pollster::block_on(GpuBootstrap::initialize(
        window.clone() as Arc<dyn SurfaceWindowTarget>
    ))
    .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;

    Ok(InteropTestContext {
        _window: window,
        gpu,
    })
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
        "fixture not found at {} — place the 4K HEVC fixture or set DVS_DECODER_FIXTURE. See {SETUP_DOC}.",
        path.display()
    ))
}

fn required_adapter_luid(gpu: &GpuContext) -> Result<dvs_gpu::DxgiAdapterLuid, String> {
    gpu.adapter_identity()
        .dxgi_luid()
        .ok_or_else(|| "wgpu adapter LUID unavailable from GpuBootstrap".to_string())
}

fn submit_empty_queue_work(gpu: &GpuContext) {
    let encoder = gpu
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("d3d11va-interop-empty-submit"),
        });
    gpu.queue().submit(Some(encoder.finish()));
}

fn validate_decoded_metadata(
    decoded: &DecodedD3d11Frame<'_>,
    previous_frame_id: Option<u64>,
    index: usize,
) -> u64 {
    let metadata = decoded.metadata();
    let frame_id = metadata.frame_id().value();
    if let Some(previous) = previous_frame_id {
        assert!(frame_id > previous, "FrameId must increase monotonically");
    } else {
        assert_eq!(frame_id, 0, "first FrameId must be 0");
    }

    assert_eq!(metadata.pixel_format(), VideoPixelFormat::Nv12);

    let allocation = metadata.dimensions().allocation();
    let visible = metadata.dimensions().visible();
    assert_eq!(
        (allocation.width(), allocation.height()),
        (3840, 2176),
        "unexpected allocation dimensions at frame {index}"
    );
    assert_eq!(
        (visible.x(), visible.y(), visible.width(), visible.height()),
        (0, 0, 3840, 2160),
        "unexpected visible dimensions at frame {index}"
    );

    frame_id
}

fn bridge_one_frame(
    session: &mut DecoderSession,
    bridge: &mut WindowsD3d11WgpuInteropBridge,
    gpu: &GpuContext,
    timeline: &mut FenceTimeline,
    previous_frame_id: &mut Option<u64>,
    bridged_count: &mut usize,
    array_slices: &mut BTreeSet<u32>,
) {
    let decoded = session
        .decode_next_d3d11()
        .expect("decode_next_d3d11")
        .unwrap_or_else(|| {
            panic!("decoder EOF before {FRAMES_TO_BRIDGE} frames (got {bridged_count})")
        });

    validate_decoded_metadata(&decoded, *previous_frame_id, *bridged_count);
    let frame_id = decoded.metadata().frame_id().value();

    let values = timeline.current().expect("timeline values");
    let (metadata, surface) = decoded.into_parts();
    assert_eq!(metadata.frame_id().value(), frame_id);

    let array_slice = surface.array_slice();
    array_slices.insert(array_slice);

    let allocation = metadata.dimensions().allocation();
    let video = bridge
        .prepare_frame(gpu, surface, values)
        .expect("prepare_frame");
    // `DecodedD3d11Frame` surface borrow ends here. The FFmpeg `AVFrame` pool slot remains
    // held in `session.current_frame` until the next `decode_next_d3d11` unrefs it.
    // Source safety relies on same-context command ordering (`CopySubresourceRegion` then
    // `Signal(ready)` enqueued before the producer releases FFmpeg's lock) and on not calling
    // `decode_next_d3d11` until after `consumed` is signalled for this frame.
    // `Flush` submits asynchronously and does not wait for GPU completion.

    assert_eq!(video.pixel_format(), GpuVideoPixelFormat::Nv12);
    assert_eq!(video.allocation_width(), allocation.width());
    assert_eq!(video.allocation_height(), allocation.height());
    let texture_size = video.texture().size();
    assert_ne!(texture_size.width, 0);
    assert_ne!(texture_size.height, 0);

    submit_empty_queue_work(gpu);
    bridge
        .signal_consumed_after_submit(gpu, values)
        .expect("signal_consumed_after_submit");
    timeline.advance().expect("timeline advance");

    *previous_frame_id = Some(frame_id);
    *bridged_count += 1;
}

#[test]
#[ignore = "requires Windows GPU, FFmpeg dev libraries, and docs/fixtures/test_4k_hevc_8bit30.mp4"]
fn windows_d3d11va_interop_fixture() {
    let fixture = fixture_path();
    require_fixture(&fixture).expect("fixture setup");

    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut app = InteropTestApp { finished: None };
    event_loop.run_app(&mut app).expect("event loop run");

    let ctx = app
        .finished
        .expect("app finished")
        .expect("GPU initialization");

    let wgpu_luid = required_adapter_luid(&ctx.gpu).expect("wgpu LUID");

    let mut session = DecoderSession::open(&fixture, wgpu_luid).expect("DecoderSession::open");
    assert_eq!(session.adapter_luid(), wgpu_luid);

    let (d3d11_device, d3d11_context) = {
        let hw = session.d3d11_hardware().expect("d3d11_hardware");
        session
            .debug_assert_same_ffmpeg_device_context(hw.context())
            .expect("FFmpeg device_context COM identity");
        (hw.device().clone(), hw.context().clone())
    };
    let external_context_lock = session
        .external_context_lock()
        .expect("external_context_lock");

    let first = session
        .decode_next_d3d11()
        .expect("first decode")
        .expect("first frame");
    let first_frame_id = validate_decoded_metadata(&first, None, 0);
    let first_metadata = first.metadata();
    let allocation = first_metadata.dimensions().allocation();
    let visible = first_metadata.dimensions().visible();
    let (first_metadata, first_surface) = first.into_parts();
    let _first_metadata = first_metadata;
    let first_array_slice = first_surface.array_slice();

    let desc = SharedNv12TextureDesc::new(allocation.width(), allocation.height())
        .expect("SharedNv12TextureDesc");
    let producer = WindowsD3d11SharedNv12Producer::new_with_external_lock(
        &d3d11_device,
        &d3d11_context,
        wgpu_luid,
        desc,
        Some(external_context_lock),
    )
    .expect("producer");
    assert_eq!(producer.adapter_luid(), wgpu_luid);

    let mut bridge =
        WindowsD3d11WgpuInteropBridge::new(&ctx.gpu, producer).expect("interop bridge");
    assert_eq!(bridge.shared_handle_open_counts(), (1, 1));

    let started = Instant::now();
    let mut timeline = FenceTimeline::new();
    let mut previous_frame_id = Some(first_frame_id);
    let mut bridged_count = 1usize;
    let mut array_slices = BTreeSet::new();
    array_slices.insert(first_array_slice);

    let values = timeline.current().expect("timeline values for first frame");
    let video = bridge
        .prepare_frame(&ctx.gpu, first_surface, values)
        .expect("prepare_frame first");
    assert_eq!(video.allocation_width(), allocation.width());
    assert_eq!(video.allocation_height(), allocation.height());
    submit_empty_queue_work(&ctx.gpu);
    bridge
        .signal_consumed_after_submit(&ctx.gpu, values)
        .expect("signal_consumed first");
    timeline.advance().expect("advance first");

    while bridged_count < FRAMES_TO_BRIDGE {
        bridge_one_frame(
            &mut session,
            &mut bridge,
            &ctx.gpu,
            &mut timeline,
            &mut previous_frame_id,
            &mut bridged_count,
            &mut array_slices,
        );
    }

    let elapsed = started.elapsed();
    let throughput_fps = bridged_count as f64 / elapsed.as_secs_f64();

    let saw_eof = loop {
        match session.decode_next_d3d11().expect("decode drain") {
            Some(_frame) => {}
            None => break true,
        }
    };

    let final_values = timeline.current().expect("final timeline values");

    eprintln!("=== Integration 4B hardware summary ===");
    eprintln!("adapter_luid: {wgpu_luid:?}");
    eprintln!("frames_decoded_and_bridged: {bridged_count}");
    eprintln!("first_frame_id: {first_frame_id}");
    eprintln!(
        "last_frame_id: {}",
        previous_frame_id.expect("last frame id")
    );
    eprintln!(
        "allocation_dimensions: {}x{}",
        allocation.width(),
        allocation.height()
    );
    eprintln!(
        "visible_dimensions: {}x{}",
        visible.width(),
        visible.height()
    );
    eprintln!("pixel_format: NV12");
    eprintln!("distinct_source_array_slices: {array_slices:?}");
    eprintln!(
        "bridge_shared_handle_open_counts: {:?}",
        bridge.shared_handle_open_counts()
    );
    eprintln!("final_timeline_frame_index: {}", final_values.frame_index());
    eprintln!(
        "final_fence_ready/consumed: {}/{}",
        final_values.ready(),
        final_values.consumed()
    );
    eprintln!("eof_drain: {saw_eof}");
    eprintln!(
        "elapsed_s: {:.3}; throughput_fps: {throughput_fps:.2} (diagnostic only)",
        elapsed.as_secs_f64()
    );

    assert!(saw_eof, "EOF drain should return Ok(None)");
    assert_eq!(bridged_count, FRAMES_TO_BRIDGE);
    assert_eq!(final_values.frame_index(), FRAMES_TO_BRIDGE as u64);
    assert_eq!(previous_frame_id, Some(FRAMES_TO_BRIDGE as u64 - 1));
}
