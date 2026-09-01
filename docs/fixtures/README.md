# GPU experiment fixtures

Experiment 2 requires a **real** hardware-decodable HEVC file placed by a developer. The repository does **not** ship video binaries. **Synthetic decoder output or auto-downloaded clips are not acceptable.**

---

## Required file

| Property | Requirement |
|----------|-------------|
| **Path** | `docs/fixtures/test_4k_hevc.mp4` |
| **Container** | MP4 (`.mp4`) |
| **Video codec** | HEVC / H.265 (`hevc`) |
| **Resolution** | **3840×2160** (4K UHD) |
| **Frame rate** | **~30 FPS** (e.g. `30000/1001` or `30/1`) |
| **Bit depth** | **8-bit** |
| **Pixel format** | **yuv420p** (8-bit 4:2:0) |
| **GOP structure** | Short enough for seek/decode testing — recommend **keyframe at least every 1–2 seconds** (e.g. GOP ≤ 60 at 30 FPS). Avoid all-intra-only files larger than necessary. |
| **Duration** | **5–30 seconds** recommended (enough for decode/interop validation without huge repo copies if ever allowed) |
| **Audio** | Optional; video stream is the requirement |

---

## Why these requirements

GPU Experiment 2 validates:

```text
real 4K HEVC MP4 → FFmpeg D3D11VA → GPU-resident D3D11 texture → wgpu
```

The file must be decodable by **FFmpeg D3D11VA** on Windows (AMD RX 580 / similar). A software-only or incompatible profile may cause false failures unrelated to interop.

---

## How to provide the fixture

1. Obtain or author a compliant clip (do not commit unless repository policy allows large binaries).
2. Place the file at:

```text
docs/fixtures/test_4k_hevc.mp4
```

3. Verify with `ffprobe` (requires FFmpeg runtime — see [FFMPEG_SETUP_WINDOWS.md](../ffmpeg/FFMPEG_SETUP_WINDOWS.md)).

---

## Verification with ffprobe

### Quick check (CSV)

```powershell
ffprobe -v error -select_streams v:0 `
  -show_entries stream=codec_name,width,height,r_frame_rate,pix_fmt,bit_depth `
  -of csv=p=0 `
  docs\fixtures\test_4k_hevc.mp4
```

**Expected output (example):**

```text
hevc,3840,2160,30/1,yuv420p,8
```

### Detailed check

```powershell
ffprobe -v error `
  -select_streams v:0 `
  -show_entries stream=codec_name,width,height,r_frame_rate,avg_frame_rate,pix_fmt,bit_depth,profile,level `
  -show_entries format=format_name,duration `
  -of default=noprint_wrappers=1 `
  docs\fixtures\test_4k_hevc.mp4
```

**Expected fields:**

```text
codec_name=hevc
width=3840
height=2160
r_frame_rate=30/1
pix_fmt=yuv420p
format_name=mov,mp4,m4a,3gp,3g2,mj2
```

### GOP / keyframe spacing (optional)

```powershell
ffprobe -v error -select_streams v:0 -show_frames `
  -show_entries frame=pict_type,pkt_pts_time `
  -of csv=p=0 `
  docs\fixtures\test_4k_hevc.mp4 | Select-String ",I"
```

Confirm **I-frames** appear at reasonable intervals (not only at frame 0).

---

## Hardware decode smoke test (after FFmpeg setup)

Requires `docs/fixtures/test_4k_hevc.mp4` and D3D11VA-enabled FFmpeg on `PATH`:

```powershell
ffmpeg -hide_banner `
  -hwaccel d3d11va -hwaccel_output_format d3d11 `
  -i docs\fixtures\test_4k_hevc.mp4 `
  -frames:v 10 -f null -
```

**Pass:** decode completes without fatal D3D11VA initialization errors.  
**Fail:** investigate FFmpeg build, GPU drivers, or fixture codec profile before running Experiment 2.

---

## FFmpeg development environment

Experiment 2 also requires FFmpeg **development libraries** (not just `ffmpeg.exe`). See:

**[docs/ffmpeg/FFMPEG_SETUP_WINDOWS.md](../ffmpeg/FFMPEG_SETUP_WINDOWS.md)**

Project-local layout:

```text
third_party/ffmpeg/
    include/
    lib/
    bin/
```

---

## Run Experiment 2 (when unblocked)

```powershell
cargo run --release -p gpu-d3d11-interop
```

The binary exits early with a clear status if the fixture or FFmpeg environment is incomplete.

---

## Status

| Item | Status |
|------|--------|
| `docs/fixtures/test_4k_hevc.mp4` | **MISSING** |
| Experiment 2 | **BLOCKED** |

Do not generate fake decoder results. Add the fixture manually, then complete FFmpeg setup before implementing the experiment.
