# Do Vale Studio 4 — Architecture Review

**Date:** 2026-09-01  
**Phase:** Architecture validation (pre-implementation)  
**Reviewer role:** Lead systems engineer  
**Scope:** Validate workspace structure, crate boundaries, and dependency direction against project documentation. No implementation beyond this review.

---

## Executive Summary

The repository is a **greenfield scaffold** that aligns well with the documented crate decomposition. All 13 crates exist, the workspace compiles, and no forbidden dependencies (FFmpeg, wgpu, egui) have been introduced. However, **no architectural contracts are enforced in code yet**: there are zero inter-crate dependencies, every library crate still contains `cargo init` boilerplate, and no shared types, error model, or threading boundaries exist.

The documentation is strong at the product/architecture level but has gaps in **dependency rules for auxiliary crates**, **type ownership** (especially `VideoFrame`), and **platform interop strategy**. Several decisions must be resolved before Phase 1 implementation begins.

---

## 1. Workspace Structure

### Current State

```
do-vale-studio-4/
├── Cargo.toml              # workspace root, 13 members
├── ARCHITECTURE.md
├── PROJECT_CONTEXT.md
├── DEVELOPMENT.md
├── PERFORMANCE.md
├── ROADMAP.md
└── crates/
    ├── dvs-app/            # binary entry point
    ├── dvs-core/           # domain layer
    ├── dvs-ui/             # egui interface
    ├── dvs-media/          # media abstraction
    ├── dvs-decoder/        # FFmpeg decode
    ├── dvs-playback/       # playback scheduler
    ├── dvs-gpu/            # GPU abstraction
    ├── dvs-render/         # render graph / compositor
    ├── dvs-audio/          # audio engine
    ├── dvs-cache/          # caching
    ├── dvs-project/        # persistence
    ├── dvs-export/         # export/render-out
    └── dvs-ai/             # AI command layer
```

`cargo check --workspace` succeeds. All crates use Rust edition 2024.

### Assessment

| Aspect | Status |
|--------|--------|
| Crate count matches `ARCHITECTURE.md` | ✅ Correct |
| Workspace membership complete | ✅ Correct |
| No premature external dependencies | ✅ Correct |
| `docs/` directory structure | ⚠️ Missing (this review creates it) |
| Crate source is architectural scaffold only | ⚠️ All `lib.rs` files contain unrelated `add()` boilerplate from `cargo init` |
| Workspace dependency management | ⚠️ `[workspace.dependencies]` is empty; no shared crate versions declared |
| Root documentation location | ✅ Acceptable; consider mirroring key docs under `docs/` later |

### Recommendations

1. **Remove `cargo init` boilerplate** from all library crates before Phase 0 implementation. Empty `lib.rs` or a single `//! crate docs` comment is preferable to misleading placeholder functions.
2. **Populate `[workspace.dependencies]`** when shared crates are introduced (`thiserror`, `tracing`, etc.) to keep versions consistent.
3. **Fix `ARCHITECTURE.md` formatting** — the high-level diagram and "Crate Responsibilities" section are merged without a clear separator, reducing readability.
4. **Do not add more crates** until a concrete need appears. Thirteen crates is appropriate for the target system but increases coordination cost during early phases.

---

## 2. Crate Responsibilities

### Documented vs. Intended Responsibilities

| Crate | Documented Role | Validation |
|-------|-----------------|------------|
| `dvs-app` | Entry point, wiring, minimal logic | ✅ Correct placement |
| `dvs-ui` | egui UI only; no decode/composite | ✅ Correct boundary |
| `dvs-core` | Pure domain: project, timeline, commands, time, IDs | ✅ Correct; must stay FFmpeg/egui-free |
| `dvs-media` | Assets, metadata, probing, codec info, capabilities | ✅ Correct |
| `dvs-decoder` | FFmpeg, HW decode, sessions, frame production, seek | ✅ Correct |
| `dvs-playback` | Scheduler: play/pause/seek/scrub/queues/timing | ✅ Correct |
| `dvs-gpu` | wgpu, textures, pipelines, pools, sync | ✅ Correct |
| `dvs-render` | Render graph, compositing, scaling, effects | ✅ Correct |
| `dvs-audio` | Decode, mix, sync, effects | ✅ Correct |
| `dvs-cache` | RAM/disk/render/thumbnail/waveform cache | ✅ Correct |
| `dvs-project` | Project files, autosave, migrations | ✅ Correct |
| `dvs-export` | Timeline render-out, encoding | ✅ Correct |
| `dvs-ai` | Provider abstraction, validated commands | ✅ Correct |

### Boundary Ambiguities to Resolve

| Question | Recommendation |
|----------|----------------|
| Where does the **project model** live — `dvs-core` or `dvs-project`? | **Domain model** (timeline, clips, tracks) in `dvs-core`. **Serialization, file format, migrations** in `dvs-project`. `dvs-project` depends on `dvs-core`, never the reverse. |
| Where does **media probing** end and **decoding** begin? | `dvs-media` owns probe results and capability metadata. `dvs-decoder` owns decode sessions and frame production. `dvs-decoder` depends on `dvs-media` types, not vice versa. |
| Who owns **cache keys**? | `dvs-cache` owns storage and eviction. Cache key derivation may use identifiers from `dvs-core` and metadata from `dvs-media`. |
| Does `dvs-export` duplicate render logic from `dvs-render`? | No. `dvs-export` orchestrates offline output; `dvs-render` provides the render graph. `dvs-export` depends on `dvs-render`. |

---

## 3. Dependency Graph

### Documented Critical Path

```text
Decoder → VideoFrame → Playback → RenderGraph → GPU → Display
```

### Proposed Allowed Dependency Graph

```text
                         dvs-app
                            │
            ┌───────────────┼───────────────┐
            ▼               ▼               ▼
         dvs-ui        dvs-playback     dvs-export
            │               │               │
            ▼               ├───────────────┤
         dvs-core           ▼               ▼
            ▲          dvs-decoder      dvs-render
            │               │               │
            │               ▼               ▼
         dvs-project     dvs-media        dvs-gpu
            │               ▲
            │               │
         dvs-ai          dvs-cache
                            │
                         dvs-audio
```

### Explicit Dependency Rules

| Crate | May depend on | Must NOT depend on |
|-------|---------------|-------------------|
| `dvs-app` | All application crates (wiring only) | Should avoid deep logic |
| `dvs-ui` | `dvs-core` | `dvs-decoder`, `dvs-gpu`, `dvs-render`, FFmpeg, wgpu |
| `dvs-core` | Nothing external to domain | `dvs-ui`, FFmpeg, wgpu, egui, any GPU/media impl |
| `dvs-media` | `dvs-core` (IDs, time) | FFmpeg, wgpu, egui |
| `dvs-decoder` | `dvs-media`, `dvs-gpu` (frame handles) | `dvs-ui`, egui |
| `dvs-playback` | `dvs-core`, `dvs-decoder`, `dvs-render`, `dvs-cache` | `dvs-ui`, egui |
| `dvs-render` | `dvs-gpu`, `dvs-core` (transform params) | `dvs-ui`, FFmpeg, egui |
| `dvs-gpu` | Platform GPU APIs via wgpu (later) | `dvs-ui`, FFmpeg, egui, `dvs-core` timeline logic |
| `dvs-audio` | `dvs-media`, `dvs-core` | `dvs-ui`, `dvs-render` |
| `dvs-cache` | `dvs-media`, `dvs-core` | `dvs-ui` |
| `dvs-project` | `dvs-core` | FFmpeg, wgpu, egui |
| `dvs-export` | `dvs-core`, `dvs-render`, `dvs-decoder` | `dvs-ui` |
| `dvs-ai` | `dvs-core` (command validation) | Direct timeline mutation, FFmpeg, wgpu |

### Current State

**No `Cargo.toml` dependencies exist between crates.** The graph is documented but not enforced. This is acceptable for this validation phase but must be wired in Phase 0 before any real code lands.

### Issues

1. **`ARCHITECTURE.md` dependency section is incomplete.** It lists a linear chain (`dvs-app → dvs-ui → dvs-core → dvs-media → dvs-playback → dvs-render → dvs-gpu`) but omits `dvs-audio`, `dvs-cache`, `dvs-project`, `dvs-export`, and `dvs-ai`.
2. **Risk of `dvs-decoder → dvs-gpu` coupling.** Decoder needs to produce GPU-native frames, so some coupling is inevitable. Mitigate by defining frame handle types in `dvs-gpu` (or a thin shared types module) that `dvs-decoder` targets, without `dvs-gpu` depending on `dvs-decoder`.
3. **Risk of circular dependency** if `dvs-render` needs decoded frames directly. Playback should mediate frame delivery to render; render should not call decoder.

### Proposed Change

Add a `docs/architecture/DEPENDENCY_RULES.md` (or extend `ARCHITECTURE.md`) with the full graph above and enforce it via `cargo-deny` or a CI dependency lint in Phase 0.

---

## 4. Threading Model

### Documented Intent

| Thread / Actor | Responsibility |
|----------------|----------------|
| UI thread | Input, egui, state presentation |
| Media worker | Demux, decode |
| Playback scheduler | Frame scheduling, timing |
| GPU submission | Rendering |
| Audio thread | Real-time audio |
| Background workers | Thumbnails, waveforms, cache, AI, proxies |

### Assessment

The threading model is **correct in principle** but **entirely unvalidated**. No message types, channels, or ownership transfer contracts exist.

### Proposed Architecture

```text
┌─────────────┐     commands      ┌──────────────────┐
│  UI thread  │ ────────────────► │   dvs-core       │
│  (dvs-ui)   │ ◄──────────────── │   (state)        │
└─────────────┘   state snapshots └────────┬─────────┘
                                           │ control
                                           ▼
                                  ┌──────────────────┐
                                  │  dvs-playback    │
                                  │  (scheduler)     │
                                  └────────┬─────────┘
                          ┌────────────────┼────────────────┐
                          ▼                ▼                ▼
                   decode worker     render submit      audio thread
                   (dvs-decoder)     (dvs-gpu)         (dvs-audio)
```

### Key Decisions Required

| Decision | Options | Recommendation |
|----------|---------|----------------|
| Async runtime | `std::thread` + channels, `tokio`, dedicated thread pools | **Dedicated threads + bounded crossbeam channels** for media/GPU path; avoid async runtime on hot path initially |
| UI ↔ engine communication | Channels, actor model, shared state + snapshots | **Command channel in, immutable state snapshot out** per frame |
| Who owns the playback clock? | `dvs-playback` exclusively | **`dvs-playback`** — UI sends transport commands, never advances time directly |
| GPU context thread affinity | Same thread as render, or dedicated | **Dedicated render submission thread** bound to GPU context; validate with wgpu constraints |

### Risks

- Calling decode or GPU operations from the UI thread (must be prevented by API design).
- Unbounded channels causing memory growth during fast scrubbing.
- Clock drift between audio and video if not owned by a single scheduler.

---

## 5. Memory Ownership Model

### Documented Principles

- Avoid per-frame allocations.
- Use frame pools, texture pools, buffer reuse.
- Bounded queues.
- Explicit ownership.
- Reference counting where appropriate.

### Proposed Ownership Rules

| Resource | Owner | Lifetime | Transfer |
|----------|-------|----------|----------|
| `VideoFrame` (decoded) | `dvs-decoder` produces; `dvs-playback` schedules | Pool-backed, bounded queue | Move or `Arc` into render |
| GPU texture / surface | `dvs-gpu` | Texture pool | Opaque handle across decoder→render |
| Timeline state | `dvs-core` | Session lifetime | Immutable snapshots to UI |
| Cache entries | `dvs-cache` | LRU / disk-managed | Keyed by media ID + params hash |
| Command history (undo) | `dvs-core` | Session lifetime | Owned exclusively by core |

### Decisions Required

| Decision | Recommendation |
|----------|----------------|
| `Arc` vs move for frames | **Move** on hot path where possible; `Arc` only when multiple consumers need the same frame (e.g., cache + display) |
| Where do pools live? | **GPU pools in `dvs-gpu`**; **CPU frame pools in `dvs-decoder`** or `dvs-cache` |
| Queue depth | **Bounded**; playback drops or replaces stale frames rather than accumulating |

### Current State

No types, no pools, no queues. Correct for this phase — ownership rules should be defined in Phase 0 types before any frame flows.

---

## 6. VideoFrame Abstraction

### Documented Model

```text
VideoFrame
├── CpuFrame          (fallback path only)
├── D3D11Frame        (Windows HW decode)
├── D3D12Frame        (future)
├── MetalFrame        (macOS)
└── VulkanFrame       (future / cross-platform)
```

### Assessment

The abstraction direction is **correct and essential**. It must not be implemented as a fake placeholder. It should emerge from the Phase 1 D3D11VA investigation when real frame handles are available.

### Proposed Design (to implement in Phase 1)

| Layer | Location | Contents |
|-------|----------|----------|
| Domain frame metadata | `dvs-media` | `FrameId`, `PixelFormat`, `ColorSpace`, `TimeStamp`, dimensions |
| Platform frame payload | `dvs-decoder` (private) + public handle | Opaque per-platform types |
| GPU handle | `dvs-gpu` | `GpuTexture`, `GpuSurface`, pool refs |
| Public enum/trait | `dvs-media` or `dvs-gpu` | **Needs decision** — see below |

### Critical Decision: Where does `VideoFrame` live?

| Option | Pros | Cons |
|--------|------|------|
| `dvs-media` | Domain-level type; decoder and playback both use it | Must not pull in GPU platform deps |
| `dvs-gpu` | Close to actual GPU handles | Couples media domain to GPU crate |
| Shared `dvs-types` crate | Clean separation | Extra crate overhead |

**Recommendation:** Define **frame metadata and a platform-agnostic `VideoFrame` enum** in `dvs-media`. Platform-specific payloads are **opaque handles** whose concrete types live in `dvs-gpu` (for GPU frames) or `dvs-decoder` (for CPU fallback). Use newtype wrappers to avoid leaking D3D/Metal types into `dvs-core` or `dvs-playback`.

```rust
// Conceptual — not to be implemented in this phase
pub enum VideoFrame {
    Cpu(CpuFrame),
    Gpu(GpuFrameHandle),  // opaque; resolved only in dvs-gpu / dvs-render
}
```

### Rules

1. **CPU RGBA is a fallback**, not the default path.
2. `dvs-core` and `dvs-ui` must never see raw pixel buffers.
3. `dvs-playback` passes `VideoFrame` handles, not decoded pixels.
4. Conversions between platform formats happen in `dvs-gpu`, on GPU where possible.

---

## 7. GPU Abstraction

### Documented Intent

`dvs-gpu` owns wgpu, textures, pipelines, shaders, pools, and synchronization. `dvs-render` owns the render graph and compositing.

### Assessment

The split between **`dvs-gpu` (device/resources)** and **`dvs-render` (graph/compositor logic)** is correct and matches industry practice (analogous to a RHI vs render pipeline).

### What Must NOT Happen Now

- Do not create stub `GpuDevice`, `GpuTexture` structs with `todo!()`.
- Do not add wgpu until Phase 1 investigation begins.
- Do not claim a cross-platform abstraction before Windows D3D11VA interop is proven.

### Proposed Phased Approach

| Phase | `dvs-gpu` deliverable |
|-------|----------------------|
| Phase 0 | Capability detection types (adapter info, VRAM estimate) — may use wgpu minimally for enumeration only |
| Phase 1a | wgpu device/init, texture pool, platform interop spike (D3D11 shared handle) |
| Phase 1b | Import path for hardware-decoded frames |
| Phase 4+ | Full compositor shaders via `dvs-render` |

### Windows-Specific Note

Hardware-decoded D3D11 textures and wgpu (typically D3D12 or Vulkan backend) require **shared handle / external memory interop**. This is the highest-risk technical area and must be validated with a spike before designing the full `GpuFrame` API.

---

## 8. Media / Decoder Abstraction

### Documented Split

| `dvs-media` | `dvs-decoder` |
|-------------|---------------|
| Asset references | FFmpeg integration |
| Metadata / probe results | Decoder session lifecycle |
| Codec capabilities | HW decoder selection |
| Media format descriptions | Frame production |
| No FFmpeg dependency | Seeking implementation |

### Assessment

**Correct separation.** `dvs-media` is the stable interface; `dvs-decoder` is the replaceable implementation (FFmpeg today, potentially other backends in the distant future).

### Proposed Interface Shape (Phase 1)

```text
dvs-media::MediaAsset        — ID, path, probe metadata
dvs-media::MediaProbe        — codec, resolution, duration, stream info
dvs-media::DecoderCapabilities — what HW paths are available
dvs-decoder::DecoderSession  — open, decode, seek, close
dvs-decoder::DecodeError     — explicit errors
```

### Rules

1. Only `dvs-decoder` links FFmpeg.
2. `dvs-media` defines traits that `dvs-decoder` implements.
3. Probing may eventually use FFmpeg internally via `dvs-decoder` or a `dvs-probe` submodule — but the **public** probe API must remain in `dvs-media`. Avoid exposing `ffmpeg-next` types across crate boundaries.

### Risk

FFmpeg Rust bindings (`ffmpeg-next`, `ffmpeg-sys-next`) vary in HW acceleration support documentation. A spike is required to confirm D3D11VA frame export before committing to the `VideoFrame` enum shape.

---

## 9. Playback Architecture

### Documented Responsibilities

Play, pause, seek, scrub, frame queue, preroll, buffering, presentation timing, dropped-frame detection. Must not depend on egui.

### Proposed Design

```text
TransportCommand (from UI)
    → dvs-playback::PlaybackEngine
        → decodes via dvs-decoder
        → schedules frames in bounded queue
        → submits to dvs-render for presentation
        → reports PlaybackStatus (FPS, drops, buffer level)
```

### Scrubbing Strategy (from `PROJECT_CONTEXT.md`)

Scrubbing must be **separate from sequential playback**:

- Nearest-frame presentation from cache.
- Delayed accurate seek (debounced).
- Decode-forward after coarse seek.
- No blocking seek per mouse event.

### Assessment

Architecture is sound. **Not implementable until** `VideoFrame`, decoder sessions, and a presentation surface exist (Phase 1–2).

### Dependency Direction

`dvs-playback` → `dvs-decoder`, `dvs-render`, `dvs-cache`, `dvs-core`  
`dvs-playback` must NOT → `dvs-ui`, `egui`

---

## 10. Render Graph Architecture

### Documented Intent

`dvs-render` owns render graph, compositing, transforms, scaling, effects, color processing. Uses `dvs-gpu` for execution.

### Proposed Graph Model

```text
Source Node (GpuFrame / texture)
    → Transform Node (scale, crop, matrix)
    → Effect Node(s) (color, blur, …)
    → Composite Node (blend layers)
    → Output Node (viewport / export target)
```

### Assessment

A full render graph is **Phase 4** scope. For Phase 1, a **single-node passthrough** (decode → scale → display) is the correct vertical slice.

### Rules

1. Render graph nodes are **data descriptions**; GPU execution stays in `dvs-gpu`.
2. `dvs-render` must not import FFmpeg.
3. Export (offline) reuses the same graph via `dvs-export` with a different output target.

### Risk

Premature graph generalization before passthrough works. Implement passthrough first; generalize graph API after Phase 2 playback is stable.

---

## 11. Error Handling

### Documented Intent

Explicit error types: `MediaOpenError`, `DecoderError`, `HardwareAccelerationError`, `GpuError`, `RenderError`, `ExportError`. No silent failures.

### Current State

No error types exist anywhere in the codebase.

### Proposed Strategy

| Layer | Approach |
|-------|----------|
| All crates | `thiserror` for typed errors |
| Application boundary (`dvs-app`) | Optional `anyhow` for top-level reporting only |
| Cross-crate errors | Each crate defines its own error enum; no mega-enum in `dvs-core` |
| User-visible errors | `dvs-ui` maps errors to messages; never panics on media/GPU failure |

### Phase 0 Deliverable

Define empty error enums (or enums with initial variants) per crate in `lib.rs` — this is **not placeholder logic**, it is the contract. Example locations:

- `dvs-media::MediaError`
- `dvs-decoder::DecoderError`
- `dvs-gpu::GpuError`
- `dvs-render::RenderError`
- `dvs-playback::PlaybackError`

### Recommendation

Do **not** use `Box<dyn Error>` in internal APIs. Typed errors preserve context for HW fallback decisions (e.g., retry with software decode).

---

## 12. Performance Instrumentation

### Documented Intent (`PERFORMANCE.md`)

Measure FPS, frame time, decode time, queue time, GPU upload/render time, drops, CPU/GPU/RAM/VRAM utilization. Benchmark matrix across codec, resolution, FPS, tracks, effects. Performance gates defined (4K HEVC 30 FPS zero drops).

### Current State

No tracing, no metrics, no benchmark crates, no `criterion` setup.

### Proposed Infrastructure (Phase 0)

| Tool | Purpose |
|------|---------|
| `tracing` + `tracing-subscriber` | Structured logging and span timing |
| `criterion` (dev-dep) | Micro-benchmarks for decode/render hot paths |
| Custom `dvs-metrics` module (in `dvs-playback` or standalone) | Frame budget tracking, drop counters |
| Feature flag `metrics` | Optional overhead in release builds |

### Instrumentation Points (to add with implementation)

```text
decode: demux → packet → hw_transfer → frame_create
playback: schedule → queue_wait → present
render: upload → draw → composite → submit
```

### Assessment

Documentation is thorough. Instrumentation must be **co-designed** with the first real frame pipeline, not bolted on later. Phase 0 should add tracing infrastructure only; frame-level metrics come in Phase 1.

---

## 13. Windows GPU Strategy

### Documented Direction

- Primary target: Windows first.
- HW decode: D3D11VA initially.
- GPU API: wgpu.
- Also consider NVDEC/NVENC, Intel QSV, AMD AMF where appropriate.

### Proposed Implementation Sequence

| Step | Action |
|------|--------|
| 1 | Enumerate GPU adapters (wgpu or DXGI) — Phase 0 |
| 2 | FFmpeg D3D11VA decode to D3D11 texture — Phase 1 spike |
| 3 | Import D3D11 texture into wgpu via shared handle — Phase 1 spike |
| 4 | GPU color conversion / scaling shader — Phase 1 |
| 5 | Display in native viewport (not egui texture) — Phase 1 |
| 6 | NVDEC native path evaluation — Phase 1+ if D3D11VA insufficient |

### Critical Technical Path

```text
FFmpeg (D3D11VA)
    → ID3D11Texture2D
    → DXGI shared handle
    → wgpu ExternalTexture / backend-specific import
    → shader (NV12/P010 → RGB)
    → viewport swapchain
```

### Risks

| Risk | Severity | Mitigation |
|------|----------|------------|
| wgpu external memory support on Windows | **High** | Early spike; fallback to CPU copy path documented as degraded mode |
| NV12/P010 shader conversion cost | Medium | Measure; may be negligible vs CPU RGBA conversion |
| egui + separate viewport integration | **High** | Research `egui::Viewport` / raw window handle embedding early |
| Multi-GPU systems | Medium | Adapter selection logic in Phase 0/1 |

### Assessment

Strategy is correct. **D3D11VA first** is the right call for broad hardware support via FFmpeg. Zero-copy is a **goal**, not a guaranteed outcome of the first spike.

---

## 14. Future macOS GPU Strategy

### Documented Direction

- HW decode: VideoToolbox.
- GPU: Metal via wgpu backend.
- Frame path: `CVPixelBuffer` / `IOSurface` → Metal texture → wgpu.

### Proposed Approach

| Component | macOS path |
|-----------|------------|
| Decode | FFmpeg VideoToolbox or native VT API |
| Frame type | `MetalFrame` / `IOSurfaceHandle` in `VideoFrame` enum |
| GPU import | wgpu Metal backend texture from IOSurface |
| UI | Same egui shell; same separate viewport strategy |

### Platform Abstraction Rule

`dvs-gpu` should expose **capability-based APIs**, not a lowest-common-denominator fake unified texture type. Platform modules: `dvs-gpu/src/d3d11/`, `dvs-gpu/src/metal/` (behind `cfg` gates).

### Assessment

macOS is correctly deferred but **must influence** `VideoFrame` design now — the enum must accommodate both D3D11 and Metal handles without refactor.

---

## 15. Risks and Unresolved Technical Decisions

### High-Priority Risks

| # | Risk | Impact |
|---|------|--------|
| R1 | D3D11VA → wgpu interop may require CPU copy on some drivers | Core zero-copy promise at risk |
| R2 | egui viewport embedding for video panel | May block Phase 1 display |
| R3 | 13 crates with zero enforced dependencies | Architectural drift during early commits |
| R4 | FFmpeg HW frame export API complexity | Schedule risk for Phase 1 |
| R5 | Rust edition 2024 toolchain maturity | Build/CI compatibility |

### Unresolved Decisions

| # | Decision | Status | Blocking |
|---|----------|--------|----------|
| D1 | `VideoFrame` owning crate (`dvs-media` vs shared types) | **Recommended: `dvs-media`** | Phase 1 |
| D2 | wgpu backend on Windows (Vulkan vs DX12) | **Needs spike** | Phase 1 |
| D3 | Viewport strategy (child window vs texture blit) | **Unresolved** | Phase 1 |
| D4 | Channel library (`crossbeam-channel` vs `std::sync::mpsc`) | **Recommended: crossbeam bounded** | Phase 0 |
| D5 | Whether `dvs-decoder` depends on `dvs-gpu` for handle types | **Recommended: yes, one-way** | Phase 1 |
| D6 | Software decode fallback path design | **Unresolved** | Phase 1 |
| D7 | Audio crate dependency on decoder vs own FFmpeg link | **Recommended: share via `dvs-decoder`** | Phase 5 |
| D8 | Benchmark CI infrastructure | **Unresolved** | Phase 1 |
| D9 | `cargo-deny` / dependency lint in CI | **Recommended for Phase 0** | Phase 0 |
| D10 | Logging format and log levels for production | **Unresolved** | Phase 0 |

---

## Validation Checklist

| Item | Pass / Fail | Notes |
|------|-------------|-------|
| Crate list matches architecture | ✅ Pass | All 13 present |
| Dependency direction documented | ⚠️ Partial | Auxiliary crates missing from `ARCHITECTURE.md` |
| Dependencies enforced in Cargo.toml | ❌ Fail | Zero inter-crate deps |
| No forbidden deps added | ✅ Pass | No FFmpeg, wgpu, egui |
| Core independent from UI/media | ✅ Pass | No code violates this yet |
| VideoFrame abstraction designed | ⚠️ Partial | Conceptual only |
| Threading model defined | ⚠️ Partial | High-level only |
| Error model exists | ❌ Fail | Not started |
| Performance instrumentation exists | ❌ Fail | Not started |
| No fake GPU abstraction | ✅ Pass | No stubs created |
| No premature playback/timeline code | ✅ Pass | Only scaffold |

---

## Recommended Changes Before Phase 1

### Documentation

1. Fix `ARCHITECTURE.md` diagram/formatting.
2. Add complete dependency graph including all 13 crates.
3. Document `dvs-core` / `dvs-project` split explicitly.

### Workspace / Code Hygiene

4. Remove `cargo init` `add()` boilerplate from all crates.
5. Wire `Cargo.toml` dependencies per the graph in Section 3 (empty crate bodies are fine).
6. Add `cargo-deny` or equivalent dependency lint.
7. Populate `[workspace.dependencies]` for shared crates.

### Phase 0 Implementation Order

8. Error types per crate (`thiserror`).
9. `tracing` setup in `dvs-app`.
10. Core domain primitives in `dvs-core`: `TimeCode`, `FrameIndex`, `MediaId`, `ProjectId`.
11. GPU adapter enumeration spike (minimal wgpu for listing adapters only).
12. CI: `cargo check`, `cargo test`, `cargo clippy`, dependency lint.

---

## What Should Be Implemented Next

Per `ROADMAP.md` Phase 0, in order:

1. **Remove scaffold boilerplate** and wire crate dependencies.
2. **Error model** — typed errors in each crate.
3. **Logging** — `tracing` in `dvs-app`.
4. **Core primitives** — time, IDs in `dvs-core`.
5. **Dependency lint** — enforce acyclic graph in CI.
6. **GPU capability detection spike** — adapter enumeration only.
7. **Profiling infrastructure** — tracing spans, `criterion` workspace setup.

Phase 1 (after Phase 0 gate) begins with FFmpeg integration and the D3D11VA → wgpu interop spike. Do not start Phase 1 until D1, D2, D3 are at least spike-validated.

---

## Summary

### What is correct

- Thirteen-crate decomposition matches the documented architecture.
- Critical path (decode → frame → playback → render → GPU) is well defined.
- Separation of concerns between UI, core, media, decoder, GPU, and render is sound.
- No forbidden dependencies or premature implementations exist.
- Product documentation (`PROJECT_CONTEXT`, `PERFORMANCE`, `ROADMAP`) is thorough and consistent with architecture goals.
- GPU-first, no-CPU-RGBA-default principles are clearly stated and correct.

### What should change

- Remove `cargo init` boilerplate from all library crates.
- Enforce the dependency graph in `Cargo.toml` and CI before real code lands.
- Complete `ARCHITECTURE.md` with auxiliary crate dependencies and fix formatting.
- Clarify `dvs-core` vs `dvs-project` ownership split in documentation.
- Define `VideoFrame` ownership location (`dvs-media` recommended) before Phase 1.

### Unresolved decisions

- wgpu backend choice on Windows (Vulkan vs DX12).
- Video viewport embedding strategy with egui.
- Whether D3D11VA → wgpu can achieve zero-copy on target hardware.
- Software decode fallback API shape.
- Audio decoder sharing strategy (Phase 5).

### Next step

**Phase 0 foundation only:** error types, tracing, core primitives, dependency wiring, dependency lint, GPU adapter enumeration spike. No FFmpeg, no playback, no timeline, no fake abstractions.
