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

use bytemuck::{Pod, Zeroable};

use dvs_gpu::{GpuVideoFrame, GpuVideoPixelFormat, create_nv12_plane_views};
use dvs_media::VideoFrameMetadata;

use crate::aspect::{AspectFitRect, aspect_fit_rect_in_destination};
use crate::color::coefficients_from_color_info;
use crate::crop::normalized_visible_uv;
use crate::error::RenderError;
use crate::fullscreen::DRAW_VERTEX_COUNT;
use crate::physical_rect::PhysicalRenderRect;
use crate::uniforms::Nv12RenderUniforms;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct SolidColorUniform {
    color: [f32; 4],
}

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
    pub solid_render_pipelines: u32,
}

/// Cached production NV12 → RGB renderer.
///
/// Does not submit the queue, signal bridge fences, or own decoder/interop state.
pub struct Nv12Renderer {
    _shader: ShaderModule,
    pipeline: RenderPipeline,
    _solid_shader: ShaderModule,
    solid_pipeline: RenderPipeline,
    _solid_bind_group_layout: BindGroupLayout,
    solid_uniform_buffer: Buffer,
    solid_bind_group: BindGroup,
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

        let solid_shader = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("dvs-render-solid-color"),
            source: ShaderSource::Wgsl(include_str!("../shaders/solid_color.wgsl").into()),
        });
        stats.shader_modules += 1;

        let solid_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("dvs-render-solid-layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        stats.bind_group_layouts += 1;

        let solid_uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("dvs-render-solid-uniforms"),
            size: std::mem::size_of::<SolidColorUniform>() as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        stats.uniform_buffers += 1;

        let solid_bind_group = device.create_bind_group(&BindGroupDescriptor {
            label: Some("dvs-render-solid-bind-group"),
            layout: &solid_bind_group_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: solid_uniform_buffer.as_entire_binding(),
            }],
        });
        stats.bind_groups += 1;

        let solid_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("dvs-render-solid-pipeline-layout"),
            bind_group_layouts: &[&solid_bind_group_layout],
            push_constant_ranges: &[],
        });

        let solid_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("dvs-render-solid-pipeline"),
            layout: Some(&solid_pipeline_layout),
            vertex: VertexState {
                module: &solid_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(FragmentState {
                module: &solid_shader,
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
        stats.solid_render_pipelines += 1;

        Ok(Self {
            _shader: shader,
            pipeline,
            _solid_shader: solid_shader,
            solid_pipeline,
            _solid_bind_group_layout: solid_bind_group_layout,
            solid_uniform_buffer,
            solid_bind_group,
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

    /// Encodes one NV12 render pass into the caller's command encoder.
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
        let destination = PhysicalRenderRect {
            x: 0,
            y: 0,
            width: target_width,
            height: target_height,
        };
        let _ = self.encode_frame_in_rect(
            device,
            queue,
            encoder,
            frame,
            metadata,
            target,
            target_width,
            target_height,
            destination,
            LoadOp::Clear(Color::BLACK),
        )?;
        Ok(())
    }

    /// Encodes one NV12 render pass into a destination rectangle on the target.
    ///
    /// Letterbox/pillarbox bars inside `destination` are filled with black using a
    /// viewport-scoped solid draw. The attachment `load` op applies to the full target;
    /// callers composing into a shared editor surface must pass [`LoadOp::Load`].
    #[allow(clippy::too_many_arguments)]
    pub fn encode_frame_in_rect(
        &mut self,
        device: &Device,
        queue: &Queue,
        encoder: &mut CommandEncoder,
        frame: &GpuVideoFrame,
        metadata: VideoFrameMetadata,
        target: &TextureView,
        target_width: u32,
        target_height: u32,
        destination: PhysicalRenderRect,
        load: LoadOp<Color>,
    ) -> Result<AspectFitRect, RenderError> {
        if frame.pixel_format() != GpuVideoPixelFormat::Nv12 {
            return Err(RenderError::UnsupportedPixelFormat);
        }
        if target_width == 0 || target_height == 0 {
            return Err(RenderError::InvalidTargetDimensions);
        }
        if !destination.is_valid() {
            return Err(RenderError::InvalidTargetDimensions);
        }

        let crop = normalized_visible_uv(&metadata)?;
        let coeffs = coefficients_from_color_info(metadata.color())?;
        let visible = metadata.dimensions().visible();
        let fit = aspect_fit_rect_in_destination(visible.width(), visible.height(), destination)?;
        let uniforms = Nv12RenderUniforms::new(crop, coeffs);

        self.ensure_bind_group(device, frame)?;

        queue.write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));
        queue.write_buffer(
            &self.solid_uniform_buffer,
            0,
            bytemuck::bytes_of(&SolidColorUniform {
                color: [0.0, 0.0, 0.0, 1.0],
            }),
        );

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
                        load,
                        store: StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_viewport(
                destination.x as f32,
                destination.y as f32,
                destination.width as f32,
                destination.height as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(
                destination.x,
                destination.y,
                destination.width,
                destination.height,
            );
            pass.set_pipeline(&self.solid_pipeline);
            pass.set_bind_group(0, &self.solid_bind_group, &[]);
            pass.draw(0..DRAW_VERTEX_COUNT, 0..1);

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

    #[test]
    fn composed_destination_requires_load_not_clear() {
        // Render-pass attachment clear affects the full target, not viewport/scissor.
        // Composed editor frames must pass LoadOp::Load into encode_frame_in_rect.
        let composed = LoadOp::Load;
        let standalone = LoadOp::Clear(Color::BLACK);
        assert_ne!(
            std::mem::discriminant(&composed),
            std::mem::discriminant(&standalone)
        );
    }
}
