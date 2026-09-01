# FFmpeg Development Setup — Windows

**Project:** Do Vale Studio 4  
**Purpose:** Reproducible FFmpeg **development** layout for Rust FFI, D3D11VA hardware decode, and GPU Experiment 2.  
**Status:** Setup guide — not yet applied on this machine.

---

## Overview

Do Vale Studio 4 needs **two distinct FFmpeg artifacts** on Windows:

| Artifact | What it is | Used for |
|----------|------------|----------|
| **Development libraries** | `.lib` import libraries + C headers (`include/`) | **Rust compile time** — `ffmpeg-sys-next` / `bindgen` |
| **Runtime binaries** | `.dll` + `ffmpeg.exe` / `ffprobe.exe` (`bin/`) | **Runtime** — loading DLLs, manual verification, fixture probing |

Having `ffmpeg.exe` on `PATH` does **not** mean Rust can link FFmpeg.  
Having `FFMPEG_DIR` set does **not** mean CLI tools work unless `bin/` is on `PATH`.

Both must be configured deliberately.

---

## 1. Required FFmpeg version

| Requirement | Value |
|-------------|-------|
| Minimum major version | **FFmpeg 6.0** |
| Recommended | **FFmpeg 6.1.x** or **7.x** (latest stable shared MSVC build) |
| ABI | **MSVC x64** (matches Rust `x86_64-pc-windows-msvc` default) |
| Architecture | **64-bit only** |

**Why not MinGW/MSYS2 builds:** The default Rust Windows toolchain is MSVC. Import libraries (`.lib`) and DLL naming must match the MSVC toolchain used by `ffmpeg-sys-next`.

**Version pinning:** Record the exact build used in your local notes (e.g. `ffmpeg-7.1-full_build-shared`). Experiment 2 documentation should cite the version once validated.

---

## 2. Required libraries

### Rust FFI link libraries (MSVC `.lib`)

These must exist under `%FFMPEG_DIR%\lib\`:

| Library | Role | Required for Experiment 2 |
|---------|------|-------------------------|
| `avcodec.lib` | Decoders, D3D11VA hwaccel, `AV_PIX_FMT_D3D11` | **Yes** |
| `avformat.lib` | Open MP4, read packets | **Yes** |
| `avutil.lib` | Common types, `AVFrame`, pixel formats, hw frames | **Yes** |
| `avfilter.lib` | Filter graph (linked by `ffmpeg-next` even if unused) | **Yes** (link-time) |
| `swscale.lib` | Software color conversion | **Yes** (link-time; **must not** be used on Experiment 2 render path) |
| `swresample.lib` | Audio resampling | **Yes** (link-time; video-only experiment does not call it) |
| `avdevice.lib` | Device I/O | Optional |

### Runtime DLLs (shared build)

Matching DLLs must exist under `%FFMPEG_DIR%\bin\` (names vary by version):

```text
avcodec-61.dll      (or avcodec-60.dll, avcodec-62.dll — version suffix varies)
avformat-61.dll
avutil-59.dll
avfilter-10.dll
swscale-8.dll
swresample-5.dll
```

DLL major version numbers change between FFmpeg releases. Verify the actual filenames in your `bin/` folder.

### D3D11VA-specific requirement

The FFmpeg build must be compiled **with D3D11VA support enabled**. This is present in standard **"full"** Windows shared builds from reputable sources (see §6). A minimal/GPL-only build without hardware acceleration is **not sufficient**.

---

## 3. Required headers

Headers must exist under `%FFMPEG_DIR%\include\`:

```text
include/
    libavcodec/
        avcodec.h
        d3d11va.h          ← required for D3D11VA types
        dxva2.h            ← often present alongside D3D11VA
    libavformat/
        avformat.h
    libavutil/
        avutil.h
        hwcontext.h        ← required for AVHWFramesContext / GPU frames
        hwcontext_d3d11va.h
        pixfmt.h
    libswscale/
        swscale.h
    libswresample/
        swresample.h
    libavfilter/
        avfilter.h
```

`ffmpeg-sys-next` uses **bindgen** (via **libclang**) to parse these headers at compile time. Missing headers or missing `LIBCLANG_PATH` causes build failures unrelated to FFmpeg itself.

---

## 4. Required DLLs

For **running** any Rust binary linked against shared FFmpeg:

1. Add `%FFMPEG_DIR%\bin` to the process `PATH` (user or session environment variable).
2. Alternatively, copy required DLLs next to the built executable (`target\release\gpu-d3d11-interop.exe`).

**Minimum DLL set** (shared build): all `av*.dll`, `swscale-*.dll`, `swresample-*.dll` that correspond to the linked `.lib` files.

**CLI tools** in the same `bin/` folder:

| Tool | Purpose |
|------|---------|
| `ffmpeg.exe` | Transcode, hwaccel smoke tests |
| `ffprobe.exe` | Verify fixture format |

These are **runtime verification tools**, not Rust link inputs.

---

## 5. Required D3D11VA support

Experiment 2 requires hardware decode to produce **GPU-resident** `AV_PIX_FMT_D3D11` frames.

The FFmpeg build must include:

- `--enable-d3d11va` (or equivalent in prebuilt "full" package)
- HEVC decoder: `hevc` / `hevc_cuvid` not required — standard `hevc` software registration plus D3D11VA hwaccel wrapper

### Verify D3D11VA at runtime (CLI)

After `bin/` is on `PATH`:

```powershell
ffmpeg -hide_banner -hwaccels
```

**Expected:** list includes `d3d11va`.

```powershell
ffmpeg -hide_banner -hwaccel d3d11va -hwaccel_output_format d3d11 -c:v hevc -i docs\fixtures\test_4k_hevc.mp4 -frames:v 1 -f null -
```

**Expected:** no fatal hwaccel initialization error; decode proceeds (once fixture exists).

### Verify D3D11VA in linked libraries (dev)

After Rust FFI is wired (Experiment 2 implementation phase), the code will call:

- `av_hwdevice_ctx_create(..., AV_HWDEVICE_TYPE_D3D11VA, ...)`
- `av_hwframe_transfer_data` must **not** be called on the render path

At this setup stage, CLI verification is sufficient.

---

## 6. Project-local layout and acquisition

### Target directory structure

```text
<repo-root>/third_party/ffmpeg/
    include/
    lib/
    bin/
```

This path is **gitignored** (see root `.gitignore`). Do not commit FFmpeg binaries unless repository policy changes.

### Recommended source: gyan.dev full shared MSVC build

1. Download **release full** shared build:  
   https://www.gyan.dev/ffmpeg/builds/  
   File pattern: `ffmpeg-release-full-shared.7z` (or versioned equivalent, e.g. `ffmpeg-7.1-full_build-shared.zip`).

2. Extract the archive. The top-level folder typically contains `bin/`, `include/`, `lib/` directly.

3. Copy or junction into the project:

```powershell
# From repository root — adjust source path to your download location
$src = "C:\Downloads\ffmpeg-7.1-full_build-shared"
$dst = "third_party\ffmpeg"

New-Item -ItemType Directory -Force -Path $dst | Out-Null
Copy-Item -Recurse -Force "$src\bin"  "$dst\bin"
Copy-Item -Recurse -Force "$src\include" "$dst\include"
Copy-Item -Recurse -Force "$src\lib"   "$dst\lib"
```

### Alternative: build from source

Use [ffmpeg-windows-build-helpers](https://github.com/rdp/ffmpeg-windows-build-helpers) or MSYS2 only if you need custom flags. Ensure:

- `--enable-shared`
- `--enable-d3d11va`
- `--toolchain=msvc` (or install into MSVC-compatible layout)
- x86_64 target

Building from source is slower but gives full control over enabled features.

### Alternative: vcpkg

```powershell
vcpkg install ffmpeg:x64-windows
```

Set `VCPKG_ROOT` and ensure `ffmpeg-sys-next` can find the vcpkg installed tree, **or** copy the vcpkg `installed\x64-windows\` include/lib/bin layout into `third_party/ffmpeg/`. vcpkg FFmpeg builds usually include D3D11VA on Windows.

---

## 7. Environment configuration

### Required variables

| Variable | Value (example) | Purpose |
|----------|-----------------|---------|
| `FFMPEG_DIR` | `C:\proyectos-cursor\do-vale-studio-4\third_party\ffmpeg` | **Compile time** — `ffmpeg-sys-next` build script locates `include/` and `lib/` |
| `LIBCLANG_PATH` | `C:\Program Files\LLVM\bin` or VS LLVM path | **Compile time** — bindgen/clang DLL directory |
| `PATH` | prepend `%FFMPEG_DIR%\bin` | **Runtime** — load FFmpeg DLLs and run `ffmpeg`/`ffprobe` |

### PowerShell (current session)

```powershell
$repo = "C:\proyectos-cursor\do-vale-studio-4"
$env:FFMPEG_DIR = "$repo\third_party\ffmpeg"
$env:PATH = "$env:FFMPEG_DIR\bin;$env:PATH"

# LLVM — use one of:
$env:LIBCLANG_PATH = "C:\Program Files\LLVM\bin"
# or Visual Studio bundled LLVM:
# $env:LIBCLANG_PATH = "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\Llvm\x64\bin"
```

### Persistent (user environment)

Set via Windows **System Properties → Environment Variables**, or:

```powershell
[System.Environment]::SetEnvironmentVariable("FFMPEG_DIR", "C:\proyectos-cursor\do-vale-studio-4\third_party\ffmpeg", "User")
```

Restart the terminal/IDE after persistent changes.

### Optional: repo-local `.env`

Root `.env` is gitignored. You may create a local `.env` for tooling that loads it (not used by Cargo automatically):

```env
FFMPEG_DIR=C:\proyectos-cursor\do-vale-studio-4\third_party\ffmpeg
LIBCLANG_PATH=C:\Program Files\LLVM\bin
```

### `FFMPEG_DIR` validation

`FFMPEG_DIR` must point at the directory that **directly** contains `include` and `lib`:

```powershell
Test-Path "$env:FFMPEG_DIR\include\libavcodec\avcodec.h"   # True
Test-Path "$env:FFMPEG_DIR\lib\avcodec.lib"                # True
Test-Path "$env:FFMPEG_DIR\bin\ffmpeg.exe"                 # True (shared build)
```

**Common mistake:** setting `FFMPEG_DIR` to the parent of an extra nesting level (e.g. `...\ffmpeg-7.1-full_build-shared\ffmpeg-7.1-full_build-shared`).

---

## 8. Clean-machine reproduction checklist

On a fresh Windows 10/11 x64 machine:

1. Install **Visual Studio Build Tools** (or VS Community) with **Desktop development with C++**.
2. Install **LLVM/clang** for bindgen, **or** use the LLVM component bundled with VS (set `LIBCLANG_PATH` accordingly).
3. Install **Rust** (`rustup`) with default `x86_64-pc-windows-msvc` toolchain.
4. Clone `do-vale-studio-4`.
5. Download FFmpeg **full shared MSVC** build (§6).
6. Populate `third_party/ffmpeg/{include,lib,bin}`.
7. Set `FFMPEG_DIR`, `LIBCLANG_PATH`, and prepend `bin` to `PATH`.
8. Run verification commands (§9–10).
9. Place test fixture at `docs/fixtures/test_4k_hevc.mp4` (see [fixtures README](../fixtures/README.md)).
10. When Experiment 2 is implemented: `cargo build -p gpu-d3d11-interop` from repo root.

**Only** `tests/gpu_d3d11_interop` may depend on FFmpeg during the experiment phase. Production crates (`dvs-decoder`, `dvs-media`, etc.) remain FFmpeg-free until the spike succeeds.

---

## 9. Verify D3D11VA support

```powershell
# 1. Hardware accelerators
ffmpeg -hide_banner -hwaccels

# Expected output includes:
# d3d11va

# 2. D3D11VA device registration
ffmpeg -hide_banner -decoders | Select-String -Pattern "hevc"

# Expected: HEVC decoder listed (e.g. hevc, hevc_qsv, etc.)

# 3. D3D11 hwaccel for HEVC (requires fixture)
ffmpeg -hide_banner `
  -hwaccel d3d11va -hwaccel_output_format d3d11 `
  -i docs\fixtures\test_4k_hevc.mp4 `
  -frames:v 5 -f null -

# Expected: no "Failed to initialise D3D11VA" or similar fatal error
```

### Inspect build configuration (optional)

```powershell
ffmpeg -hide_banner -buildconf | Select-String -Pattern "d3d11va|dxva2"
```

**Expected:** `--enable-d3d11va` (and often `--enable-dxva2`).

---

## 10. Verify HEVC decoding support

```powershell
# Decoders
ffmpeg -hide_banner -decoders | Select-String -Pattern "^\s*V.*\s+hevc"

# Encoders (not required for Experiment 2)
ffmpeg -hide_banner -encoders | Select-String -Pattern "hevc"

# Protocols / formats (MP4)
ffmpeg -hide_banner -formats | Select-String -Pattern "mp4|mov"

# Pixel formats (D3D11 output — after hwaccel decode)
ffmpeg -hide_banner -pix_fmts | Select-String -Pattern "d3d11"
```

**Expected:**

- `hevc` decoder present
- `mp4` / `mov` demuxer present
- `d3d11` pixel format listed (for GPU frame output)

### Fixture probe (requires `test_4k_hevc.mp4`)

```powershell
ffprobe -v error `
  -select_streams v:0 `
  -show_entries stream=codec_name,width,height,r_frame_rate,pix_fmt,bit_depth `
  -of default=noprint_wrappers=1 `
  docs\fixtures\test_4k_hevc.mp4
```

**Expected:**

```text
codec_name=hevc
width=3840
height=2160
r_frame_rate=30/1
pix_fmt=yuv420p
```

---

## 11. Rust linking (`ffmpeg-sys-next`)

### Isolation rule

| Crate | FFmpeg dependency |
|-------|-------------------|
| `tests/gpu_d3d11_interop` | **Allowed** (Experiment 2 only) |
| `dvs-decoder`, `dvs-media`, all other production crates | **Forbidden** until spike validated |

### Expected `Cargo.toml` (Experiment 2 phase — not yet added)

```toml
[dependencies]
ffmpeg-next = "7"
# or ffmpeg-sys-next directly for lower-level spike
```

`ffmpeg-next` pulls in `ffmpeg-sys-next`, which:

1. Reads `FFMPEG_DIR` at **build** time.
2. Runs **bindgen** against `include/libavcodec/avcodec.h` (needs `LIBCLANG_PATH`).
3. Links `avcodec.lib`, `avformat.lib`, `avutil.lib`, `avfilter.lib`, `swscale.lib`, `swresample.lib`.
4. Produces a Rust binary that **loads FFmpeg DLLs at runtime** (shared build).

### Build commands (future)

```powershell
# From repository root, with FFMPEG_DIR and LIBCLANG_PATH set
cargo check -p gpu-d3d11-interop
cargo build --release -p gpu-d3d11-interop
```

### Runtime DLL resolution

If the executable fails with "avcodec-XX.dll not found":

- Ensure `%FFMPEG_DIR%\bin` is on `PATH`, **or**
- Copy DLLs to `target\release\`.

### Static linking (not recommended for Phase 1)

Static FFmpeg builds on Windows still often require bundled DLLs or have licensing/size implications. Use **shared** builds for the experiment unless you have a specific reason otherwise.

---

## 12. Runtime vs development — quick reference

| Question | Development libs | Runtime binaries |
|----------|------------------|------------------|
| Where? | `third_party/ffmpeg/lib` + `include` | `third_party/ffmpeg/bin` |
| Env var | `FFMPEG_DIR` | `PATH` (to `bin`) |
| Consumed by | `cargo build` / `ffmpeg-sys-next` | `gpu-d3d11-interop.exe`, `ffmpeg.exe` |
| File types | `.lib`, `.h` | `.dll`, `.exe` |
| Needed to compile Rust? | **Yes** | No |
| Needed to run Rust binary? | No (uses DLLs) | **Yes** |
| Needed for ffprobe fixture check? | No | **Yes** |

---

## 13. Troubleshooting

| Symptom | Likely cause | Fix |
|---------|--------------|-----|
| `Could not find ffmpeg with vcpkg` | No vcpkg integration | Set `FFMPEG_DIR` explicitly |
| `Unable to find libclang` | `LIBCLANG_PATH` unset | Install LLVM; set path to `libclang.dll` directory |
| `cannot open input file 'avcodec.lib'` | Wrong `FFMPEG_DIR` or incomplete extract | Verify `lib\avcodec.lib` exists |
| `avcodec-61.dll was not found` at runtime | `bin` not on `PATH` | Prepend `%FFMPEG_DIR%\bin` to `PATH` |
| `d3d11va` missing from `-hwaccels` | Wrong FFmpeg build (essentials/minimal) | Use **full** shared build |
| HEVC decode works but no `d3d11` pix fmt | Old FFmpeg | Upgrade to 6.x/7.x full build |
| bindgen parse errors | MSVC vs MinGW header mismatch | Use MSVC FFmpeg build with MSVC Rust toolchain |

---

## 14. Related documentation

- [GPU Experiment 2](../gpu/GPU_EXPERIMENT_2.md) — blocked until fixture + this setup complete
- [GPU Architecture Spike](../gpu/GPU_ARCHITECTURE_SPIKE.md) — D3D11VA → wgpu interop design
- [Fixtures README](../fixtures/README.md) — `test_4k_hevc.mp4` requirements
- [third_party/README.md](../../third_party/README.md) — local dependency layout

---

## Current machine status (2026-09-01)

| Check | Status |
|-------|--------|
| `third_party/ffmpeg/` populated | **No** |
| `FFMPEG_DIR` set | **No** |
| `LIBCLANG_PATH` set | **No** |
| `ffmpeg` on PATH | **No** |
| Fixture `test_4k_hevc.mp4` | **Missing** |

Experiment 2 remains **blocked**.
