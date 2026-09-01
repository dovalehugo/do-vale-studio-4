//! GPU uniform buffer layout for NV12 rendering.

use bytemuck::{Pod, Zeroable};

use crate::color::YuvToRgbCoefficients;
use crate::crop::Nv12CropUv;

/// WGSL `RenderUniforms` mirror uploaded each frame.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Nv12RenderUniforms {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub y_scale: f32,
    pub y_offset: f32,
    pub uv_scale: f32,
    pub uv_offset: f32,
    pub mat_r: [f32; 4],
    pub mat_g: [f32; 4],
    pub mat_b: [f32; 4],
}

impl Nv12RenderUniforms {
    /// Builds uniforms from visible crop and color coefficients.
    pub fn new(crop: Nv12CropUv, coeffs: YuvToRgbCoefficients) -> Self {
        Self {
            uv_min: crop.uv_min,
            uv_max: crop.uv_max,
            y_scale: coeffs.y_scale,
            y_offset: coeffs.y_offset,
            uv_scale: coeffs.uv_scale,
            uv_offset: coeffs.uv_offset,
            mat_r: [coeffs.r.y, coeffs.r.u, coeffs.r.v, 0.0],
            mat_g: [coeffs.g.y, coeffs.g.u, coeffs.g.v, 0.0],
            mat_b: [coeffs.b.y, coeffs.b.u, coeffs.b.v, 0.0],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{MatrixPrimaries, coefficients_from_primaries};
    use dvs_media::ColorRange;

    #[test]
    fn uniform_size_is_shader_compatible() {
        assert_eq!(std::mem::size_of::<Nv12RenderUniforms>() % 16, 0);
        let coeffs =
            coefficients_from_primaries(MatrixPrimaries::BT_709, ColorRange::Limited).expect("c");
        let uniforms = Nv12RenderUniforms::new(
            Nv12CropUv {
                uv_min: [0.0, 0.0],
                uv_max: [1.0, 2160.0 / 2176.0],
            },
            coeffs,
        );
        assert!(uniforms.y_scale > 1.0);
    }
}
