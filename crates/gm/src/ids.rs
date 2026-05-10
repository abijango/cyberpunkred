//! Slug-based newtype IDs shared across GM-layer modules.
//!
//! Each ID wraps a `String` slug used to address authored content (gigs,
//! beats, factions, encounters) loaded from RON. Runtime instance IDs
//! (UUIDs) live in `cpr_rules::types` (see [`cpr_rules::types::EntityId`],
//! [`cpr_rules::types::NpcId`]).
//!
//! These types are pre-staged in WP-601 because every Phase 6 WP refers
//! to them. Defining them here once prevents shadow types from appearing
//! in WP-602–613.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! slug_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
        pub struct $name(pub String);

        impl $name {
            /// Borrow the slug as a `&str`.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_string())
            }
        }
    };
}

slug_id!(
    /// Identifier for an authored Gig (Beat Chart). See WP-602.
    GigId
);
slug_id!(
    /// Identifier for a Beat within a Gig. See WP-602.
    BeatId
);
slug_id!(
    /// Identifier for a `MechanicalHook` within a Beat. See WP-602 / WP-613.
    MechanicalHookId
);
slug_id!(
    /// Identifier for an authored combat Encounter. See WP-610.
    EncounterId
);
slug_id!(
    /// Identifier for a Faction. See WP-611.
    FactionId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_returns_inner_slug() {
        let id = BeatId::from("hook-1");
        assert_eq!(format!("{id}"), "hook-1");
    }

    #[test]
    fn from_str_and_string_equivalent() {
        let a = GigId::from("hot_property");
        let b = GigId::from(String::from("hot_property"));
        assert_eq!(a, b);
    }

    #[test]
    fn ron_round_trip() {
        let id = EncounterId::from("warehouse_lobby");
        let serialized = ron::to_string(&id).expect("serialize");
        let back: EncounterId = ron::from_str(&serialized).expect("deserialize");
        assert_eq!(id, back);
    }

    #[test]
    fn as_str_borrows_slug() {
        let id = FactionId::from("maelstrom");
        assert_eq!(id.as_str(), "maelstrom");
    }
}
