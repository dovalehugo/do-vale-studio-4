//! Windows hardware integration test: decode → bridge → NV12 render (90 frames).

#![cfg(target_os = "windows")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use dvs_decoder::{DecodedD3d11Frame, DecoderSession};
use dvs_gpu::{
    FenceTimeline, GpuBootstrap, GpuContext, GpuVideoPixelFormat, SharedNv12TextureDesc,
    SurfaceWindowTarget, WindowsD3d11SharedNv12Producer, WindowsD3d11WgpuInteropBridge,
};
use dvs_media::VideoPixelFormat;
use dvs_render::{Nv12Renderer, Nv12RendererConfig, RenderSurface};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

const DEFAULT_FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";
const FRAMES_TO_RENDER: usize = 90;

struct RenderTestApp {
    finished: Option<Result<RenderTestContext, String>>,
}

struct RenderTestContext {
    _window: Arc<Window>,
    gpu: GpuContext,
    surface: RenderSurface,
}

impl ApplicationHandler for RenderTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("dvs-render NV12 hardware test")
                .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
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

fn initialize_gpu(window: Arc<Window>) -> Result<RenderTestContext, String> {
    let gpu = pollster::block_on(GpuBootstrap::initialize(
        window.clone() as Arc<dyn SurfaceWindowTarget>
    ))
    .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;

    let size = window.inner_size();
    let surface = RenderSurface::configure(&gpu, size.width.max(1), size.height.max(1))
        .map_err(|e| format!("RenderSurface::configure failed: {e}"))?;

    Ok(RenderTestContext {
        _window: window,
        gpu,
        surface,
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

fn validate_decoded_metadata(
    decoded: &DecodedD3d11Frame<'_>,
    previous_frame_id: Option<u64>,
    index: usize,
) -> dvs_media::VideoFrameMetadata {
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

    metadata
}

#[allow(clippy::too_many_arguments)]
fn render_one_frame(
    session: &mut DecoderSession,
    bridge: &mut WindowsD3d11WgpuInteropBridge,
    gpu: &GpuContext,
    surface: &RenderSurface,
    renderer: &mut Nv12Renderer,
    timeline: &mut FenceTimeline,
    previous_frame_id: &mut Option<u64>,
    rendered_count: &mut usize,
    array_slices: &mut BTreeSet<u32>,
) {
    let decoded = session
        .decode_next_d3d11()
        .expect("decode_next_d3d11")
        .unwrap_or_else(|| {
            panic!("decoder EOF before {FRAMES_TO_RENDER} frames (got {rendered_count})")
        });

    let metadata = validate_decoded_metadata(&decoded, *previous_frame_id, *rendered_count);
    let frame_id = metadata.frame_id().value();

    let values = timeline.current().expect("timeline values");
    let (metadata, surface_ref) = decoded.into_parts();
    assert_eq!(metadata.frame_id().value(), frame_id);

    array_slices.insert(surface_ref.array_slice());

    let video = bridge
        .prepare_frame(gpu, surface_ref, values)
        .expect("prepare_frame");

    assert_eq!(video.pixel_format(), GpuVideoPixelFormat::Nv12);
    assert_eq!(video.allocation_width(), 3840);
    assert_eq!(video.allocation_height(), 2176);

    let (surface_texture, target_view) = surface.acquire_frame(gpu).expect("acquire_frame");
    let config = surface.configuration();

    let mut encoder = gpu
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("dvs-render-nv12-hardware"),
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
        .expect("encode_frame");

    gpu.queue().submit(Some(encoder.finish()));
    bridge
        .signal_consumed_after_submit(gpu, values)
        .expect("signal_consumed_after_submit");
    timeline.advance().expect("timeline advance");
    surface_texture.present();

    *previous_frame_id = Some(frame_id);
    *rendered_count += 1;
}

#[test]
#[ignore = "requires Windows GPU, FFmpeg dev libraries, and docs/fixtures/test_4k_hevc_8bit30.mp4"]
fn windows_nv12_render_fixture() {
    let fixture = fixture_path();
    require_fixture(&fixture).expect("fixture setup");

    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut app = RenderTestApp { finished: None };
    event_loop.run_app(&mut app).expect("event loop run");

    let ctx = app
        .finished
        .expect("app finished")
        .expect("GPU initialization");

    let wgpu_luid = ctx.gpu.adapter_identity().dxgi_luid().expect("wgpu LUID");

    let mut session = DecoderSession::open(&fixture, wgpu_luid).expect("DecoderSession::open");

    let (d3d11_device, d3d11_context) = {
        let hw = session.d3d11_hardware().expect("d3d11_hardware");
        (hw.device().clone(), hw.context().clone())
    };
    let external_context_lock = session
        .external_context_lock()
        .expect("external_context_lock");

    let first = session
        .decode_next_d3d11()
        .expect("first decode")
        .expect("first frame");
    let first_metadata = validate_decoded_metadata(&first, None, 0);
    let color = first_metadata.color();
    let allocation = first_metadata.dimensions().allocation();
    let visible = first_metadata.dimensions().visible();
    let (first_metadata, first_surface) = first.into_parts();
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

    let mut bridge =
        WindowsD3d11WgpuInteropBridge::new(&ctx.gpu, producer).expect("interop bridge");
    assert_eq!(bridge.shared_handle_open_counts(), (1, 1));

    let encoding = ctx.surface.output_encoding();
    let mut renderer = Nv12Renderer::new(
        ctx.gpu.device(),
        Nv12RendererConfig {
            target_format: encoding.format,
        },
    )
    .expect("Nv12Renderer::new");
    let init_stats = renderer.resource_stats();

    let mut timeline = FenceTimeline::new();
    let mut previous_frame_id = Some(first_metadata.frame_id().value());
    let mut rendered_count = 0usize;
    let mut array_slices = BTreeSet::new();
    array_slices.insert(first_array_slice);

    // First frame: prepare → render → submit → signal_consumed
    {
        let values = timeline.current().expect("timeline values");
        let video = bridge
            .prepare_frame(&ctx.gpu, first_surface, values)
            .expect("prepare_frame first");
        let (surface_texture, target_view) = ctx.surface.acquire_frame(&ctx.gpu).expect("acquire");
        let config = ctx.surface.configuration();
        let mut encoder =
            ctx.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("dvs-render-nv12-first"),
                });
        renderer
            .encode_frame(
                ctx.gpu.device(),
                ctx.gpu.queue(),
                &mut encoder,
                video,
                first_metadata,
                &target_view,
                config.width,
                config.height,
            )
            .expect("encode first");
        ctx.gpu.queue().submit(Some(encoder.finish()));
        bridge
            .signal_consumed_after_submit(&ctx.gpu, values)
            .expect("signal_consumed first");
        timeline.advance().expect("advance first");
        surface_texture.present();
        rendered_count += 1;
    }

    while rendered_count < FRAMES_TO_RENDER {
        render_one_frame(
            &mut session,
            &mut bridge,
            &ctx.gpu,
            &ctx.surface,
            &mut renderer,
            &mut timeline,
            &mut previous_frame_id,
            &mut rendered_count,
            &mut array_slices,
        );
    }

    let saw_eof = loop {
        match session.decode_next_d3d11().expect("decode drain") {
            Some(_frame) => {}
            None => break true,
        }
    };

    let final_stats = renderer.resource_stats();
    let final_values = timeline.current().expect("final timeline values");

    eprintln!("=== Integration 5 hardware summary ===");
    eprintln!("decoded: {rendered_count}");
    eprintln!("bridged: {rendered_count}");
    eprintln!("rendered: {rendered_count}");
    eprintln!("first_frame_id: 0");
    eprintln!(
        "last_frame_id: {}",
        previous_frame_id.expect("last frame id")
    );
    eprintln!("pixel_format: NV12");
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
    eprintln!("color_matrix: {:?}", color.matrix());
    eprintln!("color_range: {:?}", color.range());
    eprintln!("color_transfer: {:?}", color.transfer());
    eprintln!("color_primaries: {:?}", color.primaries());
    eprintln!("target_format: {:?}", encoding.format);
    eprintln!("output_encoding: {}", encoding.summary());
    eprintln!("distinct_source_array_slices: {array_slices:?}");
    eprintln!(
        "bridge_shared_handle_open_counts: {:?}",
        bridge.shared_handle_open_counts()
    );
    eprintln!("renderer_shader_modules: {}", final_stats.shader_modules);
    eprintln!(
        "renderer_render_pipelines: {}",
        final_stats.render_pipelines
    );
    eprintln!("renderer_samplers: {}", final_stats.samplers);
    eprintln!(
        "renderer_bind_group_layouts: {}",
        final_stats.bind_group_layouts
    );
    eprintln!("renderer_uniform_buffers: {}", final_stats.uniform_buffers);
    eprintln!("renderer_bind_groups: {}", final_stats.bind_groups);
    eprintln!("eof_drain: {saw_eof}");
    eprintln!("init_shader_modules: {}", init_stats.shader_modules);

    assert!(saw_eof, "EOF drain should return Ok(None)");
    assert_eq!(rendered_count, FRAMES_TO_RENDER);
    assert_eq!(final_values.frame_index(), FRAMES_TO_RENDER as u64);
    assert_eq!(previous_frame_id, Some(FRAMES_TO_RENDER as u64 - 1));
    assert_eq!(final_stats.shader_modules, 2);
    assert_eq!(final_stats.render_pipelines, 1);
    assert_eq!(final_stats.samplers, 1);
    assert_eq!(final_stats.bind_group_layouts, 2);
    assert_eq!(final_stats.uniform_buffers, 2);
    assert_eq!(final_stats.bind_groups, 2);
    assert_eq!(bridge.shared_handle_open_counts(), (1, 1));
}
