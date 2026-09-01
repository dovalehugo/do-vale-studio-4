# Do Vale Studio 4 — GPU Production Integration Plan

**Date:** 2026-09-01  
**Baseline commit:** `a5fdb42b6436c0f23f3960b3cdc6ed94d98e7b5d`  
**Status:** Integrations 0–2, 3A, 3B, 3C, **4**, and **5** complete. Integration 3 interop bridge is **complete**: shared-resource import and two-cycle bidirectional fence synchronization are **hardware-validated**. Integration 4 provides real FFmpeg D3D11VA decode and production interop bridge wiring. Integration 5 provides production NV12 WGSL sampling/rendering with automated and human visual validation **complete**.
**Evidence:** GPU Experiments 0–2 PASS (`docs/gpu/GPU_EXPERIMENT_2.md`, `DOVALE_STUDIO_4_HANDOFF_EXPERIMENT_2.md`)

This document transfers the validated experiment pipeline into production crates. It defines proposed APIs, ownership, threading, initialization, unsafe invariants, and staged milestones. **Integration 1** implements platform-independent video metadata in `dvs-media`; GPU, decoder, and playback runtime code are not started.

---

## 1. Scope and non-goals

### In scope (first production vertical slice)

- Windows DX12/wgpu path only
- Single HEVC clip, sequential playback
- D3D11VA → one GPU `CopySubresourceRegion` per frame → shared NV12 → wgpu import → WGSL BT.709 limited → dedicated video viewport
- Bidirectional timeline fence (`ready = 2N+1`, `consumed = 2N+2`)
- D3D11 keyed-mutex producer contract
- wgpu **raw/present queue** Wait/Signal only
- **Single** shared NV12 texture (serializes producer/consumer)
- Bounded channels, explicit errors, no silent fallback

### Out of scope for initial slice

- Multi-buffered shared textures (future optimization milestone)
- Software decode fallback on Windows primary path
- CPU readback, `av_hwframe_transfer_data`, swscale, CPU RGBA staging
- Timeline editing, multi-track compositor, scrubbing polish, audio
- macOS Metal / VideoToolbox
- Changes to `tests/gpu_d3d11_interop` behavior (regression reference only)

### Preserved experiment artifacts

| Artifact | Role |
|----------|------|
| `tests/gpu_d3d11_interop` | Regression harness; `--visual`, `--visual-diagnostic`, release benchmark |
| `docs/gpu/GPU_EXPERIMENT_2.md` | Validated evidence record |
| `tests/gpu_nv12`, `tests/gpu_probe` | Prior shader and capability experiments |

---

## 2. Validated experiment evidence (facts)

From GPU Experiment 2 (commit `a5fdb42`):

| Fact | Evidence |
|------|----------|
| `AV_PIX_FMT_D3D11` decode to `ID3D11Texture2D` NV12 3840×2176 (2160 visible) | Steps 1–32 |
| Decoder texture not shareable; shareable pool texture required | `D3D11_BIND_DECODER` only |
| `CopySubresourceRegion` GPU copy per frame | Step 15 |
| `SHARED_NTHANDLE \| SHARED_KEYEDMUTEX` shareable NV12 | Step 17 |
| D3D12 `OpenSharedHandle` on same adapter (RX 580) | Step 18 |
| wgpu-hal DX12 import + NV12 Plane0/Plane1 views | Steps 33–34 |
| WGSL BT.709 limited + 2160/2176 crop | Step 35 |
| Bidirectional fence fixes continuous green frame | Diagnostic + fix |
| wgpu init **before** FFmpeg/D3D11VA selects correct adapter | Empirical on test machine |
| Release benchmark: 90/90 frames, 61.07 wall-clock FPS, 1.474 s | Step 39 |
| Human `--visual`: continuous real HEVC PASS | Documented user confirmation |
| D3D12 `IDXGIKeyedMutex` on opened resource: **E_NOINTERFACE** | Keyed mutex D3D11-side only |
| Probe D3D12 command queue must **not** be used for production Wait/Signal | Experiment fix |

**Terminology:** GPU-resident pipeline with **one GPU→GPU copy** per frame. Not zero-copy.

---

## 3. Obsolete pre-Experiment-2 assumptions

Documentation written before Experiment 2 completion should be read with these corrections:

| Obsolete assumption | Current status |
|---------------------|----------------|
| D3D11VA → wgpu interop **unproven** / highest risk | **Proven** on Windows 10 + RX 580 (Experiment 2) |
| wgpu Windows backend **undecided** (Vulkan vs DX12) | **DX12 primary** for video interop path |
| CPU RGBA / `av_hwframe_transfer_data` as practical mitigation for this HW path | **Rejected** for initial Windows vertical slice; degraded paths deferred |
| `VideoFrame` owning crate **unresolved** | **Resolved:** metadata in `dvs-media`; GPU resources in `dvs-gpu`; decoder submits via typed ingest API |
| Adapter initialization order **unresolved** | **Resolved:** wgpu DX12 (+ surface) before FFmpeg D3D11VA session |
| Fence sync model **one-way D3D11 Signal → D3D12 Wait** sufficient | **Insufficient**; bidirectional timeline required for texture reuse |
| Keyed mutex as cross-API D3D12 sync | **Invalid** on opened NV12; D3D11 producer contract only |
| "Do not implement until Experiments 0–4 pass" | Experiments 0–2 **complete**; production extraction may begin per milestones below |
| `GPU_ARCHITECTURE_SPIKE.md` Experiment 2–6 **UNVALIDATED** | Experiment 2 supersedes spike Experiments 2, 3, 4, 5, 6 for Windows DX12 path |
| egui as video compositor | Still invalid; dedicated viewport remains required |

---

## 4. Internal dependency graph (Integration 0 — wired)

`dvs-app` is the **composition root**. It depends on all vertical-slice crates because it owns initialization order and runtime wiring. The graph is **acyclic**.

```text
dvs-app
  → dvs-core
  → dvs-ui
  → dvs-media
  → dvs-gpu
  → dvs-decoder
  → dvs-render
  → dvs-playback

dvs-playback
  → dvs-media
  → dvs-decoder
  → dvs-render

dvs-decoder
  → dvs-media
  → dvs-gpu

dvs-render
  → dvs-media
  → dvs-gpu

dvs-ui
  → dvs-core

dvs-gpu
  → (no internal crates)

dvs-media
  → (no internal crates)
```

### Compile-time edges (Cargo.toml)

| Crate | Depends on | Must NOT depend on |
|-------|------------|-------------------|
| `dvs-media` | — | FFmpeg, wgpu, windows, egui, any internal crate |
| `dvs-gpu` | — (external deps added in Integration 2+) | `dvs-decoder`, FFmpeg, egui |
| `dvs-decoder` | `dvs-media`, `dvs-gpu` | `dvs-ui`, `dvs-playback`, `dvs-render`, egui |
| `dvs-render` | `dvs-media`, `dvs-gpu` | `dvs-decoder`, FFmpeg, fence/COM types |
| `dvs-playback` | `dvs-media`, `dvs-decoder`, `dvs-render` | `dvs-ui`, `dvs-app`, wgpu-hal, COM, egui |
| `dvs-ui` | `dvs-core` | `dvs-decoder`, `dvs-gpu`, `dvs-render`, `dvs-playback` |
| `dvs-app` | all slice crates above | decode/sync on UI thread |

**Acyclic rule:** `dvs-gpu` does not depend on `dvs-decoder`. Playback mediates decode → render. Windows COM types never reach `dvs-media`, `dvs-playback`, `dvs-ui`, or `dvs-app`.

**Integration 0 scope:** path dependencies in `[workspace.dependencies]` and crate manifests only. No production API, no external crates, no runtime behavior.

---

## 5. Architectural ownership

| Concern | Owning crate | Notes |
|---------|--------------|-------|
| Dimensions, visible vs allocation height, pixel format | `dvs-media` | Platform-independent |
| Color range / matrix / primaries | `dvs-media` | BT.709 limited for slice |
| PTS, time base, frame identity | `dvs-media` | No GPU types |
| wgpu instance, adapter, device, queue, surface | `dvs-gpu` | Thread-affine to GPU thread |
| Adapter LUID / identity | `dvs-gpu` | Integration 3A: exact DXGI LUID from `ID3D12Device::GetAdapterLuid`. Integration 4: decoder-side LUID comparison |
| Shareable NV12 texture + NT handle lifetime | `dvs-gpu` | Single texture for slice |
| Shared fence HANDLE + cached `ID3D12Fence` | `dvs-gpu` | Open once, retain |
| `ContinuousFramebufferTimeline` | `dvs-gpu` | `ready`/`consumed` values |
| D3D11 Wait/Signal, keyed mutex, copy into shareable | `dvs-gpu` | `WindowsD3d11InteropBridge` |
| wgpu raw-queue Wait(ready) / Signal(consumed) | `dvs-gpu` | After render submission hook |
| Imported wgpu NV12 texture + plane views | `dvs-gpu` | `GpuVideoFrame` interior |
| FFmpeg, D3D11VA session, packets, seek | `dvs-decoder` | Private `AVFrame`; never public |
| Decoder surface → GPU ingest conversion | `dvs-decoder` | Builds `dvs_gpu::D3d11DecodedSurfaceRef` from private frame; calls `dvs-gpu` ingest |
| `D3d11DecodedSurfaceRef` type (Windows) | `dvs-gpu` | `#[cfg(windows)]`; public ingest input, not owned by decoder crate |
| NV12→RGB WGSL, bind groups, render pass | `dvs-render` | Samples `GpuVideoFrame` views |
| Viewport scale/crop parameters | `dvs-render` | Uses `dvs-media` metadata |
| Playback clock, scheduling, drop policy | `dvs-playback` | No COM/fence |
| UI transport commands | `dvs-ui` / `dvs-app` | Never decode or sync |
| Validated initialization order | `dvs-app` | wgpu → adapter LUID → FFmpeg D3D11VA → bridge → render → playback threads |

---

## 6. Proposed API contracts (not implemented)

Types below are **design targets**. Signatures are illustrative.

### 6.1 `dvs-media` (public, platform-independent)

```rust
/// Stable frame identity for scheduling and metrics.
pub struct FrameId(pub u64);

/// Presentation timestamp in stream time base (rational ticks).
pub struct MediaTimestamp {
    pub pts: i64,
    pub time_base_num: i32,
    pub time_base_den: i32,
}

pub enum PixelFormat {
    Nv12,
    // future: P010, Rgba8 (cpu fallback only)
}

pub enum ColorRange {
    Limited,
    Full,
}

pub enum ColorMatrix {
    Bt709,
    // future: Bt2020, Bt601
}

pub enum ColorPrimaries {
    Bt709,
}

/// Allocation vs visible region (decoder padding).
pub struct VideoDimensions {
    pub coded_width: u32,
    pub coded_height: u32,   // e.g. 2176
    pub visible_width: u32,
    pub visible_height: u32,   // e.g. 2160
}

pub struct VideoFrameMetadata {
    pub frame_id: FrameId,
    pub timestamp: MediaTimestamp,
    pub dimensions: VideoDimensions,
    pub pixel_format: PixelFormat,
    pub color_range: ColorRange,
    pub color_matrix: ColorMatrix,
    pub color_primaries: ColorPrimaries,
}

pub enum MediaError {
    InvalidDimensions,
    UnsupportedPixelFormat,
}
```

| Type | Send | Sync | Platform | wgpu/Windows exposed |
|------|------|------|----------|----------------------|
| All above | Yes | Yes | None | **No** |

**Created by:** `dvs-decoder` when producing a frame.  
**Destroyed:** N/A (Copy type).

---

### 6.2 `dvs-gpu` (public handles; platform modules private)

```rust
/// Opaque GPU-resident frame ready for render sampling.
/// Interior: imported wgpu NV12 texture + plane views + timeline generation.
pub struct GpuVideoFrame {
    pub metadata: dvs_media::VideoFrameMetadata,
    // private: ImportedNv12Resource, timeline_index
}

/// Non-owning token returned after ingest; playback awaits render completion.
pub struct GpuFrameSlot {
    pub frame_id: dvs_media::FrameId,
    pub timeline_generation: u64,
}

pub struct AdapterIdentity {
    pub name: String,
    pub luid_high: i32,
    pub luid_low: u32,
}

/// Application-visible GPU context (no HAL pointers).
pub struct GpuContext {
    // private: wgpu Device/Queue/Surface, adapter identity
}

/// Monotonic fence timeline: ready = 2N+1, consumed = 2N+2.
pub struct FenceTimeline {
    frame_index: u64,
}

impl FenceTimeline {
    pub fn wait_consumed_before_reuse(&self) -> Option<u64>;
    pub fn ready_value(&self) -> u64;
    pub fn consumed_value(&self) -> u64;
    pub fn advance(&mut self);
}

/// Borrowed D3D11 decoder surface for one GPU ingest operation.
/// Owned by `dvs-gpu` (Windows public API). Constructed by `dvs-decoder`
/// from its private AVFrame; must not outlive the decoder borrow.
#[cfg(target_os = "windows")]
pub struct D3d11DecodedSurfaceRef<'a> {
    pub metadata: dvs_media::VideoFrameMetadata,
    pub array_slice: u32,
    // private: ID3D11Texture2D* — only dvs-gpu/dvs-decoder ingest path may access
    _marker: std::marker::PhantomData<&'a ()>,
}

#[cfg(windows)]
pub struct WindowsD3d11InteropBridge {
    // private: shareable texture, fence bundle, context4, keyed mutex
}

impl WindowsD3d11InteropBridge {
    /// D3D11 Wait(prev consumed) → AcquireSync → CopySubresourceRegion → ReleaseSync → Signal(ready).
    pub fn ingest_d3d11_frame(
        &mut self,
        timeline: &FenceTimeline,
        frame: D3d11DecodedSurfaceRef<'_>,
    ) -> Result<(), GpuError>;

    /// wgpu raw queue Wait(ready).
    pub fn wait_ready_on_present_queue(
        &self,
        ctx: &GpuContext,
        timeline: &FenceTimeline,
    ) -> Result<(), GpuError>;

    /// wgpu raw queue Signal(consumed) — call after render submit.
    pub fn signal_consumed_on_present_queue(
        &self,
        ctx: &GpuContext,
        timeline: &FenceTimeline,
    ) -> Result<(), GpuError>;
}

pub enum GpuError {
    AdapterNotFound,
    WgpuInitFailed(String),
    InteropBridgeFailed(String),
    KeyedMutexAcquireTimeout,
    FenceWaitFailed(String),
    FenceSignalFailed(String),
    InvalidTimeline,
    UnsupportedPlatform,
}
```

| Type | Send | Sync | Notes |
|------|------|------|-------|
| `GpuVideoFrame` | **No** | **No** | GPU thread only unless wrapped in channel message with explicit transfer protocol |
| `GpuContext` | **No** | **No** | GPU/render thread affine |
| `D3d11DecodedSurfaceRef` | **No** | **No** | `#[cfg(windows)]`; constructed by decoder, typed in `dvs-gpu` |
| `FenceTimeline` | Yes | No | Owned by playback; values sent to GPU thread |
| `AdapterIdentity` | Yes | Yes | Captured at init |

**wgpu exposure:** `dvs-render` may receive `&GpuVideoFrame` and call `dvs_gpu::render_views(frame)` returning `(&TextureView, &TextureView)` — views borrowed for one render pass. **Do not** expose `wgpu::Device` to `dvs-decoder` or `dvs-playback`.

**Created by:** `GpuContext::initialize`, `WindowsD3d11InteropBridge::create`.  
**Destroyed by:** `GpuContext::shutdown`, bridge drop after playback stops.

---

### 6.3 `dvs-decoder` (public session API; FFmpeg private)

```rust
pub struct DecoderSession {
    // private: AVFormatContext, AVCodecContext, hw_device, AvFrame pool
}

pub struct DecoderOpenOptions {
    pub path: std::path::PathBuf,
    pub required_adapter: dvs_gpu::AdapterIdentity,
}

impl DecoderSession {
    pub fn open(options: DecoderOpenOptions) -> Result<Self, DecoderError>;

    pub fn seek(&mut self, timestamp: dvs_media::MediaTimestamp) -> Result<(), DecoderError>;

    /// Decode next displayable frame and invoke GPU ingest closure.
    /// Does NOT copy into shareable texture itself — passes `dvs_gpu::D3d11DecodedSurfaceRef`.
    #[cfg(target_os = "windows")]
    pub fn decode_next_for_gpu_ingest<F, R>(
        &mut self,
        ingest: F,
    ) -> Result<Option<R>, DecoderError>
    where
        F: FnOnce(dvs_gpu::D3d11DecodedSurfaceRef<'_>) -> Result<R, dvs_gpu::GpuError>;
}

pub enum DecoderError {
    OpenFailed(String),
    SeekFailed(String),
    DecodeFailed(String),
    NotD3d11Frame,
    EndOfStream,
    AdapterMismatch,
}
```

| Rule | Detail |
|------|--------|
| No `AVFrame` in public API | Private `AVFrame`; public surface type is `dvs_gpu::D3d11DecodedSurfaceRef` |
| Surface ref ownership | Type lives in `dvs-gpu`; decoder constructs it from private FFmpeg state |
| `Send`/`Sync` | `DecoderSession` is **not** `Sync`; owned by decoder thread |
| Platform cfg | `decode_next_for_gpu_ingest` is `#[cfg(target_os = "windows")]` only for slice |

---

### 6.4 `dvs-render` (public renderer)

```rust
pub struct ViewportDesc {
    pub width: u32,
    pub height: u32,
}

pub struct VideoRenderer {
    // private: pipeline, bind group layout, sampler
}

impl VideoRenderer {
    pub fn new(ctx: &dvs_gpu::GpuContext, viewport: ViewportDesc) -> Result<Self, RenderError>;

    /// Records render pass into ctx's surface target. Does NOT Signal consumed.
    pub fn draw_nv12_frame(
        &self,
        ctx: &mut dvs_gpu::GpuContext,
        frame: &dvs_gpu::GpuVideoFrame,
    ) -> Result<(), RenderError>;
}

pub enum RenderError {
    PipelineCreationFailed(String),
    SurfaceLost,
    DrawFailed(String),
}
```

`dvs-render` never calls `CopySubresourceRegion` or fence APIs.

---

### 6.5 `dvs-playback` (public engine)

```rust
pub enum TransportCommand {
    Play,
    Pause,
    Seek(dvs_media::MediaTimestamp),
    Shutdown,
}

pub struct PlaybackConfig {
    pub max_inflight_frames: u32, // 1 for single-texture slice
}

pub struct PlaybackEngine {
    // private: channels to decoder + GPU threads, clock state
}

pub struct PlaybackMetrics {
    pub frames_scheduled: u64,
    pub frames_presented: u64,
    pub frames_dropped: u64,
    pub last_decode_ms: f64,
    pub last_render_ms: f64,
}

impl PlaybackEngine {
    pub fn start(/* handles to decoder, gpu, render */) -> Result<Self, PlaybackError>;
    pub fn send_command(&self, cmd: TransportCommand) -> Result<(), PlaybackError>;
    pub fn poll_metrics(&self) -> PlaybackMetrics;
}

pub enum PlaybackError {
    EngineNotRunning,
    ChannelClosed,
    DecodeFailed(DecoderError),
    GpuFailed(GpuError),
    RenderFailed(RenderError),
}
```

Playback owns **scheduling** and **drop policy**; it sends work items, not fence values, to the GPU thread.

---

### 6.6 `dvs-app` / `dvs-ui`

```rust
// dvs-app: wires threads, creates video viewport window, passes TransportCommand from UI
pub struct AppRuntime { /* ... */ }

// dvs-ui: emits TransportCommand; displays PlaybackMetrics; never touches GpuContext
```

---

## 7. Unsafe boundary invariants

All proven by Experiment 2; production code must enforce in documentation and code review:

| ID | Invariant |
|----|-----------|
| U1 | D3D11 decode device and wgpu device are the **same physical adapter** (LUID match) |
| U2 | wgpu DX12 initialized **before** FFmpeg `d3d11va` device creation |
| U3 | Shareable NV12 descriptor matches imported `ID3D12Resource` (3840×2176 NV12, `SHARED_NTHANDLE \| SHARED_KEYEDMUTEX`) |
| U4 | `OpenSharedHandle` for texture and fence runs **once** at init; cached `ID3D12Fence` reused |
| U5 | wgpu `Wait(ready)` on **raw/present queue** before render pass samples imported texture |
| U6 | wgpu `Signal(consumed)` on **same raw/present queue** immediately after render `queue.submit` |
| U7 | D3D11 `Wait(previous_consumed)` on GPU queue before overwriting shareable texture (N > 0) |
| U8 | `AcquireSync(0)` → copy → `ReleaseSync(0)` around every D3D11 producer write |
| U9 | Fence values monotonic: `ready = 2N+1`, `consumed = 2N+2`; never reuse incorrectly |
| U10 | `AVFrame` / decoder `ID3D11Texture2D` remains valid through copy submission |
| U11 | Imported wgpu texture outlives all `TextureView`s and bind groups sampling it |
| U12 | No concurrent mutation of imported texture (single-texture serialization) |
| U13 | `create_texture_from_hal` initial state matches wgpu 27 expectations (`TextureUses` documented at import) |
| U14 | No `av_hwframe_transfer_data`, Map, or staging readback on hot path |

Unsafe blocks confined to `dvs-gpu` (`windows` + `wgpu-hal` interop module).

---

## 8. Threading model (first vertical slice)

### Threads

| Thread | Owns | Must never |
|--------|------|------------|
| **UI** | egui state, transport intents | Decode, GPU sync, `wgpu::Queue::submit` |
| **Playback / scheduler** | clock, `FenceTimeline` index, drop policy | COM, FFmpeg, wgpu HAL |
| **Decoder** | `DecoderSession`, `AVFrame` | wgpu submit, fence Signal/Wait |
| **GPU / render** | `GpuContext`, `WindowsD3d11InteropBridge`, `VideoRenderer` | FFmpeg calls, UI |

### Channel topology (bounded `crossbeam-channel` proposed)

```text
UI ──TransportCommand──► Playback
Playback ──DecodeRequest──► Decoder
Decoder ──dvs_gpu::D3d11DecodedSurfaceRef (via ingest closure)──► GPU (serialized)
Playback ──PresentFrame(slot)──► GPU
GPU ──FramePresented / FrameDropped──► Playback
Playback ──PlaybackMetrics snapshot──► UI (read-only, per frame or 120 Hz)
```

**Suggested capacities:** decode queue depth 2; present queue depth 1 (matches single texture).

### Per-frame handshake (single texture)

```text
Playback: schedule frame N
Decoder:  decode → build `D3d11DecodedSurfaceRef` → bridge.ingest (D3D11 path)
GPU:      bridge.ingest (D3D11 path) → wait_ready → render.draw → signal_consumed → advance timeline
Playback: on consumed ack, schedule N+1 (or drop if late)
```

### Backpressure and dropping

With **one** shareable texture:

- At most **one** frame in flight between ingest and `Signal(consumed)`.
- If decoder finishes next frame before consumed: **drop** decode result or block decoder thread (prefer **drop + metric** to keep decoder thread bounded).
- Playback clock continues; dropped frames increment `PlaybackMetrics.frames_dropped`.
- No unbounded queue of decoded surfaces.

**No tokio** on hot path. `std::thread` + bounded channels only.

---

## 9. Initialization order

Exact startup sequence for `dvs-app` (Windows slice):

| Step | Action | Failure behavior |
|------|--------|------------------|
| 1 | Create application + **video viewport window** (winit) | Exit with `GpuError::SurfaceLost` / init error dialog |
| 2 | `GpuBootstrap::initialize` — wgpu DX12, surface, `TEXTURE_FORMAT_NV12` | **Fail fast** — no decode without GPU |
| 3 | Capture `AdapterIdentity` including exact DXGI LUID from wgpu DX12 device | Fail if LUID extraction fails on Windows |
| 4 | `DecoderSession::open` with `required_adapter` — FFmpeg D3D11VA, `AV_PIX_FMT_D3D11` | **Fail fast** — no software fallback in slice |
| 4b | `validate_same_adapter(expected, decoder_luid)` | Fail if LUID mismatch |
| 5 | `WindowsD3d11InteropBridge::create` — shareable NV12, shared fence, probe bootstrap Signal(1) | Fail with typed `GpuError` |
| 6 | Import shareable texture into wgpu; create plane views; cache fence | Fail — do not continue |
| 7 | `VideoRenderer::new` — compile WGSL (same coefficients as experiment) | Fail |
| 8 | Spawn decoder, GPU, playback threads; `PlaybackEngine::start` | Roll back threads; release GPU |

**No silent fallback** at any step. Errors surface to UI as typed enums.

---

## 10. Experiment → production module mapping

| Experiment module | Production target |
|-------------------|-------------------|
| `wgpu_hal_interop.rs` | `dvs-gpu/src/dx12/context.rs`, `import.rs`, `fence.rs` |
| `main.rs` shareable texture, copy, fence, keyed mutex | `dvs-gpu/src/windows/d3d11_bridge.rs` |
| `main.rs` FFmpeg probe/decode | `dvs-decoder/src/ffmpeg/d3d11va_session.rs` |
| `render_path.rs` | `dvs-render/src/nv12_passthrough.rs` |
| `shaders/nv12_to_rgb.wgsl` | `dvs-render/shaders/nv12_to_rgb.wgsl` (copy verbatim coefficients) |
| `multi_frame.rs` timeline + loop | `dvs-playback/src/engine.rs` + `dvs-gpu/src/fence_timeline.rs` |
| `visual_validation.rs` | `dvs-app` integration test / manual QA |
| `visual_diagnostic.rs` | Remains in experiment crate (Integration 8 regression) |

---

## 11. Implementation milestones

Every milestone must compile (`cargo check --workspace`). No milestone modifies experiment behavior until Integration 8.

### Integration 0 — Documentation and dependency graph ✅

| Field | Detail |
|-------|--------|
| **Status** | **Complete** — compile-time path deps wired; no production API |
| **Files** | This file; `ARCHITECTURE.md`; `ROADMAP.md`; root + slice `Cargo.toml` manifests |
| **Dependencies** | `[workspace.dependencies]` path entries; internal edges per §4 (no external deps) |
| **Acceptance** | `cargo check --workspace`; acyclic `cargo tree`; experiment crate untouched |
| **Verify** | `cargo check --workspace`, `cargo test --workspace`, `cargo tree -p dvs-app` |
| **Untouched** | All `src/**` logic (scaffold retained), experiments, external dependencies |
| **Rollback** | Revert doc/TOML dependency edges only |

### Integration 1 — `dvs-media` metadata contracts ✅

| Field | Detail |
|-------|--------|
| **Files** | `crates/dvs-media/src/{lib,dimensions,pixel_format,color,time,metadata,error}.rs` |
| **Dependencies** | `thiserror` only |
| **Public types** | `FrameId`, `Extent2D`, `VisibleRect`, `VideoDimensions`, `VideoPixelFormat`, `VideoColorInfo` (+ `ColorRange`, `ColorMatrix`, `ColorPrimaries`, `TransferCharacteristic`), `TimeBase`, `MediaTimestamp`, `VideoFrameMetadata`, `MetadataError` |
| **Acceptance** | Unit tests for dimensions, time base, color, metadata, and `Send + Sync` assertions |
| **Verify** | `cargo test -p dvs-media`; `cargo clippy -p dvs-media -- -D warnings` |
| **Untouched** | GPU, decoder, playback, experiments |

### Integration 2 — `dvs-gpu` context, adapter identity, errors ✅

| Field | Detail |
|-------|--------|
| **Files** | `crates/dvs-gpu/src/{lib,context,adapter,error,fence_timeline}.rs` |
| **Dependencies** | `wgpu 27.0.1`, `thiserror`, `raw-window-handle 0.6` |
| **Public types** | `GpuBackend`, `GpuDeviceType`, `AdapterIdentity`, `GpuBootstrap`, `GpuContext`, `SurfaceWindowTarget`, `GpuError`, `FenceTimeline`, `FrameFenceValues` |
| **LUID note** | wgpu 27 `AdapterInfo` does not expose DXGI LUID. Integration 2 captures safe public identity only. Exact LUID moves to Integration 3 (`wgpu-hal` + Windows). Vendor/device IDs are not a LUID substitute. |
| **Acceptance** | `GpuBootstrap::initialize` (async, surface-backed DX12 path); `FenceTimeline` unit tests match experiment values (`2N+1` ready, `2N+2` consumed) |
| **Verify** | `cargo test -p dvs-gpu`; `cargo clippy -p dvs-gpu -- -D warnings` |
| **Untouched** | FFmpeg, D3D11 interop bridge, render, experiments |

### Integration 3A — DXGI adapter LUID extraction ✅

| Field | Detail |
|-------|--------|
| **Files** | `crates/dvs-gpu/src/luid.rs`, `crates/dvs-gpu/src/windows/{mod,dxgi_luid}.rs` |
| **Dependencies** | `windows 0.58` (cfg windows only); wgpu `hal` re-export |
| **Public types** | `DxgiAdapterLuid`, `validate_same_adapter`; `AdapterIdentity::dxgi_luid()` |
| **Unsafe policy** | Crate `#![deny(unsafe_code)]`; audited unsafe only in `windows/dxgi_luid.rs` (two blocks, each with preceding `SAFETY` comment) |
| **Acceptance** | HAL path: `ID3D12Device::GetAdapterLuid` via `device.as_hal::<Dx12>()`; pure unit tests for `DxgiAdapterLuid` and `validate_same_adapter` |
| **Runtime note** | Production `GpuBootstrap::initialize` LUID attachment is **compilation-verified only** on Windows. A real window/app runtime test is pending (`dvs-app`). **GPU Experiment 2** remains the runtime evidence for DXGI/D3D11/wgpu interop. |
| **Verify** | `cargo test -p dvs-gpu`; `cargo clippy -p dvs-gpu -- -D warnings`; `cargo check -p dvs-gpu` on Windows |
| **Untouched** | Shared textures, fences, D3D11 bridge, decoder, experiments |

### Integration 3B — Windows D3D11 shared NV12 producer ✅ (hardware-validated)

| Field | Detail |
|-------|--------|
| **Baseline** | `b351b0eb01d497bd73ba1f0b636bf142d946f270` |
| **Files** | `crates/dvs-gpu/src/{nv12_allocation,error}.rs`, `crates/dvs-gpu/src/windows/{d3d11_device,d3d11_surface,shared_nv12,owned_handle}.rs`, `crates/dvs-gpu/tests/windows_d3d11_shared.rs` |
| **Dependencies** | Extended `windows 0.58` cfg-windows features only (`Win32_Graphics_Direct3D11`, `Dxgi`, etc.); no new crates, no wgpu-hal |
| **Public types** | `D3d11DecodedSurfaceRef`, `SharedNv12TextureDesc`, `WindowsD3d11SharedNv12Producer` (Windows-only) |
| **Unsafe policy** | Crate `#![deny(unsafe_code)]`; audited unsafe in `windows/{owned_handle,d3d11_device,d3d11_surface,shared_nv12}.rs` only |
| **Acceptance** | One shareable NV12 texture (`SHARED_NTHANDLE \| SHARED_KEYEDMUTEX`), NT texture/fence handles, `ID3D11Fence` shared sync, keyed-mutex guarded `CopySubresourceRegion`, D3D11 `Wait(consumed)` / `Signal(ready)` ordering; exact D3D11 adapter LUID validation against wgpu `DxgiAdapterLuid` |
| **Runtime evidence** | `cargo test -p dvs-gpu --test windows_d3d11_shared -- --ignored --nocapture` on Windows hardware (synthetic NV12 source in test only) |
| **Not implemented** | D3D12 `OpenSharedHandle`, wgpu-hal texture import, plane views, shaders, FFmpeg, playback |
| **Verify** | `cargo test -p dvs-gpu`; `cargo clippy -p dvs-gpu -- -D warnings`; ignored hardware test |
| **Untouched** | Experiment 2 sources, `dvs-decoder`, `context.rs` / `adapter.rs` / `luid.rs` / `fence_timeline.rs` |

### Integration 3C — D3D12/wgpu shared NV12 consumer ✅ (hardware-validated)

| Field | Detail |
|-------|--------|
| **Baseline** | `a4c005c8783198034a0abb24dd0aa0da9c53b06c` |
| **Files** | `crates/dvs-gpu/src/gpu_video_frame.rs`, `crates/dvs-gpu/src/windows/{dx12_import,dx12_queue_sync,interop_bridge}.rs`, `crates/dvs-gpu/tests/windows_d3d11_wgpu_interop.rs` |
| **Public types** | `GpuVideoFrame`, `GpuVideoPixelFormat`, `WindowsD3d11WgpuInteropBridge` (Windows-only) |
| **Acceptance** | One-time `OpenSharedHandle` (texture=1, fence=1); D3D12 descriptor validation; `texture_from_raw` + `create_texture_from_hal`; raw queue `Wait(ready)` / `Signal(consumed)`; two-frame hardware cycle with synthetic D3D11 NV12 source |
| **Runtime evidence** | `cargo test -p dvs-gpu --test windows_d3d11_wgpu_interop -- --ignored --nocapture` on Windows hardware. Validates shared-handle import and bidirectional fence sync only — **not** real FFmpeg frames, **not** WGSL sampling, **not** visual production validation |
| **Not implemented** | FFmpeg (Integration 4), decoder, WGSL shaders, render pipeline (Integration 5), playback, surface presentation |
| **Verify** | `cargo test -p dvs-gpu`; ignored interop hardware test |

### Integration 3 — Windows D3D11/D3D12 interop bridge

| Field | Detail |
|-------|--------|
| **Status** | **Complete** — 3B producer + 3C consumer hardware-validated (synthetic D3D11 test texture); FFmpeg decode wiring (Integration 4) and wgpu render sampling (Integration 5) complete |

### Integration 4 — `dvs-decoder` D3D11VA session extraction

| Field | Detail |
|-------|--------|
| **Status** | **Complete (4A + 4B)** — real FFmpeg D3D11VA borrowed surfaces validated through `decode_next_d3d11`; 90-frame hardware test bridges decoder surfaces through `WindowsD3d11WgpuInteropBridge` into wgpu-imported NV12; no CPU readback, software fallback, rendering, or pixel-content validation yet |
| **Files** | `crates/dvs-decoder/src/{lib,session,error,metadata}.rs`, `ffmpeg/{mod,ffi,raii,d3d11va}.rs`, `tests/windows_d3d11va_decode.rs`, `tests/windows_d3d11va_interop.rs` |
| **Dependencies** | `ffmpeg-sys-next`, `dvs-media`, `dvs-gpu` |
| **Acceptance (4A)** | Opens fixture; validates DXGI LUID against wgpu; `decode_next_d3d11` returns `VideoFrameMetadata` + borrowed `D3d11DecodedSurfaceRef`; no shareable copy, bridge, render, or CPU transfer inside decoder |
| **Acceptance (4B)** | Real decoded surfaces enter `WindowsD3d11SharedNv12Producer` + `WindowsD3d11WgpuInteropBridge`; GPU-only `CopySubresourceRegion` and `Signal(ready)` enqueued in order on FFmpeg's `device_context` under FFmpeg lock; `Flush` submits asynchronously (does not wait for GPU completion); source slice reuse is ordered by same immediate context before the next `decode_next_d3d11`; existing fence/keyed-mutex protocol; wgpu queue waits for `ready` before accessing the shareable destination; `consumed` protects destination reuse only; empty `queue.submit` exercises completion/release only (no NV12 sampling); `DecoderD3d11Hardware` exposes FFmpeg D3D11 device/context for producer setup |
| **Verify** | `cargo test -p dvs-decoder` + `cargo test -p dvs-decoder --test windows_d3d11va_decode -- --ignored --nocapture` + `cargo test -p dvs-decoder --test windows_d3d11va_interop -- --ignored --nocapture --test-threads=1` |
| **Untouched** | `dvs-playback`, `dvs-app`, Experiment 2 |

### Integration 5 — `dvs-render` NV12 passthrough renderer

| Field | Detail |
|-------|--------|
| **Status** | **Complete** — automated hardware validation PASS (90/90); initial human visual validation **FAIL** (oversized clip vertices incorrectly transformed by viewport origin/extent while UV domain remained oversized); regression correction applied (fixed clip-space fullscreen triangle; aspect fit via rectangular viewport/scissor only); repeated human visual validation **PASS** (recognizable complete real frame; no diagonal or horizontal edge streaks; colors and orientation visually plausible; crop/aspect accepted) |
| **Files** | `crates/dvs-render/src/{lib,error,color,crop,aspect,fullscreen,output,uniforms,nv12_renderer,surface}.rs`, `crates/dvs-render/shaders/nv12_to_rgb.wgsl`, `crates/dvs-render/tests/windows_nv12_render.rs`, `crates/dvs-render/examples/windows_nv12_visual.rs`, `crates/dvs-gpu/src/nv12_plane_views.rs` |
| **Dependencies** | `dvs-gpu`, `dvs-media`, `wgpu`, `bytemuck`, `thiserror` |
| **Acceptance** | Production `Nv12Renderer` samples imported NV12 plane views with metadata-driven YUV→RGB; 90-frame hardware test uses real render passes between `prepare_frame` and `signal_consumed_after_submit`; manual visual example defers `signal_consumed` until exit; SDR path only (unsupported HDR/PQ/HLG rejected); no CPU readback, CPU GPU wait, or software fallback |
| **Not implemented** | Continuous timed playback (Integration 6), audio, app wiring, full colorimetric certification, HDR display support |
| **Verify** | `cargo test -p dvs-render` + `cargo test -p dvs-render --test windows_nv12_render -- --ignored --nocapture --test-threads=1` + `cargo run -p dvs-render --example windows_nv12_visual --release` (human PASS) |
| **Untouched** | `dvs-playback`, `dvs-app`, Experiment 2 |

### Integration 6 — `dvs-playback` continuous single-clip slice

| Field | Detail |
|-------|--------|
| **Files** | `crates/dvs-playback/src/{lib,engine,clock,metrics,error}.rs` |
| **Dependencies** | `dvs-decoder`, `dvs-render`, `dvs-media`, `crossbeam-channel` |
| **Acceptance** | 90-frame sequential run matches experiment throughput order-of-magnitude; metrics exported |
| **Verify** | `cargo test -p dvs-playback -- --ignored` or dedicated bin |
| **Untouched** | `dvs-ui` |

### Integration 7 — `dvs-app` native viewport hookup

| Field | Detail |
|-------|--------|
| **Files** | `crates/dvs-app/src/{main,runtime,viewport}.rs`, minimal `dvs-ui` transport hooks |
| **Acceptance** | User opens app, loads fixture, sees continuous video in viewport; ESC exits |
| **Verify** | Manual + `cargo run -p dvs-app` |
| **Untouched** | Timeline, project system |

### Integration 8 — Experiment 2 regression via production APIs

| Field | Detail |
|-------|--------|
| **Files** | `tests/gpu_d3d11_interop` refactored to call `dvs-gpu`/`dvs-decoder`/`dvs-render` OR parallel `tests/gpu_production_regression` |
| **Acceptance** | Same PASS criteria as Experiment 2; `--visual-diagnostic` keys preserved |
| **Verify** | `cargo run -p gpu-d3d11-interop -- --visual` + release benchmark |
| **Rollback** | Keep experiment self-contained if refactor risks regression |

### Future — Multi-buffered shared textures

Separate milestone after slice stable. Increases throughput; relaxes single-texture serialization. Not part of initial integration.

---

## 12. Open decisions (still require choice)

| ID | Decision | Options | Recommendation |
|----|----------|---------|----------------|
| OD1 | Video viewport embedding with egui shell | Child `winit` window (experiment style) vs `egui::Viewport` vs raw child HWND | **Child window** first — matches Experiment 2; egui embed later |
| OD2 | Channel crate | `std::sync::mpsc` vs `crossbeam-channel` | **crossbeam bounded** (per architecture review) |
| OD3 | `dvs-gpu` interop integration test location | Dev-dep test in crate vs workspace `tests/` | Workspace `tests/` binary to avoid feature creep in lib |
| OD4 | Software decode fallback timing | Phase 1 vs later | **Defer** until GPU slice ships; document as explicit degraded mode |
| OD5 | Vulkan DX12 fallback backend | Secondary wgpu backend | **Defer**; Experiment 2 validated DX12 on RX 580 only |
| OD6 | `GpuVideoFrame` cross-thread transfer | GPU thread only vs `Arc` + mutex | **GPU thread only** for slice; channels carry schedule tokens |
| OD7 | Workspace `wgpu` version pin | 27.0.1 (experiment) | **Pin 27.0.1** until interop re-validated on upgrade |

---

## 13. Performance gates (from experiment)

| Gate | Target | Experiment result |
|------|--------|-------------------|
| 4K HEVC 30 FPS sequential | 0 drops, throughput ≥ 29.97 FPS | 61.07 FPS wall-clock, 90/90 |
| No CPU pixel transfer | Profiling / code inspection | Confirmed in experiment |
| Single-texture serialization | Functional correctness | Proven; throughput acceptable on RX 580 |

Production must re-measure after integration; do not assume identical FPS until Integration 6 benchmark.

---

## 14. References

- `docs/gpu/GPU_EXPERIMENT_2.md` — validated evidence
- `DOVALE_STUDIO_4_HANDOFF_EXPERIMENT_2.md` — handoff summary
- `docs/gpu/GPU_ARCHITECTURE_SPIKE.md` — historical spike (partially superseded)
- `docs/architecture/ARCHITECTURE_REVIEW.md` — pre-implementation review
- `tests/gpu_d3d11_interop/` — reference implementation

---

## Document status

| Item | Status |
|------|--------|
| Experiment evidence | **Validated** (GPU Experiment 2 PASS) |
| Integration 0 dependency wiring | **Complete** (compile-time only) |
| Integration 1 `dvs-media` metadata | **Complete** (`FrameId`, `Extent2D`, `VisibleRect`, `VideoDimensions`, `VideoPixelFormat`, `VideoColorInfo`, `TimeBase`, `MediaTimestamp`, `VideoFrameMetadata`, `MetadataError`) |
| Integration 2 `dvs-gpu` foundation | **Complete** (`GpuBootstrap`, `GpuContext`, `AdapterIdentity`, `GpuError`, `FenceTimeline`) |
| Integration 3A DXGI LUID | **Complete** (HAL extraction + pure tests; **compilation-verified**, runtime via `GpuBootstrap` pending real window test) |
| Integration 3B D3D11 producer | **Complete** (shareable NV12 + fence + keyed mutex; **hardware-validated** on Windows; synthetic test source only) |
| Integration 3C D3D12/wgpu consumer | **Complete** (shared-handle import + two-cycle bidirectional sync; **hardware-validated** with synthetic D3D11 test texture; plane views created in test only — no FFmpeg, no WGSL render, no visual production validation) |
| Integration 3 interop bridge | **Complete** (3B+3C); real FFmpeg source → Integration 4; real wgpu sampling/render → Integration 5 |
| Integration 4A D3D11VA decoder session | **Complete** — real FFmpeg D3D11VA borrowed surfaces validated via `windows_d3d11va_decode` (hardware evidence; not an RX 580 requirement) |
| Integration 4B decoder → interop bridge | **Complete** — 90 real frames bridged via `windows_d3d11va_interop`; GPU-only copy; no rendering or CPU readback |
| Integration 4 overall | **Complete** — decoder session + production interop bridge wiring validated on hardware |
| Integration 5 NV12 WGSL renderer | **Complete** — automated 90/90 hardware validation PASS; initial human visual FAIL (transformed oversized-triangle geometry); regression correction applied; repeated human visual PASS; SDR-only; no playback timing or audio |
| Production API | **Partial** (`dvs-media` + `dvs-gpu` + `dvs-decoder` + `dvs-render` complete through Integration 5; playback/app wiring not started) |
| Production runtime | **Not started** — continuous playback remains Integration 6 |
| CPU fallback | **Not introduced** |
| Experiment 2 regression crate | **Isolated** (`tests/gpu_d3d11_interop` unchanged) |
