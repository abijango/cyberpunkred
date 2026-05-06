//! The live game-state container for a play session.
//!
//! [`World`] holds the player character, on-scene NPCs, current location,
//! optional combat / netrun / gig sub-state, and a game clock. It is the
//! mutable argument every [`crate::resolution::Resolution::resolve`] takes —
//! and the single place mutable game state lives during a play session.
//!
//! Held types like [`CombatState`] / [`NetrunState`] / [`GigState`] are
//! intentionally empty here; later WPs (WP-301 combat, WP-401 netrun,
//! WP-604 gig orchestration) populate them. Defining the slots now lets
//! every downstream WP refer to a stable [`World`] shape.

use crate::character::Character;
use crate::types::{EntityId, NpcId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Live game state for a play session.
///
/// `World` is **mutable** — the resolution machinery threads `&mut World`
/// through every roll site so combat damage, scene transitions, clock
/// advances, etc. can be applied in place. Saves are produced by serialising
/// this struct.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct World {
    /// The player character — the focal entity of the game session.
    pub pc: Character,
    /// On-scene NPCs (allies and adversaries) keyed by their persistent
    /// [`NpcId`]. Per-scene `EntityId ↔ NpcId` mapping for combat-grid
    /// lookup is owned by the combat layer, not by `World`.
    pub npcs: HashMap<NpcId, Character>,
    /// Current location, if known. `None` between scenes (e.g. travelling).
    pub location: Option<LocationId>,
    /// Active combat state, if any. `Some` only during a combat encounter.
    pub combat: Option<CombatState>,
    /// Active netrun state, if any. `Some` only during a netrun.
    pub netrun: Option<NetrunState>,
    /// Active gig state, if any. `Some` only during a gig.
    pub gig: Option<GigState>,
    /// In-fiction time, minute resolution.
    pub clock: GameClock,
}

impl World {
    /// Build a fresh world centred on `pc`.
    ///
    /// NPC and location slots are empty; the clock starts at day 1 / 00:00;
    /// no combat / netrun / gig is active.
    pub fn new(pc: Character) -> Self {
        Self {
            pc,
            npcs: HashMap::new(),
            location: None,
            combat: None,
            netrun: None,
            gig: None,
            clock: GameClock {
                day: 1,
                minutes_into_day: 0,
            },
        }
    }

    /// Resolve a grid [`EntityId`] to the underlying character, if any.
    ///
    /// At this layer the only mapping known is *PC ↔ EntityId*, derived from
    /// the PC's [`crate::types::CharacterId`] (same UUID, different newtype).
    /// NPC entity-id resolution is a per-scene concern owned by combat:
    /// when a combat encounter mints `EntityId`s for the participating NPCs,
    /// it stores the `EntityId → NpcId` map inside [`CombatState`].
    pub fn entity(&self, id: EntityId) -> Option<&Character> {
        if id.0 == self.pc.id.0 {
            Some(&self.pc)
        } else {
            None
        }
    }

    /// Mutable analogue of [`Self::entity`].
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Character> {
        if id.0 == self.pc.id.0 {
            Some(&mut self.pc)
        } else {
            None
        }
    }
}

/// Game clock — tracks in-fiction time at minute resolution.
///
/// `minutes_into_day` is bounded to `0..1440` (24 × 60) by convention. The
/// type does **not** enforce this — combat / scene code is responsible for
/// rolling over to the next day and zeroing the field. This keeps the
/// container small and lets the canonical day-roll happen exactly once,
/// in the time-advancement code, rather than on every assignment.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GameClock {
    /// Day index. Conventionally starts at `1` for the first session day.
    pub day: u32,
    /// Minutes elapsed since midnight. `0..1440`.
    pub minutes_into_day: u16,
}

/// Combat state placeholder. Populated by a later WP.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatState;

/// Netrun state placeholder. Populated by a later WP.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetrunState;

/// Gig state placeholder. Populated by a later WP.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GigState;

/// Location identifier — content slug into the location catalog.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct LocationId(pub String);

#[cfg(test)]
pub(crate) mod test_support {
    //! Test helpers shared across `cpr_rules` test modules.
    //!
    //! Currently exposes [`fresh_pc`] — a minimal valid `Character` for use
    //! anywhere a real PC is required to construct a [`World`] (for example,
    //! the `Resolution`-trait test in `resolution::tests`).

    use super::*;
    use crate::character::{Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::effects::EffectStack;
    use crate::types::{CharacterId, Eurobucks};
    use uuid::Uuid;

    /// Construct a minimal valid PC. Stats are typical Solo numbers; every
    /// collection field uses [`Default`]. The character is unwounded
    /// (`current_state == WoundState::None` via `Wounds::default`) and has
    /// no cyberware, items, or active effects.
    pub(crate) fn fresh_pc() -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xC1)),
            name: "Test PC".into(),
            handle: None,
            role: Role::Solo,
            role_rank: 4,
            stats: StatBlock {
                int: 5,
                r#ref: 7,
                dex: 6,
                tech: 4,
                cool: 5,
                will: 5,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds::default(),
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::fresh_pc;
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_world_construction() {
        let pc = fresh_pc();
        let world = World::new(pc.clone());

        assert_eq!(world.pc, pc);
        assert!(world.npcs.is_empty(), "no NPCs at construction");
        assert!(world.location.is_none(), "no location at construction");
        assert!(world.combat.is_none(), "no combat at construction");
        assert!(world.netrun.is_none(), "no netrun at construction");
        assert!(world.gig.is_none(), "no gig at construction");
        assert_eq!(world.clock.day, 1, "clock starts at day 1");
        assert_eq!(world.clock.minutes_into_day, 0, "clock starts at 00:00");
    }

    #[test]
    fn test_entity_lookup_pc() {
        let pc = fresh_pc();
        let pc_uuid = pc.id.0;
        let world = World::new(pc.clone());

        let pc_entity = EntityId(pc_uuid);
        let found = world.entity(pc_entity);
        assert_eq!(found, Some(&pc));
    }

    #[test]
    fn test_entity_lookup_missing() {
        let world = World::new(fresh_pc());
        let unknown = EntityId(Uuid::from_u128(0xDEADBEEF));
        assert!(world.entity(unknown).is_none());

        let mut world = world;
        assert!(world.entity_mut(unknown).is_none());
    }
}
