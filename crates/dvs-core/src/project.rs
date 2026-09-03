//! Project document root: identity, media pool, timeline, and selection.

use crate::error::EditorError;
use crate::ids::ProjectId;
use crate::media_pool::MediaPool;
use crate::selection::Selection;
use crate::timeline::Timeline;

/// Error returned when project construction fails validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectError {
    /// Project names must contain at least one character.
    EmptyName,
}

/// Root domain object for an editor project.
///
/// Invariants:
/// - `id` is always a valid [`ProjectId`] (non-zero).
/// - `name` is never empty.
/// - Media pool stores only [`crate::MediaAssetId`] values (no paths).
/// - Timeline clips reference registered assets and never overlap on a track.
///
/// This type performs no I/O and holds no file paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    id: ProjectId,
    name: String,
    media_pool: MediaPool,
    timeline: Timeline,
    selection: Selection,
}

impl Project {
    /// Creates an empty project with an explicit identity and display name.
    ///
    /// Returns [`ProjectError::EmptyName`] when `name` is empty. Whitespace-only
    /// names are accepted as-is; normalization is deferred until persistence or
    /// UI policy defines it.
    pub fn new(id: ProjectId, name: impl Into<String>) -> Result<Self, ProjectError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ProjectError::EmptyName);
        }
        Ok(Self {
            id,
            name,
            media_pool: MediaPool::new(),
            timeline: Timeline::new(),
            selection: Selection::None,
        })
    }

    /// Returns the project identity.
    pub const fn id(&self) -> ProjectId {
        self.id
    }

    /// Returns the project display name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Logical media pool.
    pub const fn media_pool(&self) -> &MediaPool {
        &self.media_pool
    }

    /// Project timeline.
    pub const fn timeline(&self) -> &Timeline {
        &self.timeline
    }

    /// Current editor selection.
    pub const fn selection(&self) -> Selection {
        self.selection
    }

    pub(crate) fn media_pool_mut(&mut self) -> &mut MediaPool {
        &mut self.media_pool
    }

    pub(crate) fn timeline_mut(&mut self) -> &mut Timeline {
        &mut self.timeline
    }

    pub(crate) fn set_selection_raw(&mut self, selection: Selection) {
        self.selection = selection;
    }

    pub(crate) fn clear_selection_if(&mut self, pred: impl Fn(Selection) -> bool) {
        if pred(self.selection) {
            self.selection = Selection::None;
        }
    }

    /// Validates that `selection` refers to objects present in this project.
    pub(crate) fn ensure_selection_valid(&self, selection: Selection) -> Result<(), EditorError> {
        match selection {
            Selection::None => Ok(()),
            Selection::MediaAsset(id) => {
                if self.media_pool.contains(id) {
                    Ok(())
                } else {
                    Err(EditorError::InvalidSelection)
                }
            }
            Selection::Track(id) => {
                if self.timeline.track(id).is_ok() {
                    Ok(())
                } else {
                    Err(EditorError::InvalidSelection)
                }
            }
            Selection::Clip(id) => {
                if self.timeline.contains_clip_id(id) {
                    Ok(())
                } else {
                    Err(EditorError::InvalidSelection)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ProjectId;

    #[test]
    fn project_preserves_id_and_name() {
        let id = ProjectId::new(10).expect("id");
        let project = Project::new(id, "Demo Reel").expect("project");
        assert_eq!(project.id(), id);
        assert_eq!(project.name(), "Demo Reel");
        assert!(project.media_pool().is_empty());
        assert!(project.timeline().tracks().is_empty());
        assert_eq!(project.selection(), Selection::None);
    }

    #[test]
    fn project_rejects_empty_name() {
        let id = ProjectId::new(1).expect("id");
        assert_eq!(Project::new(id, ""), Err(ProjectError::EmptyName));
    }

    #[test]
    fn project_accepts_whitespace_only_name_without_normalization() {
        let id = ProjectId::new(2).expect("id");
        let project = Project::new(id, "   ").expect("whitespace name");
        assert_eq!(project.name(), "   ");
    }
}
