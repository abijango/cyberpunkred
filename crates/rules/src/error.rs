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
    /// A NET Action was attempted but no netrun is currently active on
    /// `World::netrun`.
    ///
    /// The Netrunner must Jack In first (p.198) before any Interface
    /// Ability (save Scanner) can be used.
    ///
    /// See p.198 (Jack In/Out).
    NetrunNotActive,
    /// A Control (or other floor-targeted) ability was used on a floor
    /// that is not a [`crate::netrunning::architecture::Floor::ControlNode`].
    ///
    /// See p.199 (Control): "Take a Control Node at current floor."
    NotAControlNode {
        /// The floor index the Netrunner was on when the attempt was made.
        floor_idx: usize,
    },
    /// A NET Action was attempted but the Netrunner has already consumed
    /// all their NET Actions for this turn.
    ///
    /// See p.197 (NET Actions per turn table): the number of NET Actions is
    /// determined by Interface rank.
    NoNetActionsRemaining,

    /// A [`crate::netrunning::abilities::slide::SlideAction`] was rejected
    /// because the target floor is occupied by a Demon rather than a Black
    /// ICE program.
    ///
    /// Per p.200: "Attempt to flee combat with a single Non-Demon Black ICE
    /// Program as a NET Action." Demons cannot be Slid.
    ///
    /// See p.200 (Slide Interface Ability).
    CannotSlideDemon,

    /// A [`crate::netrunning::abilities::slide::SlideAction`] was rejected
    /// because the Slide action has already been used this turn.
    ///
    /// Per p.200: "You can only attempt to Slide once per Turn."
    ///
    /// See p.200 (Slide Interface Ability).
    SlideAlreadyUsedThisTurn,

    /// A [`crate::netrunning::abilities::slide::SlideAction`] was rejected
    /// because the target floor index does not point to a Black ICE or Demon
    /// floor in the current architecture.
    ///
    /// The Slide ability requires an active Black ICE opponent; attempting
    /// it on a Password, ControlNode, File, or an out-of-bounds index is
    /// illegal.
    ///
    /// See p.200 (Slide Interface Ability).
    SlideTargetNotBlackIce {
        /// The floor index the action targeted.
        floor_idx: usize,
    },
    /// A Virus deployment was attempted but the Netrunner is not on the
    /// bottom (deepest) floor of the NET Architecture.
    ///
    /// Per p.200: "Once you have reached the lowest level of the NET
    /// Architecture you can leave your own Virus in the Architecture."
    ///
    /// See p.200 (Virus Interface Ability).
    NotOnBottomFloor {
        /// The floor the Netrunner is currently on (0-indexed from top).
        current_floor: usize,
        /// The index of the deepest revealed floor (`revealed_floors - 1`).
        bottom_floor: usize,
    },
    /// A program slug was referenced but no entry with that slug exists in
    /// the programs catalog (WP-208 / `content/catalogs/programs.ron`).
    ///
    /// Raised by [`crate::netrunning::programs::attackers::activate_attacker`]
    /// when the requested program or target program slug is unknown.
    ///
    /// See p.201 (Programs).
    ProgramNotFound(ProgramId),
    /// An Attacker activation was requested for a program whose class is not
    /// `AntiPersonnelAttacker` or `AntiProgramAttacker`.
    ///
    /// Booster and Defender programs are activated passively (they apply their
    /// effect while Rezzed); they cannot be used as an Attacker. See p.202:
    /// "The Three Kinds of Non-Black ICE Programs."
    ///
    /// See p.201, p.202.
    WrongProgramClass {
        /// The slug of the program that was rejected.
        program: ProgramId,
        /// Human-readable description of what class was expected.
        expected: String,
        /// Human-readable description of the class that was found.
        found: String,
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
            RulesError::CatalogLoadFailed { path, source } => {
                write!(f, "catalog load failed for {}: {source}", path.display())
            }
            RulesError::NetrunNotActive => {
                write!(f, "no active netrun: must Jack In before using Interface Abilities")
            }
            RulesError::NotAControlNode { floor_idx } => {
                write!(f, "floor {floor_idx} is not a Control Node")
            }
            RulesError::NoNetActionsRemaining => {
                write!(f, "no NET Actions remaining this turn")
            }
            RulesError::CannotSlideDemon => {
                write!(
                    f,
                    "cannot Slide a Demon: Slide only works on Non-Demon Black ICE (p.200)"
                )
            }
            RulesError::SlideAlreadyUsedThisTurn => {
                write!(
                    f,
                    "Slide already used this turn: can only attempt Slide once per Turn (p.200)"
                )
            }
            RulesError::SlideTargetNotBlackIce { floor_idx } => {
                write!(
                    f,
                    "floor {floor_idx} is not a Black ICE floor; Slide requires a Black ICE target (p.200)"
                )
            }
            RulesError::NotOnBottomFloor {
                current_floor,
                bottom_floor,
            } => write!(
                f,
                "cannot deploy Virus: at floor {current_floor}, but must be on the bottom floor {bottom_floor} (p.200)"
            ),
            RulesError::ProgramNotFound(id) => {
                write!(f, "program not found in catalog: '{}'", id.0)
            }
            RulesError::WrongProgramClass {
                program,
                expected,
                found,
            } => write!(
                f,
                "program '{}' has wrong class: expected {expected}, found {found} (p.202)",
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
