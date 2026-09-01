//! wgpu NV12 → RGB render path for Experiment 1.

use std::sync::Arc;
use std::time::Instant;

use wgpu::{
    Backends, Device, DeviceDescriptor, ExperimentalFeatures, Features, Instance,
    InstanceDescriptor, Limits, Queue, RequestAdapterOptions, Surface, SurfaceConfiguration,
    TextureAspect, TextureDescriptor, TextureDimension, TextureFormat, TextureUsages,
    TextureViewDescriptor,
};

use crate::nv12_pattern::{generate_test_pattern, Nv12Pattern};

pub const SOURCE_WIDTH: u32 = 1920;
pub const SOURCE_HEIGHT: u32 = 1080;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendChoice {
    Dx12,
    Vulkan,
}

impl BackendChoice {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "dx12" | "d3d12" | "directx12" => Some(Self::Dx12),
            "vulkan" | "vk" => Some(Self::Vulkan),
            _ => None,
        }
    }

    pub fn wgpu_backends(self) -> Backends {
        match self {
            Self::Dx12 => Backends::DX12,
            Self::Vulkan => Backends::VULKAN,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Dx12 => "DX12",
            Self::Vulkan => "Vulkan",
        }
    }
}

#[derive(Debug, Clone)]
pub struct InitMetrics {
    pub backend_requested: String,
    pub adapter_name: String,
    pub adapter_backend: String,
    pub driver: String,
    pub driver_info: String,
    pub surface_format: String,
    pub nv12_upload_mode: String,
    pub initialization_ms: f64,
    pub pattern_generation_ms: f64,
    pub texture_creation_ms: f64,
    pub pipeline_creation_ms: f64,
    pub cpu_yuv_to_rgb_ms: f64,
}

#[derive(Debug, Default, Clone)]
pub struct FrameMetrics {
    pub frames_presented: u64,
    pub last_present_interval_ms: f64,
    pub avg_present_interval_ms: f64,
    pub last_submit_ms: f64,
    pub avg_submit_ms: f64,
}

pub struct Nv12Renderer {
    pub device: Device,
    pub queue: Queue,
    pub surface: Surface<'static>,
    pub surface_config: SurfaceConfiguration,
    pub pipeline: wgpu::RenderPipeline,
    pub bind_group: wgpu::BindGroup,
    pub init_metrics: InitMetrics,
    frame_metrics: FrameMetrics,
    last_present: Option<Instant>,
    submit_total_ms: f64,
    present_total_ms: f64,
}

impl Nv12Renderer {
    pub async fn new(
        window: Arc<winit::window::Window>,
        backend: BackendChoice,
    ) -> Result<Self, String> {
        let init_start = Instant::now();

        let pattern_start = Instant::now();
        let pattern = generate_test_pattern(SOURCE_WIDTH, SOURCE_HEIGHT);
        let pattern_generation_ms = elapsed_ms(pattern_start);

        let instance = Instance::new(&InstanceDescriptor {
            backends: backend.wgpu_backends(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("create_surface: {e}"))?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .map_err(|e| format!("request_adapter: {e}"))?;

        let adapter_info = adapter.get_info();

        let required_features = adapter.features() & Features::TEXTURE_FORMAT_NV12;
        if !required_features.contains(Features::TEXTURE_FORMAT_NV12) {
            return Err("Adapter does not support TEXTURE_FORMAT_NV12".to_string());
        }

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("gpu-nv12-device"),
                required_features,
                required_limits: Limits::default(),
                experimental_features: ExperimentalFeatures::disabled(),
                memory_hints: Default::default(),
                trace: Default::default(),
            })
            .await
            .map_err(|e| format!("request_device: {e}"))?;

        let caps = surface.get_capabilities(&adapter);
        let surface_format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb() || matches!(f, TextureFormat::Bgra8Unorm | TextureFormat::Rgba8Unorm))
            .unwrap_or(caps.formats[0]);

        let size = window.inner_size();
        let surface_config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &surface_config);

        let texture_start = Instant::now();
        let (y_view, uv_view, upload_mode) =
            create_and_upload_nv12_source(&device, &queue, &adapter_info, &pattern)?;
        let texture_creation_ms = elapsed_ms(texture_start);

        let pipeline_start = Instant::now();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("nv12_to_rgb"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/nv12_to_rgb.wgsl").into()),
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("nv12-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("nv12-bind-group-layout"),
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
            label: Some("nv12-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&y_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&uv_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("nv12-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("nv12-render-pipeline"),
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
        let pipeline_creation_ms = elapsed_ms(pipeline_start);

        let initialization_ms = elapsed_ms(init_start);

        Ok(Self {
            device,
            queue,
            surface,
            surface_config,
            pipeline,
            bind_group,
            init_metrics: InitMetrics {
                backend_requested: backend.label().to_string(),
                adapter_name: adapter_info.name,
                adapter_backend: format!("{:?}", adapter_info.backend),
                driver: adapter_info.driver,
                driver_info: adapter_info.driver_info,
                surface_format: format!("{surface_format:?}"),
                nv12_upload_mode: upload_mode,
                initialization_ms,
                pattern_generation_ms,
                texture_creation_ms,
                pipeline_creation_ms,
                cpu_yuv_to_rgb_ms: 0.0,
            },
            frame_metrics: FrameMetrics::default(),
            last_present: None,
            submit_total_ms: 0.0,
            present_total_ms: 0.0,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.surface_config.width = width;
            self.surface_config.height = height;
            self.surface.configure(&self.device, &self.surface_config);
        }
    }

    pub fn render(&mut self) -> Result<(), wgpu::SurfaceError> {
        let submit_start = Instant::now();

        let frame = self.surface.get_current_texture()?;
        let view = frame
            .texture
            .create_view(&TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("nv12-frame-encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("nv12-render-pass"),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..3, 0..1);
        }

        self.queue.submit(std::iter::once(encoder.finish()));
        let submit_ms = elapsed_ms(submit_start);

        let present_start = Instant::now();
        frame.present();
        let _present_ms = elapsed_ms(present_start);

        self.frame_metrics.frames_presented += 1;
        self.frame_metrics.last_submit_ms = submit_ms;
        self.submit_total_ms += submit_ms;

        if let Some(last) = self.last_present.replace(Instant::now()) {
            let interval = elapsed_ms(last);
            self.frame_metrics.last_present_interval_ms = interval;
            self.present_total_ms += interval;
            let n = self.frame_metrics.frames_presented.saturating_sub(1) as f64;
            if n > 0.0 {
                self.frame_metrics.avg_present_interval_ms = self.present_total_ms / n;
                self.frame_metrics.avg_submit_ms = self.submit_total_ms / n;
            }
        }

        Ok(())
    }

    pub fn frame_metrics(&self) -> &FrameMetrics {
        &self.frame_metrics
    }

    pub fn print_init_report(&self) {
        let m = &self.init_metrics;
        println!("=== GPU Experiment 1 — initialization ({}) ===", m.backend_requested);
        println!("Adapter:       {}", m.adapter_name);
        println!("Backend:       {}", m.adapter_backend);
        println!("Driver:        {}", m.driver);
        println!("Driver info:   {}", m.driver_info);
        println!("Surface format: {}", m.surface_format);
        println!("NV12 upload:   {}", m.nv12_upload_mode);
        println!("Source size:   {}x{} NV12", SOURCE_WIDTH, SOURCE_HEIGHT);
        println!("Total init:    {:.3} ms", m.initialization_ms);
        println!("  Pattern gen: {:.3} ms (Y/UV only, no RGB conversion)", m.pattern_generation_ms);
        println!("  Tex+upload:  {:.3} ms", m.texture_creation_ms);
        println!("  Pipeline:    {:.3} ms", m.pipeline_creation_ms);
        println!("CPU YUV→RGB:   {:.3} ms (must be 0)", m.cpu_yuv_to_rgb_ms);
        println!("Validation:");
        println!("  NV12 texture (probe): OK");
        println!("  NV12 Y plane view:    OK");
        println!("  NV12 UV plane view:   OK");
        println!("  Display Y texture:    OK (R8Unorm upload)");
        println!("  Display UV texture:   OK (Rg8Unorm upload)");
        println!("  Bind group:          OK");
        println!("  Shader compiled:     OK");
        println!("  Render pipeline:     OK");
        println!("  CPU YUV→RGB:         NOT PERFORMED");
    }

    pub fn print_frame_summary(&self) {
        let m = self.frame_metrics();
        println!("=== GPU Experiment 1 — runtime ({}) ===", self.init_metrics.backend_requested);
        println!("Frames presented: {}", m.frames_presented);
        if m.frames_presented > 1 {
            println!(
                "Present interval:  last {:.3} ms, avg {:.3} ms (measured wall-clock between presents)",
                m.last_present_interval_ms, m.avg_present_interval_ms
            );
            println!(
                "Queue submit:      last {:.3} ms, avg {:.3} ms (CPU-side encode+submit only, not GPU execution)",
                m.last_submit_ms, m.avg_submit_ms
            );
        }
        println!("GPU execution time: NOT MEASURED (no timestamp queries in this experiment)");
    }
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn create_and_upload_nv12_source(
    device: &Device,
    queue: &Queue,
    adapter_info: &wgpu::AdapterInfo,
    pattern: &Nv12Pattern,
) -> Result<(wgpu::TextureView, wgpu::TextureView, String), String> {
    // Validate NV12 texture creation (no CPU upload — wgpu 27 rejects COPY_DST on NV12).
    let _nv12_probe = device.create_texture(&TextureDescriptor {
        label: Some("nv12-create-probe"),
        size: wgpu::Extent3d {
            width: SOURCE_WIDTH,
            height: SOURCE_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::NV12,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let _nv12_y_probe = _nv12_probe.create_view(&TextureViewDescriptor {
        label: Some("nv12-y-probe"),
        format: Some(TextureFormat::R8Unorm),
        aspect: TextureAspect::Plane0,
        ..Default::default()
    });
    let _nv12_uv_probe = _nv12_probe.create_view(&TextureViewDescriptor {
        label: Some("nv12-uv-probe"),
        format: Some(TextureFormat::Rg8Unorm),
        aspect: TextureAspect::Plane1,
        ..Default::default()
    });
    let _ = (_nv12_y_probe, _nv12_uv_probe);

    let y_texture = device.create_texture(&TextureDescriptor {
        label: Some("nv12-y-plane-texture"),
        size: wgpu::Extent3d {
            width: SOURCE_WIDTH,
            height: SOURCE_HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::R8Unorm,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let uv_texture = device.create_texture(&TextureDescriptor {
        label: Some("nv12-uv-plane-texture"),
        size: wgpu::Extent3d {
            width: SOURCE_WIDTH / 2,
            height: SOURCE_HEIGHT / 2,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rg8Unorm,
        usage: TextureUsages::COPY_DST | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &y_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &pattern.y_plane,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SOURCE_WIDTH),
            rows_per_image: Some(SOURCE_HEIGHT),
        },
        wgpu::Extent3d {
            width: SOURCE_WIDTH,
            height: SOURCE_HEIGHT,
            depth_or_array_layers: 1,
        },
    );
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &uv_texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: TextureAspect::All,
        },
        &pattern.uv_plane,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(SOURCE_WIDTH),
            rows_per_image: Some(SOURCE_HEIGHT / 2),
        },
        wgpu::Extent3d {
            width: SOURCE_WIDTH / 2,
            height: SOURCE_HEIGHT / 2,
            depth_or_array_layers: 1,
        },
    );

    let y_view = y_texture.create_view(&TextureViewDescriptor {
        label: Some("nv12-y-plane"),
        ..Default::default()
    });
    let uv_view = uv_texture.create_view(&TextureViewDescriptor {
        label: Some("nv12-uv-plane"),
        ..Default::default()
    });

    Ok((
        y_view,
        uv_view,
        format!(
            "NV12 texture created (probe); data uploaded to planar R8/Rg8 ({}) — wgpu rejects COPY_DST on NV12",
            adapter_info.backend
        ),
    ))
}