//! Interface Ability: Control — seize a Control Node at the current floor.
//!
//! ## Rulebook (p.199)
//!
//! > **Control** — Allows you to control things attached to the NET
//! > Architectures like cameras, drones, turrets, laser grids, elevators,
//! > sprinklers, etc., using a Control Node. Each Node has a DV required to
//! > take control of it as a NET Action. Operating each individual thing
//! > attached to the node requires a separate NET Action once you have taken
//! > hold of the Control Node, and can be done from anywhere in the
//! > Architecture as long as you are still in control of the Control Node.
//! > **Each Control Node can only be activated once per Turn.** The DV to
//! > wrest a Control Node currently held by another Netrunner or a Demon is
//! > equal to the Control Check they made to take control of it. You lose
//! > control of any Control Nodes you hold in an Architecture when you Jack
//! > Out.
//!
//! ## Roll formula (p.199)
//!
//! ```text
//! Interface_rank + INT + 1d10  vs.  DV
//! ```
//!
//! Where:
//! - **DV** is the node's listed DV (from `Floor::ControlNode { dv, .. }`),
//!   **or**, if the node is currently held by another Netrunner or Demon, the
//!   value of their original Control Check (provided by the caller as
//!   `ControlAction::dv`).
//! - **Interface_rank** is the Netrunner's Role Ability rank (`role_rank`
//!   when `role == Role::Netrunner`), playing the "skill" column.
//! - **INT** is the linked STAT for Interface checks (p.199).
//!
//! ## OperateControlledNode (out of scope)
//!
//! Once a node is held, the Netrunner can **Operate** it once per Turn as an
//! additional NET Action. This is a separate action (`OperateControlledNode`)
//! which is out of scope for WP-407. The `controlled_nodes` list in
//! [`crate::netrunning::state::NetrunState`] tracks held floors; a future WP
//! should expose `OperateControlledNode { floor_idx: usize }` that verifies
//! membership in that list.
//!
//! ## NET Action accounting
//!
//! Control is a **NET Action** (p.199). This implementation consumes exactly
//! one `net_actions_used_this_turn` from [`crate::netrunning::state::NetrunState`]
//! and rejects the attempt with [`crate::error::RulesError::NoNetActionsRemaining`]
//! if the budget is already exhausted.
//!
//! ## Duplication guard
//!
//! If the Netrunner already holds the node (`controlled_nodes.contains`), this
//! ability is a no-op on `controlled_nodes` — the floor index is not added
//! twice. The action still consumes a NET Action and rolls normally; the
//! outcome's `captured_floor` will be `None` (already held by self, nothing
//! new to capture). The caller should check this case before spending the
//! action if desired.
//!
//! See p.199.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

// ---------------------------------------------------------------------------
// ControlAction
// ---------------------------------------------------------------------------

/// A **NET Action** to seize a Control Node at the current floor. See p.199.
///
/// The caller is responsible for determining the correct `dv`:
/// - If the node is uncontested, `dv` is the floor's listed DV
///   (`Floor::ControlNode { dv, .. }`).
/// - If the node is currently held by another Netrunner or Demon, `dv` is the
///   final value of their original Control Check (p.199: "The DV to wrest a
///   Control Node currently held by another Netrunner or a Demon is equal to
///   the Control Check they made to take control of it").
///
/// The `floor_idx` must equal `world.netrun.as_ref().unwrap().current_floor`.
/// If they do not match the action is rejected with
/// [`RulesError::NotAControlNode`]. This validates that the caller constructed
/// the action against the floor the Netrunner is actually standing on.
///
/// See p.199.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ControlAction {
    /// The entity performing the Control attempt (must be the jacked-in
    /// Netrunner whose `EntityId` matches `world.netrun.netrunner`).
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
    /// The DV to beat: either the node's listed DV, or the previous holder's
    /// Control Check value if the node is currently held. See p.199.
    pub dv: DV,
    /// The floor index the Netrunner is attempting to seize.
    ///
    /// Must equal `world.netrun.current_floor` at resolution time; mismatches
    /// are rejected with [`RulesError::NotAControlNode`] to prevent
    /// off-floor targeting.
    pub floor_idx: usize,
}

// ---------------------------------------------------------------------------
// ControlOutcome
// ---------------------------------------------------------------------------

/// Outcome of a [`ControlAction`].
///
/// On success, `captured_floor` contains the floor index just seized. On
/// failure (roll did not beat DV, or node already held by self), it is `None`.
///
/// See p.199 (Control Interface Ability).
#[derive(Clone, Debug, PartialEq)]
pub struct ControlOutcome {
    /// Full breakdown of the Interface + d10 roll, including margin vs. DV.
    pub breakdown: CheckBreakdown,
    /// The floor index captured by this action, or `None` if the roll failed
    /// or the node was already held by this Netrunner.
    ///
    /// On `Some(idx)`, `world.netrun.controlled_nodes` now contains `idx`.
    pub captured_floor: Option<usize>,
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for ControlAction {
    type Outcome = Result<ControlOutcome, RulesError>;

    /// Resolve the Control NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Verify `world.netrun` is `Some`. Return
    ///    [`RulesError::NetrunNotActive`] if not.
    /// 2. Verify `world.netrun.net_actions_used_this_turn <
    ///    world.netrun.net_actions_max_this_turn`. Return
    ///    [`RulesError::NoNetActionsRemaining`] if exhausted.
    /// 3. Verify `self.floor_idx == world.netrun.current_floor`. Return
    ///    [`RulesError::NotAControlNode`] if not (caller targeting wrong
    ///    floor).
    /// 4. Look up the Netrunner entity via [`World::entity_mut`]. Return
    ///    [`RulesError::EntityNotFound`] if missing.
    /// 5. Spend luck via `actor.spend_luck(self.luck_to_spend)`. Return
    ///    [`RulesError::InsufficientLuck`] on failure.
    /// 6. Capture INT and Interface rank.
    /// 7. Consume one NET Action (`net_actions_used_this_turn += 1`).
    /// 8. Roll `d10_with_crits(rng)`.
    /// 9. Build [`CheckBreakdown`].
    /// 10. On success and node not already held by self: push `floor_idx`
    ///     into `controlled_nodes`. Set `captured_floor = Some(floor_idx)`.
    ///
    /// See p.199 (Control, Interface Abilities).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — verify netrun is active. See p.199.
        {
            let netrun = world.netrun.as_ref().ok_or(RulesError::NetrunNotActive)?;

            // Step 2 — verify NET Action budget (p.197: Interface rank → action count).
            if netrun.net_actions_used_this_turn >= netrun.net_actions_max_this_turn {
                return Err(RulesError::NoNetActionsRemaining);
            }

            // Step 3 — verify floor targeting. The caller must supply the index
            // that matches the Netrunner's current position. See p.199.
            if self.floor_idx != netrun.current_floor {
                return Err(RulesError::NotAControlNode {
                    floor_idx: self.floor_idx,
                });
            }
        }

        // Step 4 — look up the Netrunner entity.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 5 — validate and spend luck (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 6 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198: "Interface is the Netrunner
        // Role Ability"). There is no `SkillId::Interface` in the closed
        // enum — the role-ability rank plays the skill column.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 7 — consume one NET Action (must happen before rolling so
        // action is "spent" regardless of roll outcome, per tabletop
        // convention: actions are declared and consumed on declaration).
        // SAFETY: borrow ends after step 6.
        let netrun = world
            .netrun
            .as_mut()
            .expect("netrun was Some in step 1; cannot have changed");
        netrun.net_actions_used_this_turn += 1;

        // Step 8 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 9 — build the breakdown.
        // stat_value   = INT (the STAT linked to Interface checks)
        // skill_value  = Interface Role Ability rank (plays the skill column)
        // modifier_total = 0 (no situational modifiers in base Control)
        // See p.199.
        let breakdown =
            CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, self.dv);

        // Step 10 — on success, capture the node unless already held.
        // See p.199.
        let captured_floor = if breakdown.success {
            let netrun = world
                .netrun
                .as_mut()
                .expect("netrun was Some in step 1; cannot have changed");
            // Duplication guard: !map.contains_key equivalent for Vec.
            if !netrun.controlled_nodes.contains(&self.floor_idx) {
                netrun.take_control_node(self.floor_idx);
                Some(self.floor_idx)
            } else {
                // Already held by self — no-op on controlled_nodes.
                None
            }
        } else {
            None
        };

        Ok(ControlOutcome {
            breakdown,
            captured_floor,
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
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a Netrunner PC with specified INT and Interface rank.
    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// Build a [`World`] with an active netrun at `floor_idx`, with `max_actions`
    /// NET Actions available. The architecture has one revealed floor at index
    /// `floor_idx`.
    fn world_with_netrun(
        pc: crate::character::Character,
        floor_idx: usize,
        max_actions: u8,
        actions_used: u8,
    ) -> World {
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        let mut state = NetrunState::start(pc_id, NetArchId("test-arch".into()), 5);
        state.current_floor = floor_idx;
        // Reveal enough floors so we're not blocked by pathfinder logic.
        state.revealed_floors = floor_idx + 1;
        state.net_actions_max_this_turn = max_actions;
        state.net_actions_used_this_turn = actions_used;
        world.netrun = Some(state);
        world
    }

    // -------------------------------------------------------------------------
    // test_control_takes_node
    // -------------------------------------------------------------------------

    /// On a successful roll, `captured_floor` is `Some(floor_idx)` and
    /// `controlled_nodes` contains the floor.
    ///
    /// Setup: INT=8, Interface=7, DV=10 → guaranteed success with d10 net ≥ 0
    /// on most seeds. We iterate seeds to guarantee at least one success.
    ///
    /// See p.199 (Control).
    #[test]
    fn test_control_takes_node() {
        let dv = DV(10);

        let mut found_success = false;
        for seed in 0u64..200 {
            let pc = netrunner_pc(8, 7);
            let pc_id = EntityId(pc.id.0);
            let mut world = world_with_netrun(pc, 2, 3, 0);

            let action = ControlAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv,
                floor_idx: 2,
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action.resolve(&mut world, &mut rng).unwrap();

            if outcome.breakdown.success {
                assert_eq!(
                    outcome.captured_floor,
                    Some(2),
                    "seed {seed}: success must set captured_floor = Some(2)"
                );
                let netrun = world.netrun.as_ref().unwrap();
                assert!(
                    netrun.controlled_nodes.contains(&2),
                    "seed {seed}: controlled_nodes must contain floor 2"
                );
                // NET Action consumed.
                assert_eq!(
                    netrun.net_actions_used_this_turn, 1,
                    "seed {seed}: one NET Action must be consumed"
                );
                found_success = true;
                break;
            }
        }
        assert!(
            found_success,
            "expected at least one successful Control roll for INT=8, Interface=7, DV=10"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_fails_low_roll
    // -------------------------------------------------------------------------

    /// On a failed roll, `captured_floor` is `None` and `controlled_nodes`
    /// remains empty. The NET Action is still consumed.
    ///
    /// Setup: INT=1, Interface=1, DV=24 → nearly guaranteed failure.
    ///
    /// See p.199 (Control).
    #[test]
    fn test_control_fails_low_roll() {
        let dv = DV(24); // DV::INCREDIBLE — almost impossible with INT=1, Interface=1

        let mut found_failure = false;
        for seed in 0u64..500 {
            let pc = netrunner_pc(1, 1);
            let pc_id = EntityId(pc.id.0);
            let mut world = world_with_netrun(pc, 0, 3, 0);

            let action = ControlAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv,
                floor_idx: 0,
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action.resolve(&mut world, &mut rng).unwrap();

            if !outcome.breakdown.success {
                assert_eq!(
                    outcome.captured_floor, None,
                    "seed {seed}: failure must set captured_floor = None"
                );
                let netrun = world.netrun.as_ref().unwrap();
                assert!(
                    netrun.controlled_nodes.is_empty(),
                    "seed {seed}: controlled_nodes must remain empty on failure"
                );
                // NET Action still consumed even on failure.
                assert_eq!(
                    netrun.net_actions_used_this_turn, 1,
                    "seed {seed}: one NET Action consumed even on failure"
                );
                found_failure = true;
                break;
            }
        }
        assert!(
            found_failure,
            "expected at least one failed Control roll for INT=1, Interface=1, DV=24"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_rejects_non_control_floor
    // -------------------------------------------------------------------------

    /// Attempting Control on a floor index that does not match `current_floor`
    /// returns `Err(RulesError::NotAControlNode)`.
    ///
    /// The Netrunner is on floor 0; the action targets floor 3.
    ///
    /// See p.199 (Control: "at current floor").
    #[test]
    fn test_control_rejects_non_control_floor() {
        let pc = netrunner_pc(8, 7);
        let pc_id = EntityId(pc.id.0);
        // Netrunner is on floor 0.
        let mut world = world_with_netrun(pc, 0, 3, 0);

        // Target floor 3 — mismatch.
        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(10),
            floor_idx: 3,
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::NotAControlNode { floor_idx: 3 })),
            "expected NotAControlNode(3), got {result:?}"
        );
        // No NET Action consumed on pre-roll rejection.
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            0,
            "no NET Action consumed when floor targeting check fails"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_consumes_one_action
    // -------------------------------------------------------------------------

    /// Each successful and failed Control attempt increments
    /// `net_actions_used_this_turn` by exactly 1.
    ///
    /// Also verifies that when the budget is exhausted,
    /// `Err(RulesError::NoNetActionsRemaining)` is returned.
    ///
    /// See p.197 (NET Actions table), p.199 (Control is a NET Action).
    #[test]
    fn test_control_consumes_one_action() {
        // Use INT=8, Interface=7 so we can reliably get successes.
        let pc = netrunner_pc(8, 7);
        let pc_id = EntityId(pc.id.0);
        // Give 2 NET Actions max; start with 0 used.
        let mut world = world_with_netrun(pc, 0, 2, 0);

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(10),
            floor_idx: 0,
        };

        // First action: should succeed (consumes 1 action).
        let mut rng = Rng::seed_from_u64(42);
        let _outcome1 = action.resolve(&mut world, &mut rng).unwrap();
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            1,
            "first action must increment net_actions_used to 1"
        );

        // Second action: tries the same floor; already controlled → captured_floor=None,
        // but action is consumed. used goes to 2.
        let _outcome2 = action.resolve(&mut world, &mut rng).unwrap();
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            2,
            "second action must increment net_actions_used to 2"
        );

        // Third action: budget exhausted → NoNetActionsRemaining.
        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::NoNetActionsRemaining)),
            "third action must fail with NoNetActionsRemaining, got {result:?}"
        );
        // Count must not increase further.
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            2,
            "net_actions_used must not change when action is rejected"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_rejects_no_netrun
    // -------------------------------------------------------------------------

    /// Attempting Control with no active netrun returns
    /// `Err(RulesError::NetrunNotActive)`.
    #[test]
    fn test_control_rejects_no_netrun() {
        let pc = netrunner_pc(8, 7);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc); // no netrun set

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(10),
            floor_idx: 0,
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::NetrunNotActive)),
            "expected NetrunNotActive, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_rejects_entity_not_found
    // -------------------------------------------------------------------------

    /// A bad entity ID returns `Err(RulesError::EntityNotFound)`.
    #[test]
    fn test_control_rejects_entity_not_found() {
        use uuid::Uuid;
        let pc = netrunner_pc(8, 7);
        let mut world = world_with_netrun(pc, 0, 3, 0);

        let bad_id = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = ControlAction {
            netrunner: bad_id,
            luck_to_spend: 0,
            dv: DV(10),
            floor_idx: 0,
        };
        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::EntityNotFound(id)) if id == bad_id),
            "expected EntityNotFound, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_validates_luck
    // -------------------------------------------------------------------------

    /// Spending more luck than available returns `Err(RulesError::InsufficientLuck)`.
    #[test]
    fn test_control_validates_luck() {
        let mut pc = netrunner_pc(8, 7);
        pc.luck_pool = 0;
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 0, 3, 0);

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            dv: DV(10),
            floor_idx: 0,
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
        // NET Action must NOT be consumed on a luck failure (luck is checked
        // before the action is "spent").
        assert_eq!(
            world.netrun.as_ref().unwrap().net_actions_used_this_turn,
            0,
            "luck failure must not consume a NET Action"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_luck_adds_to_check
    // -------------------------------------------------------------------------

    /// Spending N luck adds N to the final check value.
    #[test]
    fn test_control_luck_adds_to_check() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 0, 3, 0);

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            dv: DV(13),
            floor_idx: 0,
        };
        let mut rng = Rng::seed_from_u64(7);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(
            outcome.breakdown.luck_spent, 3,
            "luck_spent must be 3 in breakdown"
        );
        // final = stat + skill + 0 + luck + d10.net = 6+4+3+d10.net
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 3 + outcome.breakdown.d10.net
        );
        assert_eq!(world.pc.luck_pool, 7, "luck_pool 10 - 3 = 7");
    }

    // -------------------------------------------------------------------------
    // test_control_node_not_added_twice
    // -------------------------------------------------------------------------

    /// If the Netrunner already holds the node (`controlled_nodes.contains`),
    /// a successful re-roll returns `captured_floor = None` and does not
    /// duplicate the entry in `controlled_nodes`.
    ///
    /// See module doc (duplication guard) and p.199.
    #[test]
    fn test_control_node_not_added_twice() {
        // Use high stats to get a reliable success.
        let pc = netrunner_pc(10, 10);
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 0, 5, 0);

        // Pre-seed the node as already held.
        world.netrun.as_mut().unwrap().controlled_nodes.push(0);

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(6),
            floor_idx: 0,
        };

        // Find a seed that produces a success.
        let mut found_success = false;
        for seed in 0u64..200 {
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action.resolve(&mut world, &mut rng).unwrap();
            if outcome.breakdown.success {
                assert_eq!(
                    outcome.captured_floor, None,
                    "seed {seed}: already-held node must give captured_floor = None"
                );
                let count = world
                    .netrun
                    .as_ref()
                    .unwrap()
                    .controlled_nodes
                    .iter()
                    .filter(|&&x| x == 0)
                    .count();
                assert_eq!(
                    count, 1,
                    "seed {seed}: floor 0 must appear exactly once in controlled_nodes"
                );
                found_success = true;
                break;
            }
        }
        assert!(
            found_success,
            "expected at least one success for INT=10, Interface=10, DV=6"
        );
    }

    // -------------------------------------------------------------------------
    // test_control_roll_formula
    // -------------------------------------------------------------------------

    /// Verify the roll formula: stat=INT, skill=Interface rank, modifier=0.
    ///
    /// See p.199 (Interface + 1d10 vs DV).
    #[test]
    fn test_control_roll_formula() {
        let pc = netrunner_pc(6, 4); // INT=6, Interface=4
        let pc_id = EntityId(pc.id.0);
        let mut world = world_with_netrun(pc, 0, 3, 0);

        let action = ControlAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(10),
            floor_idx: 0,
        };
        let mut rng = Rng::seed_from_u64(99);
        let outcome = action.resolve(&mut world, &mut rng).unwrap();

        assert_eq!(
            outcome.breakdown.stat_value, 6,
            "stat_value must be INT (6)"
        );
        assert_eq!(
            outcome.breakdown.skill_value, 4,
            "skill_value must be Interface rank (4)"
        );
        assert_eq!(outcome.breakdown.modifier_total, 0);
        assert_eq!(outcome.breakdown.dv, DV(10));
        // final_value = INT + Interface + 0 + 0 + d10.net
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + outcome.breakdown.d10.net
        );
    }
}
