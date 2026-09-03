//! Pure domain layer for Do Vale Studio 4.
//!
//! `dvs-core` owns editor identity types, timeline time, the project document
//! (media pool, tracks, clips, selection), and the undoable editor session.
//! It must stay free of egui, winit, wgpu, FFmpeg, and I/O.
//!
//! Media playback timing (`TimeBase`, `MediaTimestamp`, `FrameId`) remains in
//! `dvs-media`. This crate does not depend on `dvs-media`.

#![forbid(unsafe_code)]

mod editor;
mod error;
mod history;
mod ids;
mod media_pool;
mod project;
mod selection;
mod time;
mod timeline;

pub use editor::{EditCommand, Editor};
pub use error::EditorError;
pub use history::History;
pub use ids::{ClipId, IdError, MediaAssetId, ProjectId, TrackId};
pub use media_pool::MediaPool;
pub use project::{Project, ProjectError};
pub use selection::Selection;
pub use time::{SourceOffset, TimelineDuration, TimelinePosition, TimelineRange};
pub use timeline::{Clip, Timeline, Track, TrackKind};

#[cfg(test)]
mod integration_tests;
