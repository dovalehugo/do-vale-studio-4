//! Editor session: commands, selection, undo, and redo.

use crate::error::EditorError;
use crate::history::{ClipTiming, History, HistoryEntry};
use crate::ids::{ClipId, MediaAssetId, TrackId};
use crate::project::Project;
use crate::selection::Selection;
use crate::time::{SourceOffset, TimelineDuration, TimelinePosition, TimelineRange};
use crate::timeline::{Clip, Track, TrackKind};

/// Explicit edit command. Callers supply all identity values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditCommand {
    /// Register a media asset identity in the pool.
    RegisterAsset {
        /// Asset identity allocated by the caller.
        id: MediaAssetId,
    },
    /// Remove an unused media asset from the pool.
    UnregisterAsset {
        /// Asset to remove.
        id: MediaAssetId,
    },
    /// Append a track to the timeline.
    AddTrack {
        /// Track identity allocated by the caller.
        id: TrackId,
        /// Non-empty display name.
        name: String,
        /// Track kind.
        kind: TrackKind,
    },
    /// Remove a track and all of its clips.
    RemoveTrack {
        /// Track to remove.
        id: TrackId,
    },
    /// Insert a clip onto a track.
    InsertClip {
        /// Clip identity allocated by the caller.
        id: ClipId,
        /// Destination track.
        track_id: TrackId,
        /// Registered media asset.
        asset_id: MediaAssetId,
        /// Timeline start.
        start: TimelinePosition,
        /// Media source offset.
        source_offset: SourceOffset,
        /// Positive duration.
        duration: TimelineDuration,
    },
    /// Move a clip in time and optionally to another track of the same kind.
    MoveClip {
        /// Clip to move.
        id: ClipId,
        /// Destination track (may equal the current track).
        track_id: TrackId,
        /// New timeline start.
        start: TimelinePosition,
    },
    /// Change a clip's timeline placement and source window.
    TrimClip {
        /// Clip to trim.
        id: ClipId,
        /// New timeline start.
        start: TimelinePosition,
        /// New media source offset.
        source_offset: SourceOffset,
        /// New positive duration.
        duration: TimelineDuration,
    },
    /// Split a clip at an interior timeline position.
    SplitClip {
        /// Existing clip (becomes the left segment).
        id: ClipId,
        /// Strictly interior cut position.
        at: TimelinePosition,
        /// Identity for the new right segment.
        right_id: ClipId,
    },
    /// Delete a clip.
    DeleteClip {
        /// Clip to delete.
        id: ClipId,
    },
}

/// Pure editor controller over a [`Project`] with undo/redo history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Editor {
    project: Project,
    history: History,
}

impl Editor {
    /// Creates an editor session for `project` with empty history.
    pub fn new(project: Project) -> Self {
        Self {
            project,
            history: History::new(),
        }
    }

    /// Immutable project view.
    pub const fn project(&self) -> &Project {
        &self.project
    }

    /// Undo/redo stacks (not part of persisted project state).
    pub const fn history(&self) -> &History {
        &self.history
    }

    /// Sets selection after validating the target exists.
    pub fn set_selection(&mut self, selection: Selection) -> Result<(), EditorError> {
        self.project.ensure_selection_valid(selection)?;
        self.project.set_selection_raw(selection);
        Ok(())
    }

    /// Applies `command`, recording history when project state changes.
    pub fn apply(&mut self, command: EditCommand) -> Result<(), EditorError> {
        let selection_before = self.project.selection();
        if let Some(entry) = self.execute(command, selection_before)? {
            self.history.push(entry);
        }
        Ok(())
    }

    /// Restores the previous editorial state.
    pub fn undo(&mut self) -> Result<(), EditorError> {
        let entry = self.history.pop_undo()?;
        match entry.undo(&mut self.project) {
            Ok(()) => {
                self.history.push_redo(entry);
                Ok(())
            }
            Err(err) => {
                self.history.restore_undo(entry);
                Err(err)
            }
        }
    }

    /// Reapplies the last undone operation.
    pub fn redo(&mut self) -> Result<(), EditorError> {
        let entry = self.history.pop_redo()?;
        match entry.redo(&mut self.project) {
            Ok(()) => {
                self.history.push_undo_keep_redo(entry);
                Ok(())
            }
            Err(err) => {
                self.history.restore_redo(entry);
                Err(err)
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn inject_undo_entry_for_test(&mut self, entry: crate::history::HistoryEntry) {
        self.history.inject_undo_for_test(entry);
    }

    #[cfg(test)]
    pub(crate) fn inject_redo_entry_for_test(&mut self, entry: crate::history::HistoryEntry) {
        self.history.inject_redo_for_test(entry);
    }

    fn execute(
        &mut self,
        command: EditCommand,
        selection_before: Selection,
    ) -> Result<Option<HistoryEntry>, EditorError> {
        match command {
            EditCommand::RegisterAsset { id } => {
                self.project.media_pool_mut().register(id)?;
                Ok(Some(HistoryEntry::RegisterAsset {
                    id,
                    selection_before,
                }))
            }
            EditCommand::UnregisterAsset { id } => {
                if !self.project.media_pool().contains(id) {
                    return Err(EditorError::AssetNotFound);
                }
                if self.project.timeline().asset_in_use(id) {
                    return Err(EditorError::AssetInUse);
                }
                self.project.media_pool_mut().unregister(id)?;
                self.project
                    .clear_selection_if(|s| s == Selection::MediaAsset(id));
                Ok(Some(HistoryEntry::UnregisterAsset {
                    id,
                    selection_before,
                }))
            }
            EditCommand::AddTrack { id, name, kind } => {
                let track = Track::new(id, name, kind)?;
                let index = self.project.timeline().tracks().len();
                self.project.timeline_mut().add_track(track.clone())?;
                Ok(Some(HistoryEntry::AddTrack {
                    index,
                    track,
                    selection_before,
                }))
            }
            EditCommand::RemoveTrack { id } => {
                let (index, track) = self.project.timeline_mut().remove_track(id)?;
                self.project.clear_selection_if(|s| match s {
                    Selection::Track(tid) => tid == id,
                    Selection::Clip(cid) => track.find_clip(cid).is_some(),
                    _ => false,
                });
                Ok(Some(HistoryEntry::RemoveTrack {
                    index,
                    track,
                    selection_before,
                }))
            }
            EditCommand::InsertClip {
                id,
                track_id,
                asset_id,
                start,
                source_offset,
                duration,
            } => {
                if self.project.timeline().contains_clip_id(id) {
                    return Err(EditorError::DuplicateId);
                }
                if !self.project.media_pool().contains(asset_id) {
                    return Err(EditorError::AssetNotFound);
                }
                let clip = Clip::new(id, asset_id, start, source_offset, duration);
                self.project
                    .timeline_mut()
                    .track_mut(track_id)?
                    .insert_clip(clip.clone())?;
                Ok(Some(HistoryEntry::InsertClip {
                    track_id,
                    clip,
                    selection_before,
                }))
            }
            EditCommand::MoveClip {
                id,
                track_id: to_track,
                start,
            } => {
                let from_track = self.project.timeline().clip_track_id(id)?;
                let from_kind = self.project.timeline().track(from_track)?.kind();
                let to_kind = self.project.timeline().track(to_track)?.kind();
                if from_kind != to_kind {
                    return Err(EditorError::IncompatibleTrackKind);
                }
                let clip = self
                    .project
                    .timeline()
                    .find_clip(id)
                    .map(|(_, c)| c.clone())
                    .ok_or(EditorError::ClipNotFound)?;
                let from = ClipTiming {
                    start: clip.start(),
                    source_offset: clip.source_offset(),
                    duration: clip.duration(),
                };
                let to = ClipTiming {
                    start,
                    source_offset: clip.source_offset(),
                    duration: clip.duration(),
                };
                if from_track == to_track && from == to {
                    return Ok(None);
                }
                let candidate = TimelineRange::new(to.start, to.duration);
                if from_track == to_track {
                    self.project
                        .timeline()
                        .track(from_track)?
                        .ensure_no_overlap(candidate, Some(id))?;
                } else {
                    self.project
                        .timeline()
                        .track(to_track)?
                        .ensure_no_overlap(candidate, None)?;
                }

                let mut moved = self
                    .project
                    .timeline_mut()
                    .track_mut(from_track)?
                    .remove_clip(id)?;
                moved.set_timing(to.start, to.source_offset, to.duration);
                match self
                    .project
                    .timeline_mut()
                    .track_mut(to_track)?
                    .reinsert_sorted(moved.clone())
                {
                    Ok(()) => Ok(Some(HistoryEntry::MoveClip {
                        id,
                        from_track,
                        to_track,
                        from,
                        to,
                        selection_before,
                    })),
                    Err(err) => {
                        moved.set_timing(from.start, from.source_offset, from.duration);
                        self.project
                            .timeline_mut()
                            .track_mut(from_track)?
                            .reinsert_sorted(moved)?;
                        Err(err)
                    }
                }
            }
            EditCommand::TrimClip {
                id,
                start,
                source_offset,
                duration,
            } => {
                let track_id = self.project.timeline().clip_track_id(id)?;
                let clip = self
                    .project
                    .timeline()
                    .find_clip(id)
                    .map(|(_, c)| c.clone())
                    .ok_or(EditorError::ClipNotFound)?;
                let before = ClipTiming {
                    start: clip.start(),
                    source_offset: clip.source_offset(),
                    duration: clip.duration(),
                };
                let after = ClipTiming {
                    start,
                    source_offset,
                    duration,
                };
                if before == after {
                    return Ok(None);
                }
                let candidate = TimelineRange::new(after.start, after.duration);
                self.project
                    .timeline()
                    .track(track_id)?
                    .ensure_no_overlap(candidate, Some(id))?;

                let track = self.project.timeline_mut().track_mut(track_id)?;
                let mut trimmed = track.remove_clip(id)?;
                trimmed.set_timing(after.start, after.source_offset, after.duration);
                match track.reinsert_sorted(trimmed.clone()) {
                    Ok(()) => Ok(Some(HistoryEntry::TrimClip {
                        id,
                        track_id,
                        before,
                        after,
                        selection_before,
                    })),
                    Err(err) => {
                        trimmed.set_timing(before.start, before.source_offset, before.duration);
                        track.reinsert_sorted(trimmed)?;
                        Err(err)
                    }
                }
            }
            EditCommand::SplitClip { id, at, right_id } => {
                if self.project.timeline().contains_clip_id(right_id) {
                    return Err(EditorError::DuplicateId);
                }
                let track_id = self.project.timeline().clip_track_id(id)?;
                let clip = self
                    .project
                    .timeline()
                    .find_clip(id)
                    .map(|(_, c)| c.clone())
                    .ok_or(EditorError::ClipNotFound)?;
                let start = clip.start();
                let end = clip.end()?;
                if at <= start || at >= end {
                    return Err(EditorError::SplitNotInterior);
                }
                let left_duration = start.checked_duration_until(at)?;
                let right_duration = at.checked_duration_until(end)?;
                let right_offset = clip.source_offset().checked_add(left_duration)?;
                let left_before = ClipTiming {
                    start,
                    source_offset: clip.source_offset(),
                    duration: clip.duration(),
                };
                let left_after = ClipTiming {
                    start,
                    source_offset: clip.source_offset(),
                    duration: left_duration,
                };
                let right = Clip::new(right_id, clip.asset_id(), at, right_offset, right_duration);

                let left_range = TimelineRange::new(left_after.start, left_after.duration);
                let right_range = right.range();
                self.project
                    .timeline()
                    .track(track_id)?
                    .ensure_no_overlap(left_range, Some(id))?;
                self.project
                    .timeline()
                    .track(track_id)?
                    .ensure_no_overlap(right_range, Some(id))?;

                let track = self.project.timeline_mut().track_mut(track_id)?;
                let mut left = track.remove_clip(id)?;
                left.set_timing(
                    left_after.start,
                    left_after.source_offset,
                    left_after.duration,
                );
                match track.reinsert_sorted(left.clone()) {
                    Ok(()) => match track.reinsert_sorted(right.clone()) {
                        Ok(()) => Ok(Some(HistoryEntry::SplitClip {
                            track_id,
                            left_id: id,
                            left_before,
                            left_after,
                            right,
                            selection_before,
                        })),
                        Err(err) => match track.remove_clip(id) {
                            Ok(mut shortened) => {
                                shortened.set_timing(
                                    left_before.start,
                                    left_before.source_offset,
                                    left_before.duration,
                                );
                                match track.reinsert_sorted(shortened) {
                                    Ok(()) => Err(err),
                                    Err(restore_err) => Err(restore_err),
                                }
                            }
                            Err(remove_err) => Err(remove_err),
                        },
                    },
                    Err(err) => {
                        left.set_timing(
                            left_before.start,
                            left_before.source_offset,
                            left_before.duration,
                        );
                        match track.reinsert_sorted(left) {
                            Ok(()) => Err(err),
                            Err(restore_err) => Err(restore_err),
                        }
                    }
                }
            }
            EditCommand::DeleteClip { id } => {
                let track_id = self.project.timeline().clip_track_id(id)?;
                let clip = self
                    .project
                    .timeline_mut()
                    .track_mut(track_id)?
                    .remove_clip(id)?;
                self.project
                    .clear_selection_if(|s| s == Selection::Clip(id));
                Ok(Some(HistoryEntry::DeleteClip {
                    track_id,
                    clip,
                    selection_before,
                }))
            }
        }
    }
}
