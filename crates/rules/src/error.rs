//! Errors raised by the `cpr_rules` crate.
//!
//! A single error type, [`RulesError`], covers all rules-engine failure
//! modes. Variants are added as later WPs introduce new failure cases
//! (e.g. invalid skill check inputs, illegal combat actions). Callers
//! pattern-match on the variant; the [`std::fmt::Display`] impl produces
//! a human-readable message for logs / UI.

use crate::effects::ProgramId;
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
    /// The number of NET Actions per turn is determined by Interface rank
    /// (p.197). `net_actions_used_this_turn >= net_actions_max_this_turn`
    /// triggers this error; state is left unchanged.
    ///
    /// See p.197 (NET Actions table).
    NoNetActionsRemaining,
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

    /// A program activation was attempted with an unknown catalog slug.
    ///
    /// Raised by [`crate::netrunning::programs::active::activate_booster_or_defender`]
    /// when the slug in [`crate::netrunning::programs::active::ActivateProgram::program`]
    /// does not appear in the provided [`crate::catalog::Catalog<Program>`].
    ///
    /// See p.201.
    ProgramNotFound(ProgramId),

    /// A program activation was attempted with the wrong program class.
    ///
    /// [`crate::netrunning::programs::active::activate_booster_or_defender`]
    /// only handles `Booster` and `Defender` programs. Attacker programs are
    /// handled by WP-413. Supplying an Attacker slug returns this error.
    ///
    /// See p.201.
    ProgramWrongClass {
        /// The program slug that was supplied.
        program: ProgramId,
        /// What class was expected (human-readable).
        expected: &'static str,
        /// What class the program actually has (human-readable).
        got: String,
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
            RulesError::CatalogLoadFailed { path, source } => {
                write!(f, "catalog load failed for {}: {source}", path.display())
            }
            RulesError::ProgramNotFound(id) => {
                write!(f, "program not found in catalog: '{}'", id.0)
            }
            RulesError::ProgramWrongClass {
                program,
                expected,
                got,
            } => write!(
                f,
                "program '{}' has wrong class: expected {expected}, got {got}",
                program.0
            ),
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
