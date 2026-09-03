//! Editor selection state.

use crate::ids::{ClipId, MediaAssetId, TrackId};

/// Single-selection target for the editor domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Selection {
    /// Nothing selected.
    #[default]
    None,
    /// A registered media asset.
    MediaAsset(MediaAssetId),
    /// A timeline track.
    Track(TrackId),
    /// A timeline clip.
    Clip(ClipId),
}
