//! Errors raised by the `cpr_rules` crate.
//!
//! A single error type, [`RulesError`], covers all rules-engine failure
//! modes. Variants are added as later WPs introduce new failure cases
//! (e.g. invalid skill check inputs, illegal combat actions). Callers
//! pattern-match on the variant; the [`std::fmt::Display`] impl produces
//! a human-readable message for logs / UI.

use crate::types::EntityId;
use std::fmt;
use std::path::PathBuf;

/// Failure modes raised by the `cpr_rules` crate.
///
/// New variants will be added as later Work Packages introduce additional
/// rules-engine failure cases. Callers should `match` exhaustively and
/// expect the enum to grow over time.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RulesError {
    /// A LUCK spend was attempted but the remaining pool could not
    /// cover the request. The character's `luck_pool` is unchanged.
    /// See rulebook p.130 ("Using Your LUCK").
    InsufficientLuck {
        /// The number of LUCK Points the caller asked to spend.
        requested: u8,
        /// The number of LUCK Points actually in the pool at the time
        /// of the request.
        available: u8,
    },
    /// A resolution referenced an [`EntityId`] that does not resolve to
    /// any character in the current [`crate::world::World`] — neither the
    /// PC nor any on-scene NPC. Raised by check / attack resolutions
    /// when the actor (or defender) cannot be found.
    EntityNotFound(EntityId),
    /// A NET Action was attempted but there is no active `NetrunState` in
    /// `world.netrun`. This indicates the Netrunner is not jacked in.
    ///
    /// See p.198 (Jack In/Out) — only jacked-in Netrunners can use Interface
    /// Abilities.
    NoActiveNetrun,
    /// A NET Action was attempted but the Netrunner has already exhausted
    /// their NET Action budget for this turn.
    ///
    /// See p.197 (NET Actions table).
    NoNetActionsRemaining,
    /// A NET Action requires a specific floor type but the current floor is
    /// a different type. For example, Backdoor (p.199) can only be used on
    /// a [`crate::netrunning::architecture::Floor::Password`] floor.
    ///
    /// See p.199 (Backdoor Interface Ability).
    WrongFloorType {
        /// Human-readable description of the expected floor type.
        expected: &'static str,
        /// Human-readable description of what was found instead.
        found: &'static str,
    },
    /// A Phase 2 catalog loader (skills, weapons, armor, …) failed to
    /// read or parse its on-disk RON file, or rejected the file's
    /// contents on a domain-level invariant (duplicate slug,
    /// linked-stat disagreement, etc.). The `path` and `source` are
    /// surfaced for diagnostic logging in `tools/content-validator`.
    CatalogLoadFailed {
        /// Filesystem path that the loader attempted to read.
        path: PathBuf,
        /// Stringified description of the underlying failure (an I/O
        /// error message, a RON parse error, or a loader-enforced
        /// invariant violation).
        source: String,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::InsufficientLuck {
                requested,
                available,
            } => write!(
                f,
                "insufficient LUCK: requested {requested}, available {available}"
            ),
            RulesError::EntityNotFound(id) => {
                write!(f, "entity not found in world: {:?}", id.0)
            }
            RulesError::NoActiveNetrun => {
                write!(f, "no active netrun: Netrunner is not jacked in")
            }
            RulesError::NoNetActionsRemaining => {
                write!(f, "no NET Actions remaining for this turn")
            }
            RulesError::WrongFloorType { expected, found } => {
                write!(f, "wrong floor type: expected {expected}, found {found}")
            }
            RulesError::CatalogLoadFailed { path, source } => {
                write!(f, "catalog load failed for {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RulesError {}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_insufficient_luck_display() {
        let e = RulesError::InsufficientLuck {
            requested: 5,
            available: 2,
        };
        let s = format!("{e}");
        assert!(s.contains("insufficient LUCK"));
        assert!(s.contains("5"));
        assert!(s.contains("2"));
    }

    #[test]
    fn test_entity_not_found_display() {
        let id = EntityId(Uuid::from_u128(0xC0FFEE));
        let e = RulesError::EntityNotFound(id);
        let s = format!("{e}");
        assert!(s.contains("entity not found"));
    }

    #[test]
    fn test_catalog_load_failed_display() {
        let e = RulesError::CatalogLoadFailed {
            path: PathBuf::from("content/catalogs/skills.ron"),
            source: "boom".to_string(),
        };
        let s = format!("{e}");
        assert!(s.contains("catalog load failed"));
        assert!(s.contains("skills.ron"));
        assert!(s.contains("boom"));
    }
}
