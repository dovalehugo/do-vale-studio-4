//! Bounded undo/redo history of reversible edit records.

use crate::error::EditorError;
use crate::ids::{ClipId, MediaAssetId, TrackId};
use crate::project::Project;
use crate::selection::Selection;
use crate::time::{SourceOffset, TimelineDuration, TimelinePosition};
use crate::timeline::{Clip, Track};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipTiming {
    pub start: TimelinePosition,
    pub source_offset: SourceOffset,
    pub duration: TimelineDuration,
}

/// Self-contained reversible record. The same value is moved between undo/redo stacks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum HistoryEntry {
    RegisterAsset {
        id: MediaAssetId,
        selection_before: Selection,
    },
    UnregisterAsset {
        id: MediaAssetId,
        selection_before: Selection,
    },
    AddTrack {
        index: usize,
        track: Track,
        selection_before: Selection,
    },
    RemoveTrack {
        index: usize,
        track: Track,
        selection_before: Selection,
    },
    InsertClip {
        track_id: TrackId,
        clip: Clip,
        selection_before: Selection,
    },
    DeleteClip {
        track_id: TrackId,
        clip: Clip,
        selection_before: Selection,
    },
    MoveClip {
        id: ClipId,
        from_track: TrackId,
        to_track: TrackId,
        from: ClipTiming,
        to: ClipTiming,
        selection_before: Selection,
    },
    TrimClip {
        id: ClipId,
        track_id: TrackId,
        before: ClipTiming,
        after: ClipTiming,
        selection_before: Selection,
    },
    SplitClip {
        track_id: TrackId,
        left_id: ClipId,
        left_before: ClipTiming,
        left_after: ClipTiming,
        right: Clip,
        selection_before: Selection,
    },
}

impl HistoryEntry {
    pub(crate) fn undo(&self, project: &mut Project) -> Result<(), EditorError> {
        match self {
            Self::RegisterAsset {
                id,
                selection_before,
            } => {
                project.media_pool_mut().unregister(*id)?;
                project.clear_selection_if(|s| s == Selection::MediaAsset(*id));
                project.set_selection_raw(*selection_before);
            }
            Self::UnregisterAsset {
                id,
                selection_before,
            } => {
                project.media_pool_mut().register(*id)?;
                project.set_selection_raw(*selection_before);
            }
            Self::AddTrack {
                track,
                selection_before,
                ..
            } => {
                let id = track.id();
                let (_, removed) = project.timeline_mut().remove_track(id)?;
                project.clear_selection_if(|s| match s {
                    Selection::Track(tid) => tid == id,
                    Selection::Clip(cid) => removed.find_clip(cid).is_some(),
                    _ => false,
                });
                project.set_selection_raw(*selection_before);
            }
            Self::RemoveTrack {
                index,
                track,
                selection_before,
            } => {
                project
                    .timeline_mut()
                    .insert_track_at(*index, track.clone());
                project.set_selection_raw(*selection_before);
            }
            Self::InsertClip {
                track_id,
                clip,
                selection_before,
            } => {
                let id = clip.id();
                project
                    .timeline_mut()
                    .track_mut(*track_id)?
                    .remove_clip(id)?;
                project.clear_selection_if(|s| s == Selection::Clip(id));
                project.set_selection_raw(*selection_before);
            }
            Self::DeleteClip {
                track_id,
                clip,
                selection_before,
            } => {
                project
                    .timeline_mut()
                    .track_mut(*track_id)?
                    .reinsert_sorted(clip.clone())?;
                project.set_selection_raw(*selection_before);
            }
            Self::MoveClip {
                id,
                from_track,
                to_track,
                from,
                selection_before,
                ..
            } => {
                move_clip(project, *id, *to_track, *from_track, *from)?;
                project.set_selection_raw(*selection_before);
            }
            Self::TrimClip {
                id,
                track_id,
                before,
                selection_before,
                ..
            } => {
                set_clip_timing(project, *track_id, *id, *before)?;
                project.set_selection_raw(*selection_before);
            }
            Self::SplitClip {
                track_id,
                left_id,
                left_before,
                right,
                selection_before,
                ..
            } => {
                undo_split(project, *track_id, *left_id, *left_before, right)?;
                project.clear_selection_if(|s| {
                    s == Selection::Clip(right.id()) || s == Selection::Clip(*left_id)
                });
                project.set_selection_raw(*selection_before);
            }
        }
        Ok(())
    }

    pub(crate) fn redo(&self, project: &mut Project) -> Result<(), EditorError> {
        match self {
            Self::RegisterAsset {
                id,
                selection_before,
            } => {
                project.media_pool_mut().register(*id)?;
                project.set_selection_raw(*selection_before);
            }
            Self::UnregisterAsset {
                id,
                selection_before: _,
            } => {
                project.media_pool_mut().unregister(*id)?;
                project.clear_selection_if(|s| s == Selection::MediaAsset(*id));
            }
            Self::AddTrack {
                index,
                track,
                selection_before,
            } => {
                project
                    .timeline_mut()
                    .insert_track_at(*index, track.clone());
                project.set_selection_raw(*selection_before);
            }
            Self::RemoveTrack { track, .. } => {
                let id = track.id();
                let (_, removed) = project.timeline_mut().remove_track(id)?;
                project.clear_selection_if(|s| match s {
                    Selection::Track(tid) => tid == id,
                    Selection::Clip(cid) => removed.find_clip(cid).is_some(),
                    _ => false,
                });
            }
            Self::InsertClip {
                track_id,
                clip,
                selection_before,
            } => {
                project
                    .timeline_mut()
                    .track_mut(*track_id)?
                    .reinsert_sorted(clip.clone())?;
                project.set_selection_raw(*selection_before);
            }
            Self::DeleteClip {
                track_id,
                clip,
                selection_before: _,
            } => {
                let id = clip.id();
                project
                    .timeline_mut()
                    .track_mut(*track_id)?
                    .remove_clip(id)?;
                project.clear_selection_if(|s| s == Selection::Clip(id));
            }
            Self::MoveClip {
                id,
                from_track,
                to_track,
                to,
                selection_before,
                ..
            } => {
                move_clip(project, *id, *from_track, *to_track, *to)?;
                project.set_selection_raw(*selection_before);
            }
            Self::TrimClip {
                id,
                track_id,
                after,
                selection_before,
                ..
            } => {
                set_clip_timing(project, *track_id, *id, *after)?;
                project.set_selection_raw(*selection_before);
            }
            Self::SplitClip {
                track_id,
                left_id,
                left_after,
                right,
                selection_before,
                ..
            } => {
                redo_split(project, *track_id, *left_id, *left_after, right)?;
                project.set_selection_raw(*selection_before);
            }
        }
        Ok(())
    }
}

fn undo_split(
    project: &mut Project,
    track_id: TrackId,
    left_id: ClipId,
    left_before: ClipTiming,
    right: &Clip,
) -> Result<(), EditorError> {
    let right_id = right.id();
    let removed_right = project
        .timeline_mut()
        .track_mut(track_id)?
        .remove_clip(right_id)?;
    match set_clip_timing(project, track_id, left_id, left_before) {
        Ok(()) => Ok(()),
        Err(err) => match project
            .timeline_mut()
            .track_mut(track_id)?
            .reinsert_sorted(removed_right)
        {
            Ok(()) => Err(err),
            Err(restore_err) => Err(restore_err),
        },
    }
}

fn redo_split(
    project: &mut Project,
    track_id: TrackId,
    left_id: ClipId,
    left_after: ClipTiming,
    right: &Clip,
) -> Result<(), EditorError> {
    use crate::time::TimelineRange;

    let left_range = TimelineRange::new(left_after.start, left_after.duration);
    let right_range = right.range();
    project
        .timeline()
        .track(track_id)?
        .ensure_no_overlap(left_range, Some(left_id))?;
    project
        .timeline()
        .track(track_id)?
        .ensure_no_overlap(right_range, Some(left_id))?;

    let left = project
        .timeline()
        .find_clip(left_id)
        .map(|(_, c)| c.clone())
        .ok_or(EditorError::ClipNotFound)?;
    let left_before = ClipTiming {
        start: left.start(),
        source_offset: left.source_offset(),
        duration: left.duration(),
    };

    set_clip_timing(project, track_id, left_id, left_after)?;
    match project
        .timeline_mut()
        .track_mut(track_id)?
        .reinsert_sorted(right.clone())
    {
        Ok(()) => Ok(()),
        Err(err) => match set_clip_timing(project, track_id, left_id, left_before) {
            Ok(()) => Err(err),
            Err(restore_err) => Err(restore_err),
        },
    }
}

fn set_clip_timing(
    project: &mut Project,
    track_id: TrackId,
    id: ClipId,
    timing: ClipTiming,
) -> Result<(), EditorError> {
    use crate::time::TimelineRange;

    let candidate = TimelineRange::new(timing.start, timing.duration);
    project
        .timeline()
        .track(track_id)?
        .ensure_no_overlap(candidate, Some(id))?;

    let track = project.timeline_mut().track_mut(track_id)?;
    let mut clip = track.remove_clip(id)?;
    let original = ClipTiming {
        start: clip.start(),
        source_offset: clip.source_offset(),
        duration: clip.duration(),
    };
    clip.set_timing(timing.start, timing.source_offset, timing.duration);
    match track.reinsert_sorted(clip.clone()) {
        Ok(()) => Ok(()),
        Err(err) => {
            clip.set_timing(original.start, original.source_offset, original.duration);
            match track.reinsert_sorted(clip) {
                Ok(()) => Err(err),
                Err(restore_err) => Err(restore_err),
            }
        }
    }
}

fn move_clip(
    project: &mut Project,
    id: ClipId,
    from_track: TrackId,
    to_track: TrackId,
    timing: ClipTiming,
) -> Result<(), EditorError> {
    use crate::time::TimelineRange;

    let _ = project.timeline().track(from_track)?;
    let _ = project.timeline().track(to_track)?;
    let candidate = TimelineRange::new(timing.start, timing.duration);
    if from_track == to_track {
        project
            .timeline()
            .track(from_track)?
            .ensure_no_overlap(candidate, Some(id))?;
    } else {
        project
            .timeline()
            .track(to_track)?
            .ensure_no_overlap(candidate, None)?;
    }

    let mut clip = project
        .timeline_mut()
        .track_mut(from_track)?
        .remove_clip(id)?;
    let original = ClipTiming {
        start: clip.start(),
        source_offset: clip.source_offset(),
        duration: clip.duration(),
    };
    clip.set_timing(timing.start, timing.source_offset, timing.duration);
    match project
        .timeline_mut()
        .track_mut(to_track)?
        .reinsert_sorted(clip.clone())
    {
        Ok(()) => Ok(()),
        Err(err) => {
            clip.set_timing(original.start, original.source_offset, original.duration);
            match project
                .timeline_mut()
                .track_mut(from_track)?
                .reinsert_sorted(clip)
            {
                Ok(()) => Err(err),
                Err(restore_err) => Err(restore_err),
            }
        }
    }
}

/// Undo/redo stacks for the editor session (not persisted with the project).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl History {
    /// Creates empty stacks.
    pub const fn new() -> Self {
        Self {
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
        self.redo.clear();
    }

    pub(crate) fn pop_undo(&mut self) -> Result<HistoryEntry, EditorError> {
        self.undo.pop().ok_or(EditorError::NothingToUndo)
    }

    pub(crate) fn pop_redo(&mut self) -> Result<HistoryEntry, EditorError> {
        self.redo.pop().ok_or(EditorError::NothingToRedo)
    }

    pub(crate) fn push_redo(&mut self, entry: HistoryEntry) {
        self.redo.push(entry);
    }

    pub(crate) fn push_undo_keep_redo(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
    }

    pub(crate) fn restore_undo(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
    }

    pub(crate) fn restore_redo(&mut self, entry: HistoryEntry) {
        self.redo.push(entry);
    }

    /// Number of undo entries.
    pub fn undo_len(&self) -> usize {
        self.undo.len()
    }

    #[cfg(test)]
    pub(crate) fn inject_undo_for_test(&mut self, entry: HistoryEntry) {
        self.undo.push(entry);
    }

    #[cfg(test)]
    pub(crate) fn inject_redo_for_test(&mut self, entry: HistoryEntry) {
        self.redo.push(entry);
    }

    /// Number of redo entries.
    pub fn redo_len(&self) -> usize {
        self.redo.len()
    }

    /// Whether undo is available.
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    /// Whether redo is available.
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}
