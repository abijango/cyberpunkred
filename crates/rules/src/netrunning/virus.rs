//! Interface Ability: Virus deployment — install a persistent Virus at the
//! bottom floor of a NET Architecture so its effect survives jack-out.
//!
//! ## Rulebook (p.200)
//!
//! > **Virus** — Once you have reached the lowest level of the NET Architecture
//! > you can leave your own Virus in the Architecture. Describe to the GM what
//! > you want the virus to do. Depending on what you want to do, this can require
//! > as many NET Actions as the GM determines. A more powerful Virus will require
//! > a higher DV to leave in the Architecture.
//! >
//! > Using this ability is the only way a Netrunner can make a change to a NET
//! > Architecture that persists after they Jack Out.
//!
//! ## Key rules notes
//!
//! - **Bottom floor required.** The Netrunner must be at the deepest revealed
//!   floor of the NET Architecture before deploying a Virus (p.200). Attempting
//!   to deploy from any other floor returns
//!   [`RulesError::NotOnBottomFloor`].
//!
//! - **Roll formula** (p.199): `INT + Interface_rank + 1d10 vs virus.dv_to_install`.
//!   "Interface" is the Netrunner Role Ability rank (`character.role_rank`).
//!
//! - **NET Actions consumed** equals `virus.net_actions_to_install` (set by the
//!   GM, p.200). The action block spends these before the roll — if the
//!   Netrunner cannot cover the cost, deployment is rejected with
//!   [`RulesError::NoNetActionsRemaining`].
//!
//! - **On success**, the Virus is pushed to
//!   [`NetrunState::queued_viruses`][crate::netrunning::state::NetrunState::queued_viruses].
//!   The queued list is drained on jack-out by
//!   [`NetrunState::drain_viruses_for_jackout`][crate::netrunning::state::NetrunState::drain_viruses_for_jackout],
//!   which the jack-out resolution code (WP-417+) calls when the Netrunner
//!   exits from the bottom floor. The effect then persists in the Architecture
//!   state.
//!
//! - **On failure**, the Virus is discarded and the NET Actions are still
//!   consumed (p.200 does not grant a refund on a failed install).
//!
//! See p.200.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::state::Virus;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::EntityId;
use crate::world::World;

// ---------------------------------------------------------------------------
// Action
// ---------------------------------------------------------------------------

/// A Virus deployment NET Action — install a [`Virus`] at the bottom floor
/// of the active NET Architecture.
///
/// The Netrunner must be at the deepest revealed floor (`current_floor + 1 ==
/// revealed_floors`) before this action is valid (p.200). The roll is
/// `INT + Interface_rank + 1d10` vs [`Virus::dv_to_install`]. On success the
/// Virus is pushed onto [`NetrunState::queued_viruses`]; it persists in the
/// Architecture after jack-out when drained by
/// [`NetrunState::drain_viruses_for_jackout`].
///
/// See p.200 (Virus Interface Ability).
#[derive(Clone, Debug, PartialEq)]
pub struct DeployVirusAction {
    /// The Netrunner performing the deployment.
    pub netrunner: EntityId,
    /// The Virus to install. Carries the DV and NET-Action cost.
    pub virus: Virus,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// Outcome of a [`DeployVirusAction`].
///
/// `breakdown` is always populated (it records the Interface + d10 roll).
/// `installed` is `true` only when the check succeeded. `net_actions_consumed`
/// is the number of NET Actions the action spent regardless of success or
/// failure.
///
/// See p.200 (Virus Interface Ability).
#[derive(Clone, Debug, PartialEq)]
pub struct VirusDeploymentOutcome {
    /// Full breakdown of the `INT + Interface_rank + d10` roll vs the Virus DV.
    pub breakdown: CheckBreakdown,
    /// Whether the Virus was successfully installed.
    ///
    /// `true` iff `breakdown.success == true`. On `true`, the Virus has been
    /// pushed to [`NetrunState::queued_viruses`] and will persist in the
    /// Architecture after jack-out.
    pub installed: bool,
    /// Number of NET Actions consumed by this deployment attempt.
    ///
    /// Always equals [`Virus::net_actions_to_install`] for the submitted
    /// virus, regardless of success or failure.
    pub net_actions_consumed: u8,
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for DeployVirusAction {
    /// `Result` so pre-condition failures (floor check, entity lookup,
    /// insufficient LUCK, no NET Actions remaining) can short-circuit without
    /// rolling.
    type Outcome = Result<VirusDeploymentOutcome, RulesError>;

    /// Resolve the Virus deployment NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Check `world.netrun` — return [`RulesError::NetrunNotActive`] if
    ///    absent.
    /// 2. Verify the Netrunner is on the bottom floor. The bottom floor is
    ///    `revealed_floors - 1` in [`NetrunState`]. If `current_floor !=
    ///    revealed_floors - 1`, return [`RulesError::NotOnBottomFloor`].
    /// 3. Verify enough NET Actions remain for `virus.net_actions_to_install`.
    ///    Return [`RulesError::NoNetActionsRemaining`] if the cost exceeds what
    ///    is left.
    /// 4. Look up the Netrunner via `world.entity_mut`. Return
    ///    [`RulesError::EntityNotFound`] if absent.
    /// 5. Validate and spend LUCK via `actor.spend_luck(self.luck_to_spend)`.
    ///    Return [`RulesError::InsufficientLuck`] on failure.
    /// 6. Capture `INT` and `role_rank` from the Netrunner.
    /// 7. Roll `d10_with_crits(rng)`.
    /// 8. Build the [`CheckBreakdown`] from `INT + role_rank + d10 vs
    ///    virus.dv_to_install`.
    /// 9. Spend the NET Actions on `world.netrun`.
    /// 10. On success, push the Virus to `world.netrun.queued_viruses`.
    ///
    /// See p.200 (Virus Interface Ability) and p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — require an active netrun. See p.200.
        let netrun = world.netrun.as_ref().ok_or(RulesError::NetrunNotActive)?;

        // Step 2 — must be on the bottom floor. See p.200.
        // Bottom floor = deepest revealed floor = revealed_floors - 1.
        let bottom_floor = netrun.revealed_floors.saturating_sub(1);
        if netrun.current_floor != bottom_floor {
            return Err(RulesError::NotOnBottomFloor {
                current_floor: netrun.current_floor,
                bottom_floor,
            });
        }

        // Step 3 — check NET Action budget. See p.200 / p.197.
        // The Virus costs net_actions_to_install NET Actions regardless of success.
        let actions_used = netrun.net_actions_used_this_turn;
        let actions_max = netrun.net_actions_max_this_turn;
        let actions_available = actions_max.saturating_sub(actions_used);
        if self.virus.net_actions_to_install > actions_available {
            return Err(RulesError::NoNetActionsRemaining);
        }

        // Step 4 — look up the Netrunner entity (mutable for LUCK spend).
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 5 — validate and spend LUCK before rolling. See p.130.
        actor.spend_luck(self.luck_to_spend)?;

        // Step 6 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198: "Interface is the Netrunner
        // Role Ability"). There is no `SkillId::Interface` in the closed enum.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 7 — roll with crit rules. See p.129–130.
        let d10 = d10_with_crits(rng);

        // Step 8 — build the breakdown. See p.200.
        let breakdown = CheckBreakdown::new(
            int,
            interface_rank,
            0,
            self.luck_to_spend,
            d10,
            self.virus.dv_to_install,
        );

        let installed = breakdown.success;
        let net_actions_consumed = self.virus.net_actions_to_install;

        // Step 9 — spend the NET Actions.
        // SAFETY: we checked the budget above; this will not overflow.
        let netrun = world.netrun.as_mut().expect("checked in step 1");
        netrun.net_actions_used_this_turn = netrun
            .net_actions_used_this_turn
            .saturating_add(net_actions_consumed);

        // Step 10 — on success, queue the Virus for jack-out persistence. See p.200.
        if installed {
            netrun.queue_virus(self.virus.clone());
        }

        Ok(VirusDeploymentOutcome {
            breakdown,
            installed,
            net_actions_consumed,
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
    use crate::netrunning::state::{NetrunState, VirusEffect};
    use crate::types::{EntityId, DV};
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

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

    /// Build a simple Virus suitable for tests.
    fn simple_virus(dv: u8, actions: u8) -> Virus {
        Virus {
            description: "test virus".into(),
            effect: VirusEffect::AlterIcon("asp".into()),
            dv_to_install: DV(dv),
            net_actions_to_install: actions,
        }
    }

    /// Build a `CritD10` with a known net value (non-crit d10 of `roll`).
    fn fake_d10_net(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    /// Walk seeds until we find one whose `d10_with_crits` net == `target`.
    fn seed_with_d10_net(target: i16) -> u64 {
        use crate::dice::d10_with_crits;
        for seed in 0..2_000_000u64 {
            let mut r = crate::rng::Rng::seed_from_u64(seed);
            if d10_with_crits(&mut r).net == target {
                return seed;
            }
        }
        panic!("no seed found with d10 net = {target}");
    }

    /// Construct a world with a Netrunner PC and a pre-configured [`NetrunState`]
    /// where `current_floor == revealed_floors - 1` (i.e., on the bottom floor).
    fn world_at_bottom(int: u8, role_rank: u8, revealed_floors: usize) -> (World, EntityId) {
        let pc = netrunner_pc(int, role_rank);
        let entity_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let arch = NetArchId("test-arch".into());
        let mut state = NetrunState::start(entity_id, arch, role_rank);
        state.revealed_floors = revealed_floors;
        state.current_floor = revealed_floors.saturating_sub(1); // bottom floor
                                                                 // Give plenty of NET Actions for most tests.
        state.net_actions_max_this_turn = 10;
        state.net_actions_used_this_turn = 0;

        world.netrun = Some(state);
        (world, entity_id)
    }

    // -------------------------------------------------------------------------
    // test_deploy_virus_succeeds_on_bottom_floor
    // -------------------------------------------------------------------------

    /// Deploy a Virus with a roll that beats the DV.
    ///
    /// Verified against p.200: roll must beat `dv_to_install`. On success the
    /// Virus appears in `queued_viruses`. `installed == true`.
    ///
    /// See p.200.
    #[test]
    fn test_deploy_virus_succeeds_on_bottom_floor() {
        // INT 6, Interface 4 → pool = 10.
        // Use DV 6 and a forced d10 net of 5 → final = 6+4+5 = 15 ≥ 6 → success.
        let seed = seed_with_d10_net(5);
        let (mut world, entity_id) = world_at_bottom(6, 4, 3);

        let virus = simple_virus(6, 1);
        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: virus.clone(),
            luck_to_spend: 0,
        };

        let mut rng = crate::rng::Rng::seed_from_u64(seed);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("deploy must succeed");

        assert!(
            outcome.installed,
            "virus must be marked installed on success"
        );
        assert_eq!(
            outcome.net_actions_consumed, 1,
            "consumed exactly 1 NET Action"
        );
        assert!(outcome.breakdown.success, "breakdown must report success");

        // Virus must be in the queued list.
        let netrun = world.netrun.as_ref().expect("netrun still active");
        assert_eq!(netrun.queued_viruses.len(), 1, "one virus queued");
        assert_eq!(
            netrun.queued_viruses[0], virus,
            "queued virus matches the deployed one"
        );
    }

    // -------------------------------------------------------------------------
    // test_deploy_virus_fails_not_on_bottom
    // -------------------------------------------------------------------------

    /// Deploying a Virus when not on the bottom floor must return
    /// [`RulesError::NotOnBottomFloor`] — no dice roll, no state mutation.
    ///
    /// Per p.200: "Once you have reached the lowest level…"
    ///
    /// See p.200.
    #[test]
    fn test_deploy_virus_fails_not_on_bottom() {
        // INT 6, Interface 4.
        let pc = netrunner_pc(6, 4);
        let entity_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let arch = NetArchId("test-arch".into());
        let mut state = NetrunState::start(entity_id, arch, 4);
        // 4 floors revealed; place the Netrunner at floor 1 (not the bottom = 3).
        state.revealed_floors = 4;
        state.current_floor = 1; // not the bottom (bottom is 3)
        state.net_actions_max_this_turn = 10;
        world.netrun = Some(state);

        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: simple_virus(6, 1),
            luck_to_spend: 0,
        };

        let mut rng = crate::rng::Rng::seed_from_u64(42);
        let mut probe = crate::rng::Rng::seed_from_u64(42);
        let err = action
            .resolve(&mut world, &mut rng)
            .expect_err("must fail when not on bottom floor");

        // Correct error variant.
        assert!(
            matches!(
                err,
                RulesError::NotOnBottomFloor {
                    current_floor: 1,
                    bottom_floor: 3
                }
            ),
            "expected NotOnBottomFloor(current=1, bottom=3), got {err:?}"
        );

        // RNG must not have advanced (no roll taken).
        use crate::dice::d10_with_crits;
        assert_eq!(
            d10_with_crits(&mut rng).net,
            d10_with_crits(&mut probe).net,
            "RNG must not advance on a floor check failure"
        );

        // No viruses queued.
        let netrun = world.netrun.as_ref().unwrap();
        assert!(
            netrun.queued_viruses.is_empty(),
            "no virus queued on failure"
        );
    }

    // -------------------------------------------------------------------------
    // test_deploy_consumes_actions
    // -------------------------------------------------------------------------

    /// Virus deployment must consume exactly `virus.net_actions_to_install` NET
    /// Actions, both on success and on failure.
    ///
    /// See p.200 (the book does not grant a refund on a failed install) and p.197
    /// (NET Actions per turn).
    #[test]
    fn test_deploy_consumes_actions() {
        // --- Success path: DV 1 (auto-succeed) with 2-action virus.
        let (mut world_s, entity_id) = world_at_bottom(6, 4, 3);
        let virus_2 = simple_virus(1, 2); // DV 1 → always success; 2 NET Actions
        let action_s = DeployVirusAction {
            netrunner: entity_id,
            virus: virus_2,
            luck_to_spend: 0,
        };
        let mut rng_s = crate::rng::Rng::seed_from_u64(0);
        let outcome_s = action_s.resolve(&mut world_s, &mut rng_s).unwrap();
        assert_eq!(
            outcome_s.net_actions_consumed, 2,
            "success: consumed 2 actions"
        );
        assert_eq!(
            world_s.netrun.as_ref().unwrap().net_actions_used_this_turn,
            2,
            "state must reflect 2 actions consumed on success"
        );

        // --- Failure path: DV 255 (auto-fail), still consumes actions.
        // Find a seed that produces a net < 1 (so INT=1, Interface=1 + d10 < 255 always fails).
        let (mut world_f, entity_id_f) = world_at_bottom(1, 1, 3);
        let virus_f = simple_virus(255, 1); // DV 255 → always fails; 1 NET Action
        let action_f = DeployVirusAction {
            netrunner: entity_id_f,
            virus: virus_f,
            luck_to_spend: 0,
        };
        let mut rng_f = crate::rng::Rng::seed_from_u64(0);
        let outcome_f = action_f.resolve(&mut world_f, &mut rng_f).unwrap();
        assert!(!outcome_f.installed, "DV 255 must fail to install");
        assert_eq!(
            outcome_f.net_actions_consumed, 1,
            "failure: still consumed 1 action"
        );
        assert_eq!(
            world_f.netrun.as_ref().unwrap().net_actions_used_this_turn,
            1,
            "state must reflect 1 action consumed on failure"
        );
        // No virus queued on failure.
        assert!(
            world_f.netrun.as_ref().unwrap().queued_viruses.is_empty(),
            "no virus queued on a failed install"
        );
    }

    // -------------------------------------------------------------------------
    // test_virus_drained_on_jackout
    // -------------------------------------------------------------------------

    /// After deploying (and queuing) a Virus, `drain_viruses_for_jackout` returns
    /// it and leaves the queue empty — the standard jack-out persistence path.
    ///
    /// This test covers the full lifecycle: deploy → queue → drain.
    ///
    /// See p.200 (Virus persists after Jack Out) and WP-402 drain API.
    #[test]
    fn test_virus_drained_on_jackout() {
        // INT 8, Interface 6, DV 1 → guaranteed success for any d10 ≥ 0.
        let (mut world, entity_id) = world_at_bottom(8, 6, 3);

        let virus = Virus {
            description: "Change all passwords every 5 minutes".into(),
            effect: VirusEffect::Custom("rotate passwords".into()),
            dv_to_install: DV(1),
            net_actions_to_install: 1,
        };

        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: virus.clone(),
            luck_to_spend: 0,
        };

        let mut rng = crate::rng::Rng::seed_from_u64(0);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("deploy must succeed with DV 1");
        assert!(outcome.installed, "virus must be installed");

        // Queue should hold exactly the deployed virus.
        let netrun = world.netrun.as_mut().expect("netrun active");
        assert_eq!(
            netrun.queued_viruses.len(),
            1,
            "one virus in queue before drain"
        );

        // Jack Out — drain the queue.
        let drained = netrun.drain_viruses_for_jackout();
        assert_eq!(drained.len(), 1, "drain must return exactly 1 virus");
        assert_eq!(
            drained[0], virus,
            "drained virus must match the deployed one"
        );
        assert!(
            netrun.queued_viruses.is_empty(),
            "queue must be empty after drain"
        );
    }

    // -------------------------------------------------------------------------
    // test_deploy_no_net_actions_remaining
    // -------------------------------------------------------------------------

    /// When the Netrunner has no NET Actions left, deployment is rejected.
    ///
    /// See p.197 (NET Actions per turn table) and p.200 (Virus costs NET Actions).
    #[test]
    fn test_deploy_no_net_actions_remaining() {
        let (mut world, entity_id) = world_at_bottom(6, 4, 3);
        // Drain all actions.
        let netrun = world.netrun.as_mut().unwrap();
        netrun.net_actions_used_this_turn = netrun.net_actions_max_this_turn;

        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: simple_virus(6, 1),
            luck_to_spend: 0,
        };
        let mut rng = crate::rng::Rng::seed_from_u64(0);
        let err = action
            .resolve(&mut world, &mut rng)
            .expect_err("must fail when no NET Actions remain");
        assert!(
            matches!(err, RulesError::NoNetActionsRemaining),
            "expected NoNetActionsRemaining, got {err:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_deploy_netrun_not_active
    // -------------------------------------------------------------------------

    /// When no netrun is active, deployment is rejected immediately.
    ///
    /// See p.198 (Jack In required before Interface Abilities).
    #[test]
    fn test_deploy_netrun_not_active() {
        let pc = netrunner_pc(6, 4);
        let entity_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        // world.netrun is None by default.

        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: simple_virus(6, 1),
            luck_to_spend: 0,
        };
        let mut rng = crate::rng::Rng::seed_from_u64(0);
        let err = action
            .resolve(&mut world, &mut rng)
            .expect_err("must fail when not jacked in");
        assert!(
            matches!(err, RulesError::NetrunNotActive),
            "expected NetrunNotActive, got {err:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_deploy_uses_int_and_interface_formula
    // -------------------------------------------------------------------------

    /// Verify the roll formula is `INT + Interface_rank + d10 vs DV`.
    ///
    /// See p.199 (Interface check formula) and p.200 (Virus DV).
    #[test]
    fn test_deploy_uses_int_and_interface_formula() {
        // INT 7, Interface 3, d10 net 5 → final = 15. DV 13 → success.
        let breakdown = CheckBreakdown::new(7, 3, 0, 0, fake_d10_net(5), DV(13));
        assert_eq!(breakdown.stat_value, 7, "stat_value must be INT");
        assert_eq!(
            breakdown.skill_value, 3,
            "skill_value must be Interface rank"
        );
        assert_eq!(breakdown.final_value, 15, "INT(7)+Interface(3)+d10(5)=15");
        assert!(breakdown.success, "15 ≥ DV(13) → success");

        // Integration: resolve and inspect the breakdown columns.
        let (mut world, entity_id) = world_at_bottom(7, 3, 2);
        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: simple_virus(13, 1), // DV 13
            luck_to_spend: 0,
        };
        let seed = seed_with_d10_net(5);
        let mut rng = crate::rng::Rng::seed_from_u64(seed);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(outcome.breakdown.stat_value, 7, "stat_value == INT(7)");
        assert_eq!(
            outcome.breakdown.skill_value, 3,
            "skill_value == Interface(3)"
        );
        assert_eq!(outcome.breakdown.final_value, 7 + 3 + 5, "7+3+5=15");
        assert!(outcome.installed, "15 ≥ DV(13) → installed");
    }

    // -------------------------------------------------------------------------
    // test_deploy_luck_adds_to_check
    // -------------------------------------------------------------------------

    /// Spending LUCK adds it to the final check value and debits the pool.
    ///
    /// See p.130 (Using Your LUCK).
    #[test]
    fn test_deploy_luck_adds_to_check() {
        // INT 6, Interface 4, DV 20. With d10 net 5 and 0 LUCK: final=15 < 20.
        // With 5 LUCK: final = 6+4+5+5 = 20 ≥ 20 → success.
        let seed = seed_with_d10_net(5);
        let (mut world, entity_id) = world_at_bottom(6, 4, 2);

        let action = DeployVirusAction {
            netrunner: entity_id,
            virus: simple_virus(20, 1),
            luck_to_spend: 5,
        };
        let mut rng = crate::rng::Rng::seed_from_u64(seed);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(outcome.breakdown.luck_spent, 5, "5 LUCK spent");
        // final = 6 + 4 + 5(luck) + 5(d10) = 20
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 5 + 5,
            "LUCK adds to final"
        );
        assert!(outcome.installed, "20 ≥ DV(20) → installed");
        // LUCK pool debited.
        let remaining = world.entity(entity_id).unwrap().luck_remaining();
        assert_eq!(remaining, 10 - 5, "luck pool debited by 5");
    }
}
