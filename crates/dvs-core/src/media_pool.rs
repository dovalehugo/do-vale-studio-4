//! Logical media pool: registered [`MediaAssetId`] values only.
//!
//! Paths and [`dvs_media::MediaAsset`] records live outside this crate.

use std::collections::BTreeSet;

use crate::error::EditorError;
use crate::ids::MediaAssetId;

/// Ordered set of media asset identities known to the project.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MediaPool {
    assets: BTreeSet<MediaAssetId>,
}

impl MediaPool {
    /// Creates an empty media pool.
    pub const fn new() -> Self {
        Self {
            assets: BTreeSet::new(),
        }
    }

    /// Returns whether `id` is registered.
    pub fn contains(&self, id: MediaAssetId) -> bool {
        self.assets.contains(&id)
    }

    /// Registers `id`. Duplicate identities are rejected.
    pub fn register(&mut self, id: MediaAssetId) -> Result<(), EditorError> {
        if !self.assets.insert(id) {
            return Err(EditorError::DuplicateId);
        }
        Ok(())
    }

    /// Removes a registered asset. Missing identities are rejected.
    pub fn unregister(&mut self, id: MediaAssetId) -> Result<(), EditorError> {
        if !self.assets.remove(&id) {
            return Err(EditorError::AssetNotFound);
        }
        Ok(())
    }

    /// Returns registered identities in ascending ID order.
    pub fn ids(&self) -> impl Iterator<Item = MediaAssetId> + '_ {
        self.assets.iter().copied()
    }

    /// Number of registered assets.
    pub fn len(&self) -> usize {
        self.assets.len()
    }

    /// Returns true when no assets are registered.
    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}
