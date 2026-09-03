//! Safe UI definitions for Do Vale Studio 4.
//!
//! This crate owns egui widgets and layout only. It must not own windows,
//! surfaces, GPU devices, decoders, bridges, or playback scheduling.

#![forbid(unsafe_code)]

/// Logical (egui points) axis-aligned rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LogicalRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Physical pixel viewport clamped to a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalViewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Fixed Integration 8A.2 shell metrics in egui points.
pub const TOP_BAR_HEIGHT: f32 = 28.0;
pub const TIMELINE_HEIGHT: f32 = 160.0;
pub const MEDIA_POOL_WIDTH: f32 = 220.0;
pub const INSPECTOR_WIDTH: f32 = 240.0;

const PANEL_FILL: egui::Color32 = egui::Color32::from_rgb(28, 28, 30);
const PANEL_STROKE: egui::Color32 = egui::Color32::from_rgb(55, 55, 58);
const TITLE_COLOR: egui::Color32 = egui::Color32::from_rgb(210, 210, 214);

fn opaque_panel_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(PANEL_FILL)
        .stroke(egui::Stroke::new(1.0_f32, PANEL_STROKE))
        .inner_margin(egui::Margin::symmetric(8, 6))
}

fn panel_title(ui: &mut egui::Ui, title: &str) {
    ui.add(
        egui::Label::new(egui::RichText::new(title).size(13.0).color(TITLE_COLOR))
            .selectable(false),
    );
}

/// Converts a logical egui rect to a physical viewport clamped to the surface.
///
/// Zero or invalid inputs yield a zero-sized viewport without panicking.
pub fn logical_rect_to_physical_viewport(
    rect: LogicalRect,
    pixels_per_point: f32,
    surface_width: u32,
    surface_height: u32,
) -> PhysicalViewport {
    if !(pixels_per_point.is_finite() && pixels_per_point > 0.0)
        || surface_width == 0
        || surface_height == 0
        || !rect.x.is_finite()
        || !rect.y.is_finite()
        || !rect.width.is_finite()
        || !rect.height.is_finite()
        || rect.width <= 0.0
        || rect.height <= 0.0
    {
        return PhysicalViewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    let mut x0 = (rect.x * pixels_per_point).floor() as i64;
    let mut y0 = (rect.y * pixels_per_point).floor() as i64;
    let mut x1 = ((rect.x + rect.width) * pixels_per_point).ceil() as i64;
    let mut y1 = ((rect.y + rect.height) * pixels_per_point).ceil() as i64;

    let sw = surface_width as i64;
    let sh = surface_height as i64;
    x0 = x0.clamp(0, sw);
    y0 = y0.clamp(0, sh);
    x1 = x1.clamp(0, sw);
    y1 = y1.clamp(0, sh);

    if x1 <= x0 || y1 <= y0 {
        return PhysicalViewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }

    PhysicalViewport {
        x: x0 as u32,
        y: y0 as u32,
        width: (x1 - x0) as u32,
        height: (y1 - y0) as u32,
    }
}

/// Paints the Integration 8A.2 static editor shell and returns the Program Monitor rect.
///
/// Panels are non-interactive labels only. The Program Monitor interior uses a
/// transparent frame so the NV12 pass remains visible under egui `LoadOp::Load`.
pub fn paint_editor_shell(ctx: &egui::Context) -> LogicalRect {
    egui::TopBottomPanel::top("dvs_ui_top_bar")
        .exact_height(TOP_BAR_HEIGHT)
        .resizable(false)
        .frame(opaque_panel_frame())
        .show(ctx, |ui| {
            panel_title(ui, "Do Vale Studio 4");
        });

    egui::TopBottomPanel::bottom("dvs_ui_timeline")
        .exact_height(TIMELINE_HEIGHT)
        .resizable(false)
        .frame(opaque_panel_frame())
        .show(ctx, |ui| {
            panel_title(ui, "Timeline");
        });

    egui::SidePanel::left("dvs_ui_media_pool")
        .exact_width(MEDIA_POOL_WIDTH)
        .resizable(false)
        .frame(opaque_panel_frame())
        .show(ctx, |ui| {
            panel_title(ui, "Media Pool");
        });

    egui::SidePanel::right("dvs_ui_inspector")
        .exact_width(INSPECTOR_WIDTH)
        .resizable(false)
        .frame(opaque_panel_frame())
        .show(ctx, |ui| {
            panel_title(ui, "Inspector");
        });

    let mut monitor = LogicalRect {
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
    };

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, |ui| {
            let rect = ui.max_rect();
            monitor = LogicalRect {
                x: rect.min.x,
                y: rect.min.y,
                width: rect.width(),
                height: rect.height(),
            };
            // Title only; no opaque fill over the video area.
            ui.add(
                egui::Label::new(
                    egui::RichText::new("Program Monitor")
                        .size(12.0)
                        .color(TITLE_COLOR),
                )
                .selectable(false),
            );
        });

    monitor
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_to_physical_clamps_to_surface() {
        let rect = LogicalRect {
            x: -10.0,
            y: -5.0,
            width: 2000.0,
            height: 2000.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 1.0, 1280, 720);
        assert_eq!(vp.x, 0);
        assert_eq!(vp.y, 0);
        assert_eq!(vp.width, 1280);
        assert_eq!(vp.height, 720);
    }

    #[test]
    fn logical_to_physical_zero_surface_is_empty() {
        let rect = LogicalRect {
            x: 10.0,
            y: 10.0,
            width: 100.0,
            height: 80.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 1.0, 0, 720);
        assert_eq!(vp.width, 0);
        assert_eq!(vp.height, 0);
    }

    #[test]
    fn logical_to_physical_scales_with_pixels_per_point() {
        let rect = LogicalRect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        let vp = logical_rect_to_physical_viewport(rect, 2.0, 800, 600);
        assert_eq!(vp.x, 20);
        assert_eq!(vp.y, 40);
        assert_eq!(vp.width, 200);
        assert_eq!(vp.height, 100);
    }

    #[test]
    fn paint_editor_shell_produces_shapes_and_valid_monitor() {
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        };
        let mut monitor = LogicalRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        let output = ctx.run(raw, |ctx| {
            monitor = paint_editor_shell(ctx);
        });
        assert!(!output.shapes.is_empty());
        assert!(monitor.width > 0.0);
        assert!(monitor.height > 0.0);
        assert!(monitor.x >= MEDIA_POOL_WIDTH - 1.0);
        assert!(monitor.y >= TOP_BAR_HEIGHT - 1.0);

        let vp = logical_rect_to_physical_viewport(monitor, 1.0, 1280, 720);
        assert!(vp.width > 0);
        assert!(vp.height > 0);
        assert!(vp.x + vp.width <= 1280);
        assert!(vp.y + vp.height <= 720);
    }

    #[test]
    fn monitor_rect_remains_valid_for_tall_and_wide_windows() {
        for (w, h) in [(400.0_f32, 900.0), (1600.0, 500.0), (1280.0, 720.0)] {
            let ctx = egui::Context::default();
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            let mut monitor = LogicalRect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            };
            let _ = ctx.run(raw, |ctx| {
                monitor = paint_editor_shell(ctx);
            });
            let vp = logical_rect_to_physical_viewport(monitor, 1.0, w as u32, h as u32);
            // Even when side panels consume most width, conversion must not panic
            // and must stay inside the surface bounds.
            assert!(vp.x + vp.width <= w as u32);
            assert!(vp.y + vp.height <= h as u32);
        }
    }
}
