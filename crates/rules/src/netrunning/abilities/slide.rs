//! Interface Ability: Slide — flee from a single Non-Demon Black ICE.
//!
//! ## Rulebook (p.200)
//!
//! > **Slide** — Attempt to flee combat with a single Non-Demon Black ICE
//! > Program as a **NET Action**. If you are able to roll a successful Slide
//! > Check against the Program's Perception + 1d10 you can escape the Black
//! > ICE to an adjacent floor of the elevator, but not past a Password or
//! > other NET obstruction. A Black ICE Program that has been successfully
//! > slid away from stops following the Netrunner and becomes a Black ICE
//! > laying in wait right where it was slid away from. **You can only
//! > attempt to Slide once per Turn.** You can't Slide preemptively.
//!
//! ## Roll formula
//!
//! Netrunner: `INT + Interface_rank + 1d10`
//! ICE: `ICE.per + 1d10`
//! Higher roll wins. Tie goes to the ICE (p.200 RAW: "a successful Slide
//! Check *against*" the ICE implies the Netrunner must *exceed* the ICE
//! roll; on equal totals the Netrunner fails to escape).
//!
//! ## Constraints (p.200)
//!
//! - **NET Action** — costs one NET Action per turn.
//! - **Once per Turn** — tracked on [`NetrunState::slide_used_this_turn`].
//! - **Non-Demon only** — targeting a `Floor::Demon` returns
//!   [`RulesError::CannotSlideDemon`].
//! - **Cannot slide preemptively** — the caller must ensure the ICE is
//!   `BlackIceState::InCombat` before invoking; this WP does not re-check
//!   that precondition because the higher-level turn engine (WP-414) owns
//!   the combat-engagement gate. The `target_ice_floor` index must point at
//!   a `Floor::BlackIce`.
//! - **Cannot slide past a Password** — enforced by the higher-level
//!   movement layer (WP-403+). This WP resolves only the contested roll.
//!
//! ## State mutation on success (p.200)
//!
//! On a successful Slide the target ICE floor's `state` is set to
//! [`BlackIceState::Slid`]. The ICE stops following the Netrunner and
//! returns to lying in wait at the same floor — [`BlackIceState::Slid`]
//! represents this in-flight state; a future WP can advance it to
//! [`BlackIceState::LyingInWait`] at turn start to model the "right where
//! it was slid away from" clause.
//!
//! ## See p.200.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::architecture::{BlackIceState, Floor};
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::EntityId;
use crate::world::World;

/// A **NET Action** Slide check. See p.200.
///
/// The Netrunner rolls `INT + Interface_rank + 1d10` against the target
/// ICE's `per + 1d10`. If the Netrunner's total strictly exceeds the ICE's
/// total the Slide succeeds and the ICE's floor state is set to
/// [`BlackIceState::Slid`].
///
/// ## Preconditions checked by `resolve`
///
/// 1. A netrun is active (`World::netrun` is `Some`).
/// 2. The Netrunner entity exists.
/// 3. `slide_used_this_turn` is `false` (enforces once-per-Turn, p.200).
/// 4. `target_ice_floor` points at a `Floor::BlackIce` entry in the
///    architecture's floor list.
/// 5. The target floor is **not** a `Floor::Demon` — returns
///    [`RulesError::CannotSlideDemon`].
///
/// ## See p.200.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlideAction {
    /// The Netrunner performing the Slide.
    pub netrunner: EntityId,
    /// Index into the architecture's `floors` list for the Black ICE to
    /// flee from. Must be a `Floor::BlackIce` (not `Floor::Demon`).
    ///
    /// See p.200 (Slide).
    pub target_ice_floor: usize,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

/// Outcome of a resolved [`SlideAction`].
///
/// Both `breakdown` (the Netrunner's contested roll) and `ice_breakdown`
/// (the ICE's roll) are always populated regardless of success or failure.
#[derive(Clone, Debug, PartialEq)]
pub struct SlideOutcome {
    /// Full breakdown of the Netrunner's `INT + Interface + 1d10` roll.
    ///
    /// See p.200 (Slide roll formula).
    pub breakdown: CheckBreakdown,
    /// The ICE's contested roll: `ICE.per + 1d10`.
    ///
    /// Stored as a `CheckBreakdown` for consistency; the `stat_value` field
    /// carries the ICE's PER stat, `skill_value` is 0 (the ICE has no skill
    /// rank separate from PER), and `dv` is set to `DV(0)` (unused — the
    /// ICE roll is not compared to a fixed DV but to the Netrunner's total).
    ///
    /// See p.200 and pp.206–207 (PER column in the Black ICE table).
    pub ice_breakdown: CheckBreakdown,
    /// `true` iff the Netrunner's `final_value` strictly exceeds the ICE's
    /// `final_value`. A tie is a failure for the Netrunner.
    ///
    /// See p.200.
    pub slid: bool,
}

impl Resolution for SlideAction {
    type Outcome = Result<SlideOutcome, RulesError>;

    /// Resolve the Slide NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Check that a netrun is active; return [`RulesError::NetrunNotActive`]
    ///    if not.
    /// 2. Check `slide_used_this_turn`; return
    ///    [`RulesError::SlideAlreadyUsedThisTurn`] if already used.
    /// 3. Look up the Netrunner entity; return
    ///    [`RulesError::EntityNotFound`] if missing.
    /// 4. Validate and spend LUCK via `actor.spend_luck(self.luck_to_spend)`.
    /// 5. Capture `INT` and `role_rank` (Interface rank).
    /// 6. Fetch the target floor from the architecture's floor list; return
    ///    [`RulesError::CannotSlideDemon`] if it is a `Floor::Demon`, or
    ///    [`RulesError::SlideTargetNotBlackIce`] if it is not a
    ///    `Floor::BlackIce`.
    /// 7. Extract the ICE's `per` stat.
    /// 8. Roll `d10_with_crits(rng)` for the Netrunner, then
    ///    `d10_with_crits(rng)` for the ICE (in this order, so replays are
    ///    deterministic).
    /// 9. Build both `CheckBreakdown`s.
    /// 10. Determine `slid = runner_total > ice_total`.
    /// 11. If `slid`, set `floor.state = BlackIceState::Slid`.
    /// 12. Set `slide_used_this_turn = true` and increment
    ///     `net_actions_used_this_turn`.
    /// 13. Return `Ok(SlideOutcome { breakdown, ice_breakdown, slid })`.
    ///
    /// See p.200 (Slide), p.130 (LUCK), p.198 (NET Actions).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — netrun must be active. See p.198.
        let netrun = world.netrun.as_ref().ok_or(RulesError::NetrunNotActive)?;

        // Step 2 — once per Turn gate. See p.200.
        if netrun.slide_used_this_turn {
            return Err(RulesError::SlideAlreadyUsedThisTurn);
        }

        // Step 3 — look up the Netrunner entity.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 4 — validate and spend LUCK (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 5 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank lives in `role_rank` (p.198: "Interface is the
        // Netrunner Role Ability").
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 6 — fetch and validate the target floor.
        // The floor list is stored inline on `NetrunState::floors` (added by
        // this WP). We reborrow `world.netrun` as immutable here (the mutable
        // borrow of `actor` above has ended).
        //
        // We validate then mutate in separate borrows: once immutably to
        // inspect the floor type and read PER, then mutably (in step 11) to
        // set `BlackIceState::Slid`.
        let ice_per: u8 = {
            let netrun = world.netrun.as_ref().expect("netrun was Some at step 1");
            let floor = netrun.floors.get(self.target_ice_floor).ok_or(
                RulesError::SlideTargetNotBlackIce {
                    floor_idx: self.target_ice_floor,
                },
            )?;
            match floor {
                Floor::Demon { .. } => return Err(RulesError::CannotSlideDemon),
                Floor::BlackIce { ice_per, .. } => *ice_per,
                _ => {
                    return Err(RulesError::SlideTargetNotBlackIce {
                        floor_idx: self.target_ice_floor,
                    })
                }
            }
        };

        // Step 8 — roll dice. Netrunner rolls first, then ICE (deterministic
        // order — mandatory for replay correctness). See p.200.
        let runner_d10 = d10_with_crits(rng);
        let ice_d10 = d10_with_crits(rng);

        // Step 9 — build breakdowns.
        // Netrunner: INT + Interface_rank + luck_to_spend + d10. See p.200.
        // DV for the runner breakdown is set to DV(0) because we compare
        // against the ICE's roll, not a fixed DV. `success` and `margin` in
        // this breakdown reflect runner_total vs. 0 — callers should use
        // `SlideOutcome::slid` for the actual result.
        use crate::types::DV;
        let breakdown = CheckBreakdown::new(
            int,
            interface_rank,
            0,
            self.luck_to_spend,
            runner_d10,
            DV(0),
        );

        // ICE breakdown: stat_value = ICE.per, skill_value = 0, modifier = 0,
        // luck = 0, dv = DV(0) (not checked against a fixed DV).
        let ice_breakdown = CheckBreakdown::new(i16::from(ice_per), 0, 0, 0, ice_d10, DV(0));

        // Step 10 — determine outcome: runner must strictly exceed the ICE.
        // A tie is a Slide failure (p.200: "successful Slide Check *against*"
        // the ICE implies the runner must beat, not merely match, the ICE).
        let slid = breakdown.final_value > ice_breakdown.final_value;

        // Step 11 — mutate floor state on success.
        if slid {
            if let Some(Floor::BlackIce { state, .. }) = world
                .netrun
                .as_mut()
                .and_then(|nr| nr.floors.get_mut(self.target_ice_floor))
            {
                *state = BlackIceState::Slid;
            }
        }

        // Step 12 — mark slide used and consume one NET Action.
        if let Some(netrun) = world.netrun.as_mut() {
            netrun.slide_used_this_turn = true;
            netrun.net_actions_used_this_turn = netrun.net_actions_used_this_turn.saturating_add(1);
        }

        Ok(SlideOutcome {
            breakdown,
            ice_breakdown,
            slid,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::black_ice::BlackIceId;
    use crate::catalog::demons::DemonId;
    use crate::character::Role;
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::architecture::{BlackIceState, Floor};
    use crate::netrunning::state::NetrunState;
    use crate::types::EntityId;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// Build a minimal `NetrunState` with a single `Floor::BlackIce` at
    /// index 0 whose PER is `ice_per`. The `slide_used_this_turn` flag
    /// starts at `false`.
    fn netrun_with_black_ice(netrunner: EntityId, ice_per: u8) -> NetrunState {
        let mut nr = NetrunState::start(netrunner, NetArchId("test-arch".into()), 5);
        nr.floors = vec![Floor::BlackIce {
            template: BlackIceId("hellhound".into()),
            state: BlackIceState::InCombat,
            ice_per,
        }];
        nr
    }

    // -------------------------------------------------------------------------
    // test_slide_succeeds_high_roll
    // -------------------------------------------------------------------------

    /// When the Netrunner's roll strictly exceeds the ICE's roll the Slide
    /// succeeds, `slid == true`, and the floor state is set to
    /// `BlackIceState::Slid`.
    ///
    /// We construct the scenario with a fixed RNG seed that drives the
    /// Netrunner's d10 high and the ICE's d10 low relative to the stat gap.
    /// INT 8 + Interface 6 + d10 vs ICE PER 2 + d10: with any non-critical
    /// failure d10 the Netrunner's base of 14 dwarfs the ICE's base of 2.
    #[test]
    fn test_slide_succeeds_high_roll() {
        // INT 8, Interface 6 → base 14. ICE PER 2 → base 2.
        // Unless the runner crits down and ICE crits up, runner wins.
        let pc = netrunner_pc(8, 6);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        world.netrun = Some(netrun_with_black_ice(pc_id, 2));

        let action = SlideAction {
            netrunner: pc_id,
            target_ice_floor: 0,
            luck_to_spend: 0,
        };

        // Try seeds until we get a success (should be almost every seed).
        let mut found = false;
        for seed in 0u64..50 {
            let mut world2 = world.clone();
            let mut rng = crate::rng::Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world2, &mut rng)
                .expect("resolve must not error");
            if outcome.slid {
                // Verify floor state was updated.
                match world2.netrun.as_ref().unwrap().floors.first() {
                    Some(Floor::BlackIce { state, .. }) => {
                        assert_eq!(
                            *state,
                            BlackIceState::Slid,
                            "floor state must be Slid on success"
                        );
                    }
                    other => panic!("expected BlackIce floor, got {other:?}"),
                }
                // Verify slide_used_this_turn was set.
                assert!(
                    world2.netrun.as_ref().unwrap().slide_used_this_turn,
                    "slide_used_this_turn must be true after use"
                );
                found = true;
                break;
            }
        }
        assert!(
            found,
            "expected at least one successful Slide in 50 seeds (INT=8, Interface=6 vs ICE PER=2)"
        );
    }

    // -------------------------------------------------------------------------
    // test_slide_fails_low_roll
    // -------------------------------------------------------------------------

    /// When the Netrunner's roll does not strictly exceed the ICE's roll,
    /// `slid == false` and the floor state remains `InCombat`.
    ///
    /// We use INT 1 + Interface 1 (base 2) vs ICE PER 8 (base 8) — the
    /// ICE base is so high the runner will almost always lose.
    #[test]
    fn test_slide_fails_low_roll() {
        let pc = netrunner_pc(1, 1);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        world.netrun = Some(netrun_with_black_ice(pc_id, 8));

        let action = SlideAction {
            netrunner: pc_id,
            target_ice_floor: 0,
            luck_to_spend: 0,
        };

        let mut found = false;
        for seed in 0u64..100 {
            let mut world2 = world.clone();
            let mut rng = crate::rng::Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world2, &mut rng)
                .expect("resolve must not error");
            if !outcome.slid {
                // Floor state must remain InCombat.
                match world2.netrun.as_ref().unwrap().floors.first() {
                    Some(Floor::BlackIce { state, .. }) => {
                        assert_eq!(
                            *state,
                            BlackIceState::InCombat,
                            "floor state must stay InCombat on failure"
                        );
                    }
                    other => panic!("expected BlackIce floor, got {other:?}"),
                }
                found = true;
                break;
            }
        }
        assert!(
            found,
            "expected at least one failed Slide in 100 seeds (INT=1, Interface=1 vs ICE PER=8)"
        );
    }

    // -------------------------------------------------------------------------
    // test_slide_rejects_demon
    // -------------------------------------------------------------------------

    /// Targeting a `Floor::Demon` returns `Err(RulesError::CannotSlideDemon)`.
    ///
    /// Per p.200: "Attempt to flee combat with a single **Non-Demon** Black
    /// ICE Program." Demons are explicitly excluded.
    #[test]
    fn test_slide_rejects_demon() {
        let pc = netrunner_pc(8, 6);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        // Build a NetrunState with a Demon floor at index 0.
        let mut nr = NetrunState::start(pc_id, NetArchId("test-arch".into()), 5);
        nr.floors = vec![Floor::Demon {
            template: DemonId("imp".into()),
            control_nodes: vec![],
        }];
        world.netrun = Some(nr);

        let action = SlideAction {
            netrunner: pc_id,
            target_ice_floor: 0,
            luck_to_spend: 0,
        };

        let mut rng = crate::rng::Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);

        assert!(
            matches!(result, Err(RulesError::CannotSlideDemon)),
            "expected CannotSlideDemon, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_slide_consumes_one_action
    // -------------------------------------------------------------------------

    /// After a successful Slide, `net_actions_used_this_turn` is incremented
    /// by 1 and `slide_used_this_turn` is `true`. A second `SlideAction`
    /// in the same turn returns `Err(RulesError::SlideAlreadyUsedThisTurn)`.
    ///
    /// Per p.200: "You can only attempt to Slide once per Turn."
    #[test]
    fn test_slide_consumes_one_action() {
        let pc = netrunner_pc(8, 6);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        world.netrun = Some(netrun_with_black_ice(pc_id, 2));

        let action = SlideAction {
            netrunner: pc_id,
            target_ice_floor: 0,
            luck_to_spend: 0,
        };

        // First resolution — must succeed and consume one NET Action.
        let mut rng = crate::rng::Rng::seed_from_u64(0);
        let _outcome = action
            .resolve(&mut world, &mut rng)
            .expect("first Slide must not error");

        let nr = world.netrun.as_ref().unwrap();
        assert_eq!(
            nr.net_actions_used_this_turn, 1,
            "first Slide consumes one NET Action"
        );
        assert!(
            nr.slide_used_this_turn,
            "slide_used_this_turn must be true after first use"
        );

        // Second resolution this turn — must be rejected.
        let mut rng2 = crate::rng::Rng::seed_from_u64(1);
        let result = action.resolve(&mut world, &mut rng2);
        assert!(
            matches!(result, Err(RulesError::SlideAlreadyUsedThisTurn)),
            "expected SlideAlreadyUsedThisTurn on second use, got {result:?}"
        );
    }
}
