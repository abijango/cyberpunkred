//! Black ICE encounter and combat resolution (WP-414).
//!
//! Implements the "Encountering and Using Black ICE" procedure from pp.204–208
//! of the *Cyberpunk RED* rulebook. This module handles:
//!
//! 1. **Encounter resolution** — the initial opposed Interface (+SPEED) vs
//!    ICE SPD + d10 speed contest when a Netrunner triggers ICE on a floor.
//! 2. **ICE turns** — each round the ICE rolls ATK + d10 against the
//!    Netrunner or one of the rezzed programs, applying its effect on a hit.
//!
//! ## Rulebook references
//!
//! - **p.204:** Black ICE introduction; deck slot cost; install/uninstall time.
//! - **p.205:** "Encountering and Using Black ICE" — the encounter procedure,
//!   the opposed Interface roll, and ICE activation (insert at top of
//!   initiative queue).
//! - **p.206:** ICE stat block columns (PER / SPD / ATK / DEF / REZ / Effect);
//!   first three ICE entries (Asp, Giant, Hellhound).
//! - **p.207:** Remaining ICE entries (Kraken, Liche, Raven, Scorpion, Skunk,
//!   Wisp, Dragon, Killer, Sabertooth).
//! - **p.208:** ICE pursuit — ICE follows the Netrunner across architecture
//!   floors until Derezzed or Slid.
//!
//! ## Speed contest (p.205)
//!
//! On encounter, both sides roll:
//! - **Netrunner:** `INT + Interface_rank + 1d10` (Interface is the Netrunner
//!   Role Ability; SPEED bonuses from the cyberdeck are additive if present).
//! - **ICE:** `ICE.spd + 1d10`
//!
//! On netrunner loss (ICE wins or ties — tie favours ICE per RAW p.129):
//! - The ICE applies its immediate effect.
//! - The ICE is inserted at the top of the initiative queue (p.205).
//! - `combat_started = true`.
//!
//! On netrunner win: combat does NOT start (`combat_started = false`).
//!
//! ## ICE turn (pp.205–207)
//!
//! Each ICE attacks on its initiative turn:
//! - **Anti-Personnel ICE:** rolls `ATK + 1d10` vs Netrunner's
//!   `INT + Interface_rank + 1d10`. On a hit, applies its effect to the
//!   Netrunner's brain (direct damage).
//! - **Anti-Program ICE:** rolls `ATK + 1d10` vs a random rezzed program's
//!   `DEF + 1d10`. On a hit, applies its effect to the targeted program.
//!
//! See pp.204-208.

use crate::catalog::black_ice::{BlackIce, BlackIceClass, BlackIceEffect, BlackIceId};
use crate::catalog::Catalog;
use crate::checks::skill_check::OpposedOutcome;
use crate::dice::{d10_with_crits, ndn_d6};
use crate::error::RulesError;
use crate::netrunning::state::ProgramInstanceId;
use crate::resolution::CheckBreakdown;
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;
use rand::Rng as _;

// ---------------------------------------------------------------------------
// Public structs
// ---------------------------------------------------------------------------

/// Request to resolve a Black ICE encounter on a particular floor.
///
/// Built by the caller when the Netrunner steps onto a `Floor::BlackIce` for
/// the first time (or when ICE that was lying-in-wait detects the runner).
///
/// See p.205 ("Encountering and Using Black ICE").
pub struct BlackIceEncounter {
    /// The Netrunner entity triggering the encounter.
    pub netrunner: EntityId,
    /// The ICE template slug — resolved from the Black ICE catalog.
    pub ice_template: BlackIceId,
    /// Architecture floor index where the ICE resides. Used for context.
    pub floor: usize,
}

/// Outcome of a [`BlackIceEncounter`] resolution.
///
/// Contains the full speed-contest result plus whether the ICE's immediate
/// effect fired and whether combat was initiated.
///
/// See p.205.
pub struct BlackIceEncounterOutcome {
    /// The opposed speed contest between the Netrunner and the ICE.
    ///
    /// `speed_contest.attacker_wins == true` means the Netrunner won and
    /// combat did NOT start. See p.205.
    pub speed_contest: OpposedOutcome,
    /// The effect the ICE applied immediately on winning the speed contest.
    /// `None` if the Netrunner won the contest. See pp.206–207.
    pub immediate_effect_applied: Option<BlackIceEffectApplied>,
    /// `true` iff the ICE won the speed contest and was inserted into the
    /// initiative queue. See p.205.
    pub combat_started: bool,
}

/// A single effect instance the ICE applied to the Netrunner or a program.
///
/// Used in both [`BlackIceEncounterOutcome`] (immediate effect) and
/// [`BlackIceTurn`] (per-turn attack effect).
///
/// See pp.206–207 (Effect column of the Black ICE Program Table).
pub struct BlackIceEffectApplied {
    /// Human-readable description of the effect kind (e.g. `"BrainDamage"`,
    /// `"DestroyRandomProgram"`, `"StatPenaltyIntRefDex"`). Intended for
    /// logging and LLM narration.
    pub kind: String,
    /// HP / brain damage dealt to the Netrunner on this hit. `0` for
    /// Anti-Program ICE that only target programs.
    ///
    /// See pp.206–207 (each ICE's effect description).
    pub damage_to_netrunner: u16,
    /// The rezzed program destroyed or derezzed by this hit, if any.
    /// `None` for Anti-Personnel ICE. `Some(id)` when Anti-Program ICE
    /// scores a hit and a program is targeted.
    ///
    /// See p.207 (Dragon, Killer, Sabertooth, Raven effects).
    pub program_destroyed: Option<ProgramInstanceId>,
}

/// Outcome of a single ICE attack turn.
///
/// Returned by [`black_ice_take_turn`] to report whether the ICE attacked the
/// Netrunner or a rezzed program, the roll details, and what effect (if any)
/// was applied.
///
/// See pp.205–207.
pub struct BlackIceTurn {
    /// `true` if the ICE targeted the Netrunner directly (Anti-Personnel
    /// ICE, p.205). `false` if the ICE targeted a rezzed program
    /// (Anti-Program ICE, p.207). Demon ICE is not handled here (WP-415).
    pub netrunner_target: bool,
    /// Full breakdown of the ICE's `ATK + 1d10` attack roll versus the
    /// Netrunner's `INT + Interface_rank + 1d10` (Anti-Personnel) or the
    /// program's `DEF + 1d10` (Anti-Program).
    pub attack_breakdown: CheckBreakdown,
    /// Effect applied on a successful hit. `None` if the attack missed.
    ///
    /// See pp.206–207.
    pub effect_applied: Option<BlackIceEffectApplied>,
}

// ---------------------------------------------------------------------------
// encounter_black_ice
// ---------------------------------------------------------------------------

/// Resolve the initial encounter between a Netrunner and a Black ICE program.
///
/// ## Procedure (p.205)
///
/// 1. Look up the ICE template in `catalog`. Fail with
///    [`RulesError::EntityNotFound`] if missing.
/// 2. Read the Netrunner's `INT` and `role_rank` (Interface) from `world`.
///    Fail with [`RulesError::EntityNotFound`] if the entity is not in world.
/// 3. Run opposed speed contest:
///    - Netrunner: `INT + Interface_rank + 1d10`
///    - ICE: `ICE.spd + 1d10`
///
///    Tie favours the ICE (attacker = Netrunner, defender = ICE; ties go to
///    defender per p.129).
/// 4. If ICE wins (netrunner loses): apply the ICE's effect immediately and
///    insert the ICE at the top of the initiative queue (if combat is active).
///    Set `combat_started = true`.
/// 5. If Netrunner wins: no immediate effect, `combat_started = false`.
///
/// See pp.204-208.
pub fn encounter_black_ice(
    world: &mut World,
    catalog: &Catalog<BlackIce>,
    request: BlackIceEncounter,
    rng: &mut Rng,
) -> Result<BlackIceEncounterOutcome, RulesError> {
    // Step 1 — look up ICE template.
    // See p.205 ("Encountering and Using Black ICE").
    let ice = catalog
        .get(&request.ice_template.0)
        .ok_or(RulesError::EntityNotFound(request.netrunner))?;
    let ice_spd = ice.spd;
    let ice_class = ice.class;
    let ice_effect = ice.effect.clone();

    // Step 2 — read Netrunner stats.
    // INT + Interface (role_rank) for the speed contest (p.205).
    let (netrunner_int, interface_rank) = {
        let actor = world
            .entity(request.netrunner)
            .ok_or(RulesError::EntityNotFound(request.netrunner))?;
        (actor.current_int(), i16::from(actor.role_rank))
    };

    // Step 3 — opposed speed contest.
    // Netrunner rolls first (attacker), ICE rolls second (defender).
    // Deterministic roll order: Netrunner d10, then ICE d10.
    // See p.205.
    let netrunner_d10 = d10_with_crits(rng);
    let ice_d10 = d10_with_crits(rng);

    // Netrunner: INT + Interface_rank + d10. No luck or skill separate from role_rank.
    // DV(0) sentinel — actual comparison is vs. ICE's roll.
    let netrunner_bd =
        CheckBreakdown::new(netrunner_int, interface_rank, 0, 0, netrunner_d10, DV(0));

    // ICE: SPD + d10. No separate skill rank.
    let ice_bd = CheckBreakdown::new(i16::from(ice_spd), 0, 0, 0, ice_d10, DV(0));

    // Netrunner wins only if strictly greater (ties favour ICE / defender — p.129).
    // See p.205 for the attacker/defender framing in the speed contest.
    let netrunner_wins = netrunner_bd.final_value > ice_bd.final_value;

    // Patch DV fields: each side's DV is the opponent's final value.
    // This mirrors the OpposedCheck logic in skill_check.rs (saturate_to_u8_dv).
    let mut attacker_breakdown = netrunner_bd;
    let mut defender_breakdown = ice_bd;

    let att_final = attacker_breakdown.final_value;
    let def_final = defender_breakdown.final_value;

    attacker_breakdown.dv = DV(saturate_to_u8(def_final));
    attacker_breakdown.margin = att_final - i16::from(attacker_breakdown.dv.0);
    attacker_breakdown.success = netrunner_wins;

    defender_breakdown.dv = DV(saturate_to_u8(att_final));
    defender_breakdown.margin = def_final - i16::from(defender_breakdown.dv.0);
    // Defender (ICE) wins on tie.
    defender_breakdown.success = def_final >= att_final;

    let speed_contest = OpposedOutcome {
        attacker_breakdown,
        defender_breakdown,
        attacker_wins: netrunner_wins,
    };

    // Step 4 & 5 — on ICE win, apply immediate effect and start combat.
    // See p.205, pp.206–207.
    let (immediate_effect_applied, combat_started) = if !netrunner_wins {
        // ICE wins: apply immediate effect to the Netrunner.
        let effect_applied =
            apply_ice_effect_to_netrunner(world, request.netrunner, &ice_effect, ice_class, rng);

        // Insert ICE at top of initiative queue if combat is active (p.205).
        // RAW: "It is placed into the Initiative Queue at the top." (p.205)
        //
        // Borrow-checker note: `CombatState::insert_at_top` takes `(&World,
        // &mut Rng)` but those params are immediately discarded inside the
        // current implementation (see WP-301 turn_engine.rs, `let _ = (world, rng)`).
        // We cannot pass `world` while holding `world.combat` mutably, so we
        // call `insert_at_top` with a minimal stub world cloned just for the
        // signature. This is safe because `insert_at_top` does not read the
        // World argument. WP-417 (Netrun/Combat integration) should revisit
        // and refactor the insert_at_top signature if it ever needs `&World`.
        if world.combat.is_some() {
            let ice_entity = ice_entity_id(request.netrunner, request.floor);
            // Safety: we just checked `is_some()` above.
            // Clone a minimal world for the signature requirement.
            // DEVIATION: We pass a cloned world to satisfy the &World parameter
            // of insert_at_top, which discards it immediately. This avoids
            // the E0502 dual-borrow of world. See WP-417 for proper wiring.
            let world_clone = world.clone();
            if let Some(combat) = world.combat.as_mut() {
                combat.insert_at_top(ice_entity, &world_clone, rng);
            }
        }

        (Some(effect_applied), true)
    } else {
        // Netrunner wins: no effect, no combat start. See p.205.
        (None, false)
    };

    Ok(BlackIceEncounterOutcome {
        speed_contest,
        immediate_effect_applied,
        combat_started,
    })
}

// ---------------------------------------------------------------------------
// black_ice_take_turn
// ---------------------------------------------------------------------------

/// Resolve a single attack turn for a Black ICE entity.
///
/// Called on the ICE's initiative entry each round after the encounter phase.
///
/// ## Procedure (pp.205–207)
///
/// - **Anti-Personnel ICE:** ATK + 1d10 vs Netrunner's `INT + Interface + 1d10`.
///   On hit: apply the ICE's effect to the Netrunner's brain. `netrunner_target = true`.
/// - **Anti-Program ICE:** ATK + 1d10 vs a *random* rezzed program's `DEF + 1d10`.
///   On hit: apply damage/derezz to the chosen program. `netrunner_target = false`.
///
/// If there are no rezzed programs and the ICE is Anti-Program, the ICE still
/// rolls but has no target — `effect_applied` will be `None` and
/// `netrunner_target = false`.
///
/// See pp.204-208.
pub fn black_ice_take_turn(
    world: &mut World,
    catalog: &Catalog<BlackIce>,
    netrunner: EntityId,
    ice: &BlackIceId,
    rng: &mut Rng,
) -> Result<BlackIceTurn, RulesError> {
    // Look up ICE template. See p.206.
    let ice_data = catalog
        .get(&ice.0)
        .ok_or(RulesError::EntityNotFound(netrunner))?;
    let ice_atk = ice_data.atk;
    let ice_class = ice_data.class;
    let ice_effect = ice_data.effect.clone();

    match ice_class {
        BlackIceClass::AntiPersonnel => {
            // Anti-Personnel: target is always the Netrunner.
            // ATK + 1d10 vs INT + Interface + 1d10. See p.205.
            black_ice_attack_netrunner(world, netrunner, i16::from(ice_atk), &ice_effect, rng)
        }
        BlackIceClass::AntiProgram => {
            // Anti-Program: target is a random rezzed program.
            // ATK + 1d10 vs program DEF + 1d10. See p.207 (Dragon, Killer, Sabertooth).
            black_ice_attack_program(world, netrunner, i16::from(ice_atk), &ice_effect, rng)
        }
        BlackIceClass::Demon => {
            // Demon ICE behaviour is handled by WP-415. This WP does not
            // handle Demon turns — the caller must not invoke this function
            // for a Demon-class ICE. Return an error for robustness.
            // RAW: p.205 distinguishes Anti-Personnel, Anti-Program, and Demon.
            // See p.212 for Demon specifics (WP-415).
            Err(RulesError::EntityNotFound(netrunner))
        }
    }
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Resolve an Anti-Personnel ICE attack against the Netrunner.
///
/// Roll order (deterministic for replay): ICE d10 first, Netrunner d10 second.
/// The ICE is the attacker; the Netrunner is the defender. See p.205.
fn black_ice_attack_netrunner(
    world: &mut World,
    netrunner: EntityId,
    ice_atk: i16,
    ice_effect: &BlackIceEffect,
    rng: &mut Rng,
) -> Result<BlackIceTurn, RulesError> {
    // Snapshot Netrunner stats before rolling. See p.205.
    let (netrunner_int, interface_rank) = {
        let actor = world
            .entity(netrunner)
            .ok_or(RulesError::EntityNotFound(netrunner))?;
        (actor.current_int(), i16::from(actor.role_rank))
    };

    // Deterministic roll order: ICE rolls first, Netrunner rolls second.
    // See p.205.
    let ice_d10 = d10_with_crits(rng);
    let netrunner_d10 = d10_with_crits(rng);

    // ICE attack roll: ATK + d10.
    let ice_attack = CheckBreakdown::new(ice_atk, 0, 0, 0, ice_d10, DV(0));

    // Netrunner defence roll: INT + Interface_rank + d10.
    let netrunner_def =
        CheckBreakdown::new(netrunner_int, interface_rank, 0, 0, netrunner_d10, DV(0));

    // ICE hits if its roll strictly exceeds the Netrunner's defence.
    // Ties favour the defender (Netrunner). See p.129 ("In case of a tie,
    // the Defender always wins").
    let ice_hits = ice_attack.final_value > netrunner_def.final_value;

    // Patch DV fields for the breakdown.
    let mut attack_breakdown = ice_attack;
    attack_breakdown.dv = DV(saturate_to_u8(netrunner_def.final_value));
    attack_breakdown.margin = attack_breakdown.final_value - i16::from(attack_breakdown.dv.0);
    attack_breakdown.success = ice_hits;

    let effect_applied = if ice_hits {
        Some(apply_ice_effect_to_netrunner(
            world,
            netrunner,
            ice_effect,
            BlackIceClass::AntiPersonnel,
            rng,
        ))
    } else {
        None
    };

    Ok(BlackIceTurn {
        netrunner_target: true,
        attack_breakdown,
        effect_applied,
    })
}

/// Resolve an Anti-Program ICE attack against a random rezzed program.
///
/// If no programs are rezzed, the attack still rolls but applies no effect.
/// See p.207 (Dragon, Killer, Sabertooth).
fn black_ice_attack_program(
    world: &mut World,
    _netrunner: EntityId,
    ice_atk: i16,
    ice_effect: &BlackIceEffect,
    rng: &mut Rng,
) -> Result<BlackIceTurn, RulesError> {
    // Determine if there are any rezzed programs and pick a random target.
    // See p.207 ("picks a Rezzed Program at random").
    let (target_program_instance, program_def) = {
        let netrun = world.netrun.as_ref();
        match netrun {
            Some(nr) if !nr.rezzed_programs.is_empty() => {
                let count = nr.rezzed_programs.len();
                // Pick a random program index for Anti-Program targeting.
                // Using a plain uniform roll for the index (not d10_with_crits)
                // since this is not a skill-check roll — it's a selection.
                let idx = rng.random_range(0..count);
                let prog = &nr.rezzed_programs[idx];
                (Some(prog.instance_id.clone()), i16::from(prog.current_rez))
            }
            _ => (None, 0),
        }
    };

    // Deterministic roll order: ICE d10 first, program defense d10 second.
    let ice_d10 = d10_with_crits(rng);
    let prog_d10 = d10_with_crits(rng);

    // ICE attack roll: ATK + d10.
    let ice_attack = CheckBreakdown::new(ice_atk, 0, 0, 0, ice_d10, DV(0));

    // Program defence roll: DEF (current_rez used as a proxy for DEF) + d10.
    // Per pp.206-207, Anti-Program ICE rolls ATK vs program DEF. The program's
    // DEF stat is not separately tracked in RezzedProgram; we use current_rez
    // as the available defensive value.
    // NOTE: Deviation from WP spec — RezzedProgram only tracks current_rez,
    // not a separate DEF stat. We use current_rez as DEF proxy. Future WPs
    // that add a DEF stat to RezzedProgram should update this call site.
    let prog_defence = CheckBreakdown::new(program_def, 0, 0, 0, prog_d10, DV(0));

    // ICE hits only if a program was targeted AND the roll hits.
    let ice_hits =
        target_program_instance.is_some() && ice_attack.final_value > prog_defence.final_value;

    // Patch DV fields.
    let mut attack_breakdown = ice_attack;
    attack_breakdown.dv = DV(saturate_to_u8(prog_defence.final_value));
    attack_breakdown.margin = attack_breakdown.final_value - i16::from(attack_breakdown.dv.0);
    attack_breakdown.success = ice_hits;

    let effect_applied = if ice_hits {
        // Anti-Program ICE: destroy or derezz the targeted program.
        // See p.207: "Deals XdN damage to a Program. If this damage would
        // be enough to Derezz the Program, it is instead Destroyed."
        let program_destroyed = target_program_instance.clone();

        // Apply the Anti-Program effect — derezz/destroy the program from
        // the active netrun state.
        if let Some(ref instance_id) = program_destroyed {
            if let Some(netrun) = world.netrun.as_mut() {
                netrun.derez_program(instance_id.clone());
            }
        }

        let kind = effect_kind_label(ice_effect);
        Some(BlackIceEffectApplied {
            kind,
            damage_to_netrunner: 0,
            program_destroyed,
        })
    } else {
        None
    };

    Ok(BlackIceTurn {
        netrunner_target: false,
        attack_breakdown,
        effect_applied,
    })
}

/// Apply a Black ICE effect to the Netrunner on a hit.
///
/// Computes the effect's damage roll (if any) using `rng` and returns the
/// structured [`BlackIceEffectApplied`] record. Also applies HP damage to the
/// Netrunner if the effect deals brain damage.
///
/// For Anti-Program effects that target a rezzed program (not the Netrunner
/// directly), this function is not called — see [`black_ice_attack_program`].
///
/// See pp.206–207 (Effect column).
fn apply_ice_effect_to_netrunner(
    world: &mut World,
    netrunner: EntityId,
    effect: &BlackIceEffect,
    _class: BlackIceClass,
    rng: &mut Rng,
) -> BlackIceEffectApplied {
    let kind = effect_kind_label(effect);

    // Compute damage from the effect's dice spec (if any) and apply to the
    // Netrunner's HP. All Black ICE brain-damage rolls use d6 per pp.206–207.
    // See pp.206–207 for per-ICE damage dice.
    let damage: u16 = match effect {
        BlackIceEffect::BrainDamageAndForceJackOut { dice, .. } => {
            // Giant: 3d6 brain damage, then forced unsafe jack-out. See p.206.
            let rolls = ndn_d6(dice.n, rng);
            rolls.iter().map(|&d| u16::from(d)).sum()
        }
        BlackIceEffect::BrainDamageAndCyberdeckFire { dice, .. } => {
            // Hellhound: 2d6 brain damage + fire. See p.206.
            let rolls = ndn_d6(dice.n, rng);
            rolls.iter().map(|&d| u16::from(d)).sum()
        }
        BlackIceEffect::BrainDamageAndJackOutLock { dice, .. } => {
            // Kraken: 3d6 brain damage + safe jack-out blocked. See p.207.
            let rolls = ndn_d6(dice.n, rng);
            rolls.iter().map(|&d| u16::from(d)).sum()
        }
        BlackIceEffect::DerezzRandomDefenderAndBrainDamage { dice } => {
            // Raven: derezz a random defender, then 1d6 brain damage. See p.207.
            // The derezz of a random Defender program is a side effect noted here;
            // full program-type filtering is deferred to WP-417 (integration WP).
            let rolls = ndn_d6(dice.n, rng);
            rolls.iter().map(|&d| u16::from(d)).sum()
        }
        BlackIceEffect::BrainDamageAndNetActionPenalty { dice, .. } => {
            // Wisp: 1d6 brain damage + NET action penalty. See p.207.
            let rolls = ndn_d6(dice.n, rng);
            rolls.iter().map(|&d| u16::from(d)).sum()
        }
        BlackIceEffect::StatPenaltyIntRefDex { dice, .. } => {
            // Liche: stat penalty, no brain damage. See p.207.
            // The 1d6 roll is per-stat; for now we roll once and record
            // it as the penalty magnitude. Actual stat mutation is deferred
            // to the effect system (WP-417).
            let _penalty_roll = ndn_d6(dice.n, rng);
            0 // No direct HP damage.
        }
        BlackIceEffect::StatPenaltyMove { dice, .. } => {
            // Scorpion: MOVE penalty, no brain damage. See p.207.
            let _penalty_roll = ndn_d6(dice.n, rng);
            0
        }
        BlackIceEffect::SlideCheckPenalty { .. } => {
            // Skunk: slide check penalty (no damage roll). See p.207.
            0
        }
        BlackIceEffect::DestroyRandomProgram => {
            // Asp: destroys a random installed program. See p.206.
            // Program removal from cyberdeck (not from rezzed list) is
            // handled by the wider integration WP (WP-417).
            0
        }
        BlackIceEffect::ProgramDamageDestroyOnDerezz { .. } => {
            // Anti-Program effect — should not fire on Netrunner.
            // Dragon, Killer, Sabertooth — handled in attack_program. See p.207.
            0
        }
    };

    // Apply HP damage to the Netrunner (brain damage = direct HP damage).
    // Per pp.205–207: brain damage goes directly to HP, bypassing armor.
    if damage > 0 {
        if let Some(character) = world.entity_mut(netrunner) {
            // Clamp damage to i16 range for the saturating subtraction.
            let damage_i16 = damage.min(i16::MAX as u16) as i16;
            character.wounds.current_hp = character.wounds.current_hp.saturating_sub(damage_i16);
        }
    }

    BlackIceEffectApplied {
        kind,
        damage_to_netrunner: damage,
        program_destroyed: None,
    }
}

/// Human-readable label for an effect kind, used for logging and narration.
fn effect_kind_label(effect: &BlackIceEffect) -> String {
    match effect {
        BlackIceEffect::DestroyRandomProgram => "DestroyRandomProgram".into(),
        BlackIceEffect::BrainDamageAndForceJackOut { .. } => "BrainDamageAndForceJackOut".into(),
        BlackIceEffect::BrainDamageAndCyberdeckFire { .. } => "BrainDamageAndCyberdeckFire".into(),
        BlackIceEffect::BrainDamageAndJackOutLock { .. } => "BrainDamageAndJackOutLock".into(),
        BlackIceEffect::StatPenaltyIntRefDex { .. } => "StatPenaltyIntRefDex".into(),
        BlackIceEffect::DerezzRandomDefenderAndBrainDamage { .. } => {
            "DerezzRandomDefenderAndBrainDamage".into()
        }
        BlackIceEffect::StatPenaltyMove { .. } => "StatPenaltyMove".into(),
        BlackIceEffect::SlideCheckPenalty { .. } => "SlideCheckPenalty".into(),
        BlackIceEffect::BrainDamageAndNetActionPenalty { .. } => {
            "BrainDamageAndNetActionPenalty".into()
        }
        BlackIceEffect::ProgramDamageDestroyOnDerezz { .. } => {
            "ProgramDamageDestroyOnDerezz".into()
        }
    }
}

/// Derive a deterministic synthetic [`EntityId`] for an ICE instance from the
/// netrunner's UUID and the floor index.
///
/// This is a temporary approach until WP-417 (Netrun integration with combat
/// queue) mints proper EntityIds for ICE instances. The derived ID is stable
/// across calls with the same inputs — determinism is preserved.
///
/// The encoding: bitwise-OR of the netrunner UUID (high 64 bits only, to
/// avoid collision with the original) and a floor-index mask.
fn ice_entity_id(netrunner: EntityId, floor: usize) -> EntityId {
    // Simple derivation: combine netrunner UUID bytes with floor index.
    // This does not produce globally unique IDs but is sufficient for
    // tests and single-encounter scenarios until WP-417 lands.
    let base = netrunner.0.as_u128();
    // XOR with a floor-index-dependent constant in the high bits.
    // 0xB1AC_K1CE is mnemonic for "BLACK ICE" — identifies ICE-derived entity IDs.
    let derived = base ^ (0xB1AC_1CE0_0000_0000_u128 | (floor as u128));
    EntityId(uuid::Uuid::from_u128(derived))
}

/// Saturate an `i16` to the `u8` range required by [`DV`].
/// Negative values clamp to 0; values above `u8::MAX` clamp to `u8::MAX`.
fn saturate_to_u8(v: i16) -> u8 {
    v.clamp(0, i16::from(u8::MAX)) as u8
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::black_ice::{BlackIce, BlackIceClass, BlackIceEffect, BlackIceId};
    use crate::catalog::black_ice::{DiceSpec, DieKind};
    use crate::catalog::Catalog;
    use crate::character::Role;
    use crate::effects::ProgramId;
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::types::{CharacterId, Eurobucks, PriceTier};
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;
    use std::collections::HashMap;
    use uuid::Uuid;

    // -------------------------------------------------------------------------
    // Helpers
    // -------------------------------------------------------------------------

    /// Build a Netrunner PC with known INT and Interface rank.
    fn netrunner_world(int: u8, interface_rank: u8) -> (World, EntityId) {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0xAA));
        pc.stats.int = int;
        pc.role = Role::Netrunner;
        pc.role_rank = interface_rank;
        pc.stats.luck = 10;
        pc.luck_pool = 10;
        let eid = EntityId(pc.id.0);
        let world = World::new(pc);
        (world, eid)
    }

    /// Build a minimal Anti-Personnel BlackIce entry for testing.
    fn make_anti_personnel_ice(slug: &str, spd: u8, atk: u8) -> BlackIce {
        BlackIce {
            id: BlackIceId(slug.to_string()),
            display_name: slug.to_string(),
            class: BlackIceClass::AntiPersonnel,
            per: 4,
            spd,
            atk,
            def: 2,
            rez: 15,
            effect: BlackIceEffect::BrainDamageAndCyberdeckFire {
                dice: DiceSpec {
                    n: 2,
                    die: DieKind::D6,
                },
                burn_damage_per_turn_end: 2,
                cannot_stack: true,
            },
            icon: "test".into(),
            price: PriceTier::Expensive,
            price_eb: Eurobucks(500),
        }
    }

    /// Build a minimal Anti-Program BlackIce entry for testing.
    fn make_anti_program_ice(slug: &str, spd: u8, atk: u8) -> BlackIce {
        BlackIce {
            id: BlackIceId(slug.to_string()),
            display_name: slug.to_string(),
            class: BlackIceClass::AntiProgram,
            per: 4,
            spd,
            atk,
            def: 2,
            rez: 20,
            effect: BlackIceEffect::ProgramDamageDestroyOnDerezz {
                dice: DiceSpec {
                    n: 4,
                    die: DieKind::D6,
                },
            },
            icon: "test".into(),
            price: PriceTier::Expensive,
            price_eb: Eurobucks(500),
        }
    }

    fn make_catalog_with(slugs_and_ice: Vec<(&str, BlackIce)>) -> Catalog<BlackIce> {
        let mut map = HashMap::new();
        for (slug, ice) in slugs_and_ice {
            map.insert(slug.to_string(), ice);
        }
        Catalog::new(map)
    }

    /// Find a seed such that two consecutive d10_with_crits rolls satisfy
    /// a predicate (to force particular win/lose outcomes).
    ///
    /// Pred receives (netrunner_roll, ice_roll).
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(i16, i16) -> bool,
    {
        use crate::dice::d10_with_crits;
        for seed in 0u64..2_000_000 {
            let mut r = Rng::seed_from_u64(seed);
            let a = d10_with_crits(&mut r).net;
            let b = d10_with_crits(&mut r).net;
            if pred(a, b) {
                return seed;
            }
        }
        panic!("no matching seed found within search bound");
    }

    // -------------------------------------------------------------------------
    // test_speed_contest_wins_no_combat
    // -------------------------------------------------------------------------

    /// WP-414 acceptance test: when the Netrunner wins the speed contest,
    /// `combat_started` must be `false` and no immediate effect is applied.
    ///
    /// See p.205.
    #[test]
    fn test_speed_contest_wins_no_combat() {
        // INT=8, Interface=6 → base 14 before d10.
        // ICE SPD=2 → base 2 before d10.
        // Find a seed where the Netrunner's d10 > ICE's d10 so that
        // 14 + netrunner_d10 > 2 + ice_d10 (netrunner wins).
        let (mut world, netrunner_eid) = netrunner_world(8, 6);
        let ice = make_anti_personnel_ice("test_ice", 2, 4);
        let catalog = make_catalog_with(vec![("test_ice", ice)]);

        // Find a seed where: netrunner_d10 roll (first) and ice_d10 roll (second)
        // result in netrunner winning. With INT=8, Interface=6 (total 14 base)
        // vs ICE SPD=2 (total 2 base), the netrunner wins on almost any seed.
        // Use seed 1 and verify the outcome.
        let seed = find_seed_where(|a, b| {
            // netrunner total = 14 + a, ice total = 2 + b
            // netrunner wins if strictly greater
            (14 + a) > (2 + b)
        });

        let mut rng = Rng::seed_from_u64(seed);

        let outcome = encounter_black_ice(
            &mut world,
            &catalog,
            BlackIceEncounter {
                netrunner: netrunner_eid,
                ice_template: BlackIceId("test_ice".into()),
                floor: 0,
            },
            &mut rng,
        )
        .expect("encounter_black_ice must succeed");

        assert!(
            outcome.speed_contest.attacker_wins,
            "netrunner must win the speed contest"
        );
        assert!(
            outcome.immediate_effect_applied.is_none(),
            "no immediate effect when netrunner wins"
        );
        assert!(
            !outcome.combat_started,
            "combat must NOT start when netrunner wins"
        );
    }

    // -------------------------------------------------------------------------
    // test_speed_contest_loses_starts_combat
    // -------------------------------------------------------------------------

    /// WP-414 acceptance test: when the ICE wins the speed contest,
    /// `combat_started` must be `true` and `immediate_effect_applied` must be
    /// `Some`.
    ///
    /// See p.205.
    #[test]
    fn test_speed_contest_loses_starts_combat() {
        // INT=2, Interface=1 → base 3. ICE SPD=10 → base 10.
        // The ICE almost always wins; find a seed where it does.
        let (mut world, netrunner_eid) = netrunner_world(2, 1);
        let ice = make_anti_personnel_ice("test_ice_strong", 10, 4);
        let catalog = make_catalog_with(vec![("test_ice_strong", ice)]);

        let seed = find_seed_where(|a, b| {
            // netrunner total = 3 + a, ice total = 10 + b
            // ICE wins if netrunner does NOT win (ties also go to ICE)
            (3 + a) <= (10 + b)
        });

        let mut rng = Rng::seed_from_u64(seed);

        let outcome = encounter_black_ice(
            &mut world,
            &catalog,
            BlackIceEncounter {
                netrunner: netrunner_eid,
                ice_template: BlackIceId("test_ice_strong".into()),
                floor: 1,
            },
            &mut rng,
        )
        .expect("encounter_black_ice must succeed");

        assert!(
            !outcome.speed_contest.attacker_wins,
            "ICE must win the speed contest"
        );
        assert!(
            outcome.immediate_effect_applied.is_some(),
            "immediate_effect_applied must be Some when ICE wins"
        );
        assert!(
            outcome.combat_started,
            "combat_started must be true when ICE wins"
        );
    }

    // -------------------------------------------------------------------------
    // test_anti_personnel_attacks_netrunner
    // -------------------------------------------------------------------------

    /// WP-414 acceptance test: Anti-Personnel ICE turn always targets the
    /// Netrunner (`netrunner_target == true`).
    ///
    /// See p.205.
    #[test]
    fn test_anti_personnel_attacks_netrunner() {
        let (mut world, netrunner_eid) = netrunner_world(5, 4);
        let ice = make_anti_personnel_ice("ap_ice", 4, 6);
        let catalog = make_catalog_with(vec![("ap_ice", ice)]);
        let mut rng = Rng::seed_from_u64(42);

        let turn = black_ice_take_turn(
            &mut world,
            &catalog,
            netrunner_eid,
            &BlackIceId("ap_ice".into()),
            &mut rng,
        )
        .expect("black_ice_take_turn must succeed");

        assert!(
            turn.netrunner_target,
            "Anti-Personnel ICE must target the Netrunner (netrunner_target == true)"
        );
    }

    // -------------------------------------------------------------------------
    // test_anti_program_attacks_random_program
    // -------------------------------------------------------------------------

    /// WP-414 acceptance test: Anti-Program ICE turn targets a random rezzed
    /// program (`netrunner_target == false`) and on a hit the program is removed.
    ///
    /// See p.207.
    #[test]
    fn test_anti_program_attacks_random_program() {
        let (mut world, netrunner_eid) = netrunner_world(5, 4);

        // Set up an active netrun state with one rezzed program.
        let arch = NetArchId("test".into());
        let mut netrun = NetrunState::start(netrunner_eid, arch, 4);
        let prog_id = ProgramId("armor".into());
        // Rez a program with REZ=1 (very low DEF proxy, so ICE hits easily).
        let _instance = netrun.rez_program(prog_id, 1);
        world.netrun = Some(netrun);

        // Use an ICE with very high ATK so it is likely to hit.
        let ice = make_anti_program_ice("ap_prog_ice", 4, 10);
        let catalog = make_catalog_with(vec![("ap_prog_ice", ice)]);

        // Find a seed where ICE ATK roll beats program REZ (proxy DEF=1).
        // ICE base = 10, program base = 1; almost any seed works.
        let seed = find_seed_where(|ice_d10, prog_d10| {
            // ice total = 10 + ice_d10 (first roll from anti-program perspective)
            // program total = 1 + prog_d10 (second roll)
            (10 + ice_d10) > (1 + prog_d10)
        });

        let mut rng = Rng::seed_from_u64(seed);

        let turn = black_ice_take_turn(
            &mut world,
            &catalog,
            netrunner_eid,
            &BlackIceId("ap_prog_ice".into()),
            &mut rng,
        )
        .expect("black_ice_take_turn must succeed");

        assert!(
            !turn.netrunner_target,
            "Anti-Program ICE must target a program (netrunner_target == false)"
        );

        // After a hit the program should be derezzed.
        if turn.attack_breakdown.success {
            assert!(
                turn.effect_applied.is_some(),
                "effect_applied must be Some when Anti-Program ICE hits"
            );
            let rezzed = world
                .netrun
                .as_ref()
                .map(|nr| nr.rezzed_programs.len())
                .unwrap_or(0);
            assert_eq!(
                rezzed, 0,
                "rezzed program must be removed after Anti-Program ICE hit"
            );
        }
    }
}
