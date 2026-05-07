//! Melee attack resolution — WP-307.
//!
//! Implements the melee combat rules from pp.175–179 of the *Cyberpunk RED
//! Core Rules*:
//!
//! - **p.175:** "Melee Combat is resolved: Attacker's DEX + Relevant Melee
//!   Attack Skill + 1d10 vs. Defender's DEX + Evasion Skill + 1d10."
//! - **p.175:** Defender may use Evasion, Brawling, or Martial Arts — picks
//!   highest effective rank; ties are broken by Evasion > Brawling > MA.
//! - **p.176:** BODY damage bonus die count (WP-307 spec thresholds):
//!   BODY 1–4 → +0, BODY 5–7 → +1, BODY 8–10 → +2, BODY 11+ → +3.
//! - **p.176:** Brawling damage scales with BODY: ≤4 → 1d6, 5–6 → 2d6,
//!   7–10 → 3d6, 11+ → 4d6. Brawling does NOT halve defender armor.
//! - **p.176:** Melee weapons ignore half the defender's armor (round up).
//! - **p.178:** Martial Arts attacks ignore half the defender's armor and
//!   deal damage based on BODY (same table as Brawling).
//! - **p.179:** Martial Arts Special Moves — Strike, Hold, Sweep, Throw,
//!   Disarm, Choke, Resolution.
//!
//! ## Half-armor note (pp.175–176, 178)
//!
//! Per RAW:
//! - **Melee weapons** (`MeleeWeaponChoice::Weapon`): ignore half armor (p.176).
//! - **Brawling** (`MeleeWeaponChoice::Brawling`): do NOT ignore half armor (p.176).
//! - **Martial Arts** (`MeleeWeaponChoice::MartialArts`): ignore half armor (p.178).
//!
//! The `half_armor` flag on `MeleeAttackOutcome` signals whether armor halving
//! applies. Since the damage pipeline (`apply_damage`, WP-303) does not have a
//! half-armor input, callers must halve the defender's SP externally before
//! or after calling `resolve`. This is flagged in the PR.
//!
//! ## Weapon dice (deviation from spec)
//!
//! The WP-307 API does not carry a weapon catalog reference. For
//! `MeleeWeaponChoice::Weapon(_)`, this module rolls 1d6 as the base die and
//! adds body bonus dice per the WP-307 thresholds. Callers modelling heavier
//! weapons should pass the weapon's extra dice contribution via
//! `additional_modifiers`. This is flagged in the PR.
//!
//! ## Opposed check (p.175)
//!
//! Melee resolution delegates to `OpposedCheck` (WP-101) which implements the
//! full determinism contract: attacker LUCK validation + spend, defender LUCK
//! validation + spend, attacker d10, defender d10 — in that fixed order.
//!
//! See pp.175–179.

use crate::catalog::critical_injuries::CritTable;
use crate::catalog::skills::MartialArtsForm;
use crate::catalog::{critical_injuries::CriticalInjury, Catalog};
use crate::character::WeaponId;
use crate::checks::skill_check::{NamedModifier, OpposedCheck};
use crate::combat::critical_injury::{
    apply_critical_injury, check_critical_trigger, CriticalInjuryApplied,
};
use crate::combat::damage::{apply_damage, DamageApplication, DamageOutcome, HitLocation};
use crate::dice::ndn_d6;
use crate::effects::SkillId;
use crate::error::RulesError;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, Stat};
use crate::world::World;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Selects the weapon (or unarmed style) used for the melee attack.
///
/// Drives skill selection, damage dice, and whether half-armor applies.
/// See pp.175–178.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeleeWeaponChoice {
    /// An equipped melee weapon, looked up by catalog slug.
    /// Uses `SkillId::MeleeWeapon`; halves defender armor. See pp.175–176.
    Weapon(WeaponId),
    /// Unarmed brawling.
    /// Uses `SkillId::Brawling`; does NOT halve defender armor. See p.176.
    Brawling,
    /// Martial arts attack with a specific form.
    /// Uses `SkillId::MartialArts(form)`; halves defender armor. See p.178.
    MartialArts(MartialArtsForm),
}

/// A Martial Arts Special Move the attacker may attempt on a hit.
///
/// Mapped to [`SpecialMoveEffect`] when the attack succeeds. The detailed
/// eligibility requirements (e.g. Iron Grip needs an active Grapple) are
/// pre-conditions the caller must enforce; this layer only fires the effect.
///
/// See p.179.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MartialArtsSpecialMove {
    /// A standard damaging strike. p.179.
    Strike,
    /// Grab and hold — initiates a Grapple. p.177.
    Hold,
    /// Sweep the target off their feet (trip / knock prone). p.179.
    Sweep,
    /// Throw the target to the ground, dealing BODY damage. p.177.
    Throw,
    /// Disarm the target — one held weapon drops or is taken. p.179.
    Disarm,
    /// Choke the target, dealing BODY damage bypassing armor. p.177.
    Choke,
    /// Recovery special move — regain footing. p.179.
    Resolution,
}

/// The concrete game-state effect that fires when a special move succeeds.
///
/// Returned in [`MeleeAttackOutcome::special_move_effect`]. The caller is
/// responsible for updating full game state (marking grappled, prone, etc.).
///
/// See p.179.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpecialMoveEffect {
    /// Target is now Grappled. See p.177.
    Held,
    /// Target is swept / tripped — treat as prone. See p.179.
    Swept,
    /// Target is thrown to the ground. See p.177.
    Thrown,
    /// Target is disarmed. The `WeaponId` names the dropped weapon; empty
    /// string is the placeholder when the caller has not specified a weapon.
    /// See p.179.
    Disarmed(WeaponId),
    /// Target is being choked. See p.177.
    Choked,
    /// Recovery succeeded. See p.179.
    Resolved,
}

/// The skill the defender uses to evade a melee attack.
///
/// Per p.175, the defender may choose Evasion, Brawling, or Martial Arts.
/// When `None` is passed in [`MeleeAttack::defender_skill_election`], this
/// module auto-elects the skill with the highest effective rank.
///
/// See p.175.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DefenderSkillElection {
    /// Use DEX + Evasion (the default in the resolution box on p.175).
    Evasion,
    /// Use DEX + Brawling. Permitted per p.175.
    Brawling,
    /// Use DEX + MartialArts(form). Permitted per p.175.
    MartialArts(MartialArtsForm),
}

/// A melee attack attempt resolved via the opposed check on p.175.
///
/// Call [`Resolution::resolve`] to get a [`MeleeAttackOutcome`].
///
/// See pp.175–179.
pub struct MeleeAttack {
    /// The attacking entity.
    pub attacker: EntityId,
    /// The defending entity.
    pub target: EntityId,
    /// Weapon or unarmed style — drives skill and damage.
    pub weapon_or_unarmed: MeleeWeaponChoice,
    /// LUCK points the attacker spends before rolling. See p.130.
    pub luck_to_spend: u8,
    /// GM/Beat-applied situational modifiers on the attacker's roll.
    pub additional_modifiers: Vec<NamedModifier>,
    /// Optional Martial Arts special move to attempt on a hit. See p.179.
    pub martial_arts_special_move: Option<MartialArtsSpecialMove>,
    /// Defender's elected evasion skill. `None` → auto-elect highest. See p.175.
    pub defender_skill_election: Option<DefenderSkillElection>,
    /// LUCK the defender commits to the evasion roll. See p.130.
    pub defender_luck_to_spend: u8,
    /// GM/Beat-applied situational modifiers on the defender's roll.
    pub additional_defender_modifiers: Vec<NamedModifier>,
    /// Critical Injury catalog for applying crits on a triggering hit.
    /// `None` skips crit application entirely. See p.187.
    pub crit_catalog: Option<Catalog<CriticalInjury>>,
    /// Body location targeted. Defaults to `Body` for standard melee.
    /// See p.184.
    pub hit_location: HitLocation,
}

/// Structured outcome of a [`MeleeAttack`] resolution.
///
/// See pp.175–179.
#[derive(Clone, Debug, PartialEq)]
pub struct MeleeAttackOutcome {
    /// The attacker's full check breakdown (DEX + skill + d10). See p.175.
    pub attack_breakdown: CheckBreakdown,
    /// The defender's full check breakdown (DEX + elected skill + d10). See p.175.
    pub defender_breakdown: CheckBreakdown,
    /// `true` iff the attacker's final value strictly exceeds the defender's.
    /// Ties favour the defender. See p.175.
    pub hit: bool,
    /// Individual damage die rolls (d6 values) on a hit. Empty on a miss.
    pub damage_rolls: Vec<u8>,
    /// Total pre-armor damage (sum of `damage_rolls`). `0` on a miss.
    pub damage_total: u16,
    /// Extra d6s contributed by the attacker's BODY bonus. See p.176 and
    /// the WP-307 spec thresholds (BODY 5–7 → +1, 8–10 → +2, 11+ → +3).
    pub body_bonus: u8,
    /// Structured damage outcome from WP-303. `None` on a miss.
    pub damage_outcome: Option<DamageOutcome>,
    /// Critical Injury applied if two or more d6s showed 6. See p.187.
    pub critical: Option<CriticalInjuryApplied>,
    /// Special move effect, if the attacker declared one and hit. See p.179.
    pub special_move_effect: Option<SpecialMoveEffect>,
    /// The skill the defender used (for logging / UI).
    pub defender_skill_used: SkillId,
    /// `true` if this attack type halves defender armor (weapon or MA).
    /// See pp.176, 178.
    pub half_armor: bool,
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for MeleeAttack {
    /// `Result` so LUCK and entity-lookup failures short-circuit cleanly.
    type Outcome = Result<MeleeAttackOutcome, RulesError>;

    /// Resolve this melee attack against `world`, drawing dice from `rng`.
    ///
    /// Determinism order:
    /// 1. Validate entities exist.
    /// 2. Pre-validate LUCK on both sides.
    /// 3. Elect defender skill.
    /// 4. Resolve opposed check via `OpposedCheck` (attacker LUCK + d10,
    ///    defender LUCK + d10, in that order).
    /// 5. On a hit: roll damage dice + body bonus dice.
    /// 6. Apply damage through WP-303.
    /// 7. Check and apply critical injury (WP-305) if triggered.
    /// 8. Emit special move effect if declared.
    ///
    /// See pp.175–179.
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // ----------------------------------------------------------------
        // 1. Validate entities. Pre-validation so no state is mutated on error.
        // ----------------------------------------------------------------
        if world.entity(self.attacker).is_none() {
            return Err(RulesError::EntityNotFound(self.attacker));
        }
        if world.entity(self.target).is_none() {
            return Err(RulesError::EntityNotFound(self.target));
        }

        // ----------------------------------------------------------------
        // 2. Determine attacker skill + half-armor flag. See p.175.
        // ----------------------------------------------------------------
        let (attacker_skill, half_armor) = attacker_skill_for_choice(&self.weapon_or_unarmed);

        // ----------------------------------------------------------------
        // 3. Elect defender skill (highest of Evasion/Brawling/MA). See p.175.
        // ----------------------------------------------------------------
        let defender_skill =
            elect_defender_skill(world, self.target, &self.defender_skill_election);

        // ----------------------------------------------------------------
        // 4. Resolve opposed check via WP-101's OpposedCheck.
        //    Order: attacker LUCK spend, attacker d10; defender LUCK spend,
        //    defender d10. See p.175, pp.128–130.
        // ----------------------------------------------------------------
        let opposed = OpposedCheck {
            attacker: self.attacker,
            attacker_stat: Stat::Dex,
            attacker_skill: attacker_skill.clone(),
            attacker_luck: self.luck_to_spend,
            defender: self.target,
            defender_stat: Stat::Dex,
            defender_skill: defender_skill.clone(),
            defender_luck: self.defender_luck_to_spend,
            additional_attacker_modifiers: self.additional_modifiers.clone(),
            additional_defender_modifiers: self.additional_defender_modifiers.clone(),
        };

        let opposed_outcome = opposed.resolve(world, rng)?;
        let attack_breakdown = opposed_outcome.attacker_breakdown;
        let defender_breakdown = opposed_outcome.defender_breakdown;
        let hit = opposed_outcome.attacker_wins;

        // ----------------------------------------------------------------
        // 5. Miss → return early with zeroed damage fields.
        // ----------------------------------------------------------------
        if !hit {
            return Ok(MeleeAttackOutcome {
                attack_breakdown,
                defender_breakdown,
                hit: false,
                damage_rolls: vec![],
                damage_total: 0,
                body_bonus: 0,
                damage_outcome: None,
                critical: None,
                special_move_effect: None,
                defender_skill_used: defender_skill,
                half_armor,
            });
        }

        // ----------------------------------------------------------------
        // 6. Damage rolls. See pp.176, 178.
        // ----------------------------------------------------------------
        let attacker_body = world
            .entity(self.attacker)
            .map(|c| c.current_body())
            .unwrap_or(1) as u8;

        let (total_dice, body_bonus_dice) =
            damage_dice_for_choice(&self.weapon_or_unarmed, attacker_body);

        // Roll all damage dice in one pass (base + body bonus are already
        // folded into total_dice by damage_dice_for_choice). All dice
        // participate in the critical trigger check. See pp.176, 178, 187.
        let damage_rolls: Vec<u8> = ndn_d6(total_dice, rng);

        let damage_total: u16 = damage_rolls.iter().map(|&d| u16::from(d)).sum();

        // ----------------------------------------------------------------
        // 7. Apply damage through WP-303. See p.186.
        // ----------------------------------------------------------------
        let triggered_critical = check_critical_trigger(&damage_rolls);
        let dmg_app = DamageApplication {
            target: self.target,
            raw_damage: damage_total,
            location: self.hit_location,
            bypass_armor: false,
            source_label: format!("melee ({:?})", self.weapon_or_unarmed),
            triggered_critical,
        };
        let damage_outcome = apply_damage(world, dmg_app);

        // ----------------------------------------------------------------
        // 8. Critical injury. See p.187.
        // ----------------------------------------------------------------
        let critical = if triggered_critical {
            if let Some(catalog) = &self.crit_catalog {
                apply_critical_injury(world, self.target, CritTable::Body, catalog, rng)
            } else {
                None
            }
        } else {
            None
        };

        // ----------------------------------------------------------------
        // 9. Special move effect on hit. See p.179.
        // ----------------------------------------------------------------
        let special_move_effect = self.martial_arts_special_move.map(special_move_effect_for);

        Ok(MeleeAttackOutcome {
            attack_breakdown,
            defender_breakdown,
            hit,
            damage_rolls,
            damage_total,
            body_bonus: body_bonus_dice,
            damage_outcome: Some(damage_outcome),
            critical,
            special_move_effect,
            defender_skill_used: defender_skill,
            half_armor,
        })
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Return `(attacker_skill_id, half_armor_applies)` for the chosen weapon.
///
/// Half-armor applies for melee weapons (p.176) and martial arts (p.178),
/// but NOT for brawling (p.176).
///
/// See pp.175–178.
fn attacker_skill_for_choice(choice: &MeleeWeaponChoice) -> (SkillId, bool) {
    match choice {
        // Melee weapons use MeleeWeapon skill and halve defender armor. p.175–176.
        MeleeWeaponChoice::Weapon(_) => (SkillId::MeleeWeapon, true),
        // Brawling uses Brawling skill; does NOT halve armor. p.176.
        MeleeWeaponChoice::Brawling => (SkillId::Brawling, false),
        // Martial Arts uses MartialArts(form) and halves defender armor. p.178.
        MeleeWeaponChoice::MartialArts(form) => (SkillId::MartialArts(form.clone()), true),
    }
}

/// Compute `(total_dice_to_roll, body_bonus_count)` for a melee attack.
///
/// The tuple represents:
/// - `total_dice_to_roll`: the number of d6s to roll in total.
/// - `body_bonus_count`: how many of those dice count as the WP-307 body
///   bonus (reported in `MeleeAttackOutcome::body_bonus`).
///
/// For `Weapon(_)`: base = 1 die + body bonus dice per WP-307 thresholds.
/// For `Brawling` / `MartialArts`: body-scaled total (pp.176, 178); the
/// bonus count is `total - 1` (the extra dice beyond the single base die).
///
/// Body bonus die thresholds (WP-307 spec):
/// - BODY 1–4 → +0
/// - BODY 5–7 → +1
/// - BODY 8–10 → +2
/// - BODY 11+ → +3
///
/// See pp.176, 178.
fn damage_dice_for_choice(choice: &MeleeWeaponChoice, body: u8) -> (u8, u8) {
    match choice {
        MeleeWeaponChoice::Weapon(_) => {
            // 1 base die; body bonus adds extra dice. See pp.175–176.
            // (Full weapon dice require the weapon catalog — see PR deviation.)
            let bonus = body_bonus_dice(body);
            // total to roll = 1 + bonus; body_bonus_count = bonus.
            (1 + bonus, bonus)
        }
        MeleeWeaponChoice::Brawling | MeleeWeaponChoice::MartialArts(_) => {
            // The entire dice pool scales with BODY. pp.176, 178.
            let total = brawling_damage_dice(body);
            // body_bonus is the extra dice beyond the base 1d6.
            let bonus = total.saturating_sub(1);
            // We roll all `total` dice in one pass; bonus tracks the delta.
            (total, bonus)
        }
    }
}

/// Body bonus die count per WP-307 spec thresholds.
///
/// Thresholds (WP-307):
/// - BODY 1–4 → 0 dice
/// - BODY 5–7 → 1 die
/// - BODY 8–10 → 2 dice
/// - BODY 11+ → 3 dice
///
/// See p.176.
pub(crate) fn body_bonus_dice(body: u8) -> u8 {
    // See pp.175–179.
    match body {
        0..=4 => 0,
        5..=7 => 1,
        8..=10 => 2,
        _ => 3, // 11 or higher
    }
}

/// Total brawling / martial arts damage dice based on BODY.
///
/// Per p.176 (Brawling table) and p.178 (Martial Arts damage table):
/// - BODY 4 or under → 1d6
/// - BODY 5–6 → 2d6
/// - BODY 7–10 → 3d6
/// - BODY 11 or higher → 4d6
///
/// See pp.176, 178.
pub(crate) fn brawling_damage_dice(body: u8) -> u8 {
    // See pp.176, 178.
    match body {
        0..=4 => 1,
        5..=6 => 2,
        7..=10 => 3,
        _ => 4, // 11 or higher
    }
}

/// Elect the defender's evasion skill per p.175.
///
/// If the caller supplied a [`DefenderSkillElection`], use it directly.
/// Otherwise auto-elect the skill with the highest effective rank among
/// Evasion, Brawling, and all Martial Arts forms the character has ranks in.
/// Ties break by precedence: Evasion > Brawling > MartialArts.
///
/// See p.175.
pub(crate) fn elect_defender_skill(
    world: &World,
    defender_id: EntityId,
    election: &Option<DefenderSkillElection>,
) -> SkillId {
    // Honour explicit election. See p.175.
    if let Some(choice) = election {
        return match choice {
            DefenderSkillElection::Evasion => SkillId::Evasion,
            DefenderSkillElection::Brawling => SkillId::Brawling,
            DefenderSkillElection::MartialArts(form) => SkillId::MartialArts(form.clone()),
        };
    }

    // Auto-elect highest rank. Fallback to Evasion if entity not found.
    let defender = match world.entity(defender_id) {
        Some(c) => c,
        None => return SkillId::Evasion,
    };

    let evasion_rank = defender.current_skill(&SkillId::Evasion);
    let brawling_rank = defender.current_skill(&SkillId::Brawling);

    // Scan all canonical MA forms for the highest rank.
    let best_ma: Option<(i16, MartialArtsForm)> = {
        use crate::catalog::skills::MartialArtsForm::*;
        let forms = [
            Karate,
            Taekwondo,
            Judo,
            Aikido,
            Boxing,
            Capoeira,
            Wrestling,
            AnimalKungFu,
        ];
        forms
            .into_iter()
            .filter_map(|form| {
                let skill = SkillId::MartialArts(form.clone());
                let rank = defender.current_skill(&skill);
                if rank > 0 {
                    Some((rank, form))
                } else {
                    None
                }
            })
            .max_by_key(|(rank, _)| *rank)
    };

    // Start with Evasion; upgrade to Brawling if strictly higher; then MA.
    let mut best_skill = SkillId::Evasion;
    let mut best_rank = evasion_rank;

    if brawling_rank > best_rank {
        best_skill = SkillId::Brawling;
        best_rank = brawling_rank;
    }

    if let Some((ma_rank, ma_form)) = best_ma {
        if ma_rank > best_rank {
            best_skill = SkillId::MartialArts(ma_form);
        }
    }

    best_skill
}

/// Map a `MartialArtsSpecialMove` to the concrete `SpecialMoveEffect`.
///
/// `Disarm` carries an empty `WeaponId` placeholder — the caller should
/// replace it with the actual weapon the target is holding.
///
/// See p.179.
fn special_move_effect_for(mv: MartialArtsSpecialMove) -> SpecialMoveEffect {
    // See p.179.
    match mv {
        MartialArtsSpecialMove::Strike => SpecialMoveEffect::Resolved,
        MartialArtsSpecialMove::Hold => SpecialMoveEffect::Held,
        MartialArtsSpecialMove::Sweep => SpecialMoveEffect::Swept,
        MartialArtsSpecialMove::Throw => SpecialMoveEffect::Thrown,
        MartialArtsSpecialMove::Disarm => SpecialMoveEffect::Disarmed(WeaponId("".into())),
        MartialArtsSpecialMove::Choke => SpecialMoveEffect::Choked,
        MartialArtsSpecialMove::Resolution => SpecialMoveEffect::Resolved,
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

    /// Build a (World, attacker EntityId, defender EntityId).
    ///
    /// Both characters have no armor. The attacker's skill is inserted at rank
    /// `attacker_skill_rank` for `attacker_skill_id`. The defender has
    /// Evasion at `defender_evasion`.
    #[allow(clippy::too_many_arguments)]
    fn make_world(
        attacker_id: u128,
        defender_id: u128,
        attacker_body: u8,
        attacker_dex: u8,
        attacker_skill_rank: u8,
        attacker_skill_id: SkillId,
        defender_dex: u8,
        defender_evasion: u8,
    ) -> (World, EntityId, EntityId) {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(attacker_id));
        pc.stats.dex = attacker_dex;
        pc.stats.body = attacker_body;
        pc.stats.luck = 6;
        pc.luck_pool = 6;
        if attacker_skill_rank > 0 {
            pc.skills
                .ranks
                .insert(attacker_skill_id, attacker_skill_rank);
        }
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;

        let mut npc = fresh_pc();
        npc.id = CharacterId(Uuid::from_u128(defender_id));
        npc.stats.dex = defender_dex;
        npc.stats.luck = 6;
        npc.luck_pool = 6;
        npc.skills.ranks.insert(SkillId::Evasion, defender_evasion);
        recompute_wounds(&mut npc);
        npc.wounds.current_hp = npc.wounds.max_hp as i16;

        let att_eid = EntityId(Uuid::from_u128(attacker_id));
        let def_eid = EntityId(Uuid::from_u128(defender_id));

        let mut world = World::new(pc);
        world.npcs.insert(NpcId(Uuid::from_u128(defender_id)), npc);

        (world, att_eid, def_eid)
    }

    // ---- Acceptance tests ---------------------------------------------------

    /// Acceptance: `test_melee_opposed_check` — defender rolls DEX + Evasion.
    ///
    /// Attacker: DEX 8, MeleeWeapon 6 (base 14). Defender: DEX 4, Evasion 2
    /// (base 6). A seed where att d10 beats the gap must produce a hit, and
    /// the defender breakdown must show skill_value == 2 (Evasion). See p.175.
    #[test]
    fn test_melee_opposed_check() {
        let (mut world, att_id, def_id) =
            make_world(0xA1, 0xB1, 6, 8, 6, SkillId::MeleeWeapon, 4, 2);

        // att_base = 14, def_base = 6 → gap of 8. Any non-critical roll wins.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (14_i16 + a as i16) > (6_i16 + b as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let attack = MeleeAttack {
            attacker: att_id,
            target: def_id,
            weapon_or_unarmed: MeleeWeaponChoice::Weapon(WeaponId("medium_melee".into())),
            luck_to_spend: 0,
            additional_modifiers: vec![],
            martial_arts_special_move: None,
            defender_skill_election: None,
            defender_luck_to_spend: 0,
            additional_defender_modifiers: vec![],
            crit_catalog: None,
            hit_location: HitLocation::Body,
        };

        let outcome = attack.resolve(&mut world, &mut rng).expect("must succeed");
        assert!(outcome.hit, "huge advantage attacker must hit");
        // Defender used Evasion (only skill they have). See p.175.
        assert_eq!(
            outcome.defender_skill_used,
            SkillId::Evasion,
            "defender must use Evasion when it is their only skill"
        );
        // Breakdown stat/skill values must reflect the characters' stats.
        assert_eq!(outcome.attack_breakdown.stat_value, 8, "attacker DEX");
        assert_eq!(
            outcome.attack_breakdown.skill_value, 6,
            "attacker MeleeWeapon"
        );
        assert_eq!(outcome.defender_breakdown.stat_value, 4, "defender DEX");
        assert_eq!(
            outcome.defender_breakdown.skill_value, 2,
            "defender Evasion"
        );
    }

    /// Acceptance: `test_body_bonus_applied` — BODY 6 → +1 damage die.
    ///
    /// A Brawling attack with BODY 6 must produce `body_bonus == 1` and roll
    /// exactly 2d6. See p.176.
    #[test]
    fn test_body_bonus_applied() {
        // Verify the pure helper directly.
        assert_eq!(
            body_bonus_dice(6),
            1,
            "BODY 6 must be +1 die (BODY 5–7 bracket)"
        );

        // Integration: run a full Brawling resolve with BODY 6.
        let (mut world, att_id, def_id) = make_world(
            0xC1,
            0xD1,
            /*body=*/ 6,
            /*dex=*/ 10,
            /*skill=*/ 6,
            SkillId::Brawling,
            /*def_dex=*/ 2,
            /*def_evasion=*/ 0,
        );

        // Guarantee a hit: att_base = 16, def_base = 2 → huge gap.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (16_i16 + a as i16) > (2_i16 + b as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let attack = MeleeAttack {
            attacker: att_id,
            target: def_id,
            weapon_or_unarmed: MeleeWeaponChoice::Brawling,
            luck_to_spend: 0,
            additional_modifiers: vec![],
            martial_arts_special_move: None,
            defender_skill_election: None,
            defender_luck_to_spend: 0,
            additional_defender_modifiers: vec![],
            crit_catalog: None,
            hit_location: HitLocation::Body,
        };

        let outcome = attack.resolve(&mut world, &mut rng).expect("must succeed");
        assert!(outcome.hit, "must hit");
        assert_eq!(outcome.body_bonus, 1, "BODY 6 must produce body_bonus == 1");
        // Brawling BODY 6 → 2 total dice (1 base + 1 bonus). See p.176.
        assert_eq!(
            outcome.damage_rolls.len(),
            2,
            "Brawling BODY 6 must roll exactly 2d6"
        );
        assert_eq!(
            outcome.damage_total,
            outcome
                .damage_rolls
                .iter()
                .map(|&d| u16::from(d))
                .sum::<u16>(),
            "damage_total must equal sum of damage_rolls"
        );
    }

    /// Acceptance: `test_brawling_uses_brawling_skill` — unarmed brawl uses
    /// `SkillId::Brawling`. See p.176.
    #[test]
    fn test_brawling_uses_brawling_skill() {
        let (skill, half_armor) = attacker_skill_for_choice(&MeleeWeaponChoice::Brawling);
        assert_eq!(
            skill,
            SkillId::Brawling,
            "MeleeWeaponChoice::Brawling must use SkillId::Brawling (p.176)"
        );
        assert!(
            !half_armor,
            "Brawling must NOT halve defender armor (p.176)"
        );
    }

    /// Acceptance: `test_martial_arts_form_uses_specific_skill` — passing
    /// `MartialArts(Karate)` uses `SkillId::MartialArts(Karate)`. See p.178.
    #[test]
    fn test_martial_arts_form_uses_specific_skill() {
        use crate::catalog::skills::MartialArtsForm;

        let choice = MeleeWeaponChoice::MartialArts(MartialArtsForm::Karate);
        let (skill, half_armor) = attacker_skill_for_choice(&choice);
        assert_eq!(
            skill,
            SkillId::MartialArts(MartialArtsForm::Karate),
            "MartialArts(Karate) must use SkillId::MartialArts(Karate) (p.178)"
        );
        assert!(half_armor, "Martial Arts must halve defender armor (p.178)");
    }

    /// Acceptance: `test_defender_picks_highest_skill` — defender with Evasion 4
    /// and Brawling 6 auto-elects Brawling. See p.175.
    #[test]
    fn test_defender_picks_highest_skill() {
        use crate::catalog::skills::MartialArtsForm;

        let att_uuid = Uuid::from_u128(0xE1);
        let def_uuid = Uuid::from_u128(0xF1);

        let mut pc = fresh_pc();
        pc.id = CharacterId(att_uuid);

        let mut npc = fresh_pc();
        npc.id = CharacterId(def_uuid);
        npc.skills.ranks.insert(SkillId::Evasion, 4);
        npc.skills.ranks.insert(SkillId::Brawling, 6);

        let mut world = World::new(pc);
        world.npcs.insert(NpcId(def_uuid), npc);

        let def_eid = EntityId(def_uuid);

        // No explicit election → Brawling 6 beats Evasion 4.
        let elected = elect_defender_skill(&world, def_eid, &None);
        assert_eq!(
            elected,
            SkillId::Brawling,
            "Brawling 6 > Evasion 4 → must elect Brawling (p.175)"
        );

        // With Karate 8 also present: Karate beats Brawling.
        world
            .npcs
            .get_mut(&NpcId(def_uuid))
            .unwrap()
            .skills
            .ranks
            .insert(SkillId::MartialArts(MartialArtsForm::Karate), 8);

        let elected_ma = elect_defender_skill(&world, def_eid, &None);
        assert_eq!(
            elected_ma,
            SkillId::MartialArts(MartialArtsForm::Karate),
            "Karate 8 > Brawling 6 → must elect Karate (p.175)"
        );
    }

    // ---- Additional regression tests ----------------------------------------

    /// Regression: `test_defender_uses_evasion_when_highest` — only Evasion 3
    /// set, so Evasion is elected.
    #[test]
    fn test_defender_uses_evasion_when_highest() {
        let (world, _att_id, def_id) = make_world(0xA2, 0xB2, 6, 8, 6, SkillId::MeleeWeapon, 4, 3);
        let elected = elect_defender_skill(&world, def_id, &None);
        assert_eq!(elected, SkillId::Evasion);
    }

    /// Regression: miss returns zero damage, no damage_outcome, no critical.
    #[test]
    fn test_miss_returns_zero_damage() {
        // Defender has huge advantage: att base=4, def base=14.
        let (mut world, att_id, def_id) =
            make_world(0xC2, 0xD2, 6, 4, 0, SkillId::MeleeWeapon, 8, 6);

        // Find a seed where defender wins (4+a <= 14+b is almost always true).
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let b = d10(r);
            (4_i16 + a as i16) <= (14_i16 + b as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let attack = MeleeAttack {
            attacker: att_id,
            target: def_id,
            weapon_or_unarmed: MeleeWeaponChoice::Brawling,
            luck_to_spend: 0,
            additional_modifiers: vec![],
            martial_arts_special_move: None,
            defender_skill_election: None,
            defender_luck_to_spend: 0,
            additional_defender_modifiers: vec![],
            crit_catalog: None,
            hit_location: HitLocation::Body,
        };

        let outcome = attack.resolve(&mut world, &mut rng).expect("must not err");
        assert!(!outcome.hit);
        assert_eq!(outcome.damage_rolls, vec![]);
        assert_eq!(outcome.damage_total, 0);
        assert!(outcome.damage_outcome.is_none());
        assert!(outcome.critical.is_none());
        assert!(outcome.special_move_effect.is_none());
    }

    /// Regression: unknown attacker returns EntityNotFound.
    #[test]
    fn test_unknown_attacker_returns_err() {
        let pc = fresh_pc();
        let def_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        let unknown = EntityId(Uuid::from_u128(0xDEAD));
        let mut rng = Rng::seed_from_u64(0);

        let attack = MeleeAttack {
            attacker: unknown,
            target: def_id,
            weapon_or_unarmed: MeleeWeaponChoice::Brawling,
            luck_to_spend: 0,
            additional_modifiers: vec![],
            martial_arts_special_move: None,
            defender_skill_election: None,
            defender_luck_to_spend: 0,
            additional_defender_modifiers: vec![],
            crit_catalog: None,
            hit_location: HitLocation::Body,
        };

        let err = attack
            .resolve(&mut world, &mut rng)
            .expect_err("unknown attacker must Err");
        assert!(matches!(err, RulesError::EntityNotFound(id) if id == unknown));
    }

    /// Regression: body_bonus_dice thresholds match WP-307 spec.
    #[test]
    fn test_body_bonus_dice_thresholds() {
        assert_eq!(body_bonus_dice(1), 0);
        assert_eq!(body_bonus_dice(4), 0);
        assert_eq!(body_bonus_dice(5), 1);
        assert_eq!(body_bonus_dice(7), 1);
        assert_eq!(body_bonus_dice(8), 2);
        assert_eq!(body_bonus_dice(10), 2);
        assert_eq!(body_bonus_dice(11), 3);
        assert_eq!(body_bonus_dice(20), 3);
    }

    /// Regression: brawling_damage_dice thresholds match p.176.
    #[test]
    fn test_brawling_damage_dice_thresholds() {
        // p.176: ≤4 → 1d6, 5–6 → 2d6, 7–10 → 3d6, 11+ → 4d6.
        assert_eq!(brawling_damage_dice(4), 1);
        assert_eq!(brawling_damage_dice(5), 2);
        assert_eq!(brawling_damage_dice(6), 2);
        assert_eq!(brawling_damage_dice(7), 3);
        assert_eq!(brawling_damage_dice(10), 3);
        assert_eq!(brawling_damage_dice(11), 4);
    }

    /// Regression: half_armor flag is correct per choice. pp.176, 178.
    #[test]
    fn test_half_armor_flag_per_choice() {
        let (_, w) = attacker_skill_for_choice(&MeleeWeaponChoice::Weapon(WeaponId("x".into())));
        assert!(w, "Weapon must halve armor (p.176)");

        let (_, b) = attacker_skill_for_choice(&MeleeWeaponChoice::Brawling);
        assert!(!b, "Brawling must NOT halve armor (p.176)");

        use crate::catalog::skills::MartialArtsForm;
        let (_, m) =
            attacker_skill_for_choice(&MeleeWeaponChoice::MartialArts(MartialArtsForm::Judo));
        assert!(m, "MartialArts must halve armor (p.178)");
    }

    /// Regression: special move effects map correctly. See p.179.
    #[test]
    fn test_special_move_effect_mapping() {
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Hold),
            SpecialMoveEffect::Held
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Sweep),
            SpecialMoveEffect::Swept
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Throw),
            SpecialMoveEffect::Thrown
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Choke),
            SpecialMoveEffect::Choked
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Resolution),
            SpecialMoveEffect::Resolved
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Strike),
            SpecialMoveEffect::Resolved
        ));
        assert!(matches!(
            special_move_effect_for(MartialArtsSpecialMove::Disarm),
            SpecialMoveEffect::Disarmed(_)
        ));
    }
}
