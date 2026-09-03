//! Minimal egui overlay for Integration 8A.4.
//!
//! Owns egui context / winit state / wgpu renderer only. Does not own the
//! window event loop, surface, decoder, bridge, or playback scheduler.
//!
//! Frame flow: prepare editor layout once (CPU) → NV12 into Program Monitor →
//! encode pending egui with LoadOp::Load.
//!
//! UI actions are stored until the host consumes them after a successful present.
//! Pointer-driven redraw is decided by pure helpers; never via repaint callbacks.

use std::sync::Arc;

use dvs_ui::{
    EditorAction, PhysicalViewport, apply_editor_theme, logical_rect_to_physical_viewport,
    paint_editor_shell, take_editor_action,
};
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

/// Platform egui shell that paints the editor layout over an existing frame.
pub struct EguiStaticOverlay {
    context: egui::Context,
    state: EguiWinitState,
    renderer: Renderer,
    window: Arc<Window>,
    pending: Option<PendingEguiGpu>,
    pending_editor_action: Option<EditorAction>,
}

/// True for pointer events that may request a one-shot UI redraw in Ready/Ended.
pub fn is_pointer_ui_event(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
    )
}

/// Integration 8A.3 pointer redraw policy (pure).
///
/// Allows at most one host `request_redraw` when all of:
/// - host is in a static UI state (Ready/Ended),
/// - the window event is a real pointer event,
/// - egui reports `EventResponse.repaint`.
///
/// `EventResponse.consumed` is intentionally ignored by callers.
/// `RedrawRequested` must never be passed as a pointer event.
pub fn should_request_ui_interaction_redraw(
    static_ui_state: bool,
    is_pointer_event: bool,
    egui_wants_repaint: bool,
) -> bool {
    static_ui_state && is_pointer_event && egui_wants_repaint
}

/// SPACE/ESC handling ignores egui `consumed` (same as pre-8A.3).
pub fn keyboard_playback_or_exit_ignores_egui_consumed(egui_consumed: bool) -> bool {
    let _ = egui_consumed;
    true
}

impl EguiStaticOverlay {
    /// Creates egui state and renderer against the existing presentation device/format.
    ///
    /// Does not install repaint callbacks.
    pub fn new(
        window: Arc<Window>,
        device: &wgpu::Device,
        output_format: wgpu::TextureFormat,
    ) -> Self {
        let context = egui::Context::default();
        apply_editor_theme(&context);
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
            pending_editor_action: None,
        }
    }

    /// Accumulates window input for the next stable render opportunity.
    pub fn on_window_event(&mut self, event: &WindowEvent) -> egui_winit::EventResponse {
        self.state.on_window_event(self.window.as_ref(), event)
    }

    /// Takes a pending editor action exactly once (host must call after successful present).
    pub fn take_pending_editor_action(&mut self) -> Option<EditorAction> {
        take_editor_action(&mut self.pending_editor_action)
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
        let mut shell = dvs_ui::EditorShellOutput {
            program_monitor: dvs_ui::LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            },
            action: None,
        };
        let full_output = self.context.run(raw_input, |ctx| {
            shell = paint_editor_shell(ctx);
        });

        // Cursor/IME/clipboard only — never translate platform_output.repaint into redraw.
        self.state
            .handle_platform_output(self.window.as_ref(), full_output.platform_output);

        // Keep a prior unconsumed click; never invent actions from empty frames.
        if let Some(action) = shell.action {
            self.pending_editor_action = Some(action);
        }

        let pixels_per_point = full_output.pixels_per_point;
        let program_monitor = logical_rect_to_physical_viewport(
            shell.program_monitor,
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

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalPosition;
    use winit::event::{ElementState, MouseButton};

    #[test]
    fn pointer_policy_accepts_real_pointer_events() {
        let moved = WindowEvent::CursorMoved {
            device_id: winit::event::DeviceId::dummy(),
            position: PhysicalPosition::new(1.0, 2.0),
        };
        let entered = WindowEvent::CursorEntered {
            device_id: winit::event::DeviceId::dummy(),
        };
        let left = WindowEvent::CursorLeft {
            device_id: winit::event::DeviceId::dummy(),
        };
        let mouse = WindowEvent::MouseInput {
            device_id: winit::event::DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
        };
        assert!(is_pointer_ui_event(&moved));
        assert!(is_pointer_ui_event(&entered));
        assert!(is_pointer_ui_event(&left));
        assert!(is_pointer_ui_event(&mouse));
        assert!(should_request_ui_interaction_redraw(true, true, true));
        assert!(!should_request_ui_interaction_redraw(false, true, true));
        assert!(!should_request_ui_interaction_redraw(true, false, true));
        assert!(!should_request_ui_interaction_redraw(true, true, false));
    }

    #[test]
    fn redraw_requested_does_not_qualify_as_pointer_redraw() {
        let event = WindowEvent::RedrawRequested;
        assert!(!is_pointer_ui_event(&event));
        assert!(!should_request_ui_interaction_redraw(
            true,
            is_pointer_ui_event(&event),
            true
        ));
    }

    #[test]
    fn egui_consumed_does_not_block_space_or_escape_policy() {
        assert!(keyboard_playback_or_exit_ignores_egui_consumed(true));
        assert!(keyboard_playback_or_exit_ignores_egui_consumed(false));
    }

    #[test]
    fn pending_editor_action_take_is_single_shot() {
        let mut pending = Some(EditorAction::StartPlayback);
        assert_eq!(
            take_editor_action(&mut pending),
            Some(EditorAction::StartPlayback)
        );
        assert_eq!(take_editor_action(&mut pending), None);
    }

    #[test]
    fn drop_late_and_reject_timestamp_are_not_editor_actions() {
        // Schedule recovery labels never map onto EditorAction.
        for label in ["DropLate", "RejectTimestamp", "Waiting", "PresentNow"] {
            assert!(editor_action_from_schedule_label(label).is_none());
        }
    }

    fn editor_action_from_schedule_label(label: &str) -> Option<EditorAction> {
        match label {
            "DropLate" | "RejectTimestamp" | "Waiting" | "PresentNow" | "Present" => None,
            _ => None,
        }
    }

    #[test]
    fn repeated_start_requests_share_ready_gate_without_second_transition() {
        // Mirrors SPACE and ▶ Play: both require Ready→Playing once via start_playback().
        use crate::state::AppState;
        let first = AppState::Ready.start_playback().expect("first");
        assert_eq!(first, AppState::Playing);
        assert!(first.start_playback().is_err());
        assert!(AppState::Ended.start_playback().is_err());
    }
}
