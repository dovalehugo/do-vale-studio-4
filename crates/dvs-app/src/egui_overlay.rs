//! Minimal egui overlay for Integration 8A.2.
//!
//! Owns egui context / winit state / wgpu renderer only. Does not own the
//! window event loop, surface, decoder, bridge, or playback scheduler.
//! Never requests redraws from egui repaint signals.
//!
//! Frame flow: prepare editor layout once (CPU) → NV12 into Program Monitor →
//! encode pending egui with LoadOp::Load.

use std::sync::Arc;

use dvs_ui::{PhysicalViewport, logical_rect_to_physical_viewport, paint_editor_shell};
use egui::epaint::ClippedPrimitive;
use egui_wgpu::{Renderer, RendererOptions, ScreenDescriptor};
use egui_winit::State as EguiWinitState;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::error::AppError;

struct PendingEguiGpu {
    pixels_per_point: f32,
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
}

/// Platform egui shell that paints the static editor layout over an existing frame.
pub struct EguiStaticOverlay {
    context: egui::Context,
    state: EguiWinitState,
    renderer: Renderer,
    window: Arc<Window>,
    pending: Option<PendingEguiGpu>,
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
            pending: None,
        }
    }

    /// Accumulates window input for the next stable render opportunity.
    ///
    /// The returned [`egui_winit::EventResponse`] is intentionally unused: Integration
    /// 8A must not call `request_redraw` from egui repaint/consumed signals.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> egui_winit::EventResponse {
        self.state.on_window_event(self.window.as_ref(), event)
    }

    /// Runs the editor shell once and returns the Program Monitor physical viewport.
    ///
    /// Stores GPU-ready egui output for [`Self::encode_pending_after_video`]. Call at most
    /// once per presented frame, before creating the command encoder.
    pub fn prepare_editor_frame(
        &mut self,
        surface_width: u32,
        surface_height: u32,
    ) -> Result<PhysicalViewport, AppError> {
        if surface_width == 0 || surface_height == 0 {
            self.pending = None;
            return Ok(PhysicalViewport {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            });
        }

        let raw_input = self.state.take_egui_input(self.window.as_ref());
        let mut monitor_logical = dvs_ui::LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        let full_output = self.context.run(raw_input, |ctx| {
            monitor_logical = paint_editor_shell(ctx);
        });

        // Cursor/IME/clipboard only — never translate repaint into request_redraw.
        self.state
            .handle_platform_output(self.window.as_ref(), full_output.platform_output);

        let pixels_per_point = full_output.pixels_per_point;
        let program_monitor = logical_rect_to_physical_viewport(
            monitor_logical,
            pixels_per_point,
            surface_width,
            surface_height,
        );

        self.pending = Some(PendingEguiGpu {
            pixels_per_point,
            shapes: full_output.shapes,
            textures_delta: full_output.textures_delta,
        });

        Ok(program_monitor)
    }

    /// Encodes the pending egui frame with `LoadOp::Load` onto `target_view`.
    ///
    /// Call only after NV12 encoding and before the single queue submit.
    pub fn encode_pending_after_video(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &wgpu::TextureView,
        width: u32,
        height: u32,
    ) -> Result<(), AppError> {
        let Some(pending) = self.pending.take() else {
            return Ok(());
        };
        if width == 0 || height == 0 {
            return Ok(());
        }

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [width, height],
            pixels_per_point: pending.pixels_per_point,
        };

        for (id, image_delta) in &pending.textures_delta.set {
            self.renderer
                .update_texture(device, queue, *id, image_delta);
        }

        let paint_jobs: Vec<ClippedPrimitive> = self
            .context
            .tessellate(pending.shapes, pending.pixels_per_point);

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

        for id in &pending.textures_delta.free {
            self.renderer.free_texture(id);
        }

        Ok(())
    }
}
