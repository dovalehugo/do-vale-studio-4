# Do Vale Studio 4 — Roadmap

## Phase 0 — Foundation

- [x] Repository
- [x] Workspace
- [x] Architecture
- [x] Integration 0 — acyclic internal dependency wiring (`docs/architecture/GPU_PRODUCTION_INTEGRATION_PLAN.md` §4)
- [ ] Error model
- [ ] Logging
- [ ] Profiling infrastructure
- [ ] GPU capability detection

---

## Phase 1 — GPU + Media Foundation

> **Experiment status (2026-09-01):** GPU Experiments 0–2 PASS. The Windows D3D11VA → wgpu DX12 GPU-resident path is validated in `tests/gpu_d3d11_interop`. Production integration is planned in `docs/architecture/GPU_PRODUCTION_INTEGRATION_PLAN.md` — **not yet in `crates/`**.

- [x] GPU capability detection (Experiment 0 — `tests/gpu_probe`)
- [x] GPU texture pipeline / NV12 shader path (Experiment 1 — `tests/gpu_nv12`)
- [x] Windows D3D11VA → wgpu interop (Experiment 2 — `tests/gpu_d3d11_interop`)
- [x] Integration 0 — dependency graph wiring (compile-time only; no production API)
- [x] Integration 2 — `dvs-gpu` safe DX12 context, adapter identity, `FenceTimeline`
- [x] Integration 3A — exact DXGI adapter LUID HAL extraction (`dvs-gpu`; compilation-verified; runtime via `GpuBootstrap` pending)
- [x] Integration 3B — D3D11 shared NV12 producer (`WindowsD3d11SharedNv12Producer`; hardware-validated)
- [x] Integration 3C — D3D12/wgpu consumer (`WindowsD3d11WgpuInteropBridge`, `GpuVideoFrame`; hardware-validated)
- [x] Integration 3 — D3D11/D3D12 interop bridge (producer + consumer; decoder/render wiring pending)
- [x] Integration 4A — `dvs-decoder` FFmpeg D3D11VA session (hardware ignored test PASS; borrowed D3D11 surfaces via production API; no CPU readback/fallback/copy/bridge/render)
- [x] Integration 4B — real decoded D3D11 surfaces → production interop bridge (`windows_d3d11va_interop` 90-frame hardware PASS; GPU-only copy; no rendering/CPU readback)
- [x] Integration 4 — complete (4A + 4B)
- [x] Integration 5 — `dvs-render` NV12 WGSL renderer (**COMPLETE** — automated 90/90 PASS; initial human visual FAIL on transformed oversized-triangle geometry; regression correction applied; repeated human visual PASS; recognizable complete real frame; no diagonal/streak artifacts; SDR-only; no playback/audio)
- [ ] Production crate extraction (Integrations 6–8)
- [x] FFmpeg integration (`dvs-decoder`) — decode + interop bridge validated; no playback/app wiring
- [ ] Media probing (`dvs-media`)
- [ ] Hardware decoder detection (production)
- [x] VideoFrame / metadata abstraction (`dvs-media`)
- [x] GPU frame abstraction (`dvs-gpu`) — interop bridge + NV12 plane views complete
- [x] GPU scaling (production viewport) — Integration 5 renderer implements aspect-fit letterbox
- [ ] Native video viewport (`dvs-app`)

SUCCESS CRITERION:

4K HEVC hardware decoded and rendered without mandatory
GPU → CPU → GPU frame conversion.

**Validated in experiment** (commit `a5fdb42`). **Production render path validated** through Integration 5 (automated 90/90 + human visual PASS); continuous playback remains Integration 6.

---

## Phase 2 — Playback

- [ ] Play
- [ ] Pause
- [ ] Seek
- [ ] Scrub
- [ ] Frame queue
- [ ] Preroll
- [ ] Presentation clock
- [ ] Dropped frame detection
- [ ] A/V timing foundation

---

## Phase 3 — Timeline

- [ ] Multiple video tracks
- [ ] Multiple audio tracks
- [ ] Clips
- [ ] Trim
- [ ] Split
- [ ] Move
- [ ] Snap
- [ ] Linked clips
- [ ] Keyframes
- [ ] Undo/redo

---

## Phase 4 — GPU Compositor

- [ ] Multiple tracks
- [ ] Alpha
- [ ] Transform
- [ ] Crop
- [ ] Blend modes
- [ ] Masks
- [ ] Adjustment layers

---

## Phase 5 — Audio

- [ ] Audio decode
- [ ] Audio playback
- [ ] A/V synchronization
- [ ] Waveforms
- [ ] Mixer
- [ ] Volume
- [ ] Pan
- [ ] Fades
- [ ] Effects

---

## Phase 6 — Effects

- [ ] GPU shader system
- [ ] Color correction
- [ ] LUT
- [ ] Blur
- [ ] Sharpen
- [ ] Transitions
- [ ] Text
- [ ] Keyframed effects

---

## Phase 7 — Export

- [ ] Render pipeline
- [ ] H.264
- [ ] HEVC
- [ ] AV1
- [ ] Hardware encoding
- [ ] Export queue
- [ ] Progress
- [ ] Cancellation

---

## Phase 8 — Project System

- [ ] Project files
- [ ] Autosave
- [ ] Recovery
- [ ] Cache
- [ ] Proxy management
- [ ] Media relinking

---

## Phase 9 — AI

- [ ] Provider abstraction
- [ ] Transcription
- [ ] Subtitles
- [ ] Silence detection
- [ ] Scene detection
- [ ] Semantic search
- [ ] AI commands
- [ ] AI editing assistant

---

## Phase 10 — Plugins

- [ ] Plugin API
- [ ] Video effects
- [ ] Audio effects
- [ ] Generators
- [ ] VST3 integration