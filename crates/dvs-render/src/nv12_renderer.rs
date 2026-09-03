//! Production NV12 WGSL renderer.

use wgpu::{
    BindGroup, BindGroupDescriptor, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
    BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferDescriptor, BufferUsages,
    Color, ColorTargetState, ColorWrites, CommandEncoder, Device, FragmentState, LoadOp,
    Operations, PipelineLayoutDescriptor, Queue, RenderPassColorAttachment, RenderPassDescriptor,
    RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor,
    ShaderModule, ShaderModuleDescriptor, ShaderSource, ShaderStages, StoreOp, Texture,
    TextureSampleType, TextureView, TextureViewDimension, VertexState,
};

use dvs_gpu::{GpuVideoFrame, GpuVideoPixelFormat, create_nv12_plane_views};
use dvs_media::VideoFrameMetadata;

use crate::aspect::{AspectFitRect, aspect_fit_rect};

fn clamp_destination_to_surface(
    destination: AspectFitRect,
    target_width: u32,
    target_height: u32,
) -> AspectFitRect {
    if target_width == 0 || target_height == 0 {
        return AspectFitRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let x = destination.x.min(target_width);
    let y = destination.y.min(target_height);
    let max_w = target_width.saturating_sub(x);
    let max_h = target_height.saturating_sub(y);
    AspectFitRect {
        x,
        y,
        width: destination.width.min(max_w),
        height: destination.height.min(max_h),
    }
}
use crate::color::coefficients_from_color_info;
use crate::crop::normalized_visible_uv;
use crate::error::RenderError;
use crate::fullscreen::DRAW_VERTEX_COUNT;
use crate::uniforms::Nv12RenderUniforms;

/// Target output configuration for pipeline creation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nv12RendererConfig {
    /// Swapchain or offscreen render target format.
    pub target_format: wgpu::TextureFormat,
}

/// Counts of GPU resources created by [`Nv12Renderer`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Nv12RendererResourceStats {
    pub shader_modules: u32,
    pub render_pipelines: u32,
    pub samplers: u32,
    pub bind_group_layouts: u32,
    pub uniform_buffers: u32,
    pub bind_groups: u32,
}

/// Cached production NV12 → RGB renderer.
///
/// Does not submit the queue, signal bridge fences, or own decoder/interop state.
pub struct Nv12Renderer {
    _shader: ShaderModule,
    pipeline: RenderPipeline,
    sampler: Sampler,
    bind_group_layout: BindGroupLayout,
    uniform_buffer: Buffer,
    bind_group: Option<BindGroup>,
    cached_texture_id: Option<*const Texture>,
    target_format: wgpu::TextureFormat,
    stats: Nv12RendererResourceStats,
}

impl Nv12Renderer {
    /// Creates cached shader, pipeline, sampler, and uniform resources.
    pub fn new(device: &Device, config: Nv12RendererConfig) -> Result<Self, RenderError> {
        let mut stats = Nv12RendererResourceStats::default();

        let shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("dvs-render-nv12-to-rgb"),
            source: ShaderSource::Wgsl(include_str!("../shaders/nv12_to_rgb.wgsl").into()),
        });
        stats.shader_modules += 1;

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("dvs-render-nv12-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        stats.samplers += 1;

        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("dvs-render-nv12-layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        stats.bind_group_layouts += 1;

        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("dvs-render-nv12-uniforms"),
            size: std::mem::size_of::<Nv12RenderUniforms>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        stats.uniform_buffers += 1;

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("dvs-render-nv12-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("dvs-render-nv12-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(ColorTargetState {
                    format: config.target_format,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });
        stats.render_pipelines += 1;

        Ok(Self {
            _shader: shader,
            pipeline,
            sampler,
            bind_group_layout,
            uniform_buffer,
            bind_group: None,
            cached_texture_id: None,
            target_format: config.target_format,
            stats,
        })
    }

    /// Returns resource creation counts (stable after construction except bind groups).
    pub fn resource_stats(&self) -> Nv12RendererResourceStats {
        self.stats
    }

    /// Returns the configured target format.
    pub fn target_format(&self) -> wgpu::TextureFormat {
        self.target_format
    }

    /// Encodes one NV12 render pass into the full target surface.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_frame(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        frame: &GpuVideoFrame,
        metadata: VideoFrameMetadata,
        target: &TextureView,
        target_width: u32,
        target_height: u32,
    ) -> Result<(), RenderError> {
        self.encode_frame_in_region(
            device,
            queue,
            encoder,
            frame,
            metadata,
            target,
            target_width,
            target_height,
            AspectFitRect {
                x: 0,
                y: 0,
                width: target_width,
                height: target_height,
            },
        )
        .map(|_| ())
    }

    /// Encodes one NV12 pass aspect-fitted inside `destination` (surface pixels).
    ///
    /// Returns the absolute surface-space rectangle used for viewport/scissor.
    /// The destination region is clamped to the target surface. A zero-sized
    /// destination skips drawing and returns the empty clamped region.
    #[allow(clippy::too_many_arguments)]
    pub fn encode_frame_in_region(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        frame: &GpuVideoFrame,
        metadata: VideoFrameMetadata,
        target: &TextureView,
        target_width: u32,
        target_height: u32,
        destination: AspectFitRect,
    ) -> Result<AspectFitRect, RenderError> {
        if frame.pixel_format() != GpuVideoPixelFormat::Nv12 {
            return Err(RenderError::UnsupportedPixelFormat);
        }
        if target_width == 0 || target_height == 0 {
            return Err(RenderError::InvalidTargetDimensions);
        }

        let region = clamp_destination_to_surface(destination, target_width, target_height);
        if region.width == 0 || region.height == 0 {
            // Still clear the surface so prior contents do not leak.
            let _ = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("dvs-render-nv12-clear"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            return Ok(region);
        }

        let crop = normalized_visible_uv(&metadata)?;
        let coeffs = coefficients_from_color_info(metadata.color())?;
        let visible = metadata.dimensions().visible();
        let local_fit = aspect_fit_rect(
            visible.width(),
            visible.height(),
            region.width,
            region.height,
        )?;
        let fit = AspectFitRect {
            x: region.x + local_fit.x,
            y: region.y + local_fit.y,
            width: local_fit.width,
            height: local_fit.height,
        };
        let uniforms = Nv12RenderUniforms::new(crop, coeffs);

        self.ensure_bind_group(device, frame)?;

        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let bind_group = self
            .bind_group
            .as_ref()
            .ok_or(RenderError::BindGroupCreationFailed)?;

        {
            let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("dvs-render-nv12-pass"),
                color_attachments: &[Some(RenderPassColorAttachment {
                    view: target,
                    resolve_target: None,
                    depth_slice: None,
                    ops: Operations {
                        load: LoadOp::Clear(Color::BLACK),
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(
                fit.x as f32,
                fit.y as f32,
                fit.width as f32,
                fit.height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(fit.x, fit.y, fit.width, fit.height);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..DRAW_VERTEX_COUNT, 0..1);
        }

        Ok(fit)
    }

    fn ensure_bind_group(
        &mut self,
        device: &Device,
        frame: &GpuVideoFrame,
    ) -> Result<(), RenderError> {
        let texture_ptr = std::ptr::from_ref(frame.texture());
        if self.cached_texture_id == Some(texture_ptr) && self.bind_group.is_some() {
            return Ok(());
        }

        let planes = create_nv12_plane_views(device, frame)?;
        let bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("dvs-render-nv12-bind-group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(planes.y_view()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(planes.uv_view()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&self.sampler),
                },
            ],
        });
        self.stats.bind_groups += 1;
        self.bind_group = Some(bind_group);
        self.cached_texture_id = Some(texture_ptr);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_stats_default_is_zero_before_construction() {
        let stats = Nv12RendererResourceStats::default();
        assert_eq!(stats.shader_modules, 0);
    }
}
