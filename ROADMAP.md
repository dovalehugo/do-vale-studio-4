# Do Vale Studio 4 — Roadmap

## Phase 0 — Foundation

- [ ] Repository
- [ ] Workspace
- [ ] Architecture
- [ ] Error model
- [ ] Logging
- [ ] Profiling infrastructure
- [ ] GPU capability detection

---

## Phase 1 — GPU + Media Foundation

- [ ] FFmpeg integration
- [ ] Media probing
- [ ] Hardware decoder detection
- [ ] VideoFrame abstraction
- [ ] GPU frame abstraction
- [ ] Windows D3D11VA
- [ ] GPU-native frame investigation
- [ ] GPU texture pipeline
- [ ] GPU scaling
- [ ] Native video viewport

SUCCESS CRITERION:

4K HEVC hardware decoded and rendered without mandatory
GPU → CPU → GPU frame conversion.

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