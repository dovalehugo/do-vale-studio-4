//! Safe UI definitions for Do Vale Studio 4.
//!
//! This crate owns egui widgets and labels only. It must not own windows,
//! surfaces, GPU devices, decoders, bridges, or playback scheduling.

#![forbid(unsafe_code)]

/// Static Integration 8A.1 overlay label text.
pub const STATIC_SHELL_LABEL: &str = "Do Vale Studio 4 · UI 8A.1";

/// Draws the Integration 8A.1 static shell label into the egui context.
///
/// The label is non-interactive and does not request animation or continuous repaint.
pub fn paint_static_shell_label(ctx: &egui::Context) {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(STATIC_SHELL_LABEL)
                            .size(14.0)
                            .color(egui::Color32::from_rgb(230, 230, 230)),
                    )
                    .selectable(false),
                );
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_label_text_is_non_empty() {
        assert!(!STATIC_SHELL_LABEL.is_empty());
        assert!(STATIC_SHELL_LABEL.contains("8A.1"));
    }

    #[test]
    fn paint_static_shell_label_completes_one_pass() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        };
        let output = ctx.run(raw, |ctx| {
            paint_static_shell_label(ctx);
        });
        assert!(
            !output.shapes.is_empty(),
            "static label must produce paint shapes"
        );
    }
}
