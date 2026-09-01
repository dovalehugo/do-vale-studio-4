mod nv12_pattern;
mod renderer;

use std::sync::Arc;

use renderer::{BackendChoice, Nv12Renderer};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Nv12Renderer>,
    backend: BackendChoice,
    max_frames: Option<u64>,
}

impl App {
    fn new(backend: BackendChoice, max_frames: Option<u64>) -> Self {
        Self {
            window: None,
            renderer: None,
            backend,
            max_frames,
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(format!(
                            "DVS4 Experiment 1 — NV12→RGB ({})",
                            self.backend.label()
                        ))
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32)),
                )
                .expect("create window"),
        );

        let renderer =
            pollster::block_on(Nv12Renderer::new(window.clone(), self.backend)).expect("renderer");

        renderer.print_init_report();
        self.renderer = Some(renderer);
        self.window = Some(window);
        event_loop.set_control_flow(ControlFlow::Poll);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(renderer) = &self.renderer {
                    renderer.print_frame_summary();
                }
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    match renderer.render() {
                        Ok(()) => {
                            if let Some(max) = self.max_frames {
                                if renderer.frame_metrics().frames_presented >= max {
                                    renderer.print_frame_summary();
                                    event_loop.exit();
                                }
                            }
                        }
                        Err(wgpu::SurfaceError::Lost) => {
                            let size = self.window.as_ref().unwrap().inner_size();
                            renderer.resize(size.width, size.height);
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => event_loop.exit(),
                        Err(err) => eprintln!("render error: {err:?}"),
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed() && matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                    if let Some(renderer) = &self.renderer {
                        renderer.print_frame_summary();
                    }
                    event_loop.exit();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

fn parse_args() -> (BackendChoice, Option<u64>) {
    let mut backend = None;
    let mut max_frames = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--backend" || arg == "-b" {
            if let Some(value) = args.next() {
                backend = BackendChoice::parse(&value);
            }
        } else if let Some(stripped) = arg.strip_prefix("--backend=") {
            backend = BackendChoice::parse(stripped);
        } else if arg == "--frames" || arg == "-f" {
            if let Some(value) = args.next() {
                max_frames = value.parse().ok();
            }
        } else if let Some(stripped) = arg.strip_prefix("--frames=") {
            max_frames = stripped.parse().ok();
        }
    }

    if backend.is_none() {
        if let Ok(value) = std::env::var("DVS_GPU_BACKEND") {
            backend = BackendChoice::parse(&value);
        }
    }

    (backend.unwrap_or(BackendChoice::Dx12), max_frames)
}

fn main() {
    let (backend, max_frames) = parse_args();
    println!("Starting GPU Experiment 1 with backend: {}", backend.label());
    if let Some(frames) = max_frames {
        println!("Auto-exit after {frames} frames");
    }

    let event_loop = EventLoop::new().expect("event loop");
    let mut app = App::new(backend, max_frames);
    event_loop.run_app(&mut app).expect("run app");
}
