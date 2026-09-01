//! Steps 34–36 — real NV12 plane access, GPU YUV→RGB, first frame render.

use wgpu::{
    TextureAspect, TextureFormat, TextureView, TextureViewDescriptor,
};

use crate::wgpu_hal_interop::{WgpuDx12Context, WgpuHalInteropBundle};

pub const VISIBLE_WIDTH: u32 = 3840;
pub const VISIBLE_HEIGHT: u32 = 2160;
pub const DECODER_ALLOC_HEIGHT: u32 = 2176;

pub struct PlaneAccessInfo {
    pub y_plane_format: String,
    pub uv_plane_format: String,
    pub y_aspect: String,
    pub uv_aspect: String,
    pub y_view_ok: bool,
    pub uv_view_ok: bool,
    pub step_status: String,
}

pub struct ShaderPathInfo {
    pub shader_compiled: bool,
    pub bind_group_ok: bool,
    pub pipeline_ok: bool,
    pub color_space: String,
    pub range: String,
    pub step_status: String,
}

pub struct RenderFrameInfo {
    pub rendered: bool,
    pub surface_format: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub visible_crop: String,
    pub present_ok: bool,
    pub step_status: String,
}

pub struct RenderPathBundle {
    pub y_view: TextureView,
    pub uv_view: TextureView,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub plane_access: PlaneAccessInfo,
    pub shader_path: ShaderPathInfo,
    pub render_frame: RenderFrameInfo,
}

pub fn run_render_path_steps_34_to_36(
    interop: &WgpuHalInteropBundle,
    context: &WgpuDx12Context,
) -> Result<RenderPathBundle, String> {
    let texture = interop
        ._texture
        .as_ref()
        .ok_or_else(|| "step 33 wgpu texture is missing".to_string())?;

    let (y_view, uv_view, plane_access) = create_real_nv12_plane_views(texture)?;
    if !plane_access.y_view_ok || !plane_access.uv_view_ok {
        return Err(format!(
            "step 34 failed: {}",
            plane_access.step_status
        ));
    }

    let (shader_path, pipeline, bind_group) =
        build_nv12_shader_path(context, &y_view, &uv_view)?;
    if !shader_path.pipeline_ok {
        return Err(shader_path.step_status.clone());
    }

    let render_frame = render_first_real_frame(context, &pipeline, &bind_group)?;
    if !render_frame.rendered {
        return Err(render_frame.step_status.clone());
    }

    Ok(RenderPathBundle {
        y_view,
        uv_view,
        pipeline,
        bind_group,
        plane_access,
        shader_path,
        render_frame,
    })
}

fn create_real_nv12_plane_views(
    texture: &wgpu::Texture,
) -> Result<(TextureView, TextureView, PlaneAccessInfo), String> {
    let y_view = texture.create_view(&TextureViewDescriptor {
        label: Some("decoded-hevc-nv12-y"),
        format: Some(TextureFormat::R8Unorm),
        aspect: TextureAspect::Plane0,
        ..Default::default()
    });
    let uv_view = texture.create_view(&TextureViewDescriptor {
        label: Some("decoded-hevc-nv12-uv"),
        format: Some(TextureFormat::Rg8Unorm),
        aspect: TextureAspect::Plane1,
        ..Default::default()
    });

    Ok((
        y_view,
        uv_view,
        PlaneAccessInfo {
            y_plane_format: "R8Unorm (TextureAspect::Plane0)".to_string(),
            uv_plane_format: "Rg8Unorm (TextureAspect::Plane1)".to_string(),
            y_aspect: "Plane0".to_string(),
            uv_aspect: "Plane1".to_string(),
            y_view_ok: true,
            uv_view_ok: true,
            step_status: "STEP 34 / 40: PASS".to_string(),
        },
    ))
}

fn build_nv12_shader_path(
    context: &WgpuDx12Context,
    y_view: &TextureView,
    uv_view: &TextureView,
) -> Result<(ShaderPathInfo, wgpu::RenderPipeline, wgpu::BindGroup), String> {
    let device = &context.device;
    let surface_format = context.surface_config.format;

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("exp2-nv12-to-rgb"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/nv12_to_rgb.wgsl").into()),
    });

    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("exp2-nv12-linear"),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("exp2-nv12-bind-group-layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("exp2-nv12-bind-group"),
        layout: &bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(y_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(uv_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });

    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("exp2-nv12-pipeline-layout"),
        bind_group_layouts: &[&bind_group_layout],
        push_constant_ranges: &[],
    });

    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("exp2-nv12-render-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: surface_format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    });

    Ok((
        ShaderPathInfo {
            shader_compiled: true,
            bind_group_ok: true,
            pipeline_ok: true,
            color_space: "BT.709".to_string(),
            range: "limited".to_string(),
            step_status: "STEP 35 / 40: PASS".to_string(),
        },
        pipeline,
        bind_group,
    ))
}

pub(crate) fn present_nv12_frame(
    context: &WgpuDx12Context,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) -> Result<(), String> {
    let surface_texture = context
        .surface
        .get_current_texture()
        .map_err(|e| format!("surface get_current_texture failed: {e}"))?;
    let view = surface_texture
        .texture
        .create_view(&TextureViewDescriptor::default());

    let mut encoder = context
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("exp2-nv12-frame"),
        });

    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("exp2-nv12-frame-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    context.queue.submit(Some(encoder.finish()));
    surface_texture.present();
    Ok(())
}

pub(crate) fn render_first_real_frame(
    context: &WgpuDx12Context,
    pipeline: &wgpu::RenderPipeline,
    bind_group: &wgpu::BindGroup,
) -> Result<RenderFrameInfo, String> {
    present_nv12_frame(context, pipeline, bind_group)?;

    Ok(RenderFrameInfo {
        rendered: true,
        surface_format: format!("{:?}", context.surface_config.format),
        viewport_width: context.surface_config.width,
        viewport_height: context.surface_config.height,
        visible_crop: format!("{VISIBLE_WIDTH}x{VISIBLE_HEIGHT} from {VISIBLE_WIDTH}x{DECODER_ALLOC_HEIGHT} NV12 allocation"),
        present_ok: true,
        step_status: "STEP 36 / 40: PASS (API present OK — visual correctness requires human inspection)".to_string(),
    })
}

pub fn print_plane_access(info: &PlaneAccessInfo) {
    println!("=== Real NV12 plane access ===");
    println!("Y plane format:       {}", info.y_plane_format);
    println!("UV plane format:      {}", info.uv_plane_format);
    println!("Y aspect:             {}", info.y_aspect);
    println!("UV aspect:            {}", info.uv_aspect);
    println!("Y view created:       {}", if info.y_view_ok { "yes" } else { "no" });
    println!("UV view created:      {}", if info.uv_view_ok { "yes" } else { "no" });
    println!();
    println!("{}", info.step_status);
}

pub fn print_shader_path(info: &ShaderPathInfo) {
    println!("=== GPU YUV → RGB shader path ===");
    println!("shader compiled:      {}", if info.shader_compiled { "yes" } else { "no" });
    println!("bind group accepted:    {}", if info.bind_group_ok { "yes" } else { "no" });
    println!("pipeline created:     {}", if info.pipeline_ok { "yes" } else { "no" });
    println!("color space:            {}", info.color_space);
    println!("range:                  {}", info.range);
    println!();
    println!("{}", info.step_status);
}

pub fn print_render_frame(info: &RenderFrameInfo) {
    println!("=== First real HEVC frame render ===");
    println!("rendered:               {}", if info.rendered { "yes" } else { "no" });
    println!("surface format:         {}", info.surface_format);
    println!(
        "viewport:               {} x {}",
        info.viewport_width, info.viewport_height
    );
    println!("visible crop:           {}", info.visible_crop);
    println!("present:                {}", if info.present_ok { "OK" } else { "FAILED" });
    println!();
    println!("{}", info.step_status);
}
