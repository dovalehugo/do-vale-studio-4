# Do Vale Studio 4 — GPU Experiment 1: NV12 → RGB Rendering

**Date:** 2026-09-01  
**Machine:** Windows 10 (build 19045), AMD Radeon RX 580  
**Experiment package:** `tests/gpu_nv12` (isolated, not part of production crates)  
**wgpu version:** 27.0.1  
**Prerequisite:** [GPU_PROBE_RESULTS.md](./GPU_PROBE_RESULTS.md) (Experiment 0)

---

## 1. Objective

Validate the GPU rendering path:

**synthetic NV12 plane data → WGSL YUV→RGB shader → GPU-scaled RGB → window presentation**

This experiment does **not** use FFmpeg, D3D11VA, CPU RGBA conversion, `av_hwframe_transfer_data()`, or swscale. It does **not** validate D3D11VA → wgpu import. The term "zero-copy" is not used here.

Success criteria:

1. Create NV12-compatible plane textures on the GPU.
2. Bind Y and UV planes to a WGSL shader.
3. Convert YUV to RGB entirely on the GPU (BT.709 limited range).
4. Scale 1920×1080 source to 1280×720 window via GPU sampling.
5. Present through an RGBA8/BGRA8 swapchain.
6. Run on both DX12 and Vulkan backends.

---

## 2. Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│ CPU (initialization only)                                         │
│  generate_test_pattern() → Y plane bytes + UV plane bytes       │
│  (BT.709 limited-range YUV constants — no RGB conversion)       │
└────────────────────────────┬────────────────────────────────────┘
                             │ queue.write_texture()
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ GPU textures                                                    │
│  NV12 probe texture (Plane0=R8, Plane1=Rg8 views — no upload)   │
│  R8Unorm Y plane (1920×1080)                                    │
│  Rg8Unorm UV plane (960×540)                                    │
└────────────────────────────┬────────────────────────────────────┘
                             │ bind group (Y, UV, linear sampler)
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ WGSL fragment shader                                            │
│  textureSample(y_plane) + textureSample(uv_plane)               │
│  bt709_limited_yuv_to_rgb()                                     │
└────────────────────────────┬────────────────────────────────────┘
                             │ fullscreen triangle, linear filter
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│ Swapchain render target                                         │
│  Bgra8UnormSrgb (1280×720 window)                              │
└─────────────────────────────────────────────────────────────────┘
```

**Components:**

| File | Role |
|------|------|
| `tests/gpu_nv12/src/main.rs` | winit event loop, `--backend` / `DVS_GPU_BACKEND`, `--frames` auto-exit |
| `tests/gpu_nv12/src/nv12_pattern.rs` | Synthetic Y/UV test pattern generator |
| `tests/gpu_nv12/src/renderer.rs` | wgpu device, textures, pipeline, metrics |
| `tests/gpu_nv12/shaders/nv12_to_rgb.wgsl` | BT.709 limited-range YUV→RGB conversion |

**NV12 upload workaround:** wgpu 27 on this hardware rejects `COPY_DST` on `TextureFormat::NV12` for both DX12 and Vulkan. The experiment still creates an NV12 texture with Y/UV plane views (validating Experiment 0 capability), but uploads test data to separate `R8Unorm` / `Rg8Unorm` textures that match the shader's sampling model. The shader path is identical to sampling NV12 plane views.

---

## 3. Test pattern

Source resolution: **1920×1080 NV12** (3 rows × 4 columns of macro-patches).

| Row | Col 0 | Col 1 | Col 2 | Col 3 |
|-----|-------|-------|-------|-------|
| 0 | White | Yellow | Cyan | Green |
| 1 | Magenta | Red | Blue | Skin tone |
| 2 | Black | Gray (Y=64) | Gray (Y=128) | Gray (Y=192) + horizontal luma gradient |

**Special regions:**

- **Bottom-left (row 2, col 0):** horizontal U chroma sweep (V fixed at 128).
- **Bottom-right (row 2, col 3):** horizontal luma gradient (Y 16→235, neutral chroma).

All patch colors are defined as **precomputed BT.709 limited-range YUV triples** on the CPU. No RGB values are computed on the CPU. Visual correctness of the conversion is immediately obvious from the color bars, grayscale steps, skin-tone patch, and gradients.

---

## 4. Shader design

**File:** `tests/gpu_nv12/shaders/nv12_to_rgb.wgsl`

**Inputs:**

- `@binding(0)` — Y plane (`texture_2d<f32>`, R8 normalized)
- `@binding(1)` — UV plane (`texture_2d<f32>`, Rg8 normalized)
- `@binding(2)` — linear sampler (GPU scaling)

**Matrix / range assumptions:**

- **Color space:** ITU-R BT.709 (HD/UHD)
- **Range:** 8-bit limited (studio swing)
  - Y ∈ [16/255, 235/255]
  - UV centered at 0.5 (128/255)
- **Conversion:** Standard limited-range matrix:

```
y_adj = Y - 16/255
u_adj = U - 0.5
v_adj = V - 0.5

R = 1.164383 * y_adj + 1.792741 * v_adj
G = 1.164383 * y_adj - 0.213249 * u_adj - 0.532909 * v_adj
B = 1.164383 * y_adj + 2.112402 * u_adj
```

- **Gamma:** Not applied in shader (linear matrix on normalized values; sRGB output via `Bgra8UnormSrgb` swapchain format).

**Geometry:** Single fullscreen triangle (no vertex buffer), UV coords span beyond [0,1] for edge-to-edge coverage with linear filtering.

---

## 5. Backend configuration

**Run commands:**

```bash
cargo run --release -p gpu-nv12 -- --backend dx12
cargo run --release -p gpu-nv12 -- --backend vulkan
cargo run --release -p gpu-nv12 -- --backend dx12 --frames 120   # auto-exit after N frames
```

**Environment variable:** `DVS_GPU_BACKEND=dx12` or `vulkan` (CLI `--backend` takes precedence).

**Backend selection:** `wgpu::Backends::DX12` or `wgpu::Backends::VULKAN` via `InstanceDescriptor.backends`.

**Required feature:** `Features::TEXTURE_FORMAT_NV12`

**Presentation:** `PresentMode::Fifo` (vsync), surface format `Bgra8UnormSrgb`.

---

## 6. Results on DX12

**Adapter:** AMD Radeon (TM) RX 580  
**Driver:** 31.0.21923.11000  
**Surface format:** `Bgra8UnormSrgb`  
**NV12 upload mode:** NV12 texture created (probe); data uploaded to planar R8/Rg8 — wgpu rejects COPY_DST on NV12

### Initialization (measured)

| Metric | Value |
|--------|-------|
| Total initialization | **213.6 ms** |
| Pattern generation (Y/UV only) | 3.1 ms |
| Texture create + upload | 0.7 ms |
| Pipeline + shader compile | 13.1 ms |
| CPU YUV→RGB conversion | **0.0 ms** |

### Runtime (120 frames, measured wall-clock)

| Metric | Value |
|--------|-------|
| Frames presented | 120 |
| Present interval (avg) | **16.46 ms** (~60.8 FPS) |
| Present interval (last) | 16.76 ms |
| Queue submit (avg) | **16.09 ms** (CPU encode + submit only) |
| Queue submit (last) | 16.41 ms |
| GPU execution time | **NOT MEASURED** |

### Validation

- NV12 texture creation (probe): OK
- NV12 Y/UV plane views: OK
- R8/Rg8 display textures + upload: OK
- Bind group: OK
- Shader compilation: OK
- Render pipeline: OK
- Presentation: OK (120 frames, no errors)
- CPU YUV→RGB: NOT PERFORMED

---

## 7. Results on Vulkan

**Adapter:** AMD Radeon (TM) RX 580  
**Driver:** AMD proprietary driver 25.8.1  
**Surface format:** `Bgra8UnormSrgb`  
**NV12 upload mode:** Same workaround as DX12

### Initialization (measured)

| Metric | Value |
|--------|-------|
| Total initialization | **117.6 ms** |
| Pattern generation (Y/UV only) | 2.6 ms |
| Texture create + upload | 1.4 ms |
| Pipeline + shader compile | 1.2 ms |
| CPU YUV→RGB conversion | **0.0 ms** |

### Runtime (120 frames, measured wall-clock)

| Metric | Value |
|--------|-------|
| Frames presented | 120 |
| Present interval (avg) | **16.26 ms** (~61.5 FPS) |
| Present interval (last) | 15.44 ms |
| Queue submit (avg) | **16.04 ms** (CPU encode + submit only) |
| Queue submit (last) | 15.21 ms |
| GPU execution time | **NOT MEASURED** |

### Validation

Same as DX12 — all checks passed, 120 frames presented without error.

---

## 8. Performance observations

1. **CPU YUV→RGB is zero** by design. Pattern generation (~3 ms once) writes Y/U/V bytes only.
2. **Both backends sustain ~60 FPS** at 1280×720 with a 1920×1080 NV12 source, consistent with `PresentMode::Fifo` vsync.
3. **Present interval ≈ queue submit time** on both backends, indicating the measured interval is dominated by vsync wait rather than CPU or GPU work for this trivial shader.
4. **Vulkan initialized faster** than DX12 on this run (117.6 ms vs 213.6 ms), largely due to pipeline compile time difference (1.2 ms vs 13.1 ms). Single-run variance may apply.
5. **GPU execution time was not measured.** No timestamp queries were used. Queue submit time measures CPU-side command encoding and submission, not GPU shader execution.
6. **Scaling is entirely on the GPU** via linear texture sampling in the fragment shader (no CPU resize, no mip chain).

---

## 9. Limitations

1. **No direct NV12 texture upload.** wgpu 27 rejects `COPY_DST` on `TextureFormat::NV12` on AMD RX 580 for both DX12 and Vulkan. Display path uses equivalent R8/Rg8 planar textures.
2. **NV12 probe texture is not sampled.** It validates creation and plane views only; the shader binds the uploaded R8/Rg8 textures.
3. **No screenshot capture implemented.** Visual validation requires running the window interactively. The test pattern layout is deterministic and documented above.
4. **No GPU timestamp queries.** Cannot report actual shader execution time.
5. **Single GPU, single OS tested.** Results are specific to Windows 10 + AMD RX 580 + wgpu 27.
6. **No gamma management.** Shader outputs linear RGB clamped to [0,1]; display gamma relies on sRGB swapchain format.
7. **Does not prove D3D11VA decoder texture import.** That is Experiment 2.

---

## 10. Confirmed

- NV12 texture can be created on GPU (DX12 and Vulkan).
- NV12 Y plane view (`TextureAspect::Plane0`, `R8Unorm`) can be created.
- NV12 UV plane view (`TextureAspect::Plane1`, `Rg8Unorm`) can be created.
- Separate R8/Rg8 planar textures can be uploaded and bound.
- WGSL BT.709 limited-range YUV→RGB shader compiles and runs on both backends.
- Bind group, render pipeline, and presentation succeed.
- GPU linear scaling from 1920×1080 to 1280×720 works.
- `Bgra8UnormSrgb` swapchain presentation works.
- CPU performs **no** YUV→RGB conversion (0 ms).
- Backend selection via `--backend` and `DVS_GPU_BACKEND` works.
- ~60 FPS sustained with vsync on both backends.

---

## 11. Unconfirmed

- Whether NV12 textures with `COPY_DST` can be uploaded on other GPUs or future wgpu versions.
- Whether sampling directly from an NV12 multi-planar texture (instead of separate R8/Rg8 uploads) produces identical results on this hardware (probe texture was not bound to the shader).
- Actual GPU shader execution time (no timestamp queries).
- Visual correctness on non-sRGB displays or with HDR output.
- D3D11VA / FFmpeg hardware frame import into wgpu (explicitly out of scope).
- P010 10-bit rendering path (Experiment 0 confirmed format support; not tested here).

---

## 12. Next experiment

**Experiment 2 — D3D11VA shared-handle import into wgpu (DX12)**

Goal: Decode a real video frame with D3D11VA, obtain the NV12 D3D11 texture, import it into wgpu via `ExternalTexture` / shared HANDLE, and render through the same YUV→RGB shader validated here.

This is the critical interop step that Experiment 1 intentionally does not cover.

---

## Reproduce

```bash
# From repository root
cargo test -p gpu-nv12
cargo run --release -p gpu-nv12 -- --backend dx12
cargo run --release -p gpu-nv12 -- --backend vulkan
```

Press **Escape** or close the window to print runtime metrics. Use `--frames 120` for unattended runs.
