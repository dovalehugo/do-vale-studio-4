# Do Vale Studio 4 — Architecture

> **GPU path status (2026-09-01):** GPU Experiment 2 PASS — validated in `tests/gpu_d3d11_interop`. **Integration 0** wires acyclic compile-time dependencies only (`docs/architecture/GPU_PRODUCTION_INTEGRATION_PLAN.md`). No production API or runtime path exists yet. `dvs-app` is the composition root and owns validated initialization order.

## High-Level Architecture

```text
                    ┌──────────────────────┐
                    │       dvs-ui         │
                    │                      │
                    │ egui / eframe        │
                    │ Timeline             │
                    │ Media Browser         │
                    │ Inspector             │
                    │ Transport             │
                    └──────────┬───────────┘
                               │
                         Commands / State
                               │
                               ▼
                    ┌──────────────────────┐
                    │      dvs-core        │
                    │                      │
                    │ Project              │
                    │ Timeline             │
                    │ Commands             │
                    │ Time                 │
                    │ Media IDs            │
                    └──────────┬───────────┘
                               │
              ┌────────────────┼────────────────┐
              │                │                │
              ▼                ▼                ▼
        dvs-media         dvs-playback      dvs-audio
              │                │                │
              ▼                ▼                │
        dvs-decoder       Frame Scheduler      │
              │                │                │
              └────────────────┼────────────────┘
                               ▼
                         dvs-render
                               │
                               ▼
                           dvs-gpu
                               │
                   ┌───────────┼───────────┐
                   ▼           ▼           ▼
                Decode       Effects    Composite
                   │           │           │
                   └───────────┼───────────┘
                               ▼
                             wgpu
                               │
                               ▼
                            Display

                            Crate Responsibilities
dvs-app

Application entry point.

Responsibilities:

startup
shutdown
window creation
dependency wiring

Must contain minimal application logic.

dvs-ui

Application interface.

Responsibilities:

egui
layout
timeline UI
media browser
inspector
transport controls
dialogs

Must not decode video.

Must not contain FFmpeg logic.

dvs-core

Pure domain layer.

Responsibilities:

project model
timeline
clips
tracks
commands
undo/redo
time
IDs

Must not depend on FFmpeg.

Must not depend on egui.

Must be easy to unit test.

dvs-media

Media abstraction.

Responsibilities:

media assets
metadata
media probing
codec information
media capabilities
dvs-decoder
   Owns FFmpeg integration, D3D11VA sessions, and decode/seek lifecycle.
   Keeps AVFrame private; constructs `dvs_gpu::D3d11DecodedSurfaceRef` for ingest.
   Depends on: `dvs-media`, `dvs-gpu`.
   Must not expose AVFrame, FFmpeg, or COM types above its public boundary.

dvs-gpu
   Owns wgpu device/queue/surface, adapter identity (LUID), Windows interop bridge,
   shareable NV12 + fence lifetime, bidirectional timeline fence, imported GPU frames,
   and `D3d11DecodedSurfaceRef` (Windows ingest type).
   No internal crate dependencies for the vertical slice.
   Must not depend on FFmpeg or `dvs-decoder`.

dvs-playback

Playback scheduler.

Responsibilities:

play
pause
seek
scrub
frame queue
preroll
buffering
presentation timing
dropped-frame detection

Depends on: `dvs-decoder`, `dvs-render`, `dvs-media`.
Must not access COM pointers, wgpu-hal, or fence values.

dvs-render

Video rendering engine.

Responsibilities:

render graph
compositing
transforms
scaling
effects
color processing
NV12 plane sampling and YUV→RGB (via `dvs-gpu` frame handles)

Depends on: `dvs-gpu`, `dvs-media`.
Must not import FFmpeg, perform `CopySubresourceRegion`, or manage fence values.

dvs-audio

Audio engine.

Responsibilities:

audio decoding
mixing
synchronization
playback
effects
dvs-cache

Caching.

Responsibilities:

RAM cache
disk cache
render cache
thumbnails
waveform cache
dvs-project

Project persistence.

Responsibilities:

project files
autosave
recovery
migrations
project versions
dvs-export

Final rendering/export.

Responsibilities:

render timeline
hardware encoding
software encoding fallback
output formats
dvs-ai

AI integration.

Responsibilities:

provider abstraction
transcription
semantic analysis
AI commands
validation

AI must not directly mutate the core model.

Dependency Rules

`dvs-app` is the composition root (initialization and wiring).

Internal dependency graph (Integration 0 — wired, acyclic):

dvs-app
  → dvs-core, dvs-ui, dvs-media, dvs-gpu, dvs-decoder, dvs-render, dvs-playback

dvs-playback
  → dvs-media, dvs-decoder, dvs-render

dvs-decoder
  → dvs-media, dvs-gpu

dvs-render
  → dvs-media, dvs-gpu

dvs-ui
  → dvs-core

dvs-gpu
  → (no internal crates)

dvs-media
  → (no internal crates)

Rules:

- The dependency graph must remain acyclic.
- `dvs-gpu` must not depend on `dvs-decoder`.
- Windows COM types must not reach `dvs-media`, `dvs-playback`, `dvs-ui`, or `dvs-app`.
- Integration 0 adds compile-time edges only; no production runtime or external dependencies yet.

Core must remain independent from UI and multimedia implementation.

Performance Boundary

The critical path is:

Decoder
↓
VideoFrame
↓
Playback
↓
RenderGraph
↓
GPU
↓
Display

This path must minimize:

CPU copies
GPU copies
synchronization stalls
allocations
blocking operations
Threading

The application should eventually contain:

UI thread:

input
UI
state presentation

Media worker:

demux
decode

Playback scheduler:

frame scheduling

GPU submission:

rendering

Audio thread:

real-time audio

Background workers:

thumbnails
waveform
cache
AI
proxies

Exact threading architecture is specified in `docs/architecture/GPU_PRODUCTION_INTEGRATION_PLAN.md` §8.

First vertical slice (proposed):

UI thread — input, egui, transport commands (no decode, no GPU sync)
Playback/scheduler thread — clock, frame scheduling, drop policy
Decoder thread — FFmpeg D3D11VA; builds `dvs_gpu::D3d11DecodedSurfaceRef` for ingest
GPU/render thread — interop bridge, fence Wait/Signal on wgpu raw queue, render submit

Bounded channels between threads. No tokio on the hot media path.

Memory

Avoid per-frame allocations.

Use:

frame pools
texture pools
buffer reuse
reference counting where appropriate
bounded queues

Memory ownership must be explicit.

Error Handling

Errors must be represented explicitly.

Examples:

MediaOpenError
DecoderError
HardwareAccelerationError
GpuError
RenderError
ExportError

No silent failures.