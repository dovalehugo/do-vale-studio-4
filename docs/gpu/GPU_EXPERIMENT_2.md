# Do Vale Studio 4 — GPU Experiment 2: D3D11VA → GPU-Resident wgpu Pipeline

**Date:** 2026-09-01  
**Machine:** Windows 10 22H2 (build 19045), AMD Ryzen 7 1700X, 32 GB RAM  
**GPU:** AMD Radeon (TM) RX 580 8 GB  
**Status:** **PASS** (architecture, automated validation, and human continuous-playback validation complete)
**Experiment package:** `tests/gpu_d3d11_interop`  
**Fixture:** `docs/fixtures/test_4k_hevc_8bit30.mp4` (HEVC Main 3840×2160 30000/1001)

---

## Final classification

| Phase | Status |
|-------|--------|
| Steps 1–32 (D3D11VA → shared fence) | **PASS** |
| Steps 33–36 (wgpu import + first render) | **PASS** |
| Steps 37–39 (90-frame wall-clock benchmark) | **PASS** |
| Corrective bidirectional fence sync | **PASS** |
| Human visual validation (`--visual`) | **PASS** |
| **Experiment 2 final** | **PASS** |

---

## Status history (do not conflate)

### Earlier partial result (pre-diagnostic)

After the first multi-frame corrective run, automated steps 37–39 passed and wall-clock throughput exceeded the fixture rate, but **`--visual` showed a solid green frame**. Human visual validation was **FAIL**. Final status was **PARTIAL** / **PASS EXCEPT HUMAN VISUAL VALIDATION**.

### Diagnostic phase

`--visual-diagnostic` isolated the failure:

| Test | Result |
|------|--------|
| DIAG 3 — Synthetic NV12 control | Colors visible (surface/shader/bindings OK) |
| DIAG 4 — Continuous real Plane0 | Black |
| DIAG 5 — Continuous real Plane1 | Black |
| DIAG 6 — Continuous real NV12 full | Solid green |
| TEST 7 — Frozen real import (no keyed mutex) | Real frame visible |
| TEST 8 — Frozen real import (keyed mutex) | Real frame visible |

**Conclusion:** The imported NV12 resource and both planes are valid. The continuous path failed because the single shared texture was reused by D3D11 before D3D12/wgpu finished sampling it. Missing **D3D12→D3D11 reverse synchronization** caused zero/stale plane contents (Y≈0 → BT.709 limited-range clamp → green).

**Keyed-mutex finding:** Missing `AcquireSync`/`ReleaseSync` participation was a real API-contract issue on `SHARED_NTHANDLE | SHARED_KEYEDMUTEX` textures, but it was **not** the differentiating cause — both frozen TEST 7 (no mutex) and TEST 8 (with mutex) displayed the real image.

### Final corrected result

Bidirectional shared timeline fence on the continuous path:

```text
D3D11 Wait(previous consumed)
→ AcquireSync(0)
→ CopySubresourceRegion
→ ReleaseSync(0)
→ D3D11 Signal(ready = 2N+1)
→ wgpu raw/present queue Wait(ready)
→ render / submit / present
→ wgpu raw/present queue Signal(consumed = 2N+2)
→ next D3D11 Wait(consumed)
```

Human validation (`cargo run -p gpu-d3d11-interop -- --visual`):

- Real HEVC video plays continuously
- Playback appears fluid
- Solid green frame resolved
- **Human visual validation: PASS**

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
  → shared GPU fence (bidirectional D3D11 ↔ wgpu timeline)
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

---

## 9. Keyed mutex

- D3D11: `IDXGIKeyedMutex` available on shareable texture
- D3D12: `QueryInterface` for `IDXGIKeyedMutex` on opened resource → **E_NOINTERFACE** (not used for D3D12-side mutex)
- **Continuous path:** D3D11 `AcquireSync(0, 5000)` → copy → `ReleaseSync(0)` (required by `SHARED_KEYEDMUTEX` contract)
- **Diagnostic:** TEST 7 (no mutex) and TEST 8 (with mutex) both displayed frozen real frames — mutex was not the frozen-vs-continuous differentiator

---

## 10. Shared GPU fence synchronization (final)

- `ID3D11Device5` + `ID3D11DeviceContext4`: available
- `ID3D11Fence` with `D3D11_FENCE_FLAG_SHARED`
- Shared NT fence HANDLE created once; D3D12/wgpu open **once** and retain `ID3D12Fence`
- **Step 32 bootstrap:** D3D11 `Signal(1)` + probe D3D12 queue `Wait(1)` (validation only)
- **Continuous path:** bidirectional timeline on wgpu-hal **raw/present queue only** (not the probe's separate D3D12 command queue)

Per frame N (0-based):

| Value | Formula |
|-------|---------|
| `ready` | `2N + 1` |
| `consumed` | `2N + 2` |
| D3D11 wait before reuse (N > 0) | `2N` (= previous `consumed`) |

GPU-side only; no CPU polling or `WaitForSingleObject` as proof.

---

## 11. wgpu-hal DX12 external resource mechanism

1. Pre-init wgpu DX12 with surface-based adapter selection
2. `device.as_hal::<Dx12>()` → `OpenSharedHandle` on texture + fence
3. `raw_queue().Wait` / `raw_queue().Signal` on cached fence
4. `texture_from_raw` + `create_texture_from_hal`

Imported and wgpu-wrapped `ID3D12Resource` pointers matched (same COM object).

---

## 12. NV12 plane access

| Plane | Representation |
|-------|----------------|
| Y | `TextureFormat::R8Unorm`, `TextureAspect::Plane0` |
| UV | `TextureFormat::Rg8Unorm`, `TextureAspect::Plane1` |

---

## 13. WGSL YUV → RGB

- Shader: `tests/gpu_d3d11_interop/shaders/nv12_to_rgb.wgsl`
- Color space: BT.709, limited range
- Visible crop: `2160/2176` excludes decoder padding

---

## 14. Multi-frame strategy

- **Single** bounded shareable D3D11 NV12 texture (reused)
- **Single** shared fence HANDLE + cached `ID3D12Fence`
- Measured run: exactly **90** decode → copy → sync → render → present cycles
- Monotonic fence timeline (`2N+1` / `2N+2`)
- **Serialization:** one texture enforces producer/consumer handoff; multi-buffering may be evaluated later (not implemented)

---

## 15. Visual validation

| Mode | Command | Purpose |
|------|---------|---------|
| Continuous playback | `cargo run -p gpu-d3d11-interop -- --visual` | Human validation (PASS) |
| Root-cause diagnostic | `cargo run -p gpu-d3d11-interop -- --visual-diagnostic` | Tests 1–8 (preserved) |

Window: 1280×720. Human confirmed: real moving video, fluid playback, no green corruption.

---

## 16. Final release benchmark metrics

**Command:** `cargo run --release -p gpu-d3d11-interop`
**Measurement type:** WALL-CLOCK END-TO-END THROUGHPUT (not GPU timestamp queries; not `--visual` refresh rate)

**Run date:** 2026-09-01 (final corrected build)

| Metric | Value |
|--------|-------|
| Frames attempted | 90 |
| Frames decoded | 90 |
| GPU copies | 90 |
| Frames rendered | 90 |
| Present calls | 90 |
| Dropped / failed frames | 0 |
| Total elapsed | **1.474 s** (1473.70 ms) |
| Average FPS (frames / elapsed) | **61.07** |
| Average frame time | **16.37 ms** |
| P50 / P95 / P99 frame time | Not implemented |
| Fixture FPS target | ~29.97 (30000/1001) |
| Throughput ≥ fixture rate | **YES** |
| Hardware decoder | D3D11VA → `AV_PIX_FMT_D3D11` |
| Adapter / GPU | AMD Radeon (TM) RX 580 (Dx12) |
| Pixel format | NV12 3840×2176 allocation, 2160 visible |
| Bidirectional timeline active | **YES** (diagnostics frames 0–4 logged) |
| Keyed-mutex failures | None |
| Fence failures | None |
| Fence values used (continuous) | 180 (`2 × 90`) |
| Cached fence OpenSharedHandle in loop | 0 |

**Phase breakdown (cumulative wall-clock):**

| Phase | ms |
|-------|-----|
| Decode | 75.00 |
| GPU copy | 23.06 |
| Sync | 26.91 |
| Render + present | 1369.75 |

*Earlier pre-sync-correct run reported ~60.44 FPS over 1.489 s; final corrected run: 61.07 FPS over 1.474 s. Do not treat either as isolated GPU execution time.*

---

## 17. Forbidden-path validation

| Check | Result |
|-------|--------|
| `av_hwframe_transfer_data` | **NOT USED** |
| swscale | **NOT USED** |
| CPU RGBA / YUV staging | **NOT USED** |
| GPU → CPU → GPU | **NO** |
| Software decode fallback | **NO** |
| Synthetic substitution in `--visual` | **NO** |
| GPU → GPU decoder-surface copy | **YES** |

---

## 18. Limitations

1. **Single shared texture:** Producer and consumer are serialized per frame; throughput ceiling may improve with multi-buffering (future work).
2. **Adapter init ordering:** Empirical on Windows 10 + RX 580; not proven as a universal DXGI rule.
3. **Unsafe HAL interop:** `create_texture_from_hal` requires documented lifetime/sync invariants in production.
4. **Decoder surface copy:** One GPU copy per frame from non-shareable decoder texture to shareable NV12.
5. **D3D12 keyed mutex:** Not available on opened shared NV12; D3D11-side mutex used for producer contract only.
6. **Wall-clock FPS** includes present and CPU submission, not isolated GPU timestamps.
7. **No per-frame percentile stats** in the benchmark harness.

---

## 19. Future work

- Integrate validated path into `dvs-gpu` / `dvs-render`
- Evaluate multi-buffered shared textures (remove single-texture serialization)
- Full 4K viewport scaling
- Production fence/frame pool scheduling
- Optional Vulkan interop path

---

## 20. Files

| Path | Role |
|------|------|
| `tests/gpu_d3d11_interop/src/main.rs` | Steps 1–32 probe, keyed-mutex copy, frozen import diagnostics |
| `tests/gpu_d3d11_interop/src/wgpu_hal_interop.rs` | Step 33 import; wgpu queue Wait/Signal |
| `tests/gpu_d3d11_interop/src/render_path.rs` | Steps 34–36 render |
| `tests/gpu_d3d11_interop/src/multi_frame.rs` | Steps 37–39; `ContinuousFramebufferTimeline` |
| `tests/gpu_d3d11_interop/src/visual_validation.rs` | `--visual` human validation |
| `tests/gpu_d3d11_interop/src/visual_diagnostic.rs` | `--visual-diagnostic` tests 1–8 |
| `tests/gpu_d3d11_interop/shaders/nv12_to_rgb.wgsl` | BT.709 shader with 2160/2176 crop |

---

## 21. Step summary

| Step | Result |
|------|--------|
| 1–32 | PASS |
| 33 | PASS |
| 34 | PASS |
| 35 | PASS |
| 36 | PASS |
| 37 | PASS (90 real frames, bidirectional sync) |
| 38 | PASS (cached fence; bounded reuse) |
| 39 | PASS (61.07 wall-clock FPS ≥ 29.97) |
| 40 | PASS (documentation + human visual validation) |
