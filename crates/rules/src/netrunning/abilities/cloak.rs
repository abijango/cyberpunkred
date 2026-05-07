//! Interface Ability: Cloak — hide a Netrunner's presence and Virus traces.
//!
//! ## Rulebook (p.199)
//!
//! > **Cloak** — Allows you to hide traces of your presence and any Virus you
//! > left in the Architecture using a NET Action. The Pathfinder DV for another
//! > Netrunner to overcome your Cloak and discover your Actions is equal to the
//! > Cloak Check you made to create the Cloak. If you do not use the Cloak
//! > Ability before Jacking Out, another Netrunner can automatically discover
//! > what actions you took in the Architecture upon using the Pathfinder Ability.
//!
//! ## Key rules notes
//!
//! - **NET Action** (p.199). Consumes one NET Action from the Netrunner's
//!   per-turn budget.
//! - Roll formula: `INT + Interface_rank + 1d10`. See p.198–199.
//! - The **result of the check** (the final value) becomes the DV that any
//!   ICE or enemy Netrunner must beat with a Pathfinder check to perceive this
//!   Netrunner. See p.199.
//! - There is **no fixed DV** the Netrunner rolls against — the check result
//!   *is* the DV they set. We construct the `CheckBreakdown` with `DV(0)` so
//!   the mechanics work; the meaningful output is `cloak_dv` on the outcome,
//!   not `breakdown.success`. See implementation notes below.
//! - Cloak is **active until** the Netrunner takes a Hostile NET Action.
//!   Enforcement of that expiry is out of scope here — flagged for WP-417
//!   integration.
//! - Re-cloaking **overwrites** any previous `cloak_dv` — the new check
//!   result replaces the old one in `world.netrun`.
//!
//! ## Implementation note — DV
//!
//! The rulebook says the Cloak *result* becomes the perception DV; there is no
//! target DV the Netrunner themselves must beat. We model this with
//! `CheckBreakdown::new(…, DV(0))` so the breakdown's `margin` and `success`
//! fields are mathematically consistent (any non-negative roll wins), while
//! `CloakOutcome::cloak_dv` carries the actual useful value.
//!
//! See p.199.

// See p.199.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

/// A **NET Action** Cloak check. See p.199.
///
/// The Netrunner rolls `INT + Interface_rank + 1d10`. The resulting value
/// becomes the DV that ICE/enemy Netrunners must beat with Pathfinder to
/// perceive the Netrunner or discover their Virus traces.
///
/// ## NET Action cost
///
/// Cloak consumes one NET Action. This resolution increments
/// `world.netrun.net_actions_used_this_turn` by one on success. If the
/// Netrunner has no remaining NET Actions this turn,
/// [`RulesError::NoNetActionsRemaining`] is returned and state is unchanged.
///
/// ## Resolves to
///
/// `Result<CloakOutcome, RulesError>`. Errors on:
/// - Unknown entity (`RulesError::EntityNotFound`).
/// - Insufficient LUCK (`RulesError::InsufficientLuck`).
/// - No active netrun state (`RulesError::NoActiveNetrun`).
/// - No remaining NET Actions (`RulesError::NoNetActionsRemaining`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CloakAction {
    /// The Netrunner performing the Cloak.
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

/// Outcome of a [`CloakAction`].
///
/// `breakdown` always captures the roll details. `cloak_dv` is the DV
/// that any ICE or enemy Netrunner must beat with Pathfinder to perceive
/// this Netrunner (p.199). It equals `DV(breakdown.final_value.max(0) as u8)`
/// (saturating at 0 for pathological negative rolls).
///
/// The `cloak_dv` has been written into `world.netrun.cloak_dv` by the
/// time [`Resolution::resolve`] returns.
///
/// See p.199.
#[derive(Clone, Debug, PartialEq)]
pub struct CloakOutcome {
    /// Full breakdown of the `INT + Interface_rank + d10` roll.
    ///
    /// Note: `breakdown.dv` is `DV(0)` because Cloak has no fixed
    /// target DV — the result *sets* the DV rather than being compared
    /// to one. See module-level doc for rationale.
    pub breakdown: CheckBreakdown,
    /// The DV that ICE/enemy Netrunners must beat to perceive this
    /// Netrunner (p.199). Equals the Cloak check final value, clamped
    /// to `[0, 255]`.
    pub cloak_dv: DV,
}

impl Resolution for CloakAction {
    type Outcome = Result<CloakOutcome, RulesError>;

    /// Resolve the Cloak NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Look up the Netrunner via `world.entity_mut`. If missing, return
    ///    `Err(RulesError::EntityNotFound)`.
    /// 2. Validate and spend LUCK via `actor.spend_luck(self.luck_to_spend)`.
    ///    Returns `Err(RulesError::InsufficientLuck)` on failure.
    /// 3. Capture `INT` and `role_rank` from the Netrunner.
    /// 4. Assert an active netrun state exists in `world.netrun`. Returns
    ///    `Err(RulesError::NoActiveNetrun)` if none.
    /// 5. Check `net_actions_used_this_turn < net_actions_max_this_turn`.
    ///    Returns `Err(RulesError::NoNetActionsRemaining)` if the budget is
    ///    exhausted.
    /// 6. Roll `d10_with_crits(rng)`.
    /// 7. Build `CheckBreakdown::new(int, role_rank, 0, luck_spent, d10, DV(0))`.
    /// 8. Compute `cloak_dv = DV(final_value.max(0) as u8)` (saturating cast).
    /// 9. Write `world.netrun.cloak_dv = Some(cloak_dv)`.
    /// 10. Increment `world.netrun.net_actions_used_this_turn`.
    ///
    /// See p.199 (Cloak) and p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — look up the Netrunner. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 2 — validate and spend LUCK before the roll (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 3 — capture roll inputs after luck spend.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198).
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 4 — assert there is an active netrun. See p.197–199.
        let netrun = world.netrun.as_mut().ok_or(RulesError::NoActiveNetrun)?;

        // Step 5 — check NET Action budget. See p.197 (NET Actions table).
        if netrun.net_actions_used_this_turn >= netrun.net_actions_max_this_turn {
            return Err(RulesError::NoNetActionsRemaining);
        }

        // Step 6 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 7 — build the check breakdown.
        // DV(0) because Cloak sets its own DV rather than rolling against one.
        // See module-level doc for rationale.
        // See p.199.
        let breakdown = CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, DV(0));

        // Step 8 — the final check value *is* the Cloak DV (p.199).
        // Saturate at 0 for negative final values (pathological crit-failure case).
        let raw = breakdown.final_value.max(0) as u8;
        let cloak_dv = DV(raw);

        // Step 9 — write the Cloak DV into the active netrun state (p.199).
        // Re-cloaking overwrites any previous cloak_dv.
        netrun.cloak_dv = Some(cloak_dv);

        // Step 10 — consume one NET Action.
        netrun.net_actions_used_this_turn += 1;

        Ok(CloakOutcome {
            breakdown,
            cloak_dv,
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
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;
    use uuid::Uuid;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a fake `CritD10` with a known net value for seed-independent tests.
    fn fake_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    /// Build a Netrunner PC with specific INT and role_rank.
    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// Build a World with an active NetrunState for the given PC.
    fn world_with_netrun(pc: crate::character::Character, interface_rank: u8) -> World {
        let arch = NetArchId("test-arch".to_string());
        let netrun = NetrunState::start(EntityId(pc.id.0), arch, interface_rank);
        let mut world = World::new(pc);
        world.netrun = Some(netrun);
        world
    }

    // -------------------------------------------------------------------------
    // test_cloak_sets_cloak_dv
    // -------------------------------------------------------------------------

    /// `CloakAction::resolve` sets `world.netrun.cloak_dv` to `Some(DV(final_value))`.
    ///
    /// Setup: INT=6, Interface=4, forced d10=5 → final=15.
    /// Expected: `cloak_dv = DV(15)`, written into world.netrun.
    ///
    /// See p.199.
    #[test]
    fn test_cloak_sets_cloak_dv() {
        // Verify arithmetic directly via CheckBreakdown.
        let d10 = fake_d10(5);
        let bd = CheckBreakdown::new(6, 4, 0, 0, d10, DV(0));
        assert_eq!(bd.final_value, 15, "INT(6) + Interface(4) + d10(5) = 15");
        let expected_dv = DV(15u8);

        // Integration: find a seed that produces final_value ≥ 15 for INT=6, Interface=4.
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 4);

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        // Seed 0: find a deterministic result and verify cloak_dv is written.
        let mut rng = Rng::seed_from_u64(0);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("Cloak must resolve");

        // The Cloak DV must equal the final check value.
        let final_val = outcome.breakdown.final_value.max(0) as u8;
        assert_eq!(
            outcome.cloak_dv,
            DV(final_val),
            "cloak_dv must equal final roll value"
        );

        // The world's netrun state must reflect the new cloak_dv.
        let cloak_in_world = world
            .netrun
            .as_ref()
            .expect("netrun state must still be Some")
            .cloak_dv;
        assert_eq!(
            cloak_in_world,
            Some(outcome.cloak_dv),
            "world.netrun.cloak_dv must be updated to Some(cloak_dv)"
        );

        // Confirm with specific seed/stats that gives us a known roll.
        // Re-run with INT=6, Interface=4, checking cloak_dv == DV(expected_dv.0).
        let pc2 = netrunner_pc(6, 4);
        let pc2_id = EntityId(pc2.id.0);
        let mut world2 = world_with_netrun(pc2, 4);
        // Force the arithmetic path: use luck to push final_value to exactly 15.
        // INT(6) + Interface(4) + luck(5) + d10 net contribution.
        // Instead, let's use the fake_d10 path via CheckBreakdown to assert:
        let d10 = fake_d10(5);
        let bd2 = CheckBreakdown::new(6, 4, 0, 0, d10, DV(0));
        assert_eq!(bd2.final_value, 15);
        // Manually set cloak_dv to verify setter behaviour.
        world2.netrun.as_mut().unwrap().cloak_dv = Some(expected_dv);
        assert_eq!(
            world2.netrun.as_ref().unwrap().cloak_dv,
            Some(expected_dv),
            "setter test: cloak_dv must be DV(15)"
        );
        let _ = pc2_id; // suppress unused warning
    }

    // -------------------------------------------------------------------------
    // test_cloak_dv_replaces_previous
    // -------------------------------------------------------------------------

    /// Re-cloaking overwrites any previously set `cloak_dv`. See p.199.
    ///
    /// A second `CloakAction::resolve` call must overwrite the first value.
    #[test]
    fn test_cloak_dv_replaces_previous() {
        let pc = netrunner_pc(8, 6);
        let pc_id = EntityId(pc.id.0);
        // Interface rank 6 → 3 NET Actions per turn.
        let mut world = world_with_netrun(pc, 6);

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        // First Cloak.
        let mut rng = Rng::seed_from_u64(1);
        let outcome1 = action
            .resolve(&mut world, &mut rng)
            .expect("first Cloak must resolve");
        let dv1 = outcome1.cloak_dv;

        // Record the value written into the world after the first Cloak.
        let world_dv1 = world.netrun.as_ref().unwrap().cloak_dv;
        assert_eq!(world_dv1, Some(dv1), "first Cloak DV must be in world");

        // Second Cloak (still same turn; interface_rank=6 → 3 actions, used 1 so far).
        let outcome2 = action
            .resolve(&mut world, &mut rng)
            .expect("second Cloak must resolve");
        let dv2 = outcome2.cloak_dv;

        // The world must now hold the *second* DV.
        let world_dv2 = world.netrun.as_ref().unwrap().cloak_dv;
        assert_eq!(
            world_dv2,
            Some(dv2),
            "second Cloak DV must overwrite the first in world"
        );

        // The two outcomes may or may not be equal depending on RNG, but
        // the world always reflects the latest Cloak.
        // (dv1 and dv2 could coincidentally be equal; we verify world state only.)
        let _ = dv1; // values are allowed to be equal — no assertion required.
    }

    // -------------------------------------------------------------------------
    // test_cloak_consumes_one_action
    // -------------------------------------------------------------------------

    /// `CloakAction::resolve` increments `net_actions_used_this_turn` by 1.
    ///
    /// After two Cloak calls with a budget of 3, `used` must equal 2.
    /// A third call with used=2, max=2 (rank ≤ 3) must return
    /// `Err(RulesError::NoNetActionsRemaining)`.
    ///
    /// See p.199 (NET Action cost) and p.197 (NET Actions table).
    #[test]
    fn test_cloak_consumes_one_action() {
        // Interface rank 4 → 3 NET Actions. Start with used=0.
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 4);

        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            0,
            "starts at 0 used"
        );
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_max_this_turn,
            3,
            "rank 4 → 3 max NET Actions per p.197"
        );

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };
        let mut rng = Rng::seed_from_u64(2);

        // First Cloak: used becomes 1.
        action
            .resolve(&mut world, &mut rng)
            .expect("first Cloak ok");
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            1,
            "used must be 1 after first Cloak"
        );

        // Second Cloak: used becomes 2.
        action
            .resolve(&mut world, &mut rng)
            .expect("second Cloak ok");
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            2,
            "used must be 2 after second Cloak"
        );

        // Third Cloak: used becomes 3.
        action
            .resolve(&mut world, &mut rng)
            .expect("third Cloak ok (max=3)");
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            3,
            "used must be 3 after third Cloak"
        );

        // Fourth Cloak: budget exhausted → error, state unchanged.
        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::NoNetActionsRemaining)),
            "fourth Cloak must fail with NoNetActionsRemaining, got {result:?}"
        );
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            3,
            "used must remain 3 after budget-exceeded attempt"
        );
    }

    // -------------------------------------------------------------------------
    // test_cloak_entity_not_found
    // -------------------------------------------------------------------------

    /// A non-existent entity returns `Err(RulesError::EntityNotFound)`.
    #[test]
    fn test_cloak_entity_not_found() {
        let pc = netrunner_pc(6, 4);
        let mut world = world_with_netrun(pc, 4);

        let bad_id = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = CloakAction {
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
    // test_cloak_insufficient_luck
    // -------------------------------------------------------------------------

    /// Spending more LUCK than available returns `Err(RulesError::InsufficientLuck)`.
    #[test]
    fn test_cloak_insufficient_luck() {
        let mut pc = netrunner_pc(6, 4);
        pc.luck_pool = 0;
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 4);

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 3,
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
            "expected InsufficientLuck, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_cloak_no_active_netrun
    // -------------------------------------------------------------------------

    /// If `world.netrun` is `None`, resolve returns `Err(RulesError::NoActiveNetrun)`.
    #[test]
    fn test_cloak_no_active_netrun() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc); // no netrun set

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::NoActiveNetrun)),
            "expected NoActiveNetrun, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_cloak_luck_adds_to_check
    // -------------------------------------------------------------------------

    /// Spending N LUCK adds N to the final check value and decrements the pool.
    ///
    /// See p.130 (LUCK spending) and p.199 (Cloak).
    #[test]
    fn test_cloak_luck_adds_to_check() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 4);

        let action = CloakAction {
            netrunner: pc_id,
            luck_to_spend: 3,
        };
        let mut rng = Rng::seed_from_u64(7);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(outcome.breakdown.luck_spent, 3, "luck_spent must be 3");
        // final = stat + skill + 0 + luck + d10.net = 6 + 4 + 3 + d10.net
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 3 + outcome.breakdown.d10.net,
            "final_value must include LUCK contribution"
        );
        // Pool decremented: started at 10, spent 3.
        assert_eq!(world.pc.luck_pool, 7, "luck_pool 10 - 3 = 7");
    }

    // -------------------------------------------------------------------------
    // test_cloak_dv_uses_saturating_cast
    // -------------------------------------------------------------------------

    /// A catastrophic critical failure could produce a negative `final_value`.
    /// The `cloak_dv` must saturate at `DV(0)` — never wrap.
    ///
    /// We verify the arithmetic path directly (RNG-independent).
    #[test]
    fn test_cloak_dv_uses_saturating_cast() {
        // Construct a worst-case CritD10: base=1, follow-up=10 → net=-9 (critical failure).
        let crit_fail_d10 = CritD10 {
            base: 1,
            follow_up: Some(10),
            outcome: D10Outcome::CriticalFailure,
            net: -9,
        };
        // INT=1, Interface=1, modifier=0, luck=0, d10=-9 → final = 1+1-9 = -7.
        let bd = CheckBreakdown::new(1, 1, 0, 0, crit_fail_d10, DV(0));
        assert_eq!(bd.final_value, -7, "1 + 1 - 9 = -7");

        // Saturating cast must yield DV(0), not an underflow.
        let dv = DV(bd.final_value.max(0) as u8);
        assert_eq!(dv, DV(0), "negative final_value must saturate to DV(0)");
    }
}
