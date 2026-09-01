//! Aspect-preserving viewport fitting (contain / letterbox / pillarbox).

use crate::error::RenderError;

/// Pixel-space rectangle for aspect-fit presentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AspectFitRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// NDC viewport parameters for the vertex shader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectFitNdc {
    pub origin: [f32; 2],
    pub extent: [f32; 2],
}

/// Computes a centered contain-fit rectangle inside the target.
pub fn aspect_fit_rect(
    source_width: u32,
    source_height: u32,
    target_width: u32,
    target_height: u32,
) -> Result<AspectFitRect, RenderError> {
    if source_width == 0 || source_height == 0 {
        return Err(RenderError::InvalidCrop {
            detail: "source dimensions must be non-zero",
        });
    }
    if target_width == 0 || target_height == 0 {
        return Err(RenderError::InvalidTargetDimensions);
    }

    let source_aspect = source_width as f32 / source_height as f32;
    let target_aspect = target_width as f32 / target_height as f32;

    let (fit_w, fit_h) = if source_aspect > target_aspect {
        let fit_w = target_width;
        let fit_h = ((target_width as f32 / source_aspect).round() as u32).max(1);
        (fit_w, fit_h)
    } else {
        let fit_h = target_height;
        let fit_w = ((target_height as f32 * source_aspect).round() as u32).max(1);
        (fit_w, fit_h)
    };

    let x = (target_width - fit_w) / 2;
    let y = (target_height - fit_h) / 2;

    Ok(AspectFitRect {
        x,
        y,
        width: fit_w,
        height: fit_h,
    })
}

/// Converts a pixel-space aspect-fit rectangle to NDC viewport parameters.
pub fn aspect_fit_to_ndc(
    fit: AspectFitRect,
    target_width: u32,
    target_height: u32,
) -> AspectFitNdc {
    let tw = target_width as f32;
    let th = target_height as f32;

    let left = (fit.x as f32 / tw) * 2.0 - 1.0;
    let top = 1.0 - (fit.y as f32 / th) * 2.0;
    let width_ndc = (fit.width as f32 / tw) * 2.0;
    let height_ndc = (fit.height as f32 / th) * 2.0;

    AspectFitNdc {
        origin: [left, top - height_ndc],
        extent: [width_ndc / 2.0, height_ndc / 2.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equal_aspect_fills_target() {
        let fit = aspect_fit_rect(1920, 1080, 1280, 720).expect("fit");
        assert_eq!(fit.x, 0);
        assert_eq!(fit.y, 0);
        assert_eq!(fit.width, 1280);
        assert_eq!(fit.height, 720);
    }

    #[test]
    fn wider_source_produces_letterbox() {
        let fit = aspect_fit_rect(3840, 2160, 1280, 1024).expect("fit");
        assert_eq!(fit.width, 1280);
        assert!(fit.height < 1024);
        assert!(fit.y > 0);
    }

    #[test]
    fn taller_source_produces_pillarbox() {
        let fit = aspect_fit_rect(1080, 1920, 1280, 720).expect("fit");
        assert_eq!(fit.height, 720);
        assert!(fit.width < 1280);
        assert!(fit.x > 0);
    }

    #[test]
    fn fixture_aspect_fills_validation_window_rectangularly() {
        let fit = aspect_fit_rect(3840, 2160, 1280, 720).expect("fit");
        assert_eq!(fit.x, 0);
        assert_eq!(fit.y, 0);
        assert_eq!(fit.width, 1280);
        assert_eq!(fit.height, 720);
    }

    #[test]
    fn zero_target_rejected() {
        let err = aspect_fit_rect(1920, 1080, 0, 720).unwrap_err();
        assert!(matches!(err, RenderError::InvalidTargetDimensions));
    }
}
