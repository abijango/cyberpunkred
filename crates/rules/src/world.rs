//! The live game-state container for a play session.
//!
//! [`World`] holds the player character, on-scene NPCs, current location,
//! optional combat / netrun / gig sub-state, and a game clock. It is the
//! mutable argument every [`crate::resolution::Resolution::resolve`] takes —
//! and the single place mutable game state lives during a play session.
//!
//! [`CombatState`] is now the real type from WP-301 (`crate::combat`).
//! [`NetrunState`] / [`GigState`] remain placeholders until WP-401 / WP-604
//! land. Defining the slots now lets every downstream WP refer to a stable
//! [`World`] shape.

use crate::character::Character;
use crate::combat::CombatState;
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
    /// Lookup walks the PC first, then the NPC table. The mapping is by
    /// UUID equality: an [`EntityId`] resolves to a [`Character`] iff the
    /// underlying [`uuid::Uuid`] matches that character's
    /// [`crate::types::CharacterId`] (PC) or its [`NpcId`] (NPC) — both of
    /// which are different newtypes around the *same* UUID per WP-006's
    /// design intent.
    ///
    /// Combat-engine code that mints fresh `EntityId`s for ad-hoc grid
    /// participants (drones, summoned constructs) still owns its own
    /// `EntityId → ...` map inside [`CombatState`]; this method covers the
    /// common case where the entity is a known character.
    pub fn entity(&self, id: EntityId) -> Option<&Character> {
        if id.0 == self.pc.id.0 {
            return Some(&self.pc);
        }
        self.npcs
            .iter()
            .find_map(|(npc_id, c)| (npc_id.0 == id.0).then_some(c))
    }

    /// Mutable analogue of [`Self::entity`].
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Character> {
        if id.0 == self.pc.id.0 {
            return Some(&mut self.pc);
        }
        self.npcs
            .iter_mut()
            .find_map(|(npc_id, c)| (npc_id.0 == id.0).then_some(c))
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
            complementary_bonuses: Vec::new(),
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

    #[test]
    fn test_entity_lookup_npc() {
        // An NPC inserted into `World::npcs` must resolve via `entity()` /
        // `entity_mut()`. The lookup matches `EntityId.0 == NpcId.0`
        // (same UUID, different newtype).
        let mut npc = fresh_pc();
        let npc_uuid = Uuid::from_u128(0xA1A1A1);
        npc.id = crate::types::CharacterId(npc_uuid);
        let mut world = World::new(fresh_pc());
        world
            .npcs
            .insert(crate::types::NpcId(npc_uuid), npc.clone());

        let entity = EntityId(npc_uuid);
        assert_eq!(world.entity(entity), Some(&npc));
        assert!(world.entity_mut(entity).is_some());
    }
}
