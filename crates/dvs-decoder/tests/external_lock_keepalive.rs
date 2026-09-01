//! Ownership proof: external lock keepalive retains FFmpeg hardware device.

#![cfg(target_os = "windows")]

use dvs_decoder::DecoderSession;
use dvs_gpu::{
    D3d11ExternalContextLock, D3d11ExternalContextLockConfigError,
    D3d11ExternalContextLockKeepalive, WindowsD3d11SharedNv12Producer,
};

#[test]
fn external_lock_requires_owned_keepalive() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<D3d11ExternalContextLock>();
    // `assert_copy::<D3d11ExternalContextLock>()` must not compile; the pre-audit `Copy`
    // token allowed dangling `lock_ctx` after `DecoderSession` drop.
}

#[test]
fn external_lock_raw_constructor_is_unsafe() {
    let keepalive = D3d11ExternalContextLockKeepalive::new(());
    // SAFETY: Both callbacks are absent; null `lock_ctx` is the documented no-op configuration.
    let lock = unsafe {
        D3d11ExternalContextLock::new_with_keepalive(None, None, std::ptr::null_mut(), keepalive)
    }
    .expect("no-op lock");
    drop(lock);
}

#[test]
fn external_lock_rejects_fabricated_half_pairs_without_unsafe_invoke() {
    extern "C" fn noop(_ctx: *mut std::ffi::c_void) {}

    let err = D3d11ExternalContextLock::validate_callback_configuration(
        Some(noop),
        None,
        std::ptr::null_mut(),
    )
    .expect_err("half pair");
    assert_eq!(
        err,
        D3d11ExternalContextLockConfigError::AcquireWithoutRelease
    );
}

#[test]
#[ignore = "requires Windows GPU, FFmpeg dev libraries, and docs/fixtures/test_4k_hevc_8bit30.mp4"]
fn producer_lock_survives_decoder_session_drop() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use dvs_gpu::{GpuBootstrap, SharedNv12TextureDesc, SurfaceWindowTarget};
    use winit::application::ApplicationHandler;
    use winit::event::WindowEvent;
    use winit::event_loop::{ActiveEventLoop, EventLoop};
    use winit::platform::windows::EventLoopBuilderExtWindows;
    use winit::window::{Window, WindowId};

    struct App {
        gpu: Option<dvs_gpu::GpuContext>,
    }

    impl ApplicationHandler for App {
        fn resumed(&mut self, event_loop: &ActiveEventLoop) {
            let window = Arc::new(
                event_loop
                    .create_window(Window::default_attributes())
                    .expect("window"),
            );
            self.gpu = Some(
                pollster::block_on(GpuBootstrap::initialize(
                    window as Arc<dyn SurfaceWindowTarget>,
                ))
                .expect("gpu"),
            );
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

    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/fixtures/test_4k_hevc_8bit30.mp4");
    if !fixture.is_file() {
        return;
    }

    let mut app = App { gpu: None };
    let event_loop = EventLoop::builder()
        .with_any_thread(true)
        .build()
        .expect("event loop");
    event_loop.run_app(&mut app).expect("run");
    let gpu = app.gpu.expect("gpu");
    let luid = gpu.adapter_identity().dxgi_luid().expect("luid");

    let (device, context, lock, desc) = {
        let mut session = DecoderSession::open(&fixture, luid).expect("open");
        let hw = session.d3d11_hardware().expect("hw");
        let device = hw.device().clone();
        let context = hw.context().clone();
        let lock = session.external_context_lock().expect("lock");
        let decoded = session.decode_next_d3d11().expect("decode").expect("frame");
        let allocation = decoded.metadata().dimensions().allocation();
        let desc =
            SharedNv12TextureDesc::new(allocation.width(), allocation.height()).expect("desc");
        (device, context, lock, desc)
    };

    let producer = WindowsD3d11SharedNv12Producer::new_with_external_lock(
        &device,
        &context,
        luid,
        desc,
        Some(lock),
    )
    .expect("producer");
    let _ = producer.adapter_luid();
}
