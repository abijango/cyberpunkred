//! Program activation for Attacker-class programs.
//!
//! Implements the "Use Program" NET Action for Attacker programs (Banhammer,
//! Sword, Hellbolt, Vrizzbolt, DeckKRASH, Nervescrub, Poison Flatline,
//! Superglue). See p.201, p.203–204.
//!
//! ## Rules summary (p.201, p.203–204)
//!
//! > "If you have a Program that attacks, you may use a NET Action to make
//! > an attack with it. Roll 1d10 and add your Interface Ability Rank and the
//! > Program's ATK stat. The target rolls 1d10 and adds their DEF stat (if
//! > attacking a Program) or their Interface Rank (if attacking a Netrunner)
//! > to try to beat your attack. If your attack succeeds, apply the Program's
//! > Effect. The Program automatically Derezzes after its Attack."
//!
//! ## Attack formula (p.201)
//!
//! - **Attacker:** `INT + Interface_rank + Program.ATK + 1d10`
//! - **Defender (Program target):** `program.DEF + 1d10`
//! - **Defender (Netrunner target):** `INT + Interface_rank + 1d10`
//! - Ties favour the defender (p.129).
//!
//! ## Self-derezz (p.201)
//!
//! > "The Program automatically Derezzes after its Attack."
//!
//! After the attack is resolved (regardless of hit or miss), the attacker
//! program is removed from [`NetrunState::rezzed_programs`].
//!
//! ## Acceptance tests
//!
//! - `test_banhammer_attacks_program` — Banhammer (Anti-Program) hits and
//!   deals 3d6 REZ damage to a rezzed program.
//! - `test_sword_attacks_target` — Sword hits and deals damage.
//! - `test_attacker_self_derezzes_after_use` — Attacker is removed from
//!   `rezzed_programs` after the attack, regardless of hit/miss.
//! - `test_rejects_booster_class` — passing a Booster program slug returns
//!   `Err(RulesError::WrongProgramClass)`.
//!
//! See p.201, p.203–204.

use crate::catalog::programs::{DiceSpec, DieKind, Program, ProgramClass, ProgramEffect};
use crate::catalog::Catalog;
use crate::dice::{d10_with_crits, ndn_d6};
use crate::effects::ProgramId;
use crate::error::RulesError;
use crate::netrunning::state::ProgramInstanceId;
use crate::resolution::CheckBreakdown;
use crate::rng::Rng;
use crate::types::EntityId;
use crate::world::World;

// ---------------------------------------------------------------------------
// Public request / outcome types
// ---------------------------------------------------------------------------

/// Who or what the Attacker program is targeting.
///
/// See p.201: "The target rolls 1d10 and adds their DEF stat (if
/// attacking a Program) or their Interface Rank (if attacking a Netrunner)."
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttackTarget {
    /// Targeting a rezzed program by its instance ID. The program's DEF
    /// stat is the defender's bonus. See p.201.
    Program(ProgramInstanceId),
    /// Targeting a Netrunner entity directly. The target Netrunner's
    /// `INT + Interface_rank` forms the defense roll. See p.201, p.204.
    Netrunner(EntityId),
    /// Targeting a Black ICE by floor index. The ICE's DEF is used.
    /// See p.201, p.205.
    BlackIce(usize),
}

/// Request to activate one Attacker program against a target. See p.201.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttackerActivation {
    /// The Netrunner entity performing the attack.
    pub netrunner: EntityId,
    /// The program being activated (must be Attacker class). Identified by
    /// catalog slug.
    pub program: ProgramId,
    /// The instance of the rezzed attacker program (used to remove it from
    /// `rezzed_programs` after use). See p.201.
    pub program_instance: ProgramInstanceId,
    /// What the program is aimed at.
    pub target: AttackTarget,
}

/// The effect applied after a successful Attacker program hit.
///
/// Each variant corresponds to one shape of `ProgramEffect`. See pp.203–204.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AttackerEffectApplied {
    /// The attacker damaged a rezzed program (e.g. Banhammer, Sword).
    ///
    /// `rez_lost` is the total damage dealt. `derezzed` is `true` when
    /// the target program's REZ drops to 0 or below.
    ///
    /// See p.201 ("Defeating a Program") and pp.203–204 (Banhammer, Sword).
    ProgramDamage {
        /// Total REZ damage dealt to the target program.
        rez_lost: u8,
        /// `true` iff the target program is Derezzed (REZ reached 0).
        derezzed: bool,
    },
    /// The attacker dealt direct brain damage to a Netrunner.
    ///
    /// `hp_lost` is total HP damage before any mitigation that the caller
    /// must apply. See p.204 (Hellbolt, Vrizzbolt, etc.).
    NetrunnerBrainDamage {
        /// HP of brain damage before mitigation.
        hp_lost: u8,
    },
    /// The attacker damaged a Black ICE program by floor index.
    BlackIceDamage {
        /// Total REZ damage dealt to the Black ICE.
        rez_lost: u8,
        /// `true` iff the Black ICE is Derezzed (REZ reached 0).
        derezzed: bool,
    },
    /// The attack roll missed. No damage applied.
    Missed,
}

/// Full outcome of an [`AttackerActivation`].
///
/// See p.201 (Use Program NET Action), pp.203–204 (effect tables).
#[derive(Clone, Debug, PartialEq)]
pub struct AttackerOutcome {
    /// Full breakdown of the attacker's Interface + ATK + d10 roll vs the
    /// defender's DEF + d10 (or Interface + d10). See p.201.
    pub breakdown: CheckBreakdown,
    /// The individual die values for each damage die rolled. See pp.203–204.
    pub damage_rolls: Vec<u8>,
    /// Sum of `damage_rolls`. `0` on a miss.
    pub damage_total: u16,
    /// The effect applied (or `Missed` if the attack failed). See p.201.
    pub effect_applied: AttackerEffectApplied,
    /// `true` if the attacker program was removed from `rezzed_programs`.
    ///
    /// Per p.201: "The Program automatically Derezzes after its Attack."
    /// This field is always `true` on a successfully resolved activation.
    pub self_derezzed: bool,
}

// ---------------------------------------------------------------------------
// activate_attacker
// ---------------------------------------------------------------------------

/// Resolve an Attacker program activation against the given target.
///
/// ## Sequence (p.201)
///
/// 1. Validate that `request.program` is an Attacker-class program.
/// 2. Look up the Netrunner entity in `world`.
/// 3. Roll the attack: `INT + Interface_rank + program.ATK + d10_with_crits`.
/// 4. Roll the defense:
///    - Targeting a **Program**: `DEF + d10` (DEF from the rezzed program's
///      catalog entry).
///    - Targeting a **Netrunner**: `INT + Interface_rank + d10`.
///    - Targeting a **Black ICE**: treated as a program target (DEF=0 stub,
///      since Black ICE cataloging is WP-209).
/// 5. If attack total > defense total (ties favour defender per p.129):
///    - Roll damage dice specified by `ProgramEffect`.
///    - Reduce the target's REZ / apply brain damage.
/// 6. Remove the attacker program from `NetrunState::rezzed_programs`.
///
/// ## Errors
///
/// - [`RulesError::WrongProgramClass`] — `request.program` is not an
///   Attacker-class program (Booster / Defender rejected).
/// - [`RulesError::EntityNotFound`] — the netrunner entity or target
///   netrunner entity is not in `world`.
/// - [`RulesError::ProgramNotFound`] — the program slug is not in `catalog`.
/// - [`RulesError::NetrunNotActive`] — `world.netrun` is `None`.
///
/// See p.201, p.203–204.
pub fn activate_attacker(
    world: &mut World,
    catalog: &Catalog<Program>,
    request: AttackerActivation,
    rng: &mut Rng,
) -> Result<AttackerOutcome, RulesError> {
    // Step 1 — look up the program in the catalog. See p.201.
    let program = catalog
        .get(&request.program.0)
        .ok_or_else(|| RulesError::ProgramNotFound(request.program.clone()))?;

    // Step 1a — validate that it is an Attacker-class program. See p.202.
    // "A Program whose Class specifies a type of target [...] is only
    // effective when used against its intended target." Booster and Defender
    // programs cannot be "activated" as attacks.
    match program.class {
        ProgramClass::AntiPersonnelAttacker | ProgramClass::AntiProgramAttacker => {
            // Valid — continue.
        }
        _ => {
            return Err(RulesError::WrongProgramClass {
                program: request.program.clone(),
                expected: "Attacker (AntiPersonnelAttacker or AntiProgramAttacker)",
                got: format!("{:?}", program.class),
                found: format!("{:?}", program.class),
            });
        }
    }

    // Step 2 — look up the netrunner entity. See p.201.
    let netrunner_entity = world
        .entity(request.netrunner)
        .ok_or(RulesError::EntityNotFound(request.netrunner))?;

    // Capture attack inputs (INT + Interface_rank + ATK). See p.201.
    // Interface rank is `role_rank` — the Netrunner Role Ability. See p.198.
    let att_int = netrunner_entity.current_int();
    let att_interface = netrunner_entity.role_rank as i16;
    let program_atk = program.atk as i16;
    // Snapshot DEF for use in target-program defence roll if needed.
    let program_def = program.def;

    // Step 3 — roll attack d10. See p.201.
    let att_d10 = d10_with_crits(rng);
    // Attack total: INT + Interface_rank + ATK + d10.net (p.201).
    let att_total = att_int + att_interface + program_atk + att_d10.net;

    // Step 4 — compute defense total. See p.201.
    // "The target rolls 1d10 and adds their DEF stat (if attacking a Program)
    // or their Interface Rank (if attacking a Netrunner)."
    let (def_total, _def_d10) = match &request.target {
        AttackTarget::Program(instance_id) => {
            // Defending program: look up its DEF from the rezzed list.
            // We need the program's catalog entry to get DEF.
            let target_rez_entry = world
                .netrun
                .as_ref()
                .and_then(|ns| {
                    ns.rezzed_programs
                        .iter()
                        .find(|rp| &rp.instance_id == instance_id)
                })
                .ok_or(RulesError::NetrunNotActive)?;

            let target_program_slug = target_rez_entry.program.0.clone();
            let target_catalog_entry = catalog
                .get(&target_program_slug)
                .ok_or(RulesError::ProgramNotFound(ProgramId(target_program_slug)))?;

            let target_def = target_catalog_entry.def as i16;
            let def_d10 = d10_with_crits(rng);
            (target_def + def_d10.net, def_d10)
        }
        AttackTarget::Netrunner(target_id) => {
            // Defending netrunner: INT + Interface_rank + d10. See p.201.
            let target_entity = world
                .entity(*target_id)
                .ok_or(RulesError::EntityNotFound(*target_id))?;
            let def_int = target_entity.current_int();
            let def_interface = target_entity.role_rank as i16;
            let def_d10 = d10_with_crits(rng);
            (def_int + def_interface + def_d10.net, def_d10)
        }
        AttackTarget::BlackIce(_floor_idx) => {
            // Black ICE: DEF is a stub (WP-209 lands the Black ICE catalog).
            // Use program_def as the defender's bonus, defaulting to 0 since
            // `program_def` is the *attacker's* DEF field. Black ICE DEF will
            // be wired in once WP-209 provides the catalog.
            //
            // For now, defence = 0 + d10 (treating unresolved ICE DEF as 0).
            // See p.201, p.203.
            let _ = program_def; // suppress unused warning
            let def_d10 = d10_with_crits(rng);
            (def_d10.net, def_d10)
        }
    };

    // Step 4a — build CheckBreakdown for the attacker side.
    // stat_value   = INT
    // skill_value  = Interface_rank + ATK (both contribute to the attack roll)
    // modifier_total = 0 (no situational modifiers at this layer)
    // luck_spent = 0 (activating programs does not spend LUCK in RAW)
    // dv = saturated defense total
    let dv_raw = def_total.clamp(0, i16::from(u8::MAX)) as u8;
    let breakdown = CheckBreakdown::new(
        att_int,
        att_interface + program_atk,
        0,
        0,
        att_d10,
        crate::types::DV(dv_raw),
    );

    // Step 5 — determine hit or miss. Ties favour the defender (p.129).
    let hit = att_total > def_total;

    // Step 5a — roll damage on hit. See pp.203–204.
    let (damage_rolls, damage_total, effect_applied) = if hit {
        apply_effect(world, &request, program, rng)?
    } else {
        (vec![], 0u16, AttackerEffectApplied::Missed)
    };

    // Step 6 — self-derezz: remove the attacker program from the rezzed list.
    // "The Program automatically Derezzes after its Attack." (p.201)
    let self_derezzed = if let Some(ns) = world.netrun.as_mut() {
        ns.derez_program(request.program_instance.clone()).is_some()
    } else {
        false
    };

    Ok(AttackerOutcome {
        breakdown,
        damage_rolls,
        damage_total,
        effect_applied,
        self_derezzed,
    })
}

// ---------------------------------------------------------------------------
// Internal: apply effect on hit
// ---------------------------------------------------------------------------

/// Apply the damage / effect on a hit, returning
/// `(damage_rolls, damage_total, effect_applied)`. See pp.203–204.
fn apply_effect(
    world: &mut World,
    request: &AttackerActivation,
    program: &Program,
    rng: &mut Rng,
) -> Result<(Vec<u8>, u16, AttackerEffectApplied), RulesError> {
    match &program.effect {
        // Banhammer and Sword — Anti-Program damage. See p.203, p.204.
        ProgramEffect::AnyAttackerProgramDamage {
            dice_vs_non_black_ice,
            dice_vs_black_ice,
        } => {
            let dice_spec = match &request.target {
                AttackTarget::BlackIce(_) => dice_vs_black_ice,
                _ => dice_vs_non_black_ice,
            };
            let rolls = roll_dice(dice_spec, rng);
            let total: u16 = rolls.iter().map(|&d| u16::from(d)).sum();

            let effect = match &request.target {
                AttackTarget::Program(instance_id) => {
                    // Apply damage to the target program's REZ.
                    let ns = world.netrun.as_mut().ok_or(RulesError::NetrunNotActive)?;
                    let target = ns
                        .rezzed_programs
                        .iter_mut()
                        .find(|rp| &rp.instance_id == instance_id)
                        .ok_or(RulesError::NetrunNotActive)?;

                    let rez_lost = total.min(u16::from(target.current_rez)) as u8;
                    target.current_rez = target.current_rez.saturating_sub(rez_lost);
                    let derezzed = target.current_rez == 0;
                    if derezzed {
                        // Derez it from the list.
                        let iid = instance_id.clone();
                        ns.derez_program(iid);
                    }
                    AttackerEffectApplied::ProgramDamage { rez_lost, derezzed }
                }
                AttackTarget::BlackIce(floor_idx) => {
                    // Black ICE REZ management is stub — WP-209 lands the
                    // authoritative ICE state. We report rez_lost and derezzed
                    // but cannot mutate ICE state here.
                    let _ = floor_idx;
                    AttackerEffectApplied::BlackIceDamage {
                        rez_lost: total.min(255) as u8,
                        derezzed: false, // stub: WP-209 provides live ICE REZ
                    }
                }
                AttackTarget::Netrunner(target_id) => {
                    // AnyAttackerProgramDamage aimed at a Netrunner is unusual
                    // (Banhammer is anti-program) but not impossible in RAW.
                    let hp_lost = total.min(255) as u8;
                    if let Some(target_char) = world.entity_mut(*target_id) {
                        target_char.wounds.current_hp = target_char
                            .wounds
                            .current_hp
                            .saturating_sub(i16::from(hp_lost));
                    }
                    AttackerEffectApplied::NetrunnerBrainDamage { hp_lost }
                }
            };
            Ok((rolls, total, effect))
        }

        // Hellbolt — BrainDamageWithBurn (2d6 + 2 HP/turn). See p.204.
        ProgramEffect::BrainDamageWithBurn { dice, .. } => {
            let rolls = roll_dice(dice, rng);
            let total: u16 = rolls.iter().map(|&d| u16::from(d)).sum();
            let hp_lost = total.min(255) as u8;
            if let AttackTarget::Netrunner(target_id) = &request.target {
                if let Some(target_char) = world.entity_mut(*target_id) {
                    target_char.wounds.current_hp = target_char
                        .wounds
                        .current_hp
                        .saturating_sub(i16::from(hp_lost));
                }
            }
            // The burn-per-turn side-effect is tracked by the caller via the
            // effect-stack mechanism; we only report the immediate brain damage.
            Ok((
                rolls,
                total,
                AttackerEffectApplied::NetrunnerBrainDamage { hp_lost },
            ))
        }

        // Vrizzbolt — BrainDamageAndNetActionPenalty (1d6 + -1 NET Action). See p.204.
        ProgramEffect::BrainDamageAndNetActionPenalty { dice, .. } => {
            let rolls = roll_dice(dice, rng);
            let total: u16 = rolls.iter().map(|&d| u16::from(d)).sum();
            let hp_lost = total.min(255) as u8;
            if let AttackTarget::Netrunner(target_id) = &request.target {
                if let Some(target_char) = world.entity_mut(*target_id) {
                    target_char.wounds.current_hp = target_char
                        .wounds
                        .current_hp
                        .saturating_sub(i16::from(hp_lost));
                }
            }
            // The NET Action penalty is a deferred effect tracked by the caller
            // via the effect-stack; we report only the immediate brain damage.
            Ok((
                rolls,
                total,
                AttackerEffectApplied::NetrunnerBrainDamage { hp_lost },
            ))
        }

        // DeckKRASH — ForceUnsafeJackOut. No dice damage; the "damage" is
        // the unsafe jack-out which the caller must execute. See p.204.
        ProgramEffect::ForceUnsafeJackOut => {
            // ForceUnsafeJackOut has no damage dice. The unsafe jack-out
            // effect must be carried out by the caller (GM / Beat layer).
            // We report brain damage of 0 so the outcome type stays uniform.
            Ok((
                vec![],
                0,
                AttackerEffectApplied::NetrunnerBrainDamage { hp_lost: 0 },
            ))
        }

        // Nervescrub — NervescrubStatDrain. No dice damage; stat drain is a
        // deferred effect. See p.204.
        ProgramEffect::NervescrubStatDrain { .. } => Ok((
            vec![],
            0,
            AttackerEffectApplied::NetrunnerBrainDamage { hp_lost: 0 },
        )),

        // Poison Flatline — DestroyRandomInstalledProgram. No dice damage;
        // the destruction is deferred. See p.204.
        ProgramEffect::DestroyRandomInstalledProgram => Ok((
            vec![],
            0,
            AttackerEffectApplied::NetrunnerBrainDamage { hp_lost: 0 },
        )),

        // Superglue — BlockJackOutAndProgress. Duration dice rolled; the
        // block is a deferred effect. See p.204.
        ProgramEffect::BlockJackOutAndProgress { dice } => {
            // Roll the duration dice so the RNG advances deterministically,
            // but the locking effect is deferred to the caller's effect-stack.
            let rolls = roll_dice(dice, rng);
            let _ = rolls;
            Ok((
                vec![],
                0,
                AttackerEffectApplied::NetrunnerBrainDamage { hp_lost: 0 },
            ))
        }

        // Booster and Defender effects cannot reach here because activate_attacker
        // already gates on ProgramClass. Add explicit arms as a defensive
        // compile-time guard so any new ProgramEffect variant causes a compile
        // error rather than a silent incorrect match.
        ProgramEffect::BoostCheck { .. }
        | ProgramEffect::BlockBlackIceDamage { .. }
        | ProgramEffect::NullifyAttackerAtk
        | ProgramEffect::StopFirstNonBlackIceEffect => {
            // These should be unreachable because activate_attacker gates on
            // ProgramClass before reaching apply_effect. Guard retained for
            // safety; a panic here indicates a logic error in the caller.
            unreachable!(
                "apply_effect called with a non-Attacker ProgramEffect variant — \
                 this is a bug in activate_attacker's class gate"
            )
        }
    }
}

// ---------------------------------------------------------------------------
// Internal: roll a DiceSpec
// ---------------------------------------------------------------------------

/// Roll `spec.n` dice of kind `spec.die` and return the individual results.
///
/// Currently only `DieKind::D6` dice are used by all non-Black-ICE Attacker
/// programs (pp.203–204). `DieKind::D10` is supported for forward-compatibility.
///
/// See `DiceSpec` in [`crate::catalog::programs`].
fn roll_dice(spec: &DiceSpec, rng: &mut Rng) -> Vec<u8> {
    match spec.die {
        DieKind::D6 => ndn_d6(spec.n, rng),
        DieKind::D10 => (0..spec.n).map(|_| crate::dice::d10(rng)).collect(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::programs::load_programs_catalog;
    use crate::catalog::Catalog;
    use crate::character::{Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::dice::d10;
    use crate::effects::EffectStack;
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::types::{CharacterId, Eurobucks};
    use crate::world::World;
    use rand::SeedableRng;
    use std::path::PathBuf;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("programs.ron");
        p
    }

    fn load_catalog() -> Catalog<Program> {
        load_programs_catalog(&catalog_path()).expect("catalog must load")
    }

    fn make_netrunner(uuid: u128, int: u8, role_rank: u8, luck: u8) -> crate::character::Character {
        crate::character::Character {
            id: CharacterId(Uuid::from_u128(uuid)),
            name: "Test Netrunner".into(),
            handle: None,
            role: Role::Netrunner,
            role_rank,
            stats: StatBlock {
                int,
                r#ref: 6,
                dex: 6,
                tech: 6,
                cool: 5,
                will: 5,
                luck,
                r#move: 5,
                body: 5,
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds {
                current_hp: 35,
                max_hp: 35,
                ..Default::default()
            },
            humanity: 50,
            luck_pool: luck,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    fn entity_id(uuid: u128) -> EntityId {
        EntityId(Uuid::from_u128(uuid))
    }

    fn arch_id(s: &str) -> NetArchId {
        NetArchId(s.to_string())
    }

    /// Find the first seed where `pred(&mut Rng)` returns `true` (scans up to
    /// 2,000,000 seeds). Used to pin specific dice values for deterministic tests.
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(&mut Rng) -> bool,
    {
        for seed in 0u64..2_000_000 {
            let mut r = Rng::seed_from_u64(seed);
            if pred(&mut r) {
                return seed;
            }
        }
        panic!("no matching seed found within search bound");
    }

    // -----------------------------------------------------------------------
    // test_banhammer_attacks_program
    // -----------------------------------------------------------------------

    /// `test_banhammer_attacks_program`:
    /// Banhammer (Anti-Program, ATK 1, 3d6 vs non-Black-ICE) attacks a rezzed
    /// program. On a hit the target program loses REZ equal to the 3d6 sum.
    ///
    /// Setup:
    /// - Netrunner: INT 8, Interface 6 → attack base 15 + ATK 1 = 16 before d10.
    /// - Target program: DEF 0 (eraser has DEF 0). → defense base 0 before d10.
    /// - We choose a seed that produces att_d10 = 7 and def_d10 = 3, so
    ///   att_total = 16 + 7 = 23 > def_total = 0 + 3 = 3 → hit guaranteed.
    ///
    /// See p.201, p.203.
    #[test]
    fn test_banhammer_attacks_program() {
        let catalog = load_catalog();

        // Build world with netrunner (uuid 0x10).
        let nr = make_netrunner(0x10, 8, 6, 5);
        let mut world = World::new(nr);

        // Start a netrun.
        let mut ns = NetrunState::start(entity_id(0x10), arch_id("corp-lvl1"), 6);

        // Rez the target program (eraser, DEF 0, REZ 7). Instance will be 0x01.
        let target_instance = ns.rez_program(ProgramId("eraser".into()), 7);
        // Rez the banhammer (attacker). Instance will be 0x02.
        let banhammer_instance = ns.rez_program(ProgramId("banhammer".into()), 0);

        world.netrun = Some(ns);

        // Find a seed where:
        //   d10 #1 (attacker roll) != 1 and != 10 (avoid crits in this test),
        //   d10 #2 (defender roll) != 1 and != 10,
        //   and att_total > def_total.
        // Attacker base = 8 + 6 + 1 = 15. Defender base = 0.
        // Any non-crit pair where att_d10 > def_d10 - 15 suffices.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let d = d10(r);
            a != 1 && a != 10 && d != 1 && d != 10 && (15 + a as i16 > d as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let req = AttackerActivation {
            netrunner: entity_id(0x10),
            program: ProgramId("banhammer".into()),
            program_instance: banhammer_instance.clone(),
            target: AttackTarget::Program(target_instance.clone()),
        };

        let outcome = activate_attacker(&mut world, &catalog, req, &mut rng)
            .expect("activate_attacker must succeed");

        // Attack must have hit.
        assert!(
            outcome.breakdown.success,
            "attack must hit (att base >> def base)"
        );

        // Damage rolls: 3d6 (Banhammer vs non-Black-ICE, p.203).
        assert_eq!(
            outcome.damage_rolls.len(),
            3,
            "Banhammer rolls 3d6 vs non-Black-ICE (p.203)"
        );

        // Damage total = sum of rolls.
        let expected_total: u16 = outcome.damage_rolls.iter().map(|&d| u16::from(d)).sum();
        assert_eq!(
            outcome.damage_total, expected_total,
            "damage_total must equal sum of damage_rolls"
        );

        // Effect applied: ProgramDamage.
        assert!(
            matches!(
                &outcome.effect_applied,
                AttackerEffectApplied::ProgramDamage { .. }
            ),
            "Banhammer vs program must produce ProgramDamage"
        );

        if let AttackerEffectApplied::ProgramDamage { rez_lost, derezzed } = &outcome.effect_applied
        {
            assert_eq!(
                *rez_lost,
                outcome.damage_total.min(7) as u8,
                "rez_lost must equal damage capped at target's max REZ"
            );
            // eraser has 7 REZ; 3d6 min = 3 so eraser may or may not be derezzed.
            if *rez_lost >= 7 {
                assert!(*derezzed, "must be derezzed when rez_lost >= original REZ");
            }
        }

        // Banhammer must be removed from rezzed_programs after use (p.201).
        assert!(
            outcome.self_derezzed,
            "Attacker must self-derezz after use (p.201)"
        );
        let ns = world.netrun.as_ref().unwrap();
        assert!(
            !ns.rezzed_programs
                .iter()
                .any(|rp| rp.instance_id == banhammer_instance),
            "banhammer must be removed from rezzed_programs after use"
        );
    }

    // -----------------------------------------------------------------------
    // test_sword_attacks_target
    // -----------------------------------------------------------------------

    /// `test_sword_attacks_target`:
    /// Sword (Anti-Program, ATK 1, 2d6 vs non-Black-ICE) attacks a rezzed
    /// program and the attack lands. Verifies 2d6 damage dice.
    ///
    /// See p.204.
    #[test]
    fn test_sword_attacks_target() {
        let catalog = load_catalog();

        let nr = make_netrunner(0x20, 7, 5, 5);
        let mut world = World::new(nr);

        let mut ns = NetrunState::start(entity_id(0x20), arch_id("militech-sec"), 5);
        let target_instance = ns.rez_program(ProgramId("eraser".into()), 7);
        let sword_instance = ns.rez_program(ProgramId("sword".into()), 0);
        world.netrun = Some(ns);

        // Sword base = INT 7 + Interface 5 + ATK 1 = 13 before d10.
        // Target (eraser) DEF = 0. Guaranteed hit if att_d10 >= 1 (which is
        // always), since 13 + 1 = 14 > 0 + 10 = 10 at most. For robustness,
        // find a seed where both d10s are normal and att_total > def_total.
        let seed = find_seed_where(|r| {
            let a = d10(r);
            let d = d10(r);
            a != 1 && a != 10 && d != 1 && d != 10 && (13 + a as i16 > d as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let req = AttackerActivation {
            netrunner: entity_id(0x20),
            program: ProgramId("sword".into()),
            program_instance: sword_instance.clone(),
            target: AttackTarget::Program(target_instance.clone()),
        };

        let outcome = activate_attacker(&mut world, &catalog, req, &mut rng)
            .expect("activate_attacker must succeed");

        assert!(outcome.breakdown.success, "sword attack must hit");

        // Sword rolls 2d6 vs non-Black-ICE (p.204).
        assert_eq!(
            outcome.damage_rolls.len(),
            2,
            "Sword rolls 2d6 vs non-Black-ICE (p.204)"
        );

        assert!(
            matches!(
                &outcome.effect_applied,
                AttackerEffectApplied::ProgramDamage { .. }
            ),
            "Sword vs program must produce ProgramDamage"
        );

        // Sword must self-derezz.
        assert!(
            outcome.self_derezzed,
            "Sword must self-derezz after use (p.201)"
        );
        let ns = world.netrun.as_ref().unwrap();
        assert!(
            !ns.rezzed_programs
                .iter()
                .any(|rp| rp.instance_id == sword_instance),
            "sword must be removed from rezzed_programs"
        );
    }

    // -----------------------------------------------------------------------
    // test_attacker_self_derezzes_after_use
    // -----------------------------------------------------------------------

    /// `test_attacker_self_derezzes_after_use`:
    /// Even on a miss, the Attacker program must be removed from
    /// `rezzed_programs`. See p.201.
    ///
    /// We engineer a guaranteed miss by using a seed where the defender's
    /// total greatly exceeds the attacker's total.
    #[test]
    fn test_attacker_self_derezzes_after_use() {
        let catalog = load_catalog();

        // Weak netrunner: INT 1, Interface 1, ATK 1 → base 3.
        let nr = make_netrunner(0x30, 1, 1, 5);
        let mut world = World::new(nr);

        let mut ns = NetrunState::start(entity_id(0x30), arch_id("gang-hideout"), 1);
        // Target: eraser with DEF 0. But we need the defense d10 to beat the
        // attack. Since defense base = 0 + d10 and attack base = 3 + d10,
        // we need defense d10 > attack d10 + 3.
        // To get a near-certain miss, we give the target a stand-in slug with
        // high DEF. But since eraser has DEF=0, we have to rely on d10 variance.
        // Instead, use a Netrunner target (INT+Interface+d10) with high stats.
        //
        // Actually let's use a Netrunner target to guarantee a miss more easily.
        // Add a strong defender NPC.
        let strong_npc = make_netrunner(0x31, 10, 10, 0); // INT 10, Interface 10
        world
            .npcs
            .insert(crate::types::NpcId(Uuid::from_u128(0x31)), strong_npc);

        let banhammer_instance = ns.rez_program(ProgramId("banhammer".into()), 0);
        world.netrun = Some(ns);

        // Find a seed where att_total < def_total.
        // att = 1 + 1 + 1 + att_d10, def = 10 + 10 + def_d10.
        // att_base = 3, def_base = 20. Even att_d10=10 (max), att ≤ 23 < 20 + 3 = 23.
        // So we need att_d10 < def_d10 + 17, or basically any non-crit att
        // against a non-crit def.
        let seed = find_seed_where(|r| {
            let a = d10(r); // att d10
            let d = d10(r); // def d10
            (3i16 + a as i16) <= (20i16 + d as i16)
        });
        let mut rng = Rng::seed_from_u64(seed);

        let req = AttackerActivation {
            netrunner: entity_id(0x30),
            program: ProgramId("banhammer".into()),
            program_instance: banhammer_instance.clone(),
            target: AttackTarget::Netrunner(entity_id(0x31)),
        };

        let outcome = activate_attacker(&mut world, &catalog, req, &mut rng)
            .expect("activate_attacker must succeed even on a miss");

        // Verify it's a miss.
        assert!(
            !outcome.breakdown.success,
            "attack must miss (weak attacker vs strong defender)"
        );
        assert!(
            matches!(outcome.effect_applied, AttackerEffectApplied::Missed),
            "effect_applied must be Missed on a miss"
        );
        assert_eq!(outcome.damage_rolls, vec![], "no damage rolls on a miss");
        assert_eq!(outcome.damage_total, 0, "damage_total must be 0 on a miss");

        // Self-derezz must still happen on a miss. See p.201.
        assert!(
            outcome.self_derezzed,
            "Attacker must self-derezz after use, even on a miss (p.201)"
        );
        let ns = world.netrun.as_ref().unwrap();
        assert!(
            !ns.rezzed_programs
                .iter()
                .any(|rp| rp.instance_id == banhammer_instance),
            "banhammer must be removed from rezzed_programs even after a miss"
        );
    }

    // -----------------------------------------------------------------------
    // test_rejects_booster_class
    // -----------------------------------------------------------------------

    /// `test_rejects_booster_class`:
    /// Passing a Booster program slug (e.g. "eraser") to `activate_attacker`
    /// must return `Err(RulesError::WrongProgramClass)`.
    ///
    /// See p.202: "Booster programs improve abilities; they are not Attackers."
    #[test]
    fn test_rejects_booster_class() {
        let catalog = load_catalog();

        let nr = make_netrunner(0x40, 6, 4, 5);
        let mut world = World::new(nr);

        let mut ns = NetrunState::start(entity_id(0x40), arch_id("office"), 4);
        // Rez eraser (a Booster) as the "attacking" program.
        let eraser_instance = ns.rez_program(ProgramId("eraser".into()), 7);
        // Rez a target.
        let target_instance = ns.rez_program(ProgramId("flak".into()), 7);
        world.netrun = Some(ns);

        let mut rng = Rng::seed_from_u64(99);

        let req = AttackerActivation {
            netrunner: entity_id(0x40),
            program: ProgramId("eraser".into()), // Booster!
            program_instance: eraser_instance,
            target: AttackTarget::Program(target_instance),
        };

        let err = activate_attacker(&mut world, &catalog, req, &mut rng)
            .expect_err("activating a Booster as an Attacker must fail");

        assert!(
            matches!(err, RulesError::WrongProgramClass { .. }),
            "expected WrongProgramClass, got: {err:?}"
        );
    }
}
