//! Surface configuration and frame acquisition helpers.

use wgpu::{SurfaceConfiguration, TextureView, TextureViewDescriptor};

use dvs_gpu::GpuContext;

use crate::error::RenderError;
use crate::output::{OutputEncoding, select_surface_format};

/// Configured presentation surface state for validation targets.
pub struct RenderSurface {
    config: SurfaceConfiguration,
    encoding: OutputEncoding,
}

impl RenderSurface {
    /// Selects an SDR output format and configures the bootstrap surface.
    pub fn configure(context: &GpuContext, width: u32, height: u32) -> Result<Self, RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidTargetDimensions);
        }

        let caps = context.surface_capabilities();
        let encoding = select_surface_format(&caps.formats)?;
        let config = context
            .configure_surface(width, height, encoding.format)
            .map_err(|_| RenderError::InvalidTargetDimensions)?;

        Ok(Self { config, encoding })
    }

    /// Reconfigures the surface after a window resize.
    pub fn resize(
        &mut self,
        context: &GpuContext,
        width: u32,
        height: u32,
    ) -> Result<(), RenderError> {
        if width == 0 || height == 0 {
            return Err(RenderError::InvalidTargetDimensions);
        }
        self.config.width = width;
        self.config.height = height;
        context
            .configure_surface(width, height, self.config.format)
            .map_err(|_| RenderError::InvalidTargetDimensions)?;
        Ok(())
    }

    /// Returns the active surface configuration.
    pub fn configuration(&self) -> &SurfaceConfiguration {
        &self.config
    }

    /// Returns the output encoding contract for this surface.
    pub fn output_encoding(&self) -> OutputEncoding {
        self.encoding
    }

    /// Acquires the next swapchain texture and a default color attachment view.
    pub fn acquire_frame(
        &self,
        context: &GpuContext,
    ) -> Result<(wgpu::SurfaceTexture, TextureView), RenderError> {
        let frame = context.surface().get_current_texture()?;
        let view = frame.texture.create_view(&TextureViewDescriptor::default());
        Ok((frame, view))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_encoding_summary_is_non_empty() {
        let encoding = OutputEncoding {
            format: wgpu::TextureFormat::Bgra8Unorm,
            target_applies_srgb_encoding: false,
            renderer_output_is_nonlinear_video_rgb: true,
        };
        assert!(encoding.summary().contains("Bgra8Unorm"));
    }
}
