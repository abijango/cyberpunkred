//! Interface Ability: Backdoor — bypass a Password floor.
//!
//! ## Rulebook (p.199)
//!
//! > **Backdoor** — Use a NET Action to make an Interface Check (DV equal to
//! > the DV of the Password) to get past a Password in a NET Architecture.
//! > On a success, you bypass the password and move on to the next floor.
//!
//! ## Key rules notes
//!
//! - **NET Action**, not a Meat Action (p.198). Consumes one of the
//!   Netrunner's NET Action slots for the turn.
//! - Roll formula: `INT + Interface_rank + 1d10` vs. the Password floor's DV.
//!   "Interface" is the Netrunner Role Ability rank (`character.role_rank`).
//!   There is no `SkillId::Interface` in the closed enum — the role-ability
//!   rank plays the role of the "skill" column (same as Scanner, p.199).
//! - The current floor **must** be a `Floor::Password`. If it is any other
//!   floor type (BlackIce, ControlNode, File, Demon), `Err(WrongFloorType)`
//!   is returned without consuming an action.
//! - On success: `current_floor` advances by 1 (entering the next floor).
//! - On failure: `current_floor` stays. The Netrunner may attempt again on a
//!   later turn (there is no mention of a restriction on re-tries in RAW).
//! - Validates that the Netrunner has at least one NET Action remaining before
//!   resolving; returns `Err(NoNetActionsRemaining)` if not.
//! - Requires an active netrun (`world.netrun.is_some()`); returns
//!   `Err(NoActiveNetrun)` otherwise.
//!
//! ## Architecture lookup
//!
//! `world.netrun` holds the [`NetrunState`] which records `current_floor` and
//! the `architecture` id. We currently look up the floor from
//! `world.netrun.architecture` via the `architecture_floors` field embedded in
//! [`NetrunState`] … however that is not yet available. Per WP-402, the
//! `NetrunState` does **not** embed the architecture floors — only the
//! [`NetArchId`]. The GM/scene layer is responsible for resolving
//! `NetArchId → NetArchitecture`. Since Backdoor is purely a rules-layer WP,
//! this implementation takes the architecture as a direct argument to
//! `resolve_with_arch` and also provides a `resolve` that expects
//! `world.netrun_arch` to be set. To remain self-contained within the rules
//! crate (no GM layer dependency) we store the [`NetArchitecture`] inline on
//! the World when a netrun is active.
//!
//! **Design decision:** `World` does not yet have a `netrun_arch` field.
//! Rather than modifying `World` (which would touch a shared crate and require
//! cross-WP coordination), we embed the needed architecture slice directly in
//! the `BackdoorAction`. The caller (GM layer / test) provides the slice of
//! floors so the resolution can extract the current floor's DV. This is
//! consistent with how p.199 specifies the DV — it comes from the floor the
//! Netrunner is currently on, and the GM layer has that information.
//!
//! The `floors` field on `BackdoorAction` is `&[Floor]` passed at call time.
//! This is a deliberate pragmatic deviation from the strict `Resolution` trait
//! signature (which takes only `&mut World` and `&mut Rng`). To bridge this,
//! `BackdoorAction` stores the floors as an owned `Vec<Floor>` so the impl
//! can be carried to the trait call site. See the module doc for the
//! deviation note.
//!
//! See p.199 (Backdoor), p.198 (NET Actions budget), p.197 (NET Actions
//! per turn table).
//!
//! [`NetrunState`]: crate::netrunning::state::NetrunState
//! [`NetArchId`]: crate::netrunning::architecture::NetArchId
//! [`NetArchitecture`]: crate::netrunning::architecture::NetArchitecture

// See p.199.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::architecture::Floor;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

/// A **NET Action** Backdoor check. See p.199.
///
/// The netrunner rolls `INT + Interface_rank + 1d10` vs. the current floor's
/// Password DV. On success `current_floor` advances by 1; on failure the
/// floor is unchanged.
///
/// ## Caller responsibilities
///
/// - `floors` must be the full ordered floor list of the NET Architecture
///   currently being run (same slice that was passed to
///   [`NetrunState`][crate::netrunning::state::NetrunState] at jack-in time).
///   The action reads `floors[netrun.current_floor]`.
/// - `world.netrun` must be `Some`. If it is `None` the resolve returns
///   `Err(RulesError::NoActiveNetrun)`.
/// - The netrunner entity (`self.netrunner`) must resolve in `world`.
///
/// ## Deviations from the public `Resolution` trait
///
/// The strict `Resolution::resolve(&self, world, rng)` signature does not
/// carry the architecture floors (they live in the GM layer). To keep this
/// WP within `crates/rules`, the caller embeds the relevant floors as
/// `Vec<Floor>` directly on the action struct. The GM layer (a later WP)
/// will wrap this cleanly once the architecture registry is wired in.
///
/// See p.199 (Backdoor), p.198 (NET Actions).
#[derive(Clone, Debug, PartialEq)]
pub struct BackdoorAction {
    /// The netrunner performing the Backdoor. Must exist in `world`.
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
    /// The ordered floors of the active NET Architecture. The action reads
    /// `floors[world.netrun.current_floor]`.
    ///
    /// This field bridges the rules/GM layer boundary for this WP. A later
    /// WP will replace it with a registry lookup. See module doc.
    pub floors: Vec<Floor>,
}

/// Outcome of a [`BackdoorAction`].
///
/// `breakdown` is always populated (success or failure). `passed` mirrors
/// `breakdown.success` and is the primary branch signal for callers.
///
/// ## State mutations on success
///
/// When `passed == true`, `world.netrun.current_floor` has been incremented
/// by 1 (the netrunner is now on the next floor). On failure, the floor is
/// unchanged. In both cases, `net_actions_used_this_turn` is incremented by 1.
///
/// See p.199 (Backdoor).
#[derive(Clone, Debug, PartialEq)]
pub struct BackdoorOutcome {
    /// Full breakdown of the Interface + d10 roll, including margin vs. DV.
    pub breakdown: CheckBreakdown,
    /// `true` iff the Password DV was met or exceeded (i.e. the floor was
    /// bypassed). Mirrors `breakdown.success`.
    pub passed: bool,
}

impl Resolution for BackdoorAction {
    type Outcome = Result<BackdoorOutcome, RulesError>;

    /// Resolve the Backdoor NET Action against `world`.
    ///
    /// ## Steps (p.199)
    ///
    /// 1. Verify `world.netrun` is `Some` → `Err(NoActiveNetrun)` otherwise.
    /// 2. Validate the netrunner has at least one NET Action remaining →
    ///    `Err(NoNetActionsRemaining)` otherwise.
    /// 3. Look up the netrunner entity → `Err(EntityNotFound)` if missing.
    /// 4. Validate and spend luck (`actor.spend_luck`) → `Err(InsufficientLuck)`.
    /// 5. Capture `INT` and `role_rank` (Interface rank) from the netrunner.
    /// 6. Read `world.netrun.current_floor`; look up `self.floors[current_floor]`.
    ///    - Must be `Floor::Password { dv }` → `Err(WrongFloorType)` otherwise.
    /// 7. Roll `d10_with_crits(rng)`.
    /// 8. Build `CheckBreakdown::new(int, role_rank, 0, luck_spent, d10, dv)`.
    /// 9. Consume one NET Action (`netrun.net_actions_used_this_turn += 1`).
    /// 10. On success: `netrun.current_floor += 1`. On failure: unchanged.
    /// 11. Return `Ok(BackdoorOutcome { breakdown, passed: breakdown.success })`.
    ///
    /// See p.199 (Backdoor), p.130 (LUCK), p.197–198 (NET Actions budget).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — verify an active netrun exists. See p.198 (Jack In/Out).
        let netrun = world.netrun.as_ref().ok_or(RulesError::NoActiveNetrun)?;

        // Step 2 — check NET Action budget. See p.197 (NET Actions table).
        let actions_remaining = netrun
            .net_actions_max_this_turn
            .saturating_sub(netrun.net_actions_used_this_turn);
        if actions_remaining == 0 {
            return Err(RulesError::NoNetActionsRemaining);
        }

        // Capture current floor index before we release the borrow.
        let current_floor = netrun.current_floor;

        // Step 6 — look up current floor type. See p.199 (Backdoor).
        let dv: DV = match self.floors.get(current_floor) {
            Some(Floor::Password { dv }) => *dv,
            Some(Floor::BlackIce { .. }) => {
                return Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "BlackIce",
                });
            }
            Some(Floor::ControlNode { .. }) => {
                return Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "ControlNode",
                });
            }
            Some(Floor::File { .. }) => {
                return Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "File",
                });
            }
            Some(Floor::Demon { .. }) => {
                return Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "Demon",
                });
            }
            None => {
                return Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "out-of-bounds floor index",
                });
            }
        };

        // Step 3 — look up the entity. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 4 — validate and spend luck (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 5 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198: "Interface is the Netrunner
        // Role Ability"). There is no `SkillId::Interface` in the closed
        // enum — the role-ability rank plays the role of the "skill" column.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 7 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 8 — build the breakdown. See p.199.
        // stat_value    = INT (the STAT linked to Interface checks)
        // skill_value   = Interface Role Ability rank (plays the skill column)
        // modifier_total = 0 (no situational modifiers in base Backdoor)
        let breakdown = CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, dv);
        let passed = breakdown.success;

        // Step 9 — consume one NET Action. See p.197–198.
        {
            let netrun = world.netrun.as_mut().expect("netrun was Some above");
            netrun.net_actions_used_this_turn += 1;

            // Step 10 — advance floor on success. See p.199.
            if passed {
                netrun.current_floor += 1;
            }
        }

        Ok(BackdoorOutcome { breakdown, passed })
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

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a `CritD10` with a known net value, simulating a normal (non-crit)
    /// roll of `roll`. Used to make RNG-independent assertions.
    fn fake_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    /// Build a fresh Netrunner PC with specific INT and role_rank.
    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// Build a minimal `World` with an active netrun. `current_floor = 0`.
    ///
    /// Floors live on the `BackdoorAction`, not in the World — the architecture
    /// registry is a GM-layer concern. This helper only wires up `NetrunState`.
    fn world_in_netrun(pc: crate::character::Character, net_actions_max: u8) -> (World, EntityId) {
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        // Interface rank 5 → max 3 NET actions; override below.
        let mut state = NetrunState::start(pc_id, NetArchId("test-arch".into()), 5);
        state.net_actions_max_this_turn = net_actions_max;
        state.net_actions_used_this_turn = 0;
        world.netrun = Some(state);
        (world, pc_id)
    }

    // -------------------------------------------------------------------------
    // test_backdoor_passes_password_floor
    // -------------------------------------------------------------------------

    /// A forced high d10 must produce `passed=true` and advance `current_floor`
    /// by 1. See p.199 (Backdoor success).
    ///
    /// Setup: INT=6, Interface=4, Password DV=8.
    /// We need d10 such that 6+4+d10 >= 8, i.e. d10 >= -2 (always true for
    /// positive d10). We pick a seed that gives a positive d10 to guarantee
    /// success against DV 8.
    #[test]
    fn test_backdoor_passes_password_floor() {
        let pc = netrunner_pc(6, 4);
        let floors = vec![
            Floor::Password { dv: DV(8) },
            Floor::File {
                dv: DV(8),
                contents: crate::netrunning::architecture::FileContents::Data("Prize".to_string()),
            },
        ];
        let (world, pc_id) = world_in_netrun(pc, 3);

        // With INT=6 and Interface=4 against DV=8, any d10 >= -2 succeeds.
        // Seed 0 reliably produces a non-negative d10.
        let mut found = false;
        for seed in 0u64..100 {
            // Reset world for each attempt.
            let pc2 = netrunner_pc(6, 4);
            let (mut w2, id2) = world_in_netrun(pc2, 3);
            let mut rng = Rng::seed_from_u64(seed);
            let action = BackdoorAction {
                netrunner: id2,
                luck_to_spend: 0,
                floors: floors.clone(),
            };
            let outcome = action.resolve(&mut w2, &mut rng).expect("should not error");
            if outcome.passed {
                assert!(
                    outcome.breakdown.success,
                    "passed must equal breakdown.success"
                );
                assert_eq!(
                    w2.netrun.as_ref().unwrap().current_floor,
                    1,
                    "current_floor must advance to 1 on success (seed {seed})"
                );
                assert_eq!(
                    w2.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "one action must be consumed"
                );
                found = true;
                break;
            }
        }

        // Also verify directly with a known-good breakdown (seed-independent).
        // INT=6, Interface=4, d10=5 → final=15 vs DV=8 → success.
        let _ = (world, pc_id); // suppress unused warning
        let d10 = fake_d10(5);
        let bd = CheckBreakdown::new(6, 4, 0, 0, d10, DV(8));
        assert!(bd.success, "INT(6)+Interface(4)+d10(5)=15 >= DV(8)");
        assert!(found, "no passing seed found in 100 attempts");
    }

    // -------------------------------------------------------------------------
    // test_backdoor_fails_high_dv
    // -------------------------------------------------------------------------

    /// A forced low d10 (against a high DV) must produce `passed=false` and
    /// leave `current_floor` unchanged. See p.199 (Backdoor failure).
    ///
    /// Setup: INT=1, Interface=1 vs DV=24 (Legendary). Max d10=10 → total=12 < 24.
    #[test]
    fn test_backdoor_fails_high_dv() {
        // Direct arithmetic: INT=1, Interface=1, d10=10 → total=12 < DV=24.
        let d10 = fake_d10(10);
        let bd = CheckBreakdown::new(1, 1, 0, 0, d10, DV(24));
        assert!(!bd.success, "1+1+10=12 < 24 → failure");

        let floors = vec![Floor::Password { dv: DV(24) }];

        // Find a seed that produces failure (any seed where the roll doesn't
        // explode enough to reach 24 - 2 = 22 on a single d10 is fine).
        let mut found_failure = false;
        for seed in 0u64..200 {
            let pc = netrunner_pc(1, 1);
            let (mut world, pc_id) = world_in_netrun(pc, 3);
            let mut rng = Rng::seed_from_u64(seed);
            let action = BackdoorAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                floors: floors.clone(),
            };
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("should not error");
            if !outcome.passed {
                assert!(
                    !outcome.breakdown.success,
                    "passed must equal breakdown.success"
                );
                assert_eq!(
                    world.netrun.as_ref().unwrap().current_floor,
                    0,
                    "current_floor must stay at 0 on failure (seed {seed})"
                );
                assert_eq!(
                    world.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "one action must be consumed even on failure"
                );
                found_failure = true;
                break;
            }
        }

        assert!(
            found_failure,
            "no failing seed found for INT=1 Interface=1 vs DV=24 in 200 attempts"
        );
    }

    // -------------------------------------------------------------------------
    // test_backdoor_rejects_non_password_floor
    // -------------------------------------------------------------------------

    /// If the current floor is not `Floor::Password`, Backdoor must return
    /// `Err(WrongFloorType)` without consuming a NET action. See p.199.
    #[test]
    fn test_backdoor_rejects_non_password_floor() {
        use crate::catalog::black_ice::BlackIceId;
        use crate::netrunning::architecture::BlackIceState;

        let floors_black_ice = vec![Floor::BlackIce {
            template: BlackIceId("hellhound".into()),
            state: BlackIceState::LyingInWait,
        }];

        let pc = netrunner_pc(6, 4);
        let (mut world, pc_id) = world_in_netrun(pc, 3);
        let mut rng = Rng::seed_from_u64(0);

        let action = BackdoorAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            floors: floors_black_ice,
        };

        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(
                result,
                Err(RulesError::WrongFloorType {
                    expected: "Password",
                    found: "BlackIce"
                })
            ),
            "expected WrongFloorType(Password, BlackIce), got {result:?}"
        );

        // No NET action should have been consumed on the error path. See p.199.
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            0,
            "WrongFloorType must not consume a NET action"
        );
    }

    // -------------------------------------------------------------------------
    // test_backdoor_consumes_one_action
    // -------------------------------------------------------------------------

    /// Backdoor must increment `net_actions_used_this_turn` by exactly 1 on
    /// both success and failure. See p.198 (NET Actions as the unit of cost).
    #[test]
    fn test_backdoor_consumes_one_action() {
        let floors = vec![
            Floor::Password { dv: DV(8) },
            Floor::File {
                dv: DV(8),
                contents: crate::netrunning::architecture::FileContents::Data("Data".to_string()),
            },
        ];

        // --- success path ---
        let pc_s = netrunner_pc(8, 8); // high stats to guarantee success vs DV 8
        let (mut world_s, id_s) = world_in_netrun(pc_s, 5);
        let mut rng_s = Rng::seed_from_u64(1);
        let action_s = BackdoorAction {
            netrunner: id_s,
            luck_to_spend: 0,
            floors: floors.clone(),
        };
        let _ = action_s.resolve(&mut world_s, &mut rng_s).unwrap();
        assert_eq!(
            world_s.netrun.as_ref().unwrap().net_actions_used_this_turn,
            1,
            "success: exactly 1 action consumed"
        );

        // --- failure path (high DV) ---
        let high_dv_floors = vec![Floor::Password { dv: DV(24) }];
        let pc_f = netrunner_pc(1, 1); // low stats, high DV → will almost certainly fail
        let (world_f, id_f) = world_in_netrun(pc_f, 5);
        // Find a seed that fails
        for seed in 0u64..200 {
            let pc_f2 = netrunner_pc(1, 1);
            let (mut w2, id2) = world_in_netrun(pc_f2, 5);
            let mut rng_f = Rng::seed_from_u64(seed);
            let action_f = BackdoorAction {
                netrunner: id2,
                luck_to_spend: 0,
                floors: high_dv_floors.clone(),
            };
            let outcome = action_f.resolve(&mut w2, &mut rng_f).unwrap();
            if !outcome.passed {
                assert_eq!(
                    w2.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "failure: exactly 1 action consumed (seed {seed})"
                );
                break;
            }
        }

        // suppress unused warnings
        let _ = (world_f, id_f);
    }

    // -------------------------------------------------------------------------
    // Additional coverage tests
    // -------------------------------------------------------------------------

    /// `test_backdoor_requires_active_netrun`: calling without an active netrun
    /// must return `Err(NoActiveNetrun)`.
    #[test]
    fn test_backdoor_requires_active_netrun() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        // World with no active netrun.
        let mut world = World::new(pc);
        let mut rng = Rng::seed_from_u64(0);

        let action = BackdoorAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            floors: vec![Floor::Password { dv: DV(8) }],
        };

        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::NoActiveNetrun)),
            "expected NoActiveNetrun, got {result:?}"
        );
    }

    /// `test_backdoor_exhausted_net_actions`: if all NET actions are consumed,
    /// must return `Err(NoNetActionsRemaining)`.
    #[test]
    fn test_backdoor_exhausted_net_actions() {
        let floors = vec![Floor::Password { dv: DV(8) }];
        let pc = netrunner_pc(6, 4);
        let (mut world, pc_id) = world_in_netrun(pc, 2);
        // Use all 2 actions.
        world.netrun.as_mut().unwrap().net_actions_used_this_turn = 2;
        let mut rng = Rng::seed_from_u64(0);

        let action = BackdoorAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            floors,
        };

        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::NoNetActionsRemaining)),
            "expected NoNetActionsRemaining(max=2), got {result:?}"
        );
    }

    /// `test_backdoor_validates_luck`: spending more luck than available returns
    /// `Err(InsufficientLuck)`. The floor type check runs first (after action
    /// budget), but luck is validated after the floor type is confirmed.
    #[test]
    fn test_backdoor_validates_luck() {
        let floors = vec![Floor::Password { dv: DV(8) }];
        let mut pc = netrunner_pc(6, 4);
        pc.luck_pool = 0;
        let (mut world, pc_id) = world_in_netrun(pc, 3);
        let mut rng = Rng::seed_from_u64(0);

        let action = BackdoorAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            floors,
        };

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

    /// `test_backdoor_entity_not_found`: unknown entity must return
    /// `Err(EntityNotFound)`.
    #[test]
    fn test_backdoor_entity_not_found() {
        use uuid::Uuid;

        let floors = vec![Floor::Password { dv: DV(8) }];
        let pc = netrunner_pc(6, 4);
        let (mut world, _) = world_in_netrun(pc, 3);
        let mut rng = Rng::seed_from_u64(0);

        let bad_id = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = BackdoorAction {
            netrunner: bad_id,
            luck_to_spend: 0,
            floors,
        };

        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::EntityNotFound(id)) if id == bad_id),
            "expected EntityNotFound, got {result:?}"
        );
    }

    /// `test_backdoor_luck_adds_to_check`: spending LUCK adds to final value
    /// and decrements the pool.
    #[test]
    fn test_backdoor_luck_adds_to_check() {
        let floors = vec![
            Floor::Password { dv: DV(8) },
            Floor::File {
                dv: DV(8),
                contents: crate::netrunning::architecture::FileContents::Data("Prize".to_string()),
            },
        ];
        let pc = netrunner_pc(6, 4); // luck_pool = 10
        let (mut world, pc_id) = world_in_netrun(pc, 3);
        let mut rng = Rng::seed_from_u64(7);

        let action = BackdoorAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            floors,
        };

        let outcome = action.resolve(&mut world, &mut rng).unwrap();
        assert_eq!(outcome.breakdown.luck_spent, 3);
        // final = int + interface + 0 + luck + d10.net
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 3 + outcome.breakdown.d10.net
        );
        // Pool decremented.
        assert_eq!(world.pc.luck_pool, 7, "luck_pool 10 - 3 = 7");
    }
}
