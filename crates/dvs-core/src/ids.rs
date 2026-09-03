//! Typed identity values for the Do Vale Studio 4 domain.
//!
//! Each ID is a distinct newtype over [`NonZeroU64`]. Zero is reserved and
//! rejected at construction so an ID always represents a concrete entity.
//! Controllers and persistence allocate values later; these types never generate
//! IDs from global state, atomics, or randomness.

use std::num::NonZeroU64;

/// Error returned when a domain identity value is rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IdError {
    /// Zero is reserved and cannot identify an entity.
    ZeroReserved,
}

macro_rules! domain_id {
    (
        $(#[$meta:meta])*
        $name:ident
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(NonZeroU64);

        impl $name {
            /// Creates an ID from a non-zero primitive value.
            ///
            /// Returns [`IdError::ZeroReserved`] when `value == 0`.
            pub const fn new(value: u64) -> Result<Self, IdError> {
                match NonZeroU64::new(value) {
                    Some(inner) => Ok(Self(inner)),
                    None => Err(IdError::ZeroReserved),
                }
            }

            /// Returns the primitive representation of this ID.
            pub const fn get(self) -> u64 {
                self.0.get()
            }
        }
    };
}

domain_id!(
    /// Stable identity of a project document in the domain model.
    ProjectId
);

domain_id!(
    /// Stable identity of a media asset referenced by the project.
    MediaAssetId
);

domain_id!(
    /// Stable identity of a timeline track (model defined in a later phase).
    TrackId
);

domain_id!(
    /// Stable identity of a timeline clip (model defined in a later phase).
    ClipId
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn project_id_rejects_zero_and_round_trips() {
        assert_eq!(ProjectId::new(0), Err(IdError::ZeroReserved));
        let id = ProjectId::new(1).expect("non-zero");
        assert_eq!(id.get(), 1);
        assert_eq!(ProjectId::new(u64::MAX).expect("max").get(), u64::MAX);
    }

    #[test]
    fn media_asset_id_rejects_zero_and_round_trips() {
        assert_eq!(MediaAssetId::new(0), Err(IdError::ZeroReserved));
        let id = MediaAssetId::new(42).expect("non-zero");
        assert_eq!(id.get(), 42);
    }

    #[test]
    fn track_and_clip_ids_reject_zero_and_round_trip() {
        assert_eq!(TrackId::new(0), Err(IdError::ZeroReserved));
        assert_eq!(ClipId::new(0), Err(IdError::ZeroReserved));
        assert_eq!(TrackId::new(7).expect("track").get(), 7);
        assert_eq!(ClipId::new(9).expect("clip").get(), 9);
    }

    #[test]
    fn ids_support_eq_ord_and_hash() {
        let a = ProjectId::new(1).expect("a");
        let b = ProjectId::new(2).expect("b");
        let a2 = ProjectId::new(1).expect("a2");
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert!(a < b);

        let mut set = HashSet::new();
        assert!(set.insert(a));
        assert!(!set.insert(a2));
        assert!(set.insert(b));
    }

    #[test]
    fn distinct_id_domains_are_separate_types() {
        let project = ProjectId::new(1).expect("project");
        let media = MediaAssetId::new(1).expect("media");
        let track = TrackId::new(1).expect("track");
        let clip = ClipId::new(1).expect("clip");

        // Same primitive, different typed wrappers — cannot be compared across domains.
        assert_eq!(project.get(), media.get());
        assert_eq!(track.get(), clip.get());

        let mut projects = HashSet::new();
        let mut media_assets = HashSet::new();
        assert!(projects.insert(project));
        assert!(media_assets.insert(media));
        assert_eq!(projects.len(), 1);
        assert_eq!(media_assets.len(), 1);
    }

    #[test]
    fn ids_are_not_produced_by_implicit_global_state() {
        // Construction is pure and caller-supplied; repeated calls with the same
        // input yield equal values without advancing any generator.
        let first = MediaAssetId::new(99).expect("first");
        let second = MediaAssetId::new(99).expect("second");
        assert_eq!(first, second);
        assert_eq!(first.get(), 99);
    }
}
