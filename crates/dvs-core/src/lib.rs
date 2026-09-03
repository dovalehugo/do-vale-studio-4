//! Pure domain layer for Do Vale Studio 4.
//!
//! `dvs-core` owns editor identity types and the minimal [`Project`] root. It
//! must stay free of egui, winit, wgpu, FFmpeg, and I/O.
//!
//! Media timing (`TimeBase`, `MediaTimestamp`, `FrameId`) remains in `dvs-media`
//! for now. Timeline-specific time types are deferred to Integration 8B.4 so
//! this crate does not depend on (or cycle with) `dvs-media`.

#![forbid(unsafe_code)]

mod ids;
mod project;

pub use ids::{ClipId, IdError, MediaAssetId, ProjectId, TrackId};
pub use project::{Project, ProjectError};
