//! Faction definitions and reputation tracking.
//!
//! Populated by Phase 6 work packages:
//! - WP-611 — Faction and reputation tracking
//!
//! See `IMPLEMENTATION_PLAN.md` §6.
//!
//! Implements faction definitions and reputation drift driven by
//! `CampaignLog` events. See `IMPLEMENTATION_PLAN.md` §6 (WP-611).

#![forbid(unsafe_code)]

use crate::ids::FactionId;
use crate::log::types::{CampaignLog, LogEvent, LogEventKind};
use crate::npc::entity::NpcTemplateId;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// FactionDef
// ---------------------------------------------------------------------------

/// Definition of a single faction, including its slug ID, display name, and
/// the NPC templates that count as members.
///
/// Faction standing is tracked on the [`CampaignLog`] (keyed by
/// [`FactionId`]), not on this struct — `FactionDef` is authored content
/// (RON-loadable). Standing is mutable runtime state.
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FactionDef {
    /// Slug identifier for this faction (e.g. `"maelstrom"`, `"arasaka"`).
    pub id: FactionId,
    /// Human-readable name shown in the UI and LLM context
    /// (e.g. `"Maelstrom"`, `"Arasaka Corporation"`).
    pub display_name: String,
    /// NPC templates whose death or endangerment affects faction standing.
    ///
    /// Slugs must resolve against the loaded NPC template catalog; unresolved
    /// slugs are silently skipped by `update_faction_standing`.
    pub members: Vec<NpcTemplateId>,
}

// ---------------------------------------------------------------------------
// update_faction_standing
// ---------------------------------------------------------------------------

/// Update faction standing on a [`CampaignLog`] in response to a single
/// [`LogEvent`].
///
/// Standing is clamped to `−10..=+10` by
/// [`CampaignLog::set_faction_standing`] — callers do not need to clamp
/// themselves. Unknown factions (those not present in `factions`) are
/// silently ignored; the caller is responsible for populating the map before
/// calling this function.
///
/// ## Per-event mapping
///
/// | `LogEventKind` variant                             | Effect |
/// |-----------------------------------------------------|--------|
/// | `NpcKilled { by_player: true, .. }`                | For each faction whose `members` contains the killed NPC, decrement standing by **2** (clamped to −10). The player is directly responsible — the harshest penalty. |
/// | `NpcKilled { by_player: false, witnesses, .. }`    | For each faction whose members appear in `witnesses`, decrement standing by **1**. The player was present/complicit but not the killer — a lighter penalty. |
/// | `GigCompleted { outcome: Success, .. }`            | No faction effect by default. Future PRs that attach faction tags to gigs will refine this. |
/// | All other variants                                  | No faction effect. |
///
/// ## Witness vs killer asymmetry
///
/// The two-tier penalty (killer: −2, witness faction member: −1) reflects the
/// Cyberpunk RED world's social logic: factions track *who pulls the trigger*
/// most severely, but still note *who was there when it happened*. A player
/// who lets an ally handle a kill but stands by suffers reputational bleed,
/// not full culpability.
pub fn update_faction_standing(
    log: &mut CampaignLog,
    factions: &HashMap<FactionId, FactionDef>,
    event: &LogEvent,
) {
    match &event.kind {
        // ── Player killed an NPC ─────────────────────────────────────────
        // Decrement standing by 2 for each faction the killed NPC belongs to.
        // The player is directly responsible — harshest standing penalty.
        LogEventKind::NpcKilled {
            npc,
            by_player: true,
            ..
        } => {
            for (faction_id, faction_def) in factions {
                if faction_def.members.contains(npc) {
                    let current = log.faction_standing.get(faction_id).copied().unwrap_or(0i8);
                    // Use i16 arithmetic to avoid i8 overflow before clamping
                    let new_value = (current as i16 - 2).clamp(-10, 10) as i8;
                    log.set_faction_standing(faction_id.clone(), new_value);
                }
            }
        }

        // ── NPC killed, not by the player, but faction members witnessed ─
        // Decrement standing by 1 for factions with a member in `witnesses`.
        // Player was present/complicit but not the killer — lighter penalty.
        LogEventKind::NpcKilled {
            by_player: false,
            witnesses,
            ..
        } => {
            for (faction_id, faction_def) in factions {
                // Check if any witness is a member of this faction
                let faction_witnessed = witnesses
                    .iter()
                    .any(|witness| faction_def.members.contains(witness));
                if faction_witnessed {
                    let current = log.faction_standing.get(faction_id).copied().unwrap_or(0i8);
                    // Use i16 arithmetic to avoid i8 overflow before clamping
                    let new_value = (current as i16 - 1).clamp(-10, 10) as i8;
                    log.set_faction_standing(faction_id.clone(), new_value);
                }
            }
        }

        // ── GigCompleted (Success) ───────────────────────────────────────
        // Neutral; no faction effect by default. Future PRs (e.g. gig-specific
        // faction tags) will wire in standing changes here.
        LogEventKind::GigCompleted { .. } => {}

        // ── All other variants ───────────────────────────────────────────
        // No faction reputation effect.
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::types::{GameClockSnapshot, LogEventKind};
    use cpr_rules::types::CharacterId;
    use uuid::Uuid;

    fn test_character_id() -> CharacterId {
        CharacterId(Uuid::from_u128(0x611_0000_0000_0000))
    }

    fn maelstrom_id() -> FactionId {
        FactionId::from("maelstrom")
    }

    fn garcia_id() -> NpcTemplateId {
        NpcTemplateId::from("maelstrom_garcia")
    }

    fn ramos_id() -> NpcTemplateId {
        NpcTemplateId::from("maelstrom_ramos")
    }

    fn make_maelstrom_faction(members: Vec<NpcTemplateId>) -> FactionDef {
        FactionDef {
            id: maelstrom_id(),
            display_name: "Maelstrom".to_string(),
            members,
        }
    }

    fn make_factions(def: FactionDef) -> HashMap<FactionId, FactionDef> {
        let mut map = HashMap::new();
        map.insert(def.id.clone(), def);
        map
    }

    fn make_event(kind: LogEventKind) -> LogEvent {
        LogEvent {
            at: GameClockSnapshot(0),
            kind,
        }
    }

    /// `test_killing_member_lowers_faction` — killing a faction member with
    /// `by_player: true` drops standing by 2 from the initial 0.
    #[test]
    fn test_killing_member_lowers_faction() {
        let mut log = CampaignLog::new(test_character_id());
        // log.faction_standing is empty (default 0 standing)
        let factions = make_factions(make_maelstrom_faction(vec![garcia_id()]));

        let event = make_event(LogEventKind::NpcKilled {
            npc: garcia_id(),
            by_player: true,
            witnesses: vec![],
        });

        update_faction_standing(&mut log, &factions, &event);

        assert_eq!(
            log.faction_standing.get(&maelstrom_id()).copied(),
            Some(-2i8),
            "killing a Maelstrom member should drop standing to -2"
        );
    }

    /// `test_floors_at_minus_10` — standing cannot drop below -10 no matter
    /// how many kills occur.
    #[test]
    fn test_floors_at_minus_10() {
        let mut log = CampaignLog::new(test_character_id());
        // Pre-set Maelstrom standing to -9
        log.set_faction_standing(maelstrom_id(), -9);

        let factions = make_factions(make_maelstrom_faction(vec![garcia_id(), ramos_id()]));

        // First kill: -9 - 2 = -11, but clamped to -10
        let event1 = make_event(LogEventKind::NpcKilled {
            npc: garcia_id(),
            by_player: true,
            witnesses: vec![],
        });
        update_faction_standing(&mut log, &factions, &event1);

        // Should already be -10
        assert_eq!(
            log.faction_standing.get(&maelstrom_id()).copied(),
            Some(-10i8),
            "standing must clamp at -10 after first kill"
        );

        // Second kill: still -10 (floor holds)
        let event2 = make_event(LogEventKind::NpcKilled {
            npc: ramos_id(),
            by_player: true,
            witnesses: vec![],
        });
        update_faction_standing(&mut log, &factions, &event2);

        assert_eq!(
            log.faction_standing.get(&maelstrom_id()).copied(),
            Some(-10i8),
            "standing must remain at -10 (floor) after second kill"
        );
    }

    /// `test_unknown_faction_silently_ignored` — with an empty factions map
    /// the function completes without panic and does not mutate the log.
    #[test]
    fn test_unknown_faction_silently_ignored() {
        let mut log = CampaignLog::new(test_character_id());
        let factions: HashMap<FactionId, FactionDef> = HashMap::new();

        let event = make_event(LogEventKind::NpcKilled {
            npc: garcia_id(),
            by_player: true,
            witnesses: vec![],
        });

        // Must not panic
        update_faction_standing(&mut log, &factions, &event);

        // Log faction_standing must remain empty (no mutation)
        assert!(
            log.faction_standing.is_empty(),
            "empty factions map must produce no standing change"
        );
    }

    /// `test_witness_decrement_lighter_than_killer` — NpcKilled with
    /// `by_player: false` but a faction member in `witnesses` decrements
    /// standing by 1 (not 2).
    #[test]
    fn test_witness_decrement_lighter_than_killer() {
        let mut log = CampaignLog::new(test_character_id());
        // Standing starts at 0
        let factions = make_factions(make_maelstrom_faction(vec![garcia_id()]));

        let unrelated_npc = NpcTemplateId::from("random_civilian");
        let event = make_event(LogEventKind::NpcKilled {
            npc: unrelated_npc,
            by_player: false,
            // A Maelstrom member witnessed the killing
            witnesses: vec![garcia_id()],
        });

        update_faction_standing(&mut log, &factions, &event);

        assert_eq!(
            log.faction_standing.get(&maelstrom_id()).copied(),
            Some(-1i8),
            "witness decrement must be -1, not -2"
        );
    }
}
