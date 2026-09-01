# Do Vale Studio 4 — GPU Experiment 2: D3D11VA → GPU-Resident wgpu Pipeline

**Date:** 2026-09-01  
**Machine:** Windows 10 22H2 (build 19045), AMD Ryzen 7 1700X, 32 GB RAM  
**GPU:** AMD Radeon (TM) RX 580 8 GB  
**Status:** **PASS EXCEPT HUMAN VISUAL VALIDATION** (architecture + automated corrective validation complete; human confirmation of rendered image still required)  
**Experiment package:** `tests/gpu_d3d11_interop`  
**Fixture:** `docs/fixtures/test_4k_hevc_8bit30.mp4` (HEVC Main 3840×2160 30000/1001)

---

## Final classification

**Automated:** PASS EXCEPT HUMAN VISUAL VALIDATION  

Architecture integrity remains proven. Corrective validation fixed fence reuse, frame accounting, wall-clock FPS measurement, and documentation accuracy. **Do not declare full Experiment 2 PASS until a human confirms the visual validation window shows correct fixture content.**

---

## 1. Objective

Validate that a real FFmpeg D3D11VA hardware-decoded HEVC frame (`AV_PIX_FMT_D3D11`) can reach the wgpu DX12 renderer **without CPU readback**:

```text
HEVC 4K fixture
  → FFmpeg D3D11VA (AV_PIX_FMT_D3D11)
  → ID3D11Texture2D decoder surface (NV12 backing)
  → GPU CopySubresourceRegion → shareable D3D11 NV12 texture
  → NT shared HANDLE
  → ID3D12Resource (same adapter)
  → shared GPU fence (D3D11 Signal → D3D12/wgpu Wait on cached fence)
  → wgpu-hal DX12 OpenSharedHandle + create_texture_from_hal
  → NV12 Plane0/Plane1 texture views
  → WGSL BT.709 limited-range YUV→RGB
  → surface present (1280×720 validation window)
```

This is a **GPU-resident pipeline**. It is **not** zero-copy — a GPU-to-GPU copy from the decoder surface into a shareable texture is required.

---

## 2. Hardware and OS

| Item | Value |
|------|-------|
| OS | Windows 10 22H2 (19045) |
| CPU | AMD Ryzen 7 1700X (8C/16T) |
| RAM | 32 GB |
| GPU | AMD Radeon (TM) RX 580 8 GB |
| Displays | 2× 1080p ~60 Hz |

---

## 3. Software versions

| Component | Version |
|-----------|---------|
| FFmpeg runtime | 9.0.1-full_build-www.gyan.dev |
| Rust edition | 2024 |
| wgpu | 27.0.1 |
| wgpu-hal | 27.0.4 |
| wgpu-core | 27.0.3 |
| windows crate | 0.58 |
| ffmpeg-sys-next | 8 |

---

## 4. Fixture

| Property | Value |
|----------|-------|
| File | `docs/fixtures/test_4k_hevc_8bit30.mp4` |
| Codec | HEVC Main |
| Resolution | 3840×2160 |
| Frame rate | 30000/1001 (~29.97 FPS) |
| Bit depth | 8-bit |
| Pixel format (container) | yuv420p |

---

## 5. FFmpeg D3D11VA configuration

- Hardware device: `d3d11va`
- `get_format` selected: `AV_PIX_FMT_D3D11` (171)
- Candidates observed: dxva2_vld, d3d11va_vld, d3d11, d3d12, vaapi, cuda, vulkan, yuv420p
- `hw_frames_ctx` present; hardware format `d3d11`, software backing `nv12`

---

## 6. Decoder texture properties

| Property | Value |
|----------|-------|
| Width | 3840 |
| Height (allocation) | 2176 |
| Visible height | 2160 |
| Format | DXGI_FORMAT_NV12 (103) |
| ArraySize | 20 (decoder pool) |
| BindFlags | 0x200 (`D3D11_BIND_DECODER`) |
| MiscFlags | 0 (not shareable) |

**Why decoder texture was not directly shareable:** Bind flags are decoder-only; no `D3D11_RESOURCE_MISC_SHARED_NTHANDLE`.

---

## 7. Shareable D3D11 texture

| Property | Value |
|----------|-------|
| Size | 3840×2176 NV12 |
| BindFlags | 0x8 (`D3D11_BIND_SHADER_RESOURCE`) |
| MiscFlags | 0x900 (`SHARED_NTHANDLE \| SHARED_KEYEDMUTEX`) |
| Copy | `CopySubresourceRegion` (decoder array slice → subresource 0) |
| CPU transfer | None |

---

## 8. D3D12 cross-API open

- Same adapter: AMD Radeon (TM) RX 580
- `OpenSharedHandle` → `ID3D12Resource` NV12 3840×2176
- Layout: `D3D12_TEXTURE_LAYOUT_UNKNOWN`
- Flags: 0x20

---

## 9. Keyed mutex experiment (failed)

- D3D11: `IDXGIKeyedMutex` available on shareable texture
- D3D12: `QueryInterface` for `IDXGIKeyedMutex` on opened resource → **E_NOINTERFACE (0x80004002)**
- **Not used** as final synchronization architecture

---

## 10. Shared GPU fence synchronization (validated)

- `ID3D11Device5` + `ID3D11DeviceContext4`: available
- `ID3D11Fence` with `D3D11_FENCE_FLAG_SHARED`
- Shared NT fence HANDLE created once
- D3D12 and wgpu each open the shared fence **once** and retain `ID3D12Fence`
- Per frame: D3D11 `Signal(n)` → D3D12/wgpu `Wait(cached_fence, n)` only (no re-open)
- GPU-side only; no CPU wait used as proof

---

## 11. wgpu-hal DX12 external resource mechanism

**Adapter selection (empirical, machine-specific):**

On the tested Windows 10 + AMD Radeon RX 580 configuration, initializing and selecting the wgpu DX12 adapter **before** creating the FFmpeg/D3D11VA device was required for the experiment to consistently select the RX 580 rather than Microsoft Basic Render Driver. **This is an empirical machine/driver-specific observation and has not been established as a universal DXGI requirement.**

**Mechanism used (wgpu 27.0.1 / wgpu-hal 27.0.4):**

1. Pre-init wgpu DX12 with surface-based adapter selection (`request_adapter` + winit `run_app`, 1280×720 window)
2. After decode/copy/fence: `device.as_hal::<Dx12>()`
3. `raw_device().OpenSharedHandle` on the NT texture handle
4. `raw_device().OpenSharedHandle` on the fence HANDLE **once**; retain `ID3D12Fence`
5. `raw_queue().Wait` on cached fence
6. `wgpu_hal::dx12::Device::texture_from_raw`
7. `device.create_texture_from_hal::<Dx12>`

Imported and wgpu-wrapped `ID3D12Resource` pointers matched (same COM object).

---

## 12. NV12 plane access

| Plane | Representation |
|-------|----------------|
| Y | `TextureFormat::R8Unorm`, `TextureAspect::Plane0` |
| UV | `TextureFormat::Rg8Unorm`, `TextureAspect::Plane1` |

Both planes reference the real imported decoder NV12 resource.

---

## 13. WGSL YUV → RGB

- Shader: `tests/gpu_d3d11_interop/shaders/nv12_to_rgb.wgsl`
- Color space: BT.709
- Range: limited (8-bit)
- Visible crop: `VISIBLE_V_SCALE = 2160/2176` excludes decoder padding lines

---

## 14. Multi-frame strategy

- Bounded single shareable D3D11 NV12 texture (reused)
- Bounded single shared fence HANDLE
- Bounded single wgpu-imported `ID3D12Fence` (opened once)
- Bounded single wgpu imported NV12 texture
- Measured run: exactly **90** decode → copy → sync → render → present cycles
- Monotonic fence values starting after the Step 32/33 init signal
- No unbounded HANDLE or texture allocation per frame

---

## 15. Visual validation

- Window: **1280×720** (16:9), not 256×256
- Path: real NV12 planes + existing WGSL + present
- Automated status after corrective run: **VISUAL VALIDATION READY**
- **Human visual validation: PENDING** — a person must confirm fixture content, orientation, aspect, no green/purple corruption, chroma, crop, BT.709 appearance
- API present success alone does **not** prove visual correctness

---

## 16. Performance measurements

**Measurement type: WALL-CLOCK END-TO-END THROUGHPUT**  
(Not GPU execution time — no GPU timestamp queries.)

Measured interval = exactly the 90-frame loop (decode + D3D11 copy submit + fence sync + wgpu submit + present).

Corrected values are produced by each run of `cargo run -p gpu-d3d11-interop` and printed as:

- `frames_processed`
- `elapsed_seconds`
- `FPS = frames_processed / elapsed_seconds`
- whether FPS ≥ 30000/1001 (~29.97)

Do not reuse older 61.77 FPS figures unless a new run reproduces them.

---

## 17. Forbidden-path validation

| Check | Result |
|-------|--------|
| `av_hwframe_transfer_data` in normal path | **NOT USED** |
| swscale in normal path | **NOT USED** |
| CPU RGBA conversion | **NOT USED** |
| GPU → CPU → GPU | **NO** |
| GPU → GPU decoder-surface copy | **YES** |

---

## 18. Limitations

1. **Adapter init ordering:** Empirical on this Windows 10 + RX 580 setup; not proven as a universal DXGI rule.
2. **Unsafe HAL interop:** `create_texture_from_hal` / `texture_from_raw` require documented lifetime/sync invariants in production.
3. **Decoder surface copy:** One GPU copy per frame from non-shareable decoder texture to shareable NV12.
4. **Keyed mutex:** Not available on D3D12 for opened shared NV12 on RX 580.
5. **Visual validation:** Requires human confirmation; automated PASS does not imply image quality PASS.
6. **Wall-clock FPS** includes present and CPU submission, not isolated GPU timestamps.

---

## 19. Future work

- Human visual confirmation to upgrade status from PARTIAL / PASS-EXCEPT-VISUAL to full PASS
- Integrate validated path into `dvs-gpu` / `dvs-render`
- Full 4K viewport scaling
- Production fence/frame pool scheduling
- Optional Vulkan interop path

---

## 20. Files

| Path | Role |
|------|------|
| `tests/gpu_d3d11_interop/src/main.rs` | Steps 1–32 decode/interop probe |
| `tests/gpu_d3d11_interop/src/wgpu_hal_interop.rs` | Step 33 wgpu import + cached fence |
| `tests/gpu_d3d11_interop/src/render_path.rs` | Steps 34–36 render |
| `tests/gpu_d3d11_interop/src/multi_frame.rs` | Steps 37–39 validation |
| `tests/gpu_d3d11_interop/shaders/nv12_to_rgb.wgsl` | BT.709 shader with 2160/2176 crop |

---

## 21. Step summary

| Step | Result |
|------|--------|
| 1–32 | PASS (D3D11VA through shared GPU fence) |
| 33 | PASS (wgpu-hal DX12 external resource; fence cached) |
| 34 | PASS (real NV12 plane views) |
| 35 | PASS (GPU YUV→RGB shader) |
| 36 | PASS (API present; visual correctness = human PENDING) |
| 37 | PASS (exactly 90 measured real frames) |
| 38 | PASS (cached fence; bounded reuse) |
| 39 | PASS when wall-clock FPS ≥ fixture rate |
| 40 | Documentation corrected (this file) |
