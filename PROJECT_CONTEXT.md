# Do Vale Studio 4

## Project Identity

Do Vale Studio 4 is a professional native desktop Non-Linear Video Editor
(NLE) for Windows and macOS.

The application is designed from the beginning for professional media
workflows including 4K, 6K and eventually higher resolutions.

The long-term goal is a professional editing platform comparable in
architecture and capabilities to applications such as DaVinci Resolve,
Adobe Premiere Pro and Final Cut Pro.

This is NOT a browser video editor.

This is NOT a simple video player.

This is NOT a React application with a video element.

The application must use a native media and rendering architecture.

---

# Primary Technology Direction

Core:

- Rust

UI:

- egui
- eframe

GPU:

- wgpu

Media:

- FFmpeg

Windows hardware video:

- D3D11VA initially
- NVDEC/NVENC where appropriate
- Intel hardware acceleration where appropriate
- AMD hardware acceleration where appropriate

macOS hardware video:

- VideoToolbox

macOS GPU:

- Metal through the selected wgpu backend

---

# Fundamental Architecture Principle

The application must be GPU-first.

Video frames should remain GPU-resident whenever technically possible.

Avoid:

GPU → CPU → GPU

copies in the playback/render path.

CPU processing of every decoded frame must be considered an architectural
problem unless explicitly justified.

The application must not convert every hardware decoded frame to CPU RGBA
before rendering.

---

# Rendering Principle

egui is responsible for application UI.

egui is NOT the video compositor.

The video renderer must be a dedicated GPU rendering subsystem.

Conceptually:

Media Decoder
    ↓
VideoFrame
    ↓
GPU Frame
    ↓
Render Graph
    ↓
GPU Compositor
    ↓
Video Viewport

egui renders the surrounding application interface.

---

# VideoFrame Abstraction

The media layer must not expose raw RGBA buffers as its only frame type.

The architecture must support GPU-native frames.

Conceptual model:

VideoFrame
├── CpuFrame
├── D3D11Frame
├── D3D12Frame
├── MetalFrame
└── VulkanFrame

The exact implementation must be determined during architecture design.

---

# Hardware Acceleration

The application must detect available hardware capabilities.

The system should determine:

- GPU vendor
- GPU model
- VRAM
- graphics backend
- hardware video decoder support
- hardware encoder support
- supported codecs
- supported pixel formats
- GPU feature support

The application must never assume that a specific GPU exists.

---

# Performance Goals

Initial targets:

- 4K 30 FPS smooth playback
- 4K 60 FPS target
- responsive scrubbing
- no unnecessary CPU frame conversion
- GPU accelerated scaling
- GPU accelerated compositing

Future targets:

- multiple simultaneous 4K tracks
- 6K playback
- GPU effects
- color correction
- real-time transitions

Performance must always be measured rather than assumed.

---

# Timeline

The timeline must eventually support:

- multiple video tracks
- multiple audio tracks
- clips
- images
- text
- adjustment layers
- markers
- keyframes
- transitions
- effects
- trim
- split
- ripple editing
- snapping
- linked audio/video
- undo
- redo

Timeline logic must live outside the UI.

---

# Playback

Playback is a dedicated subsystem.

It must manage:

- playhead
- frame scheduling
- decode scheduling
- frame queues
- presentation timestamps
- buffering
- preroll
- seeking
- scrubbing
- dropped frames
- synchronization

Playback must never depend directly on egui widgets.

---

# Scrubbing

Scrubbing must be designed separately from sequential playback.

Fast scrubbing should not trigger an expensive blocking seek for every mouse
movement.

The system should use:

- cached frames
- nearest-frame presentation
- delayed accurate seek
- decode-forward
- asynchronous scheduling

---

# Cache

The application must eventually support:

- RAM frame cache
- persistent disk cache
- thumbnail cache
- waveform cache
- render cache

Cache invalidation must account for media and render parameters.

---

# Audio

The architecture must eventually support:

- multiple audio tracks
- synchronized playback
- waveform generation
- volume
- pan
- fades
- keyframes
- EQ
- compression
- effects
- mixing

Audio must be synchronized using the same master timeline.

---

# Export

The export subsystem must eventually support:

- H.264
- HEVC
- AV1
- professional codecs where technically and legally appropriate

Hardware encoding should be selected whenever available.

---

# AI

AI is an extension of the editor, not the editor itself.

AI must operate through validated editor commands.

Example:

User:
"Remove all silences longer than 1.5 seconds."

AI:

REMOVE_SILENCE
track=1
threshold=-35dB
duration=1.5s

Then:

AI command
    ↓
Validation
    ↓
Editor Command
    ↓
Timeline
    ↓
Undo/Redo

AI must never directly mutate internal editor state.

---

# Plugin Architecture

The architecture should eventually support:

- video effects
- audio effects
- transitions
- generators

Potential plugin technologies may include:

- WASM
- VST3
- native APIs where required

Plugin support must not compromise the stability of the core engine.

---

# Architecture Rules

1. Core domain logic must not depend on UI.
2. Core domain logic must not depend directly on FFmpeg.
3. UI must not perform media decoding.
4. UI must not perform video compositing.
5. Video rendering must be GPU accelerated.
6. Hardware decoding must be preferred when available.
7. Software decoding must remain a fallback.
8. Long-running tasks must be asynchronous.
9. Playback must have higher priority than background jobs.
10. Memory ownership must be explicit.
11. Threading boundaries must be documented.
12. Every performance-critical subsystem must be measurable.
13. Do not introduce abstractions without a concrete reason.
14. Do not optimize without profiling.
15. Do not hide performance problems behind proxies.
16. Do not rewrite architecture without documenting why.

---

# Development Philosophy

Do not implement the entire application at once.

Build vertically.

Every milestone should produce a working application.

The first milestone is the media/GPU pipeline.

The first major success criterion is:

4K HEVC
    ↓
Hardware Decoder
    ↓
GPU-resident frame
    ↓
GPU processing
    ↓
Display

Only after this pipeline is proven should large editing features be added.