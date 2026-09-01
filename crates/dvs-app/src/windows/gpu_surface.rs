//! Windows-only GPU surface bootstrap helpers.

use std::sync::Arc;

use dvs_gpu::{GpuBootstrap, GpuContext, SurfaceWindowTarget};
use dvs_render::RenderSurface;
use winit::window::Window;

use crate::error::AppError;

/// Initializes the production GPU context and presentation surface for a window.
pub async fn initialize_gpu(window: Arc<Window>) -> Result<(GpuContext, RenderSurface), AppError> {
    let gpu = GpuBootstrap::initialize(window.clone() as Arc<dyn SurfaceWindowTarget>)
        .await
        .map_err(AppError::Gpu)?;
    let size = window.inner_size();
    let surface = RenderSurface::configure(&gpu, size.width.max(1), size.height.max(1))
        .map_err(AppError::Render)?;
    Ok((gpu, surface))
}
