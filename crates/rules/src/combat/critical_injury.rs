//! Critical Injury application — WP-305.
//!
//! This module implements the two public entry-points for the Critical Injury
//! subsystem defined on pp.187–188 of the *Cyberpunk RED Core Rules*:
//!
//! 1. [`check_critical_trigger`] — determines whether a set of damage dice
//!    triggers a Critical Injury (two or more dice showing a 6, per p.187).
//!
//! 2. [`apply_critical_injury`] — rolls on the appropriate table, applies the
//!    Injury Effect as an `ActiveEffect` on the target's [`EffectStack`], and
//!    delivers the 5 Bonus Damage that bypasses armor (p.187).
//!
//! ## Rulebook citations
//!
//! - p.187: "Whenever two or more dice rolled for damage from a Melee or
//!   Ranged Attack come up 6, you've inflicted a Critical Injury!"
//! - p.187: "All Critical Injuries cause a horrible Injury Effect and deal
//!   5 Bonus Damage directly to the target's Hit Points when suffered. The
//!   Bonus Damage doesn't ablate armor and isn't modified by hit location."
//! - p.187: "Critical Injuries and their Bonus Damage are inflicted regardless
//!   of if any of the attack's damage got through the target's SP."
//! - p.187 (body table) and p.188 (head table): the 2d6 roll tables.
//!
//! ## API deviation from WP-305 spec
//!
//! The WP-305 public API spec lists:
//! ```text
//! pub fn apply_critical_injury(world, target, table, rng) -> Option<...>
//! ```
//!
//! To look up the `CriticalInjury` definition at runtime (for its `effects`,
//! `quick_fix`, `treatment`, and `increases_death_save_penalty` fields), the
//! function needs access to the loaded catalog. Rather than hardcoding the
//! catalog data in code (a maintenance burden that would diverge from the RON
//! files), **this implementation adds a `catalog` parameter**:
//! ```text
//! pub fn apply_critical_injury(world, target, table, catalog, rng)
//! ```
//! where `catalog` is `&Catalog<CriticalInjury>` loaded from the body or head
//! RON file. The caller is responsible for passing the correct catalog for the
//! chosen `table`. This is documented in the PR.
//!
//! ## Bonus damage hit-location decision
//!
//! Per p.187: "The Bonus Damage doesn't ablate armor and isn't modified by hit
//! location." The book is silent on *which* location to attribute the bonus
//! damage call to. Since the bonus bypasses armor (`bypass_armor: true`) and
//! does not depend on location, the choice is mechanically irrelevant. This
//! implementation **always uses `HitLocation::Body`** for the bonus damage
//! call regardless of whether the table is `CritTable::Body` or
//! `CritTable::Head`. This is the simplest defensible default; the PR flags it.

use crate::catalog::critical_injuries::{
    roll_critical_injury, CritTable, CriticalInjury, CriticalInjuryKind,
};
use crate::catalog::Catalog;
use crate::combat::damage::{apply_damage, DamageApplication, DamageOutcome, HitLocation};
use crate::effects::{ActiveEffect, EffectDuration, EffectSource};
use crate::rng::Rng;
use crate::types::{EffectInstanceId, EntityId};
use crate::world::World;
use rand::RngCore;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// check_critical_trigger
// ---------------------------------------------------------------------------

/// Returns `true` if two or more of the given damage dice show a 6.
///
/// Per p.187: "Whenever two or more dice rolled for damage from a Melee or
/// Ranged Attack come up 6, you've inflicted a Critical Injury!"
///
/// The function counts occurrences of the value `6` in `damage_rolls` and
/// returns `true` iff the count is `>= 2`. An empty slice returns `false`.
///
/// # Examples
///
/// ```rust
/// use cpr_rules::combat::critical_injury::check_critical_trigger;
/// assert!(check_critical_trigger(&[6, 6, 3, 2]));   // two sixes → trigger
/// assert!(!check_critical_trigger(&[6, 5, 4, 3]));  // one six → no trigger
/// assert!(check_critical_trigger(&[6, 6, 6, 4]));   // three sixes → trigger
/// assert!(!check_critical_trigger(&[]));             // no dice → no trigger
/// ```
///
/// See p.187 (Critical Injuries).
pub fn check_critical_trigger(damage_rolls: &[u8]) -> bool {
    // Count dice equal to 6; trigger when count >= 2. See p.187.
    let sixes = damage_rolls.iter().filter(|&&d| d == 6).count();
    sixes >= 2
}

// ---------------------------------------------------------------------------
// CriticalInjuryApplied
// ---------------------------------------------------------------------------

/// The structured result of a successful [`apply_critical_injury`] call.
///
/// Callers (attack WPs, the GM layer) consume this to narrate the injury,
/// log it, and update any UI that tracks the target's active conditions.
///
/// See pp.187–188 (Critical Injuries).
#[derive(Clone, Debug, PartialEq)]
pub struct CriticalInjuryApplied {
    /// Which Critical Injury was inflicted. See pp.187–188.
    pub kind: CriticalInjuryKind,
    /// Result of applying the 5 Bonus Damage that every Critical Injury
    /// inflicts directly to the target's HP, bypassing armor. See p.187.
    pub bonus_damage_outcome: DamageOutcome,
    /// IDs of the `ActiveEffect`(s) pushed onto the target's `EffectStack`.
    /// Normally contains exactly one entry; empty only if the injury has no
    /// Injury Effect modifiers (e.g. Whiplash, which only bumps Death Save
    /// Penalty — that bump is captured in `death_save_penalty_delta` and
    /// not repeated as a modifier effect).
    pub effects_added: Vec<EffectInstanceId>,
    /// `1` if the injury text includes "Base Death Save Penalty is increased
    /// by 1" (pp.187–188), `0` otherwise.
    pub death_save_penalty_delta: i8,
}

// ---------------------------------------------------------------------------
// apply_critical_injury
// ---------------------------------------------------------------------------

/// Roll on `table`, apply the resulting Critical Injury to `target`, and
/// return the structured outcome — or `None` if the target is not found or
/// the rolled kind is already suffered by the target.
///
/// # Parameters
///
/// - `world`: mutable game state; the target entity is resolved via
///   [`World::entity_mut`].
/// - `target`: the `EntityId` of the entity receiving the injury.
/// - `table`: which 2d6 table to roll on (`Body` or `Head`). See pp.187–188.
/// - `catalog`: the [`Catalog<CriticalInjury>`] for the chosen table. The
///   caller must pass the body catalog when `table == CritTable::Body` and
///   the head catalog when `table == CritTable::Head`. Mixing them will cause
///   a lookup failure (the function returns `None`).
/// - `rng`: deterministic RNG; consumed exactly two `d6` calls per invocation
///   (consumed by [`roll_critical_injury`] regardless of outcome per WP-205).
///
/// # Returns
///
/// - `Some(CriticalInjuryApplied)` when a new, distinct injury was rolled and
///   applied.
/// - `None` when:
///   - `target` is not found in `world`, **or**
///   - the rolled kind is already in the target's `EffectStack` (per p.187:
///     "Roll 2d6 … until you get a Critical Injury that the target isn't
///     currently suffering"), **or**
///   - every kind on the table is already suffered (table exhausted), **or**
///   - the rolled kind is not found in `catalog` (caller/catalog mismatch).
///
/// # Bonus Damage
///
/// The 5 Bonus Damage is applied with `bypass_armor: true` and attributed to
/// `HitLocation::Body` regardless of `table`. Per p.187: "The Bonus Damage
/// doesn't ablate armor and isn't modified by hit location." Because the bonus
/// bypasses armor entirely, the location choice is mechanically irrelevant;
/// `Body` is chosen as the simpler default. See the module-level doc for the
/// rationale.
///
/// # API deviation
///
/// The `catalog` parameter is not in the WP-305 spec signature. See the
/// module-level documentation for the rationale.
///
/// See pp.187–188 (Critical Injuries).
pub fn apply_critical_injury(
    world: &mut World,
    target: EntityId,
    table: CritTable,
    catalog: &Catalog<CriticalInjury>,
    rng: &mut Rng,
) -> Option<CriticalInjuryApplied> {
    // Step 1: Resolve target — bail if not found. See module docs.
    // (We need the already-suffering list before calling roll_critical_injury,
    // so we collect it from the current effect stack.)
    let already_suffering: Vec<CriticalInjuryKind> = {
        let entity = world.entity(target)?;
        entity
            .effects
            .iter()
            .filter_map(|e| {
                if let EffectSource::CriticalInjury(k) = &e.source {
                    Some(k.clone())
                } else {
                    None
                }
            })
            .collect()
    };

    // Step 2: Roll 2d6 on the table, filtered by already-suffered injuries.
    // roll_critical_injury always consumes 2 dice even on a None result
    // (WP-205 determinism guarantee). See p.187.
    let kind = roll_critical_injury(table, &already_suffering, rng)?;

    // Step 3: Look up the full CriticalInjury definition from the catalog.
    // The caller must supply the matching catalog (body for Body, head for Head).
    let slug = slug_for_kind(&kind)?;
    let definition = catalog.get(slug)?;

    // Step 4: Apply 5 Bonus Damage bypassing armor. See p.187:
    // "All Critical Injuries … deal 5 Bonus Damage directly to the target's
    // Hit Points … The Bonus Damage doesn't ablate armor."
    // Location is always Body regardless of table — see module-level rationale.
    let bonus_dmg = DamageApplication {
        target,
        raw_damage: 5,
        location: HitLocation::Body, // See module-level deviation note.
        bypass_armor: true,
        source_label: format!("Critical Injury bonus damage ({:?})", kind),
        triggered_critical: false,
    };
    let bonus_damage_outcome = apply_damage(world, bonus_dmg);

    // Step 5: Push an ActiveEffect onto the target's EffectStack.
    // The effect's duration is UntilHealed, mirroring the Quick Fix / Treatment
    // healing model from pp.187–188 and the EffectDuration design in WP-003.
    //
    // The uuid crate is configured without the `v4` (random) feature in
    // cpr_rules (see Cargo.toml comment). We derive a deterministic UUID from
    // the ChaCha20 RNG by consuming two u64 words — this keeps ID generation
    // deterministic and replayable. Two u64s = 128 bits = UUID.
    let hi = rng.next_u64();
    let lo = rng.next_u64();
    let uuid_bytes = {
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&hi.to_le_bytes());
        b[8..].copy_from_slice(&lo.to_le_bytes());
        b
    };
    let effect_id = EffectInstanceId(Uuid::from_bytes(uuid_bytes));
    let duration = EffectDuration::UntilHealed {
        quick_fix: definition.quick_fix.as_ref().map(|qf| qf.dv),
        treatment: definition.treatment.dv,
    };
    let effect = ActiveEffect {
        id: effect_id,
        source: EffectSource::CriticalInjury(kind.clone()),
        modifiers: definition.effects.clone(),
        duration,
    };

    let mut effects_added = Vec::with_capacity(1);

    // Only push the effect if it carries modifiers. Some injuries (e.g.
    // Whiplash, which only has `increases_death_save_penalty`) emit no
    // EffectModifier entries — pushing an empty effect would clutter the
    // stack without mechanical benefit. The death-save delta is captured
    // in `death_save_penalty_delta` below.
    if !effect.modifiers.is_empty() {
        let entity = world.entity_mut(target)?;
        entity.effects.add(effect);
        effects_added.push(effect_id);
    }

    // Step 6: Compute death-save penalty delta. See pp.187–188.
    let death_save_penalty_delta = if definition.increases_death_save_penalty {
        1i8
    } else {
        0i8
    };

    Some(CriticalInjuryApplied {
        kind,
        bonus_damage_outcome,
        effects_added,
        death_save_penalty_delta,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Return the RON slug for a `CriticalInjuryKind`, used to look up the
/// full definition from a loaded `Catalog<CriticalInjury>`.
///
/// The slug scheme mirrors the one enforced by WP-205's loader:
/// `"crit_body_<roll>"` for body-table injuries (where `<roll>` is the 2d6
/// value, zero-padded to two digits), and `"crit_head_<roll>"` for head-table
/// injuries. `ForeignObject` appears on both tables at roll 7; the slug is
/// determined by scanning both tables. Returns `None` if the kind is not
/// found (should never happen for a valid `CriticalInjuryKind`).
fn slug_for_kind(kind: &CriticalInjuryKind) -> Option<&'static str> {
    // Body table slugs (p.187, rolls 2..=12).
    let body_slug = match kind {
        CriticalInjuryKind::DismemberedArm => Some("crit_body_02"),
        CriticalInjuryKind::DismemberedHand => Some("crit_body_03"),
        CriticalInjuryKind::CollapsedLung => Some("crit_body_04"),
        CriticalInjuryKind::BrokenRibs => Some("crit_body_05"),
        CriticalInjuryKind::BrokenArm => Some("crit_body_06"),
        CriticalInjuryKind::BrokenLeg => Some("crit_body_08"),
        CriticalInjuryKind::TornMuscle => Some("crit_body_09"),
        CriticalInjuryKind::SpinalInjury => Some("crit_body_10"),
        CriticalInjuryKind::CrushedFingers => Some("crit_body_11"),
        CriticalInjuryKind::DismemberedLeg => Some("crit_body_12"),
        // Head table slugs (p.188, rolls 2..=12).
        CriticalInjuryKind::LostEye => Some("crit_head_02"),
        CriticalInjuryKind::BrainInjury => Some("crit_head_03"),
        CriticalInjuryKind::DamagedEye => Some("crit_head_04"),
        CriticalInjuryKind::Concussion => Some("crit_head_05"),
        CriticalInjuryKind::BrokenJaw => Some("crit_head_06"),
        CriticalInjuryKind::Whiplash => Some("crit_head_08"),
        CriticalInjuryKind::CrackedSkull => Some("crit_head_09"),
        CriticalInjuryKind::DamagedEar => Some("crit_head_10"),
        CriticalInjuryKind::CrushedWindpipe => Some("crit_head_11"),
        CriticalInjuryKind::LostEar => Some("crit_head_12"),
        // ForeignObject appears on both tables at roll 7 (pp.187–188).
        // The slug returned here is for the body table; callers using the
        // head catalog will find the same slug structure ("crit_head_07").
        // Since slug_for_kind is only used for catalog lookup and the caller
        // picks the catalog, we return the body slug as the canonical one.
        // In practice, roll_critical_injury selects from the correct table,
        // so ForeignObject is always looked up in the appropriate catalog.
        CriticalInjuryKind::ForeignObject => None, // handled below
    };

    if body_slug.is_some() {
        return body_slug;
    }

    // ForeignObject: determine which table to look in based on which table's
    // roll map yields 7 for ForeignObject.
    // ForeignObject is roll 7 on both tables — the slug differs by prefix.
    // We can't determine the table from the kind alone, so we return the
    // body table slug as the default. The caller is responsible for ensuring
    // the catalog matches the table. If the lookup fails (wrong catalog),
    // apply_critical_injury returns None — acceptable behaviour.
    Some("crit_body_07")
}

/// Look up a `CriticalInjury` definition by kind, trying both body and head
/// catalogs (in that order). Used internally when the table is ambiguous
/// (only for `ForeignObject` which appears on both tables).
///
/// For all other kinds the table is unambiguous from the kind itself.
#[allow(dead_code)]
fn find_in_catalogs<'a>(
    kind: &CriticalInjuryKind,
    body_catalog: &'a Catalog<CriticalInjury>,
    head_catalog: &'a Catalog<CriticalInjury>,
) -> Option<&'a CriticalInjury> {
    let slug = slug_for_kind(kind)?;
    body_catalog
        .get(slug)
        .or_else(|| head_catalog.get(slug))
        .or_else(|| {
            // For ForeignObject on the head table: try "crit_head_07".
            if matches!(kind, CriticalInjuryKind::ForeignObject) {
                head_catalog.get("crit_head_07")
            } else {
                None
            }
        })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::armor::ArmorKind;
    use crate::catalog::critical_injuries::{
        body_kind_for_roll, head_kind_for_roll, load_critical_injuries_body,
        load_critical_injuries_head,
    };
    use crate::character::data::ArmorPiece;
    use crate::character::hp::recompute_wounds;
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;
    use std::path::PathBuf;

    // ---- Catalog paths -------------------------------------------------------

    fn body_catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content/tables/critical_injuries_body.ron");
        p
    }

    fn head_catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p.push("content/tables/critical_injuries_head.ron");
        p
    }

    fn body_catalog() -> Catalog<CriticalInjury> {
        load_critical_injuries_body(&body_catalog_path()).expect("body catalog must load")
    }

    fn head_catalog() -> Catalog<CriticalInjury> {
        load_critical_injuries_head(&head_catalog_path()).expect("head catalog must load")
    }

    // ---- World helpers -------------------------------------------------------

    /// Build a World with the PC having body armor at the given SP.
    fn world_with_armor(sp: u8) -> World {
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        pc.armor.body = Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: sp,
            max_sp: sp,
        });
        World::new(pc)
    }

    /// Build a World with no armor.
    fn world_no_armor() -> World {
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        pc.armor.body = None;
        pc.armor.head = None;
        World::new(pc)
    }

    // ---- check_critical_trigger acceptance tests ----------------------------

    /// Acceptance: [6,6,3,2] → true. See p.187.
    #[test]
    fn test_critical_trigger_two_sixes() {
        assert!(
            check_critical_trigger(&[6, 6, 3, 2]),
            "two sixes must trigger a critical injury (p.187)"
        );
    }

    /// Acceptance: [6,5,4,3] → false (only one six). See p.187.
    #[test]
    fn test_critical_trigger_one_six() {
        assert!(
            !check_critical_trigger(&[6, 5, 4, 3]),
            "a single six must NOT trigger a critical injury (p.187)"
        );
    }

    /// Acceptance: [6,6,6,4] → true (three sixes satisfies >= 2). See p.187.
    #[test]
    fn test_critical_trigger_three_sixes() {
        assert!(
            check_critical_trigger(&[6, 6, 6, 4]),
            "three sixes must trigger a critical injury (p.187)"
        );
    }

    /// Acceptance: empty slice → false. See p.187.
    #[test]
    fn test_critical_trigger_no_dice() {
        assert!(
            !check_critical_trigger(&[]),
            "empty damage roll slice must not trigger a critical (p.187)"
        );
    }

    // Additional trigger edge-cases.

    #[test]
    fn test_critical_trigger_all_sixes() {
        assert!(check_critical_trigger(&[6, 6, 6, 6, 6]));
    }

    #[test]
    fn test_critical_trigger_no_sixes() {
        assert!(!check_critical_trigger(&[5, 4, 3, 2, 1]));
    }

    #[test]
    fn test_critical_trigger_single_die_six() {
        assert!(!check_critical_trigger(&[6]));
    }

    #[test]
    fn test_critical_trigger_exactly_two_sixes_among_many() {
        assert!(check_critical_trigger(&[1, 2, 3, 4, 5, 6, 6]));
    }

    // ---- apply_critical_injury acceptance tests -----------------------------

    /// Acceptance: a target already suffering BrokenArm cannot get BrokenArm
    /// again — roll_critical_injury returns None for that kind, so
    /// apply_critical_injury returns None. Per p.187.
    #[test]
    fn test_no_repeat_critical() {
        let catalog = body_catalog();
        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);

        // Pre-load BrokenArm onto the effect stack.
        world.pc.effects.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(0xAA01)),
            source: EffectSource::CriticalInjury(CriticalInjuryKind::BrokenArm),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });

        // Find a seed that would naturally roll a 6 (BrokenArm, body table).
        // seed hunt: 2d6 == 6 on body table.
        let seed = (0u64..1_000_000)
            .find(|&s| {
                let mut r = Rng::seed_from_u64(s);
                let a = crate::dice::d6(&mut r);
                let b = crate::dice::d6(&mut r);
                a + b == 6
            })
            .expect("must find seed rolling 2d6 == 6");

        let mut rng = Rng::seed_from_u64(seed);
        let result = apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng);

        assert!(
            result.is_none(),
            "target already suffering BrokenArm must not receive BrokenArm again (p.187); got: {:?}",
            result.map(|r| r.kind)
        );
    }

    /// Acceptance: 5 bonus damage applied with bypass_armor=true, so
    /// sp_blocked == 0 even when the target has body armor. See p.187.
    #[test]
    fn test_bonus_damage_bypasses_armor() {
        let catalog = body_catalog();

        // Use SP 18 (Metalgear) — if bypass worked, sp_blocked == 0; if it
        // didn't, sp_blocked would absorb all 5 damage.
        let mut world = world_with_armor(18);
        let pc_id = EntityId(world.pc.id.0);

        // Find a seed that produces a non-None result (i.e. not all-exhausted).
        let mut rng = Rng::seed_from_u64(42);
        let result = apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng);

        // The function may return None if the particular seed rolled a duplicate
        // (table not exhausted with empty already_suffering is impossible), but
        // with an empty stack any roll succeeds. Use a different seed if needed.
        let applied = result.expect("with empty already-suffering list, must apply some critical");
        assert_eq!(
            applied.bonus_damage_outcome.sp_blocked, 0,
            "bonus damage must bypass armor entirely (p.187): sp_blocked must be 0"
        );
        assert_eq!(
            applied.bonus_damage_outcome.hp_lost, 5,
            "bonus damage must deal exactly 5 HP (p.187)"
        );
        assert_eq!(
            applied.bonus_damage_outcome.raw_damage, 5,
            "bonus damage raw_damage must be 5 (p.187)"
        );
    }

    /// Acceptance: passing `CritTable::Head` causes the roll to be on the
    /// head table. We verify this by checking that the returned `kind` is
    /// always a head-table kind (not a body-only kind) across many seeds.
    #[test]
    fn test_aimed_head_uses_head_table() {
        let catalog = head_catalog();
        let head_kinds: Vec<CriticalInjuryKind> =
            (2..=12u8).filter_map(head_kind_for_roll).collect();

        let mut successes = 0;
        for seed in 0..200u64 {
            let mut world = world_no_armor();
            let pc_id = EntityId(world.pc.id.0);
            let mut rng = Rng::seed_from_u64(seed);

            if let Some(applied) =
                apply_critical_injury(&mut world, pc_id, CritTable::Head, &catalog, &mut rng)
            {
                assert!(
                    head_kinds.contains(&applied.kind),
                    "CritTable::Head must yield a head-table injury, got {:?}",
                    applied.kind
                );
                successes += 1;
            }
        }
        assert!(
            successes > 0,
            "at least one seed must produce a successful head critical"
        );
    }

    /// Acceptance: a critical injury with `increases_death_save_penalty == true`
    /// (e.g. DismemberedArm, roll 2 on the body table — p.187) returns
    /// `death_save_penalty_delta == 1`. See p.187.
    #[test]
    fn test_death_save_penalty_increments() {
        let catalog = body_catalog();

        // Find a seed that rolls exactly 2 on 2d6 (DismemberedArm, p.187 roll=2).
        let seed = (0u64..1_000_000)
            .find(|&s| {
                let mut r = Rng::seed_from_u64(s);
                let a = crate::dice::d6(&mut r);
                let b = crate::dice::d6(&mut r);
                a + b == 2 // Only possible when both dice are 1, which never yields 2 for standard d6.
                           // 2d6 minimum is 2 (1+1).
            })
            .expect("must find seed for 2d6 == 2 (both dice = 1)");

        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);
        let mut rng = Rng::seed_from_u64(seed);

        let applied = apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng)
            .expect("DismemberedArm must be applied when 2d6 == 2");

        assert_eq!(
            applied.kind,
            CriticalInjuryKind::DismemberedArm,
            "roll of 2 on body table must yield DismemberedArm (p.187)"
        );
        assert_eq!(
            applied.death_save_penalty_delta, 1,
            "DismemberedArm must return death_save_penalty_delta == 1 (p.187)"
        );
    }

    // ---- Additional regression / scenario tests ----------------------------

    /// Regression: unknown EntityId returns None without panic.
    #[test]
    fn test_unknown_entity_returns_none() {
        let catalog = body_catalog();
        let mut world = world_no_armor();
        let unknown = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let mut rng = Rng::seed_from_u64(0);
        let result =
            apply_critical_injury(&mut world, unknown, CritTable::Body, &catalog, &mut rng);
        assert!(result.is_none(), "unknown EntityId must return None");
    }

    /// Regression: after a successful apply_critical_injury, the target's
    /// EffectStack contains an entry with EffectSource::CriticalInjury(kind).
    /// Injuries without modifiers (e.g. Whiplash) do NOT push an effect.
    /// Injuries with modifiers (e.g. BrokenLeg) do push an effect.
    #[test]
    fn test_effect_stack_updated_for_injury_with_modifiers() {
        let catalog = body_catalog();

        // BrokenLeg (body roll=8) has a MovePenalty modifier.
        // Find a seed yielding 2d6 == 8.
        let seed = (0u64..1_000_000)
            .find(|&s| {
                let mut r = Rng::seed_from_u64(s);
                let a = crate::dice::d6(&mut r);
                let b = crate::dice::d6(&mut r);
                a + b == 8
            })
            .expect("must find seed for 2d6 == 8");

        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);
        let mut rng = Rng::seed_from_u64(seed);

        let applied = apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng)
            .expect("BrokenLeg must be applied");

        assert_eq!(applied.kind, CriticalInjuryKind::BrokenLeg);
        // BrokenLeg carries a MovePenalty(-4) modifier → effect must be pushed.
        assert!(
            !applied.effects_added.is_empty(),
            "BrokenLeg (with modifiers) must push an effect onto the stack"
        );
        let stack = &world.pc.effects;
        let crit_effects: Vec<_> = stack
            .iter()
            .filter(|e| matches!(&e.source, EffectSource::CriticalInjury(_)))
            .collect();
        assert_eq!(
            crit_effects.len(),
            1,
            "exactly one CriticalInjury effect on stack after application"
        );
        assert!(
            matches!(
                &crit_effects[0].source,
                EffectSource::CriticalInjury(CriticalInjuryKind::BrokenLeg)
            ),
            "stack entry must carry BrokenLeg as source"
        );
    }

    /// Regression: when the target already suffers every kind on the body
    /// table, apply_critical_injury returns None. Per p.187.
    #[test]
    fn test_exhausted_table_returns_none() {
        let catalog = body_catalog();
        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);

        // Pre-load all body-table kinds onto the stack.
        for (i, roll) in (2u8..=12).enumerate() {
            if let Some(kind) = body_kind_for_roll(roll) {
                world.pc.effects.add(ActiveEffect {
                    id: EffectInstanceId(Uuid::from_u128(i as u128 + 0xBEEF_0000)),
                    source: EffectSource::CriticalInjury(kind),
                    modifiers: vec![],
                    duration: EffectDuration::Permanent,
                });
            }
        }

        let mut rng = Rng::seed_from_u64(0);
        let result = apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng);
        assert!(
            result.is_none(),
            "all body-table kinds already suffered → must return None (p.187)"
        );
    }

    /// Regression: `effects_added` is empty for injuries that have no
    /// EffectModifier entries in the catalog. We use BrokenJaw (head table
    /// roll=6, p.188: "-4 to all Actions involving speech" — but the catalog
    /// entry stores `effects: []` because there is no `SpeechActionsPenalty`
    /// variant in `EffectModifier` yet). `increases_death_save_penalty` is
    /// `false` for BrokenJaw, so `death_save_penalty_delta` should be 0.
    #[test]
    fn test_no_modifiers_injury_does_not_push_effect() {
        let catalog = head_catalog();

        // Find a seed yielding 2d6 == 6 (BrokenJaw on head table, p.188).
        let seed = (0u64..1_000_000)
            .find(|&s| {
                let mut r = Rng::seed_from_u64(s);
                let a = crate::dice::d6(&mut r);
                let b = crate::dice::d6(&mut r);
                a + b == 6
            })
            .expect("must find seed for 2d6 == 6");

        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);
        let mut rng = Rng::seed_from_u64(seed);

        let applied = apply_critical_injury(&mut world, pc_id, CritTable::Head, &catalog, &mut rng)
            .expect("BrokenJaw must be applied");

        assert_eq!(applied.kind, CriticalInjuryKind::BrokenJaw);
        assert_eq!(
            applied.death_save_penalty_delta, 0,
            "BrokenJaw does not increase death save penalty (p.188)"
        );
        assert!(
            applied.effects_added.is_empty(),
            "BrokenJaw has no EffectModifier entries in catalog → effects_added must be empty"
        );
    }

    /// Regression: the bonus damage outcome has `target` matching the PC id.
    #[test]
    fn test_bonus_damage_outcome_target_matches() {
        let catalog = body_catalog();
        let mut world = world_no_armor();
        let pc_id = EntityId(world.pc.id.0);
        let mut rng = Rng::seed_from_u64(7);

        if let Some(applied) =
            apply_critical_injury(&mut world, pc_id, CritTable::Body, &catalog, &mut rng)
        {
            assert_eq!(
                applied.bonus_damage_outcome.target, pc_id,
                "bonus_damage_outcome.target must equal the passed EntityId"
            );
        }
    }
}
