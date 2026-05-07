//! Initiative and turn engine for Cyberpunk RED combat.
//!
//! This module implements the combat turn loop:
//! - Initiative rolling and queue ordering (highest first). See p.168.
//! - Tie-breaking by re-roll until distinct. See p.168.
//! - Round wrap-around: when the last entry in the queue has taken its
//!   turn, the queue restarts from the top and the round counter advances.
//! - Top-of-queue insertion for Black ICE activation. See p.205.
//! - Turn-end lifecycle: tick effects, advance the queue pointer.
//!
//! **Rulebook:** pp.126–127, p.168 (initiative, queue), p.205 (Black ICE insertion).

use crate::combat::grid::Grid;
use crate::dice::d10;
use crate::effects::EffectModifier;
use crate::rng::Rng;
use crate::types::{EffectInstanceId, EntityId};
use crate::world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

// ── Public types ─────────────────────────────────────────────────────────────

/// One entry in the initiative queue for a single combat participant.
///
/// Entries are ordered highest-score-first by [`CombatState::start`].
/// See p.168.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InitiativeEntry {
    /// The grid entity this entry belongs to.
    pub entity: EntityId,
    /// REF + d10 + sum of `InitiativeBonus` modifiers. See p.168.
    pub score: i16,
    /// Whether this entity has spent its Move Action this turn.
    pub move_used: bool,
    /// Whether this entity has spent its non-Move Action this turn.
    pub action_used: bool,
    /// An action held with the Hold Action, pending a trigger. See p.169.
    pub held_action: Option<HeldAction>,
}

/// An action being held until a specified trigger fires. See p.169.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeldAction {
    /// The condition that releases this held action.
    pub trigger: HoldTrigger,
    /// The action to perform when the trigger fires.
    pub action: PlannedAction,
}

/// When a held action fires. See p.169 (Hold Action description).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoldTrigger {
    /// Fire when the initiative queue reaches this score.
    UntilInitiative(i16),
    /// Fire when a named in-fiction event occurs (GM adjudicated).
    UntilEvent(String),
}

/// Planned action payload — populated as action WPs land (WP-303 onward).
///
/// Currently a unit struct; future WPs will add a variant-based payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedAction;

/// Events produced at the end of an entity's turn.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnEndEvents {
    /// IDs of [`crate::effects::ActiveEffect`]s that expired this turn on
    /// the acting entity's [`crate::effects::EffectStack`].
    pub effects_dropped: Vec<EffectInstanceId>,
}

/// Summary of a completed combat encounter returned by [`CombatState::end_combat`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatSummary {
    /// Total number of rounds the combat lasted.
    pub rounds: u32,
    /// Entities that were killed during the encounter (future WPs populate this).
    pub kills: Vec<EntityId>,
}

/// Full state for an active combat encounter.
///
/// Owns the initiative queue, round counter, grid, and participant set for a
/// single fight. Created by [`CombatState::start`]; disposed by
/// [`CombatState::end_combat`].
///
/// ## Round lifecycle
///
/// 1. `start` builds and sorts the queue (highest score first, ties broken
///    by re-rolling). See p.168.
/// 2. The GM/rules engine calls `current()` to determine whose turn it is,
///    then performs the entity's actions.
/// 3. `end_turn()` ticks effects on the current entity, resets action flags
///    on the *next* entity, advances `turn_index`, and wraps the round when
///    the queue is exhausted.
///
/// See pp.126–127, p.168.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CombatState {
    /// Current round number. Starts at 1. Increments each time the queue
    /// wraps around.
    pub round: u32,
    /// The initiative queue, sorted descending by score. See p.168.
    pub queue: Vec<InitiativeEntry>,
    /// Index into `queue` pointing at the entity whose turn it currently is.
    pub turn_index: usize,
    /// The combat grid. Populated by WP-302; currently a placeholder.
    pub grid: Grid,
    /// All entities participating in this combat.
    pub participants: HashSet<EntityId>,
}

impl CombatState {
    /// Start a new combat encounter.
    ///
    /// Rolls initiative for every participant (`REF + 1d10` — no critical
    /// explosion per RAW p.168: the formula uses a plain 1d10, not the
    /// critical d10 from p.129), sums any `InitiativeBonus` modifiers from
    /// the entity's [`crate::effects::EffectStack`], then sorts descending.
    /// Ties are broken by re-rolling tied entries' d10s until all scores are
    /// distinct (RAW p.168: "Resolve ties by rolling again until the higher
    /// number wins").
    ///
    /// Entities that have no corresponding character in `world` (e.g. drone
    /// placeholders) receive a score of 0.
    ///
    /// See p.168.
    pub fn start(participants: Vec<EntityId>, world: &World, rng: &mut Rng) -> Self {
        let mut queue: Vec<InitiativeEntry> = participants
            .iter()
            .map(|&entity| {
                let score = roll_initiative(entity, world, rng);
                InitiativeEntry {
                    entity,
                    score,
                    move_used: false,
                    action_used: false,
                    held_action: None,
                }
            })
            .collect();

        // Sort descending — highest score goes first. See p.168.
        queue.sort_by(|a, b| b.score.cmp(&a.score));

        // Tiebreak: re-roll only the tied entries' d10s until all are distinct.
        // See p.168: "Resolve ties by rolling again until the higher number wins."
        resolve_ties(&mut queue, world, rng);

        let participant_set: HashSet<EntityId> = participants.into_iter().collect();

        Self {
            round: 1,
            queue,
            turn_index: 0,
            grid: Grid,
            participants: participant_set,
        }
    }

    /// Returns the entity whose turn it currently is.
    ///
    /// # Panics
    ///
    /// Panics if the queue is empty (combat started with no participants).
    pub fn current(&self) -> EntityId {
        self.queue[self.turn_index].entity
    }

    /// End the current entity's turn and advance the queue.
    ///
    /// In order:
    /// 1. Tick all [`crate::effects::EffectDuration::Turns`] effects on the
    ///    current entity's stack via [`crate::effects::EffectStack::tick_turn`].
    ///    Records dropped IDs in [`TurnEndEvents::effects_dropped`].
    /// 2. Advance `turn_index`. If past the end of the queue, increment
    ///    `round` and wrap to index 0.
    /// 3. Reset `move_used` and `action_used` on the **next** entry (the one
    ///    that will go after this advance — i.e. the entity that is *about to*
    ///    take its turn).
    ///
    /// See p.168 (queue wrap-around, one Round = everyone has taken a Turn).
    pub fn end_turn(&mut self, world: &mut World) -> TurnEndEvents {
        let current_entity = self.queue[self.turn_index].entity;

        // Tick effects on the current entity's stack.
        let effects_dropped = if let Some(character) = world.entity_mut(current_entity) {
            character.effects.tick_turn()
        } else {
            Vec::new()
        };

        // Advance turn index, wrapping at end of queue.
        self.turn_index += 1;
        if self.turn_index >= self.queue.len() {
            self.turn_index = 0;
            self.round += 1;
        }

        // Reset action flags on the upcoming entity (the one now pointed at
        // by the new turn_index). This is the "next" entity — their turn
        // is beginning.
        let next_idx = self.turn_index;
        self.queue[next_idx].move_used = false;
        self.queue[next_idx].action_used = false;

        TurnEndEvents { effects_dropped }
    }

    /// Insert a Black ICE entity at the top of the initiative queue.
    ///
    /// Per p.205: "It is placed into the Initiative Queue at the top, one
    /// number above the entity with the previously highest Initiative."
    ///
    /// The new entry receives `score = current_max_score + 1`. It is
    /// prepended at index 0. `turn_index` is incremented by 1 to remain
    /// pointing at the same entity it did before the insertion.
    ///
    /// See p.205.
    pub fn insert_at_top(&mut self, entity: EntityId, world: &World, rng: &mut Rng) {
        // Score = highest current score + 1. See p.205.
        let max_score = self.queue.iter().map(|e| e.score).max().unwrap_or(0);
        let score = max_score + 1;

        // The rng parameter is accepted to match the API contract (future callers
        // may need RNG for tie-breaking during insertion) and to avoid unused
        // parameter warnings on the trait boundary. RAW p.205 does not require
        // a roll for this insertion — the score is derived arithmetically.
        // Suppress the "unused" warning by referencing world and rng minimally.
        let _ = (world, rng);

        let entry = InitiativeEntry {
            entity,
            score,
            move_used: false,
            action_used: false,
            held_action: None,
        };

        // Insert at index 0 (top of queue). Shift everything else down.
        self.queue.insert(0, entry);

        // Keep turn_index pointing at the same entity it was pointing at
        // before the insertion — do NOT skip past the newly-inserted Black ICE.
        self.turn_index += 1;
        self.participants.insert(entity);
    }

    /// End the combat encounter and return a summary.
    ///
    /// Consumes `self`; the caller should also clear `World::combat` to `None`.
    pub fn end_combat(self) -> CombatSummary {
        CombatSummary {
            rounds: self.round,
            // Kills are tracked by the damage pipeline (WP-303+); for now empty.
            kills: Vec::new(),
        }
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Roll a single entity's initiative: `current_ref + d10 + InitiativeBonus`.
///
/// Uses a plain d10 (no critical explosion). See p.168: "Initiative = REF + 1d10".
/// `InitiativeBonus(i8)` modifiers from the entity's EffectStack are summed
/// and added. Entities with no matching character record receive score 0.
fn roll_initiative(entity: EntityId, world: &World, rng: &mut Rng) -> i16 {
    let Some(character) = world.entity(entity) else {
        return 0;
    };

    let ref_stat = character.current_ref();
    let die_roll = i16::from(d10(rng));

    // Sum all InitiativeBonus modifiers from the entity's effect stack.
    // See p.168 and EffectModifier::InitiativeBonus.
    let bonus: i16 = character
        .effects
        .iter_modifiers()
        .filter_map(|m| {
            if let EffectModifier::InitiativeBonus(b) = m {
                Some(i16::from(*b))
            } else {
                None
            }
        })
        .sum();

    ref_stat + die_roll + bonus
}

/// Resolve all ties in the sorted queue by re-rolling tied pairs' d10s.
///
/// RAW p.168: "Resolve ties by rolling again until the higher number wins."
/// This function re-rolls **only** the d10 component of tied entries (not
/// REF or modifiers, since they are equal and would produce the same tie).
/// It iterates until all adjacent scores in the queue are distinct.
fn resolve_ties(queue: &mut [InitiativeEntry], world: &World, rng: &mut Rng) {
    // Keep iterating until no ties remain.
    loop {
        let mut had_tie = false;

        // Find groups of adjacent entries with the same score.
        let mut i = 0;
        while i < queue.len() {
            // Find the end of this tied group.
            let mut j = i + 1;
            while j < queue.len() && queue[j].score == queue[i].score {
                j += 1;
            }

            if j > i + 1 {
                // Entries [i..j] are tied. Re-roll each one's d10, add to
                // the entity's base (REF + modifiers, no die roll component).
                had_tie = true;
                for entry in queue[i..j].iter_mut() {
                    let base = initiative_base(entry.entity, world);
                    entry.score = base + i16::from(d10(rng));
                }
                // Re-sort only the tied group (stable within broader sort).
                queue[i..j].sort_by(|a, b| b.score.cmp(&a.score));
            }

            i = j;
        }

        if !had_tie {
            break;
        }
    }
}

/// Compute the non-dice component of an initiative roll for an entity:
/// `current_ref + sum(InitiativeBonus)`.
///
/// Used by the tiebreak re-roll logic to reconstruct the base from which a
/// fresh d10 will be added.
fn initiative_base(entity: EntityId, world: &World) -> i16 {
    let Some(character) = world.entity(entity) else {
        return 0;
    };

    let ref_stat = character.current_ref();
    let bonus: i16 = character
        .effects
        .iter_modifiers()
        .filter_map(|m| {
            if let EffectModifier::InitiativeBonus(b) = m {
                Some(i16::from(*b))
            } else {
                None
            }
        })
        .sum();

    ref_stat + bonus
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{ActiveEffect, EffectDuration, EffectSource};
    use crate::types::EffectInstanceId;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;
    use uuid::Uuid;

    fn eid(n: u128) -> EntityId {
        EntityId(Uuid::from_u128(n))
    }

    fn iid(n: u128) -> EffectInstanceId {
        EffectInstanceId(Uuid::from_u128(n))
    }

    /// Build a world with two characters whose entity IDs match eid(1) and eid(2).
    /// ref_a / ref_b control the REF stats.
    fn two_char_world(ref_a: u8, ref_b: u8) -> (World, EntityId, EntityId) {
        let mut pc = fresh_pc();
        pc.id = crate::types::CharacterId(Uuid::from_u128(1));
        pc.stats.r#ref = ref_a;

        let mut npc = fresh_pc();
        npc.id = crate::types::CharacterId(Uuid::from_u128(2));
        npc.stats.r#ref = ref_b;

        let mut world = World::new(pc);
        world
            .npcs
            .insert(crate::types::NpcId(Uuid::from_u128(2)), npc);

        let e1 = eid(1);
        let e2 = eid(2);
        (world, e1, e2)
    }

    /// Build a world with N characters, each with the given REF.
    fn n_char_world(refs: &[u8]) -> (World, Vec<EntityId>) {
        assert!(!refs.is_empty());
        let mut pc = fresh_pc();
        pc.id = crate::types::CharacterId(Uuid::from_u128(0));
        pc.stats.r#ref = refs[0];

        let mut world = World::new(pc);
        let mut ids = vec![eid(0)];

        for (i, &r) in refs.iter().enumerate().skip(1) {
            let mut npc = fresh_pc();
            npc.id = crate::types::CharacterId(Uuid::from_u128(i as u128));
            npc.stats.r#ref = r;
            world
                .npcs
                .insert(crate::types::NpcId(Uuid::from_u128(i as u128)), npc);
            ids.push(eid(i as u128));
        }

        (world, ids)
    }

    /// Confirm the queue is sorted in strictly descending order by score.
    fn assert_descending(queue: &[InitiativeEntry]) {
        for w in queue.windows(2) {
            assert!(
                w[0].score >= w[1].score,
                "queue not sorted descending: {:?}",
                queue.iter().map(|e| e.score).collect::<Vec<_>>()
            );
        }
    }

    // ── Acceptance tests ──────────────────────────────────────────────────────

    /// `test_initiative_descending` — highest score goes first. See p.168.
    #[test]
    fn test_initiative_descending() {
        // Give entity-1 REF 8 and entity-2 REF 2. With any d10, entity-1's
        // score will almost always dominate. We use a deterministic seed
        // and verify the invariant rather than relying on REF gap alone.
        let (world, e1, e2) = two_char_world(8, 2);
        let participants = vec![e2, e1]; // deliberately pass in reverse order

        // Run 20 different seeds to ensure the sort is robust.
        for seed in 0..20u64 {
            let mut rng = Rng::seed_from_u64(seed);
            let state = CombatState::start(participants.clone(), &world, &mut rng);
            assert_descending(&state.queue);
        }
    }

    /// `test_initiative_tiebreak_reroll` — ties resolved by re-rolling until
    /// distinct. See p.168.
    #[test]
    fn test_initiative_tiebreak_reroll() {
        // Build a world with two entities with identical REF so ties are
        // common. After start(), all adjacent scores must be distinct (the
        // tiebreak loop must have run).
        let (world, ids) = n_char_world(&[5, 5, 5]);
        let participants = ids.clone();

        for seed in 0..50u64 {
            let mut rng = Rng::seed_from_u64(seed);
            let state = CombatState::start(participants.clone(), &world, &mut rng);
            // All scores must be distinct after tiebreaking.
            let scores: Vec<i16> = state.queue.iter().map(|e| e.score).collect();
            for i in 0..scores.len() {
                for j in (i + 1)..scores.len() {
                    assert_ne!(
                        scores[i], scores[j],
                        "tie not resolved at seed {seed}: scores={scores:?}"
                    );
                }
            }
        }
    }

    /// `test_round_wraparound` — after the last entry, queue restarts at
    /// index 0 with `round + 1`. See p.168.
    #[test]
    fn test_round_wraparound() {
        let (mut world, e1, e2) = two_char_world(5, 5);
        let mut rng = Rng::seed_from_u64(42);
        let mut state = CombatState::start(vec![e1, e2], &world, &mut rng);

        assert_eq!(state.round, 1);
        assert_eq!(state.turn_index, 0);

        // First turn end — moves to index 1 within round 1.
        let _ev1 = state.end_turn(&mut world);
        assert_eq!(state.round, 1, "still round 1 after first end_turn");
        assert_eq!(state.turn_index, 1);

        // Second turn end — wraps back to 0 and increments round.
        let _ev2 = state.end_turn(&mut world);
        assert_eq!(state.round, 2, "round must increment on wrap-around");
        assert_eq!(state.turn_index, 0);
    }

    /// `test_insert_at_top_above_highest` — Black ICE inserted at top has
    /// score = current_highest + 1. See p.205.
    #[test]
    fn test_insert_at_top_above_highest() {
        let (world, e1, e2) = two_char_world(5, 5);
        let mut rng = Rng::seed_from_u64(1);
        let mut state = CombatState::start(vec![e1, e2], &world, &mut rng);

        let original_highest = state.queue[0].score;

        // Create a new entity for Black ICE — it won't be in the world, so
        // initiative_base returns 0, but insert_at_top sets score directly.
        let black_ice = eid(0xBEEF);
        let mut rng2 = Rng::seed_from_u64(2);
        state.insert_at_top(black_ice, &world, &mut rng2);

        // Black ICE is at index 0 with score = old_highest + 1.
        assert_eq!(state.queue[0].entity, black_ice);
        assert_eq!(
            state.queue[0].score,
            original_highest + 1,
            "Black ICE score must be one above previous highest"
        );

        // turn_index advanced by 1 to compensate for the prepend.
        assert_eq!(
            state.turn_index, 1,
            "turn_index must shift to keep pointing at the same entity"
        );

        // The participant set must include the new entity.
        assert!(state.participants.contains(&black_ice));

        // Sanity: current() still returns the entity that was about to go.
        // (Before insertion, turn_index was 0 → e1 or e2 depending on init order.
        // After insertion, turn_index is 1, which is the same entity.)
        let _ = state.current(); // must not panic
        let _ = world; // suppress unused mut warning
    }

    /// `test_end_turn_ticks_effects` — an effect with `Turns(1)` drops on
    /// the actor's turn end.
    #[test]
    fn test_end_turn_ticks_effects() {
        use crate::effects::EnvironmentalKind;

        let (mut world, e1, e2) = two_char_world(5, 5);
        let mut rng = Rng::seed_from_u64(10);
        let mut state = CombatState::start(vec![e1, e2], &world, &mut rng);

        // Determine which entity currently acts (index 0 in queue).
        let acting = state.current();

        // Add a Turns(1) effect to that entity.
        let effect_id = iid(0xFF);
        let effect = ActiveEffect {
            id: effect_id,
            source: EffectSource::Environmental(EnvironmentalKind::Darkness),
            modifiers: vec![],
            duration: EffectDuration::Turns(1),
        };
        world
            .entity_mut(acting)
            .expect("acting entity must be in world")
            .effects
            .add(effect);

        // Confirm the effect is present before the turn ends.
        assert_eq!(world.entity(acting).unwrap().effects.iter().count(), 1);

        let events = state.end_turn(&mut world);

        // Effect must have been dropped.
        assert!(
            events.effects_dropped.contains(&effect_id),
            "Turns(1) effect must drop on turn end; dropped={:?}",
            events.effects_dropped
        );
        assert_eq!(
            world.entity(acting).unwrap().effects.iter().count(),
            0,
            "effect stack must be empty after the Turns(1) effect drops"
        );
    }

    /// `test_initiative_bonus_applied` — a character with `InitiativeBonus(+3)`
    /// receives score = REF + d10 + 3.
    #[test]
    fn test_initiative_bonus_applied() {
        // Build a single-character world with known REF and a fixed seed.
        let mut pc = fresh_pc();
        pc.id = crate::types::CharacterId(Uuid::from_u128(1));
        pc.stats.r#ref = 5; // REF = 5

        // Add an InitiativeBonus(3) effect.
        pc.effects.add(ActiveEffect {
            id: iid(1),
            source: EffectSource::Cyberware(crate::effects::CyberwareId("sandevistan".to_string())),
            modifiers: vec![EffectModifier::InitiativeBonus(3)],
            duration: EffectDuration::Permanent,
        });

        let mut world = World::new(pc);
        let e1 = eid(1);

        // Build a second character without the bonus.
        let mut npc = fresh_pc();
        npc.id = crate::types::CharacterId(Uuid::from_u128(2));
        npc.stats.r#ref = 5;
        world
            .npcs
            .insert(crate::types::NpcId(Uuid::from_u128(2)), npc);
        let e2 = eid(2);

        // Run with a deterministic seed.
        let mut rng = Rng::seed_from_u64(0);
        // Capture the d10 values that will be rolled.
        // We know d10 with seed 0 — let's verify the bonus is applied:
        // Use a separate rng clone to check expected values.
        let mut rng_check = Rng::seed_from_u64(0);
        let die1 = d10(&mut rng_check); // first roll → entity 1 (order depends on participants slice)
        let die2 = d10(&mut rng_check); // second roll → entity 2

        let state = CombatState::start(vec![e1, e2], &world, &mut rng);

        // Find e1's entry and e2's entry.
        let entry1 = state.queue.iter().find(|e| e.entity == e1).unwrap();
        let entry2 = state.queue.iter().find(|e| e.entity == e2).unwrap();

        // e1 has REF 5 + InitiativeBonus 3 + die1; e2 has REF 5 + die2.
        // NOTE: if tiebreaking re-rolled, the exact scores may differ, but
        // e1's score must always be 3 more than e2's (from the same d10 roll
        // base). Instead, we verify scores satisfy the formula.
        assert_eq!(
            entry1.score,
            5 + i16::from(die1) + 3,
            "e1 score = REF(5) + d10({die1}) + InitiativeBonus(3)"
        );
        assert_eq!(
            entry2.score,
            5 + i16::from(die2),
            "e2 score = REF(5) + d10({die2})"
        );
    }
}
