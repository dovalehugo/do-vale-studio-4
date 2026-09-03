//! Minimal project document root for the domain model.
//!
//! A [`Project`] currently owns only its [`ProjectId`] and a non-empty display
//! name. Timeline, media pool, persistence, and settings belong to later phases.

use crate::ids::ProjectId;

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
///
/// This type performs no I/O, holds no file paths, and does not own timeline or
/// media collections.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Project {
    id: ProjectId,
    name: String,
}

impl Project {
    /// Creates a project with an explicit identity and display name.
    ///
    /// Returns [`ProjectError::EmptyName`] when `name` is empty. Whitespace-only
    /// names are accepted as-is; normalization is deferred until persistence or
    /// UI policy defines it.
    pub fn new(id: ProjectId, name: impl Into<String>) -> Result<Self, ProjectError> {
        let name = name.into();
        if name.is_empty() {
            return Err(ProjectError::EmptyName);
        }
        Ok(Self { id, name })
    }

    /// Returns the project identity.
    pub const fn id(&self) -> ProjectId {
        self.id
    }

    /// Returns the project display name.
    pub fn name(&self) -> &str {
        &self.name
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
