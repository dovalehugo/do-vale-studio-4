//! Green-frame root-cause visual diagnostic (`--visual-diagnostic`).
//!
//! Keys 1–6: surface / shader / synthetic / live real planes.
//! Keys 7–8: frozen real import experiments (one variable each).
//! Submodes while in 7/8: Y=Plane0, U=Plane1, F=full NV12.
//! Does not alter benchmark or `--visual` behavior.

use std::path::Path;
use std::sync::Arc;

use windows::Win32::Graphics::Direct3D12::ID3D12Fence;
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::core::Interface;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::wgpu_hal_interop::{self, WgpuDx12Context};
use wgpu::hal::api::Dx12;

const FIXTURE_REL: &str = "docs/fixtures/test_4k_hevc_8bit30.mp4";
const SYNTH_W: u32 = 1920;
const SYNTH_H: u32 = 1080;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrozenFamily {
    Test7NoMutex,
    Test8KeyedMutex,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrozenPlane {
    Plane0,
    Plane1,
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiagTest {
    Surface,
    ConstantShader,
    SyntheticControl,
    RealPlane0,
    RealPlane1,
    RealNv12,
    Frozen {
        family: FrozenFamily,
        plane: FrozenPlane,
    },
}

impl DiagTest {
    fn from_digit(c: char) -> Option<Self> {
        match c {
            '1' => Some(Self::Surface),
            '2' => Some(Self::ConstantShader),
            '3' => Some(Self::SyntheticControl),
            '4' => Some(Self::RealPlane0),
            '5' => Some(Self::RealPlane1),
            '6' => Some(Self::RealNv12),
            '7' => Some(Self::Frozen {
                family: FrozenFamily::Test7NoMutex,
                plane: FrozenPlane::Full,
            }),
            '8' => Some(Self::Frozen {
                family: FrozenFamily::Test8KeyedMutex,
                plane: FrozenPlane::Full,
            }),
            _ => None,
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Surface => "DIAG 1 — SURFACE (magenta)",
            Self::ConstantShader => "DIAG 2 — CONSTANT SHADER (cyan)",
            Self::SyntheticControl => "DIAG 3 — SYNTHETIC NV12 CONTROL",
            Self::RealPlane0 => "DIAG 4 — REAL PLANE0 Y-ONLY",
            Self::RealPlane1 => "DIAG 5 — REAL PLANE1 RAW",
            Self::RealNv12 => "DIAG 6 — REAL NV12 FULL",
            Self::Frozen {
                family: FrozenFamily::Test7NoMutex,
                plane: FrozenPlane::Plane0,
            } => "DIAG 7 — FROZEN REAL Plane0 (no keyed mutex)",
            Self::Frozen {
                family: FrozenFamily::Test7NoMutex,
                plane: FrozenPlane::Plane1,
            } => "DIAG 7 — FROZEN REAL Plane1 (no keyed mutex)",
            Self::Frozen {
                family: FrozenFamily::Test7NoMutex,
                plane: FrozenPlane::Full,
            } => "DIAG 7 — FROZEN REAL NV12 FULL (no keyed mutex)",
            Self::Frozen {
                family: FrozenFamily::Test8KeyedMutex,
                plane: FrozenPlane::Plane0,
            } => "DIAG 8 — FROZEN REAL Plane0 (keyed mutex)",
            Self::Frozen {
                family: FrozenFamily::Test8KeyedMutex,
                plane: FrozenPlane::Plane1,
            } => "DIAG 8 — FROZEN REAL Plane1 (keyed mutex)",
            Self::Frozen {
                family: FrozenFamily::Test8KeyedMutex,
                plane: FrozenPlane::Full,
            } => "DIAG 8 — FROZEN REAL NV12 FULL (keyed mutex)",
        }
    }

    fn needs_live_real_decode(self) -> bool {
        matches!(self, Self::RealPlane0 | Self::RealPlane1 | Self::RealNv12)
    }

    fn frozen_family(self) -> Option<FrozenFamily> {
        match self {
            Self::Frozen { family, .. } => Some(family),
            _ => None,
        }
    }
}

struct Pipelines {
    constant: wgpu::RenderPipeline,
    synthetic: wgpu::RenderPipeline,
    plane0: wgpu::RenderPipeline,
    plane1: wgpu::RenderPipeline,
    full: wgpu::RenderPipeline,
}

struct DiagnosticResources {
    probe: crate::ProbeResult,
    context: WgpuDx12Context,
    _imported_texture: wgpu::Texture,
    cached_fence: ID3D12Fence,
    timeline: crate::multi_frame::ContinuousFramebufferTimeline,
    /// One-shot fence values for TEST 7/8 frozen import (not the continuous timeline).
    frozen_fence_value: u64,
    real_bind_group: wgpu::BindGroup,
    synthetic_bind_group: wgpu::BindGroup,
    _synth_y: wgpu::Texture,
    _synth_uv: wgpu::Texture,
    pipelines: Pipelines,
    active: DiagTest,
    /// Which frozen family has a completed one-shot import (no further D3D11 writes).
    frozen_prepared: Option<FrozenFamily>,
    frame_counter: u64,
}

enum DiagState {
    Uninitialized,
    Ready(DiagnosticResources),
    Failed(String),
}

struct VisualDiagnosticApp {
    fixture: std::path::PathBuf,
    state: DiagState,
}

impl VisualDiagnosticApp {
    fn new(fixture: std::path::PathBuf) -> Self {
        Self {
            fixture,
            state: DiagState::Uninitialized,
        }
    }

    fn print_banner() {
        println!();
        println!("==================================================");
        println!("VISUAL DIAGNOSTIC MODE (green-frame root cause)");
        println!("==================================================");
        println!();
        println!("Real HEVC fixture: {FIXTURE_REL}");
        println!("Window: 1280x720");
        println!();
        println!("Keys:");
        println!("  1  SURFACE — solid magenta");
        println!("  2  CONSTANT SHADER — cyan (no sample)");
        println!("  3  SYNTHETIC NV12 — Exp1-style control");
        println!("  4  REAL Plane0 Y-only grayscale (live decode)");
        println!("  5  REAL Plane1 raw RG (live decode)");
        println!("  6  REAL NV12 full BT.709 (live decode)");
        println!("  7  TEST 7 FROZEN REAL IMPORT (no keyed mutex)");
        println!("  8  TEST 8 FROZEN REAL IMPORT (AcquireSync/ReleaseSync)");
        println!("     While in 7/8: Y=Plane0  U=Plane1  F=full NV12");
        println!("  ESC / close — exit");
        println!();
        println!("Attribution:");
        println!("  7 visible, 8 unused → reverse-reuse race");
        println!("  7 black, 8 visible → keyed-mutex producer required");
        println!("  both black → wgpu 27 external/planar state next");
        println!();
        println!("This mode does NOT report Experiment 2 FPS.");
        println!("==================================================");
        println!();
    }

    fn resize_surface(context: &mut WgpuDx12Context, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        context.surface_config.width = width;
        context.surface_config.height = height;
        context
            .surface
            .configure(&context.device, &context.surface_config);
    }

    fn set_active(res: &mut DiagnosticResources, test: DiagTest) {
        res.active = test;
        res.context
            ._window
            .set_title(&format!("Do Vale Studio 4 — {}", test.title()));
        println!("Active test: {}", test.title());
    }

    fn activate_test(res: &mut DiagnosticResources, test: DiagTest) -> Result<(), String> {
        if let Some(family) = test.frozen_family() {
            // Always re-run the one-shot for this family so 7 vs 8 stay attributable.
            let with_keyed = matches!(family, FrozenFamily::Test8KeyedMutex);
            println!();
            println!("==================================================");
            if with_keyed {
                println!("TEST 8 — KEYED-MUTEX PRODUCER CONTROL (one-shot)");
            } else {
                println!("TEST 7 — FROZEN REAL IMPORT (one-shot, no keyed mutex)");
            }
            println!("==================================================");
            crate::diagnostic_frozen_real_import(
                &mut res.probe,
                &res.context,
                &res.cached_fence,
                res.frozen_fence_value,
                with_keyed,
            )?;
            res.frozen_fence_value += 1;
            res.frozen_prepared = Some(family);
            println!("Frozen family prepared; no further D3D11 writes until 7/8 pressed again.");
            println!("Submodes: press Y (Plane0), U (Plane1), F (full).");
            println!();
        } else {
            res.frozen_prepared = None;
        }
        Self::set_active(res, test);
        Ok(())
    }

    fn set_frozen_plane(res: &mut DiagnosticResources, plane: FrozenPlane) {
        let Some(family) = res.active.frozen_family() else {
            println!("Y/U/F submodes only apply after pressing 7 or 8");
            return;
        };
        if res.frozen_prepared != Some(family) {
            println!("Frozen import not prepared for this family — press 7 or 8 first");
            return;
        }
        Self::set_active(res, DiagTest::Frozen { family, plane });
    }

    fn present_surface_magenta(context: &WgpuDx12Context) -> Result<(), String> {
        let surface_texture = context
            .surface
            .get_current_texture()
            .map_err(|e| format!("get_current_texture: {e}"))?;
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("diag-surface"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("diag-surface-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 1.0,
                            g: 0.0,
                            b: 1.0,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        context.queue.submit(Some(encoder.finish()));
        surface_texture.present();
        Ok(())
    }

    fn present_pipeline(
        context: &WgpuDx12Context,
        pipeline: &wgpu::RenderPipeline,
        bind_group: &wgpu::BindGroup,
    ) -> Result<(), String> {
        let surface_texture = context
            .surface
            .get_current_texture()
            .map_err(|e| format!("get_current_texture: {e}"))?;
        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = context
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("diag-draw"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("diag-draw-pass"),
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

    fn advance_real_frame(res: &mut DiagnosticResources) -> Result<(), String> {
        let mut decoded = 0u32;
        let mut copies = 0u32;
        let mut decode_ms = 0.0;
        let mut copy_ms = 0.0;
        let mut sync_ms = 0.0;

        let run = |res: &mut DiagnosticResources,
                   decoded: &mut u32,
                   copies: &mut u32,
                   decode_ms: &mut f64,
                   copy_ms: &mut f64,
                   sync_ms: &mut f64|
         -> Result<(), String> {
            crate::multi_frame::decode_copy_and_sync(
                &mut res.probe,
                &res.context,
                &res.cached_fence,
                &mut res.timeline,
                decoded,
                copies,
                decode_ms,
                copy_ms,
                sync_ms,
            )
        };

        match run(
            res,
            &mut decoded,
            &mut copies,
            &mut decode_ms,
            &mut copy_ms,
            &mut sync_ms,
        ) {
            Ok(()) => Ok(()),
            Err(err) if err.contains("EOF") => {
                crate::restart_fixture_decode(&res.probe)?;
                run(
                    res,
                    &mut decoded,
                    &mut copies,
                    &mut decode_ms,
                    &mut copy_ms,
                    &mut sync_ms,
                )
            }
            Err(err) => Err(err),
        }
    }

    fn redraw(res: &mut DiagnosticResources) -> Result<(), String> {
        let live = res.active.needs_live_real_decode();
        if live {
            Self::advance_real_frame(res)?;
        }

        match res.active {
            DiagTest::Surface => Self::present_surface_magenta(&res.context)?,
            DiagTest::ConstantShader => {
                Self::present_pipeline(&res.context, &res.pipelines.constant, &res.real_bind_group)?
            }
            DiagTest::SyntheticControl => Self::present_pipeline(
                &res.context,
                &res.pipelines.synthetic,
                &res.synthetic_bind_group,
            )?,
            DiagTest::RealPlane0
            | DiagTest::Frozen {
                plane: FrozenPlane::Plane0,
                ..
            } => Self::present_pipeline(&res.context, &res.pipelines.plane0, &res.real_bind_group)?,
            DiagTest::RealPlane1
            | DiagTest::Frozen {
                plane: FrozenPlane::Plane1,
                ..
            } => Self::present_pipeline(&res.context, &res.pipelines.plane1, &res.real_bind_group)?,
            DiagTest::RealNv12
            | DiagTest::Frozen {
                plane: FrozenPlane::Full,
                ..
            } => Self::present_pipeline(&res.context, &res.pipelines.full, &res.real_bind_group)?,
        }

        if live {
            crate::multi_frame::finish_continuous_frame_consumer(
                &res.context,
                &res.cached_fence,
                &mut res.timeline,
            )?;
        }

        res.frame_counter += 1;
        if res.frame_counter == 1 || res.frame_counter.is_multiple_of(120) {
            println!(
                "diag frames={} active={} timeline_frame={} frozen_prepared={:?}",
                res.frame_counter,
                res.active.title(),
                res.timeline.frame_index(),
                res.frozen_prepared
            );
        }
        Ok(())
    }

    fn log_static_audit(probe: &crate::ProbeResult, context: &WgpuDx12Context) {
        println!("=== Static audit instrumentation ===");
        println!(
            "shareable misc_flags: 0x{:08X} (KEYEDMUTEX bit set if SHARED_KEYEDMUTEX used)",
            probe.shareable_texture.desc.misc_flags
        );
        println!("KEYEDMUTEX AcquireSync/ReleaseSync in live path: YES (continuous playback)");
        println!("imported texture size (wgpu desc): 3840x2176 NV12 TEXTURE_BINDING");
        println!("Plane0 view: R8Unorm + TextureAspect::Plane0");
        println!("Plane1 view: Rg8Unorm + TextureAspect::Plane1");

        if let Some(shareable) = probe._shareable_texture.as_ref() {
            unsafe {
                if let Ok(device) = shareable.0.GetDevice() {
                    if let Ok(dxgi) = device.cast::<IDXGIDevice>() {
                        if let Ok(adapter) = dxgi.GetAdapter() {
                            if let Ok(desc) = adapter.GetDesc() {
                                println!(
                                    "D3D11 adapter LUID: high={} low={} name={}",
                                    desc.AdapterLuid.HighPart,
                                    desc.AdapterLuid.LowPart,
                                    probe.d3d12_open.adapter_name
                                );
                            }
                        }
                    }
                }
            }
        }

        println!("wgpu adapter name: {}", context.adapter_name);
        unsafe {
            if let Some(hal) = context.device.as_hal::<Dx12>() {
                let wait_q = Interface::as_raw(hal.raw_queue()) as usize;
                println!("wgpu DX12 present/Wait queue COM ptr: 0x{wait_q:x}");
            }
        }
        if let Some(q) = probe.shared_fence_sync.d3d12_command_queue_ptr() {
            println!("probe SharedFenceSync D3D12 Wait queue COM ptr: 0x{q:x}");
            println!("TEST 7/8: Signal D3D11 fence then Wait ONLY on wgpu present queue");
        }
        println!("==================================================");
        println!();
    }

    fn build_pipelines(
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        layout: &wgpu::PipelineLayout,
        shader: &wgpu::ShaderModule,
    ) -> Pipelines {
        let make = |entry: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(layout),
                vertex: wgpu::VertexState {
                    module: shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: shader,
                    entry_point: Some(entry),
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
            })
        };

        Pipelines {
            constant: make("fs_constant"),
            synthetic: make("fs_synthetic_full"),
            plane0: make("fs_plane0_y_only"),
            plane1: make("fs_plane1_raw"),
            full: make("fs_full_nv12"),
        }
    }

    fn create_synthetic_bind_group(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
    ) -> (wgpu::Texture, wgpu::Texture, wgpu::BindGroup) {
        let (y_bytes, uv_bytes) = generate_synth_nv12(SYNTH_W, SYNTH_H);
        let y_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("diag-synth-y"),
            size: wgpu::Extent3d {
                width: SYNTH_W,
                height: SYNTH_H,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let uv_tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("diag-synth-uv"),
            size: wgpu::Extent3d {
                width: SYNTH_W / 2,
                height: SYNTH_H / 2,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg8Unorm,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &y_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &y_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SYNTH_W),
                rows_per_image: Some(SYNTH_H),
            },
            wgpu::Extent3d {
                width: SYNTH_W,
                height: SYNTH_H,
                depth_or_array_layers: 1,
            },
        );
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &uv_tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &uv_bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SYNTH_W),
                rows_per_image: Some(SYNTH_H / 2),
            },
            wgpu::Extent3d {
                width: SYNTH_W / 2,
                height: SYNTH_H / 2,
                depth_or_array_layers: 1,
            },
        );
        let y_view = y_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let uv_view = uv_tex.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diag-synth-bg"),
            layout,
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
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        });
        (y_tex, uv_tex, bind_group)
    }

    fn initialize_pipeline(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title("Do Vale Studio 4 — DIAG visual diagnostic")
                        .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 720u32)),
                )
                .map_err(|e| format!("create_window failed: {e}"))?,
        );

        let context = pollster::block_on(wgpu_hal_interop::init_wgpu_context_with_window(window))?;
        let probe = crate::probe_format_and_open_decoder(&self.fixture)?;

        Self::log_static_audit(&probe, &context);

        let wgpu_interop = wgpu_hal_interop::import_shared_d3d12_nv12_into_wgpu(
            context,
            probe._shared_nt_handle.handle(),
            probe.shared_fence_sync.fence_handle(),
            &probe.d3d12_open.adapter_name,
            probe.shared_fence_sync.info.synchronization_valid,
        );
        if !wgpu_interop.info.interop_valid {
            return Err(wgpu_interop
                .info
                .error
                .unwrap_or_else(|| "wgpu interop failed".to_string()));
        }

        let imported_texture = wgpu_interop
            ._texture
            .as_ref()
            .ok_or_else(|| "imported texture missing".to_string())?;
        let context_ref = wgpu_interop
            ._context
            .as_ref()
            .ok_or_else(|| "context missing".to_string())?;
        let cached_fence = wgpu_interop
            .cached_wgpu_fence
            .as_ref()
            .ok_or_else(|| "cached fence missing".to_string())?
            .clone();

        let y_view = imported_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("diag-real-y"),
            format: Some(wgpu::TextureFormat::R8Unorm),
            aspect: wgpu::TextureAspect::Plane0,
            ..Default::default()
        });
        let uv_view = imported_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("diag-real-uv"),
            format: Some(wgpu::TextureFormat::Rg8Unorm),
            aspect: wgpu::TextureAspect::Plane1,
            ..Default::default()
        });

        let device = &context_ref.device;
        let surface_format = context_ref.surface_config.format;
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diag-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/diagnostic.wgsl").into()),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("diag-linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("diag-bgl"),
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
        let real_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("diag-real-bg"),
            layout: &bgl,
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
        let (synth_y, synth_uv, synthetic_bind_group) =
            Self::create_synthetic_bind_group(device, &context_ref.queue, &bgl, &sampler);
        let pll = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("diag-pll"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipelines = Self::build_pipelines(device, surface_format, &pll, &shader);

        drop(y_view);
        drop(uv_view);

        let context = wgpu_interop
            ._context
            .ok_or_else(|| "context missing after take".to_string())?;
        let imported_texture = wgpu_interop
            ._texture
            .ok_or_else(|| "texture missing after take".to_string())?;

        let resources = DiagnosticResources {
            probe,
            context,
            _imported_texture: imported_texture,
            cached_fence,
            timeline: crate::multi_frame::ContinuousFramebufferTimeline::new(),
            frozen_fence_value: 2,
            real_bind_group,
            synthetic_bind_group,
            _synth_y: synth_y,
            _synth_uv: synth_uv,
            pipelines,
            active: DiagTest::Surface,
            frozen_prepared: None,
            frame_counter: 0,
        };
        self.state = DiagState::Ready(resources);
        if let DiagState::Ready(res) = &mut self.state {
            Self::set_active(res, DiagTest::Surface);
        }
        Ok(())
    }
}

impl ApplicationHandler for VisualDiagnosticApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !matches!(self.state, DiagState::Uninitialized) {
            return;
        }
        match self.initialize_pipeline(event_loop) {
            Ok(()) => {
                Self::print_banner();
                event_loop.set_control_flow(ControlFlow::Poll);
                if let DiagState::Ready(res) = &self.state {
                    res.context._window.request_redraw();
                }
            }
            Err(err) => {
                self.state = DiagState::Failed(err);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let DiagState::Ready(res) = &mut self.state {
                    Self::resize_surface(&mut res.context, size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let DiagState::Ready(res) = &mut self.state {
                    if let Err(err) = Self::redraw(res) {
                        eprintln!("diagnostic frame error: {err}");
                        self.state = DiagState::Failed(err);
                        event_loop.exit();
                    }
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if !event.state.is_pressed() {
                    return;
                }
                if matches!(event.logical_key, Key::Named(NamedKey::Escape)) {
                    event_loop.exit();
                    return;
                }
                if let Key::Character(c) = &event.logical_key {
                    let lower = c.to_ascii_lowercase();
                    if let Some(ch) = lower.chars().next() {
                        match ch {
                            'y' => {
                                if let DiagState::Ready(res) = &mut self.state {
                                    Self::set_frozen_plane(res, FrozenPlane::Plane0);
                                }
                            }
                            'u' => {
                                if let DiagState::Ready(res) = &mut self.state {
                                    Self::set_frozen_plane(res, FrozenPlane::Plane1);
                                }
                            }
                            'f' => {
                                if let DiagState::Ready(res) = &mut self.state {
                                    Self::set_frozen_plane(res, FrozenPlane::Full);
                                }
                            }
                            digit => {
                                if let Some(test) = DiagTest::from_digit(digit) {
                                    if let DiagState::Ready(res) = &mut self.state {
                                        if let Err(err) = Self::activate_test(res, test) {
                                            eprintln!("activate test error: {err}");
                                            self.state = DiagState::Failed(err);
                                            event_loop.exit();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let DiagState::Ready(res) = &self.state {
            res.context._window.request_redraw();
        }
    }
}

/// Minimal Exp1-style color-bar pattern (Y/UV only).
fn generate_synth_nv12(width: u32, height: u32) -> (Vec<u8>, Vec<u8>) {
    let mut y = vec![16u8; (width * height) as usize];
    let mut uv = vec![128u8; (width * height / 2) as usize];
    let cols = 4u32;
    let rows = 3u32;
    let pw = width / cols;
    let ph = height / rows;
    let patches: [[(u8, u8, u8); 4]; 3] = [
        [
            (235, 128, 128),
            (210, 16, 146),
            (170, 166, 16),
            (149, 44, 21),
        ],
        [
            (105, 202, 222),
            (76, 84, 255),
            (29, 255, 107),
            (180, 98, 118),
        ],
        [
            (16, 128, 128),
            (64, 128, 128),
            (128, 128, 128),
            (192, 128, 128),
        ],
    ];
    for row in 0..rows {
        for col in 0..cols {
            let (yy, uu, vv) = patches[row as usize][col as usize];
            let x0 = col * pw;
            let y0 = row * ph;
            for py in y0..(y0 + ph).min(height) {
                for px in x0..(x0 + pw).min(width) {
                    y[(py * width + px) as usize] = yy;
                }
            }
            for py in (y0 / 2)..((y0 + ph).min(height) / 2) {
                for px in (x0 / 2)..((x0 + pw).min(width) / 2) {
                    let i = ((py * (width / 2) + px) * 2) as usize;
                    uv[i] = uu;
                    uv[i + 1] = vv;
                }
            }
        }
    }
    (y, uv)
}

pub fn run_visual_diagnostic(fixture: &Path) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| format!("EventLoop::new failed: {e}"))?;
    let mut app = VisualDiagnosticApp::new(fixture.to_path_buf());
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("EventLoop::run_app failed: {e}"))?;
    if let DiagState::Failed(err) = app.state {
        return Err(err);
    }
    Ok(())
}
