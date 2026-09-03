//! Minimal egui overlay for Integration 8A.1.
//!
//! Owns egui context / winit state / wgpu renderer only. Does not own the
//! window event loop, surface, decoder, bridge, or playback scheduler.
//! Never requests redraws from egui repaint signals.

use std::sync::Arc;

use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::error::AppError;

/// Platform egui shell that paints the static `dvs-ui` label over an existing frame.
pub struct EguiStaticOverlay {
    context: egui::Context,
    state: EguiWinitState,
    renderer: Renderer,
    window: Arc<Window>,
}

impl EguiStaticOverlay {
    /// Creates egui state and renderer against the existing presentation device/format.
    ///
    /// Does not install repaint callbacks and does not request redraws.
    pub fn new(
        window: Arc<Window>,
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
    ) -> Self {
        let context = egui::Context::default();
        let max_texture_side = device.limits().max_texture_dimension_2d as usize;
        let state = EguiWinitState::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture_side),
        );
        let renderer = Renderer::new(
            device,
            output_format,
            RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            },
        );

        Self {
            context,
            state,
            renderer,
            window,
        }
    }

    /// Accumulates window input for the next stable render opportunity.
    ///
    /// The returned [`egui_winit::EventResponse`] is intentionally unused: Integration
    /// 8A.1 must not call `request_redraw` from egui repaint/consumed signals.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> egui_winit::EventResponse {
        self.state.on_window_event(self.window.as_ref(), event)
    }

    /// Runs the static label pass and encodes egui with `LoadOp::Load` onto `target_view`.
    ///
    /// Call only from an existing Integration 7 present/display path after NV12 encoding
    /// and before the single queue submit.
    pub fn encode_after_video(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), AppError> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        let raw_input = self.state.take_egui_input(self.window.as_ref());
        let full_output = self.context.run(raw_input, |ctx| {
            dvs_ui::paint_static_shell_label(ctx);
        });

        // Cursor/IME/clipboard only — never translate repaint into request_redraw.
        self.state
            .handle_platform_output(self.window.as_ref(), full_output.platform_output);

        let pixels_per_point = full_output.pixels_per_point;
        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point,
        };

        for (id, image_delta) in &full_output.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }

        let paint_jobs = self
            .context
            .tessellate(full_output.shapes, pixels_per_point);

        let callback_buffers =
            self.renderer
                .update_buffers(device, queue, encoder, &paint_jobs, &screen_descriptor);
        if !callback_buffers.is_empty() {
            return Err(AppError::Fatal(
                "egui overlay produced unexpected callback command buffers".to_string(),
            ));
        }

        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("dvs-app-egui-overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target_view,
                        resolve_target: None,
                        depth_slice: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();
            self.renderer
                .render(&mut render_pass, &paint_jobs, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        Ok(())
    }
}
