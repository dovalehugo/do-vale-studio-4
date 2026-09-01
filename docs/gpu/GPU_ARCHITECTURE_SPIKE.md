# Do Vale Studio 4 — GPU Architecture Spike

**Date:** 2026-09-01  
**Phase:** Technical investigation (pre-implementation)  
**Primary target:** Windows  
**Status:** Documentation and analysis only. No FFmpeg, no playback, no UI, no timeline.

---

## 1. Problem

### The Studio 3 bottleneck

Do Vale Studio 3 (and most prototype NLE/video players built on FFmpeg + a general-purpose GPU API) typically converged on this path:

```text
Hardware decoder (D3D11VA / DXVA2)
    ↓
av_hwframe_transfer_data()     ← forced GPU → CPU download
    ↓
CPU buffer (NV12 or RGBA)
    ↓
swscale / CPU color conversion
    ↓
CPU RGBA upload to GPU texture
    ↓
GPU compositor / UI texture
    ↓
Display
```

This pattern is the default in FFmpeg tutorials and many Rust media examples because it is the **easiest** integration path, not the **fastest**.

### Why it fails at professional scale

| Symptom | Root cause |
|---------|------------|
| 4K playback stutters at 30 FPS | ~33 MB/frame CPU readback + upload at 4K RGBA (3840×2160×4 ≈ 33 MB) per frame |
| High CPU usage during playback | Color conversion and memory copies on CPU cores |
| Scrubbing latency | Each seek triggers decode + full CPU transfer |
| Multi-track impossible | Per-track CPU copies multiply linearly |
| GPU idle while CPU saturated | Decoder outputs GPU memory; application immediately pulls it to CPU |

At 4K 60 FPS the sustained bandwidth requirement for a CPU RGBA round-trip is approximately **2 GB/s** of memory traffic **before** any compositing, effects, or UI work. This exceeds the practical budget on most workstations once decode, scheduling, and OS overhead are included.

### Studio 4 objective

Keep decoded video frames **GPU-resident** from hardware decode through color conversion, scaling, compositing, and presentation. CPU involvement should be limited to:

- Packet demuxing
- Decode scheduling
- Command buffer submission
- Frame metadata

CPU must **not** be on the hot path for pixel data.

---

## 2. Requirements

### Functional requirements

| ID | Requirement |
|----|-------------|
| FR-1 | Display hardware-decoded video frames without `av_hwframe_transfer_data()` on the primary path |
| FR-2 | Support NV12 and P010 (HDR) decoded formats on GPU |
| FR-3 | GPU-side YUV → RGB conversion |
| FR-4 | GPU-side scaling (source resolution → viewport resolution) |
| FR-5 | Architecture must later support macOS VideoToolbox + Metal without redesigning `VideoFrame` |
| FR-6 | egui renders application chrome; video renders in a dedicated GPU viewport |
| FR-7 | Software-decode CPU fallback must exist but must be a **degraded mode**, not the default |

### Non-functional requirements (measurable)

| ID | Requirement | Gate |
|----|-------------|------|
| NFR-1 | 4K HEVC 30 FPS sequential playback | 0 dropped frames, per `PERFORMANCE.md` |
| NFR-2 | 4K HEVC 60 FPS sequential playback | Stable when HW decode + GPU render available |
| NFR-3 | No mandatory GPU → CPU → GPU pixel path | Verified by profiling (no `Map`/`Unmap` on decode textures per frame) |
| NFR-4 | Frame budget compliance | See Section 10 |
| NFR-5 | Interop path documented with copy classification | Every stage labelled: zero-copy, GPU-copy, or CPU-copy |
| NFR-6 | Multi-GPU safe | Never assume a specific GPU exists; adapter selection explicit |

### Terminology (strict)

| Term | Meaning in this document |
|------|--------------------------|
| **Zero-copy** | Pixel data never leaves GPU memory. No `av_hwframe_transfer_data`, no `ID3D11DeviceContext::Map` for display, no CPU staging buffer on the hot path. |
| **GPU-copy** | `CopySubresourceRegion`, `copy_texture_to_texture`, or cross-queue GPU transfer. Acceptable if bounded and measured. |
| **CPU-copy** | Any path involving CPU-readable memory for pixel data. Fallback only. |

---

## 3. Candidate Architectures

### A. D3D11VA → CPU RGBA → wgpu

```text
FFmpeg D3D11VA
    → av_hwframe_transfer_data()
    → CPU NV12/RGBA
    → upload to wgpu::Texture
    → egui/wgpu render
```

| Dimension | Assessment |
|-----------|------------|
| CPU copies | **1–2 per frame** (GPU→CPU download + CPU→GPU upload) |
| GPU copies | 0 (but CPU path dominates) |
| Synchronization | Simple |
| Complexity | Low |
| Portability | High (same pattern on all platforms) |
| Performance potential | **Poor** — does not meet Studio 4 requirements |
| Implementation risk | Low |

**Verdict:** Reject as primary architecture. Acceptable only as explicit software-decode fallback when no GPU interop is available.

---

### B. D3D11VA → D3D11 GPU texture → GPU renderer (wgpu via shared handle)

```text
FFmpeg D3D11VA
    → ID3D11Texture2D (NV12 / DXGI_FORMAT_420_OPAQUE)
    → [optional GPU CopySubresourceRegion to shared pool texture]
    → DXGI NT shared handle
    → wgpu import (DX12 OpenSharedHandle or Vulkan external memory)
    → GPU shader: YUV → RGB
    → GPU shader: scale
    → wgpu swapchain present
```

| Dimension | Assessment |
|-----------|------------|
| CPU copies | **0 on hot path** (if import succeeds) |
| GPU copies | 0–1 (decoder pool → shared import texture, if formats/flags require it) |
| Synchronization | D3D11/D3D12 fence or keyed mutex between decode and render queues |
| Complexity | Medium–high |
| Portability | Windows-first; pattern mirrors macOS IOSurface path |
| Performance potential | **High** — meets Studio 4 targets |
| Implementation risk | Medium — depends on wgpu backend and driver support |

**Verdict:** **Recommended primary architecture** for Windows, pending experimental validation.

**Evidence (external, not yet validated in this repo):**

- wgpu-hal Vulkan backend exposes `texture_from_d3d11_shared_handle` (wgpu PR #6161).
- Community crates (`vtsampler`, `wgpu-native-texture-interop`, `grafting`) demonstrate D3D11 `SHARED` / `SHARED_NTHANDLE` → wgpu DX12 `OpenSharedHandle` → `create_texture_from_hal`.
- FFmpeg `AV_PIX_FMT_D3D11` stores `ID3D11Texture2D*` in `AVFrame.data[0]` without requiring `av_hwframe_transfer_data`.

---

### C. D3D11VA → D3D12 interoperability → GPU renderer

```text
FFmpeg D3D11VA
    → ID3D11Texture2D
    → DXGI shared handle
    → ID3D12Resource (OpenSharedHandle on wgpu's D3D12 device)
    → wgpu DX12 backend
    → GPU processing → present
```

This is a refinement of B where the wgpu backend is explicitly **DX12** and import happens at the D3D12 resource level.

| Dimension | Assessment |
|-----------|------------|
| CPU copies | 0 on hot path |
| GPU copies | 0–1 (same as B) |
| Synchronization | `ID3D11Fence` / `ID3D12Fence` shared handles (preferred over keyed mutex for video) |
| Complexity | Medium–high |
| Portability | Windows only |
| Performance potential | **High** |
| Implementation risk | Medium |

**Verdict:** **Recommended wgpu backend on Windows for the video interop path.** DX12 `OpenSharedHandle` is the most documented interop route for D3D11 shared textures into wgpu.

**UNVALIDATED:** Whether wgpu's DX12 backend correctly handles multi-planar NV12 imported resources for shader sampling. wgpu recently added multi-planar format support (`TextureFormat::NV12`, `Features::TEXTURE_FORMAT_NV12`) and DX12 plane-aware copy fixes (wgpu PR #9551), but import of external NV12 textures has not been tested in this project.

---

### D. Native graphics API renderer instead of wgpu

```text
FFmpeg D3D11VA
    → ID3D11Texture2D
    → D3D11/D3D12 compositor (no wgpu)
    → DXGI swapchain present
```

| Dimension | Assessment |
|-----------|------------|
| CPU copies | 0 |
| GPU copies | Minimal (same-device D3D11 shader sampling of decode texture) |
| Synchronization | Simplest — single API family |
| Complexity | High long-term (separate Windows and macOS renderers) |
| Portability | **Poor** — requires Metal renderer for macOS |
| Performance potential | **Highest on Windows** |
| Implementation risk | High — duplicates entire render graph per platform |

**Verdict:** Reject as primary architecture. The performance gain over B is marginal once B achieves GPU-resident import, but the maintenance cost of two native renderers is prohibitive for a cross-platform NLE.

**Possible compromise:** Use native D3D11 **only for the Phase 1 interop spike** to validate decode texture properties independently of wgpu complexity. Do not adopt as production architecture.

---

### E. Other alternatives

#### E1. Vulkan Video decode → Vulkan image → wgpu Vulkan backend

| Dimension | Assessment |
|-----------|------------|
| CPU copies | 0 (in theory) |
| GPU copies | 0 |
| Complexity | Very high — Vulkan Video is immature in FFmpeg Rust ecosystem |
| Portability | Cross-platform potential |
| Performance potential | High |
| Implementation risk | **Very high** |

**Verdict:** Defer. Revisit if D3D11VA → wgpu path fails.

#### E2. NVDEC native SDK → CUDA/D3D interop → wgpu

| Dimension | Assessment |
|-----------|------------|
| CPU copies | 0 |
| Complexity | High; NVIDIA-only |
| Portability | Poor |
| Performance potential | High on NVIDIA hardware |
| Implementation risk | Medium–high |

**Verdict:** Optional Phase 2+ optimization for NVIDIA-specific export/playback. Not for initial architecture.

#### E3. Decode to DMA-BUF / shared handle via Media Foundation

Windows Media Foundation can produce D3D11 textures via hardware transforms. Adds a second media stack alongside FFmpeg.

**Verdict:** Reject. Studio 4 standardizes on FFmpeg for decode.

#### E4. CPU decode + GPU upload (no HW accel)

**Verdict:** Software fallback only.

---

### Architecture comparison summary

| Option | CPU copies/frame | GPU copies | Meets 4K60 | Portability | Risk | Recommendation |
|--------|------------------|------------|------------|-------------|------|----------------|
| A. CPU RGBA | 2 | 0 | No | High | Low | Fallback only |
| B. D3D11 → wgpu | 0 | 0–1 | **Likely** | Medium | Medium | **Primary** |
| C. D3D11 → D3D12 → wgpu | 0 | 0–1 | **Likely** | Windows | Medium | **Primary (DX12 backend)** |
| D. Native D3D11 renderer | 0 | 0 | Yes | Low | High | Spike only |
| E1. Vulkan Video | 0 | 0 | Unknown | Medium | Very high | Defer |

---

## 4. wgpu Analysis

### What wgpu can do (confirmed from public API and wgpu source, UNVALIDATED in this repo)

| Capability | Status | Notes |
|------------|--------|-------|
| Multi-planar NV12 / P010 formats | **Available** | `TextureFormat::NV12`, `TextureFormat::P010`; requires `Features::TEXTURE_FORMAT_NV12` / `TEXTURE_FORMAT_P010` |
| Plane-specific texture views | **Available** | `TextureAspect::Plane0` (Y), `TextureAspect::Plane1` (UV) |
| Import D3D11 shared handle (Vulkan backend) | **Available (unsafe HAL)** | `wgpu_hal::vulkan::Device::texture_from_d3d11_shared_handle` |
| Import via D3D12 `OpenSharedHandle` (DX12 backend) | **Available (unsafe HAL)** | `wgpu_hal::dx12::Device::texture_from_raw` after `OpenSharedHandle` |
| Wrap external resource as `wgpu::Texture` | **Available (unsafe)** | `Device::create_texture_from_hal` with explicit `initial_state: TextureUses` |
| `ExternalTexture` type | **Partial** | Exists; primarily WebGPU-oriented; native import via HAL |
| YUV → RGB in shader | **Available** | Standard WGSL fragment shader sampling plane views |
| GPU scaling | **Available** | Render pass to sized render target |
| Public stable import API | **No** | Import requires `as_hal()` + `create_texture_from_hal` — unsafe, `wgpu_core` only |

### What wgpu cannot do (or does poorly)

| Limitation | Impact |
|------------|--------|
| No stable public API for external texture import | `dvs-gpu` must wrap unsafe HAL calls; breakage risk on wgpu upgrades |
| No guaranteed cross-backend import | D3D11 handle import requires Vulkan or DX12 backend; GL backend unusable for video |
| Multi-planar external import UNVALIDATED | NV12 imported from D3D11 may not map cleanly to `TextureFormat::NV12` without experiments |
| `DXGI_FORMAT_420_OPAQUE` handling unknown | FFmpeg may output opaque video format; shader sampling requirements UNVALIDATED |
| egui integration with external swapchain | egui owns its own wgpu surface; video viewport requires separate surface — UNVALIDATED |
| Zero-initialization of imported textures | Must pass correct `TextureUses` initial state or risk undefined behavior (wgpu PR #9496) |
| WebGPU `importExternalTexture` | Browser only; not applicable to native |

### Backend selection on Windows

| Backend | D3D11 interop | NV12 support | Maturity | Recommendation |
|---------|---------------|--------------|--------|----------------|
| **DX12** | `OpenSharedHandle` from NT handle — best documented | Multi-planar fixes landing (PR #9551) | High | **Primary candidate** |
| **Vulkan** | `texture_from_d3d11_shared_handle` via `VK_KHR_external_memory_win32` | Depends on driver | Medium | **Secondary candidate** — test both |
| **GL** | No viable D3D11 interop | No | N/A | **Exclude** from video path |

**UNVALIDATED:** Head-to-head benchmark of DX12 vs Vulkan import path on target hardware (NVIDIA, AMD, Intel).

### wgpu and the "zero-copy" claim

Importing a D3D11 shared texture into wgpu via `OpenSharedHandle` is **not a CPU copy**. It is a GPU resource alias across D3D11/D3D12 APIs. However:

1. It is **not free** — driver overhead, fence waits, and layout transitions have cost.
2. A **GPU copy** (`CopySubresourceRegion`) may still be required if the decoder texture lacks `D3D11_RESOURCE_MISC_SHARED` flags or `D3D11_BIND_SHADER_RESOURCE`.
3. **YUV → RGB conversion** is a mandatory GPU render pass, not optional.
4. Therefore the accurate claim is: **"GPU-resident pipeline with no CPU pixel transfer"**, not "zero-copy" in the absolute sense.

---

## 5. Windows Strategy

### Recommended architecture

```text
┌─────────────────────────────────────────────────────────────────┐
│                        dvs-decoder                              │
│  FFmpeg D3D11VA → AV_PIX_FMT_D3D11 → ID3D11Texture2D + index  │
└────────────────────────────┬────────────────────────────────────┘
                             │ VideoFrame::D3D11 (opaque handle)
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                         dvs-gpu                                 │
│  Platform interop (cfg windows):                                │
│    1. Ensure texture has SHARED + SHADER_RESOURCE flags         │
│    2. [If needed] GPU CopySubresourceRegion → pool texture      │
│    3. CreateSharedHandle (NT handle)                            │
│    4. wgpu DX12: OpenSharedHandle → create_texture_from_hal     │
│    5. Fence sync (D3D11Fence ↔ D3D12Fence)                    │
└────────────────────────────┬────────────────────────────────────┘
                             │ wgpu::Texture (NV12 or plane views)
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                        dvs-render                               │
│  Render graph pass 1: YUV → RGBA (WGSL shader, BT.709/BT.2020) │
│  Render graph pass 2: Scale (source → viewport)                │
│  Render graph pass 3: [future] Composite layers                │
└────────────────────────────┬────────────────────────────────────┘
                             │
                             ▼
┌─────────────────────────────────────────────────────────────────┐
│                     Presentation                                │
│  wgpu swapchain on child viewport window (not egui texture)     │
└─────────────────────────────────────────────────────────────────┘
```

### Device and adapter strategy

1. **Enumerate adapters** via wgpu; select the decode/render adapter (prefer discrete GPU with video decode support).
2. **Create wgpu `Device`** on DX12 backend with `Features::TEXTURE_FORMAT_NV12`.
3. **Create FFmpeg `AV_HWDEVICE_TYPE_D3D11VA`** using the D3D11 device associated with the same adapter.
   - **UNVALIDATED:** Whether wgpu exposes its underlying D3D11/D3D12 device for FFmpeg to use directly, or whether a separately created D3D11 device on the same adapter is sufficient for shared handle interop.
4. Configure FFmpeg frames context with:
   - `BindFlags = D3D11_BIND_DECODER | D3D11_BIND_SHADER_RESOURCE`
   - `MiscFlags` including `D3D11_RESOURCE_MISC_SHARED_NTHANDLE` where supported

### Texture format handling

| Decode format | DXGI format | wgpu handling |
|---------------|-------------|---------------|
| 8-bit 4:2:0 | `DXGI_FORMAT_NV12` | `TextureFormat::NV12` + plane views |
| 10-bit 4:2:0 | `DXGI_FORMAT_P010` | `TextureFormat::P010` + plane views |
| Opaque 4:2:0 | `DXGI_FORMAT_420_OPAQUE` | **UNVALIDATED** — may need `VideoProcessorBlt` or format conversion pass on D3D11 before import |

### Fallback hierarchy

```text
1. D3D11VA GPU-resident → wgpu import → GPU YUV→RGB     (primary)
2. D3D11VA GPU-resident → D3D11 GPU copy → wgpu import  (degraded: extra GPU copy)
3. D3D11VA → CPU NV12 → GPU upload of NV12 texture       (degraded: CPU involvement in upload only)
4. Software decode → CPU NV12 → GPU upload                 (last resort)
5. Software decode → CPU RGBA → GPU upload                 (emergency only — path A)
```

Paths 3–5 must be explicitly flagged as degraded modes in capability detection. Path 5 must never be silent.

### Presentation strategy

egui must **not** display video as an `egui::Image` from a CPU-uploaded texture. Recommended approach:

- **Child viewport** (`eframe`/`egui::Viewport`) hosting a separate wgpu `Surface` for video.
- egui renders panels, timeline, transport around the viewport.
- **UNVALIDATED:** Exact `eframe` multi-surface integration pattern.

Alternative (lower priority): Full-window wgpu render with egui as an overlay pass. More complex hit-testing.

---

## 6. macOS Strategy

### Target path (deferred, but influences `VideoFrame` design now)

```text
FFmpeg VideoToolbox (or native VT)
    → CVPixelBuffer (IOSurface-backed)
    → CVMetalTextureCache → MTLTexture per plane
    → wgpu Metal backend: create_texture_from_hal
    → GPU YUV → RGB shader
    → GPU scale → present
```

### Platform comparison

| Stage | Windows | macOS |
|-------|---------|-------|
| HW decode API | D3D11VA (FFmpeg) | VideoToolbox (FFmpeg) |
| GPU texture | `ID3D11Texture2D` | `MTLTexture` via `CVMetalTextureCache` |
| Interop mechanism | DXGI shared NT handle | IOSurface memory alias |
| wgpu backend | DX12 (primary) | Metal |
| wgpu import | `create_texture_from_hal::<Dx12>` | `create_texture_from_hal::<Metal>` |
| Sync | D3D11/D3D12 fence | Metal command buffer ordering / shared command queue |
| Pixel format | NV12 / P010 | `kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange` (NV12-like) |

### Abstraction rule

`VideoFrame` in `dvs-media` carries **opaque platform handles**. Platform resolution lives in `dvs-gpu`:

```text
dvs-gpu/src/
  interop/
    mod.rs          # trait GpuFrameImporter
    windows_d3d11.rs
    macos_metal.rs  # cfg(target_os = "macos")
```

Both platforms converge to `wgpu::Texture` (or plane views) inside `dvs-gpu` before entering `dvs-render`.

**Evidence (external):** `moq-video` and `wgpu-native-texture-interop` demonstrate IOSurface → Metal → wgpu without CPU copy. **UNVALIDATED** in this repo.

---

## 7. VideoFrame Abstraction

### Design principles

1. `VideoFrame` lives in **`dvs-media`** — domain type used by decoder, playback, and render.
2. Platform types do **not** leak into `dvs-core`, `dvs-ui`, or `dvs-playback` beyond the opaque handle.
3. `VideoFrame` carries **metadata**; pixel access requires `dvs-gpu` or `dvs-render`.
4. CPU RGBA is a distinct variant, not the default.

### Conceptual API

```rust
// dvs-media — conceptual, not implemented yet

/// Domain-level decoded frame. Pixel data is never exposed as a CPU buffer
/// on the primary GPU path.
pub struct VideoFrame {
    pub id: FrameId,
    pub timestamp: MediaTimestamp,
    pub duration: MediaDuration,
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,     // Nv12, P010, Rgba8, ...
    pub color_space: ColorSpace,       // Bt709, Bt2020, ...
    pub color_range: ColorRange,       // Limited, Full
    pub storage: FrameStorage,
}

pub enum FrameStorage {
    /// Software-decoded or fallback. CPU memory. Degraded path.
    Cpu(CpuFrameBuffer),

    /// GPU-resident platform frame. Opaque to consumers outside dvs-gpu.
    Gpu(GpuFrameHandle),
}

/// Opaque GPU frame reference. Internals known only to dvs-gpu.
pub struct GpuFrameHandle {
    pub backend: GpuBackend,           // D3D11, Metal, ...
    pub resource_id: GpuResourceId,
    // Platform payload stored behind Arc<dyn GpuFrameResource> in dvs-gpu
}

pub enum PixelFormat {
    Nv12,
    P010,
    Rgba8,
    // future: P016, Yuv420p (CPU only), ...
}
```

### What belongs where

| Type / concern | Crate | Rationale |
|----------------|-------|-----------|
| `VideoFrame`, `PixelFormat`, `ColorSpace` | `dvs-media` | Domain vocabulary |
| `GpuFrameHandle` (opaque) | `dvs-media` | Cross-crate frame carrier |
| `GpuFrameResource` trait impl | `dvs-gpu` | Platform texture access |
| `D3D11FrameResource { texture, index, fence }` | `dvs-gpu` | Windows-specific |
| `MetalFrameResource { cv_texture, pixel_buffer }` | `dvs-gpu` | macOS-specific |
| `import_to_wgpu(handle) → WgpuVideoSurface` | `dvs-gpu` | Interop |
| YUV→RGB, scale shaders | `dvs-render` | Render graph nodes |
| `CpuFrameBuffer` | `dvs-media` | Fallback path data |

### Frame lifetime

```text
Decoder pool → VideoFrame (GpuFrameHandle)
    → Playback scheduler (bounded queue, move semantics)
    → Render graph (borrow for one frame's passes)
    → Return to decoder pool / GPU texture pool
```

`Arc` only when cache and display need the same frame simultaneously.

---

## 8. Render Architecture

### Data flow

```text
┌──────────────┐
│   Decoder    │  FFmpeg D3D11VA (later)
│  dvs-decoder │  Produces VideoFrame { Gpu(D3D11 handle) }
└──────┬───────┘
       │
       ▼
┌──────────────┐
│  VideoFrame  │  dvs-media: metadata + opaque GpuFrameHandle
│  dvs-media   │  No pixel access at this layer
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ RenderGraph  │  dvs-render: declarative pass list
│  dvs-render  │
│              │  Pass 1: ImportNode      — dvs-gpu interop → wgpu::Texture
│              │  Pass 2: ColorConvertNode — YUV → RGBA render target
│              │  Pass 3: ScaleNode        — source res → viewport res
│              │  Pass 4: [future] CompositeNode — multi-layer blend
│              │  Pass 5: OutputNode        — swapchain / export target
└──────┬───────┘
       │
       ▼
┌──────────────┐
│     GPU      │  dvs-gpu: wgpu device, pools, shaders, sync
│   dvs-gpu    │  Executes RenderGraph passes
└──────┬───────┘
       │
       ▼
┌──────────────┐
│ Presentation │  wgpu Surface::present (viewport swapchain)
└──────────────┘
```

### Phase 1 minimal graph (passthrough)

```text
ImportNode → ColorConvertNode → ScaleNode → OutputNode
```

Four render passes. No compositor, no effects, no timeline. Sufficient to validate the full GPU-resident pipeline.

### Phase 4+ graph (multi-track)

```text
ImportNode × N
    → per-layer ColorConvert + Transform
    → CompositeNode (alpha blend)
    → OutputNode
```

Same graph structure serves preview (real-time) and export (offline) with different `OutputNode` targets.

---

## 9. Synchronization

### Resource ownership

| Resource | Owner | Created by | Destroyed by |
|----------|-------|------------|--------------|
| D3D11 decode texture | `dvs-decoder` (FFmpeg pool) | FFmpeg hwframes | FFmpeg pool release |
| Shared import pool texture | `dvs-gpu` | `dvs-gpu` texture pool | Pool eviction |
| wgpu imported texture | `dvs-gpu` | `create_texture_from_hal` | wgpu drop / pool return |
| wgpu RGBA render target | `dvs-gpu` | Texture pool | Pool return |
| Fence objects | `dvs-gpu` | Platform API | Per-session |

### Sync points

```text
Decode complete (D3D11 video context)
    │
    ├─ Signal: D3D11Fence value N
    │
    ▼
Import / GPU copy (if needed)
    │
    ├─ Wait: fence N on D3D12 queue
    ├─ [Optional] CopySubresourceRegion on D3D11 context
    ├─ Signal: D3D12Fence value M
    │
    ▼
wgpu render passes (YUV→RGB, scale)
    │
    ├─ Wait: fence M
    ├─ Execute render passes
    ├─ Signal: D3D12Fence value M+1
    │
    ▼
Present (swapchain)
    │
    └─ Wait: frame latency waitable object / present fence
```

### Rules

1. **Decoder must not block on render** — bounded queue; drop stale frames instead of backpressure to decode thread.
2. **Render must wait for decode** — fence wait before sampling imported texture.
3. **Never sample a texture still being written** — fence values per frame, monotonically increasing.
4. **Same adapter requirement** — D3D11 decode device and wgpu device must be on the same DXGI adapter. Cross-adapter shared handles fail or degrade to CPU copy.
5. ** Keyed mutex vs fence** — prefer `ID3D11Fence`/`ID3D12Fence` shared handles over `IDXGIKeyedMutex` for video; keyed mutex can cause stalls on some drivers.

### Lifetime hazards

| Hazard | Mitigation |
|--------|------------|
| FFmpeg recycles decode texture while render samples it | Reference-count `VideoFrame`; pool return only after fence signals render complete |
| wgpu `create_texture_from_hal` aliasing | Document safety contract; one import wrapper per plane; no concurrent conflicting state transitions |
| Array texture index invalid after seek | Re-validate index on each decoded frame; never cache index across seeks |

---

## 10. Performance Model

### Frame budgets (total per frame)

| Target | Budget | Resolution | Notes |
|--------|--------|------------|-------|
| 4K 30 FPS | **33.3 ms** | 3840×2160 | Phase 1 gate |
| 4K 60 FPS | **16.7 ms** | 3840×2160 | Phase 2 gate |
| 6K 30 FPS | **33.3 ms** | ~6144×3456 | Future gate |
| 6K 60 FPS | **16.7 ms** | ~6144×3456 | Stretch goal |

### Budget allocation (4K 30 FPS — 33.3 ms total)

| Stage | Budget | Cumulative | Measurement |
|-------|--------|------------|-------------|
| Demux + packet decode scheduling | 3.0 ms | 3.0 ms | `tracing` span: `decode.schedule` |
| HW decode (D3D11VA) | 5.0 ms | 8.0 ms | `decode.hw` |
| Interop import (shared handle) | 1.0 ms | 9.0 ms | `gpu.import` |
| Fence wait | 0.5 ms | 9.5 ms | `gpu.sync_wait` |
| GPU YUV → RGB | 3.0 ms | 12.5 ms | `render.color_convert` |
| GPU scale | 2.0 ms | 14.5 ms | `render.scale` |
| GPU composite (future) | 5.0 ms | 19.5 ms | `render.composite` |
| Present | 1.0 ms | 20.5 ms | `gpu.present` |
| **Headroom** | **12.8 ms** | 33.3 ms | OS, UI, audio, scheduler |

### Budget scaling for 4K 60 FPS (16.7 ms)

All GPU stages must halve. Decode may not scale linearly (hardware decode is often faster than 5 ms). Critical tightening:

| Stage | 4K60 budget |
|-------|-------------|
| Interop + sync | ≤ 1.0 ms combined |
| YUV → RGB + scale | ≤ 3.0 ms combined |
| Present | ≤ 0.5 ms |

If YUV → RGB + scale exceeds 3 ms, consider:

- Combined single-pass shader (convert + scale)
- Compute shader instead of render pass
- Render at display resolution directly (skip intermediate RGBA full-res target)

### 6K considerations

6K frame has ~2.25× the pixels of 4K. At 6K 30 FPS (33.3 ms budget), GPU shader stages scale proportionally:

| Stage | 6K30 estimated |
|-------|----------------|
| YUV → RGB | ~6.8 ms (may exceed budget) |
| Scale (if 6K → 1080p preview) | ~1.5 ms (downscale reduces cost) |

**Implication:** For 6K, always render to **viewport resolution**, never to full 6K RGBA intermediate unless exporting.

### Bandwidth reference (why CPU path fails)

| Path | 4K NV12/frame | 4K RGBA/frame | At 60 FPS |
|------|---------------|---------------|-----------|
| CPU RGBA round-trip | — | 33 MB | **~2.0 GB/s** |
| GPU NV12 (resident) | 12 MB | — | **~0.7 GB/s** (memory bus only) |
| GPU RGBA (viewport 1080p) | — | 8 MB | **~0.5 GB/s** |

GPU-resident NV12 + shader to viewport-resolution RGBA reduces bandwidth by roughly **4×** vs full-resolution CPU RGBA round-trip.

---

## 11. Decision

### Primary decision

**Adopt Architecture B/C: D3D11VA GPU-resident frames imported into wgpu (DX12 backend primary) with GPU-side YUV → RGB conversion and GPU-side scaling, presented via a dedicated wgpu viewport separate from egui.**

### Supporting decisions

| # | Decision |
|---|----------|
| D1 | wgpu DX12 backend as primary on Windows for video interop |
| D2 | Vulkan backend as fallback if DX12 import fails on specific hardware |
| D3 | `VideoFrame` in `dvs-media` with opaque `GpuFrameHandle` |
| D4 | Platform interop in `dvs-gpu`; render passes in `dvs-render` |
| D5 | NV12/P010 processed via wgpu multi-planar formats + WGSL shader |
| D6 | CPU RGBA path exists as explicit degraded fallback only |
| D7 | Video presentation in child viewport, not egui texture |
| D8 | macOS path: IOSurface → Metal → wgpu (deferred, same `VideoFrame` API) |

### What we are NOT deciding yet

- Exact FFmpeg device sharing mechanism (UNVALIDATED)
- Whether `DXGI_FORMAT_420_OPAQUE` can be imported or needs D3D11 conversion (UNVALIDATED)
- egui child viewport integration pattern (UNVALIDATED)
- DX12 vs Vulkan performance on target GPUs (UNVALIDATED)

---

## 12. Spike Implementation Plan

Minimal isolated experiments to validate the recommendation. Each experiment is a **standalone binary** in `crates/` or `experiments/` — not integrated into the application.

### Experiment 0: Adapter enumeration

| Field | Value |
|-------|-------|
| **Goal** | List wgpu adapters, backends, and `TEXTURE_FORMAT_NV12` support |
| **Dependencies** | `wgpu` only |
| **Validates** | Backend availability, feature flags |
| **Success** | At least one adapter reports DX12 + `TEXTURE_FORMAT_NV12` |
| **Status** | UNVALIDATED |

### Experiment 1: wgpu DX12 basic present

| Field | Value |
|-------|-------|
| **Goal** | Create wgpu DX12 device, render solid color to swapchain |
| **Dependencies** | `wgpu`, `winit` |
| **Validates** | Baseline wgpu DX12 rendering works on target hardware |
| **Success** | Stable 60 FPS present loop |
| **Status** | UNVALIDATED |

### Experiment 2: D3D11 shared texture → wgpu import (no decode)

| Field | Value |
|-------|-------|
| **Goal** | Create D3D11 NV12 texture with `SHARED_NTHANDLE`, fill with test pattern via D3D11, import into wgpu, sample in shader |
| **Dependencies** | `wgpu`, `windows` crate |
| **Validates** | Core interop path without FFmpeg |
| **Success** | Correct test pattern displayed; no CPU `Map` on hot path |
| **Status** | UNVALIDATED |

### Experiment 3: GPU YUV → RGB shader

| Field | Value |
|-------|-------|
| **Goal** | Given imported NV12 wgpu texture, render correct RGB output (BT.709 test vectors) |
| **Dependencies** | `wgpu` |
| **Validates** | Shader correctness, multi-planar sampling, performance |
| **Success** | PSNR within tolerance vs reference; < 3 ms at 4K on target GPU |
| **Status** | UNVALIDATED |

### Experiment 4: D3D11 → D3D12 fence synchronization

| Field | Value |
|-------|-------|
| **Goal** | D3D11 producer signals fence; wgpu D3D12 consumer waits before sampling |
| **Dependencies** | `wgpu`, `windows` |
| **Validates** | Cross-API sync without tearing |
| **Success** | No visual corruption under rapid update |
| **Status** | UNVALIDATED |

### Experiment 5: FFmpeg D3D11VA decode to GPU texture (minimal)

| Field | Value |
|-------|-------|
| **Goal** | Decode single HEVC 4K file to `AV_PIX_FMT_D3D11`; log texture format, flags, dimensions; **do not** call `av_hwframe_transfer_data` |
| **Dependencies** | `ffmpeg-next`, `windows` |
| **Validates** | FFmpeg outputs GPU texture with expected flags |
| **Success** | `ID3D11Texture2D` obtained; `BindFlags` includes `SHADER_RESOURCE` or documents fallback |
| **Status** | UNVALIDATED |

### Experiment 6: End-to-end decode → wgpu display

| Field | Value |
|-------|-------|
| **Goal** | Connect Experiment 5 output to Experiment 2 import path; display decoded frame |
| **Dependencies** | Experiments 2, 3, 5 |
| **Validates** | Full pipeline without CPU pixel transfer |
| **Success** | 4K HEVC frame displayed; profiling confirms no CPU readback |
| **Status** | UNVALIDATED |

### Experiment 7: DX12 vs Vulkan backend comparison

| Field | Value |
|-------|-------|
| **Goal** | Run Experiment 6 on both backends; compare import time, render time, stability |
| **Dependencies** | Experiment 6 |
| **Validates** | Backend selection decision D2 |
| **Success** | Data table with per-stage timings on NVIDIA, AMD, Intel |
| **Status** | UNVALIDATED |

### Experiment 8: GPU scale performance

| Field | Value |
|-------|-------|
| **Goal** | Scale 4K → 1080p viewport in single pass (combined with YUV→RGB or separate) |
| **Dependencies** | Experiment 3 |
| **Validates** | Scale fits in budget |
| **Success** | Combined pass < 4 ms at 4K→1080p |
| **Status** | UNVALIDATED |

### Experiment ordering

```text
Exp 0 → Exp 1 → Exp 2 → Exp 3 → Exp 4
                              ↓
                    Exp 5 → Exp 6 → Exp 7
                              ↓
                           Exp 8
```

Experiments 0–4 require **no FFmpeg**. Experiment 5 is the first FFmpeg touchpoint. Experiment 6 is the Phase 1 success criterion.

### What each experiment is NOT

- Not the video editor
- Not playback scheduling
- Not timeline
- Not egui integration (except optionally in Experiment 1 as a window host)
- Not a permanent crate in the workspace (use `experiments/` directory or temporary bins)

---

## Summary

### CONFIRMED

- Studio 4's GPU-first architecture direction is correct and necessary for 4K+ workflows.
- FFmpeg D3D11VA can produce GPU-resident `ID3D11Texture2D` frames (`AV_PIX_FMT_D3D11`) without `av_hwframe_transfer_data`.
- wgpu supports NV12/P010 multi-planar formats and plane-specific views.
- wgpu HAL provides unsafe mechanisms to import D3D11 shared textures (DX12 `OpenSharedHandle` and Vulkan `texture_from_d3d11_shared_handle`).
- GPU YUV → RGB conversion via WGSL shader is a well-understood, low-risk approach.
- CPU RGBA round-trip is incompatible with 4K 60 FPS targets and must not be the primary path.
- `VideoFrame` should live in `dvs-media` as an opaque GPU handle carrier; interop belongs in `dvs-gpu`.

### UNCONFIRMED

- D3D11VA decode textures can be imported into wgpu as NV12 without a CPU fallback on **our target hardware**.
- `DXGI_FORMAT_420_OPAQUE` (FFmpeg default for some paths) can be sampled or converted on GPU without CPU readback.
- wgpu DX12 vs Vulkan import performance and stability on NVIDIA, AMD, and Intel GPUs.
- FFmpeg and wgpu can share the same D3D11/D3D12 device (or if separate same-adapter devices suffice).
- egui child viewport + separate wgpu video surface integration pattern.
- Fence-based sync between FFmpeg decode and wgpu render is sufficient without keyed mutex stalls.
- Combined YUV→RGB + scale meets 4K 60 FPS budget (16.7 ms).

### RISKS

| Risk | Severity | Mitigation |
|------|----------|------------|
| wgpu external import API is unsafe and unstable | High | Wrap in `dvs-gpu` platform module; pin wgpu version; abstract behind trait |
| `420_OPAQUE` format incompatibility | High | Experiment 5; D3D11 `VideoProcessorBlt` GPU conversion if needed |
| Driver-specific interop failures | Medium | Vulkan fallback backend; degraded path hierarchy |
| egui + video viewport integration | High | Early Experiment 1 with `winit` multi-window; defer egui to Phase 2 |
| Multi-planar NV12 import into wgpu | Medium | Experiment 2; plane-split import as fallback |
| wgpu upgrade breaks HAL interop | Medium | Pin version; CI test Experiments 2+3 on upgrade |

### RECOMMENDATION

Proceed with **Architecture B/C** (D3D11VA GPU-resident → wgpu DX12 import → GPU YUV→RGB → GPU scale → viewport present). Do not implement the media engine, playback, or UI until **Experiments 0–4** pass. FFmpeg integration begins only at **Experiment 5**, after the interop path is proven without decode.

Use precise language: **"GPU-resident pipeline"**, not "zero-copy", unless a specific stage is proven to have no GPU or CPU copy.

### NEXT EXPERIMENT

**Experiment 0: Adapter enumeration** — add a minimal `experiments/gpu-spike-adapter` binary that:

1. Creates wgpu `Instance` with backends `DX12 | VULKAN`.
2. Enumerates all adapters with name, backend, device type.
3. Creates device and queries `Features::TEXTURE_FORMAT_NV12` / `TEXTURE_FORMAT_P010`.
4. Prints results to stdout.

No FFmpeg. No decode. No render graph. Takes < 100 lines. Validates foundation for all subsequent experiments.
