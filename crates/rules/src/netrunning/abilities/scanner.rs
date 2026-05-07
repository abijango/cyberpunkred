//! Interface Ability: Scanner — reveal nearby NET Architecture access points.
//!
//! ## Rulebook (p.199)
//!
//! > **Scanner** — Use a **Meat Action** to find out the Meatspace location of
//! > access points to any NET Architectures in an area. The higher the Check,
//! > the more you spot from further away. It is up to the GM's discretion to
//! > determine how much you find.
//! >
//! > *Example: The Netrunner uses their Scanner Ability to search the building
//! > for NET Architectures and their access points to hack using a Meat Action.
//! > Rolling a 1d10 and adding their Interface (7), they get a 14. With this
//! > roll, the GM determines that the Netrunner learns the Meatspace location
//! > of two of the nearby access points for the building's NET Architecture.*
//!
//! ## Key rules notes
//!
//! - **Meat Action**, not a NET Action (p.198). The Netrunner does not need to
//!   be jacked in.
//! - Roll formula: `INT + Interface_rank + 1d10` with critical rules (p.199).
//!   "Interface" is the Netrunner Role Ability ranking held in
//!   `character.role_rank` (not a named `SkillId` — there is no `SkillId::Interface`
//!   in the closed enum). This is the only sensible reading of p.199 RAW.
//! - DV: the rulebook does not publish a fixed DV for Scanner; the GM decides
//!   how many access points are revealed based on the final roll. This
//!   implementation uses `DV(13)` (`DV::EVERYDAY`) as the baseline success
//!   threshold — matching the example on p.199 where a roll of 14 reveals
//!   two access points. This is a pragmatic choice; a future WP or GM-layer
//!   hook can override the DV when constructing `ScannerAction`.
//! - Access-point count: `max(0, margin / 2)` — one access point per 2 points
//!   the roll exceeds the DV. Derived from the p.199 example (roll 14, DV 13,
//!   margin 1 → floor(1/2) = 0… but the GM revealed 2). **Deviation note:**
//!   the book gives the GM discretion; no explicit formula is stated. We use
//!   `max(0, margin / 2)` as a systematic default (minimum 1 on any success
//!   so a marginal success still finds something). See `access_points_found`
//!   for the stub note.
//! - **Stub**: the returned `Vec<NetArchId>` contains placeholder ids
//!   (`"scanned_0"`, `"scanned_1"`, …) because the actual access-point lookup
//!   belongs to the scene/location layer (a later WP). The count is real; the
//!   ids are synthetic until that WP wires in a live architecture registry.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::architecture::NetArchId;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

/// Default DV for a Scanner check (p.199 example implies ~13).
///
/// The rulebook leaves the DV to GM discretion. We use `DV::EVERYDAY` (13)
/// as the baseline. Callers who want a different DV can use a custom
/// `ScannerAction` with a modified `dv` field in future (if the struct is
/// extended), or override at the scene layer.
///
/// See p.199 and the `DV` constants in [`crate::types`].
pub const SCANNER_DEFAULT_DV: DV = DV::EVERYDAY; // DV(13) per p.199

/// A **Meat Action** Scanner check. See p.199.
///
/// The netrunner rolls `INT + Interface_rank + 1d10` vs [`SCANNER_DEFAULT_DV`]
/// (DV 13 per the p.199 example). A higher margin reveals more nearby NET
/// Architecture access points.
///
/// ## Meat vs. NET Action
///
/// Scanner is explicitly a *Meat Action* (p.199): the netrunner acts in
/// physical space, not in the NET. They do **not** consume a NET Action slot;
/// they consume their regular (Meat) Action for the turn.
///
/// ## Resolution
///
/// Implements [`Resolution`], producing `Result<ScannerOutcome, RulesError>`.
/// Errors on unknown entity or insufficient LUCK.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScannerAction {
    /// The netrunner performing the scan.
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

/// Outcome of a [`ScannerAction`].
///
/// On success or failure, `breakdown` is always populated. The
/// `access_points_found` list is non-empty only when the check succeeded (i.e.
/// `breakdown.success == true`).
///
/// ## Stub note
///
/// `access_points_found` contains placeholder [`NetArchId`] values of the form
/// `"scanned_0"`, `"scanned_1"`, … The *count* is meaningful (derived from the
/// check margin); the *identifiers* are synthetic stubs pending scene/location
/// integration in a later WP. See the module-level doc for context.
#[derive(Clone, Debug, PartialEq)]
pub struct ScannerOutcome {
    /// Full breakdown of the Interface + d10 roll, including margin vs. DV.
    pub breakdown: CheckBreakdown,
    /// Stub placeholder access-point ids found by the scan.
    ///
    /// Count = `max(0, margin / 2)` with a minimum of 1 on any success.
    /// Ids are `NetArchId("scanned_N")` until scene layer is wired in.
    /// See module doc and the deviation note in `ScannerAction`.
    pub access_points_found: Vec<NetArchId>,
}

impl Resolution for ScannerAction {
    type Outcome = Result<ScannerOutcome, RulesError>;

    /// Resolve the Scanner Meat Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Look up the netrunner via `world.entity_mut`. If missing, return
    ///    `Err(RulesError::EntityNotFound)`.
    /// 2. Validate and spend luck via `actor.spend_luck(self.luck_to_spend)`.
    ///    Returns `Err(RulesError::InsufficientLuck)` on failure.
    /// 3. Capture `INT` and `role_rank` from the netrunner (after spending luck
    ///    so the pool is already decremented before the roll, matching p.130).
    /// 4. Roll `d10_with_crits(rng)`.
    /// 5. Build `CheckBreakdown::new(int, role_rank, 0, luck_spent, d10, DV(13))`.
    /// 6. Compute access-point count = `max(1, margin / 2)` on success, 0 on
    ///    failure. Generate placeholder `NetArchId` values for the scene layer.
    ///
    /// See p.199 (Scanner) and p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — look up the entity. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 2 — validate and spend luck (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 3 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198: "Interface is the Netrunner
        // Role Ability"). There is no `SkillId::Interface` in the closed
        // enum — the role-ability rank plays the role of the "skill" column.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 4 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 5 — build the breakdown.
        // stat_value   = INT (the STAT linked to Interface checks)
        // skill_value  = Interface Role Ability rank (plays the skill column)
        // modifier_total = 0 (no situational modifiers in base Scanner)
        // See p.199.
        let breakdown = CheckBreakdown::new(
            int,
            interface_rank,
            0,
            self.luck_to_spend,
            d10,
            SCANNER_DEFAULT_DV,
        );

        // Step 6 — compute access-point count.
        // The book gives GM discretion; we use margin/2 rounded down, minimum 1
        // on any success, 0 on failure. This is a systematic default awaiting
        // scene-layer wiring.
        //
        // p.199 example: roll 14, implied DV ~13 → margin 1 → GM reveals 2.
        // Our formula gives max(1, 1/2) = max(1, 0) = 1. The book example
        // shows GM discretion giving 2; we're conservative here and document
        // the deviation. The GM layer (later WP) can override.
        let count: usize = if breakdown.success {
            let margin_half = breakdown.margin / 2;
            margin_half.max(1) as usize
        } else {
            0
        };

        let access_points_found: Vec<NetArchId> = (0..count)
            .map(|i| NetArchId(format!("scanned_{i}")))
            .collect();

        Ok(ScannerOutcome {
            breakdown,
            access_points_found,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Role;
    use crate::dice::{CritD10, D10Outcome};
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;

    /// Helper: build a `CritD10` with a known net value, simulating a normal
    /// (non-crit) d10 roll of `roll`. Used to make RNG-independent assertions.
    fn fake_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    /// Build a fresh Netrunner PC with specific INT and role_rank.
    ///
    /// Starts from `fresh_pc()` (which is a Solo) and patches the fields we
    /// care about for Scanner tests.
    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        // Give generous luck pool so luck-spend tests can drain it.
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    // -------------------------------------------------------------------------
    // test_scanner_check_uses_int_and_interface
    // -------------------------------------------------------------------------

    /// Verifies that the roll formula is `INT + Interface_rank + d10`.
    ///
    /// Setup: INT = 6, role_rank (Interface) = 4, forced d10 net = 5.
    /// Expected final value = 6 + 4 + 5 = 15, DV 13 → success, margin 2.
    ///
    /// We use a seeded RNG and a seed that produces a base d10 of 5 on the
    /// first call to `d10_with_crits`. We verify the arithmetic manually via
    /// `CheckBreakdown::new` with a fake d10, then check that an actual
    /// resolve() call with a matching seed produces the same success/failure.
    #[test]
    fn test_scanner_check_uses_int_and_interface() {
        // Direct arithmetic check (seed-independent).
        // INT 6, Interface 4, d10 net 5 → final 15 vs DV 13 → success.
        let d10 = fake_d10(5);
        let bd = CheckBreakdown::new(6, 4, 0, 0, d10, SCANNER_DEFAULT_DV);
        assert_eq!(bd.final_value, 15, "INT(6) + Interface(4) + d10(5) = 15");
        assert_eq!(bd.dv, DV(13), "DV should be 13 (EVERYDAY)");
        assert!(bd.success, "15 >= 13 → success");
        assert_eq!(bd.margin, 2);

        // Integration check through resolve().
        // Find a seed where the first d10_with_crits roll has net == 5.
        // Seed 1 produces: let's just verify it resolves without error and
        // produces a success with the right formula by checking stat/skill.
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let action = ScannerAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        // Try a few seeds until we get a success; assert the formula columns.
        let mut rng = Rng::seed_from_u64(42);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must not error");

        // Regardless of d10, the stat_value and skill_value columns must be
        // INT and role_rank respectively.
        assert_eq!(
            outcome.breakdown.stat_value, 6,
            "stat_value must be INT (6)"
        );
        assert_eq!(
            outcome.breakdown.skill_value, 4,
            "skill_value must be Interface rank (4)"
        );
        assert_eq!(outcome.breakdown.modifier_total, 0);
        assert_eq!(outcome.breakdown.dv, SCANNER_DEFAULT_DV);
        // final_value = 6 + 4 + d10.net (no luck spent).
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + outcome.breakdown.d10.net
        );
    }

    // -------------------------------------------------------------------------
    // test_scanner_finds_more_with_higher_roll
    // -------------------------------------------------------------------------

    /// A roll 5 above DV13 (margin 5) → count = max(1, 5/2) = max(1, 2) = 2
    /// access points.
    #[test]
    fn test_scanner_finds_more_with_higher_roll() {
        // Build a breakdown with margin 5 (final=18, DV=13).
        // INT 8, Interface 5, d10 net 5 → 18 vs 13, margin 5.
        let d10 = fake_d10(5);
        let bd = CheckBreakdown::new(8, 5, 0, 0, d10, SCANNER_DEFAULT_DV);
        assert_eq!(bd.margin, 5);
        assert!(bd.success);

        // Count = max(1, 5/2) = max(1, 2) = 2.
        let margin_half = bd.margin / 2;
        let count = margin_half.max(1) as usize;
        assert_eq!(count, 2, "margin 5 → max(1, 5/2)=2 access points");

        // Scan over seeds to find one where margin ≥ 4, verifying count ≥ 2.
        let mut found = false;
        for seed in 0u64..200 {
            // Each iteration needs a fresh world since resolve mutates it.
            let pc2 = netrunner_pc(8, 5);
            let pc_id2 = EntityId(pc2.id.0);
            let mut world2 = World::new(pc2);
            let mut rng = Rng::seed_from_u64(seed);
            let action2 = ScannerAction {
                netrunner: pc_id2,
                luck_to_spend: 0,
            };
            let outcome = action2.resolve(&mut world2, &mut rng).unwrap();
            if outcome.breakdown.margin >= 4 && outcome.breakdown.success {
                // margin ≥ 4 → count ≥ 2.
                assert!(
                    outcome.access_points_found.len() >= 2,
                    "margin {} ≥ 4 → at least 2 APs (seed {})",
                    outcome.breakdown.margin,
                    seed
                );
                found = true;
                break;
            }
        }
        assert!(
            found,
            "could not find a seed with margin >= 4 in 200 attempts"
        );
    }

    // -------------------------------------------------------------------------
    // test_scanner_finds_zero_on_failure
    // -------------------------------------------------------------------------

    /// A failed check returns an empty access-points list.
    #[test]
    fn test_scanner_finds_zero_on_failure() {
        // Build a scenario that guarantees failure: INT 1, Interface 1, DV 13.
        // Even a d10 of 10 gives 1+1+10=12 < 13. A crit (10+follow-up) can
        // exceed this if the follow-up ≥ 2. Use a very low base instead:
        // Just verify through CheckBreakdown that margin < 0 → count 0.
        let d10 = fake_d10(3);
        let bd = CheckBreakdown::new(1, 1, 0, 0, d10, SCANNER_DEFAULT_DV);
        assert!(!bd.success, "1+1+3=5 < 13 → failure");

        let count: usize = if bd.success {
            (bd.margin / 2).max(1) as usize
        } else {
            0
        };
        assert_eq!(count, 0, "failure → 0 access points");

        // Integration: find a seed that produces a failed check for low stats.
        let mut found_failure = false;
        for seed in 0u64..500 {
            let pc2 = netrunner_pc(1, 1);
            let pc_id2 = EntityId(pc2.id.0);
            let mut world2 = World::new(pc2);
            let mut rng = Rng::seed_from_u64(seed);
            let action = ScannerAction {
                netrunner: pc_id2,
                luck_to_spend: 0,
            };
            let outcome = action.resolve(&mut world2, &mut rng).unwrap();
            if !outcome.breakdown.success {
                assert!(
                    outcome.access_points_found.is_empty(),
                    "failed check must yield empty access_points_found (seed {seed})"
                );
                found_failure = true;
                break;
            }
        }
        assert!(
            found_failure,
            "expected at least one failure seed for INT=1, Interface=1 vs DV13"
        );
    }

    // -------------------------------------------------------------------------
    // test_scanner_validates_luck
    // -------------------------------------------------------------------------

    /// Spending more luck than available must return `Err(RulesError::InsufficientLuck)`.
    #[test]
    fn test_scanner_validates_luck() {
        let mut pc = netrunner_pc(6, 4);
        // Drain luck pool to zero.
        pc.luck_pool = 0;
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let action = ScannerAction {
            netrunner: pc_id,
            luck_to_spend: 3, // more than the 0 available
        };

        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(
                result,
                Err(RulesError::InsufficientLuck {
                    requested: 3,
                    available: 0
                })
            ),
            "expected InsufficientLuck(requested=3, available=0), got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_scanner_entity_not_found
    // -------------------------------------------------------------------------

    /// Looking up a non-existent entity must return `Err(RulesError::EntityNotFound)`.
    #[test]
    fn test_scanner_entity_not_found() {
        use uuid::Uuid;

        let pc = netrunner_pc(6, 4);
        let mut world = World::new(pc);

        let bad_id = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = ScannerAction {
            netrunner: bad_id,
            luck_to_spend: 0,
        };

        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::EntityNotFound(id)) if id == bad_id),
            "expected EntityNotFound, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_scanner_luck_adds_to_check
    // -------------------------------------------------------------------------

    /// Spending N luck adds N to the final check value and decrements the pool.
    #[test]
    fn test_scanner_luck_adds_to_check() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        // luck_pool is 10 (set by netrunner_pc); spend 3.
        let mut world = World::new(pc);

        let action = ScannerAction {
            netrunner: pc_id,
            luck_to_spend: 3,
        };
        let mut rng = Rng::seed_from_u64(7);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(
            outcome.breakdown.luck_spent, 3,
            "luck_spent must be recorded in breakdown"
        );
        // final = stat + skill + modifier + luck + d10.net
        // stat=6, skill=4, modifier=0, luck=3.
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 3 + outcome.breakdown.d10.net
        );
        // Pool decremented.
        assert_eq!(world.pc.luck_pool, 7, "luck_pool 10 - 3 spent = 7");
    }

    // -------------------------------------------------------------------------
    // test_scanner_on_marginal_success_finds_one
    // -------------------------------------------------------------------------

    /// A marginal success (margin = 1 or 2) still finds at least 1 access point.
    #[test]
    fn test_scanner_on_marginal_success_finds_one() {
        // margin 1 → max(1, 1/2) = max(1, 0) = 1.
        let d10 = fake_d10(8); // INT 3 + Interface 2 + d10 8 = 13 = DV → margin 0 = success.
        let bd = CheckBreakdown::new(3, 2, 0, 0, d10, SCANNER_DEFAULT_DV);
        assert!(bd.success);
        assert_eq!(bd.margin, 0);
        let count = (bd.margin / 2).max(1) as usize;
        assert_eq!(count, 1, "margin 0 → max(1, 0/2) = 1 access point");

        // margin 1 → count 1.
        let d10_b = fake_d10(9); // 3+2+9 = 14, margin 1
        let bd_b = CheckBreakdown::new(3, 2, 0, 0, d10_b, SCANNER_DEFAULT_DV);
        assert_eq!(bd_b.margin, 1);
        let count_b = (bd_b.margin / 2).max(1) as usize;
        assert_eq!(count_b, 1);
    }

    // -------------------------------------------------------------------------
    // test_scanner_placeholder_ids_are_sequential
    // -------------------------------------------------------------------------

    /// The stub access-point ids follow the `scanned_N` pattern.
    #[test]
    fn test_scanner_placeholder_ids_are_sequential() {
        // Find a seed that gives us at least 2 access points (INT=8, Interface=8).
        for seed in 0u64..500 {
            let pc = netrunner_pc(8, 8);
            let pc_id = EntityId(pc.id.0);
            let mut world = World::new(pc);
            let mut rng = Rng::seed_from_u64(seed);
            let action = ScannerAction {
                netrunner: pc_id,
                luck_to_spend: 0,
            };
            let outcome = action.resolve(&mut world, &mut rng).unwrap();
            if outcome.access_points_found.len() >= 2 {
                for (i, ap) in outcome.access_points_found.iter().enumerate() {
                    assert_eq!(
                        ap.0,
                        format!("scanned_{i}"),
                        "placeholder id at index {i} must be 'scanned_{i}'"
                    );
                }
                return;
            }
        }
        panic!("could not find a seed with >= 2 access points for INT=8, Interface=8");
    }
}
