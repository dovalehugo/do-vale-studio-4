//! Production NV12 WGSL renderer for Do Vale Studio 4 (Integration 5).
//!
//! Samples imported [`dvs_gpu::GpuVideoFrame`] NV12 plane views, converts YUV to
//! nonlinear video RGB using metadata-driven coefficients, and encodes a render pass
//! into a caller-provided command encoder. Bridge fence orchestration remains outside
//! this crate.

#![forbid(unsafe_code)]

mod aspect;
mod color;
mod crop;
mod error;
mod fullscreen;
mod nv12_renderer;
mod output;
mod surface;
mod uniforms;

pub use aspect::{AspectFitNdc, AspectFitRect, aspect_fit_rect, aspect_fit_to_ndc};
pub use color::{
    MatrixPrimaries, YuvToRgbCoefficients, YuvToRgbRow, coefficients_from_color_info,
    coefficients_from_primaries, limited_yuv_to_rgb, matrix_primaries,
};
pub use crop::{Nv12CropUv, normalized_visible_uv};
pub use error::RenderError;
pub use fullscreen::{
    CLIP_VERTICES, DRAW_VERTEX_COUNT, base_uv_for_vertex, base_uv_from_clip, remapped_uv,
};
pub use nv12_renderer::{Nv12Renderer, Nv12RendererConfig, Nv12RendererResourceStats};
pub use output::{OutputEncoding, select_surface_format};
pub use surface::RenderSurface;
pub use uniforms::Nv12RenderUniforms;

#[cfg(test)]
mod send_sync {
    use std::sync::{Arc, Mutex};

    use super::*;

    const fn assert_send_sync<T: Send + Sync>() {}

    const _: () = {
        assert_send_sync::<RenderError>();
        assert_send_sync::<Nv12RendererConfig>();
        assert_send_sync::<Nv12RendererResourceStats>();
        assert_send_sync::<OutputEncoding>();
    };

    #[test]
    fn public_types_are_send_and_sync() {
        fn assert_values<T: Send + Sync>(value: T) {
            let _ = Arc::new(Mutex::new(value));
        }

        assert_values(RenderError::InvalidTargetDimensions);
        assert_values(Nv12RendererConfig {
            target_format: wgpu::TextureFormat::Bgra8Unorm,
        });
    }
}
