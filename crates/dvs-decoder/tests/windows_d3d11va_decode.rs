//! Windows hardware integration test for FFmpeg D3D11VA decode.

#![cfg(target_os = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use dvs_decoder::{DecodedD3d11Frame, DecoderSession};
use dvs_gpu::{GpuBootstrap, GpuContext, SurfaceWindowTarget};
use dvs_media::VideoPixelFormat;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::windows::EventLoopBuilderExtWindows;
use winit::window::{Window, WindowId};

const DEFAULT_FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SETUP_DOC: &str = "docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md";
const FRAMES_TO_DECODE: usize = 5;

struct DecodeTestApp {
    finished: Option<Result<DecodeTestContext, String>>,
}

struct DecodeTestContext {
    _window: Arc<Window>,
    gpu: GpuContext,
}

impl ApplicationHandler for DecodeTestApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.finished.is_some() {
            return;
        }

        let window = match event_loop.create_window(
            Window::default_attributes()
                .with_title("dvs-decoder D3D11VA test")
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

fn initialize_gpu(window: Arc<Window>) -> Result<DecodeTestContext, String> {
    let gpu = pollster::block_on(GpuBootstrap::initialize(
        window.clone() as Arc<dyn SurfaceWindowTarget>
    ))
    .map_err(|e| format!("GpuBootstrap::initialize failed: {e}"))?;

    Ok(DecodeTestContext {
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

#[test]
#[ignore = "requires Windows GPU, FFmpeg dev libraries, and docs/fixtures/test_4k_hevc_8bit30.mp4"]
fn windows_d3d11va_decode_fixture() {
    let fixture = fixture_path();
    require_fixture(&fixture).expect("fixture setup");

    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    let mut app = DecodeTestApp { finished: None };
    event_loop.run_app(&mut app).expect("event loop run");

    let ctx = app
        .finished
        .expect("app finished")
        .expect("GPU initialization");

    let required_luid = required_adapter_luid(&ctx.gpu).expect("wgpu LUID");

    let mut session = DecoderSession::open(&fixture, required_luid).expect("DecoderSession::open");
    assert_eq!(session.adapter_luid(), required_luid);

    let mut previous_frame_id = None;
    let mut decoded_count = 0usize;
    let mut frame_ids = Vec::with_capacity(FRAMES_TO_DECODE);

    while decoded_count < FRAMES_TO_DECODE {
        let decoded = session
            .decode_next_d3d11()
            .expect("decode_next_d3d11")
            .unwrap_or_else(|| {
                panic!("decoder EOF before {FRAMES_TO_DECODE} frames (got {decoded_count})");
            });

        validate_decoded_frame(&decoded, previous_frame_id, decoded_count);
        let frame_id = decoded.metadata().frame_id().value();
        frame_ids.push(frame_id);
        previous_frame_id = Some(frame_id);

        let (metadata, surface) = decoded.into_parts();
        assert_eq!(metadata.frame_id().value(), frame_id);
        let _surface = surface;

        decoded_count += 1;
    }

    eprintln!(
        "decoded {decoded_count} frames; frame_ids={frame_ids:?}; adapter_luid={required_luid:?}"
    );

    let saw_eof = loop {
        match session.decode_next_d3d11().expect("decode drain") {
            Some(_frame) => {}
            None => break true,
        }
    };

    assert!(saw_eof, "EOF drain should return Ok(None)");
    assert!(decoded_count >= FRAMES_TO_DECODE);
}

fn validate_decoded_frame(
    decoded: &DecodedD3d11Frame<'_>,
    previous_frame_id: Option<u64>,
    index: usize,
) {
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
}
