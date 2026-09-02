//! Logical-to-physical Program Monitor rectangle conversion.

use dvs_render::PhysicalRenderRect;
use egui::Rect;

/// Rounding policy: origin floors, extent ceils so the monitor never shrinks below layout intent.
pub fn logical_rect_to_physical(
    rect: Rect,
    pixels_per_point: f32,
    target_width: u32,
    target_height: u32,
) -> Option<PhysicalRenderRect> {
    if !rect.is_positive() || pixels_per_point <= 0.0 {
        return None;
    }
    if target_width == 0 || target_height == 0 {
        return None;
    }

    let x = (rect.min.x * pixels_per_point).floor().max(0.0) as u32;
    let y = (rect.min.y * pixels_per_point).floor().max(0.0) as u32;
    let right = (rect.max.x * pixels_per_point).ceil().max(0.0) as u32;
    let bottom = (rect.max.y * pixels_per_point).ceil().max(0.0) as u32;

    let width = right.saturating_sub(x);
    let height = bottom.saturating_sub(y);
    if width == 0 || height == 0 {
        return None;
    }

    let clamped = dvs_render::clamp_physical_rect(
        PhysicalRenderRect {
            x,
            y,
            width,
            height,
        },
        target_width,
        target_height,
    )
    .ok()?;
    Some(clamped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_unit_scale() {
        let rect = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(800.0, 450.0));
        let physical = logical_rect_to_physical(rect, 1.0, 1280, 720).expect("physical");
        assert_eq!(physical.x, 100);
        assert_eq!(physical.y, 50);
        assert_eq!(physical.width, 800);
        assert_eq!(physical.height, 450);
    }

    #[test]
    fn fractional_dpi_125() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(100.0, 50.0));
        let physical = logical_rect_to_physical(rect, 1.25, 1920, 1080).expect("physical");
        assert_eq!(physical.x, 12);
        assert_eq!(physical.y, 12);
        assert!(physical.width >= 125);
        assert!(physical.height >= 62);
    }

    #[test]
    fn fractional_dpi_150() {
        let rect = Rect::from_min_size(egui::pos2(20.0, 20.0), egui::vec2(200.0, 100.0));
        let physical = logical_rect_to_physical(rect, 1.5, 1920, 1080).expect("physical");
        assert_eq!(physical.x, 30);
        assert_eq!(physical.y, 30);
        assert!(physical.width >= 300);
    }

    #[test]
    fn rejects_empty_rect() {
        assert!(logical_rect_to_physical(Rect::NOTHING, 1.0, 1280, 720).is_none());
    }

    #[test]
    fn clamps_to_target_extent() {
        let rect = Rect::from_min_size(egui::pos2(1200.0, 0.0), egui::vec2(200.0, 100.0));
        let physical = logical_rect_to_physical(rect, 1.0, 1280, 720).expect("physical");
        assert!(physical.max_x() < 1280);
    }
}
