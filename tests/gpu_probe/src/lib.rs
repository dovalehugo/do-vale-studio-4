//! GPU capability probing logic for Experiment 0.
//! Isolated from production crates — see `tests/gpu_probe/`.

use std::fmt::Write as _;

use wgpu::{
    Adapter, Backends, Device, DeviceDescriptor, ExperimentalFeatures, Features, Instance,
    InstanceDescriptor, Limits, Queue, TextureAspect, TextureDescriptor, TextureDimension,
    TextureFormat, TextureFormatFeatures, TextureUsages, TextureViewDescriptor,
    TextureViewDimension,
};

/// Usages required for the Studio 4 GPU video pipeline (decode import → shader → present).
pub const PIPELINE_TEXTURE_BINDING: TextureUsages = TextureUsages::TEXTURE_BINDING;
pub const PIPELINE_RENDER_TARGET: TextureUsages =
    TextureUsages::TEXTURE_BINDING.union(TextureUsages::RENDER_ATTACHMENT);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatSupportLevel {
    /// Adapter reports no capability for this format/configuration.
    NotSupported,
    /// Adapter or wgpu API reports the format exists, but creation/view/bind was not validated.
    NotValidated,
    /// `adapter.get_texture_format_features` allows the required usages (adapter-level only).
    AdapterSupported,
    /// Texture + views created successfully on a live device.
    UsableForPipeline,
}

#[derive(Debug, Clone)]
pub struct FormatProbeResult {
    pub format: TextureFormat,
    pub label: &'static str,
    pub adapter_feature_flag: Option<&'static str>,
    pub adapter_reports_feature: bool,
    pub adapter_format_features: TextureFormatFeatures,
    pub device_creation: Result<FormatSupportLevel, String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AdapterProbeResult {
    pub index: usize,
    pub name: String,
    pub vendor: u32,
    pub vendor_name: String,
    pub device_id: u32,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub features: Features,
    pub limits: Limits,
    pub format_probes: Vec<FormatProbeResult>,
    pub device_request_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeReport {
    pub wgpu_version: String,
    pub backends_requested: String,
    pub adapter_count: usize,
    pub adapters: Vec<AdapterProbeResult>,
}

pub fn vendor_name(vendor_id: u32) -> &'static str {
    match vendor_id {
        0x10DE => "NVIDIA",
        0x1002 => "AMD",
        0x8086 => "Intel",
        0x1414 => "Microsoft (WARP / Basic Render)",
        0x5143 => "Qualcomm",
        0x106B => "Apple",
        _ => "Unknown",
    }
}

pub fn backend_label(backend: wgpu::Backend) -> &'static str {
    match backend {
        wgpu::Backend::Vulkan => "Vulkan",
        wgpu::Backend::Gl => "OpenGL",
        wgpu::Backend::Metal => "Metal",
        wgpu::Backend::Dx12 => "DX12",
        wgpu::Backend::Noop => "Noop",
        wgpu::Backend::BrowserWebGpu => "BrowserWebGpu",
    }
}

pub fn device_type_label(device_type: wgpu::DeviceType) -> &'static str {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => "DiscreteGpu",
        wgpu::DeviceType::IntegratedGpu => "IntegratedGpu",
        wgpu::DeviceType::VirtualGpu => "VirtualGpu",
        wgpu::DeviceType::Cpu => "Cpu",
        wgpu::DeviceType::Other => "Other",
    }
}

pub fn support_level_label(level: FormatSupportLevel) -> &'static str {
    match level {
        FormatSupportLevel::NotSupported => "NOT SUPPORTED",
        FormatSupportLevel::NotValidated => "NOT VALIDATED",
        FormatSupportLevel::AdapterSupported => "SUPPORTED (adapter only)",
        FormatSupportLevel::UsableForPipeline => "USABLE FOR REQUIRED PIPELINE",
    }
}

pub fn texture_usages_label(usages: TextureUsages) -> String {
    let mut parts = Vec::new();
    if usages.contains(TextureUsages::COPY_SRC) {
        parts.push("COPY_SRC");
    }
    if usages.contains(TextureUsages::COPY_DST) {
        parts.push("COPY_DST");
    }
    if usages.contains(TextureUsages::TEXTURE_BINDING) {
        parts.push("TEXTURE_BINDING");
    }
    if usages.contains(TextureUsages::STORAGE_BINDING) {
        parts.push("STORAGE_BINDING");
    }
    if usages.contains(TextureUsages::RENDER_ATTACHMENT) {
        parts.push("RENDER_ATTACHMENT");
    }
    parts.join(" | ")
}

pub fn texture_format_features_summary(features: TextureFormatFeatures) -> String {
    format!(
        "allowed_usages={}; flags={:?}",
        texture_usages_label(features.allowed_usages),
        features.flags
    )
}

pub fn relevant_feature_flags(features: Features) -> Vec<&'static str> {
    let mut flags = Vec::new();
    if features.contains(Features::TEXTURE_FORMAT_NV12) {
        flags.push("TEXTURE_FORMAT_NV12");
    }
    if features.contains(Features::TEXTURE_FORMAT_P010) {
        flags.push("TEXTURE_FORMAT_P010");
    }
    if features.contains(Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES) {
        flags.push("TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES");
    }
    if features.contains(Features::TEXTURE_FORMAT_16BIT_NORM) {
        flags.push("TEXTURE_FORMAT_16BIT_NORM");
    }
    if features.contains(Features::EXTERNAL_TEXTURE) {
        flags.push("EXTERNAL_TEXTURE");
    }
    flags
}

fn adapter_reports_nv12(features: Features) -> bool {
    features.contains(Features::TEXTURE_FORMAT_NV12)
}

fn adapter_reports_p010(features: Features) -> bool {
    features.contains(Features::TEXTURE_FORMAT_P010)
}

fn required_usages_for_format(format: TextureFormat) -> TextureUsages {
    match format {
        TextureFormat::NV12 | TextureFormat::P010 => PIPELINE_TEXTURE_BINDING,
        TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm | TextureFormat::Rgba16Float => {
            PIPELINE_RENDER_TARGET
        }
        _ => PIPELINE_TEXTURE_BINDING,
    }
}

fn adapter_level_support(
    adapter: &Adapter,
    format: TextureFormat,
    required: TextureUsages,
) -> FormatSupportLevel {
    let features = adapter.get_texture_format_features(format);
    if !features.allowed_usages.contains(required) {
        return FormatSupportLevel::NotSupported;
    }
    FormatSupportLevel::AdapterSupported
}

fn try_create_texture_with_views(
    device: &Device,
    format: TextureFormat,
    required: TextureUsages,
) -> Result<(), String> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        try_create_texture_with_views_inner(device, format, required)
    }));

    match result {
        Ok(inner) => inner,
        Err(_) => Err("wgpu validation panic during texture/view creation".to_string()),
    }
}

fn try_create_texture_with_views_inner(
    device: &Device,
    format: TextureFormat,
    required: TextureUsages,
) -> Result<(), String> {
    let width = 256;
    let height = 256;

    let texture = device.create_texture(&TextureDescriptor {
        label: Some("gpu-probe-format-test"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format,
        usage: required,
        view_formats: &[],
    });

    match format {
        TextureFormat::NV12 | TextureFormat::P010 => {
            let (y_plane_format, uv_plane_format) = if format == TextureFormat::P010 {
                (TextureFormat::R16Unorm, TextureFormat::Rg16Unorm)
            } else {
                (TextureFormat::R8Unorm, TextureFormat::Rg8Unorm)
            };
            let y_view = texture.create_view(&TextureViewDescriptor {
                label: Some("gpu-probe-y-plane"),
                format: Some(y_plane_format),
                dimension: Some(TextureViewDimension::D2),
                aspect: TextureAspect::Plane0,
                ..Default::default()
            });
            let uv_view = texture.create_view(&TextureViewDescriptor {
                label: Some("gpu-probe-uv-plane"),
                format: Some(uv_plane_format),
                dimension: Some(TextureViewDimension::D2),
                aspect: TextureAspect::Plane1,
                ..Default::default()
            });
            let _ = (y_view, uv_view);
        }
        _ => {
            let _view = texture.create_view(&TextureViewDescriptor::default());
        }
    }

    Ok(())
}

fn probe_format_on_device(
    adapter: &Adapter,
    device: &Device,
    format: TextureFormat,
    label: &'static str,
    feature_flag: Option<&'static str>,
    adapter_features: Features,
) -> FormatProbeResult {
    let mut notes = Vec::new();
    let adapter_reports_feature = match feature_flag {
        Some("TEXTURE_FORMAT_NV12") => adapter_reports_nv12(adapter_features),
        Some("TEXTURE_FORMAT_P010") => adapter_reports_p010(adapter_features),
        _ => true,
    };

    if let Some(flag) = feature_flag {
        if !adapter_reports_feature {
            notes.push(format!("Adapter does not report feature `{flag}`"));
        }
    }

    let adapter_format_features = adapter.get_texture_format_features(format);
    let required = required_usages_for_format(format);

    if !adapter_format_features
        .allowed_usages
        .contains(required)
    {
        notes.push(format!(
            "Adapter format features missing required usages: {}",
            texture_usages_label(required)
        ));
    }

    let adapter_level = adapter_level_support(adapter, format, required);

    let device_creation = if adapter_level == FormatSupportLevel::NotSupported {
        Err("Adapter does not allow required usages for this format".to_string())
    } else if feature_flag.is_some() && !adapter_reports_feature {
        Err(format!(
            "Missing adapter feature `{}`",
            feature_flag.unwrap_or("")
        ))
    } else {
        try_create_texture_with_views(device, format, required).map_err(|e| e.to_string())
    };

    let device_creation = match device_creation {
        Ok(()) => Ok(FormatSupportLevel::UsableForPipeline),
        Err(message) => {
            notes.push(message);
            if adapter_level == FormatSupportLevel::AdapterSupported {
                Ok(FormatSupportLevel::NotValidated)
            } else {
                Ok(FormatSupportLevel::NotSupported)
            }
        }
    };

    FormatProbeResult {
        format,
        label,
        adapter_feature_flag: feature_flag,
        adapter_reports_feature,
        adapter_format_features,
        device_creation,
        notes,
    }
}

fn adapter_only_probe(
    adapter: &Adapter,
    features: Features,
    format: TextureFormat,
    label: &'static str,
    feature_flag: Option<&'static str>,
    error: &str,
) -> FormatProbeResult {
    FormatProbeResult {
        format,
        label,
        adapter_feature_flag: feature_flag,
        adapter_reports_feature: match feature_flag {
            Some("TEXTURE_FORMAT_NV12") => adapter_reports_nv12(features),
            Some("TEXTURE_FORMAT_P010") => adapter_reports_p010(features),
            _ => true,
        },
        adapter_format_features: adapter.get_texture_format_features(format),
        device_creation: Ok(FormatSupportLevel::NotValidated),
        notes: vec![format!("Device request failed: {error}")],
    }
}

async fn request_probe_device(
    adapter: &Adapter,
    features: Features,
) -> Result<(Device, Queue), String> {
    adapter
        .request_device(&DeviceDescriptor {
            label: Some("gpu-probe-device"),
            required_features: features,
            required_limits: Limits::default(),
            experimental_features: ExperimentalFeatures::disabled(),
            memory_hints: Default::default(),
            trace: Default::default(),
        })
        .await
        .map_err(|e| e.to_string())
}

async fn probe_single_adapter(index: usize, adapter: Adapter) -> AdapterProbeResult {
    let info = adapter.get_info();
    let features = adapter.features();
    let limits = adapter.limits();

    let probe_features = features
        & (Features::TEXTURE_FORMAT_NV12
            | Features::TEXTURE_FORMAT_P010
            | Features::TEXTURE_FORMAT_16BIT_NORM);

    let device_result = request_probe_device(&adapter, probe_features).await;

    let (format_probes, device_request_error) = match &device_result {
        Ok((device, _queue)) => {
            let probes = vec![
                probe_format_on_device(
                    &adapter,
                    device,
                    TextureFormat::NV12,
                    "NV12",
                    Some("TEXTURE_FORMAT_NV12"),
                    features,
                ),
                probe_format_on_device(
                    &adapter,
                    device,
                    TextureFormat::P010,
                    "P010",
                    Some("TEXTURE_FORMAT_P010"),
                    features,
                ),
                probe_format_on_device(
                    &adapter,
                    device,
                    TextureFormat::Rgba8Unorm,
                    "RGBA8Unorm",
                    None,
                    features,
                ),
                probe_format_on_device(
                    &adapter,
                    device,
                    TextureFormat::Bgra8Unorm,
                    "BGRA8Unorm",
                    None,
                    features,
                ),
                probe_format_on_device(
                    &adapter,
                    device,
                    TextureFormat::Rgba16Float,
                    "RGBA16Float",
                    None,
                    features,
                ),
            ];
            (probes, None)
        }
        Err(error) => {
            let probes = vec![
                adapter_only_probe(
                    &adapter,
                    features,
                    TextureFormat::NV12,
                    "NV12",
                    Some("TEXTURE_FORMAT_NV12"),
                    error,
                ),
                adapter_only_probe(
                    &adapter,
                    features,
                    TextureFormat::P010,
                    "P010",
                    Some("TEXTURE_FORMAT_P010"),
                    error,
                ),
                adapter_only_probe(
                    &adapter,
                    features,
                    TextureFormat::Rgba8Unorm,
                    "RGBA8Unorm",
                    None,
                    error,
                ),
                adapter_only_probe(
                    &adapter,
                    features,
                    TextureFormat::Bgra8Unorm,
                    "BGRA8Unorm",
                    None,
                    error,
                ),
                adapter_only_probe(
                    &adapter,
                    features,
                    TextureFormat::Rgba16Float,
                    "RGBA16Float",
                    None,
                    error,
                ),
            ];
            (probes, Some(error.clone()))
        }
    };

    AdapterProbeResult {
        index,
        name: info.name,
        vendor: info.vendor,
        vendor_name: vendor_name(info.vendor).to_string(),
        device_id: info.device,
        backend: backend_label(info.backend).to_string(),
        device_type: device_type_label(info.device_type).to_string(),
        driver: info.driver,
        driver_info: info.driver_info,
        features,
        limits,
        format_probes,
        device_request_error,
    }
}

fn failed_adapter_result(index: usize, message: String) -> AdapterProbeResult {
    AdapterProbeResult {
        index,
        name: "<probe failed>".to_string(),
        vendor: 0,
        vendor_name: "Unknown".to_string(),
        device_id: 0,
        backend: "Unknown".to_string(),
        device_type: "Unknown".to_string(),
        driver: String::new(),
        driver_info: String::new(),
        features: Features::empty(),
        limits: Limits::default(),
        format_probes: Vec::new(),
        device_request_error: Some(message),
    }
}

pub async fn run_probe() -> ProbeReport {
    let backends = Backends::DX12 | Backends::VULKAN | Backends::GL;
    let instance = Instance::new(&InstanceDescriptor {
        backends,
        ..Default::default()
    });

    let adapters = instance.enumerate_adapters(backends);
    let mut results = Vec::new();

    for (index, adapter) in adapters.into_iter().enumerate() {
        let probe_future = probe_single_adapter(index, adapter);
        let probe_result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| pollster::block_on(probe_future)));

        match probe_result {
            Ok(result) => results.push(result),
            Err(_) => results.push(failed_adapter_result(
                index,
                "panic while probing adapter (driver/wgpu-hal failure)".to_string(),
            )),
        }
    }

    ProbeReport {
        wgpu_version: "27".to_string(),
        backends_requested: "DX12 | VULKAN | GL".to_string(),
        adapter_count: results.len(),
        adapters: results,
    }
}

pub fn format_report(report: &ProbeReport) -> String {
    let mut out = String::new();

    writeln!(out, "================================================================").unwrap();
    writeln!(out, " Do Vale Studio 4 — GPU Capability Probe (Experiment 0)").unwrap();
    writeln!(out, "================================================================").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "wgpu dependency version: {}", report.wgpu_version).unwrap();
    writeln!(out, "Backends requested: {}", report.backends_requested).unwrap();
    writeln!(out, "Adapters found: {}", report.adapter_count).unwrap();
    writeln!(out).unwrap();

    for adapter in &report.adapters {
        writeln!(out, "----------------------------------------------------------------").unwrap();
        writeln!(out, "Adapter #{}", adapter.index).unwrap();
        writeln!(out, "----------------------------------------------------------------").unwrap();
        writeln!(out, "  Name:         {}", adapter.name).unwrap();
        writeln!(
            out,
            "  Vendor:       {} (0x{:04X})",
            adapter.vendor_name, adapter.vendor
        )
        .unwrap();
        writeln!(out, "  Device ID:    0x{:04X}", adapter.device_id).unwrap();
        writeln!(out, "  Backend:      {}", adapter.backend).unwrap();
        writeln!(out, "  Device type:  {}", adapter.device_type).unwrap();
        writeln!(out, "  Driver:       {}", adapter.driver).unwrap();
        writeln!(out, "  Driver info:  {}", adapter.driver_info).unwrap();
        writeln!(
            out,
            "  Relevant features: {}",
            if relevant_feature_flags(adapter.features).is_empty() {
                "(none)".to_string()
            } else {
                relevant_feature_flags(adapter.features).join(", ")
            }
        )
        .unwrap();
        writeln!(
            out,
            "  NV12 feature: {}",
            if adapter_reports_nv12(adapter.features) {
                "reported by adapter"
            } else {
                "NOT reported"
            }
        )
        .unwrap();
        writeln!(
            out,
            "  P010 feature: {}",
            if adapter_reports_p010(adapter.features) {
                "reported by adapter"
            } else {
                "NOT reported"
            }
        )
        .unwrap();
        writeln!(
            out,
            "  Max 2D tex:   {} x {}",
            adapter.limits.max_texture_dimension_2d, adapter.limits.max_texture_dimension_2d
        )
        .unwrap();
        writeln!(
            out,
            "  Max bind groups: {}",
            adapter.limits.max_bind_groups
        )
        .unwrap();
        writeln!(
            out,
            "  Max sampled tex/stage: {}",
            adapter.limits.max_sampled_textures_per_shader_stage
        )
        .unwrap();
        writeln!(
            out,
            "  Max storage tex/stage: {}",
            adapter.limits.max_storage_textures_per_shader_stage
        )
        .unwrap();

        if let Some(err) = &adapter.device_request_error {
            writeln!(out, "  Device open:  FAILED — {err}").unwrap();
        } else {
            writeln!(out, "  Device open:  OK").unwrap();
        }

        writeln!(out).unwrap();
        writeln!(out, "  Format probes:").unwrap();
        for probe in &adapter.format_probes {
            let level = match &probe.device_creation {
                Ok(level) => *level,
                Err(_) => FormatSupportLevel::NotSupported,
            };
            writeln!(out, "    [{}]", probe.label).unwrap();
            writeln!(out, "      Result: {}", support_level_label(level)).unwrap();
            if let Some(flag) = probe.adapter_feature_flag {
                writeln!(
                    out,
                    "      Adapter feature `{}`: {}",
                    flag,
                    if probe.adapter_reports_feature {
                        "present"
                    } else {
                        "absent"
                    }
                )
                .unwrap();
            }
            writeln!(
                out,
                "      Adapter format features: {}",
                texture_format_features_summary(probe.adapter_format_features)
            )
            .unwrap();
            for note in &probe.notes {
                writeln!(out, "      Note: {note}").unwrap();
            }
        }
        writeln!(out).unwrap();
    }

    writeln!(out, "================================================================").unwrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_name_maps_known_ids() {
        assert_eq!(vendor_name(0x10DE), "NVIDIA");
        assert_eq!(vendor_name(0x8086), "Intel");
        assert_eq!(vendor_name(0x9999), "Unknown");
    }

    #[test]
    fn support_level_labels_are_stable() {
        assert_eq!(
            support_level_label(FormatSupportLevel::UsableForPipeline),
            "USABLE FOR REQUIRED PIPELINE"
        );
    }

    #[test]
    fn pipeline_render_target_includes_binding_and_attachment() {
        assert!(PIPELINE_RENDER_TARGET.contains(TextureUsages::TEXTURE_BINDING));
        assert!(PIPELINE_RENDER_TARGET.contains(TextureUsages::RENDER_ATTACHMENT));
    }
}
