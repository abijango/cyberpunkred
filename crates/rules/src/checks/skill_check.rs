//! Skill check resolution — the canonical `STAT + Skill + 1d10 vs DV` and
//! its opposed-check sibling.
//!
//! Implements rulebook pp.128–130:
//! - "Skill Check Resolution" (p.129) — both DV and Opposed forms.
//! - "Difficulty Values" (p.129) — supplied by the caller via [`DV`].
//! - "Critical Successes/Failures" (pp.129–130) — already encoded in
//!   [`crate::dice::d10_with_crits`].
//! - "Modifying the Attempt" (p.130) — situational modifiers reach this
//!   layer two ways: persistent character state (cyberware, wounds, drugs)
//!   is pulled automatically from the actor's
//!   [`crate::effects::EffectStack`]; the GM/Beat layer's bespoke
//!   modifiers come in via [`SkillCheck::additional_modifiers`].
//! - "Using Your LUCK" (p.130) — LUCK is *pre-commit*: this module
//!   validates and decrements [`crate::character::Character::luck_pool`]
//!   *before* rolling the d10 so the seed-determinism contract on
//!   [`crate::resolution::Resolution`] is preserved.
//!
//! ## Outcome shape — deviation from the WP-101 spec
//!
//! The WP-101 stub prototype shows `type Outcome = CheckBreakdown;` for both
//! `Resolution` impls, but the WP description simultaneously requires
//! `Err` on insufficient LUCK and on unknown entity ids. Those two
//! constraints are mutually exclusive, so this module ships the
//! `Result<..., RulesError>` shape — which downstream attack/NET WPs need
//! anyway because they compose this primitive. See the PR description for
//! the formal write-up.

use crate::character::Character;
use crate::dice::d10_with_crits;
use crate::effects::SkillId;
use crate::error::RulesError;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, Stat, DV};
use crate::world::World;
use serde::{Deserialize, Serialize};

/// A bespoke situational modifier from the GM or Beat — e.g. *low light:
/// −1*, *complex task: −2* (rulebook p.130, "Modifying the Attempt").
///
/// Persistent character modifiers (cyberware, wounds, drugs) live on the
/// actor's [`crate::effects::EffectStack`] and are pulled automatically
/// inside [`SkillCheck::resolve`] / [`OpposedCheck::resolve`]; this struct
/// is reserved for one-off, scene-scoped adjustments that the rules
/// engine does not own.
///
/// `label` is plain text for UI/log; `value` is signed because
/// modifications can be either bonuses or penalties.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedModifier {
    /// Human-readable description (e.g. `"low light"`).
    pub label: String,
    /// Signed delta applied to the check (`+`/`-`). Stored as `i8`
    /// because the rulebook never goes higher than ±5 in a single line.
    pub value: i8,
}

/// A standard `STAT + Skill + 1d10 vs DV` check (rulebook p.129).
///
/// Built up by the caller and resolved through
/// [`Resolution::resolve`]. Resolution is a *Result* — see the module
/// docs for the rationale.
pub struct SkillCheck {
    /// Who is making the check. Resolves through [`World::entity_mut`].
    pub actor: EntityId,
    /// Linked stat — typically the natural pairing for the chosen
    /// `skill` (REF for Handgun, COOL for Persuasion, etc., per pp.81–90).
    /// The caller supplies this directly until the skill catalog
    /// (WP-201) lands a canonical skill→stat lookup.
    pub stat: Stat,
    /// The skill being rolled. An untrained skill (rank 0) still rolls,
    /// contributing only its STAT — see "When You Don't Have A Skill"
    /// (p.130).
    pub skill: SkillId,
    /// The Difficulty Value to beat. See p.129 (Difficulty Values).
    pub dv: DV,
    /// LUCK Points to spend on this check. Each point becomes `+1` to
    /// the final value (p.130). Validated *before* the roll and
    /// debited from the actor's pool *before* the d10 is consumed —
    /// this preserves the seed-determinism contract on
    /// [`Resolution`].
    pub luck_to_spend: u8,
    /// GM/Beat-applied situational modifiers. Persistent character
    /// modifiers (cyberware, wounds, drugs) are *not* placed here —
    /// they're pulled from the actor's
    /// [`crate::effects::EffectStack`].
    pub additional_modifiers: Vec<NamedModifier>,
}

/// An *Opposed* check — attacker's roll vs. defender's roll
/// (rulebook p.129). Ties favour the defender.
///
/// Both sides may spend LUCK and may have GM/Beat modifiers attached;
/// the persistent modifiers on each character are pulled from their
/// individual effect stacks during resolution.
///
/// Resolution order is fixed for determinism:
/// 1. Validate attacker LUCK.
/// 2. Spend attacker LUCK.
/// 3. Validate defender LUCK (only if attacker step succeeded).
/// 4. Spend defender LUCK.
/// 5. Roll the attacker's d10.
/// 6. Roll the defender's d10.
///
/// If any step fails, no later step runs and no further state is
/// mutated — including the dice. (Earlier successful LUCK spends are
/// **not** rolled back; this matches the per-action-validation pattern
/// used elsewhere in the engine and avoids cross-character transactional
/// machinery for a check that is, after all, a roll commit.)
pub struct OpposedCheck {
    /// Attacker entity — resolved via [`World::entity_mut`].
    pub attacker: EntityId,
    /// Attacker's linked stat for their skill.
    pub attacker_stat: Stat,
    /// Attacker's skill.
    pub attacker_skill: SkillId,
    /// LUCK the attacker is committing to this check.
    pub attacker_luck: u8,
    /// Defender entity — resolved via [`World::entity_mut`].
    pub defender: EntityId,
    /// Defender's linked stat for their skill.
    pub defender_stat: Stat,
    /// Defender's skill.
    pub defender_skill: SkillId,
    /// LUCK the defender is committing to this check.
    pub defender_luck: u8,
    /// GM/Beat-applied modifiers on the attacker's side.
    pub additional_attacker_modifiers: Vec<NamedModifier>,
    /// GM/Beat-applied modifiers on the defender's side.
    pub additional_defender_modifiers: Vec<NamedModifier>,
}

/// Outcome of an [`OpposedCheck`].
///
/// Both sides' breakdowns are returned for reporting / replay. `dv`
/// inside each [`CheckBreakdown`] is *the opponent's final value*
/// (saturated to `u8::MAX` if pathologically large) — that's the
/// number each side actually had to beat, per p.129. `attacker_wins`
/// is computed from the raw `final_value`s before saturation, so
/// pathological-magnitude rolls still resolve correctly even though
/// the breakdown's `dv` field is clamped.
pub struct OpposedOutcome {
    /// The attacker's full breakdown. `dv` is the defender's final
    /// value, saturated to `u8::MAX` if larger.
    pub attacker_breakdown: CheckBreakdown,
    /// The defender's full breakdown. `dv` is the attacker's final
    /// value, saturated to `u8::MAX` if larger.
    pub defender_breakdown: CheckBreakdown,
    /// `true` iff the attacker's final value strictly exceeds the
    /// defender's. Ties favour the defender (p.129: "In case of a
    /// tie, the Defender always wins").
    pub attacker_wins: bool,
}

/// Internal: parameter pack for [`roll_one_check`].
///
/// Bundling the per-side inputs into a struct keeps the helper's call
/// sites readable and dodges clippy's `too_many_arguments` lint, which
/// would otherwise trip on the seven-plus parameters this conceptually
/// needs.
struct CheckParams<'a> {
    actor_id: EntityId,
    stat: Stat,
    skill: &'a SkillId,
    luck_to_spend: u8,
    additional: &'a [NamedModifier],
    dv: DV,
}

/// Internal: validate + spend LUCK, look up the actor, and roll a d10
/// in the order required by the determinism contract. Produces a fully
/// populated [`CheckBreakdown`].
fn roll_one_check(
    world: &mut World,
    rng: &mut Rng,
    p: CheckParams<'_>,
) -> Result<CheckBreakdown, RulesError> {
    // 1. Resolve actor (mutable — we need to spend LUCK).
    let actor: &mut Character = world
        .entity_mut(p.actor_id)
        .ok_or(RulesError::EntityNotFound(p.actor_id))?;

    // 2. Validate + spend LUCK *before* any dice roll. spend_luck()
    //    itself returns InsufficientLuck on failure (WP-103) and is a
    //    no-op on the failing path, so the actor's pool is preserved.
    actor.spend_luck(p.luck_to_spend)?;

    // 3. Consume any pending Complementary Skill bonus for this skill
    //    (rulebook p.130). The bonus is one-shot: taking it removes
    //    it from the actor. Done *after* LUCK validation so a failed
    //    LUCK spend never consumes the bonus.
    let complementary = actor.take_complementary_bonus(p.skill);

    // 4. Snapshot the values we need from the actor. current_stat /
    //    current_skill / all_actions_penalty are `&self` queries; the
    //    `&mut Character` we hold can call them without trouble.
    let stat_value = actor.current_stat(p.stat);
    let skill_value = actor.current_skill(p.skill);
    let aap = i16::from(actor.all_actions_penalty());

    // 5. Sum GM/Beat modifiers. These are signed; rulebook ranges
    //    typically fit in i8, but we widen to i16 to match
    //    CheckBreakdown::new's input shape and avoid overflow in
    //    pathological multi-modifier stacks.
    let extra: i16 = p.additional.iter().map(|m| i16::from(m.value)).sum();
    let complementary_bonus: i16 = if complementary.is_some() { 1 } else { 0 };
    let modifier_total = aap + extra + complementary_bonus;

    // 6. Roll the d10 (with crit handling). After this point the RNG
    //    has advanced; an early-return error path would have advanced
    //    nothing.
    let d10 = d10_with_crits(rng);

    // 7. Build the breakdown — final_value/margin/success are derived.
    Ok(CheckBreakdown::new(
        stat_value,
        skill_value,
        modifier_total,
        p.luck_to_spend,
        d10,
        p.dv,
    ))
}

impl Resolution for SkillCheck {
    /// `Result` so LUCK and entity-lookup failures can short-circuit
    /// without rolling — see the module-level docs for the deviation
    /// note.
    type Outcome = Result<CheckBreakdown, RulesError>;

    /// Resolve this skill check against `world`, drawing all dice from
    /// `rng`. See p.129 for the formula.
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        roll_one_check(
            world,
            rng,
            CheckParams {
                actor_id: self.actor,
                stat: self.stat,
                skill: &self.skill,
                luck_to_spend: self.luck_to_spend,
                additional: &self.additional_modifiers,
                dv: self.dv,
            },
        )
    }
}

impl Resolution for OpposedCheck {
    /// `Result` for the same reason as [`SkillCheck`] — LUCK and
    /// entity-lookup failures must short-circuit.
    type Outcome = Result<OpposedOutcome, RulesError>;

    /// Resolve the opposed check. Order: attacker LUCK validation /
    /// spend, defender LUCK validation / spend, attacker d10, defender
    /// d10. Ties favour the defender (p.129).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Validate both actors exist *before* we mutate anything. This
        // prevents the case where the attacker spends LUCK only for us
        // to discover the defender doesn't exist.
        if world.entity(self.attacker).is_none() {
            return Err(RulesError::EntityNotFound(self.attacker));
        }
        if world.entity(self.defender).is_none() {
            return Err(RulesError::EntityNotFound(self.defender));
        }

        // Pre-validate LUCK on both sides so a subsequent
        // InsufficientLuck on the defender doesn't leave the attacker
        // having spent points for a roll that never happens. Reads are
        // cheap.
        let att_have = world
            .entity(self.attacker)
            .expect("checked above")
            .luck_remaining();
        if self.attacker_luck > att_have {
            return Err(RulesError::InsufficientLuck {
                requested: self.attacker_luck,
                available: att_have,
            });
        }
        let def_have = world
            .entity(self.defender)
            .expect("checked above")
            .luck_remaining();
        if self.defender_luck > def_have {
            return Err(RulesError::InsufficientLuck {
                requested: self.defender_luck,
                available: def_have,
            });
        }

        // Compute the attacker's breakdown without yet knowing the DV
        // (we'll patch that in once the defender has rolled). We use
        // DV(0) as a sentinel; the *real* dv is filled in below.
        let mut attacker_bd = roll_one_check(
            world,
            rng,
            CheckParams {
                actor_id: self.attacker,
                stat: self.attacker_stat,
                skill: &self.attacker_skill,
                luck_to_spend: self.attacker_luck,
                additional: &self.additional_attacker_modifiers,
                dv: DV(0),
            },
        )?;
        let mut defender_bd = roll_one_check(
            world,
            rng,
            CheckParams {
                actor_id: self.defender,
                stat: self.defender_stat,
                skill: &self.defender_skill,
                luck_to_spend: self.defender_luck,
                additional: &self.additional_defender_modifiers,
                dv: DV(0),
            },
        )?;

        // Patch each side's DV to the *opponent's final value*, and
        // recompute the derived fields so success/margin reflect the
        // opposed semantics.
        let att_final = attacker_bd.final_value;
        let def_final = defender_bd.final_value;

        attacker_bd.dv = DV(saturate_to_u8_dv(def_final));
        attacker_bd.margin = att_final - i16::from(attacker_bd.dv.0);
        // p.129: ties favour the defender. The attacker only "succeeds"
        // when their final strictly exceeds the defender's.
        attacker_bd.success = att_final > def_final;

        defender_bd.dv = DV(saturate_to_u8_dv(att_final));
        defender_bd.margin = def_final - i16::from(defender_bd.dv.0);
        // The defender wins ties (p.129) — `success == !attacker_wins`.
        defender_bd.success = def_final >= att_final;

        Ok(OpposedOutcome {
            attacker_breakdown: attacker_bd,
            defender_breakdown: defender_bd,
            attacker_wins: att_final > def_final,
        })
    }
}

/// Saturate an `i16` final-value into the `u8` shape that
/// [`DV`] requires. Negative values clamp to `0`, values above
/// `u8::MAX` clamp to `u8::MAX`. The accompanying `attacker_wins`
/// computation in [`OpposedCheck::resolve`] uses the un-saturated
/// `i16`, so this clamp is purely cosmetic (it only affects the
/// `dv` and `margin` fields *inside* the breakdown).
fn saturate_to_u8_dv(v: i16) -> u8 {
    v.clamp(0, i16::from(u8::MAX)) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Character;
    use crate::dice::d10;
    use crate::effects::{ActiveEffect, EffectDuration, EffectModifier, EffectSource, SkillId};
    use crate::types::{CharacterId, EffectInstanceId, NpcId};
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;
    use uuid::Uuid;

    /// Walk seeds in order until we find one whose initial RNG state
    /// satisfies `pred`. Mirrors the helper in `dice::tests`.
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

    /// Build a PC with a deterministic id (so tests can construct
    /// matching `EntityId`s) and a known LUCK pool.
    fn make_pc(stat_int: u8, skill_rank: u8, luck: u8) -> (Character, EntityId) {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0xC0));
        pc.stats.int = stat_int;
        pc.stats.luck = luck;
        pc.luck_pool = luck;
        if skill_rank > 0 {
            pc.skills.ranks.insert(SkillId::Education, skill_rank);
        }
        let eid = EntityId(pc.id.0);
        (pc, eid)
    }

    /// Build an NPC, registered into `World::npcs`. Returns the
    /// `EntityId` resolving to it.
    fn add_npc(world: &mut World, uuid: u128, stat_int: u8, skill_rank: u8, luck: u8) -> EntityId {
        let mut npc = fresh_pc();
        let id = Uuid::from_u128(uuid);
        npc.id = CharacterId(id);
        npc.stats.int = stat_int;
        npc.stats.luck = luck;
        npc.luck_pool = luck;
        if skill_rank > 0 {
            npc.skills.ranks.insert(SkillId::Education, skill_rank);
        }
        world.npcs.insert(NpcId(id), npc);
        EntityId(id)
    }

    #[test]
    fn test_simple_check_against_dv9() {
        // STAT 5, Skill 4, no modifiers, d10 = 5 → final 14, success vs DV 9.
        let (pc, actor) = make_pc(5, 4, 6);
        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::SIMPLE,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must succeed");
        assert_eq!(outcome.stat_value, 5);
        assert_eq!(outcome.skill_value, 4);
        assert_eq!(outcome.modifier_total, 0);
        assert_eq!(outcome.luck_spent, 0);
        assert_eq!(outcome.d10.net, 5);
        assert_eq!(outcome.final_value, 14);
        assert_eq!(outcome.dv, DV::SIMPLE);
        assert!(outcome.success);
    }

    #[test]
    fn test_critical_success_propagates() {
        // d10 = 10 → CritD10 with a follow-up; final reflects both rolls.
        let (pc, actor) = make_pc(5, 4, 6);
        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        // Pre-compute what the follow-up will be using a parallel RNG.
        let mut probe = Rng::seed_from_u64(seed);
        let _ = d10(&mut probe);
        let follow_up = d10(&mut probe);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must succeed");
        assert_eq!(outcome.d10.base, 10);
        assert_eq!(outcome.d10.follow_up, Some(follow_up));
        assert_eq!(outcome.d10.net, 10 + i16::from(follow_up));
        // final = 5 + 4 + 0 + 0 + (10 + follow_up)
        assert_eq!(
            outcome.final_value,
            5 + 4 + 10 + i16::from(follow_up),
            "final must include both the natural 10 and the follow-up roll"
        );
        assert!(outcome.success, "huge crit must beat DV13");
    }

    #[test]
    fn test_critical_failure_can_negate_high_skill() {
        // STAT 8, Skill 8, DV 15. Forced d10=1 with follow-up=10
        // → net = 1 - 10 = -9. final = 8 + 8 + 0 + 0 - 9 = 7. Fails.
        let (mut pc, actor) = make_pc(8, 8, 6);
        // The helper put the skill rank against `education`; we want it
        // at 8 specifically, so re-set just to be explicit.
        pc.skills.ranks.insert(SkillId::Education, 8);
        let mut world = World::new(pc);

        let seed = find_seed_where(|r| d10(r) == 1 && d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV(15),
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        assert_eq!(outcome.d10.base, 1);
        assert_eq!(outcome.d10.follow_up, Some(10));
        assert_eq!(outcome.d10.net, -9);
        assert_eq!(outcome.final_value, 8 + 8 - 9);
        assert_eq!(outcome.final_value, 7);
        assert!(!outcome.success, "7 < DV 15 — must fail");
    }

    #[test]
    fn test_opposed_tie_defender_wins() {
        // Construct attacker and defender with identical stats and
        // skills, then arrange a seed that produces equal d10 results
        // for both rolls. Both finals tie → attacker_wins == false.
        let (pc, attacker) = make_pc(5, 3, 6);
        let mut world = World::new(pc);
        let defender = add_npc(&mut world, 0xD0, 5, 3, 6);

        // Two consecutive d10s equal — the opposed roll consumes
        // attacker's d10 first, then defender's.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            a == b && a != 1 && a != 10 // avoid crit branches; finals tie cleanly
        });
        let mut rng = Rng::seed_from_u64(seed);

        let check = OpposedCheck {
            attacker,
            attacker_stat: Stat::Int,
            attacker_skill: SkillId::Education,
            attacker_luck: 0,
            defender,
            defender_stat: Stat::Int,
            defender_skill: SkillId::Education,
            defender_luck: 0,
            additional_attacker_modifiers: vec![],
            additional_defender_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        assert_eq!(
            outcome.attacker_breakdown.final_value, outcome.defender_breakdown.final_value,
            "the contrived seed produces a real tie"
        );
        assert!(
            !outcome.attacker_wins,
            "tie favours the defender (rulebook p.129)"
        );
        // The defender's breakdown should report success == true on a tie.
        assert!(outcome.defender_breakdown.success);
        assert!(!outcome.attacker_breakdown.success);
    }

    #[test]
    fn test_modifier_total_applied() {
        // A -2 "complex task" GM modifier should reduce the final by
        // exactly 2 vs. the no-modifier baseline.
        let (pc, actor) = make_pc(5, 4, 6);
        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![NamedModifier {
                label: "complex task".into(),
                value: -2,
            }],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        // baseline (5 + 4 + 5) = 14, minus 2 = 12.
        assert_eq!(outcome.modifier_total, -2);
        assert_eq!(outcome.final_value, 12);
        assert!(!outcome.success, "12 < DV 13");
    }

    #[test]
    fn test_no_skill_uses_stat_only() {
        // Untrained skill: rank = 0 (p.130, "When You Don't Have A Skill").
        // With STAT 5 and d10 = 5, final must be 5 + 0 + 5 = 10.
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0xCAFE));
        pc.stats.int = 5;
        // Deliberately leave skills empty.
        let actor = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            // Use Tracking — a real but un-trained skill on this PC.
            // The "untrained skill" path doesn't depend on the variant
            // (rank lookup misses → 0); pick any unconfigured one.
            skill: SkillId::Tracking,
            dv: DV::SIMPLE,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        assert_eq!(outcome.skill_value, 0, "untrained skill is rank 0");
        assert_eq!(outcome.stat_value, 5);
        assert_eq!(outcome.final_value, 10);
        assert!(outcome.success, "10 >= DV 9");
    }

    #[test]
    fn test_luck_spent_increases_check() {
        // Spending 3 LUCK adds +3 to the final and decrements
        // luck_remaining() by 3.
        let (pc, actor) = make_pc(5, 4, 6);
        let mut world = World::new(pc);
        assert_eq!(world.entity(actor).unwrap().luck_remaining(), 6);

        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 3,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        // 5 + 4 + 0 + 3 + 5 = 17.
        assert_eq!(outcome.luck_spent, 3);
        assert_eq!(outcome.final_value, 17);
        assert_eq!(
            world.entity(actor).unwrap().luck_remaining(),
            3,
            "LUCK pool must be debited by exactly 3"
        );
    }

    #[test]
    fn test_insufficient_luck_returns_err() {
        // luck_to_spend > luck_remaining must short-circuit before any
        // dice roll. The LUCK pool stays untouched and the RNG must
        // not advance.
        let (pc, actor) = make_pc(5, 4, 2);
        let mut world = World::new(pc);
        let mut rng = Rng::seed_from_u64(42);
        let mut probe = Rng::seed_from_u64(42);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 5,
            additional_modifiers: vec![],
        };
        let err = check
            .resolve(&mut world, &mut rng)
            .expect_err("insufficient LUCK must Err");
        assert_eq!(
            err,
            RulesError::InsufficientLuck {
                requested: 5,
                available: 2,
            }
        );
        assert_eq!(
            world.entity(actor).unwrap().luck_remaining(),
            2,
            "pool must be unchanged on the error path"
        );
        // The RNG's next draw must equal what it would have produced
        // had `resolve` not been called — i.e. resolve must not have
        // advanced the stream.
        assert_eq!(d10(&mut rng), d10(&mut probe));
    }

    #[test]
    fn test_unknown_actor_returns_err() {
        // EntityId not in world → EntityNotFound, no LUCK / RNG churn.
        let (pc, _actor) = make_pc(5, 4, 6);
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEADC0DE));
        let mut rng = Rng::seed_from_u64(7);
        let mut probe = Rng::seed_from_u64(7);

        let check = SkillCheck {
            actor: unknown,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let err = check
            .resolve(&mut world, &mut rng)
            .expect_err("unknown actor must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
        assert_eq!(d10(&mut rng), d10(&mut probe));
    }

    /// Bonus regression: persistent EffectStack penalties (e.g. Wound
    /// State -2 to all actions, p.186) flow through automatically. The
    /// WP body explicitly calls this out as part of the resolve
    /// contract — guarded here so a refactor that drops
    /// `all_actions_penalty()` would break the build.
    #[test]
    fn test_persistent_all_actions_penalty_applied() {
        let (mut pc, actor) = make_pc(5, 4, 6);
        pc.effects.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(0xE1)),
            source: EffectSource::WoundState(crate::effects::WoundState::Seriously),
            modifiers: vec![EffectModifier::AllActionsPenalty(-2)],
            duration: EffectDuration::Permanent,
        });
        let mut world = World::new(pc);
        let seed = find_seed_where(|r| d10(r) == 5);
        let mut rng = Rng::seed_from_u64(seed);

        let check = SkillCheck {
            actor,
            stat: Stat::Int,
            skill: SkillId::Education,
            dv: DV::EVERYDAY,
            luck_to_spend: 0,
            additional_modifiers: vec![],
        };
        let outcome = check.resolve(&mut world, &mut rng).expect("must run");
        // 5 + 4 + (-2) + 0 + 5 = 12.
        assert_eq!(outcome.modifier_total, -2);
        assert_eq!(outcome.final_value, 12);
    }
}
