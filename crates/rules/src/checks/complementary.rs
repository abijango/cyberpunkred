//! Complementary Skill bonuses (rulebook p.130, "Complementary Skills").
//!
//! When the GM rules that one Skill's success directly supports another
//! Skill's use — e.g. an Education check supporting a Library Search,
//! or a Persuasion check supporting a Trading negotiation — the
//! supporting check, *if successful*, confers a single `+1` to the
//! next use of the related Skill.
//!
//! Three rules from the page govern this implementation:
//!
//! 1. **One-shot.** "This +1 bonus only affects a subsequent attempt
//!    once" — the bonus is consumed by the next applicable check.
//! 2. **Does not stack.** "Complementary Skill bonuses do not stack."
//!    Two outstanding bonuses on the same target Skill still grant
//!    only `+1`, never `+2`.
//! 3. **GM-gated.** Whether the two Skills are "related" enough is a
//!    GM call (p.130). This module accepts whatever the GM/Beat layer
//!    decides; it doesn't try to encode a relatedness graph.
//!
//! Storage lives on [`Character`] (rather than on
//! [`crate::effects::EffectStack`]) because pending bonuses are
//! skill-specific and one-shot; modeling them as durative effects
//! would force the consumption logic into every roll site instead
//! of letting [`SkillCheck::resolve`] pull them through a single
//! call.

use crate::character::Character;
use crate::effects::SkillId;
use serde::{Deserialize, Serialize};

/// A single-use `+1` marker placed on the next use of a target Skill.
///
/// Created by the rules/GM layer when a successful related Skill check
/// has occurred and consumed by the next [`crate::checks::SkillCheck`]
/// for `target_skill`. See p.130 ("Complementary Skills").
///
/// `granted_by` records the supporting Skill so the UI / replay log
/// can narrate *why* the bonus exists. `consumed` is `false` on
/// fresh bonuses; the value returned by
/// [`Character::take_complementary_bonus`] has `consumed = true` so
/// downstream logging can distinguish a bonus that was actually
/// applied from one that was merely inspected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComplementaryBonus {
    /// The Skill that this `+1` will boost on its next use.
    pub target_skill: SkillId,
    /// The Skill whose successful check granted this bonus — recorded
    /// for narration / replay; not load-bearing on resolution.
    pub granted_by: SkillId,
    /// `false` while the bonus is pending; set to `true` on the value
    /// returned by [`Character::take_complementary_bonus`] so the
    /// caller's log can record an applied bonus distinctly.
    pub consumed: bool,
}

impl Character {
    /// Add a pending complementary bonus.
    ///
    /// Per p.130 ("Complementary Skill bonuses do not stack"), if a
    /// non-consumed bonus already exists for `bonus.target_skill`, the
    /// new bonus is **ignored** — preserving the order of the existing
    /// pending bonus. The "ignore-on-collision" choice (rather than
    /// "replace") keeps the source attribution (`granted_by`) of the
    /// first qualifying check, which is the more useful log entry.
    pub fn add_complementary_bonus(&mut self, bonus: ComplementaryBonus) {
        let already_present = self
            .complementary_bonuses
            .iter()
            .any(|b| b.target_skill == bonus.target_skill);
        if !already_present {
            self.complementary_bonuses.push(bonus);
        }
    }

    /// Take and consume any pending complementary bonus for `skill`.
    ///
    /// Returns the bonus (with `consumed = true`) on a hit and `None`
    /// otherwise. The matching bonus is removed from the vec entirely
    /// — we never need post-hoc history of consumed bonuses, and
    /// removing keeps the "does not stack" invariant trivially
    /// upheld.
    ///
    /// See p.130 ("This +1 bonus only affects a subsequent attempt
    /// once").
    pub fn take_complementary_bonus(&mut self, skill: &SkillId) -> Option<ComplementaryBonus> {
        let pos = self
            .complementary_bonuses
            .iter()
            .position(|b| !b.consumed && &b.target_skill == skill)?;
        let mut bonus = self.complementary_bonuses.remove(pos);
        bonus.consumed = true;
        Some(bonus)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checks::SkillCheck;
    use crate::dice::d10;
    use crate::resolution::Resolution;
    use crate::rng::Rng;
    use crate::types::{CharacterId, EntityId, Stat, DV};
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;
    use uuid::Uuid;

    /// Walk seeds in order until we find one whose initial RNG state
    /// satisfies `pred`. Used for reproducible d10 rolls in tests.
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

    /// Fresh PC with deterministic id and a known skill rank for the
    /// roll-through tests.
    fn make_pc() -> (Character, EntityId) {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0xC0));
        pc.stats.int = 5;
        pc.skills.ranks.insert(SkillId("education".into()), 4);
        let eid = EntityId(pc.id.0);
        (pc, eid)
    }

    #[test]
    fn test_complementary_grants_plus_one() {
        // Baseline: STAT 5 + Skill 4 + d10 = 5 → final_value 14, modifier_total 0.
        // With a pending bonus, modifier_total must be exactly +1 higher.
        let (mut pc, actor) = make_pc();
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("education".into()),
            granted_by: SkillId("library_search".into()),
            consumed: false,
        });
        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId("education".into()),
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must succeed");
        assert_eq!(
            outcome.modifier_total, 1,
            "pending complementary bonus must add +1 to modifier_total"
        );
        // Sanity: 5 + 4 + 1 + 0 + 5 = 15.
        assert_eq!(outcome.final_value, 15);
        // Bonus should have been removed from the character.
        assert_eq!(world.entity(actor).unwrap().complementary_bonuses.len(), 0);
    }

    #[test]
    fn test_complementary_consumed_after_one_use() {
        // Run two checks back-to-back. The second one must NOT receive +1.
        let (mut pc, actor) = make_pc();
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("education".into()),
            granted_by: SkillId("library_search".into()),
            consumed: false,
        });
        let mut world = World::new(pc);
        // We need two consecutive d10s of 5 — one per check.
        let seed = find_seed_where(|r| d10(r) == 5 && d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId("education".into()),
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };

        let first = check
            .resolve(&mut world, &mut rng)
            .expect("first check must run");
        assert_eq!(first.modifier_total, 1, "first use receives the +1");

        let second = check
            .resolve(&mut world, &mut rng)
            .expect("second check must run");
        assert_eq!(
            second.modifier_total, 0,
            "second use must not receive the +1 — bonus was consumed"
        );
    }

    #[test]
    fn test_complementary_does_not_stack() {
        // Adding two bonuses for the same target skill must collapse to
        // exactly one entry (per p.130). On the next check, only +1 lands.
        let (mut pc, actor) = make_pc();
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("education".into()),
            granted_by: SkillId("library_search".into()),
            consumed: false,
        });
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("education".into()),
            granted_by: SkillId("local_expert".into()),
            consumed: false,
        });
        assert_eq!(
            pc.complementary_bonuses.len(),
            1,
            "second add must be a no-op — bonuses do not stack"
        );

        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId("education".into()),
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must succeed");
        assert_eq!(
            outcome.modifier_total, 1,
            "two pending bonuses still confer only +1 (p.130)"
        );
    }

    #[test]
    fn test_take_returns_none_when_no_bonus() {
        let mut pc = fresh_pc();
        let result = pc.take_complementary_bonus(&SkillId("education".into()));
        assert!(result.is_none(), "no pending bonus → None");

        // Adding a bonus for a different skill must not satisfy the take.
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("brawling".into()),
            granted_by: SkillId("athletics".into()),
            consumed: false,
        });
        let result = pc.take_complementary_bonus(&SkillId("education".into()));
        assert!(
            result.is_none(),
            "bonus on a different target skill must not satisfy the take"
        );
        // The unrelated bonus must still be present.
        assert_eq!(pc.complementary_bonuses.len(), 1);
    }

    #[test]
    fn test_take_does_not_consume_other_skill_bonus() {
        // A pending bonus on Concentration must not be consumed by a
        // skill check on Handgun.
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0xC0));
        pc.stats.r#ref = 6;
        pc.skills.ranks.insert(SkillId("handgun".into()), 4);
        pc.add_complementary_bonus(ComplementaryBonus {
            target_skill: SkillId("concentration".into()),
            granted_by: SkillId("meditation".into()),
            consumed: false,
        });
        let actor = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Ref,
            skill: SkillId("handgun".into()),
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must succeed");
        assert_eq!(
            outcome.modifier_total, 0,
            "Handgun check must not pull the Concentration bonus"
        );

        // The unrelated Concentration bonus must still be pending.
        let bonuses = &world.entity(actor).unwrap().complementary_bonuses;
        assert_eq!(bonuses.len(), 1);
        assert_eq!(bonuses[0].target_skill, SkillId("concentration".into()));
        assert!(!bonuses[0].consumed);
    }
}
