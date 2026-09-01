# DO VALE STUDIO 4 — HANDOFF
## GPU Experiment 2 — COMPLETE

> **Final status: PASS** (2026-09-01)
> Human continuous-playback validation confirmed.
> Continue from production integration planning — do not re-run steps 1–40 unless regressing.

---

# 1. FINAL STATUS

```text
Experiment 2: 40/40 — PASS
Architecture integrity: PASS
Technical interop chain: PASS
Human visual validation: PASS (--visual)
Release benchmark (90 frames): PASS
```

---

# 2. WHAT WAS PROVEN

```text
Real 4K HEVC
→ FFmpeg D3D11VA (AV_PIX_FMT_D3D11)
→ ID3D11Texture2D NV12 decoder surface
→ GPU CopySubresourceRegion (keyed-mutex guarded)
→ shareable D3D11 NV12 (SHARED_NTHANDLE | SHARED_KEYEDMUTEX)
→ NT HANDLE → ID3D12Resource
→ bidirectional shared GPU fence (D3D11 ↔ wgpu raw/present queue)
→ wgpu-hal imported NV12 → Plane0/Plane1 views
→ WGSL BT.709 limited → surface present
```

**GPU-resident.** Not zero-copy (one GPU→GPU copy per frame).

---

# 3. GREEN FRAME — RESOLVED

## Original symptom

`--visual` showed solid green; continuous real NV12 path failed visually while automated API present succeeded.

## Diagnostic results

| Test | Outcome |
|------|---------|
| Synthetic NV12 (DIAG 3) | Colors visible |
| Continuous Plane0 / Plane1 | Black |
| Continuous full NV12 | Solid green |
| Frozen TEST 7 (no keyed mutex) | Real frame visible |
| Frozen TEST 8 (keyed mutex) | Real frame visible |

## Root cause

Single shared NV12 texture reused by D3D11 before D3D12/wgpu finished sampling. Missing **D3D12→D3D11 reverse sync** (`Signal(consumed)` on wgpu queue + `Wait(consumed)` on D3D11 before next copy).

## Fix

Bidirectional timeline fence per frame N:

```text
ready = 2N+1, consumed = 2N+2

D3D11 Wait(prev consumed) → AcquireSync → Copy → ReleaseSync
→ D3D11 Signal(ready)
→ wgpu Wait(ready) → render/present → wgpu Signal(consumed)
```

Keyed mutex: required API contract on D3D11 producer; not the frozen-vs-continuous differentiator.

---

# 4. FINAL BENCHMARK (release)

**Command:** `cargo run --release -p gpu-d3d11-interop`

| Metric | Value |
|--------|-------|
| Frames | 90 decoded / 90 copied / 90 rendered / 90 presented |
| Elapsed | 1.474 s |
| Wall-clock FPS | 61.07 |
| Fixture FPS | ~29.97 |
| Throughput ≥ fixture | YES |
| Fence failures | None |
| Keyed-mutex failures | None |

Full detail: `docs/gpu/GPU_EXPERIMENT_2.md` §16.

---

# 5. HUMAN VALIDATION

```powershell
cargo run -p gpu-d3d11-interop -- --visual
```

Confirmed: real HEVC plays continuously, fluid, no green frame.

Diagnostics preserved:

```powershell
cargo run -p gpu-d3d11-interop -- --visual-diagnostic
```

Keys 1–8 unchanged.

---

# 6. PROHIBITIONS (still in force for this experiment)

No `av_hwframe_transfer_data`, swscale, CPU RGBA/YUV staging, Map/readback, software decode, or synthetic substitution in the normal path.

---

# 7. ARCHITECTURE NOTES FOR PRODUCTION

- Single shared texture **serializes** producer/consumer; multi-buffering is future work.
- wgpu Wait/Signal only on **raw/present queue** — not a separate probe D3D12 queue.
- Production crates were **not** modified during Experiment 2.

---

# 8. NEXT STEPS

1. Plan `dvs-gpu` / `dvs-render` integration using validated sync contract.
2. Evaluate multi-buffered shared textures for throughput.
3. Do **not** declare Experiment 2 incomplete — it is **PASS**.

---

# 9. KEY FILES

- `docs/gpu/GPU_EXPERIMENT_2.md` — authoritative experiment record
- `tests/gpu_d3d11_interop/` — isolated experiment crate (unchanged architecture)
