//! Activation of Booster and Defender programs. See p.201.
//!
//! Activating a program is a NET Action. The Netrunner names a program
//! installed on their Cyberdeck; if it is a Booster or Defender it enters
//! the Rezzed state (current\_rez = max\_rez = `program.rez`), and its
//! passive [`crate::effects::EffectModifier`]s are pushed onto the
//! Netrunner's [`crate::effects::EffectStack`].
//!
//! ## One activation per Round (p.201)
//!
//! The book states that a program can only be activated once per Round.
//! This constraint is enforced by the **caller** (typically the turn engine
//! or the GM layer) — it is not re-checked here. See
//! [`test_one_per_round_constraint_documented`][tests] for the regression
//! note and rationale.
//!
//! ## Attacker programs
//!
//! Attacker programs (`ProgramClass::AntiPersonnelAttacker` and
//! `ProgramClass::AntiProgramAttacker`) are handled by WP-413. Passing an
//! Attacker to [`activate_booster_or_defender`] returns
//! [`RulesError::ProgramWrongClass`].
//!
//! ## Rulebook references
//!
//! - **p.201** — "Rezzed Programs", "Activating a Program", the one-per-Round
//!   limit, and the "Defeating a Program" sidebar.
//! - **p.202** — program table layout (Class / ATK / DEF / REZ / Effect).
//! - **p.203** — Booster effects (Eraser, See Ya, Speedy Gonzalvez, Worm) and
//!   Defender effects (Armor, Flak, Shield).

use crate::catalog::programs::{BoostableCheck, Program, ProgramClass, ProgramEffect};
use crate::catalog::Catalog;
use crate::effects::{ActiveEffect, EffectDuration, EffectModifier, EffectSource, ProgramId};
use crate::error::RulesError;
use crate::netrunning::state::ProgramInstanceId;
use crate::types::{EffectInstanceId, EntityId, Stat};
use crate::world::World;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public request / outcome types
// ---------------------------------------------------------------------------

/// A request to activate a Booster or Defender program for a Netrunner.
///
/// The caller is responsible for:
/// 1. Spending one NET Action from [`crate::netrunning::state::NetrunState`].
/// 2. Enforcing the one-activation-per-Round rule per p.201.
///
/// See p.201.
#[derive(Debug)]
pub struct ActivateProgram {
    /// The Netrunner performing the activation. Must resolve via
    /// [`World::entity`] to a [`crate::character::Character`] with an
    /// [`crate::effects::EffectStack`].
    pub netrunner: EntityId,
    /// Catalog slug of the program to activate (e.g. `"speedy_gonzalvez"`).
    pub program: ProgramId,
}

/// The result of a successful program activation.
///
/// Returned by [`activate_booster_or_defender`] when the program is valid
/// and the Netrunner entity exists.
#[derive(Debug)]
pub struct ActivationOutcome {
    /// Unique identifier assigned to this rezzed instance.
    ///
    /// Minted deterministically by [`crate::netrunning::state::NetrunState::rez_program`].
    /// The caller must retain this to derez the program later (e.g. when
    /// the Netrunner jacks out or the program is defeated).
    ///
    /// See p.201 (Defeating a Program).
    pub instance_id: ProgramInstanceId,
    /// The [`EffectInstanceId`]s of every modifier pushed onto the
    /// Netrunner's [`crate::effects::EffectStack`] by this activation.
    ///
    /// The caller must retain these to remove the effects when the program
    /// is derezzed.
    pub effects_added: Vec<EffectInstanceId>,
}

// ---------------------------------------------------------------------------
// Core activation function
// ---------------------------------------------------------------------------

/// Activate a Booster or Defender program, updating world state and the
/// Netrunner's effect stack.
///
/// # What this does (p.201)
///
/// 1. Looks up `request.program` in `catalog`. Returns
///    [`RulesError::ProgramNotFound`] if the slug is unknown.
/// 2. Validates that the program class is [`ProgramClass::Booster`] or
///    [`ProgramClass::Defender`]. Returns [`RulesError::ProgramWrongClass`]
///    if an Attacker slug is given.
/// 3. Adds the program to `world.netrun.rezzed_programs` via
///    [`crate::netrunning::state::NetrunState::rez_program`] with
///    `current_rez = max_rez = program.rez`.
/// 4. Translates the program's [`ProgramEffect`] into one or more
///    [`EffectModifier`]s and pushes them onto the Netrunner's
///    [`crate::effects::EffectStack`] with source
///    [`EffectSource::Program(program_id)`] and duration
///    [`EffectDuration::UntilEndOfNetrun`].
///
/// # One-per-Round constraint
///
/// The caller **must** enforce p.201's "a program can only be activated
/// once per Round" rule. This function does not re-check it so that the
/// same code path can be used by replay (which reconstructs each individual
/// activation from the action log).
///
/// # Errors
///
/// - [`RulesError::ProgramNotFound`] — unknown slug.
/// - [`RulesError::ProgramWrongClass`] — Attacker program passed.
/// - [`RulesError::EntityNotFound`] — `request.netrunner` does not resolve.
/// - [`RulesError::NoActiveNetrun`] — `world.netrun` is `None`.
///
/// See p.201.
pub fn activate_booster_or_defender(
    world: &mut World,
    catalog: &Catalog<Program>,
    request: ActivateProgram,
) -> Result<ActivationOutcome, RulesError> {
    // 1. Catalog lookup. See p.201.
    let program = catalog
        .get(&request.program.0)
        .ok_or_else(|| RulesError::ProgramNotFound(request.program.clone()))?;

    // 2. Class guard — Attacker programs are handled by WP-413. See p.201.
    match program.class {
        ProgramClass::Booster | ProgramClass::Defender => {}
        _ => {
            return Err(RulesError::ProgramWrongClass {
                program: request.program.clone(),
                expected: "Booster or Defender",
                got: format!("{:?}", program.class),
            });
        }
    }

    // 3. Rez the program. current_rez = max_rez = program.rez. See p.202.
    let netrun = world.netrun.as_mut().ok_or(RulesError::NoActiveNetrun)?;
    let instance_id = netrun.rez_program(request.program.clone(), program.rez);

    // 4. Translate ProgramEffect → EffectModifier(s), push onto the
    //    Netrunner's EffectStack. See p.201, p.203.
    let modifiers = effect_modifiers_for(&program.effect);
    let mut effects_added = Vec::with_capacity(1);

    // Build a deterministic EffectInstanceId from the ProgramInstanceId's
    // inner Uuid. This avoids OS entropy and keeps the crate WASM-safe.
    // We XOR the high bits to distinguish the effect UUID from the program
    // instance UUID while still tying them together.
    let effect_id = EffectInstanceId(Uuid::from_u128(
        instance_id.0.as_u128() ^ 0xEFF0_EFF0_EFF0_EFF0,
    ));

    let active_effect = ActiveEffect {
        id: effect_id,
        source: EffectSource::Program(request.program.clone()),
        modifiers,
        duration: EffectDuration::UntilEndOfNetrun,
    };

    // Resolve the Netrunner character and push the effect.
    let character = world
        .entity_mut(request.netrunner)
        .ok_or(RulesError::EntityNotFound(request.netrunner))?;
    character.effects.add(active_effect);
    effects_added.push(effect_id);

    Ok(ActivationOutcome {
        instance_id,
        effects_added,
    })
}

// ---------------------------------------------------------------------------
// ProgramEffect → EffectModifier translation
// ---------------------------------------------------------------------------

/// Translate a [`ProgramEffect`] into the set of [`EffectModifier`]s that
/// represent its passive bonus while the program is Rezzed.
///
/// Each Booster and Defender has a distinct mechanical shape; this function
/// is the single place where the catalog's semantic description becomes a
/// query-site-readable modifier. See p.203 for all Booster / Defender effects.
fn effect_modifiers_for(effect: &ProgramEffect) -> Vec<EffectModifier> {
    match effect {
        // Booster: adds `by` to a specific Interface check or NET Speed.
        // Speedy Gonzalvez (Speed) maps to StatBonus { stat: Move } because
        // NET Speed is the MOVE-equivalent inside the Architecture (p.203).
        // The other three map to NetrunCheckBonus so consumers can
        // distinguish Interface-ability rolls from raw stat queries.
        ProgramEffect::BoostCheck { check, by } => match check {
            BoostableCheck::Speed => vec![EffectModifier::StatBonus {
                stat: Stat::Move,
                by: *by,
            }],
            other => vec![EffectModifier::NetrunCheckBonus {
                check: *other,
                by: *by,
            }],
        },

        // Defender: Armor reduces Black ICE brain damage by 4. See p.203.
        ProgramEffect::BlockBlackIceDamage { reduction } => {
            vec![EffectModifier::NetrunBrainDamageReduction(*reduction)]
        }

        // Defender: Flak zeroes non-Black-ICE Attacker ATK. See p.203.
        ProgramEffect::NullifyAttackerAtk => vec![EffectModifier::NetrunAttackerAtkNullified],

        // Defender: Shield stops the first non-Black-ICE Effect. See p.203.
        ProgramEffect::StopFirstNonBlackIceEffect => {
            vec![EffectModifier::NetrunFirstEffectBlocked]
        }

        // Attacker effects should never reach this path — activate_booster_or_defender
        // rejects them before calling here. Returning an empty vec is the safe
        // fallback (no crash, no unexpected state mutation). This branch is
        // unreachable in production but guards against future refactors.
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::programs::{DiceSpec, DieKind};
    use crate::effects::EffectModifier;
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use std::collections::HashMap;

    /// Build a minimal in-memory catalog containing only the listed programs.
    fn make_catalog(programs: Vec<Program>) -> Catalog<Program> {
        let mut entries = HashMap::new();
        for p in programs {
            entries.insert(p.id.0.clone(), p);
        }
        Catalog::new(entries)
    }

    /// Build a minimal Booster program (Speedy Gonzalvez shape).
    fn speedy_gonzalvez() -> Program {
        use crate::types::{Eurobucks, PriceTier};
        Program {
            id: ProgramId("speedy_gonzalvez".to_string()),
            display_name: "Speedy Gonzalvez".to_string(),
            class: ProgramClass::Booster,
            atk: 0,
            def: 0,
            rez: 7,
            effect: ProgramEffect::BoostCheck {
                check: BoostableCheck::Speed,
                by: 2,
            },
            icon: "A tiny chrome running shoe with rocket exhausts.".to_string(),
            price: PriceTier::Everyday,
            price_eb: Eurobucks(20),
            slot_cost: 1,
        }
    }

    /// Build a minimal Defender program (Armor shape).
    fn armor_program() -> Program {
        use crate::types::{Eurobucks, PriceTier};
        Program {
            id: ProgramId("armor".to_string()),
            display_name: "Armor".to_string(),
            class: ProgramClass::Defender,
            atk: 0,
            def: 0,
            rez: 7,
            effect: ProgramEffect::BlockBlackIceDamage { reduction: 4 },
            icon: "A shimmering force-field orb.".to_string(),
            price: PriceTier::Everyday,
            price_eb: Eurobucks(20),
            slot_cost: 1,
        }
    }

    /// Build a minimal Anti-Personnel Attacker (DeckKRASH shape) for
    /// rejection testing.
    fn deckkrash_program() -> Program {
        use crate::types::{Eurobucks, PriceTier};
        Program {
            id: ProgramId("deckkrash".to_string()),
            display_name: "DeckKRASH".to_string(),
            class: ProgramClass::AntiPersonnelAttacker,
            atk: 0,
            def: 0,
            rez: 0,
            effect: ProgramEffect::ForceUnsafeJackOut,
            icon: "A skeletal hand dragging a cyberdeck into a pit.".to_string(),
            price: PriceTier::Premium,
            price_eb: Eurobucks(100),
            slot_cost: 1,
        }
    }

    /// Build a minimal Anti-Program Attacker (Banhammer shape) for rejection
    /// testing.
    fn banhammer_program() -> Program {
        use crate::types::{Eurobucks, PriceTier};
        Program {
            id: ProgramId("banhammer".to_string()),
            display_name: "Banhammer".to_string(),
            class: ProgramClass::AntiProgramAttacker,
            atk: 1,
            def: 0,
            rez: 0,
            effect: ProgramEffect::AnyAttackerProgramDamage {
                dice_vs_non_black_ice: DiceSpec {
                    n: 3,
                    die: DieKind::D6,
                },
                dice_vs_black_ice: DiceSpec {
                    n: 2,
                    die: DieKind::D6,
                },
            },
            icon: "A cartoon gavel that glows with righteous light.".to_string(),
            price: PriceTier::Premium,
            price_eb: Eurobucks(100),
            slot_cost: 1,
        }
    }

    /// Build a World with an active NetrunState for the given EntityId.
    fn world_with_netrun(pc_uuid: uuid::Uuid) -> World {
        let mut pc = fresh_pc();
        pc.id = crate::types::CharacterId(pc_uuid);
        let mut world = World::new(pc);
        let arch = NetArchId("test-arch".to_string());
        let netrun = NetrunState::start(EntityId(pc_uuid), arch, 5);
        world.netrun = Some(netrun);
        world
    }

    // -----------------------------------------------------------------------
    // Acceptance: Speedy Gonzalvez increases speed
    // -----------------------------------------------------------------------

    /// `test_speedy_gonzalvez_increases_speed`: rezzing the Speedy Gonzalvez
    /// Booster program adds `EffectModifier::StatBonus { stat: Move, by: 2 }`.
    ///
    /// Speedy Gonzalvez is a Booster with `BoostCheck { check: Speed, by: 2 }`.
    /// Speed inside the NET Architecture is the MOVE-equivalent (p.203), so
    /// the activation translates it to a `StatBonus` on `Stat::Move`.
    ///
    /// See p.201, p.203.
    #[test]
    fn test_speedy_gonzalvez_increases_speed() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0001);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![speedy_gonzalvez()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("speedy_gonzalvez".to_string()),
        };

        let outcome = activate_booster_or_defender(&mut world, &catalog, request)
            .expect("Speedy Gonzalvez activation must succeed");

        // One effect was added.
        assert_eq!(
            outcome.effects_added.len(),
            1,
            "exactly one effect added for Speedy Gonzalvez"
        );

        // The program is now rezzed.
        let netrun = world.netrun.as_ref().unwrap();
        assert_eq!(
            netrun.rezzed_programs.len(),
            1,
            "one program rezzed after activation"
        );
        assert_eq!(
            netrun.rezzed_programs[0].program,
            ProgramId("speedy_gonzalvez".to_string())
        );
        assert_eq!(
            netrun.rezzed_programs[0].current_rez, 7,
            "current_rez starts at max_rez = 7 per p.202"
        );
        assert_eq!(netrun.rezzed_programs[0].instance_id, outcome.instance_id);

        // The StatBonus modifier is on the Netrunner's EffectStack.
        let pc = world.entity(EntityId(pc_uuid)).unwrap();
        let modifiers: Vec<&EffectModifier> = pc.effects.iter_modifiers().collect();
        assert_eq!(modifiers.len(), 1, "exactly one modifier on the stack");
        assert_eq!(
            modifiers[0],
            &EffectModifier::StatBonus {
                stat: Stat::Move,
                by: 2,
            },
            "Speedy Gonzalvez must add StatBonus {{ stat: Move, by: 2 }} per p.203"
        );
    }

    // -----------------------------------------------------------------------
    // Acceptance: Armor reduces brain damage
    // -----------------------------------------------------------------------

    /// `test_armor_reduces_brain_damage`: rezzing the Armor Defender program
    /// results in a `NetrunBrainDamageReduction(4)` modifier on the
    /// Netrunner's EffectStack.
    ///
    /// Armor is a Defender program with effect `BlockBlackIceDamage { reduction: 4 }`.
    /// Activation translates that to `EffectModifier::NetrunBrainDamageReduction(4)`.
    /// Black ICE combat resolution (WP-414) consumes this modifier to lower
    /// the actual brain damage dealt.
    ///
    /// See p.201, p.203: "Armor: Lowers all brain damage you would receive by 4."
    #[test]
    fn test_armor_reduces_brain_damage() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0002);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![armor_program()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("armor".to_string()),
        };

        let outcome = activate_booster_or_defender(&mut world, &catalog, request)
            .expect("Armor activation must succeed");

        assert_eq!(outcome.effects_added.len(), 1);

        let pc = world.entity(EntityId(pc_uuid)).unwrap();
        let modifiers: Vec<&EffectModifier> = pc.effects.iter_modifiers().collect();
        assert_eq!(modifiers.len(), 1, "exactly one modifier on the stack");
        assert_eq!(
            modifiers[0],
            &EffectModifier::NetrunBrainDamageReduction(4),
            "Armor must add NetrunBrainDamageReduction(4) per p.203"
        );

        // The program is rezzed with correct REZ. See p.202.
        let netrun = world.netrun.as_ref().unwrap();
        assert_eq!(netrun.rezzed_programs[0].current_rez, 7);
    }

    // -----------------------------------------------------------------------
    // Acceptance: Attacker class is rejected
    // -----------------------------------------------------------------------

    /// `test_rejects_attacker_class`: passing an Anti-Personnel Attacker
    /// (DeckKRASH) to [`activate_booster_or_defender`] must return
    /// `Err(RulesError::ProgramWrongClass)`. Attackers are handled by WP-413.
    ///
    /// See p.201: Attacker programs "auto-Deactivate" after firing — they
    /// are not passively rezzed like Boosters/Defenders.
    #[test]
    fn test_rejects_attacker_class() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0003);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![deckkrash_program()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("deckkrash".to_string()),
        };

        let result = activate_booster_or_defender(&mut world, &catalog, request);
        assert!(
            matches!(result, Err(RulesError::ProgramWrongClass { .. })),
            "Anti-Personnel Attacker must be rejected with ProgramWrongClass, got {result:?}"
        );

        // No side effects: the program must not be rezzed.
        let netrun = world.netrun.as_ref().unwrap();
        assert!(
            netrun.rezzed_programs.is_empty(),
            "rezzed_programs must remain empty after rejection"
        );
    }

    /// `test_rejects_attacker_class` variant for Anti-Program Attackers
    /// (Banhammer). Both Attacker sub-classes must be rejected.
    #[test]
    fn test_rejects_anti_program_attacker_class() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0004);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![banhammer_program()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("banhammer".to_string()),
        };

        let result = activate_booster_or_defender(&mut world, &catalog, request);
        assert!(
            matches!(result, Err(RulesError::ProgramWrongClass { .. })),
            "Anti-Program Attacker must be rejected with ProgramWrongClass, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Acceptance: one-per-Round constraint documented
    // -----------------------------------------------------------------------

    /// `test_one_per_round_constraint_documented`: regression note documenting
    /// that the one-activation-per-Round rule (p.201) is enforced by the
    /// **caller**, not by [`activate_booster_or_defender`].
    ///
    /// This test verifies that the function itself does *not* raise an error
    /// when the same program is activated twice in one call sequence — a
    /// necessary property for replay, where individual actions are replayed
    /// in isolation. The caller (turn engine / GM layer) is responsible for
    /// rejecting duplicate activations within the same Round.
    ///
    /// See p.201: "You can only run a program once per Round."
    #[test]
    fn test_one_per_round_constraint_documented() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0005);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![speedy_gonzalvez()]);

        // First activation — always allowed.
        let r1 = activate_booster_or_defender(
            &mut world,
            &catalog,
            ActivateProgram {
                netrunner: EntityId(pc_uuid),
                program: ProgramId("speedy_gonzalvez".to_string()),
            },
        );
        assert!(r1.is_ok(), "first activation must succeed");

        // Second activation of the same program in the same call sequence:
        // `activate_booster_or_defender` does NOT block it — the caller must.
        // This documents the contract rather than the constraint.
        let r2 = activate_booster_or_defender(
            &mut world,
            &catalog,
            ActivateProgram {
                netrunner: EntityId(pc_uuid),
                program: ProgramId("speedy_gonzalvez".to_string()),
            },
        );
        assert!(
            r2.is_ok(),
            "activate_booster_or_defender does not enforce 1/round; caller must (p.201). \
             Got: {r2:?}"
        );

        // Two rezzed instances of the same program are possible (p.201:
        // "You can run multiple copies of the same Program on your Cyberdeck").
        let netrun = world.netrun.as_ref().unwrap();
        assert_eq!(
            netrun.rezzed_programs.len(),
            2,
            "two copies rezzed (caller did not enforce 1/round — that is correct here)"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: unknown program slug returns ProgramNotFound
    // -----------------------------------------------------------------------

    /// Passing an unknown slug returns `RulesError::ProgramNotFound`.
    #[test]
    fn test_unknown_slug_returns_program_not_found() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0006);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("no_such_program".to_string()),
        };

        let result = activate_booster_or_defender(&mut world, &catalog, request);
        assert!(
            matches!(result, Err(RulesError::ProgramNotFound(_))),
            "unknown slug must yield ProgramNotFound, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: no active netrun returns NoActiveNetrun
    // -----------------------------------------------------------------------

    /// Calling activation when `world.netrun` is `None` returns
    /// `RulesError::NoActiveNetrun`.
    #[test]
    fn test_no_active_netrun_returns_error() {
        let mut world = World::new(fresh_pc());
        // netrun is None — no active netrun.
        let catalog = make_catalog(vec![speedy_gonzalvez()]);
        let pc_uuid = world.pc.id.0;

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("speedy_gonzalvez".to_string()),
        };

        let result = activate_booster_or_defender(&mut world, &catalog, request);
        assert!(
            matches!(result, Err(RulesError::NoActiveNetrun)),
            "no active netrun must yield NoActiveNetrun, got {result:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: effects land on the correct duration
    // -----------------------------------------------------------------------

    /// Effects added during activation use `EffectDuration::UntilEndOfNetrun`
    /// so they are automatically cleared when the Netrunner jacks out.
    ///
    /// See p.198: "All your Programs leave the Architecture with you when
    /// you Jack Out."
    #[test]
    fn test_activation_effect_has_until_end_of_netrun_duration() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0007);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![armor_program()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("armor".to_string()),
        };

        let outcome = activate_booster_or_defender(&mut world, &catalog, request)
            .expect("Armor activation must succeed");

        let effect_id = outcome.effects_added[0];
        let pc = world.entity(EntityId(pc_uuid)).unwrap();
        let active = pc
            .effects
            .iter()
            .find(|e| e.id == effect_id)
            .expect("effect must be present on stack");

        assert_eq!(
            active.duration,
            EffectDuration::UntilEndOfNetrun,
            "program effects must last UntilEndOfNetrun (cleared on jack-out per p.198)"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: EffectSource is Program(_)
    // -----------------------------------------------------------------------

    /// Effects added by activation use `EffectSource::Program(program_id)` so
    /// the GM layer and UI can attribute the modifier to the correct program.
    #[test]
    fn test_activation_effect_source_is_program() {
        let pc_uuid = uuid::Uuid::from_u128(0xC0DE_0008);
        let mut world = world_with_netrun(pc_uuid);
        let catalog = make_catalog(vec![speedy_gonzalvez()]);

        let request = ActivateProgram {
            netrunner: EntityId(pc_uuid),
            program: ProgramId("speedy_gonzalvez".to_string()),
        };

        let outcome = activate_booster_or_defender(&mut world, &catalog, request)
            .expect("activation must succeed");

        let effect_id = outcome.effects_added[0];
        let pc = world.entity(EntityId(pc_uuid)).unwrap();
        let active = pc
            .effects
            .iter()
            .find(|e| e.id == effect_id)
            .expect("effect must be present on stack");

        assert_eq!(
            active.source,
            EffectSource::Program(ProgramId("speedy_gonzalvez".to_string())),
            "effect source must be Program(speedy_gonzalvez)"
        );
    }
}
