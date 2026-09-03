//! Pure media asset identity and source location.
//!
//! [`MediaAsset`] binds a [`MediaAssetId`] from `dvs-core` to an opaque source
//! path. Construction validates only that the path is lexically non-empty; it
//! never touches the filesystem, probes media, or rewrites path components.

use std::path::{Path, PathBuf};

use dvs_core::MediaAssetId;
use thiserror::Error;

/// Error returned when constructing a [`MediaAsset`].
#[derive(Debug, Clone, Copy, Eq, PartialEq, Error)]
pub enum MediaAssetError {
    /// The source path must contain at least one path component character.
    #[error("media asset source path must not be empty")]
    EmptySourcePath,
}

/// Domain record for a media file referenced by identity and source path.
///
/// Invariants:
/// - `id` is always a valid [`MediaAssetId`] (non-zero).
/// - `source_path` is never lexically empty (`as_os_str().is_empty()` is false).
///
/// The path is stored exactly as supplied: relative paths, non-existent paths,
/// and unnormalized components (for example `folder/../video.mp4`) are kept
/// unchanged. No I/O is performed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaAsset {
    id: MediaAssetId,
    source_path: PathBuf,
}

impl MediaAsset {
    /// Creates a media asset with an explicit identity and source path.
    ///
    /// Returns [`MediaAssetError::EmptySourcePath`] when `source_path` is
    /// lexically empty. The path is not canonicalized, absolutized, or checked
    /// for filesystem presence.
    pub fn new(id: MediaAssetId, source_path: impl Into<PathBuf>) -> Result<Self, MediaAssetError> {
        let source_path = source_path.into();
        if source_path.as_os_str().is_empty() {
            return Err(MediaAssetError::EmptySourcePath);
        }
        Ok(Self { id, source_path })
    }

    /// Returns the media asset identity.
    pub const fn id(&self) -> MediaAssetId {
        self.id
    }

    /// Returns the stored source path without modification.
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn media_asset_preserves_id() {
        let id = MediaAssetId::new(7).expect("id");
        let asset = MediaAsset::new(id, "clips/a.mp4").expect("asset");
        assert_eq!(asset.id(), id);
    }

    #[test]
    fn media_asset_preserves_exact_relative_path() {
        let id = MediaAssetId::new(1).expect("id");
        let asset = MediaAsset::new(id, PathBuf::from("media/raw/take_01.mov")).expect("asset");
        assert_eq!(asset.source_path(), Path::new("media/raw/take_01.mov"));
    }

    #[test]
    fn media_asset_rejects_empty_path_buf() {
        let id = MediaAssetId::new(1).expect("id");
        assert_eq!(
            MediaAsset::new(id, PathBuf::new()),
            Err(MediaAssetError::EmptySourcePath)
        );
    }

    #[test]
    fn media_asset_rejects_empty_string_path() {
        let id = MediaAssetId::new(1).expect("id");
        assert_eq!(
            MediaAsset::new(id, ""),
            Err(MediaAssetError::EmptySourcePath)
        );
    }

    #[test]
    fn media_asset_accepts_nonexistent_path_without_filesystem_access() {
        let id = MediaAssetId::new(3).expect("id");
        // Construction succeeds for a path that is not required to exist; no
        // filesystem probe is performed by `MediaAsset::new`.
        let path = PathBuf::from("definitely/does/not/exist/__8b2__/clip.mxf");
        let asset = MediaAsset::new(id, path.clone()).expect("nonexistent path is allowed");
        assert_eq!(asset.source_path(), path.as_path());
    }

    #[test]
    fn media_asset_does_not_normalize_dotdot_components() {
        let id = MediaAssetId::new(4).expect("id");
        let raw = PathBuf::from("folder/../video.mp4");
        let asset = MediaAsset::new(id, raw.clone()).expect("asset");
        assert_eq!(asset.source_path(), raw.as_path());
        assert_eq!(
            asset.source_path().as_os_str(),
            Path::new("folder/../video.mp4").as_os_str()
        );
    }

    #[test]
    fn getters_expose_shared_views_without_interior_mutation() {
        let id = MediaAssetId::new(5).expect("id");
        let asset = MediaAsset::new(id, "src/clip.mp4").expect("asset");

        let returned_id = asset.id();
        let _ = MediaAssetId::new(returned_id.get()).expect("copy is independent");
        assert_eq!(asset.id(), id);

        let mut path_copy = asset.source_path().to_path_buf();
        path_copy.push("mutated");
        assert_eq!(asset.source_path(), Path::new("src/clip.mp4"));
    }

    #[test]
    fn media_asset_is_cloneable_and_comparable() {
        let id = MediaAssetId::new(8).expect("id");
        let a = MediaAsset::new(id, "a.mp4").expect("a");
        let b = a.clone();
        assert_eq!(a, b);
        let c = MediaAsset::new(id, "b.mp4").expect("c");
        assert_ne!(a, c);
    }
}
