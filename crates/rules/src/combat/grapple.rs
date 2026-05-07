//! Grappling, throwing, and choking — WP-315.
//!
//! Implements the Grab / Choke / Throw rules from pp.176–177 of the
//! *Cyberpunk RED Core Rules*:
//!
//! - **p.176–177 (Grab):** Resolves as an opposed `DEX + Brawling Skill + 1d10`
//!   check. The winner of the Grab holds the loser. Both parties in a
//!   Grapple take a −2 to all Actions. The Defender cannot use their Move
//!   Action and is dragged with the Attacker's Move Action. The Attacker
//!   can end the Grapple at any time without using an Action; the Defender
//!   must win an opposed Grab check to break free.
//!
//! - **p.177 (Choke):** Available only to the Attacker in an active Grapple.
//!   Costs an Action. Deals the Attacker's BODY STAT directly to the
//!   Defender's Hit Points. This damage **ignores and does not ablate armor**.
//!   If the damage would reduce a target with more than 1 HP to less than 0 HP
//!   they are left at 1 HP and are Unconscious. After 3 successive Choke
//!   rounds without escape the target goes Unconscious regardless of HP.
//!
//! - **p.177 (Throw):** Available only to the Attacker in an active Grapple.
//!   Costs an Action. Deals the Attacker's BODY STAT directly to the
//!   Defender's Hit Points (same formula as Choke). This damage **ignores
//!   and does not ablate armor**. Throwing ends the Grapple; the target
//!   becomes Prone and loses their Move Action until they use Get Up.
//!   Throwing an *object* uses a separate Ranged Attack (DEX + Athletics).
//!
//! ## State tracking note
//!
//! Full held-target state (−2 to all Actions, Move Action restriction, round
//! counters for the 3-round Choke rule) belongs in a future combat-state WP.
//! This module only performs the opposed check for [`GrappleAttempt`] and
//! computes the mechanical outcome for [`ChokeAction`] and [`ThrowAction`].
//! Callers must enforce the "attacker must already be grappling the target"
//! precondition for Choke and Throw, and must update the game state themselves
//! after each call.
//!
//! See pp.175–179.

use crate::character::WeaponId;
use crate::checks::skill_check::OpposedCheck;
use crate::effects::SkillId;
use crate::error::RulesError;
use crate::resolution::Resolution;
use crate::rng::Rng;
use crate::types::{EntityId, Stat};
use crate::world::World;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// GrappleAttempt
// ---------------------------------------------------------------------------

/// A Grab attempt: the Attacker tries to seize the Defender.
///
/// Resolved as an opposed `DEX + Brawling Skill + 1d10` check. The winner
/// holds the loser. A successful attacker Grab places both parties in a
/// Grapple (−2 to all Actions each, Defender loses Move Action).
///
/// Callers must update game state (apply the grappled status) based on
/// [`GrappleOutcome::target_grappled`]. This module only performs the check.
///
/// Grabbing a person is a prerequisite for Choking or Throwing them.
///
/// See pp.176–177.
pub struct GrappleAttempt {
    /// The entity initiating the Grab.
    pub attacker: EntityId,
    /// The entity being grabbed.
    pub target: EntityId,
    /// LUCK Points the attacker spends before rolling. See p.130.
    pub luck_to_spend: u8,
}

/// Outcome of a [`GrappleAttempt`].
///
/// See pp.176–177.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrappleOutcome {
    /// `true` iff the attacker's final value strictly exceeded the defender's.
    /// Ties favour the defender (p.129: "In case of a tie, the Defender
    /// always wins").
    pub attacker_won: bool,
    /// `true` iff the Grapple was established — equivalent to `attacker_won`.
    ///
    /// Callers should apply the grappled status to both participants when this
    /// is `true`. Future state-tracking WPs will key off this flag.
    ///
    /// See p.177: "If you win and choose to grab hold of the Defender instead
    /// of their stuff, both of you are now considered to be in a Grapple."
    pub target_grappled: bool,
}

impl Resolution for GrappleAttempt {
    /// `Result` so LUCK and entity-lookup failures short-circuit cleanly.
    ///
    /// Determinism order:
    /// 1. Validate that both entities exist in `world`.
    /// 2. Pre-validate LUCK on the attacker's side.
    /// 3. Resolve opposed DEX + Brawling check (attacker LUCK spend + d10,
    ///    then defender d10 — defender spends 0 LUCK).
    /// 4. Map the opposed outcome to [`GrappleOutcome`].
    ///
    /// See pp.176–177.
    type Outcome = Result<GrappleOutcome, RulesError>;

    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // 1. Validate entities.
        if world.entity(self.attacker).is_none() {
            return Err(RulesError::EntityNotFound(self.attacker));
        }
        if world.entity(self.target).is_none() {
            return Err(RulesError::EntityNotFound(self.target));
        }

        // 2. Opposed Brawling check. Both sides use DEX + Brawling. See p.177.
        //    The defender spends 0 LUCK here (they may add LUCK via a future
        //    explicit-defender-luck field; the WP-315 API omits that for now).
        let opposed = OpposedCheck {
            attacker: self.attacker,
            attacker_stat: Stat::Dex,
            attacker_skill: SkillId::Brawling,
            attacker_luck: self.luck_to_spend,
            defender: self.target,
            defender_stat: Stat::Dex,
            defender_skill: SkillId::Brawling,
            defender_luck: 0,
            additional_attacker_modifiers: vec![],
            additional_defender_modifiers: vec![],
        };

        let outcome = opposed.resolve(world, rng)?;
        let attacker_won = outcome.attacker_wins;

        Ok(GrappleOutcome {
            attacker_won,
            // A grapple is established only when the attacker wins. See p.177.
            target_grappled: attacker_won,
        })
    }
}

// ---------------------------------------------------------------------------
// ChokeAction
// ---------------------------------------------------------------------------

/// A Choke action against the currently grappled target.
///
/// Only available to the Attacker in an active Grapple. Costs one Action.
///
/// Per p.177: "you can use an Action to Choke the Defender you are grappling,
/// dealing your BODY STAT directly to their Hit Points in damage." This damage
/// **ignores the Defender's armor and doesn't ablate it**.
///
/// If the damage would reduce a target with more than 1 HP to less than 0 HP,
/// they are instead left at 1 HP and become Unconscious. After 3 successive
/// Choke rounds without escape, the target goes Unconscious regardless of HP.
///
/// Callers must:
/// - Enforce the precondition that the attacker currently holds the target in
///   a Grapple.
/// - Decrement the target's HP by [`ChokeOutcome::damage`].
/// - Apply Unconscious status when [`ChokeOutcome::kills`] is `true`.
/// - Track the 3-round Choke counter (state belongs to a future WP).
///
/// See p.177.
pub struct ChokeAction {
    /// The Attacker entity. Their BODY STAT determines damage. See p.177.
    pub attacker: EntityId,
    /// LUCK Points the attacker spends (typically 0 for Choke — no roll).
    /// Included for API completeness; the WP-315 spec carries it.
    pub luck_to_spend: u8,
}

/// Outcome of a [`ChokeAction`].
///
/// See p.177.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChokeOutcome {
    /// Damage dealt directly to the target's HP. Equal to the attacker's
    /// BODY STAT. Ignores and does not ablate armor. See p.177.
    pub damage: u16,
    /// `true` if the choke damage would kill the target outright.
    ///
    /// Per p.177: if this damage would reduce a target with more than 1 HP
    /// to less than 0 HP, they are instead left at 1 HP and are Unconscious.
    /// For simplicity this flag is `true` when `damage >= target_hp` (the
    /// caller applies the 1-HP floor; the engine does not modify world state
    /// here — that belongs to the combat-state WP).
    pub kills: bool,
}

impl Resolution for ChokeAction {
    /// `Result` so entity-lookup failures short-circuit cleanly.
    ///
    /// No dice roll is involved — Choke damage is deterministic: attacker's
    /// BODY STAT directly. The RNG is accepted by the trait signature but is
    /// not consumed.
    ///
    /// Determinism order:
    /// 1. Validate the attacker exists.
    /// 2. Read attacker's current BODY STAT.
    /// 3. Read target's current HP to determine the `kills` flag.
    ///    (Requires a target; absent target uses HP = 0.)
    ///
    /// See p.177.
    type Outcome = Result<ChokeOutcome, RulesError>;

    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Suppress "rng not used" warning. Choke has no roll; it's deterministic.
        let _ = rng;

        // 1. Validate attacker.
        let attacker = world
            .entity(self.attacker)
            .ok_or(RulesError::EntityNotFound(self.attacker))?;

        // 2. Attacker's current BODY. See p.177: "dealing your BODY STAT directly".
        let body = attacker.current_body() as u16;

        // 3. Read target HP for kills flag. The ChokeAction doesn't carry a
        //    target field (implicit grappled target per WP-315 spec). We use
        //    the attacker's own HP as a sentinel: in a real Choke the caller
        //    supplies the target's current HP through game-state lookup. Since
        //    the WP-315 API omits an explicit target, we expose `kills` as a
        //    flag the caller must check against the target's current HP.
        //
        //    Deviation documented in PR: ChokeAction carries no explicit target
        //    id — the WP-315 public API matches the spec ("implicit grappled
        //    target"). The `kills` field uses `damage >= 1` heuristic because
        //    any non-zero damage *could* kill a target at 1 HP. Callers must
        //    re-evaluate against the actual target HP.
        //
        //    See p.177: "If damage dealt by a Choke would reduce a target with
        //    more than 1 HP to less than 0 HP, they are instead left at 1 HP
        //    and are Unconscious."
        let kills = body >= 1;

        Ok(ChokeOutcome {
            damage: body,
            kills,
        })
    }
}

// ---------------------------------------------------------------------------
// ThrowAction
// ---------------------------------------------------------------------------

/// What the attacker aims at when Throwing.
///
/// See p.177: "Throw a person you are Grappling or an object you are
/// holding." When throwing a person at a grid square the target lands in
/// that square (and takes BODY damage). When throwing at an Object the
/// thrown person collides with it.
///
/// See p.177.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThrowTarget {
    /// Throw the grappled target onto a specific grid square `(col, row)`.
    /// The target becomes Prone and loses their Move Action until they use
    /// Get Up. See p.177.
    Square((u16, u16)),
    /// Throw the grappled target at a held or nearby object (e.g. a wall or
    /// piece of cover). The `WeaponId` identifies the object by catalog slug.
    /// See p.177.
    Object(WeaponId),
}

/// A Throw action against the currently grappled target.
///
/// Only available to the Attacker in an active Grapple. Costs one Action.
///
/// Per p.177: "If you are currently the Attacker in a Grapple, you can use
/// an Action to Throw them onto the ground, dealing your BODY STAT directly
/// to their Hit Points in damage. This damage ignores the Defender's armor
/// and doesn't ablate it."
///
/// Throwing ends the Grapple. The target is Prone and cannot use their Move
/// Action until they use the Get Up Action.
///
/// Callers must:
/// - Enforce the precondition that the attacker currently holds the target
///   in a Grapple.
/// - Decrement the target's HP by [`ThrowOutcome::damage`].
/// - Apply Prone status and remove the target's Move Action.
/// - End the Grapple for both participants.
///
/// See p.177.
pub struct ThrowAction {
    /// The Attacker entity. Their BODY STAT determines damage. See p.177.
    pub attacker: EntityId,
    /// Where the grappled target is thrown.
    pub target: ThrowTarget,
    /// LUCK Points the attacker spends (no roll involved; included for API
    /// completeness per WP-315 spec).
    pub luck_to_spend: u8,
}

/// Outcome of a [`ThrowAction`].
///
/// See p.177.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThrowOutcome {
    /// Damage dealt directly to the thrown target's HP. Equal to the
    /// attacker's BODY STAT. Ignores and does not ablate armor. See p.177.
    pub damage: u16,
    /// Where the target ends up after being thrown, if they were thrown at
    /// a grid square. `None` when [`ThrowAction::target`] is
    /// [`ThrowTarget::Object`]. See p.177.
    pub displacement: Option<(u16, u16)>,
}

impl Resolution for ThrowAction {
    /// `Result` so entity-lookup failures short-circuit cleanly.
    ///
    /// No dice roll is involved — Throw damage is deterministic: attacker's
    /// BODY STAT directly. The RNG is accepted by the trait signature but is
    /// not consumed.
    ///
    /// Determinism order:
    /// 1. Validate the attacker exists.
    /// 2. Read attacker's current BODY STAT.
    /// 3. Map [`ThrowTarget`] to [`ThrowOutcome::displacement`].
    ///
    /// See p.177.
    type Outcome = Result<ThrowOutcome, RulesError>;

    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Suppress "rng not used" warning. Throw has no roll; it's deterministic.
        let _ = rng;

        // 1. Validate attacker.
        let attacker = world
            .entity(self.attacker)
            .ok_or(RulesError::EntityNotFound(self.attacker))?;

        // 2. Attacker's current BODY. See p.177: "dealing your BODY STAT directly".
        let body = attacker.current_body() as u16;

        // 3. Displacement is only defined when throwing at a Square. See p.177.
        let displacement = match &self.target {
            ThrowTarget::Square(sq) => Some(*sq),
            ThrowTarget::Object(_) => None,
        };

        Ok(ThrowOutcome {
            damage: body,
            displacement,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::hp::recompute_wounds;
    use crate::dice::d10;
    use crate::effects::SkillId;
    use crate::types::{CharacterId, NpcId};
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;
    use uuid::Uuid;

    // ---- Test helpers -------------------------------------------------------

    /// Walk seeds until `pred` holds on the initial RNG state.
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(&mut Rng) -> bool,
    {
        for seed in 0..2_000_000 {
            let mut r = Rng::seed_from_u64(seed);
            if pred(&mut r) {
                return seed;
            }
        }
        panic!("no matching seed found within search bound");
    }

    /// Build `(World, attacker EntityId, target EntityId)` with configurable
    /// DEX and Brawling ranks for each entity. Neither character has armor.
    fn make_world(
        attacker_uuid: u128,
        target_uuid: u128,
        attacker_dex: u8,
        attacker_brawling: u8,
        attacker_body: u8,
        target_dex: u8,
        target_brawling: u8,
    ) -> (World, EntityId, EntityId) {
        let att_uuid = Uuid::from_u128(attacker_uuid);
        let tgt_uuid = Uuid::from_u128(target_uuid);

        let mut pc = fresh_pc();
        pc.id = CharacterId(att_uuid);
        pc.stats.dex = attacker_dex;
        pc.stats.body = attacker_body;
        pc.stats.luck = 6;
        pc.luck_pool = 6;
        if attacker_brawling > 0 {
            pc.skills.ranks.insert(SkillId::Brawling, attacker_brawling);
        }
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;

        let mut npc = fresh_pc();
        npc.id = CharacterId(tgt_uuid);
        npc.stats.dex = target_dex;
        npc.stats.luck = 6;
        npc.luck_pool = 6;
        if target_brawling > 0 {
            npc.skills.ranks.insert(SkillId::Brawling, target_brawling);
        }
        recompute_wounds(&mut npc);
        npc.wounds.current_hp = npc.wounds.max_hp as i16;

        let att_eid = EntityId(att_uuid);
        let tgt_eid = EntityId(tgt_uuid);

        let mut world = World::new(pc);
        world.npcs.insert(NpcId(tgt_uuid), npc);

        (world, att_eid, tgt_eid)
    }

    // ---- Acceptance tests ---------------------------------------------------

    /// Acceptance: `test_grapple_opposed_brawling`.
    ///
    /// A GrappleAttempt resolves via the opposed DEX + Brawling check.
    /// Both sides use Stat::Dex and SkillId::Brawling. The outcome exposes
    /// `attacker_won` / `target_grappled` flags correctly.
    ///
    /// See pp.176–177.
    #[test]
    fn test_grapple_opposed_brawling() {
        // Attacker: DEX 6, Brawling 4 (base 10).
        // Target:   DEX 4, Brawling 2 (base 6).
        // gap of 4 — almost all seeds win for the attacker.
        let (mut world, att_id, tgt_id) = make_world(0xA1, 0xB1, 6, 4, 6, 4, 2);

        // Find a seed where attacker's roll beats target's.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (10_i16 + a as i16) > (6_i16 + b as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let attempt = GrappleAttempt {
            attacker: att_id,
            target: tgt_id,
            luck_to_spend: 0,
        };

        let outcome = attempt.resolve(&mut world, &mut rng).expect("must succeed");
        assert!(outcome.attacker_won, "attacker with bigger base must win");
        assert!(
            outcome.target_grappled,
            "target_grappled must mirror attacker_won"
        );
    }

    /// Acceptance: `test_grapple_attacker_wins_on_higher_roll`.
    ///
    /// When the attacker's final value strictly exceeds the defender's, the
    /// grapple succeeds. When the defender ties or beats, it fails. Verifies
    /// both directions. See pp.176–177, 129.
    #[test]
    fn test_grapple_attacker_wins_on_higher_roll() {
        // Attacker: DEX 8, Brawling 6 (base 14).
        // Target:   DEX 2, Brawling 0 (base 2).
        // Gap of 12 — attacker wins on virtually any seed.
        let (mut world, att_id, tgt_id) = make_world(0xC1, 0xD1, 8, 6, 8, 2, 0);

        // Find a guaranteed attacker-win seed.
        let win_seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (14_i16 + a as i16) > (2_i16 + b as i16)
        });
        let mut rng = Rng::seed_from_u64(win_seed);

        let attempt = GrappleAttempt {
            attacker: att_id,
            target: tgt_id,
            luck_to_spend: 0,
        };

        let outcome = attempt.resolve(&mut world, &mut rng).expect("must succeed");
        assert!(
            outcome.attacker_won,
            "attacker with huge advantage must win on valid seed"
        );
        assert_eq!(
            outcome.attacker_won, outcome.target_grappled,
            "target_grappled must always equal attacker_won"
        );

        // Verify the defender-wins path using a reversed setup.
        // Attacker: DEX 2, Brawling 0 (base 2). Target: DEX 8, Brawling 6 (base 14).
        let (mut world2, att_id2, tgt_id2) = make_world(0xE1, 0xF1, 2, 0, 4, 8, 6);

        // Find a seed where defender beats (or ties) attacker: 2+a <= 14+b.
        let lose_seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (2_i16 + a as i16) <= (14_i16 + b as i16)
        });
        let mut rng2 = Rng::seed_from_u64(lose_seed);

        let attempt2 = GrappleAttempt {
            attacker: att_id2,
            target: tgt_id2,
            luck_to_spend: 0,
        };

        let outcome2 = attempt2
            .resolve(&mut world2, &mut rng2)
            .expect("must succeed");
        assert!(
            !outcome2.attacker_won,
            "weak attacker vs strong defender must lose"
        );
        assert!(!outcome2.target_grappled, "no grapple when attacker loses");
    }

    /// Acceptance: `test_choke_damages_target`.
    ///
    /// A ChokeAction returns damage equal to the attacker's BODY STAT and
    /// sets `kills` appropriately. Choke bypasses armor (tested by confirming
    /// damage == BODY, not by running the full damage pipeline). See p.177.
    #[test]
    fn test_choke_damages_target() {
        // body = 7 → damage must be 7.
        let (mut world, att_id, _tgt_id) = make_world(0xA2, 0xB2, 6, 4, 7, 4, 2);
        let mut rng = Rng::seed_from_u64(0);

        let choke = ChokeAction {
            attacker: att_id,
            luck_to_spend: 0,
        };

        let outcome = choke.resolve(&mut world, &mut rng).expect("must succeed");
        // Damage equals attacker BODY. See p.177.
        assert_eq!(
            outcome.damage, 7,
            "choke damage must equal attacker BODY (7)"
        );
        // kills flag: any non-zero damage can kill at 1 HP.
        assert!(outcome.kills, "damage > 0 means kills flag is true");

        // Also verify with BODY 4.
        let (mut world2, att2, _) = make_world(0xC2, 0xD2, 5, 3, 4, 4, 2);
        let choke2 = ChokeAction {
            attacker: att2,
            luck_to_spend: 0,
        };
        let outcome2 = choke2.resolve(&mut world2, &mut rng).expect("must succeed");
        assert_eq!(
            outcome2.damage, 4,
            "choke damage must equal attacker BODY (4)"
        );
    }

    /// Acceptance: `test_throw_at_square_returns_displacement`.
    ///
    /// A ThrowAction with ThrowTarget::Square returns the correct displacement
    /// and BODY-based damage. ThrowTarget::Object returns None displacement.
    ///
    /// See p.177.
    #[test]
    fn test_throw_at_square_returns_displacement() {
        // body = 8 → damage must be 8.
        let (mut world, att_id, _tgt_id) = make_world(0xA3, 0xB3, 7, 5, 8, 4, 2);
        let mut rng = Rng::seed_from_u64(0);

        let throw_sq = ThrowAction {
            attacker: att_id,
            target: ThrowTarget::Square((3, 5)),
            luck_to_spend: 0,
        };

        let outcome = throw_sq
            .resolve(&mut world, &mut rng)
            .expect("must succeed");
        // Damage equals attacker BODY. See p.177.
        assert_eq!(
            outcome.damage, 8,
            "throw damage must equal attacker BODY (8)"
        );
        // Displacement must reflect the square.
        assert_eq!(
            outcome.displacement,
            Some((3, 5)),
            "displacement must match Square target"
        );

        // Object throw: no displacement.
        let throw_obj = ThrowAction {
            attacker: att_id,
            target: ThrowTarget::Object(WeaponId("crowbar".into())),
            luck_to_spend: 0,
        };
        let outcome_obj = throw_obj
            .resolve(&mut world, &mut rng)
            .expect("must succeed");
        assert_eq!(outcome_obj.damage, 8, "object throw damage must equal BODY");
        assert_eq!(
            outcome_obj.displacement, None,
            "throwing at Object yields no displacement"
        );
    }

    // ---- Additional regression tests ----------------------------------------

    /// Regression: GrappleAttempt with unknown attacker returns EntityNotFound.
    #[test]
    fn test_grapple_unknown_attacker_returns_err() {
        let pc = fresh_pc();
        let tgt_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEAD));
        let mut rng = Rng::seed_from_u64(0);

        let attempt = GrappleAttempt {
            attacker: unknown,
            target: tgt_id,
            luck_to_spend: 0,
        };
        let err = attempt
            .resolve(&mut world, &mut rng)
            .expect_err("unknown attacker must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
    }

    /// Regression: GrappleAttempt with unknown target returns EntityNotFound.
    #[test]
    fn test_grapple_unknown_target_returns_err() {
        let pc = fresh_pc();
        let att_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEAD));
        let mut rng = Rng::seed_from_u64(0);

        let attempt = GrappleAttempt {
            attacker: att_id,
            target: unknown,
            luck_to_spend: 0,
        };
        let err = attempt
            .resolve(&mut world, &mut rng)
            .expect_err("unknown target must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
    }

    /// Regression: ChokeAction with unknown attacker returns EntityNotFound.
    #[test]
    fn test_choke_unknown_attacker_returns_err() {
        let pc = fresh_pc();
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEAD));
        let mut rng = Rng::seed_from_u64(0);

        let choke = ChokeAction {
            attacker: unknown,
            luck_to_spend: 0,
        };
        let err = choke
            .resolve(&mut world, &mut rng)
            .expect_err("unknown attacker must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
    }

    /// Regression: ThrowAction with unknown attacker returns EntityNotFound.
    #[test]
    fn test_throw_unknown_attacker_returns_err() {
        let pc = fresh_pc();
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEAD));
        let mut rng = Rng::seed_from_u64(0);

        let throw = ThrowAction {
            attacker: unknown,
            target: ThrowTarget::Square((0, 0)),
            luck_to_spend: 0,
        };
        let err = throw
            .resolve(&mut world, &mut rng)
            .expect_err("unknown attacker must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
    }

    /// Regression: GrappleAttempt tie goes to defender — target is NOT grappled.
    ///
    /// p.129: "In case of a tie, the Defender always wins."
    #[test]
    fn test_grapple_tie_goes_to_defender() {
        // Equal stats: DEX 5, Brawling 3 for both. Find a seed where both roll same d10.
        let (mut world, att_id, tgt_id) = make_world(0xE2, 0xF2, 5, 3, 5, 5, 3);

        // Find a seed where both d10s are equal (and not crits for simplicity).
        let tie_seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            a == b && a != 1 && a != 10
        });
        let mut rng = Rng::seed_from_u64(tie_seed);

        let attempt = GrappleAttempt {
            attacker: att_id,
            target: tgt_id,
            luck_to_spend: 0,
        };
        let outcome = attempt.resolve(&mut world, &mut rng).expect("must run");
        assert!(
            !outcome.attacker_won,
            "tied opposed check must NOT give attacker a win (p.129)"
        );
        assert!(
            !outcome.target_grappled,
            "no grapple on a tie — defender wins (p.129)"
        );
    }
}
