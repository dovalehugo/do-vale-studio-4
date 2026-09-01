# Do Vale Studio 4 — GPU Probe Results (Experiment 0)

**Date:** 2026-09-01  
**Machine:** Windows 10 (build 19045)  
**Probe tool:** `tests/gpu_probe` (isolated, not part of production crates)  
**wgpu version:** 27.0.1

## Reproduce

```bash
cargo run --release -p gpu-probe
```

From the repository root: `c:\proyectos-cursor\do-vale-studio-4`

---

## 1. Hardware detected

| Adapter | Name | Vendor | Device ID | Type | Backend |
|---------|------|--------|-----------|------|---------|
| #0 | AMD Radeon (TM) RX 580 | AMD (0x1002) | 0x67DF | DiscreteGpu | **Vulkan** |
| #1 | AMD Radeon (TM) RX 580 | AMD (0x1002) | 0x67DF | DiscreteGpu | **DX12** |
| #2 | *(probe failed)* | — | — | — | *(driver panic)* |
| #3 | AMD Radeon (TM) RX 580 | Unknown | 0x0000 | Other | **OpenGL** |

**Primary GPU:** AMD Radeon RX 580 (discrete).

Adapter #2 triggered a `wgpu-hal` DX12 internal panic (`HRESULT 0x80004005`) during device probing. This appears to be a duplicate or secondary DX12 enumeration path on the same hardware. The probe recovered and continued.

---

## 2. Graphics backend detected

Backends requested: `DX12 | VULKAN | GL`

| Backend | Status | Suitable for video pipeline? |
|---------|--------|------------------------------|
| **Vulkan** | Device opens OK | **Yes** — NV12/P010 usable |
| **DX12** | Device opens OK | **Yes** — NV12/P010 usable (primary interop target per spike) |
| **OpenGL** | Device opens OK | **No** — NV12/P010 not supported |

**Driver info:**

- Vulkan: AMD proprietary driver 25.8.1 (AMD proprietary shader compiler)
- DX12: driver version 31.0.21923.11000
- OpenGL: 4.6.0 Compatibility Profile Context 25.8.1.250617

---

## 3. NV12 support

| Adapter | Backend | Adapter reports `TEXTURE_FORMAT_NV12` | Device texture + plane views | Classification |
|---------|---------|--------------------------------------|------------------------------|----------------|
| #0 | Vulkan | Yes | Created successfully (Y=R8Unorm, UV=Rg8Unorm) | **USABLE FOR REQUIRED PIPELINE** |
| #1 | DX12 | Yes | Created successfully | **USABLE FOR REQUIRED PIPELINE** |
| #2 | — | — | Probe failed | **NOT VALIDATED** |
| #3 | OpenGL | No | Not supported | **NOT SUPPORTED** |

**Note:** Device-level validation confirms NV12 textures can be created with `TEXTURE_BINDING` and multi-planar plane views can be created. This validates the GPU YUV→RGB shader path at the wgpu API level. It does **not** validate D3D11VA external texture import (Experiment 2).

---

## 4. P010 support

| Adapter | Backend | Adapter reports `TEXTURE_FORMAT_P010` | Device texture + plane views | Classification |
|---------|---------|--------------------------------------|------------------------------|----------------|
| #0 | Vulkan | Yes | Created successfully (Y=R16Unorm, UV=Rg16Unorm) | **USABLE FOR REQUIRED PIPELINE** |
| #1 | DX12 | Yes | Created successfully | **USABLE FOR REQUIRED PIPELINE** |
| #2 | — | — | Probe failed | **NOT VALIDATED** |
| #3 | OpenGL | No | Not supported | **NOT SUPPORTED** |

P010 requires `TEXTURE_FORMAT_P010` and `TEXTURE_FORMAT_16BIT_NORM` on the device.

---

## 5. Other format probes

| Format | Vulkan (#0) | DX12 (#1) | OpenGL (#3) | Required usages |
|--------|---------------|-----------|-------------|-----------------|
| RGBA8Unorm | USABLE | USABLE | USABLE | TEXTURE_BINDING \| RENDER_ATTACHMENT |
| BGRA8Unorm | USABLE | USABLE | USABLE | TEXTURE_BINDING \| RENDER_ATTACHMENT |
| RGBA16Float | USABLE | USABLE | USABLE | TEXTURE_BINDING \| RENDER_ATTACHMENT |

All standard compositor output formats are usable on Vulkan and DX12.

---

## 6. Relevant limits (Vulkan #0 / DX12 #1)

| Limit | Vulkan | DX12 |
|-------|--------|------|
| Max 2D texture dimension | 16384 | 16384 |
| Max bind groups | 8 | 8 |
| Max sampled textures / shader stage | 4294967295 (unlimited flag) | 1048576 |
| Max storage textures / shader stage | 4294967295 (unlimited flag) | 262144 |

4K (3840×2160) and 6K (~6144×3456) are well within texture dimension limits.

---

## 7. Relevant limitations

| Limitation | Impact |
|------------|--------|
| OpenGL backend has no NV12/P010 | GL must be excluded from the video render path |
| Adapter #2 DX12 panic | Secondary DX12 adapter enumeration is unreliable on this machine; adapter selection must be explicit |
| No external texture import tested | D3D11VA → wgpu interop remains **UNVALIDATED** (Experiment 2) |
| `EXTERNAL_TEXTURE` feature not reported on Vulkan/DX12 adapters | Public `ExternalTexture` API may not apply; HAL import path still required per spike |
| Probe uses synthetic textures, not decoded frames | Format usability ≠ decode interop usability |
| wgpu 27 required for P010 + 16-bit norm features | Pin wgpu version in production when adopted |

---

## 8. What this means for Do Vale Studio 4

### Confirmed on this hardware

1. **AMD RX 580 supports NV12 and P010** through wgpu on both **Vulkan** and **DX12** backends.
2. **GPU-side YUV processing is viable** — multi-planar textures and plane views work at device level.
3. **RGBA/BGRA/RGBA16F render targets** are usable for compositor output on Vulkan and DX12.
4. **Texture dimension limits** are not a blocker for 4K or 6K workflows.
5. **DX12 and Vulkan are both viable** wgpu backends on this machine for the render portion of the pipeline.

### Not confirmed yet

1. D3D11 shared handle import into wgpu (the critical interop step from the spike).
2. Which backend performs better for D3D11VA interop (DX12 recommended by spike, but not benchmarked).
3. Whether `DXGI_FORMAT_420_OPAQUE` FFmpeg decode output can be imported without conversion.
4. Fence synchronization between decode and render queues.

### Recommended backend for Experiment 2

**DX12 first** (per `GPU_ARCHITECTURE_SPIKE.md`), with **Vulkan as fallback** since both report full NV12/P010 support on this GPU.

OpenGL must not be used for the video pipeline.

---

## Raw probe output

```
================================================================
 Do Vale Studio 4 — GPU Capability Probe (Experiment 0)
================================================================

wgpu dependency version: 27
Backends requested: DX12 | VULKAN | GL
Adapters found: 4

----------------------------------------------------------------
Adapter #0
----------------------------------------------------------------
  Name:         AMD Radeon (TM) RX 580
  Vendor:       AMD (0x1002)
  Device ID:    0x67DF
  Backend:      Vulkan
  Device type:  DiscreteGpu
  Driver:       AMD proprietary driver
  Driver info:  25.8.1 (AMD proprietary shader compiler)
  Relevant features: TEXTURE_FORMAT_NV12, TEXTURE_FORMAT_P010, TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES, TEXTURE_FORMAT_16BIT_NORM
  NV12 feature: reported by adapter
  P010 feature: reported by adapter
  Max 2D tex:   16384 x 16384
  Max bind groups: 8
  Max sampled tex/stage: 4294967295
  Max storage tex/stage: 4294967295
  Device open:  OK

  Format probes:
    [NV12]
      Result: USABLE FOR REQUIRED PIPELINE
      Adapter feature `TEXTURE_FORMAT_NV12`: present
      Adapter format features: allowed_usages=COPY_SRC | COPY_DST | TEXTURE_BINDING; flags=TextureFormatFeatureFlags(MULTISAMPLE_X2 | MULTISAMPLE_X4 | MULTISAMPLE_X8 | MULTISAMPLE_RESOLVE)
    [P010]
      Result: USABLE FOR REQUIRED PIPELINE
      Adapter feature `TEXTURE_FORMAT_P010`: present
      Adapter format features: allowed_usages=COPY_SRC | COPY_DST | TEXTURE_BINDING; flags=TextureFormatFeatureFlags(MULTISAMPLE_X2 | MULTISAMPLE_X4 | MULTISAMPLE_X8 | MULTISAMPLE_RESOLVE)
    [RGBA8Unorm]
      Result: USABLE FOR REQUIRED PIPELINE
    [BGRA8Unorm]
      Result: USABLE FOR REQUIRED PIPELINE
    [RGBA16Float]
      Result: USABLE FOR REQUIRED PIPELINE

----------------------------------------------------------------
Adapter #1
----------------------------------------------------------------
  Name:         AMD Radeon (TM) RX 580
  Vendor:       AMD (0x1002)
  Device ID:    0x67DF
  Backend:      DX12
  Device type:  DiscreteGpu
  Driver:       31.0.21923.11000
  Relevant features: TEXTURE_FORMAT_NV12, TEXTURE_FORMAT_P010, TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES, TEXTURE_FORMAT_16BIT_NORM
  NV12 feature: reported by adapter
  P010 feature: reported by adapter
  Max 2D tex:   16384 x 16384
  Device open:  OK

  Format probes:
    [NV12]  Result: USABLE FOR REQUIRED PIPELINE
    [P010]  Result: USABLE FOR REQUIRED PIPELINE
    [RGBA8Unorm]  Result: USABLE FOR REQUIRED PIPELINE
    [BGRA8Unorm]  Result: USABLE FOR REQUIRED PIPELINE
    [RGBA16Float]  Result: USABLE FOR REQUIRED PIPELINE

----------------------------------------------------------------
Adapter #2
----------------------------------------------------------------
  Name:         <probe failed>
  Device open:  FAILED — panic while probing adapter (driver/wgpu-hal failure)

----------------------------------------------------------------
Adapter #3
----------------------------------------------------------------
  Name:         AMD Radeon (TM) RX 580
  Backend:      OpenGL
  NV12 feature: NOT reported
  P010 feature: NOT reported
  [NV12]  Result: NOT SUPPORTED
  [P010]  Result: NOT SUPPORTED
  [RGBA8Unorm]  Result: USABLE FOR REQUIRED PIPELINE
  [BGRA8Unorm]  Result: USABLE FOR REQUIRED PIPELINE
  [RGBA16Float]  Result: USABLE FOR REQUIRED PIPELINE

================================================================
```

---

## Classification legend

| Label | Meaning |
|-------|---------|
| **USABLE FOR REQUIRED PIPELINE** | Adapter reports feature, format features allow required usages, device texture + views created successfully |
| **SUPPORTED (adapter only)** | Adapter reports capability but device validation was not performed |
| **NOT VALIDATED** | Probe could not complete (device open failure or driver panic) |
| **NOT SUPPORTED** | Adapter does not report feature or format usages insufficient |
