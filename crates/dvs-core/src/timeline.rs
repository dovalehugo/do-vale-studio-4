//! Timeline, tracks, and clips.

use crate::error::EditorError;
use crate::ids::{ClipId, MediaAssetId, TrackId};
use crate::time::{SourceOffset, TimelineDuration, TimelinePosition, TimelineRange};

/// Kind of timeline track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TrackKind {
    /// Picture track.
    Video,
    /// Sound track.
    Audio,
}

/// Editorial clip placed on a track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Clip {
    id: ClipId,
    asset_id: MediaAssetId,
    start: TimelinePosition,
    source_offset: SourceOffset,
    duration: TimelineDuration,
}

impl Clip {
    /// Creates a clip with explicit identity and timing.
    pub fn new(
        id: ClipId,
        asset_id: MediaAssetId,
        start: TimelinePosition,
        source_offset: SourceOffset,
        duration: TimelineDuration,
    ) -> Self {
        Self {
            id,
            asset_id,
            start,
            source_offset,
            duration,
        }
    }

    /// Clip identity.
    pub const fn id(&self) -> ClipId {
        self.id
    }

    /// Referenced media asset.
    pub const fn asset_id(&self) -> MediaAssetId {
        self.asset_id
    }

    /// Timeline start position.
    pub const fn start(&self) -> TimelinePosition {
        self.start
    }

    /// Offset into the media source.
    pub const fn source_offset(&self) -> SourceOffset {
        self.source_offset
    }

    /// Clip duration on the timeline.
    pub const fn duration(&self) -> TimelineDuration {
        self.duration
    }

    /// Half-open timeline range occupied by this clip.
    pub const fn range(&self) -> TimelineRange {
        TimelineRange::new(self.start, self.duration)
    }

    /// Exclusive end position.
    pub fn end(&self) -> Result<TimelinePosition, EditorError> {
        self.range().end()
    }

    pub(crate) fn set_timing(
        &mut self,
        start: TimelinePosition,
        source_offset: SourceOffset,
        duration: TimelineDuration,
    ) {
        self.start = start;
        self.source_offset = source_offset;
        self.duration = duration;
    }
}

/// Ordered track containing non-overlapping clips.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    id: TrackId,
    name: String,
    kind: TrackKind,
    clips: Vec<Clip>,
}

impl Track {
    /// Creates an empty track with a non-empty name.
    pub fn new(id: TrackId, name: impl Into<String>, kind: TrackKind) -> Result<Self, EditorError> {
        let name = name.into();
        if name.is_empty() {
            return Err(EditorError::EmptyName);
        }
        Ok(Self {
            id,
            name,
            kind,
            clips: Vec::new(),
        })
    }

    /// Track identity.
    pub const fn id(&self) -> TrackId {
        self.id
    }

    /// Track display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Track kind.
    pub const fn kind(&self) -> TrackKind {
        self.kind
    }

    /// Clips in ascending start order.
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    pub(crate) fn find_clip(&self, id: ClipId) -> Option<&Clip> {
        self.clips.iter().find(|c| c.id() == id)
    }

    pub(crate) fn contains_asset(&self, asset: MediaAssetId) -> bool {
        self.clips.iter().any(|c| c.asset_id() == asset)
    }

    pub(crate) fn insert_clip(&mut self, clip: Clip) -> Result<(), EditorError> {
        #[cfg(test)]
        {
            if fail_next_clip_insert::take() {
                return Err(EditorError::Overlap);
            }
        }
        let range = clip.range();
        for existing in &self.clips {
            if existing.range().overlaps(range)? {
                return Err(EditorError::Overlap);
            }
        }
        let idx = self
            .clips
            .iter()
            .position(|c| c.start() > clip.start())
            .unwrap_or(self.clips.len());
        self.clips.insert(idx, clip);
        Ok(())
    }

    pub(crate) fn remove_clip(&mut self, id: ClipId) -> Result<Clip, EditorError> {
        let idx = self
            .clips
            .iter()
            .position(|c| c.id() == id)
            .ok_or(EditorError::ClipNotFound)?;
        Ok(self.clips.remove(idx))
    }

    /// Validates that `candidate` does not overlap any clip except `ignore`.
    pub(crate) fn ensure_no_overlap(
        &self,
        candidate: TimelineRange,
        ignore: Option<ClipId>,
    ) -> Result<(), EditorError> {
        for existing in &self.clips {
            if ignore == Some(existing.id()) {
                continue;
            }
            if existing.range().overlaps(candidate)? {
                return Err(EditorError::Overlap);
            }
        }
        Ok(())
    }

    pub(crate) fn reinsert_sorted(&mut self, clip: Clip) -> Result<(), EditorError> {
        self.insert_clip(clip)
    }
}

/// Project timeline: ordered tracks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Timeline {
    tracks: Vec<Track>,
}

impl Timeline {
    /// Creates an empty timeline.
    pub const fn new() -> Self {
        Self { tracks: Vec::new() }
    }

    /// Tracks in insertion order.
    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub(crate) fn track_index(&self, id: TrackId) -> Result<usize, EditorError> {
        self.tracks
            .iter()
            .position(|t| t.id() == id)
            .ok_or(EditorError::TrackNotFound)
    }

    pub(crate) fn track(&self, id: TrackId) -> Result<&Track, EditorError> {
        let idx = self.track_index(id)?;
        Ok(&self.tracks[idx])
    }

    pub(crate) fn track_mut(&mut self, id: TrackId) -> Result<&mut Track, EditorError> {
        let idx = self.track_index(id)?;
        Ok(&mut self.tracks[idx])
    }

    pub(crate) fn add_track(&mut self, track: Track) -> Result<(), EditorError> {
        if self.tracks.iter().any(|t| t.id() == track.id()) {
            return Err(EditorError::DuplicateId);
        }
        self.tracks.push(track);
        Ok(())
    }

    pub(crate) fn remove_track(&mut self, id: TrackId) -> Result<(usize, Track), EditorError> {
        let idx = self.track_index(id)?;
        Ok((idx, self.tracks.remove(idx)))
    }

    pub(crate) fn insert_track_at(&mut self, index: usize, track: Track) {
        self.tracks.insert(index, track);
    }

    pub(crate) fn find_clip(&self, id: ClipId) -> Option<(TrackId, &Clip)> {
        for track in &self.tracks {
            if let Some(clip) = track.find_clip(id) {
                return Some((track.id(), clip));
            }
        }
        None
    }

    pub(crate) fn contains_clip_id(&self, id: ClipId) -> bool {
        self.find_clip(id).is_some()
    }

    pub(crate) fn asset_in_use(&self, asset: MediaAssetId) -> bool {
        self.tracks.iter().any(|t| t.contains_asset(asset))
    }

    pub(crate) fn clip_track_id(&self, clip_id: ClipId) -> Result<TrackId, EditorError> {
        self.find_clip(clip_id)
            .map(|(tid, _)| tid)
            .ok_or(EditorError::ClipNotFound)
    }
}

#[cfg(test)]
pub(crate) mod fail_next_clip_insert {
    use std::cell::Cell;

    thread_local! {
        /// When `Some(n)`, the next `n` inserts succeed and the following one fails.
        static SKIP_THEN_FAIL: Cell<Option<usize>> = const { Cell::new(None) };
    }

    /// Allow `skip` successful inserts, then fail the following one.
    pub(crate) fn arm_after(skip: usize) {
        SKIP_THEN_FAIL.with(|c| c.set(Some(skip)));
    }

    pub(crate) fn take() -> bool {
        SKIP_THEN_FAIL.with(|c| match c.get() {
            Some(0) => {
                c.set(None);
                true
            }
            Some(n) => {
                c.set(Some(n - 1));
                false
            }
            None => false,
        })
    }
}
