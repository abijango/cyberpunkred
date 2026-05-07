//! Interface Ability: Eye-Dee — identify a File found in a NET Architecture.
//!
//! ## Rulebook (p.199)
//!
//! > **Eye-Dee** — Allows you to know what a found piece of data (like a File)
//! > is and its value using a NET Action. Some Files have a DV that must be
//! > beaten to learn anything from them.
//! >
//! > *Example: After discovering an interestingly titled File, the Netrunner
//! > uses their Eye-Dee Ability with a NET Action. It's a DV9 File, so the
//! > Netrunner rolls Interface (7) + 1d10 and easily rolls higher than 9.
//! > Unfortunately, the File was a dummy left in the Architecture just to
//! > waste a Netrunner's time!*
//!
//! ## Key rules notes (p.199)
//!
//! - **NET Action**: Eye-Dee consumes one NET Action (p.198).
//! - Roll formula: `INT + Interface_rank + 1d10` vs. the File's DV. See p.199.
//! - On success: the Netrunner learns what the File actually contains —
//!   real data, a decoy, or an encrypted payload requiring an additional check.
//! - Floor must contain a [`Floor::File`][crate::netrunning::architecture::Floor::File]
//!   for identification to yield a result; any other floor type produces
//!   `identification = None` (nothing to identify).
//! - On failure: the check is wasted but the NET Action is still consumed.
//!
//! ## Deviation note
//!
//! The rulebook does not specify a fixed DV for Eye-Dee independent of the
//! File — each File has its own DV embedded in the architecture. The caller
//! is responsible for supplying that DV via [`EyeDeeAction::dv`]. When a
//! File has no special DV, `DV(9)` (`DV::SIMPLE`) is a natural default per
//! the p.199 example. See p.199.

use crate::dice::d10_with_crits;
use crate::error::RulesError;
use crate::netrunning::architecture::{FileContents, Floor};
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A NET Action Eye-Dee check. See p.199.
///
/// The Netrunner rolls `INT + Interface_rank + 1d10` vs. [`EyeDeeAction::dv`].
/// On success, the File's contents are revealed via [`IdentificationDetail`].
/// The check consumes **one NET Action** — callers must decrement
/// [`crate::netrunning::state::NetrunState::net_actions_used_this_turn`] after
/// a successful resolution.
///
/// ## When `identification` is `None`
///
/// If the current floor is not a [`Floor::File`], `identification` is `None`
/// — there is nothing to identify on this floor. This is not an error; the
/// NET Action is still consumed.
///
/// See p.199 (Eye-Dee).
// Note: `Floor` is not `Eq` (it contains `f64`-using types indirectly via
// `ControlTarget`), so `EyeDeeAction` can only derive `PartialEq`. See p.199.
#[derive(Clone, Debug, PartialEq)]
pub struct EyeDeeAction {
    /// The Netrunner performing the Eye-Dee check.
    pub netrunner: EntityId,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
    /// The DV of the File on the current floor (embedded in the architecture,
    /// e.g. `DV(9)` per the p.199 example). Caller obtains this from the
    /// [`Floor::File`][crate::netrunning::architecture::Floor::File] entry.
    pub dv: DV,
    /// The floor to inspect. Eye-Dee only produces a result if this is a
    /// [`Floor::File`]; all other floor types yield `identification = None`.
    ///
    /// Taking the floor by value avoids a lifetime dependency on the
    /// architecture while keeping the API deterministic.
    ///
    /// See p.199.
    pub floor: Floor,
}

/// Outcome of a resolved [`EyeDeeAction`]. See p.199.
///
/// `breakdown` is always populated. `identification` is `Some` only when the
/// check succeeded **and** the floor was a [`Floor::File`]; it is `None` on
/// failure or if the current floor is not a File.
#[derive(Clone, Debug, PartialEq)]
pub struct EyeDeeOutcome {
    /// Full breakdown of the `INT + Interface_rank + 1d10` roll.
    pub breakdown: CheckBreakdown,
    /// What the Netrunner learned about the File, or `None`.
    ///
    /// - `None` if the check failed, or if the floor is not a `File`.
    /// - `Some(IdentificationDetail::File(_))` — real data; the contents
    ///   description is revealed.
    /// - `Some(IdentificationDetail::DecoyDetected)` — the File is a dummy.
    /// - `Some(IdentificationDetail::Encrypted { unlock_dv })` — the File
    ///   exists but is locked; a further check at `unlock_dv` is needed.
    pub identification: Option<IdentificationDetail>,
}

/// What Eye-Dee reveals about a [`Floor::File`]'s contents. See p.199.
///
/// Mirrors [`FileContents`][crate::netrunning::architecture::FileContents]
/// but adds semantic framing (the Netrunner's *knowledge* rather than the
/// architecture's *ground truth*).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentificationDetail {
    /// The File contains genuine data. The `String` is the contents description
    /// — e.g. `"Corporate payroll records"` — revealed to the Netrunner.
    ///
    /// See p.199 (Eye-Dee example where the file is a "dummy"; success with
    /// real data instead would reveal the data's description).
    File(String),
    /// The File is a decoy — a dummy left to waste the Netrunner's time.
    ///
    /// > *"Unfortunately, the File was a dummy left in the Architecture just
    /// > to waste a Netrunner's time!"* — p.199 example.
    DecoyDetected,
    /// The File exists but is encrypted; a second Interface check at
    /// `unlock_dv` is required to read its contents.
    ///
    /// The Netrunner now knows the File exists and its encryption DV —
    /// enough to decide whether attempting to crack it is worthwhile.
    ///
    /// See p.199 (Eye-Dee) and p.210 (File DV).
    Encrypted {
        /// DV for the second Interface check to decrypt the File.
        unlock_dv: DV,
    },
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for EyeDeeAction {
    type Outcome = Result<EyeDeeOutcome, RulesError>;

    /// Resolve the Eye-Dee NET Action against `world`. See p.199.
    ///
    /// ## Steps
    ///
    /// 1. Look up the Netrunner via [`World::entity_mut`]. If missing, return
    ///    `Err(RulesError::EntityNotFound)`.
    /// 2. Validate and spend LUCK via [`Character::spend_luck`]. Returns
    ///    `Err(RulesError::InsufficientLuck)` on failure.
    /// 3. Capture `INT` and `role_rank` (the Interface rank) from the character.
    /// 4. Roll `d10_with_crits(rng)`.
    /// 5. Build [`CheckBreakdown`] from `(int, interface_rank, 0, luck_spent, d10, dv)`.
    /// 6. On success, map the floor's [`FileContents`] to [`IdentificationDetail`].
    ///    On failure, or when the floor is not a `File`, `identification = None`.
    /// 7. Increment `net_actions_used_this_turn` by 1 on the active netrun state,
    ///    if one exists in the world.
    ///
    /// See p.199 (Eye-Dee), p.198 (NET Actions), p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — look up the entity. See p.199.
        let actor = world
            .entity_mut(self.netrunner)
            .ok_or(RulesError::EntityNotFound(self.netrunner))?;

        // Step 2 — validate and spend LUCK (p.130).
        actor.spend_luck(self.luck_to_spend)?;

        // Step 3 — capture roll inputs.
        // INT is the linked STAT for Interface checks (p.199).
        // Interface rank is `role_rank` (p.198–199): "Interface is the
        // Netrunner Role Ability". There is no `SkillId::Interface` in the
        // closed enum — the role-ability rank fills the skill column.
        let int = actor.current_int();
        let interface_rank = actor.role_rank as i16;

        // Step 4 — roll with crit rules (p.129–130).
        let d10 = d10_with_crits(rng);

        // Step 5 — build the breakdown.
        // stat_value   = INT (the STAT linked to Interface checks)
        // skill_value  = Interface Role Ability rank (plays the skill column)
        // modifier_total = 0 (no situational modifiers in base Eye-Dee)
        // See p.199.
        let breakdown =
            CheckBreakdown::new(int, interface_rank, 0, self.luck_to_spend, d10, self.dv);

        // Step 6 — determine identification result.
        // Only a File floor yields identification; all other floor types
        // produce `None` (nothing to Eye-Dee here). See p.199.
        let identification = if breakdown.success {
            match &self.floor {
                Floor::File { contents, .. } => Some(map_contents(contents)),
                // Non-File floor: nothing to identify. See p.199.
                _ => None,
            }
        } else {
            // Failed check: Netrunner learns nothing. See p.199.
            None
        };

        // Step 7 — consume one NET Action on the active netrun state.
        // Netrun state may not exist (e.g. in unit tests that don't set it up).
        // We increment defensively when it is present. See p.198 (NET Actions).
        if let Some(ref mut netrun) = world.netrun {
            netrun.net_actions_used_this_turn = netrun.net_actions_used_this_turn.saturating_add(1);
        }

        Ok(EyeDeeOutcome {
            breakdown,
            identification,
        })
    }
}

// ---------------------------------------------------------------------------
// Internal helper
// ---------------------------------------------------------------------------

/// Map [`FileContents`] to the Netrunner's discovered [`IdentificationDetail`].
///
/// This is a 1:1 mapping; the distinction is semantic — `FileContents` is the
/// architecture's ground truth, `IdentificationDetail` is what the Netrunner
/// perceives after a successful Eye-Dee check. See p.199.
fn map_contents(contents: &FileContents) -> IdentificationDetail {
    match contents {
        FileContents::Data(description) => IdentificationDetail::File(description.clone()),
        FileContents::Decoy => IdentificationDetail::DecoyDetected,
        FileContents::Encrypted { unlock_dv } => IdentificationDetail::Encrypted {
            unlock_dv: *unlock_dv,
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::black_ice::BlackIceId;
    use crate::character::Role;
    use crate::dice::{CritD10, D10Outcome};
    use crate::netrunning::architecture::{BlackIceState, FileContents, Floor, NetArchId};
    use crate::netrunning::state::NetrunState;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a `CritD10` with a known net value, simulating a normal
    /// (non-crit) d10 roll of `roll`.
    fn fake_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    /// Construct a Netrunner PC with given INT and role_rank.
    fn netrunner_pc(int: u8, role_rank: u8) -> crate::character::Character {
        let mut pc = fresh_pc();
        pc.role = Role::Netrunner;
        pc.stats.int = int;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        pc
    }

    /// A File floor containing real data at DV9. See p.199 example.
    fn data_file_floor() -> Floor {
        Floor::File {
            dv: DV(9),
            contents: FileContents::Data("Corporate payroll records".into()),
        }
    }

    /// A File floor containing a Decoy at DV9. See p.199 example.
    fn decoy_file_floor() -> Floor {
        Floor::File {
            dv: DV(9),
            contents: FileContents::Decoy,
        }
    }

    /// A File floor containing an Encrypted payload at DV9 (unlock DV12).
    fn encrypted_file_floor() -> Floor {
        Floor::File {
            dv: DV(9),
            contents: FileContents::Encrypted { unlock_dv: DV(12) },
        }
    }

    /// A Black ICE floor (non-File), used to test that `identification = None`
    /// when the floor has nothing to identify.
    fn non_file_floor() -> Floor {
        Floor::BlackIce {
            template: BlackIceId("hellhound".into()),
            state: BlackIceState::LyingInWait,
            ice_per: 0,
        }
    }

    /// Build a `World` with a Netrunner PC and an active `NetrunState`.
    fn world_with_netrun(int: u8, role_rank: u8) -> (World, EntityId) {
        let pc = netrunner_pc(int, role_rank);
        let pc_entity_id = EntityId(pc.id.0);
        let mut world = World::new(pc);
        world.netrun = Some(NetrunState::start(
            pc_entity_id,
            NetArchId("test-arch".into()),
            role_rank,
        ));
        (world, pc_entity_id)
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_identifies_real_data
    // -------------------------------------------------------------------------

    /// A successful Eye-Dee check on a real-data File reveals the contents.
    ///
    /// Setup: INT 7, Interface 7, d10 forced high enough for guaranteed success
    /// vs DV9. Expected: `identification = Some(IdentificationDetail::File(_))`.
    ///
    /// See p.199.
    #[test]
    fn test_eye_dee_identifies_real_data() {
        // INT 7, Interface rank 7 → base 14 before d10. Any roll ≥ 1 beats DV9.
        // Verify the formula directly.
        let d10 = fake_d10(3); // 7 + 7 + 3 = 17 vs DV9 → success, margin 8.
        let bd = CheckBreakdown::new(7, 7, 0, 0, d10, DV(9));
        assert!(bd.success, "7+7+3=17 vs DV9 must succeed");

        // Integration: resolve against world.
        let (world, pc_id) = world_with_netrun(7, 7);

        let action = EyeDeeAction {
            netrunner: pc_id,
            luck_to_spend: 0,
            dv: DV(9),
            floor: data_file_floor(),
        };

        // Find a seed that produces a success (should be very likely with INT+Interface=14).
        let mut found = false;
        for seed in 0u64..50 {
            // Re-create world each iteration since resolve mutates it.
            let (mut w, id) = world_with_netrun(7, 7);
            let a = EyeDeeAction {
                netrunner: id,
                luck_to_spend: 0,
                dv: DV(9),
                floor: data_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = a.resolve(&mut w, &mut rng).expect("resolve must not error");
            if outcome.breakdown.success {
                assert!(
                    matches!(
                        outcome.identification,
                        Some(IdentificationDetail::File(ref s)) if s == "Corporate payroll records"
                    ),
                    "successful Eye-Dee on data File must reveal contents, got {:?}",
                    outcome.identification
                );
                found = true;
                break;
            }
        }
        assert!(
            found,
            "expected at least one success seed with INT=7, Interface=7 vs DV9"
        );

        // Suppress unused variable warning.
        let _ = (world, action);
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_identifies_decoy
    // -------------------------------------------------------------------------

    /// A successful Eye-Dee check on a Decoy File reveals `DecoyDetected`.
    ///
    /// Per p.199 example: "Unfortunately, the File was a dummy left in the
    /// Architecture just to waste a Netrunner's time!"
    #[test]
    fn test_eye_dee_identifies_decoy() {
        for seed in 0u64..100 {
            let (mut world, pc_id) = world_with_netrun(7, 7);
            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(9),
                floor: decoy_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if outcome.breakdown.success {
                assert_eq!(
                    outcome.identification,
                    Some(IdentificationDetail::DecoyDetected),
                    "successful Eye-Dee on Decoy File must return DecoyDetected"
                );
                return;
            }
        }
        panic!("could not find a success seed for INT=7, Interface=7 vs DV9 in 100 attempts");
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_identifies_encrypted
    // -------------------------------------------------------------------------

    /// A successful Eye-Dee check on an Encrypted File reveals
    /// `IdentificationDetail::Encrypted { unlock_dv }`.
    #[test]
    fn test_eye_dee_identifies_encrypted() {
        for seed in 0u64..100 {
            let (mut world, pc_id) = world_with_netrun(7, 7);
            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(9),
                floor: encrypted_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if outcome.breakdown.success {
                assert_eq!(
                    outcome.identification,
                    Some(IdentificationDetail::Encrypted { unlock_dv: DV(12) }),
                    "successful Eye-Dee on Encrypted File must reveal unlock DV"
                );
                return;
            }
        }
        panic!("could not find a success seed for INT=7, Interface=7 vs DV9 in 100 attempts");
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_consumes_one_action
    // -------------------------------------------------------------------------

    /// Eye-Dee always consumes one NET Action on the active netrun state,
    /// regardless of success or failure. See p.198 (NET Actions), p.199.
    #[test]
    fn test_eye_dee_consumes_one_action() {
        // Success path.
        for seed in 0u64..200 {
            let (mut world, pc_id) = world_with_netrun(7, 7);
            assert_eq!(
                world.netrun.as_ref().unwrap().net_actions_used_this_turn,
                0,
                "starts at 0 actions"
            );

            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(9),
                floor: data_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if outcome.breakdown.success {
                assert_eq!(
                    world.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "Eye-Dee must consume exactly one NET Action on success"
                );
                // Verify failure path separately.
                break;
            }
        }

        // Failure path: high DV, low stats.
        for seed in 0u64..500 {
            let (mut world, pc_id) = world_with_netrun(1, 1);
            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(24), // DV::INCREDIBLE — virtually impossible for INT=1, Interface=1
                floor: data_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if !outcome.breakdown.success {
                assert_eq!(
                    world.netrun.as_ref().unwrap().net_actions_used_this_turn,
                    1,
                    "Eye-Dee must consume one NET Action even on failure"
                );
                assert_eq!(
                    outcome.identification, None,
                    "failed check yields no identification"
                );
                return;
            }
        }
        panic!("could not find a failure seed for INT=1, Interface=1 vs DV24 in 500 attempts");
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_non_file_floor_returns_none
    // -------------------------------------------------------------------------

    /// Eye-Dee on a non-File floor (e.g. Black ICE) yields `identification = None`
    /// even on success. See p.199.
    #[test]
    fn test_eye_dee_non_file_floor_returns_none() {
        for seed in 0u64..100 {
            let (mut world, pc_id) = world_with_netrun(7, 7);
            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(9),
                floor: non_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if outcome.breakdown.success {
                assert_eq!(
                    outcome.identification, None,
                    "Eye-Dee on a non-File floor must yield None even on success"
                );
                return;
            }
        }
        panic!("could not find a success seed for INT=7, Interface=7 vs DV9 in 100 attempts");
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_entity_not_found
    // -------------------------------------------------------------------------

    /// Looking up a non-existent entity must return `Err(RulesError::EntityNotFound)`.
    #[test]
    fn test_eye_dee_entity_not_found() {
        use uuid::Uuid;

        let pc = netrunner_pc(6, 4);
        let mut world = World::new(pc);

        let bad_id = EntityId(Uuid::from_u128(0xDEAD_BEEF));
        let action = EyeDeeAction {
            netrunner: bad_id,
            luck_to_spend: 0,
            dv: DV(9),
            floor: data_file_floor(),
        };

        let mut rng = Rng::seed_from_u64(0);
        let result = action.resolve(&mut world, &mut rng);
        assert!(
            matches!(result, Err(RulesError::EntityNotFound(id)) if id == bad_id),
            "expected EntityNotFound, got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_insufficient_luck
    // -------------------------------------------------------------------------

    /// Spending more LUCK than available must return `Err(RulesError::InsufficientLuck)`.
    #[test]
    fn test_eye_dee_insufficient_luck() {
        let mut pc = netrunner_pc(6, 4);
        pc.luck_pool = 0;
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let action = EyeDeeAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            dv: DV(9),
            floor: data_file_floor(),
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
            "expected InsufficientLuck(3/0), got {result:?}"
        );
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_luck_adds_to_check
    // -------------------------------------------------------------------------

    /// Spending N LUCK adds N to the final check value and decrements the pool.
    #[test]
    fn test_eye_dee_luck_adds_to_check() {
        let pc = netrunner_pc(6, 4);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let action = EyeDeeAction {
            netrunner: pc_id,
            luck_to_spend: 3,
            dv: DV(9),
            floor: data_file_floor(),
        };
        let mut rng = Rng::seed_from_u64(7);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must not error");

        assert_eq!(outcome.breakdown.luck_spent, 3, "luck_spent must be 3");
        // final_value = INT + Interface + 0 + 3 + d10.net
        assert_eq!(
            outcome.breakdown.final_value,
            6 + 4 + 3 + outcome.breakdown.d10.net
        );
        // Pool decremented: 10 - 3 = 7.
        assert_eq!(world.pc.luck_pool, 7, "luck pool 10 - 3 spent = 7");
    }

    // -------------------------------------------------------------------------
    // test_eye_dee_failure_no_identification
    // -------------------------------------------------------------------------

    /// A failed check — regardless of floor type — always returns
    /// `identification = None`. See p.199.
    #[test]
    fn test_eye_dee_failure_no_identification() {
        // Low stats, high DV: near-guaranteed failure.
        for seed in 0u64..500 {
            let (mut world, pc_id) = world_with_netrun(1, 1);
            let action = EyeDeeAction {
                netrunner: pc_id,
                luck_to_spend: 0,
                dv: DV(24),
                floor: data_file_floor(),
            };
            let mut rng = Rng::seed_from_u64(seed);
            let outcome = action
                .resolve(&mut world, &mut rng)
                .expect("resolve must not error");
            if !outcome.breakdown.success {
                assert_eq!(
                    outcome.identification, None,
                    "failed check must yield no identification"
                );
                return;
            }
        }
        panic!("could not find a failure seed for INT=1, Interface=1 vs DV24 in 500 attempts");
    }
}
