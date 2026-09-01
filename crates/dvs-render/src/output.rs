//! Output surface format selection and encoding contract.

use wgpu::TextureFormat;

use crate::error::RenderError;

/// Documents how renderer output maps to the presentation target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputEncoding {
    pub format: TextureFormat,
    /// When true, the swapchain applies automatic sRGB encoding on output.
    pub target_applies_srgb_encoding: bool,
    /// Renderer writes nonlinear video RGB in [0, 1].
    pub renderer_output_is_nonlinear_video_rgb: bool,
}

impl OutputEncoding {
    /// Human-readable summary for validation reports.
    pub fn summary(self) -> String {
        format!(
            "format={:?}, target_srgb_encode={}, renderer_output=nonlinear_video_rgb",
            self.format, self.target_applies_srgb_encoding,
        )
    }
}

/// Selects a supported SDR presentation format, preferring non-sRGB UNORM targets.
pub fn select_surface_format(formats: &[TextureFormat]) -> Result<OutputEncoding, RenderError> {
    for format in formats {
        if matches!(
            *format,
            TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm
        ) {
            return Ok(OutputEncoding {
                format: *format,
                target_applies_srgb_encoding: false,
                renderer_output_is_nonlinear_video_rgb: true,
            });
        }
    }

    for format in formats {
        if format.is_srgb() {
            return Ok(OutputEncoding {
                format: *format,
                target_applies_srgb_encoding: true,
                renderer_output_is_nonlinear_video_rgb: true,
            });
        }
    }

    formats
        .first()
        .copied()
        .map_or(Err(RenderError::UnsupportedOutputFormat), |format| {
            Ok(OutputEncoding {
                format,
                target_applies_srgb_encoding: format.is_srgb(),
                renderer_output_is_nonlinear_video_rgb: true,
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_bgra8_unorm_over_srgb() {
        let encoding =
            select_surface_format(&[TextureFormat::Bgra8UnormSrgb, TextureFormat::Bgra8Unorm])
                .expect("format");
        assert_eq!(encoding.format, TextureFormat::Bgra8Unorm);
        assert!(!encoding.target_applies_srgb_encoding);
    }

    #[test]
    fn falls_back_to_srgb_when_only_option() {
        let encoding = select_surface_format(&[TextureFormat::Bgra8UnormSrgb]).expect("format");
        assert!(encoding.target_applies_srgb_encoding);
    }
}
