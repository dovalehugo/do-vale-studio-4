//! Physical pixel destination rectangles for partial-target rendering.

use crate::error::RenderError;

/// Physical pixel rectangle for viewport/scissor targeting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRenderRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl PhysicalRenderRect {
    /// Returns whether the rectangle has a drawable non-zero area.
    pub const fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }

    /// Returns the inclusive maximum x edge, saturating on overflow.
    pub const fn max_x(self) -> u32 {
        self.x.saturating_add(self.width).saturating_sub(1)
    }

    /// Returns the inclusive maximum y edge, saturating on overflow.
    pub const fn max_y(self) -> u32 {
        self.y.saturating_add(self.height).saturating_sub(1)
    }
}

/// Clamps a physical rectangle to a target texture extent.
pub fn clamp_physical_rect(
    rect: PhysicalRenderRect,
    target_width: u32,
    target_height: u32,
) -> Result<PhysicalRenderRect, RenderError> {
    if target_width == 0 || target_height == 0 {
        return Err(RenderError::InvalidTargetDimensions);
    }
    if !rect.is_valid() {
        return Err(RenderError::InvalidTargetDimensions);
    }

    let x = rect.x.min(target_width.saturating_sub(1));
    let y = rect.y.min(target_height.saturating_sub(1));
    let max_w = target_width.saturating_sub(x);
    let max_h = target_height.saturating_sub(y);
    let width = rect.width.min(max_w);
    let height = rect.height.min(max_h);

    if width == 0 || height == 0 {
        return Err(RenderError::InvalidTargetDimensions);
    }

    Ok(PhysicalRenderRect {
        x,
        y,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_keeps_rect_inside_target() {
        let rect = clamp_physical_rect(
            PhysicalRenderRect {
                x: 10,
                y: 20,
                width: 100,
                height: 80,
            },
            1280,
            720,
        )
        .expect("clamp");
        assert_eq!(rect.x, 10);
        assert_eq!(rect.width, 100);
    }

    #[test]
    fn clamp_rejects_empty_rect() {
        let err = clamp_physical_rect(
            PhysicalRenderRect {
                x: 0,
                y: 0,
                width: 0,
                height: 10,
            },
            100,
            100,
        )
        .unwrap_err();
        assert!(matches!(err, RenderError::InvalidTargetDimensions));
    }

    #[test]
    fn clamp_rejects_overflowing_origin() {
        let rect = clamp_physical_rect(
            PhysicalRenderRect {
                x: 2000,
                y: 0,
                width: 10,
                height: 10,
            },
            1280,
            720,
        )
        .expect("clamp");
        assert!(rect.x < 1280);
        assert!(rect.max_x() < 1280);
    }
}
