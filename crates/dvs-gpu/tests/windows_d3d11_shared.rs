//! Windows hardware integration test for D3D11 shared NV12 producer resources.

#![cfg(target_os = "windows")]

use dvs_gpu::{
    D3d11DecodedSurfaceRef, DxgiAdapterLuid, FenceTimeline, GpuError, SharedNv12TextureDesc,
    WindowsD3d11SharedNv12Producer,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11_BIND_SHADER_RESOURCE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
    D3D11_TEXTURE2D_DESC, D3D11_USAGE_DEFAULT, D3D11CreateDevice, ID3D11Device,
    ID3D11DeviceContext, ID3D11Texture2D,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_NV12, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::{IDXGIAdapter, IDXGIDevice};
use windows::core::Interface;

const WIDTH: u32 = 3840;
const HEIGHT: u32 = 2176;

fn create_hardware_d3d11_device() -> (ID3D11Device, ID3D11DeviceContext) {
    let mut device = None;
    let mut context = None;
    let mut feature_level = D3D_FEATURE_LEVEL_11_0;
    // SAFETY: Creates a local hardware D3D11 device for isolated integration testing only.
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut device),
            Some(&mut feature_level),
            Some(&mut context),
        )
        .expect("D3D11CreateDevice");
    }
    (device.expect("device"), context.expect("context"))
}

fn extract_d3d11_adapter_luid(device: &ID3D11Device) -> DxgiAdapterLuid {
    let dxgi_device: IDXGIDevice = device.cast().expect("IDXGIDevice");
    // SAFETY: DXGI adapter queries are read-only metadata on a live D3D11 device.
    unsafe {
        let adapter: IDXGIAdapter = dxgi_device.GetAdapter().expect("GetAdapter");
        let desc = adapter.GetDesc().expect("GetDesc");
        DxgiAdapterLuid::new(desc.AdapterLuid.LowPart, desc.AdapterLuid.HighPart)
    }
}

fn create_test_nv12_texture(device: &ID3D11Device) -> ID3D11Texture2D {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: WIDTH,
        Height: HEIGHT,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_NV12,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
        CPUAccessFlags: 0,
        MiscFlags: 0,
    };
    let mut texture = None;
    // SAFETY: `device` is valid; `desc` describes a compatible NV12 test source texture.
    unsafe {
        device
            .CreateTexture2D(&desc, None, Some(&mut texture))
            .expect("CreateTexture2D source");
    }
    texture.expect("source texture")
}

#[test]
#[ignore = "requires Windows hardware D3D11 device"]
fn windows_d3d11_shared_nv12_producer_hardware() {
    let (device, context) = create_hardware_d3d11_device();
    let adapter_luid = extract_d3d11_adapter_luid(&device);
    assert_ne!(adapter_luid.low_part(), 0);

    let desc = SharedNv12TextureDesc::new(WIDTH, HEIGHT).expect("desc");
    let wrong_luid = DxgiAdapterLuid::new(
        adapter_luid.low_part().wrapping_add(1),
        adapter_luid.high_part(),
    );
    assert!(matches!(
        WindowsD3d11SharedNv12Producer::new(&device, &context, wrong_luid, desc),
        Err(GpuError::AdapterLuidMismatch { .. })
    ));

    let mut producer = WindowsD3d11SharedNv12Producer::new(&device, &context, adapter_luid, desc)
        .expect("producer");

    assert_eq!(producer.adapter_luid(), adapter_luid);
    assert_eq!(producer.desc().allocation_width(), WIDTH);
    assert_eq!(producer.desc().allocation_height(), HEIGHT);

    let source = create_test_nv12_texture(&device);
    let frame = D3d11DecodedSurfaceRef::new(&source, 0).expect("frame");
    let fence_values = FenceTimeline::new().current().expect("frame 0");

    producer
        .produce_frame(frame, fence_values)
        .expect("produce_frame frame 0");
}
