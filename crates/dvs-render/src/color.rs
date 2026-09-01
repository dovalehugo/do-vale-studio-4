//! Y′CbCr → nonlinear video RGB coefficient generation.

use dvs_media::{ColorMatrix, ColorRange, TransferCharacteristic, VideoColorInfo};

use crate::error::RenderError;

/// Kr/Kb primaries used for supported color matrices.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatrixPrimaries {
    pub kr: f32,
    pub kb: f32,
}

impl MatrixPrimaries {
    /// ITU-R BT.601 primaries.
    pub const BT_601: Self = Self {
        kr: 0.299,
        kb: 0.114,
    };

    /// ITU-R BT.709 primaries.
    pub const BT_709: Self = Self {
        kr: 0.2126,
        kb: 0.0722,
    };

    /// ITU-R BT.2020 non-constant luminance primaries.
    pub const BT_2020_NCL: Self = Self {
        kr: 0.2627,
        kb: 0.0593,
    };
}

/// Row of the 3×3 YUV→RGB matrix stored as `(y, u, v)` coefficients.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YuvToRgbRow {
    pub y: f32,
    pub u: f32,
    pub v: f32,
}

/// Full YUV→RGB conversion parameters for shader uniforms.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct YuvToRgbCoefficients {
    pub y_scale: f32,
    pub y_offset: f32,
    pub uv_scale: f32,
    pub uv_offset: f32,
    pub r: YuvToRgbRow,
    pub g: YuvToRgbRow,
    pub b: YuvToRgbRow,
}

/// Returns matrix primaries for a supported [`ColorMatrix`].
pub fn matrix_primaries(matrix: ColorMatrix) -> Result<MatrixPrimaries, RenderError> {
    match matrix {
        ColorMatrix::Bt601 => Ok(MatrixPrimaries::BT_601),
        ColorMatrix::Bt709 => Ok(MatrixPrimaries::BT_709),
        ColorMatrix::Bt2020NonConstantLuminance => Ok(MatrixPrimaries::BT_2020_NCL),
        ColorMatrix::Unspecified => Err(RenderError::UnsupportedColorMatrix(matrix)),
        _ => Err(RenderError::UnsupportedColorMatrix(matrix)),
    }
}

/// Builds YUV→RGB coefficients from Kr/Kb and range metadata.
pub fn coefficients_from_primaries(
    primaries: MatrixPrimaries,
    range: ColorRange,
) -> Result<YuvToRgbCoefficients, RenderError> {
    let (y_scale, y_offset, uv_scale, uv_offset) = match range {
        ColorRange::Limited => {
            let y_scale = 255.0 / (235.0 - 16.0);
            let uv_scale = 255.0 / (240.0 - 16.0);
            (
                y_scale,
                -(16.0 / 255.0) * y_scale,
                uv_scale,
                -(128.0 / 255.0) * uv_scale,
            )
        }
        ColorRange::Full => (1.0, 0.0, 1.0, -0.5),
        ColorRange::Unspecified => return Err(RenderError::UnsupportedColorRange(range)),
        _ => return Err(RenderError::UnsupportedColorRange(range)),
    };

    let kr = primaries.kr;
    let kb = primaries.kb;
    let kg = 1.0 - kr - kb;

    let r = YuvToRgbRow {
        y: 1.0,
        u: 0.0,
        v: 2.0 * (1.0 - kr),
    };
    let g = YuvToRgbRow {
        y: 1.0,
        u: -2.0 * (1.0 - kb) * kb / kg,
        v: -2.0 * (1.0 - kr) * kr / kg,
    };
    let b = YuvToRgbRow {
        y: 1.0,
        u: 2.0 * (1.0 - kb),
        v: 0.0,
    };

    Ok(YuvToRgbCoefficients {
        y_scale,
        y_offset,
        uv_scale,
        uv_offset,
        r,
        g,
        b,
    })
}

/// Validates SDR color metadata and returns conversion coefficients.
pub fn coefficients_from_color_info(
    color: VideoColorInfo,
) -> Result<YuvToRgbCoefficients, RenderError> {
    match color.transfer() {
        TransferCharacteristic::Bt709 => {}
        TransferCharacteristic::Srgb => {}
        TransferCharacteristic::Pq | TransferCharacteristic::Hlg => {
            return Err(RenderError::HdrTransferRejected);
        }
        TransferCharacteristic::Unspecified => {
            return Err(RenderError::UnsupportedTransfer(color.transfer()));
        }
        _ => return Err(RenderError::UnsupportedTransfer(color.transfer())),
    }

    let primaries = matrix_primaries(color.matrix())?;
    coefficients_from_primaries(primaries, color.range())
}

/// Converts limited-range YUV reference values to RGB using BT.709 coefficients.
pub fn limited_yuv_to_rgb(y: f32, u: f32, v: f32, coeffs: YuvToRgbCoefficients) -> [f32; 3] {
    let y = y * coeffs.y_scale + coeffs.y_offset;
    let u = u * coeffs.uv_scale + coeffs.uv_offset;
    let v = v * coeffs.uv_scale + coeffs.uv_offset;

    let r = y * coeffs.r.y + u * coeffs.r.u + v * coeffs.r.v;
    let g = y * coeffs.g.y + u * coeffs.g.u + v * coeffs.g.v;
    let b = y * coeffs.b.y + u * coeffs.b.u + v * coeffs.b.v;

    [r.clamp(0.0, 1.0), g.clamp(0.0, 1.0), b.clamp(0.0, 1.0)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use dvs_media::{ColorPrimaries, VideoColorInfo};

    fn bt709_limited_coeffs() -> YuvToRgbCoefficients {
        coefficients_from_color_info(VideoColorInfo::bt709_limited()).expect("coeffs")
    }

    #[test]
    fn bt709_limited_black_maps_near_zero() {
        let coeffs = bt709_limited_coeffs();
        let rgb = limited_yuv_to_rgb(16.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, coeffs);
        assert!(rgb[0].abs() < 0.02);
        assert!(rgb[1].abs() < 0.02);
        assert!(rgb[2].abs() < 0.02);
    }

    #[test]
    fn bt709_limited_white_maps_near_one() {
        let coeffs = bt709_limited_coeffs();
        let rgb = limited_yuv_to_rgb(235.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, coeffs);
        assert!((rgb[0] - 1.0).abs() < 0.02);
        assert!((rgb[1] - 1.0).abs() < 0.02);
        assert!((rgb[2] - 1.0).abs() < 0.02);
    }

    #[test]
    fn bt709_limited_neutral_chroma_is_gray() {
        let coeffs = bt709_limited_coeffs();
        let rgb = limited_yuv_to_rgb(128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, coeffs);
        let spread = (rgb[0] - rgb[1]).abs().max((rgb[1] - rgb[2]).abs());
        assert!(spread < 0.02);
    }

    #[test]
    fn bt709_limited_matches_experiment_coefficients() {
        let coeffs = bt709_limited_coeffs();
        assert!((coeffs.y_scale - 1.164383).abs() < 0.001);
        let kr = MatrixPrimaries::BT_709.kr;
        assert!((coeffs.r.v - 2.0 * (1.0 - kr)).abs() < 0.001);
        let kb = MatrixPrimaries::BT_709.kb;
        let kg = 1.0 - kr - kb;
        assert!((coeffs.g.u - -2.0 * (1.0 - kb) * kb / kg).abs() < 0.001);
        assert!((coeffs.g.v - -2.0 * (1.0 - kr) * kr / kg).abs() < 0.001);
        assert!((coeffs.b.u - 2.0 * (1.0 - kb)).abs() < 0.001);
    }

    #[test]
    fn bt601_limited_coefficients_differ_from_bt709() {
        let bt601 = coefficients_from_primaries(MatrixPrimaries::BT_601, ColorRange::Limited)
            .expect("bt601");
        let bt709 = coefficients_from_primaries(MatrixPrimaries::BT_709, ColorRange::Limited)
            .expect("bt709");
        assert_ne!(bt601.g.v, bt709.g.v);
    }

    #[test]
    fn unspecified_matrix_rejected() {
        let color = VideoColorInfo::new(
            ColorRange::Limited,
            ColorMatrix::Unspecified,
            ColorPrimaries::Bt709,
            TransferCharacteristic::Bt709,
        );
        let err = coefficients_from_color_info(color).unwrap_err();
        assert!(matches!(
            err,
            RenderError::UnsupportedColorMatrix(ColorMatrix::Unspecified)
        ));
    }

    #[test]
    fn unspecified_range_rejected() {
        let color = VideoColorInfo::new(
            ColorRange::Unspecified,
            ColorMatrix::Bt709,
            ColorPrimaries::Bt709,
            TransferCharacteristic::Bt709,
        );
        let err = coefficients_from_color_info(color).unwrap_err();
        assert!(matches!(
            err,
            RenderError::UnsupportedColorRange(ColorRange::Unspecified)
        ));
    }

    #[test]
    fn pq_transfer_rejected() {
        let color = VideoColorInfo::new(
            ColorRange::Limited,
            ColorMatrix::Bt709,
            ColorPrimaries::Bt709,
            TransferCharacteristic::Pq,
        );
        let err = coefficients_from_color_info(color).unwrap_err();
        assert!(matches!(err, RenderError::HdrTransferRejected));
    }
}
