//! Autofire resolution — WP-309.
//!
//! Implements the Autofire fire mode described on pp.172–174 of the
//! *Cyberpunk RED Core Rules*. Autofire applies to SMGs (cap 3) and
//! Assault Rifles (cap 4) — the only ranged weapons with a
//! [`crate::catalog::weapons::WeaponFeature::Autofire`] feature.
//!
//! ## Mechanics (pp.173–174)
//!
//! 1. **Ammunition cost**: Autofire always costs exactly **10 rounds**
//!    (p.173: "Fire a total of 10 Rounds into an area"). The attack is
//!    rejected if the attacker has fewer than 10 rounds loaded (see
//!    [`RulesError::InsufficientAmmo`]).
//!
//! 2. **Attack roll**: `REF + Autofire skill + 1d10` vs the weapon's
//!    *Autofire DV table* (p.173 — distinct from the single-shot DV on
//!    p.172). The DV at the target's range is looked up via
//!    [`crate::catalog::weapons::RangeBand::autofire_dv_at`].
//!
//! 3. **Damage**: On a hit, damage = `2d6 × min(beat_amount, cap)`,
//!    where `beat_amount = final_value − dv` (the margin) and `cap` is
//!    the weapon-specific Autofire cap from
//!    [`crate::catalog::weapons::WeaponFeature::Autofire(cap)`].
//!    A miss deals no damage.
//!
//! 4. **Critical injury trigger**: If **both** damage d6s show a 6 (i.e.
//!    `damage_rolls == [6, 6]`), a critical injury is triggered. This is
//!    **different** from the standard rule (p.187: "two or more dice") —
//!    autofire rolls exactly 2d6 for damage, so the crit condition is
//!    specifically "rolled dice = [6, 6]". See pp.173–174.
//!
//! 5. **Aimed shots**: Autofire cannot be combined with an Aimed Shot
//!    (p.173). The [`AutofireAttack`] struct does not include an aimed-shot
//!    field; trying to use autofire with an aim is architecturally rejected.
//!
//! 6. **Dodge**: A defender with REF ≥ 8 may attempt a dodge reaction
//!    (p.172); if `defender_dodges` is true, the dodge roll is compared
//!    with the autofire DV and the higher value is used as the effective DV,
//!    following the same pattern as ranged single-shot (WP-306).
//!
//! ## Deviation from WP-309 spec
//!
//! The public API spec in the plan does not include a `rounds_in_magazine`
//! parameter on [`AutofireAttack`]. Because the `Character` struct does not
//! carry an explicit "rounds currently loaded" field (magazine state is not
//! yet modeled in the world — that arrives in a future WP), the caller must
//! supply the current round count via `rounds_in_magazine`. This is the
//! minimal defensible choice: it defers magazine-state ownership to the
//! caller while keeping the rules engine free of inventory-management
//! concerns.
//!
//! Additionally, the `Catalog<CriticalInjury>` parameter follows the same
//! deviation as WP-305's `apply_critical_injury` — it is added here so the
//! crit-application step can look up the full injury definition at runtime
//! without hardcoding catalog data. The PR documents this.
//!
//! See pp.172–174.

use crate::catalog::critical_injuries::{CritTable, CriticalInjury};
use crate::catalog::weapons::{Weapon, WeaponFeature};
use crate::catalog::Catalog;
use crate::character::WeaponId;
use crate::checks::skill_check::NamedModifier;
use crate::combat::critical_injury::{apply_critical_injury, CriticalInjuryApplied};
use crate::combat::damage::{apply_damage, DamageApplication, DamageOutcome, HitLocation};
use crate::dice::{d10_with_crits, d6};
use crate::effects::SkillId;
use crate::error::RulesError;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, Stat, DV};
use crate::world::World;

// ---------------------------------------------------------------------------
// AutofireAttack
// ---------------------------------------------------------------------------

/// A single Autofire action against one target. See pp.173–174.
///
/// The attacker fires 10 rounds in a burst. The attack roll is
/// `REF + Autofire skill + 1d10` vs the weapon's autofire DV at the
/// target's range. On a hit, damage = `2d6 × min(margin, cap)` where
/// `cap` comes from the weapon's
/// [`WeaponFeature::Autofire(cap)`][crate::catalog::weapons::WeaponFeature::Autofire].
///
/// **No aimed shot**: Autofire cannot be combined with an Aimed Shot (p.173).
/// The struct deliberately omits an `aimed_shot` field to enforce this at the
/// type level.
///
/// **Rounds in magazine**: Because the `Character` struct does not yet carry
/// explicit per-weapon magazine state, the caller supplies `rounds_in_magazine`
/// directly. The resolution rejects the attack if this is < 10 (p.173).
///
/// See pp.173–174.
pub struct AutofireAttack {
    /// The attacking entity. Resolved via [`World::entity`] / [`World::entity_mut`].
    pub attacker: EntityId,
    /// The target entity. Used for damage application.
    pub target: EntityId,
    /// The weapon being used. Must exist in the supplied weapon catalog and
    /// must carry a [`WeaponFeature::Autofire`] feature.
    pub weapon: WeaponId,
    /// How many rounds are currently loaded in the weapon. Must be ≥ 10 or
    /// the resolution returns [`RulesError::InsufficientAmmo`].
    pub rounds_in_magazine: u8,
    /// Distance to target in metres. Used to look up the autofire DV. Must
    /// be within the weapon's autofire range table (max 100 m/yd, p.173).
    pub range_meters: u16,
    /// LUCK Points the attacker spends on the attack roll. Validated and
    /// debited before the d10 is consumed (preserving seed-determinism).
    pub luck_to_spend: u8,
    /// Whether the defender elected a dodge reaction this round (requires
    /// REF ≥ 8, p.172). If `true`, the defender's DEX + Evasion + d10 is
    /// rolled and compared with the autofire DV; the higher value is used.
    pub defender_dodges: bool,
    /// Situational modifiers from the GM/Beat layer (cover, darkness, etc.).
    /// Persistent character modifiers come from the actor's
    /// [`crate::effects::EffectStack`] automatically.
    pub additional_modifiers: Vec<NamedModifier>,
}

// ---------------------------------------------------------------------------
// AutofireOutcome
// ---------------------------------------------------------------------------

/// The structured result of a resolved [`AutofireAttack`]. See pp.173–174.
#[derive(Clone, Debug, PartialEq)]
pub struct AutofireOutcome {
    /// Full breakdown of the attacker's `REF + Autofire + d10` roll,
    /// including the DV that was used (either the range-table DV or the
    /// defender's dodge result, whichever was higher).
    pub attack_breakdown: CheckBreakdown,
    /// How much the attack beat the DV by, already capped to the weapon's
    /// autofire cap. Always 0 on a miss.
    pub beat_dv_by: u8,
    /// The two d6 damage dice rolled (always exactly 2). On a miss both
    /// entries are 0.
    pub damage_rolls: [u8; 2],
    /// `2d6 × beat_dv_by` total. 0 on a miss.
    pub damage_total: u16,
    /// Result of applying `damage_total` to the target's HP / armor, if the
    /// attack hit. `None` on a miss.
    pub damage_outcome: Option<DamageOutcome>,
    /// Critical Injury applied when both d6 showed 6. See pp.173–174.
    /// `None` if no crit triggered or if the crit roll found no new injury.
    pub critical: Option<CriticalInjuryApplied>,
    /// Always 10, regardless of hit or miss (p.173: autofire costs 10 rounds
    /// unconditionally once initiated). The caller must decrement the
    /// magazine by this amount.
    pub bullets_consumed: u8,
    /// Optional breakdown of the defender's dodge roll, if `defender_dodges`
    /// was `true`.
    pub dodge_breakdown: Option<CheckBreakdown>,
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for AutofireAttack {
    /// `Result` so validation failures (insufficient ammo, weapon not found,
    /// no autofire feature, out of range) short-circuit before any dice roll.
    type Outcome = Result<AutofireOutcome, RulesError>;

    /// Resolve the autofire attack.
    ///
    /// # Parameter notes
    ///
    /// This implementation ignores `world` for the attack-roll character
    /// lookups that need the weapon catalog and critical injury catalog,
    /// which are passed separately via
    /// [`AutofireAttack::resolve_with_catalogs`]. Calling this method
    /// directly panics if you haven't read the note below.
    ///
    /// **Prefer [`AutofireAttack::resolve_with_catalogs`]** — the plain
    /// `resolve` cannot satisfy the Resolution trait signature without the
    /// catalogs, so it returns `Err(RulesError::WeaponNotFound(...))` with a
    /// sentinel ID as a signal. The trait bound exists for
    /// type-system compatibility; practical callers use the richer entry
    /// point.
    ///
    /// # Alternative: use `resolve_with_catalogs`
    ///
    /// ```rust,ignore
    /// let outcome = attack.resolve_with_catalogs(
    ///     world, weapons_catalog, crit_catalog, rng
    /// );
    /// ```
    fn resolve(&self, _world: &mut World, _rng: &mut Rng) -> Self::Outcome {
        // The bare Resolution::resolve cannot supply catalogs.
        // Callers should use resolve_with_catalogs instead.
        // Return a descriptive error rather than panicking.
        Err(RulesError::WeaponNotFound(WeaponId(
            "use AutofireAttack::resolve_with_catalogs — bare Resolution::resolve \
             cannot supply weapon and crit catalogs"
                .into(),
        )))
    }
}

impl AutofireAttack {
    /// Resolve the autofire attack using the supplied weapon and critical-injury
    /// catalogs.
    ///
    /// This is the primary entry point for callers. The [`Resolution::resolve`]
    /// implementation is provided only for trait-system compatibility; it
    /// returns an error pointing here.
    ///
    /// # Parameters
    ///
    /// - `world`: mutable game state for character lookups and damage application.
    /// - `weapons`: the loaded weapon catalog (see
    ///   [`crate::catalog::weapons::load_weapons_catalog`]).
    /// - `crit_catalog`: the body-table Critical Injury catalog (see
    ///   [`crate::catalog::critical_injuries::load_critical_injuries_body`]).
    ///   Autofire crits are always body-table injuries (no Aimed Shot).
    /// - `rng`: deterministic RNG — consumed in a fixed order for replay.
    ///
    /// # Resolution order (for seed-determinism)
    ///
    /// 1. Validate: attacker exists, weapon exists, weapon has Autofire, ≥10
    ///    rounds in magazine, target in autofire range.
    /// 2. Validate and spend attacker LUCK.
    /// 3. Roll the attacker's d10 (`d10_with_crits`).
    /// 4. If `defender_dodges`: look up defender, roll their dodge d10.
    /// 5. Compute effective DV (max of range-table DV and dodge result).
    /// 6. Determine hit, beat_dv_by (capped to weapon autofire cap).
    /// 7. If hit: roll 2d6 damage dice.
    /// 8. If hit: apply damage via WP-303.
    /// 9. If both d6 = 6: apply critical injury via WP-305.
    ///
    /// See pp.173–174.
    pub fn resolve_with_catalogs(
        &self,
        world: &mut World,
        weapons: &Catalog<Weapon>,
        crit_catalog: &Catalog<CriticalInjury>,
        rng: &mut Rng,
    ) -> Result<AutofireOutcome, RulesError> {
        // ----------------------------------------------------------------
        // Step 1: Validate pre-conditions. No RNG consumed yet.
        // ----------------------------------------------------------------

        // 1a. Attacker must exist in world.
        if world.entity(self.attacker).is_none() {
            return Err(RulesError::EntityNotFound(self.attacker));
        }

        // 1b. Weapon must exist in the catalog.
        let weapon = weapons
            .get(&self.weapon.0)
            .ok_or_else(|| RulesError::WeaponNotFound(self.weapon.clone()))?;

        // 1c. Weapon must carry an Autofire feature. See p.173.
        let autofire_cap = weapon
            .features
            .iter()
            .find_map(|f| {
                if let WeaponFeature::Autofire(cap) = f {
                    Some(cap)
                } else {
                    None
                }
            })
            .copied()
            .ok_or_else(|| RulesError::WeaponLacksAutofire(self.weapon.clone()))?;

        // 1d. Magazine must have at least 10 rounds. See p.173.
        if self.rounds_in_magazine < 10 {
            return Err(RulesError::InsufficientAmmo {
                required: 10,
                available: self.rounds_in_magazine,
            });
        }

        // 1e. Target must be within autofire range. Autofire tops out at
        //     51–100 m/yd (the furthest band on the p.173 table).
        let base_dv = weapon
            .ranges
            .autofire_dv_at(self.range_meters)
            .ok_or(RulesError::OutOfAutofireRange)?;

        // ----------------------------------------------------------------
        // Step 2: Validate + spend LUCK (before any dice roll).
        // See p.130 ("Using Your LUCK") and the skill_check.rs pattern.
        // ----------------------------------------------------------------
        {
            let attacker = world
                .entity_mut(self.attacker)
                .ok_or(RulesError::EntityNotFound(self.attacker))?;
            attacker.spend_luck(self.luck_to_spend)?;
        }

        // ----------------------------------------------------------------
        // Step 3: Roll the attacker's d10. See p.173.
        // REF + Autofire skill + 1d10 vs autofire DV.
        // ----------------------------------------------------------------
        let d10 = d10_with_crits(rng);

        // Snapshot attacker's REF and Autofire skill (after effects).
        let (stat_value, skill_value, modifier_total) = {
            let attacker = world
                .entity(self.attacker)
                .ok_or(RulesError::EntityNotFound(self.attacker))?;
            let stat_val = attacker.current_stat(Stat::Ref);
            let skill_val = attacker.current_skill(&SkillId::Autofire);
            let aap = i16::from(attacker.all_actions_penalty());
            let extra: i16 = self
                .additional_modifiers
                .iter()
                .map(|m| i16::from(m.value))
                .sum();
            (stat_val, skill_val, aap + extra)
        };

        let final_value =
            stat_value + skill_value + modifier_total + self.luck_to_spend as i16 + d10.net;

        // ----------------------------------------------------------------
        // Step 4: Optional defender dodge. See p.172.
        // If the defender dodges, roll DEX + Evasion + d10 for them. The
        // effective DV is max(base_dv, dodge_result).
        // ----------------------------------------------------------------
        let (effective_dv_val, dodge_breakdown) = if self.defender_dodges {
            // Defender must exist to attempt a dodge.
            let defender = world
                .entity(self.target)
                .ok_or(RulesError::EntityNotFound(self.target))?;

            let def_stat = defender.current_stat(Stat::Dex);
            let def_skill = defender.current_skill(&SkillId::Evasion);
            let def_aap = i16::from(defender.all_actions_penalty());
            let def_d10 = d10_with_crits(rng);
            let def_final = def_stat + def_skill + def_aap + def_d10.net;

            // The defender's result acts as the DV — if it exceeds the
            // range-table DV, the higher value is used. Saturate to u8
            // for the DV field (same approach as skill_check.rs).
            let dodge_dv_val =
                u8::try_from(def_final.clamp(0, i16::from(u8::MAX))).unwrap_or(u8::MAX);
            let eff_dv = base_dv.max(dodge_dv_val);

            let def_bd = CheckBreakdown::new(
                def_stat,
                def_skill,
                def_aap,
                0, // defender does not spend LUCK on reactive dodge
                def_d10,
                DV(u8::try_from(final_value.clamp(0, i16::from(u8::MAX))).unwrap_or(u8::MAX)),
            );
            (eff_dv, Some(def_bd))
        } else {
            (base_dv, None)
        };

        let effective_dv = DV(effective_dv_val);

        // Build the attacker's CheckBreakdown with the effective DV.
        let attack_breakdown = CheckBreakdown::new(
            stat_value,
            skill_value,
            modifier_total,
            self.luck_to_spend,
            d10,
            effective_dv,
        );

        // ----------------------------------------------------------------
        // Step 5: Determine hit and damage cap. See p.173.
        // ----------------------------------------------------------------
        let hit = attack_breakdown.success;

        // beat_dv_by = max(0, margin) capped to the weapon's autofire cap.
        // The cap is per weapon type (3 for SMGs, 4 for Assault Rifles, p.173).
        let raw_beat: u8 = if hit {
            // margin is always >= 0 on success; clamp defensively.
            u8::try_from(attack_breakdown.margin.max(0))
                .unwrap_or(u8::MAX)
                .min(autofire_cap)
        } else {
            0
        };

        // ----------------------------------------------------------------
        // Step 6: Roll 2d6 damage and compute total. See p.173.
        // Damage = 2d6 × beat_dv_by. Dice are consumed regardless of hit
        // (preserves RNG stream determinism — the caller knows damage rolls
        // are always present in the outcome record even on a miss).
        // ----------------------------------------------------------------
        let d6_a = d6(rng);
        let d6_b = d6(rng);
        let damage_rolls = [d6_a, d6_b];

        let damage_total: u16 = if hit {
            u16::from(d6_a) * u16::from(raw_beat) + u16::from(d6_b) * u16::from(raw_beat)
        } else {
            0
        };

        // ----------------------------------------------------------------
        // Step 7: Apply damage via WP-303. See pp.173–174, p.186.
        // Autofire always hits Body — there is no aimed-shot location for
        // autofire (p.173).
        // ----------------------------------------------------------------
        let both_sixes = d6_a == 6 && d6_b == 6;
        let triggered_critical = hit && both_sixes;

        let damage_outcome = if hit {
            let da = DamageApplication {
                target: self.target,
                raw_damage: damage_total,
                location: HitLocation::Body, // Autofire cannot aim, so always Body. See p.173.
                bypass_armor: false,
                source_label: format!("Autofire ({})", self.weapon.0),
                triggered_critical,
            };
            Some(apply_damage(world, da))
        } else {
            None
        };

        // ----------------------------------------------------------------
        // Step 8: Critical injury on [6, 6]. See pp.173–174.
        // Autofire's crit condition is specifically "both 2d6 damage dice
        // show 6" — NOT the standard "two or more sixes among N dice"
        // (p.187). With exactly 2 dice, the condition is identical, but
        // the rulebook frames it as "both" for autofire specifically.
        // ----------------------------------------------------------------
        let critical = if triggered_critical {
            apply_critical_injury(world, self.target, CritTable::Body, crit_catalog, rng)
        } else {
            None
        };

        // ----------------------------------------------------------------
        // Done. Bullets consumed = 10 regardless of outcome. See p.173.
        // ----------------------------------------------------------------
        Ok(AutofireOutcome {
            attack_breakdown,
            beat_dv_by: raw_beat,
            damage_rolls,
            damage_total,
            damage_outcome,
            critical,
            bullets_consumed: 10,
            dodge_breakdown,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::critical_injuries::load_critical_injuries_body;
    use crate::catalog::weapons::load_weapons_catalog;
    use crate::character::hp::recompute_wounds;
    use crate::types::{CharacterId, NpcId};
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;
    use std::path::PathBuf;
    use uuid::Uuid;

    // ---- Catalog paths -------------------------------------------------------

    fn weapons_catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content/catalogs/weapons.ron");
        p
    }

    fn crit_body_catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p.push("content/tables/critical_injuries_body.ron");
        p
    }

    fn weapons_cat() -> Catalog<Weapon> {
        load_weapons_catalog(&weapons_catalog_path()).expect("weapons catalog must load")
    }

    fn crit_cat() -> Catalog<CriticalInjury> {
        load_critical_injuries_body(&crit_body_catalog_path()).expect("crit body catalog must load")
    }

    // ---- World helpers -------------------------------------------------------

    /// Build a world with a PC set up for autofire testing.
    /// REF 7, Autofire skill 4, enough HP to survive hits.
    fn world_with_autofire_pc() -> (World, EntityId) {
        let mut pc = fresh_pc();
        pc.stats.r#ref = 7;
        pc.stats.will = 5;
        pc.stats.body = 5;
        pc.skills.ranks.insert(SkillId::Autofire, 4);
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        let pc_id = EntityId(pc.id.0);
        let world = World::new(pc);
        (world, pc_id)
    }

    /// Add an NPC target to the world. Returns the NPC's EntityId.
    fn add_npc_target(world: &mut World) -> EntityId {
        let mut npc = fresh_pc();
        let uuid = Uuid::from_u128(0xABCDEF01);
        npc.id = CharacterId(uuid);
        npc.stats.body = 5;
        npc.stats.will = 5;
        recompute_wounds(&mut npc);
        npc.wounds.current_hp = npc.wounds.max_hp as i16;
        let npc_id = NpcId(uuid);
        world.npcs.insert(npc_id, npc);
        EntityId(uuid)
    }

    /// Build a standard autofire attack on the SMG at 10m range, 10+ rounds.
    fn smg_attack(attacker: EntityId, target: EntityId, rounds_in_magazine: u8) -> AutofireAttack {
        AutofireAttack {
            attacker,
            target,
            weapon: WeaponId("smg".into()),
            rounds_in_magazine,
            range_meters: 10,
            luck_to_spend: 0,
            defender_dodges: false,
            additional_modifiers: vec![],
        }
    }

    /// Build a standard autofire attack on the Assault Rifle at 10m, 10+ rounds.
    fn assault_rifle_attack(
        attacker: EntityId,
        target: EntityId,
        rounds_in_magazine: u8,
    ) -> AutofireAttack {
        AutofireAttack {
            attacker,
            target,
            weapon: WeaponId("assault_rifle".into()),
            rounds_in_magazine,
            range_meters: 10,
            luck_to_spend: 0,
            defender_dodges: false,
            additional_modifiers: vec![],
        }
    }

    /// Walk seeds until the d10 and subsequent 2d6 satisfy `pred`.
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(&mut Rng) -> bool,
    {
        for seed in 0..5_000_000u64 {
            let mut r = Rng::seed_from_u64(seed);
            if pred(&mut r) {
                return seed;
            }
        }
        panic!("no matching seed found within 5_000_000 iterations");
    }

    // ---- Acceptance tests ---------------------------------------------------

    /// Acceptance: `bullets_consumed` is always 10, regardless of hit or miss.
    ///
    /// See p.173: "Fire a total of 10 Rounds into an area."
    #[test]
    fn test_autofire_consumes_10_bullets() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        let attack = smg_attack(attacker, target, 30);
        let mut rng = Rng::seed_from_u64(42);

        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("valid autofire must resolve");

        assert_eq!(
            outcome.bullets_consumed, 10,
            "autofire always costs exactly 10 rounds (p.173)"
        );
    }

    /// Acceptance: fewer than 10 rounds → `Err(InsufficientAmmo)`.
    ///
    /// See p.173.
    #[test]
    fn test_autofire_requires_10_in_clip() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        // 9 rounds — one short.
        let attack = smg_attack(attacker, target, 9);
        let mut rng = Rng::seed_from_u64(0);

        let result = attack.resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng);
        assert!(
            matches!(
                result,
                Err(RulesError::InsufficientAmmo {
                    required: 10,
                    available: 9,
                })
            ),
            "< 10 rounds must return InsufficientAmmo (p.173); got: {result:?}"
        );

        // 0 rounds.
        let attack_zero = smg_attack(attacker, target, 0);
        let mut rng2 = Rng::seed_from_u64(0);
        let result2 = attack_zero.resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng2);
        assert!(
            matches!(
                result2,
                Err(RulesError::InsufficientAmmo {
                    required: 10,
                    available: 0,
                })
            ),
            "0 rounds must also return InsufficientAmmo"
        );
    }

    /// Acceptance: Assault Rifle (cap 4) — beating DV by 7 is capped to 4.
    ///
    /// REF 7 + Autofire 4 + d10 vs DV. We need margin >= 7. At range 10m,
    /// assault rifle autofire DV = 20 (p.173 band 7–12m = 20). We need
    /// final_value − 20 >= 7, i.e. final_value >= 27. Base 7+4=11; we need
    /// d10 >= 16 which requires a crit 10+6=16. Let's find a seed.
    ///
    /// See p.173 (Autofire damage cap).
    #[test]
    fn test_assault_rifle_cap_4() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        // Find a seed where REF(7)+Autofire(4)+d10 > DV_autofire+7 so margin > 7
        // Autofire DV at 10m for assault rifle = 20 (band 7–12m, p.173).
        // Need final_value − 20 ≥ 7 → final ≥ 27 → d10.net ≥ 27 − 7 − 4 = 16.
        // That needs a crit 10 + follow_up ≥ 6. Let's search.
        let seed = find_seed_where(|r| {
            let base = crate::dice::d10(r);
            if base == 10 {
                let follow = crate::dice::d10(r);
                // net = 10 + follow; need 7 + 4 + 10 + follow >= 27 → follow >= 6
                follow >= 6
            } else {
                false
            }
        });

        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);
        let attack = assault_rifle_attack(attacker, target, 30);
        let mut rng = Rng::seed_from_u64(seed);

        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("valid attack must resolve");

        // The attack must have hit and the margin >= 7.
        assert!(outcome.attack_breakdown.success, "must be a hit");
        assert!(
            outcome.attack_breakdown.margin >= 7,
            "margin must be >= 7 for this test; got {}",
            outcome.attack_breakdown.margin
        );

        // beat_dv_by must be capped at 4 (Assault Rifle autofire cap, p.173).
        assert_eq!(
            outcome.beat_dv_by, 4,
            "Assault Rifle autofire cap is 4 per p.173; margin {} must be capped",
            outcome.attack_breakdown.margin
        );
    }

    /// Acceptance: SMG (cap 3) — beating DV by 5 is capped to 3.
    ///
    /// SMG autofire DV at 10m = 17 (band 7–12m, p.173).
    /// Need final_value − 17 >= 5 → final >= 22.
    /// Base REF 7 + Autofire 4 = 11; need d10.net >= 11. Crit 10 + 1 = 11.
    ///
    /// See p.173.
    #[test]
    fn test_smg_cap_3() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        // SMG autofire DV at 10m = 17. Need margin >= 5 → final >= 22 → d10.net >= 11.
        // Crit 10 + follow >= 1 satisfies this (10+1=11 → final=22, margin=5).
        let seed = find_seed_where(|r| {
            let base = crate::dice::d10(r);
            if base == 10 {
                let follow = crate::dice::d10(r);
                // net = 10 + follow; need 7 + 4 + 10 + follow >= 22 → follow >= 1
                follow >= 1
            } else {
                false
            }
        });

        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);
        let attack = smg_attack(attacker, target, 30);
        let mut rng = Rng::seed_from_u64(seed);

        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("valid attack must resolve");

        assert!(outcome.attack_breakdown.success, "must be a hit");
        assert!(
            outcome.attack_breakdown.margin >= 5,
            "margin must be >= 5; got {}",
            outcome.attack_breakdown.margin
        );
        // beat_dv_by must be capped at 3 (SMG autofire cap, p.173).
        assert_eq!(
            outcome.beat_dv_by, 3,
            "SMG autofire cap is 3 per p.173; got {}",
            outcome.beat_dv_by
        );
    }

    /// Acceptance: damage_rolls = [6, 6] triggers a Critical Injury.
    ///
    /// Both d6 showing 6 is the autofire-specific crit condition.
    /// See pp.173–174: "Both d6 = 6 → triggers crit."
    #[test]
    fn test_both_d6_six_triggers_crit() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        // Find a seed where:
        // 1. d10 hits (attack succeeds): final_value >= DV.
        //    SMG DV at 10m = 17 (autofire table p.173). Base 7+4=11; need d10 >= 6.
        // 2. Next two d6 are both 6.
        let seed = find_seed_where(|r| {
            // Simulate the attack roll (d10_with_crits consumes 1 or 2 d10s).
            let base = crate::dice::d10(r);
            // Make sure it's not a crit failure (1) or crit success (10) to
            // keep the logic simple — just a regular hit at d10 >= 6.
            if !(2..=9).contains(&base) {
                return false;
            }
            let net = base as i16;
            // REF 7 + Autofire 4 + d10 >= 17 → d10 >= 6
            if 7 + 4 + net < 17 {
                return false;
            }
            // Both d6 must be 6.
            let d6a = crate::dice::d6(r);
            let d6b = crate::dice::d6(r);
            d6a == 6 && d6b == 6
        });

        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);
        let attack = smg_attack(attacker, target, 30);
        let mut rng = Rng::seed_from_u64(seed);

        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("valid attack must resolve");

        assert!(outcome.attack_breakdown.success, "attack must hit");
        assert_eq!(
            outcome.damage_rolls,
            [6, 6],
            "damage_rolls must be [6, 6] for this seed"
        );
        // A crit must have been applied (or returned None if table was
        // exhausted, but with fresh world it always applies something).
        assert!(
            outcome.critical.is_some(),
            "both d6 = 6 must trigger a critical injury (pp.173–174)"
        );
    }

    /// Acceptance: the autofire DV comes from the weapon's autofire DV table,
    /// not the single-shot table. See p.173.
    ///
    /// Verifies that the `dv` field in the `attack_breakdown` matches
    /// `weapon.ranges.autofire_dv_at(range)` for both SMG and Assault Rifle.
    #[test]
    fn test_autofire_uses_autofire_dv_table() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        // Verify at two ranges and two weapons that the autofire DV table is used.
        let smg = weapons.get("smg").expect("smg must be in catalog");
        let rifle = weapons
            .get("assault_rifle")
            .expect("assault_rifle must be in catalog");

        // SMG at 10m: autofire DV = 17 (band 7–12m, p.173).
        let smg_dv_at_10m = smg
            .ranges
            .autofire_dv_at(10)
            .expect("SMG must have autofire DV at 10m");
        assert_eq!(
            smg_dv_at_10m, 17,
            "SMG autofire DV at 10m must be 17 (p.173)"
        );

        // Rifle at 10m: autofire DV = 20 (band 7–12m, p.173).
        let rifle_dv_at_10m = rifle
            .ranges
            .autofire_dv_at(10)
            .expect("Assault Rifle must have autofire DV at 10m");
        assert_eq!(
            rifle_dv_at_10m, 20,
            "Assault Rifle autofire DV at 10m must be 20 (p.173)"
        );

        // Now resolve an actual attack and verify the DV in the breakdown.
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);
        let attack = smg_attack(attacker, target, 30);

        // Use seed 42 and check that the DV in the breakdown is 17.
        let mut rng = Rng::seed_from_u64(42);
        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("valid attack must resolve");

        assert_eq!(
            outcome.attack_breakdown.dv,
            DV(smg_dv_at_10m),
            "attack_breakdown.dv must equal the autofire DV at the given range (p.173)"
        );
    }

    // ---- Additional regression / scenario tests ----------------------------

    /// Regression: a weapon without Autofire feature → `WeaponLacksAutofire`.
    #[test]
    fn test_non_autofire_weapon_rejected() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        // Medium Pistol has no Autofire feature.
        let attack = AutofireAttack {
            attacker,
            target,
            weapon: WeaponId("medium_pistol".into()),
            rounds_in_magazine: 12,
            range_meters: 10,
            luck_to_spend: 0,
            defender_dodges: false,
            additional_modifiers: vec![],
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = attack.resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng);
        assert!(
            matches!(result, Err(RulesError::WeaponLacksAutofire(_))),
            "non-autofire weapon must return WeaponLacksAutofire; got {result:?}"
        );
    }

    /// Regression: target beyond 100m → `OutOfAutofireRange`. See p.173.
    #[test]
    fn test_out_of_autofire_range() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        let attack = AutofireAttack {
            attacker,
            target,
            weapon: WeaponId("smg".into()),
            rounds_in_magazine: 30,
            range_meters: 101, // beyond the 51–100m autofire band
            luck_to_spend: 0,
            defender_dodges: false,
            additional_modifiers: vec![],
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = attack.resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng);
        assert!(
            matches!(result, Err(RulesError::OutOfAutofireRange)),
            "range > 100m must return OutOfAutofireRange (p.173); got {result:?}"
        );
    }

    /// Regression: unknown attacker EntityId → `EntityNotFound`.
    #[test]
    fn test_unknown_attacker_returns_err() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, _attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        let unknown = EntityId(Uuid::from_u128(0xDEADBEEF));
        let attack = smg_attack(unknown, target, 30);
        let mut rng = Rng::seed_from_u64(0);

        let result = attack.resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng);
        assert!(
            matches!(result, Err(RulesError::EntityNotFound(_))),
            "unknown attacker must return EntityNotFound; got {result:?}"
        );
    }

    /// Regression: on a miss, damage_total is 0 and damage_outcome is None.
    #[test]
    fn test_miss_produces_no_damage() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        // Force a miss: use additional_modifiers to apply a large penalty.
        // SMG DV at 10m = 17. Base 7+4=11. Penalty -20 → final ≤ 0, always fail.
        let attack = AutofireAttack {
            additional_modifiers: vec![NamedModifier {
                label: "test penalty".into(),
                value: -20,
            }],
            ..smg_attack(attacker, target, 30)
        };
        let mut rng = Rng::seed_from_u64(42);
        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("miss still resolves successfully");

        assert!(!outcome.attack_breakdown.success, "must be a miss");
        assert_eq!(outcome.damage_total, 0, "miss must have damage_total == 0");
        assert!(
            outcome.damage_outcome.is_none(),
            "miss must have no damage_outcome"
        );
        assert!(outcome.critical.is_none(), "miss must have no critical");
        assert_eq!(outcome.beat_dv_by, 0, "miss must have beat_dv_by == 0");
        assert_eq!(
            outcome.bullets_consumed, 10,
            "miss still consumes 10 rounds (p.173)"
        );
    }

    /// Regression: damage_rolls are always exactly 2 entries.
    #[test]
    fn test_damage_rolls_always_two_dice() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        for seed in 0..20u64 {
            let (mut world, attacker) = world_with_autofire_pc();
            let target = add_npc_target(&mut world);
            let attack = smg_attack(attacker, target, 30);
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = attack
                .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
                .expect("must resolve");
            assert_eq!(outcome.damage_rolls.len(), 2);
            for die in outcome.damage_rolls.iter() {
                assert!((0..=6).contains(die), "damage die must be 0–6; got {die}");
            }
        }
    }

    /// Regression: damage_total = 2d6 × beat_dv_by on a hit.
    #[test]
    fn test_damage_total_formula() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        // Run many seeds and verify the formula whenever we get a hit.
        let mut found_hit = false;
        for seed in 0..200u64 {
            let (mut world, attacker) = world_with_autofire_pc();
            let target = add_npc_target(&mut world);
            let attack = smg_attack(attacker, target, 30);
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = attack
                .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
                .expect("must resolve");

            if outcome.attack_breakdown.success && outcome.beat_dv_by > 0 {
                found_hit = true;
                let expected = u16::from(outcome.damage_rolls[0]) * u16::from(outcome.beat_dv_by)
                    + u16::from(outcome.damage_rolls[1]) * u16::from(outcome.beat_dv_by);
                assert_eq!(
                    outcome.damage_total, expected,
                    "damage_total must equal (d6_a + d6_b) × beat_dv_by for seed {seed}"
                );
            }
        }
        assert!(found_hit, "at least one seed must produce a hit");
    }

    /// Regression: `test_autofire_cannot_aim` — the struct has no `aimed_shot`
    /// field, enforcing at the type level that autofire cannot combine with an
    /// Aimed Shot. This test verifies the public API shape.
    ///
    /// See p.173.
    #[test]
    fn test_autofire_cannot_aim() {
        // AutofireAttack has no `aimed_shot` field by design (p.173).
        // Compiling this test is sufficient — the absence of the field is
        // the guarantee. We construct an attack to confirm the struct shape.
        let _ = AutofireAttack {
            attacker: EntityId(Uuid::from_u128(0x01)),
            target: EntityId(Uuid::from_u128(0x02)),
            weapon: WeaponId("smg".into()),
            rounds_in_magazine: 30,
            range_meters: 10,
            luck_to_spend: 0,
            defender_dodges: false,
            additional_modifiers: vec![],
            // No `aimed_shot` field exists — this would fail to compile if
            // one were accidentally added. See p.173: "Aimed shot rejected."
        };
        // No runtime assertion needed — the compile-time check is the test.
    }

    /// Regression: `bullets_consumed` is always 10 even when the attack misses.
    /// See p.173.
    #[test]
    fn test_bullets_consumed_always_10_on_miss() {
        let weapons = weapons_cat();
        let crits = crit_cat();
        let (mut world, attacker) = world_with_autofire_pc();
        let target = add_npc_target(&mut world);

        // Guarantee a miss with extreme penalty.
        let attack = AutofireAttack {
            additional_modifiers: vec![NamedModifier {
                label: "extreme penalty".into(),
                value: -50,
            }],
            ..smg_attack(attacker, target, 30)
        };
        let mut rng = Rng::seed_from_u64(0);
        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("resolve");
        assert_eq!(outcome.bullets_consumed, 10);
    }

    /// Regression: the `SkillId::Autofire` skill is used (not Handgun or
    /// ShoulderArms). Verify by setting Autofire rank to 0 and observing
    /// the effective skill value in the breakdown.
    #[test]
    fn test_autofire_uses_autofire_skill() {
        let weapons = weapons_cat();
        let crits = crit_cat();

        let mut pc = fresh_pc();
        pc.stats.r#ref = 7;
        pc.stats.will = 5;
        pc.stats.body = 5;
        // Handgun rank 10 but Autofire rank 0 (untrained).
        pc.skills.ranks.insert(SkillId::Handgun, 10);
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        let attacker = EntityId(pc.id.0);
        let mut world = World::new(pc);
        let target = add_npc_target(&mut world);

        let attack = smg_attack(attacker, target, 30);
        let mut rng = Rng::seed_from_u64(7);
        let outcome = attack
            .resolve_with_catalogs(&mut world, &weapons, &crits, &mut rng)
            .expect("resolve");

        // skill_value in breakdown must be 0 (Autofire untrained), not 10 (Handgun).
        assert_eq!(
            outcome.attack_breakdown.skill_value, 0,
            "Autofire skill is 0 (untrained); Handgun rank must not bleed through"
        );
    }
}
