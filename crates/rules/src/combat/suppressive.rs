//! Suppressive fire — costs 10 bullets, forces a concentration check from
//! every unprotected enemy in the area.
//!
//! **Rulebook:** p.174.
//!
//! ## Rules summary (p.174)
//!
//! When a character uses Suppressive Fire it costs an Action and 10 Bullets.
//! Every entity **on foot**, within **25 m/yds**, **out of cover**, and **in
//! line of sight** must roll:
//!
//! > WILL + Concentration + 1d10
//! > vs.
//! > Attacker's REF + Autofire Skill + 1d10
//!
//! Entities that fail must use their next Move Action (and Run Action if
//! needed) to get into cover or as close to cover as possible.
//!
//! ## WP-310 spec deviation
//!
//! The WP-310 spec mentions "DV15" for the concentration check.  The rulebook
//! on p.174 describes an **opposed check** (target rolls WILL + Concentration
//! vs. the attacker's REF + Autofire roll), not a fixed DV.  This
//! implementation follows RAW.  The `SuppressiveFire` struct retains
//! `additional_modifiers` (for GM/Beat adjustments on the attacker side) as
//! specified in the public API; a matching `Vec<NamedModifier>` on the
//! defender side is available if needed but the WP-310 spec did not include
//! it, so none is exposed here.

use crate::catalog::skills::SkillId;
use crate::checks::skill_check::NamedModifier;
use crate::combat::grid::{Grid, LosResult};
use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::resolution::CheckBreakdown;
use crate::rng::Rng;
use crate::types::{EntityId, Stat, DV};
use crate::world::World;
use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────────

/// Number of bullets consumed by every Suppressive Fire action. See p.174.
pub const SUPPRESSIVE_FIRE_BULLET_COST: u8 = 10;

/// Maximum range of Suppressive Fire in metres. See p.174.
pub const SUPPRESSIVE_FIRE_MAX_METERS: u16 = 25;

// ── SuppressiveArea ──────────────────────────────────────────────────────────

/// The grid area swept by a Suppressive Fire action.
///
/// All entities within [`SUPPRESSIVE_FIRE_MAX_METERS`] metres (25 m/yds,
/// p.174) of `center`, in line of sight, and **not** behind live cover are
/// affected.
///
/// See p.174.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuppressiveArea {
    /// Grid square that is the focal point of the suppressive burst.
    pub center: (u16, u16),
    /// Maximum range in metres; defaults to 25 m per p.174.
    pub max_meters: u16,
}

// ── SuppressiveFire ──────────────────────────────────────────────────────────

/// A Suppressive Fire action ready to be resolved against the game world.
///
/// Build this struct, then call [`resolve`] to consume 10 bullets and run
/// the per-target concentration checks.
///
/// **Rulebook:** p.174.
pub struct SuppressiveFire {
    /// The entity performing Suppressive Fire.
    pub attacker: EntityId,
    /// The area swept by the suppressive burst.
    pub area: SuppressiveArea,
    /// GM/Beat-applied situational modifiers on the attacker's side of the
    /// opposed check (e.g. wounded, low visibility). Persistent character
    /// modifiers (cyberware, wound state) are pulled from the effect stack
    /// automatically during resolution.
    pub additional_modifiers: Vec<NamedModifier>,
}

// ── SuppressiveOutcome ───────────────────────────────────────────────────────

/// Structured outcome of a resolved [`SuppressiveFire`] action.
///
/// See p.174.
#[derive(Debug)]
pub struct SuppressiveOutcome {
    /// Always [`SUPPRESSIVE_FIRE_BULLET_COST`] (10). See p.174.
    pub bullets_consumed: u8,
    /// Entities that failed the WILL + Concentration check and must now use
    /// their next Move Action (and Run Action if needed) to seek cover.
    /// See p.174: "Anyone that fails must use their next Move Action to get
    /// into cover."
    pub forced_to_cover: Vec<EntityId>,
    /// Entities that succeeded the WILL + Concentration check and may act
    /// freely.
    pub resisted: Vec<EntityId>,
    /// Per-target check breakdowns — one entry per affected entity; in the
    /// same order as [`forced_to_cover`] + [`resisted`] combined.
    pub per_target: Vec<(EntityId, TargetCheckResult)>,
}

/// Per-target result of the opposed concentration check.
///
/// See p.174.
#[derive(Debug)]
pub struct TargetCheckResult {
    /// The target's WILL + Concentration + d10 breakdown.
    pub target_breakdown: CheckBreakdown,
    /// The attacker's REF + Autofire + d10 breakdown (same roll for all
    /// targets — rolled once and reused; each target's breakdown records
    /// the same attacker final value as its DV).
    pub attacker_breakdown: CheckBreakdown,
    /// `true` if the target succeeded (resisted the suppression).
    pub target_resisted: bool,
}

// ── resolve ──────────────────────────────────────────────────────────────────

impl SuppressiveFire {
    /// Resolve this Suppressive Fire action against `world`, consuming dice
    /// from `rng`.
    ///
    /// ## Resolution order (fixed for determinism)
    ///
    /// 1. Look up attacker; validate they exist.
    /// 2. Roll attacker's REF + Autofire + 1d10 (once, applied to all
    ///    targets). Additional modifiers are applied on the attacker side.
    /// 3. For each entity on the grid within `area.max_meters` metres:
    ///    a. Skip if the entity is the attacker.
    ///    b. Skip if LOS is blocked (`LosResult::Blocked`).
    ///    c. Skip if the entity is behind live cover (`LosResult::ThroughCover`
    ///    with `current_hp > 0`). See p.174: "out of cover".
    ///    d. Roll the entity's WILL + Concentration + 1d10.
    ///    e. Compare: attacker's final > entity's final → entity fails
    ///    (forced to cover); entity's final >= attacker's final → entity
    ///    resists (ties favour the defender, p.129).
    /// 4. Return [`SuppressiveOutcome`].
    ///
    /// ## Errors
    ///
    /// Returns [`RulesError::EntityNotFound`] if the attacker is not in
    /// `world`.
    ///
    /// **Rulebook:** p.174.
    pub fn resolve(
        &self,
        world: &mut World,
        grid: &Grid,
        rng: &mut Rng,
    ) -> Result<SuppressiveOutcome, RulesError> {
        // 1. Validate attacker exists. See p.174.
        let attacker_char = world
            .entity(self.attacker)
            .ok_or(RulesError::EntityNotFound(self.attacker))?;

        // 2. Roll the attacker's REF + Autofire + 1d10 once. See p.174.
        let att_ref = attacker_char.current_stat(Stat::Ref);
        let att_autofire = attacker_char.current_skill(&SkillId::Autofire);
        let att_aap = i16::from(attacker_char.all_actions_penalty());
        let extra: i16 = self
            .additional_modifiers
            .iter()
            .map(|m| i16::from(m.value))
            .sum();
        let att_modifier_total = att_aap + extra;
        let att_d10 = d10_with_crits(rng);
        let att_final = att_ref + att_autofire + att_modifier_total + att_d10.net;

        // Locate the attacker on the grid to determine LOS origin.
        let attacker_pos = grid.position_of(self.attacker);

        // 3. Collect affected entities and run per-target checks.
        let mut forced_to_cover: Vec<EntityId> = Vec::new();
        let mut resisted: Vec<EntityId> = Vec::new();
        let mut per_target: Vec<(EntityId, TargetCheckResult)> = Vec::new();

        // Collect candidate positions within range (sorted for determinism).
        let mut candidates: Vec<(u16, u16)> = grid
            .occupants
            .keys()
            .copied()
            .filter(|&pos| {
                // Skip the attacker's own square. See p.174.
                if let Some(ap) = attacker_pos {
                    if pos == ap {
                        return false;
                    }
                }
                // Distance filter: within max_meters. See p.174.
                if let Some(ap) = attacker_pos {
                    grid.distance_meters(ap, pos) <= self.area.max_meters
                } else {
                    // Attacker not on grid; use center for distance.
                    grid.distance_meters(self.area.center, pos) <= self.area.max_meters
                }
            })
            .collect();

        // Sort for reproducible iteration order (HashMap is unordered).
        candidates.sort_unstable();

        for pos in candidates {
            let entity = match grid.occupants.get(&pos) {
                Some(&e) => e,
                None => continue,
            };

            // Skip the attacker entity regardless of position. See p.174.
            if entity == self.attacker {
                continue;
            }

            // 3b-c. LOS and cover check. See p.174.
            let los_origin = attacker_pos.unwrap_or(self.area.center);
            let los_result = grid.line_of_sight(los_origin, pos);

            match &los_result {
                // Completely blocked — not affected. See p.174.
                LosResult::Blocked => continue,
                // Behind live cover — not affected. See p.174.
                LosResult::ThroughCover(cover) if cover.current_hp > 0 => continue,
                // Clear LOS or dead cover (cover destroyed) — affected.
                _ => {}
            }

            // 3d. Roll entity's WILL + Concentration + 1d10. See p.174.
            let target_char = match world.entity(entity) {
                Some(c) => c,
                // Entity on grid but not in world — skip gracefully.
                None => continue,
            };

            let tgt_will = target_char.current_stat(Stat::Will);
            let tgt_conc = target_char.current_skill(&SkillId::Concentration);
            let tgt_aap = i16::from(target_char.all_actions_penalty());
            let tgt_modifier_total = tgt_aap;
            let tgt_d10 = d10_with_crits(rng);
            let tgt_final = tgt_will + tgt_conc + tgt_modifier_total + tgt_d10.net;

            // 3e. Compare. Ties favour the target (defender). See pp.129, 174.
            // attacker_wins iff att_final strictly > tgt_final.
            let target_resisted = tgt_final >= att_final;

            // Build breakdowns for replay / reporting.
            // The attacker breakdown DV is the target's final (saturated).
            // The target breakdown DV is the attacker's final (saturated).
            let att_bd = CheckBreakdown::new(
                att_ref,
                att_autofire,
                att_modifier_total,
                0,
                att_d10.clone(),
                DV(saturate_dv(tgt_final)),
            );
            let tgt_bd = CheckBreakdown::new(
                tgt_will,
                tgt_conc,
                tgt_modifier_total,
                0,
                tgt_d10,
                DV(saturate_dv(att_final)),
            );

            let result = TargetCheckResult {
                target_breakdown: tgt_bd,
                attacker_breakdown: att_bd,
                target_resisted,
            };

            if target_resisted {
                resisted.push(entity);
            } else {
                forced_to_cover.push(entity);
            }
            per_target.push((entity, result));
        }

        Ok(SuppressiveOutcome {
            bullets_consumed: SUPPRESSIVE_FIRE_BULLET_COST,
            forced_to_cover,
            resisted,
            per_target,
        })
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Saturate an `i16` final-value into the `u8` shape that [`DV`] requires.
/// Negative values clamp to 0; values above `u8::MAX` clamp to `u8::MAX`.
/// See also `crate::checks::skill_check::saturate_to_u8_dv`.
fn saturate_dv(v: i16) -> u8 {
    v.clamp(0, i16::from(u8::MAX)) as u8
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::skills::SkillId;
    use crate::character::Character;
    use crate::combat::grid::{CoverInstance, Grid, TileKind};
    use crate::types::{CharacterId, NpcId};
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;
    use uuid::Uuid;

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn eid(n: u128) -> EntityId {
        EntityId(Uuid::from_u128(n))
    }

    fn cid(n: u128) -> CharacterId {
        CharacterId(Uuid::from_u128(n))
    }

    fn npc_id(n: u128) -> NpcId {
        NpcId(Uuid::from_u128(n))
    }

    /// Build a minimal PC/NPC with controlled stats.
    fn make_char(id: u128, ref_stat: u8, will: u8, autofire: u8, concentration: u8) -> Character {
        let mut c = fresh_pc();
        c.id = cid(id);
        c.stats.r#ref = ref_stat;
        c.stats.will = will;
        if autofire > 0 {
            c.skills.ranks.insert(SkillId::Autofire, autofire);
        }
        if concentration > 0 {
            c.skills.ranks.insert(SkillId::Concentration, concentration);
        }
        c
    }

    /// Build a `World` with one PC (attacker) and register an NPC target.
    fn setup_world_and_grid(
        attacker_id: u128,
        target_ids: &[(u128, u8, u8, u8, u8)], // (id, ref, will, autofire, conc)
    ) -> (World, Grid) {
        let attacker = make_char(attacker_id, 8, 5, 6, 0);
        let mut world = World::new(attacker);

        let mut grid = Grid::new(20, 20);
        let attacker_eid = eid(attacker_id);
        grid.place(attacker_eid, (0, 0));

        for (i, &(npc_uuid, ref_s, will, autofire, conc)) in target_ids.iter().enumerate() {
            let npc = make_char(npc_uuid, ref_s, will, autofire, conc);
            world.npcs.insert(npc_id(npc_uuid), npc);
            // Place targets within 10m (5 squares at 2m/sq).
            grid.place(eid(npc_uuid), (2 + i as u16, 0));
        }

        (world, grid)
    }

    // ── Acceptance tests ──────────────────────────────────────────────────────

    /// `test_suppressive_consumes_10` — outcome always reports 10 bullets used.
    ///
    /// See p.174: "it costs an Action and 10 Bullets".
    #[test]
    fn test_suppressive_consumes_10() {
        let (mut world, grid) = setup_world_and_grid(1, &[]);
        let mut rng = Rng::seed_from_u64(0);

        let action = SuppressiveFire {
            attacker: eid(1),
            area: SuppressiveArea {
                center: (0, 0),
                max_meters: SUPPRESSIVE_FIRE_MAX_METERS,
            },
            additional_modifiers: vec![],
        };

        let outcome = action
            .resolve(&mut world, &grid, &mut rng)
            .expect("must resolve without error");

        assert_eq!(
            outcome.bullets_consumed, SUPPRESSIVE_FIRE_BULLET_COST,
            "Suppressive Fire must always consume exactly 10 bullets (p.174)"
        );
        assert_eq!(outcome.bullets_consumed, 10);
    }

    /// `test_only_in_los_no_cover` — targets behind cover or blocked by a wall
    /// are not affected.
    ///
    /// See p.174: "out of cover, and in your line of sight must roll".
    #[test]
    fn test_only_in_los_no_cover() {
        // Attacker at (0,0).
        // Target A at (3,0): open, in LOS → affected.
        // Target B at (7,0): behind live cover at (6,0) → not affected.
        // Target C at (10,0): behind wall at x=9 → not affected.
        let attacker_id = 0x_A0;
        let target_a = 0x_A1;
        let target_b = 0x_A2;
        let target_c = 0x_A3;

        let attacker_char = make_char(attacker_id, 8, 5, 6, 0);
        let mut world = World::new(attacker_char);
        world
            .npcs
            .insert(npc_id(target_a), make_char(target_a, 5, 5, 0, 4));
        world
            .npcs
            .insert(npc_id(target_b), make_char(target_b, 5, 5, 0, 4));
        world
            .npcs
            .insert(npc_id(target_c), make_char(target_c, 5, 5, 0, 4));

        let mut grid = Grid::new(20, 20);
        grid.place(eid(attacker_id), (0, 0));
        grid.place(eid(target_a), (3, 0));
        grid.place(eid(target_b), (7, 0));
        grid.place(eid(target_c), (10, 0));

        // Live cover at (6,0) protects target B.
        grid.cover_objects.insert(
            (6, 0),
            CoverInstance {
                material: "concrete_barricade".to_string(),
                current_hp: 25,
                max_hp: 25,
            },
        );

        // Wall at x=9 blocks LOS to target C.
        for y in 0..20 {
            grid.set_tile((9, y), TileKind::Wall);
        }

        let mut rng = Rng::seed_from_u64(42);
        let action = SuppressiveFire {
            attacker: eid(attacker_id),
            area: SuppressiveArea {
                center: (0, 0),
                max_meters: SUPPRESSIVE_FIRE_MAX_METERS,
            },
            additional_modifiers: vec![],
        };

        let outcome = action
            .resolve(&mut world, &grid, &mut rng)
            .expect("must resolve");

        // Gather all affected entity ids from per_target.
        let affected: Vec<EntityId> = outcome.per_target.iter().map(|(e, _)| *e).collect();

        assert!(
            affected.contains(&eid(target_a)),
            "target A (open, in LOS) must be affected by suppression (p.174)"
        );
        assert!(
            !affected.contains(&eid(target_b)),
            "target B (behind live cover) must NOT be affected by suppression (p.174)"
        );
        assert!(
            !affected.contains(&eid(target_c)),
            "target C (behind wall, LOS blocked) must NOT be affected by suppression (p.174)"
        );
    }

    /// `test_failed_concentration_must_seek_cover` — entities that lose the
    /// opposed check appear in `forced_to_cover`.
    ///
    /// See p.174: "Anyone that fails must use their next Move Action to get
    /// into cover."
    #[test]
    fn test_failed_concentration_must_seek_cover() {
        // Give the attacker very high REF + Autofire (8+8=16) so their
        // suppression roll total will be nearly impossible to beat.
        // Give the target very low WILL + Concentration (1+0=1) so they fail.
        let attacker_id = 0x_B0;
        let weak_target = 0x_B1;

        let attacker_char = make_char(attacker_id, 8, 1, 8, 0); // REF 8, Autofire 8
        let mut world = World::new(attacker_char);
        // WILL 1, Concentration 0: almost certainly fails the opposed check.
        world
            .npcs
            .insert(npc_id(weak_target), make_char(weak_target, 1, 1, 0, 0));

        let mut grid = Grid::new(20, 20);
        grid.place(eid(attacker_id), (0, 0));
        // Place target within range (6m = 3 squares).
        grid.place(eid(weak_target), (3, 0));

        // Use many seeds to find one where the target fails.
        // With REF8+Autofire8 vs WILL1+Conc0, the attacker final is
        // typically 16 + d10 while the target final is 1 + d10. The
        // attacker wins on almost any non-crit-fail roll.
        // Seed 0 is almost certain to produce a fail; assert it does or
        // try a handful.
        let mut found_failure = false;
        for seed in 0u64..100 {
            let mut rng = Rng::seed_from_u64(seed);
            let action = SuppressiveFire {
                attacker: eid(attacker_id),
                area: SuppressiveArea {
                    center: (0, 0),
                    max_meters: SUPPRESSIVE_FIRE_MAX_METERS,
                },
                additional_modifiers: vec![],
            };
            let outcome = action
                .resolve(&mut world, &grid, &mut rng)
                .expect("must resolve");

            if outcome.forced_to_cover.contains(&eid(weak_target)) {
                found_failure = true;
                // Verify consistency: forced_to_cover and resisted are disjoint.
                assert!(
                    !outcome.resisted.contains(&eid(weak_target)),
                    "entity cannot be both forced_to_cover and resisted"
                );
                // The per_target entry must agree with forced_to_cover.
                let entry = outcome
                    .per_target
                    .iter()
                    .find(|(e, _)| *e == eid(weak_target));
                assert!(
                    entry.is_some(),
                    "per_target must have an entry for the target"
                );
                let (_, check) = entry.unwrap();
                assert!(
                    !check.target_resisted,
                    "target_resisted must be false for entities in forced_to_cover (p.174)"
                );
                break;
            }
        }
        assert!(
            found_failure,
            "weak target (WILL1+Conc0) must fail suppression check in at least one of 100 seeds"
        );
    }
}
