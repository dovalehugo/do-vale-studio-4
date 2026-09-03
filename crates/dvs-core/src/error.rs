//! Domain errors for editor operations.

/// Error returned by timeline, media-pool, and edit commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditorError {
    /// An identity value is already present in the project.
    DuplicateId,
    /// The referenced media asset is not registered in the media pool.
    AssetNotFound,
    /// The referenced track does not exist.
    TrackNotFound,
    /// The referenced clip does not exist.
    ClipNotFound,
    /// A required display name was empty.
    EmptyName,
    /// The media asset is still referenced by one or more clips.
    AssetInUse,
    /// The destination track kind is incompatible with the clip.
    IncompatibleTrackKind,
    /// A timeline duration must be strictly greater than zero.
    ZeroDuration,
    /// A timeline range or trim produced an invalid span.
    InvalidRange,
    /// Checked timeline arithmetic overflowed.
    TimeOverflow,
    /// Two clips on the same track overlap in time.
    Overlap,
    /// A split position must lie strictly inside the clip.
    SplitNotInterior,
    /// There is no command available to undo.
    NothingToUndo,
    /// There is no command available to redo.
    NothingToRedo,
    /// Selection targets an object that is not present in the project.
    InvalidSelection,
}
