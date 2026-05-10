//! Errors raised by the `cpr_gm` crate.
//!
//! Single error type, [`GmError`], used across all GM-layer modules.
//! Pre-staged in the WP-601 prep commit with every failure mode the
//! Phase 6 wave is expected to need. Agents implementing WP-602–613
//! should not add new variants here unless coordinated — see
//! `IMPLEMENTATION_PLAN.md` §5.2 (handling API conflicts).

use crate::ids::{BeatId, EncounterId, FactionId, GigId, MechanicalHookId};
use cpr_rules::types::Eurobucks;
use cpr_rules::RulesError;
use std::path::PathBuf;
use thiserror::Error;

/// Failure modes raised by the `cpr_gm` crate.
///
/// Variants are grouped by the WP that introduces the failing operation.
/// `RulesError` is wrapped via `#[from]` so any rules-engine error
/// surfacing in a GM-layer flow can be propagated with `?`.
#[derive(Debug, Error)]
pub enum GmError {
    /// A `cpr_rules` operation failed inside a GM-layer flow.
    #[error("rules error: {0}")]
    Rules(#[from] RulesError),

    // ── WP-603 / WP-610 / WP-605 — content loading ───────────────────────
    //
    // Note: the message field is named `detail`, not `source`. `thiserror`
    // treats a field literally named `source` as a wrapped `Error` impl.
    /// I/O or parse failure reading a Beat Chart RON file.
    #[error("beat chart load failed for {path}: {detail}")]
    BeatChartLoadFailed {
        /// Filesystem path the loader attempted to read.
        path: PathBuf,
        /// Stringified description of the underlying failure
        /// (an I/O error, a RON parse error, or a validator diagnostic).
        detail: String,
    },
    /// I/O or parse failure reading an Encounter RON file.
    #[error("encounter load failed for {path}: {detail}")]
    EncounterLoadFailed {
        /// Filesystem path the loader attempted to read.
        path: PathBuf,
        /// Stringified description of the underlying failure.
        detail: String,
    },
    /// I/O or parse failure reading an NPC template RON file.
    #[error("npc template load failed for {path}: {detail}")]
    NpcTemplateLoadFailed {
        /// Filesystem path the loader attempted to read.
        path: PathBuf,
        /// Stringified description of the underlying failure.
        detail: String,
    },

    // ── WP-603 — Beat Chart structural validation ────────────────────────
    /// A transition references a beat ID that doesn't exist in the gig.
    #[error("transition target missing in gig '{gig}': beat '{beat}'")]
    TransitionTargetMissing {
        /// Gig containing the bad transition.
        gig: GigId,
        /// Beat ID referenced by the transition that has no definition.
        beat: BeatId,
    },
    /// A beat is unreachable from `start_beat`.
    #[error("orphan beat in gig '{gig}': '{beat}'")]
    OrphanBeat {
        /// Gig the orphan was found in.
        gig: GigId,
        /// Beat that no transition points to.
        beat: BeatId,
    },
    /// No path from `start_beat` reaches a Climax → Resolution chain.
    #[error("gig '{gig}' has no Climax → Resolution path")]
    NoResolutionPath {
        /// Gig that fails the climax-reachability check.
        gig: GigId,
    },
    /// Two mechanical hooks within a gig share the same id.
    #[error("duplicate mechanical hook id in gig '{gig}': '{id}'")]
    DuplicateHookId {
        /// Gig containing the duplicate.
        gig: GigId,
        /// Hook id that appears more than once.
        id: MechanicalHookId,
    },
    /// The `start_beat` is not a `BeatKind::Hook`.
    #[error("start beat in gig '{gig}' is '{found}', expected Hook")]
    StartBeatNotHook {
        /// Gig with the mistyped start beat.
        gig: GigId,
        /// Human-readable name of the actual `BeatKind`.
        found: &'static str,
    },
    /// A beat or transition references an unknown NPC template slug.
    #[error("npc template not found: '{0}'")]
    NpcTemplateNotFound(String),
    /// A beat or encounter references an unknown location slug.
    #[error("location not found: '{0}'")]
    LocationRefNotFound(String),
    /// A beat references an unknown encounter slug.
    #[error("encounter not found: '{0}'")]
    EncounterNotFound(EncounterId),

    // ── WP-604 / WP-613 — runtime state machine ──────────────────────────
    /// A beat ID was referenced that doesn't exist in the active gig.
    #[error("beat '{beat}' not in gig '{gig}'")]
    BeatNotFound {
        /// Active gig.
        gig: GigId,
        /// Beat that was looked up.
        beat: BeatId,
    },
    /// A transition was attempted whose condition isn't currently satisfied.
    #[error("invalid transition from '{from}' in gig '{gig}'")]
    InvalidTransition {
        /// Active gig.
        gig: GigId,
        /// Beat the player was on when the transition was attempted.
        from: BeatId,
    },
    /// A hook id was referenced that isn't in the current beat.
    #[error("mechanical hook '{0}' not in current beat")]
    HookNotInCurrentBeat(MechanicalHookId),

    // ── WP-606 — NPC instantiation ───────────────────────────────────────
    /// A mook archetype slug is missing from the mook statline catalog.
    #[error("mook archetype not found in catalog: '{0}'")]
    MookArchetypeNotFound(String),
    /// A loadout references a catalog item slug that does not resolve.
    #[error("loadout {kind} '{slug}' not found in catalog")]
    LoadoutItemNotFound {
        /// Catalog kind: `"weapon"`, `"armor"`, or `"cyberware"`.
        kind: &'static str,
        /// Slug that failed to resolve.
        slug: String,
    },

    // ── WP-609 — IP awarding (LLM bonus) ─────────────────────────────────
    /// LLM-bonus IP response is malformed (e.g. a negative awarded value).
    #[error("invalid LLM IP bonus response: {0}")]
    InvalidIpBonus(String),

    // ── WP-610 — encounter instantiation ─────────────────────────────────
    /// An `EnemyPlacement` position is outside the grid bounds.
    #[error("encounter '{encounter}': enemy at ({x},{y}) is outside {width}x{height} grid")]
    EnemyPositionOutOfBounds {
        /// Encounter that placed an enemy off-grid.
        encounter: EncounterId,
        /// Enemy x coordinate.
        x: u16,
        /// Enemy y coordinate.
        y: u16,
        /// Grid width.
        width: u16,
        /// Grid height.
        height: u16,
    },
    /// The grid `tiles` string length doesn't match `width * height`.
    #[error("encounter '{encounter}' grid dimension mismatch")]
    GridDimensionMismatch {
        /// Encounter with the malformed grid.
        encounter: EncounterId,
    },

    // ── WP-611 — factions ────────────────────────────────────────────────
    /// A faction id was queried that is not registered.
    #[error("faction not found: '{0}'")]
    FactionNotFound(FactionId),

    // ── WP-612 — NPC ally hiring (Fixer integration) ─────────────────────
    /// Insufficient money to cover the hire cost.
    ///
    /// Distinct from `RulesError::IpInsufficient`. STATUS.md tracks the
    /// reuse of `IpInsufficient` for currency as known debt; this
    /// variant is the dedicated GM-layer money-shortage signal.
    #[error("insufficient funds: required {required:?}, available {available:?}")]
    InsufficientFunds {
        /// Eurobuck amount the operation needed.
        required: Eurobucks,
        /// Eurobuck amount the player actually had.
        available: Eurobucks,
    },
    /// Player's Fixer rank is below the hireable's minimum.
    #[error("hireable '{hireable}' requires Fixer rank {required}, current {current}")]
    FixerRankBelowMin {
        /// Display name or slug of the hireable.
        hireable: String,
        /// Minimum Fixer rank required.
        required: u8,
        /// Player's current Fixer rank.
        current: u8,
    },
    /// The requested hireable isn't available in the current region/rank.
    #[error("hireable '{0}' not available in current region")]
    HireableUnavailable(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_error_converts_via_from() {
        let r: RulesError = RulesError::HumanityBelowZero;
        let g: GmError = r.into();
        assert!(matches!(g, GmError::Rules(_)));
    }

    #[test]
    fn display_includes_path_for_load_failures() {
        let e = GmError::BeatChartLoadFailed {
            path: PathBuf::from("content/gigs/sample.ron"),
            detail: "boom".to_string(),
        };
        let s = format!("{e}");
        assert!(s.contains("sample.ron"));
        assert!(s.contains("boom"));
    }

    #[test]
    fn display_includes_amounts_for_insufficient_funds() {
        let e = GmError::InsufficientFunds {
            required: Eurobucks(500),
            available: Eurobucks(120),
        };
        let s = format!("{e}");
        assert!(s.contains("500"));
        assert!(s.contains("120"));
    }

    #[test]
    fn display_uses_id_slug_not_debug() {
        let e = GmError::FactionNotFound(FactionId::from("maelstrom"));
        let s = format!("{e}");
        assert!(s.contains("maelstrom"));
        // Should be the slug, not `FactionId("maelstrom")`.
        assert!(!s.contains("FactionId"));
    }
}
