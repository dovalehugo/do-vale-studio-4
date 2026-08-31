
---

# 7. PERFORMANCE.md

Este archivo será importantísimo.

```markdown
# Do Vale Studio 4 — Performance

## Philosophy

Performance is a product feature.

Do not assume performance.

Measure it.

---

# Frame Budgets

24 FPS:
41.67 ms

30 FPS:
33.33 ms

60 FPS:
16.67 ms

120 FPS:
8.33 ms

---

# Playback Metrics

The application should eventually measure:

- FPS
- frame time
- decode time
- queue time
- GPU upload time
- GPU render time
- presentation time
- dropped frames
- CPU utilization
- GPU utilization
- RAM
- VRAM

---

# Decode Metrics

Measure:

- demux
- packet decode
- hardware transfer
- software conversion
- frame creation

Hardware transfer must be treated as a critical metric.

---

# GPU Metrics

Measure:

- texture upload
- GPU render time
- compositor time
- shader time where possible
- synchronization stalls

---

# Forbidden Performance Pattern

Avoid:

GPU decoded frame
    ↓
CPU
    ↓
RGBA
    ↓
GPU

unless explicitly required by a fallback path.

---

# Preferred Pattern

Hardware decoder
    ↓
GPU-native frame
    ↓
GPU conversion
    ↓
GPU scaling
    ↓
GPU effects
    ↓
GPU compositing
    ↓
Display

---

# Benchmark Matrix

The benchmark system should eventually test:

## Codec

- H.264
- HEVC
- AV1
- ProRes

## Resolution

- 1080p
- 4K
- 6K

## Frame rate

- 24
- 30
- 60

## Tracks

- 1
- 2
- 3
- 4+

## Effects

- none
- transform
- color
- blur
- multiple effects

---

# Performance Regression

Every major renderer/media change should be benchmarked.

Record:

- machine
- OS
- GPU
- driver
- codec
- media
- resolution
- FPS
- results

Never compare results without recording the test conditions.

---

# Performance Gates

Initial gate:

4K HEVC 30 FPS:
0 dropped frames during sequential playback.

Next gate:

4K HEVC 60 FPS:
stable playback when hardware decoding and GPU rendering are supported.

Future gate:

multiple simultaneous 4K tracks.

Future gate:

6K GPU-rendered timeline.