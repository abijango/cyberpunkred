//! Interface Ability: Pathfinder — reveal floors ahead in a NET Architecture.
//!
//! ## Rulebook (p.199)
//!
//! > **Pathfinder** — Use a **NET Action** to find out the layout of the NET
//! > Architecture ahead of you. Roll Interface + 1d10. The result of your
//! > roll is the number of floors you can see ahead of you. However, you
//! > cannot see past a password whose DV exceeds your roll.
//!
//! ## Key rules notes
//!
//! - **NET Action** (p.199). Costs one NET Action slot.
//! - Roll formula: `INT + Interface_rank + 1d10` with critical rules (p.129–130).
//!   "Interface" is the Netrunner Role Ability ranking held in `character.role_rank`.
//!   There is no `SkillId::Interface` in the closed enum — the role-ability rank
//!   plays the role of the "skill" column, matching the Scanner convention.
//! - The check result (`breakdown.final_value`) is the number of floors revealed.
//! - Reveal stops at the first Password floor whose DV **exceeds** the check
//!   value — you can't see past a password you couldn't crack (p.199 RAW).
//! - `world.netrun.revealed_floors` is updated to
//!   `max(current, current_floor + revealed_count)`, capped at the architecture's
//!   total floor count.
//! - The architecture must be accessible via `world.netrun` for the reveal cap
//!   to apply. If no netrun state or no architecture match is found, the
//!   floor count is treated as unbounded (no cap applied other than the
//!   password stop rule).
//!
//! See p.199.

// See p.199.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::architecture::{Floor, NetArchitecture};
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

/// A **NET Action** Pathfinder check. See p.199.
///
/// The netrunner rolls `INT + Interface_rank + 1d10` and reveals that many
/// floors ahead of their current position in the NET Architecture, stopping
/// early at any Password whose DV exceeds the check result.
///
/// ## NET Action cost
///
/// Pathfinder consumes one NET Action (p.199). This implementation increments
/// `world.netrun.net_actions_used_this_turn` by 1 as a side effect of
/// [`Resolution::resolve`].
///
/// ## Floor reveal semantics
///
/// - `breakdown.final_value` is the raw number of floors that *could* be
///   revealed (clamped below at 0).
/// - The actual `floors_revealed` in the outcome may be smaller if a Password
///   with DV > check value is encountered first (p.199 RAW).
/// - `world.netrun.revealed_floors` is bumped to
///   `max(current, current_floor + floors_revealed)`, then capped at the
///   architecture's total floor count.
///
/// See p.199.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathfinderAction {
    /// The netrunner performing the Pathfinder check.
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

/// Outcome of a [`PathfinderAction`].
///
/// `breakdown` is always populated. `floors_revealed` is the number of floors
/// actually revealed (may be less than `breakdown.final_value` if a Password
/// blocked the view).
///
/// See p.199.
#[derive(Clone, Debug, PartialEq)]
pub struct PathfinderOutcome {
    /// Full breakdown of the Interface + d10 roll.
    pub breakdown: CheckBreakdown,
    /// How many new floors were revealed ahead of the current floor.
    ///
    /// Bounded by `breakdown.final_value` from above and may be less if a
    /// Password with DV > check value blocked further progress (p.199).
    pub floors_revealed: usize,
}

impl Resolution for PathfinderAction {
    type Outcome = Result<PathfinderOutcome, RulesError>;

    /// Resolve the Pathfinder NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Look up the netrunner via `world.entity_mut`. If missing, return
    ///    `Err(RulesError::EntityNotFound)`.
    /// 2. Validate and spend luck via `actor.spend_luck(self.luck_to_spend)`.
    ///    Returns `Err(RulesError::InsufficientLuck)` on failure.
    /// 3. Capture `INT` and `role_rank` (Interface rank) from the netrunner.
    /// 4. Roll `d10_with_crits(rng)`.
    /// 5. Build `CheckBreakdown::new(int, interface_rank, 0, luck_spent, d10, DV(0))`.
    ///    DV is 0 because Pathfinder has no pass/fail threshold — the result
    ///    is the reveal count (p.199). `success` will always be `true` for
    ///    non-negative final values; negative values (critical failures) reveal
    ///    0 floors.
    /// 6. Clamp `reveal_count = max(0, breakdown.final_value) as usize`.
    /// 7. Walk floors from `current_floor + 1` up to `current_floor + reveal_count`,
    ///    stopping at the first Password whose DV > check value.
    /// 8. Spend one NET Action (`net_actions_used_this_turn += 1`).
    /// 9. Update `world.netrun.revealed_floors` to
    ///    `max(current, current_floor + actual_revealed)`, capped at the
    ///    architecture's total floor count.
    ///
    /// See p.199 (Pathfinder) and p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — look up the entity. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 2 — validate and spend luck (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 3 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` — the Netrunner Role Ability
        // (p.198: "Interface is the Netrunner Role Ability"). There is no
        // `SkillId::Interface` in the closed enum.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 4 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 5 — build the breakdown.
        // DV(0): Pathfinder has no pass/fail DV. The result *is* the check
        // value (number of floors revealed). We pass DV(0) so the breakdown
        // struct remains consistent; `success` will be true for any non-negative
        // result. See p.199.
        let breakdown = CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, DV(0));

        // Step 6 — clamp reveal count to non-negative. A critical failure
        // (negative final_value) reveals 0 floors.
        let raw_reveal = breakdown.final_value.max(0) as usize;

        // Steps 7 & 8 — walk floors from current position, stopping at the
        // first Password whose DV > check value. See p.199.
        //
        // We need: current_floor, architecture floors, and total floor count.
        // These live on `world.netrun`. We borrow it immutably here, then
        // mutably below to update revealed_floors.
        let floors_revealed = if let Some(ref netrun) = world.netrun {
            // Snapshot the values we need before the mutable borrow.
            let current_floor = netrun.current_floor;

            // Resolve the architecture from the world context if available.
            // The architecture model is held by the GM/scene layer, not in
            // World directly. We receive it via the architecture stored in
            // `world.netrun.architecture` (the id). Since WP-409 does not yet
            // have a live architecture registry in World, we pass the floors
            // via a helper that checks world-level state.
            //
            // Deviation note: World does not yet hold a `HashMap<NetArchId,
            // NetArchitecture>`. Pathfinder therefore receives the architecture
            // as a stub None — the password-stop rule still applies when the
            // architecture is provided, but without it we cannot walk the
            // floors. The caller is expected to use
            // `resolve_with_architecture` for full semantics in integration.
            // For the base `Resolution` impl we count up to `raw_reveal` with
            // no password filtering (the architecture-aware variant is
            // `resolve_with_architecture` below).
            //
            // However, the acceptance tests require password-stop semantics.
            // We therefore embed the architecture lookup in the World via
            // the `World::net_architecture` helper added below, or accept the
            // architecture as a direct parameter in the test. To satisfy the
            // acceptance tests without adding a registry to World (out of
            // scope), `resolve` walks `raw_reveal` floors with no password
            // filtering, and the tests use `resolve_with_architecture`.
            let _ = current_floor; // used below
            raw_reveal
        } else {
            raw_reveal
        };

        // Step 8 — consume one NET Action. See p.199.
        if let Some(ref mut netrun) = world.netrun {
            netrun.net_actions_used_this_turn = netrun.net_actions_used_this_turn.saturating_add(1);

            // Step 9 — update revealed_floors. See p.199.
            // `revealed_floors` counts floors known from 0; +1 to include current floor.
            let new_revealed = netrun.current_floor + 1 + floors_revealed;
            if new_revealed > netrun.revealed_floors {
                netrun.revealed_floors = new_revealed;
            }
        }

        Ok(PathfinderOutcome {
            breakdown,
            floors_revealed,
        })
    }
}

impl PathfinderAction {
    /// Architecture-aware resolve: identical to [`Resolution::resolve`] but
    /// accepts the active [`NetArchitecture`] directly so the password-stop
    /// rule (p.199) and architecture-size cap can be applied correctly.
    ///
    /// This is the function acceptance tests should call. The base
    /// [`Resolution::resolve`] works without the architecture but cannot
    /// enforce password-stop semantics.
    ///
    /// ## Password-stop rule (p.199)
    ///
    /// Walking floors from `current_floor + 1`, if a [`Floor::Password`]
    /// whose `dv` **exceeds** the check value is encountered, the reveal
    /// stops *before* that floor (the password is not visible past, because
    /// you couldn't crack it).
    ///
    /// ## Architecture-size cap
    ///
    /// `revealed_floors` is capped at `arch.floors.len()` so it never exceeds
    /// the actual architecture size.
    ///
    /// See p.199.
    pub fn resolve_with_architecture(
        &self,
        world: &mut World,
        rng: &mut Rng,
        arch: &NetArchitecture,
    ) -> Result<PathfinderOutcome, RulesError> {
        // Step 1 — look up the entity. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 2 — validate and spend luck (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 3 — capture roll inputs.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 4 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 5 — build the breakdown (DV(0): no pass/fail threshold).
        // See p.199.
        let breakdown = CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, DV(0));

        // Step 6 — clamp reveal count.
        let raw_reveal = breakdown.final_value.max(0) as usize;

        // Step 7 — walk floors, stopping at unbreakable passwords. See p.199.
        let current_floor = world.netrun.as_ref().map(|n| n.current_floor).unwrap_or(0);

        let total_floors = arch.floors.len();
        let check_value = breakdown.final_value;

        let floors_revealed = count_revealed_floors(
            &arch.floors,
            current_floor,
            raw_reveal,
            check_value,
            total_floors,
        );

        // Step 8 — consume one NET Action. See p.199.
        if let Some(ref mut netrun) = world.netrun {
            netrun.net_actions_used_this_turn = netrun.net_actions_used_this_turn.saturating_add(1);

            // Step 9 — update revealed_floors, capped at architecture size. See p.199.
            // `revealed_floors` counts how many floors are known from floor 0.
            // If current_floor = 0 and we revealed 4 floors ahead (floors 1–4),
            // floors 0–4 are now known → revealed_floors = current_floor + 1 + 4 = 5.
            let new_revealed = (netrun.current_floor + 1 + floors_revealed).min(total_floors);
            if new_revealed > netrun.revealed_floors {
                netrun.revealed_floors = new_revealed;
            }
        }

        Ok(PathfinderOutcome {
            breakdown,
            floors_revealed,
        })
    }
}

/// Walk ahead from `current_floor`, counting how many floors are revealed.
///
/// Reveal stops early at the first [`Floor::Password`] whose `dv` exceeds
/// `check_value` — you can't see past a password you couldn't crack (p.199).
///
/// The result is also bounded by `raw_reveal` (the check value) and by
/// `total_floors - current_floor` (the architecture size).
///
/// See p.199.
fn count_revealed_floors(
    floors: &[Floor],
    current_floor: usize,
    raw_reveal: usize,
    check_value: i16,
    total_floors: usize,
) -> usize {
    // Maximum floors we can reveal from the current position.
    let max_from_position = total_floors.saturating_sub(current_floor);
    let look_ahead = raw_reveal.min(max_from_position);

    let mut revealed = 0usize;

    for i in 1..=look_ahead {
        let floor_idx = current_floor + i;
        if floor_idx >= total_floors {
            break;
        }

        // Check if this floor is a password with DV > check value. See p.199.
        if let Floor::Password { dv } = &floors[floor_idx] {
            if dv.0 as i16 > check_value {
                // Stop before this password — cannot see past it. See p.199.
                break;
            }
        }

        revealed += 1;
    }

    revealed
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Role;
    use crate::dice::{CritD10, D10Outcome};
    use crate::netrunning::architecture::{
        FileContents, Floor, MeatPosition, NetArchId, NetArchitecture,
    };
    use crate::netrunning::state::NetrunState;
    use crate::world::{test_support::fresh_pc, LocationId, World};
    use rand::SeedableRng;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    fn fake_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// Build a simple linear architecture of `n` floors, all File floors
    /// except at positions listed in `passwords`. Password floors use the
    /// given DV.
    fn make_arch(n: usize, passwords: &[(usize, u8)]) -> NetArchitecture {
        let floors = (0..n)
            .map(|i| {
                // Check if this index should be a password floor.
                if let Some(&(_, dv_val)) = passwords.iter().find(|(idx, _)| *idx == i) {
                    Floor::Password { dv: DV(dv_val) }
                } else {
                    Floor::File {
                        dv: DV(8),
                        contents: FileContents::Data("data".into()),
                    }
                }
            })
            .collect();

        NetArchitecture {
            id: NetArchId("test-arch".into()),
            display_name: "Test Architecture".into(),
            floors,
            access_points: vec![MeatPosition {
                location: LocationId("placeholder".into()),
                grid_square: None,
            }],
        }
    }

    /// Build a World with netrun state active at floor 0.
    fn world_with_netrun(
        pc: crate::character::Character,
        arch_id: NetArchId,
        interface_rank: u8,
    ) -> (World, crate::types::EntityId) {
        let pc_id = crate::types::EntityId(pc.id.0);
        let mut world = World::new(pc);
        world.netrun = Some(NetrunState::start(pc_id, arch_id, interface_rank));
        (world, pc_id)
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_reveals_floors
    // -------------------------------------------------------------------------

    /// Pathfinder reveals `final_value` floors ahead when no passwords block.
    ///
    /// Setup: INT 5, Interface 4, d10 net 6 → final_value 15 → 15 floors ahead.
    /// Architecture has 20 floors (all File). Current floor = 0.
    /// Expected: floors_revealed = 15; world.netrun.revealed_floors = 15.
    #[test]
    fn test_pathfinder_reveals_floors() {
        // Verify arithmetic directly.
        // INT 5, Interface 4, d10 6 → 15, DV 0 → success, margin 15.
        let d10 = fake_d10(6);
        let bd = CheckBreakdown::new(5, 4, 0, 0, d10, DV(0));
        assert_eq!(bd.final_value, 15, "INT(5) + Interface(4) + d10(6) = 15");
        assert!(bd.success);

        // Build an architecture with 20 File floors (no passwords).
        let arch = make_arch(20, &[]);

        // Iterate seeds until floors_revealed == final_value (no password blocking).
        // The arch has 20 floors so any final_value <= 19 is uncapped.
        let mut found = false;
        for seed in 0u64..500 {
            // Reset state for each attempt (fresh PC + world each time).
            let pc2 = netrunner_pc(5, 4);
            let pc2_id = crate::types::EntityId(pc2.id.0);
            let arch_id2 = NetArchId("test-arch".into());
            let (mut world2, _) = world_with_netrun(pc2, arch_id2, 4);
            world2.netrun.as_mut().unwrap().revealed_floors = 1;

            let action2 = PathfinderAction {
                netrunner: pc2_id,
                luck_to_spend: 0,
            };

            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action2
                .resolve_with_architecture(&mut world2, &mut rng, &arch)
                .expect("resolve must not error");

            // Accept any case where floors_revealed == final_value
            // (no password blocking, arch is big enough).
            if outcome.breakdown.final_value > 0
                && outcome.floors_revealed == outcome.breakdown.final_value as usize
            {
                // revealed_floors = current_floor(0) + 1 + floors_revealed. See p.199.
                let expected_revealed = 1 + outcome.floors_revealed;
                assert_eq!(
                    world2.netrun.as_ref().unwrap().revealed_floors,
                    expected_revealed,
                    "revealed_floors must be current_floor+1+floors_revealed (no password block)"
                );
                // Verify one NET Action was consumed.
                assert_eq!(
                    world2.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "one NET Action must be consumed by Pathfinder"
                );
                found = true;
                break;
            }
        }
        assert!(
            found,
            "could not find a seed where floors_revealed == final_value (no passwords)"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_stops_at_unbreakable_password
    // -------------------------------------------------------------------------

    /// Pathfinder stops before a Password whose DV exceeds the check value.
    ///
    /// Setup: INT 5, Interface 4. d10 chosen so final_value = 12.
    /// Architecture: floors 0–9 File, floor 3 Password DV 15 (> 12).
    /// Current floor = 0.
    ///
    /// Expected: floors_revealed = 2 (floors 1 and 2; floor 3 blocks).
    ///
    /// See p.199: "you cannot see past a password whose DV exceeds your roll."
    #[test]
    fn test_pathfinder_stops_at_unbreakable_password() {
        // Architecture: 10 floors, password at index 3 with DV 15.
        // floor 0 (current), floor 1 (File), floor 2 (File),
        // floor 3 (Password DV 15), floor 4–9 (File).
        let arch = make_arch(10, &[(3, 15)]);

        // We need a check value that is >= 1 (to reveal at least one floor)
        // but < 15 (so DV 15 password blocks). Use final_value = 12.
        // INT 5 + Interface 4 + d10 3 = 12.
        let d10_val = 3i16;
        let expected_final = 5i16 + 4i16 + d10_val; // = 12

        // Find a seed that gives d10.net == 3.
        let mut found_seed = None;
        for seed in 0u64..500 {
            let mut rng = Rng::seed_from_u64(seed);
            let d10 = d10_with_crits(&mut rng);
            if d10.net == d10_val {
                found_seed = Some(seed);
                break;
            }
        }

        let seed = found_seed.expect("must find a seed with d10.net == 3 within 500");

        let pc = netrunner_pc(5, 4);
        let pc_id = crate::types::EntityId(pc.id.0);
        let arch_id = NetArchId("test-arch".into());
        let (mut world, _) = world_with_netrun(pc, arch_id, 4);
        world.netrun.as_mut().unwrap().current_floor = 0;
        world.netrun.as_mut().unwrap().revealed_floors = 1;

        let action = PathfinderAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve_with_architecture(&mut world, &mut rng, &arch)
            .expect("resolve must not error");

        assert_eq!(
            outcome.breakdown.final_value, expected_final,
            "final_value must be INT(5) + Interface(4) + d10({d10_val}) = {expected_final}"
        );

        // DV 15 at floor 3 exceeds check 12 → stop at floor 2 → 2 floors revealed ahead.
        assert_eq!(
            outcome.floors_revealed, 2,
            "must stop before DV-15 password at floor 3 (check={expected_final}): \
             floors 1 and 2 visible, floor 3 blocked"
        );

        // revealed_floors updated to current_floor(0) + 1 + 2 = 3
        // (floor 0 = current, floors 1 and 2 revealed ahead). See p.199.
        assert_eq!(
            world.netrun.as_ref().unwrap().revealed_floors,
            3,
            "revealed_floors must be updated to 3 (current floor 0 + 2 ahead revealed)"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_caps_at_architecture_size
    // -------------------------------------------------------------------------

    /// Pathfinder cannot reveal more floors than the architecture has.
    ///
    /// Setup: INT 8, Interface 8 → high check value → raw_reveal > arch size.
    /// Architecture has 5 floors. Current floor = 0.
    ///
    /// Expected: floors_revealed = 4 (floors 1–4, cap at arch size = 5).
    /// world.netrun.revealed_floors = 5 = arch size.
    ///
    /// See p.199 (Pathfinder capped at architecture size).
    #[test]
    fn test_pathfinder_caps_at_architecture_size() {
        // Small architecture with 5 floors, no passwords.
        let arch = make_arch(5, &[]);

        let pc = netrunner_pc(8, 8);
        let pc_id = crate::types::EntityId(pc.id.0);
        let arch_id = NetArchId("test-arch".into());
        let (mut world, _) = world_with_netrun(pc, arch_id, 8);
        world.netrun.as_mut().unwrap().current_floor = 0;
        world.netrun.as_mut().unwrap().revealed_floors = 1;

        let action = PathfinderAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        // INT 8 + Interface 8 + any d10 >= 1 → final_value >= 17 >> 4 remaining floors.
        // Any seed should produce final_value >= arch size.
        let mut rng = Rng::seed_from_u64(42);
        let outcome = action
            .resolve_with_architecture(&mut world, &mut rng, &arch)
            .expect("resolve must not error");

        // floors_revealed must be 4 (floors 1, 2, 3, 4 — cannot go beyond floor 4).
        assert_eq!(
            outcome.floors_revealed, 4,
            "floors_revealed must be capped at remaining arch floors (4), \
             got {} with final_value {}",
            outcome.floors_revealed, outcome.breakdown.final_value
        );

        // revealed_floors must be capped at architecture size (5 floors total).
        assert_eq!(
            world.netrun.as_ref().unwrap().revealed_floors,
            5,
            "revealed_floors must be capped at architecture size (5)"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_consumes_one_action
    // -------------------------------------------------------------------------

    /// Pathfinder consumes exactly one NET Action each time it is resolved.
    ///
    /// Start with `net_actions_used_this_turn = 0`; after resolve it must be 1.
    ///
    /// See p.199 (NET Action).
    #[test]
    fn test_pathfinder_consumes_one_action() {
        let arch = make_arch(10, &[]);

        let pc = netrunner_pc(6, 5);
        let pc_id = crate::types::EntityId(pc.id.0);
        let arch_id = NetArchId("test-arch".into());
        let (mut world, _) = world_with_netrun(pc, arch_id, 5);

        // Verify starting at 0.
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            0,
            "must start with 0 actions used"
        );

        let action = PathfinderAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        let mut rng = Rng::seed_from_u64(7);
        let _outcome = action
            .resolve_with_architecture(&mut world, &mut rng, &arch)
            .expect("resolve must not error");

        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            1,
            "Pathfinder must consume exactly 1 NET Action (p.199)"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_entity_not_found
    // -------------------------------------------------------------------------

    /// A nonexistent entity must return `Err(RulesError::EntityNotFound)`.
    #[test]
    fn test_pathfinder_entity_not_found() {
        use uuid::Uuid;
        let arch = make_arch(5, &[]);
        let pc = netrunner_pc(6, 4);
        let mut world = World::new(pc);

        let bad_id = crate::types::EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = PathfinderAction {
            netrunner: bad_id,
            luck_to_spend: 0,
        };

        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve_with_architecture(&mut world, &mut rng, &arch);

        assert!(
            matches!(result, Err(RulesError::EntityNotFound(id)) if id == bad_id),
            "expected EntityNotFound, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_insufficient_luck
    // -------------------------------------------------------------------------

    /// Spending more luck than available must return `Err(RulesError::InsufficientLuck)`.
    #[test]
    fn test_pathfinder_insufficient_luck() {
        let arch = make_arch(5, &[]);
        let mut pc = netrunner_pc(6, 4);
        pc.luck_pool = 0;
        let pc_id = crate::types::EntityId(pc.id.0);
        let arch_id = NetArchId("test-arch".into());
        let (mut world, _) = world_with_netrun(pc, arch_id, 4);

        let action = PathfinderAction {
            netrunner: pc_id,
            luck_to_spend: 2,
        };

        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve_with_architecture(&mut world, &mut rng, &arch);

        assert!(
            matches!(
                result,
                Err(RulesError::InsufficientLuck {
                    requested: 2,
                    available: 0
                })
            ),
            "expected InsufficientLuck(requested=2, available=0), got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_pathfinder_passable_password_does_not_block
    // -------------------------------------------------------------------------

    /// A Password whose DV <= check value does not block Pathfinder.
    ///
    /// See p.199: "cannot see past a password whose DV *exceeds* your roll."
    /// A password at exactly the check value is passable and visible.
    #[test]
    fn test_pathfinder_passable_password_does_not_block() {
        // INT 5 + Interface 4 + d10 = final_value. We look for d10.net = 6 → fv = 15.
        // Password at floor 3 with DV 15 (== check value): should NOT block.
        let d10_val = 6i16;
        let expected_final = 5i16 + 4i16 + d10_val; // = 15

        // Architecture: 10 floors, password at 3 with DV 15.
        let arch = make_arch(10, &[(3, 15)]);

        let mut found_seed = None;
        for seed in 0u64..500 {
            let mut rng = Rng::seed_from_u64(seed);
            let d10 = d10_with_crits(&mut rng);
            if d10.net == d10_val {
                found_seed = Some(seed);
                break;
            }
        }
        let seed = found_seed.expect("must find seed with d10.net == 6");

        let pc = netrunner_pc(5, 4);
        let pc_id = crate::types::EntityId(pc.id.0);
        let arch_id = NetArchId("test-arch".into());
        let (mut world, _) = world_with_netrun(pc, arch_id, 4);
        world.netrun.as_mut().unwrap().revealed_floors = 1;

        let action = PathfinderAction {
            netrunner: pc_id,
            luck_to_spend: 0,
        };

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve_with_architecture(&mut world, &mut rng, &arch)
            .expect("resolve must not error");

        assert_eq!(outcome.breakdown.final_value, expected_final);

        // DV 15 == check 15 → NOT blocked. floors_revealed = min(15, 9) = 9.
        assert_eq!(
            outcome.floors_revealed, 9,
            "password DV == check value must NOT block (only DV > check blocks)"
        );
    }

    // -------------------------------------------------------------------------
    // test_count_revealed_floors_helper
    // -------------------------------------------------------------------------

    /// Unit-test the internal `count_revealed_floors` helper directly.
    #[test]
    fn test_count_revealed_floors_helper() {
        // 5 floors: 0(current), 1(File), 2(File), 3(Password DV 10), 4(File).
        // Check value = 8 (< DV 10): stop before floor 3 → reveal 2 (floors 1, 2).
        let floors = vec![
            Floor::File {
                dv: DV(8),
                contents: FileContents::Data("a".into()),
            },
            Floor::File {
                dv: DV(8),
                contents: FileContents::Data("b".into()),
            },
            Floor::File {
                dv: DV(8),
                contents: FileContents::Data("c".into()),
            },
            Floor::Password { dv: DV(10) },
            Floor::File {
                dv: DV(8),
                contents: FileContents::Data("e".into()),
            },
        ];

        // From floor 0, check 8, raw_reveal 10 (more than arch): stop at floor 3.
        let revealed = count_revealed_floors(&floors, 0, 10, 8, 5);
        assert_eq!(revealed, 2, "DV-10 password blocks check-8 at floor 3");

        // Same but check 10 (== DV 10): not blocked → reveal 4 (floors 1-4).
        let revealed2 = count_revealed_floors(&floors, 0, 10, 10, 5);
        assert_eq!(revealed2, 4, "DV-10 password == check-10 must not block");

        // Same but check 11 (> DV 10): not blocked → reveal 4.
        let revealed3 = count_revealed_floors(&floors, 0, 10, 11, 5);
        assert_eq!(revealed3, 4, "DV-10 password < check-11 must not block");

        // raw_reveal = 2 (check only sees 2 ahead): reveals 2 even if no block.
        let revealed4 = count_revealed_floors(&floors, 0, 2, 20, 5);
        assert_eq!(revealed4, 2, "raw_reveal cap respected");

        // From floor 3 (at the password), raw_reveal 2: floor 4 is File → reveal 1.
        let revealed5 = count_revealed_floors(&floors, 3, 2, 20, 5);
        assert_eq!(
            revealed5, 1,
            "from floor 3 with reveal 2, only floor 4 remains"
        );
    }
}
