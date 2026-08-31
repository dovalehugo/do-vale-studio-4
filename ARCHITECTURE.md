# Do Vale Studio 4 — Architecture

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

Hardware/software media decoding.

Responsibilities:

FFmpeg integration
hardware decoder selection
decode sessions
frame production
seeking

Must expose VideoFrame abstractions.

It must not convert every frame to CPU RGBA by default.

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
dvs-gpu

GPU abstraction.

Responsibilities:

wgpu
GPU textures
pipelines
shaders
GPU resource management
synchronization
texture pools
dvs-render

Video rendering engine.

Responsibilities:

render graph
compositing
transforms
scaling
effects
color processing
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

Allowed:

dvs-app
→ dvs-ui
→ dvs-core
→ dvs-media
→ dvs-playback
→ dvs-render
→ dvs-gpu

The dependency graph must remain acyclic.

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

Exact threading architecture must be validated during implementation.

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