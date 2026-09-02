//! egui-winit/wgpu integration owned by the application composition root.

use std::sync::Arc;

use dvs_gpu::GpuContext;
use dvs_render::PhysicalRenderRect;
use dvs_ui::{EditorUi, EditorUiOutput, EditorViewModel, UiIntent};
use egui_wgpu::Renderer as EguiRenderer;
use egui_wgpu::ScreenDescriptor;
use egui_winit::EventResponse;
use egui_winit::State as EguiWinitState;
use wgpu::{
    Color, CommandEncoderDescriptor, LoadOp, Operations, RenderPassColorAttachment,
    RenderPassDescriptor, StoreOp, TextureFormat, TextureView,
};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::error::AppError;
use crate::monitor_rect::logical_rect_to_physical;

/// Editor shell background color (deep charcoal).
const EDITOR_BACKGROUND: Color = Color {
    r: 24.0 / 255.0,
    g: 26.0 / 255.0,
    b: 30.0 / 255.0,
    a: 1.0,
};

struct PendingEguiFrame {
    shapes: Vec<egui::epaint::ClippedShape>,
    textures_delta: egui::TexturesDelta,
    pixels_per_point: f32,
}

/// Owns egui context, winit bridge, renderer, and persistent editor UI state.
///
/// # Drop order
///
/// Declared before [`GpuContext`] consumers in the application struct so the egui
/// renderer drops while the wgpu device remains valid.
pub struct EguiEditorShell {
    context: egui::Context,
    winit: EguiWinitState,
    renderer: EguiRenderer,
    editor: EditorUi,
    pixels_per_point: f32,
    last_ui_output: EditorUiOutput,
    pending: Option<PendingEguiFrame>,
    last_repaint_delay: Option<std::time::Duration>,
}

impl EguiEditorShell {
    /// Creates the editor shell using the existing production wgpu device.
    pub fn new(window: &Arc<Window>, gpu: &GpuContext, surface_format: TextureFormat) -> Self {
        let context = egui::Context::default();
        let viewport_id = context.viewport_id();
        let winit = EguiWinitState::new(
            context.clone(),
            viewport_id,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let renderer = EguiRenderer::new(
            gpu.device(),
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );
        let editor = EditorUi::new(&context);
        Self {
            context,
            winit,
            renderer,
            editor,
            pixels_per_point: window.scale_factor() as f32,
            last_ui_output: EditorUiOutput::empty(),
            pending: None,
            last_repaint_delay: None,
        }
    }

    /// Forwards a winit event to egui-winit.
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> EventResponse {
        let response = self.winit.on_window_event(window, event);
        if let WindowEvent::ScaleFactorChanged { scale_factor, .. } = event {
            self.pixels_per_point = *scale_factor as f32;
        }
        response
    }

    /// Returns whether egui wants keyboard focus for text editing.
    pub fn wants_keyboard_input(&self) -> bool {
        self.context.wants_keyboard_input()
    }

    /// Returns the latest UI output from the previous frame.
    #[allow(dead_code)]
    pub const fn last_ui_output(&self) -> &EditorUiOutput {
        &self.last_ui_output
    }

    /// Returns the physical Program Monitor rectangle for the last laid-out frame.
    pub fn program_monitor_physical(
        &self,
        target_width: u32,
        target_height: u32,
    ) -> Option<PhysicalRenderRect> {
        logical_rect_to_physical(
            self.last_ui_output.program_monitor_rect,
            self.pixels_per_point,
            target_width,
            target_height,
        )
    }

    /// Begins an egui frame from the current winit input state.
    pub fn begin_frame(&mut self, window: &Window) {
        let raw_input = self.winit.take_egui_input(window);
        self.context.begin_pass(raw_input);
    }

    /// Lays out the editor shell and records UI output.
    pub fn show_editor(&mut self, model: &EditorViewModel) -> EditorUiOutput {
        let output = self.editor.show(&self.context, model);
        self.last_ui_output = output.clone();
        output
    }

    /// Ends the egui frame and stores tessellation input for [`Self::encode_ui`].
    pub fn end_frame(&mut self, window: &Window) -> egui::PlatformOutput {
        let full_output = self.context.end_pass();
        let platform_output = full_output.platform_output.clone();
        let repaint_delay = Self::repaint_delay_from_full_output(&full_output);
        self.last_repaint_delay = repaint_delay;
        self.pending = Some(PendingEguiFrame {
            shapes: full_output.shapes,
            textures_delta: full_output.textures_delta,
            pixels_per_point: full_output.pixels_per_point,
        });
        self.winit
            .handle_platform_output(window, full_output.platform_output);
        platform_output
    }

    /// Returns the repaint delay recorded for the most recent frame.
    pub const fn last_repaint_delay(&self) -> Option<std::time::Duration> {
        self.last_repaint_delay
    }

    /// Encodes the pending egui frame on top of the target texture.
    pub fn encode_ui(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        target_view: &TextureView,
        target_width: u32,
        target_height: u32,
        load: LoadOp<Color>,
    ) -> Result<(), AppError> {
        let pending = self
            .pending
            .take()
            .ok_or_else(|| AppError::Fatal("egui frame ended before encode_ui".to_string()))?;

        let paint_jobs = self
            .context
            .tessellate(pending.shapes, pending.pixels_per_point);

        let screen_descriptor = ScreenDescriptor {
            size_in_pixels: [target_width, target_height],
            pixels_per_point: pending.pixels_per_point,
        };

        for (id, image_delta) in &pending.textures_delta.set {
            self.renderer
                .update_texture(gpu.device(), gpu.queue(), *id, image_delta);
        }

        let user_cmd_bufs = self.renderer.update_buffers(
            gpu.device(),
            gpu.queue(),
            encoder,
            &paint_jobs,
            &screen_descriptor,
        );
        for cmd_buf in user_cmd_bufs {
            gpu.queue().submit(Some(cmd_buf));
        }

        {
            let render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("dvs-app-egui"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render(
                &mut render_pass.forget_lifetime(),
                &paint_jobs,
                &screen_descriptor,
            );
        }

        for id in &pending.textures_delta.free {
            self.renderer.free_texture(id);
        }

        Ok(())
    }

    /// Encodes a full-target background clear pass.
    pub fn encode_background_clear(encoder: &mut wgpu::CommandEncoder, target_view: &TextureView) {
        {
            let _pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("dvs-app-editor-bg"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(EDITOR_BACKGROUND),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
    }

    /// Creates a command encoder for one composed frame.
    pub fn create_frame_encoder(gpu: &GpuContext) -> wgpu::CommandEncoder {
        gpu.device()
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("dvs-app-composed-frame"),
            })
    }

    /// Returns the repaint delay requested by egui for a completed frame.
    pub fn repaint_delay_from_full_output(
        full_output: &egui::FullOutput,
    ) -> Option<std::time::Duration> {
        let delay = full_output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()?;
        if delay == std::time::Duration::MAX {
            None
        } else {
            Some(delay)
        }
    }
}

/// Extracts UI intents from editor output.
pub fn collect_intents(output: &EditorUiOutput) -> Vec<UiIntent> {
    output.intents.clone()
}

/// Returns whether SPACE should be ignored because egui owns keyboard focus.
pub fn space_blocked_by_egui(shell: &EguiEditorShell) -> bool {
    shell.wants_keyboard_input()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_background_color_is_opaque() {
        assert_eq!(EDITOR_BACKGROUND.a, 1.0);
    }
}
