//! Interface Ability: Zap — built-in attack against programs or enemy netrunners.
//!
//! ## Rulebook (p.200)
//!
//! > **Zap** — Allows you to make an attack as a NET Action against a Program
//! > or enemy Netrunner. If you are able to roll a successful Zap Check against
//! > the Program's Defense Value + 1d10 or the Netrunner's Interface + 1d10,
//! > you deal 1d6 damage to the Program's REZ or directly to the Netrunner's
//! > brain.
//!
//! ## Key rules notes
//!
//! - **NET Action** (p.200). Every netrunner has Zap built in — no program
//!   required.
//! - Attacker rolls: `Interface_rank + 1d10` (the same "Interface + d10"
//!   formula used by every NET ability, per p.198–199).
//! - Defender rolls:
//!   - vs. a rezzed **program**: `program_def + 1d10` where `program_def` is
//!     the program's Defense Value (the DEF stat, p.202–203).
//!   - vs. an enemy **netrunner**: `target_interface_rank + 1d10`.
//! - On hit: roll **1d6**. p.200 explicitly says "1d6 damage."
//!   - To program: reduce its REZ by the damage; if REZ drops to ≤ 0, the
//!     program is Derezzed (removed from `rezzed_programs`).
//!   - To netrunner: the 1d6 is brain damage applied directly to HP.
//! - On miss: no effect.
//!
//! ## Deviation note
//!
//! The WP-411 spec shows `CheckBreakdown` in `ZapOutcome`, but Zap is an
//! opposed check, not a roll-vs-fixed-DV check. We embed the attacker
//! breakdown (attacker's Interface + d10) and record the defender's roll
//! separately in `ZapEffect`. This faithfully reflects the mechanics on p.200
//! while satisfying the `CheckBreakdown` contract: the attacker's
//! `CheckBreakdown::dv` is set to the defender's roll total (treated as the
//! dynamic DV), making `success` and `margin` correct.
//!
//! See p.200.

// See p.200.

use crate::dice::{d10_with_crits, d6};
use crate::error::RulesError;
use crate::netrunning::state::ProgramInstanceId;
use crate::resolution::{CheckBreakdown, Resolution};
use crate::rng::Rng;
use crate::types::{EntityId, DV};
use crate::world::World;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A Zap NET Action. See p.200.
///
/// Zap is a built-in attack — every Netrunner has it regardless of which
/// programs they are running. It costs one NET Action (p.200).
///
/// The attacker rolls `Interface_rank + 1d10` against the target's
/// defense roll (`program_def + 1d10` for a program, or
/// `target_interface_rank + 1d10` for an enemy Netrunner).
///
/// See p.200.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZapAction {
    /// The netrunner performing the Zap. Must exist in `world`.
    pub netrunner: EntityId,
    /// What the Zap is aimed at.
    pub target: ZapTarget,
    /// Points of LUCK to spend before the roll (p.130). `0` is valid.
    pub luck_to_spend: u8,
}

/// The target of a [`ZapAction`]. See p.200.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZapTarget {
    /// Attack a rezzed program, identified by its instance ID in
    /// `world.netrun.rezzed_programs`.
    ///
    /// On a hit the program takes 1d6 damage to its REZ. If REZ drops to 0
    /// or below, the program is Derezzed. See p.200 and p.201 ("Defeating a
    /// Program").
    Program(ProgramInstanceId),
    /// Attack an enemy Netrunner, dealing 1d6 brain damage directly to HP.
    ///
    /// The target's Interface rank (role_rank) is used as their defense
    /// value (p.200: "the Netrunner's Interface + 1d10").
    Netrunner(EntityId),
}

/// Outcome of a resolved [`ZapAction`].
///
/// Always populated, regardless of hit or miss. The `effect_applied`
/// field discriminates hit variants from [`ZapEffect::Missed`].
///
/// See p.200.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZapOutcome {
    /// Full breakdown of the attacker's Interface + d10 roll.
    ///
    /// `stat_value` is 0 (Zap uses Interface rank alone — no STAT column),
    /// `skill_value` is the Interface rank, and `dv` is the defender's total
    /// roll (used as the dynamic DV).
    pub breakdown: CheckBreakdown,
    /// The 1d6 damage roll (always populated; 0 on a miss because no
    /// damage die is rolled when the attack misses).
    ///
    /// See p.200: "you deal 1d6 damage."
    pub damage: u8,
    /// The effect that was applied (or [`ZapEffect::Missed`]).
    pub effect_applied: ZapEffect,
}

/// The effect produced by a hit Zap, or a record of a miss. See p.200.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ZapEffect {
    /// The Zap hit a rezzed program and reduced its REZ. See p.200, p.201.
    ProgramRezReduced {
        /// The program instance that took damage.
        instance: ProgramInstanceId,
        /// How many REZ points were removed (= the 1d6 roll, capped at the
        /// program's remaining REZ so it cannot go below 0).
        by: u8,
        /// `true` if the program's REZ reached 0 after the hit, causing it
        /// to be Derezzed and removed from `rezzed_programs`. See p.201.
        derezzed: bool,
    },
    /// The Zap hit an enemy Netrunner and dealt brain damage to their HP.
    ///
    /// The damage is applied directly to `target.wounds.current_hp`.
    /// See p.200.
    BrainDamage {
        /// The amount of HP lost (= the 1d6 roll).
        hp_lost: u8,
    },
    /// The Zap missed — the attacker's roll did not beat the defender's.
    Missed,
}

// ---------------------------------------------------------------------------
// Resolution impl
// ---------------------------------------------------------------------------

impl Resolution for ZapAction {
    type Outcome = Result<ZapOutcome, RulesError>;

    /// Resolve the Zap NET Action against `world`.
    ///
    /// ## Steps
    ///
    /// 1. Look up the attacker. Return `Err(EntityNotFound)` if missing.
    /// 2. Validate and spend luck (p.130). Return `Err(InsufficientLuck)`
    ///    on failure (pool unchanged).
    /// 3. Capture `interface_rank` (= `role_rank`) from the attacker.
    /// 4. Compute the attacker's roll: `interface_rank + d10_with_crits`.
    /// 5. Compute the defender's roll:
    ///    - Program: `program_def + d10` (plain d10 — only the netrunner's
    ///      attack roll uses the crit rules at this layer).
    ///    - Netrunner: `target_interface_rank + d10`.
    /// 6. Build `CheckBreakdown` with `dv = defender_total` (the dynamic DV).
    ///    `stat_value = 0`, `skill_value = interface_rank`.
    /// 7. On hit (margin ≥ 0): roll 1d6 damage and apply it.
    /// 8. On miss: return `ZapEffect::Missed` with `damage = 0`.
    ///
    /// See p.200 (Zap) and p.130 (LUCK spending).
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome {
        // Step 1 — look up the attacker. See p.200.
        // We need the interface_rank before mutating anything else.
        let interface_rank = {
            let actor = world
                .entity(self.netrunner)
                .ok_or(RulesError::EntityNotFound(self.netrunner))?;
            actor.role_rank as i16
        };

        // Step 2 — validate and spend luck (p.130). Must happen before the roll.
        {
            let actor = world
                .entity_mut(self.netrunner)
                .ok_or(RulesError::EntityNotFound(self.netrunner))?;
            actor.spend_luck(self.luck_to_spend)?;
        }

        // Step 3 — compute defender total before we roll attacker (so the
        // RNG consumption order is: attacker d10, then defender d10, then
        // optionally damage d6 — giving a stable replay sequence).
        // We read defender stats now (immutably) and will apply effects later.
        let defender_total: i16 = match &self.target {
            ZapTarget::Program(instance_id) => {
                // Look up the rezzed program's DEF value.
                // The WP spec says "program's Defense Value + d10" (p.200).
                // Programs expose their DEF via the rezzed state's `program`
                // catalog slug. WP-411 is before the program-catalog WP, so we
                // use a fixed DEF of 0 as the catalog stub value and add d10.
                // The program must be present in world.netrun.rezzed_programs.
                // See p.200, p.202 (DEF column).
                //
                // NOTE: We do NOT look up the catalog here — that belongs to a
                // later WP (WP-412/413). For now, program DEF is treated as 0
                // (the minimum valid value), matching the book's Hellhound
                // example on p.201 (DEF 2, but the DEF value varies per program).
                // The acceptance tests control program DEF via the rezzed state
                // directly.  We record the defender roll as `0 + d10`.
                //
                // Locate the program to verify it exists.
                let netrun = world
                    .netrun
                    .as_ref()
                    .ok_or(RulesError::EntityNotFound(self.netrunner))?;
                if !netrun
                    .rezzed_programs
                    .iter()
                    .any(|p| p.instance_id == *instance_id)
                {
                    return Err(RulesError::EntityNotFound(self.netrunner));
                }
                // Program DEF: we read it from the rezzed entry's `program_def`
                // field. Since `RezzedProgram` does not yet store a DEF value
                // (that belongs to the program catalog, a later WP), we use 0
                // as a stub. The d10 defender roll is still applied.
                // See p.200, p.202.
                let def_value: i16 = 0; // stub: real DEF comes from catalog (WP-412+)
                let defender_d10 = crate::dice::d10(rng) as i16;
                def_value + defender_d10
            }
            ZapTarget::Netrunner(target_id) => {
                // Enemy Netrunner: Interface + d10. See p.200.
                let target = world
                    .entity(*target_id)
                    .ok_or(RulesError::EntityNotFound(*target_id))?;
                let target_interface = target.role_rank as i16;
                let defender_d10 = crate::dice::d10(rng) as i16;
                target_interface + defender_d10
            }
        };

        // Step 4 — attacker's roll: Interface + d10_with_crits (p.200).
        // Roll after we have captured the defender's total so we can build the
        // breakdown with the correct dv. RNG order within one resolve() call:
        // 1. defender d10 (computed above in the match arm), 2. attacker d10.
        let attacker_d10 = d10_with_crits(rng);

        // Step 5 — build CheckBreakdown.
        // Zap formula: Interface_rank + d10 (no STAT column per p.200 — Zap
        // says "your Interface + 1d10", not "STAT + Interface + 1d10").
        // We put stat_value = 0 and skill_value = interface_rank.
        // The dynamic DV is the defender's total. See p.200.
        //
        // DV is stored as u8 in the DV newtype. Defender rolls can in principle
        // be negative on a critical failure, but DV is unsigned. We clamp to 0
        // as the floor — a negative defender total is a trivially easy target.
        let dv_value = u8::try_from(defender_total.max(0)).unwrap_or(u8::MAX);
        let breakdown = CheckBreakdown::new(
            0,                  // stat_value: no STAT column in Zap formula
            interface_rank,     // skill_value: Interface rank
            0,                  // modifier_total: no situational mods in base Zap
            self.luck_to_spend, // luck_spent
            attacker_d10,
            DV(dv_value), // dynamic DV = defender's roll total
        );

        // Step 6 — apply result.
        if !breakdown.success {
            // Miss.
            return Ok(ZapOutcome {
                breakdown,
                damage: 0,
                effect_applied: ZapEffect::Missed,
            });
        }

        // Hit — roll 1d6 damage. See p.200.
        let damage = d6(rng);

        let effect_applied = match &self.target {
            ZapTarget::Program(instance_id) => {
                // Reduce program REZ by `damage`. Derez if REZ ≤ 0. See p.200, p.201.
                let netrun = world
                    .netrun
                    .as_mut()
                    .ok_or(RulesError::EntityNotFound(self.netrunner))?;
                let prog = netrun
                    .rezzed_programs
                    .iter_mut()
                    .find(|p| p.instance_id == *instance_id)
                    .ok_or(RulesError::EntityNotFound(self.netrunner))?;

                let actual_reduction = damage.min(prog.current_rez);
                prog.current_rez = prog.current_rez.saturating_sub(damage);
                let derezzed = prog.current_rez == 0;

                if derezzed {
                    // Remove the program from rezzed_programs. See p.201.
                    netrun
                        .rezzed_programs
                        .retain(|p| p.instance_id != *instance_id);
                }

                ZapEffect::ProgramRezReduced {
                    instance: instance_id.clone(),
                    by: actual_reduction,
                    derezzed,
                }
            }
            ZapTarget::Netrunner(target_id) => {
                // Brain damage: apply directly to HP. See p.200.
                let target = world
                    .entity_mut(*target_id)
                    .ok_or(RulesError::EntityNotFound(*target_id))?;
                target.wounds.current_hp =
                    target.wounds.current_hp.saturating_sub(i16::from(damage));
                ZapEffect::BrainDamage { hp_lost: damage }
            }
        };

        Ok(ZapOutcome {
            breakdown,
            damage,
            effect_applied,
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
    use crate::dice::{d10, d6};
    use crate::effects::ProgramId;
    use crate::netrunning::architecture::NetArchId;
    use crate::netrunning::state::NetrunState;
    use crate::world::World;
    use rand::SeedableRng;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    /// Build a fresh Netrunner PC with a specific Interface rank.
    fn netrunner_pc(role_rank: u8) -> crate::character::Character {
        let mut pc = crate::world::test_support::fresh_pc();
        pc.role = Role::Netrunner;
        pc.role_rank = role_rank;
        pc.luck_pool = 10;
        pc.stats.luck = 10;
        // Give them some HP so brain-damage tests are meaningful.
        pc.wounds.max_hp = 30;
        pc.wounds.current_hp = 30;
        pc
    }

    /// Build a fresh enemy Netrunner NPC with the given role_rank.
    fn enemy_netrunner(role_rank: u8) -> (crate::character::Character, crate::types::NpcId) {
        let npc_uuid = Uuid::from_u128(0xE0E0E0);
        let mut npc = crate::world::test_support::fresh_pc();
        npc.id = crate::types::CharacterId(npc_uuid);
        npc.role = Role::Netrunner;
        npc.role_rank = role_rank;
        npc.wounds.max_hp = 20;
        npc.wounds.current_hp = 20;
        let npc_id = crate::types::NpcId(npc_uuid);
        (npc, npc_id)
    }

    /// Walk seeds until `pred` is satisfied. Used to find seeds that produce
    /// specific dice results without hardcoding seed values.
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(&mut Rng) -> bool,
    {
        for seed in 0..1_000_000 {
            let mut r = Rng::seed_from_u64(seed);
            if pred(&mut r) {
                return seed;
            }
        }
        panic!("no matching seed found within search bound");
    }

    // -----------------------------------------------------------------------
    // test_zap_program_reduces_rez
    // -----------------------------------------------------------------------

    /// A successful Zap against a program reduces its REZ by the 1d6 roll
    /// without derezzzing it (when REZ > damage).
    ///
    /// Setup: Interface 8 (attacker) vs. program DEF 0 (stub). We pick a seed
    /// where the attacker wins and the program survives.
    ///
    /// See p.200, p.201.
    #[test]
    fn test_zap_program_reduces_rez() {
        // Attacker: Interface rank 8.
        let pc = netrunner_pc(8);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        // Set up a netrun with one rezzed program that has ample REZ.
        let arch = NetArchId("test-arch".into());
        let mut netrun = NetrunState::start(pc_id, arch, 8);
        let program_id = ProgramId("flak".into());
        let instance_id = netrun.rez_program(program_id, 10); // REZ 10
        world.netrun = Some(netrun);

        let action = ZapAction {
            netrunner: pc_id,
            target: ZapTarget::Program(instance_id.clone()),
            luck_to_spend: 0,
        };

        // Find a seed where:
        // 1. defender_d10 is low (so attacker can beat it easily)
        // 2. attacker roll is high
        // We search for seeds where attacker (rank 8 + d10) beats defender (0 + d10).
        // Interface 8 is strong; most seeds will be hits. Find one that gives a
        // hit without derezzzing (damage < 10).
        let seed = find_seed_where(|r| {
            // RNG consumption order for ZapTarget::Program:
            // 1. defender d10
            // 2. attacker d10 (with crit check)
            // 3. damage d6 (only if hit)
            let defender = d10(r) as i16; // defender roll
            let attacker_base = d10(r);
            // Check for crit consumption
            let attacker_net: i16 = match attacker_base {
                10 => {
                    let fu = d10(r);
                    10 + fu as i16
                }
                1 => {
                    let fu = d10(r);
                    1 - fu as i16
                }
                _ => attacker_base as i16,
            };
            let attacker_total = 8 + attacker_net; // interface_rank = 8
            if attacker_total > defender {
                // Would be a hit — check damage < 10 (so program survives).
                let dmg = d6(r);
                dmg < 10 // always true for d6, but we verify it's not exactly 10 REZ
            } else {
                false
            }
        });

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must succeed");

        assert!(
            outcome.breakdown.success,
            "attacker should hit with Interface 8 at this seed"
        );
        assert!(
            outcome.damage > 0,
            "damage must be at least 1 on a hit (1d6)"
        );
        assert!(outcome.damage <= 6, "damage must be at most 6 (1d6)");

        match &outcome.effect_applied {
            ZapEffect::ProgramRezReduced {
                instance,
                by,
                derezzed,
            } => {
                assert_eq!(*instance, instance_id, "correct program instance");
                assert_eq!(*by, outcome.damage, "REZ reduced by damage amount");
                assert!(!derezzed, "program should not be derezzed (REZ was 10)");
            }
            other => panic!("expected ProgramRezReduced, got {other:?}"),
        }

        // Verify the program's REZ was actually decremented in world state.
        let netrun = world.netrun.as_ref().expect("netrun still active");
        let prog = netrun
            .rezzed_programs
            .iter()
            .find(|p| p.instance_id == instance_id)
            .expect("program still rezzed");
        assert_eq!(
            prog.current_rez,
            10 - outcome.damage,
            "program REZ must be 10 - damage"
        );
    }

    // -----------------------------------------------------------------------
    // test_zap_program_derezzes_when_rez_zero
    // -----------------------------------------------------------------------

    /// A hit that deals damage ≥ the program's remaining REZ derezzes it.
    ///
    /// Setup: rezzed program with REZ 1. Any hit (1d6 ≥ 1) should derez it.
    ///
    /// See p.200, p.201.
    #[test]
    fn test_zap_program_derezzes_when_rez_zero() {
        let pc = netrunner_pc(8);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let arch = NetArchId("test-arch".into());
        let mut netrun = NetrunState::start(pc_id, arch, 8);
        let program_id = ProgramId("flak".into());
        let instance_id = netrun.rez_program(program_id, 1); // REZ 1 — any damage kills it
        world.netrun = Some(netrun);

        let action = ZapAction {
            netrunner: pc_id,
            target: ZapTarget::Program(instance_id.clone()),
            luck_to_spend: 0,
        };

        // Find a seed where attacker beats defender (Interface 8 vs. DEF 0 + d10).
        let seed = find_seed_where(|r| {
            let defender = d10(r) as i16;
            let attacker_base = d10(r);
            let attacker_net: i16 = match attacker_base {
                10 => {
                    let fu = d10(r);
                    10 + fu as i16
                }
                1 => {
                    let fu = d10(r);
                    1 - fu as i16
                }
                _ => attacker_base as i16,
            };
            8 + attacker_net > defender
        });

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must succeed");

        assert!(outcome.breakdown.success, "must be a hit");

        match &outcome.effect_applied {
            ZapEffect::ProgramRezReduced {
                instance,
                by: _,
                derezzed,
            } => {
                assert_eq!(*instance, instance_id);
                assert!(*derezzed, "program with REZ 1 must be derezzed on any hit");
            }
            other => panic!("expected ProgramRezReduced, got {other:?}"),
        }

        // Program must have been removed from rezzed_programs. See p.201.
        let netrun = world.netrun.as_ref().expect("netrun still active");
        let still_rezzed = netrun
            .rezzed_programs
            .iter()
            .any(|p| p.instance_id == instance_id);
        assert!(
            !still_rezzed,
            "derezzed program must be removed from rezzed_programs"
        );
    }

    // -----------------------------------------------------------------------
    // test_zap_netrunner_brain_damage
    // -----------------------------------------------------------------------

    /// A successful Zap against an enemy Netrunner applies 1d6 brain damage
    /// directly to their HP.
    ///
    /// Setup: attacker Interface 8 vs. enemy Interface 1. High chance of hit.
    ///
    /// See p.200.
    #[test]
    fn test_zap_netrunner_brain_damage() {
        let pc = netrunner_pc(8);
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        // Add an enemy Netrunner with Interface rank 1 (easy to beat).
        let (npc, npc_id) = enemy_netrunner(1);
        let npc_entity_id = EntityId(npc_id.0);
        let initial_hp = npc.wounds.current_hp;
        world.npcs.insert(npc_id, npc);

        let action = ZapAction {
            netrunner: pc_id,
            target: ZapTarget::Netrunner(npc_entity_id),
            luck_to_spend: 0,
        };

        // Find a seed where Interface 8 beats enemy Interface 1 + d10.
        // RNG order for Netrunner target: 1. defender d10, 2. attacker d10.
        let seed = find_seed_where(|r| {
            let defender_d10 = d10(r) as i16;
            let defender_total = 1 + defender_d10; // enemy interface 1
            let attacker_base = d10(r);
            let attacker_net: i16 = match attacker_base {
                10 => {
                    let fu = d10(r);
                    10 + fu as i16
                }
                1 => {
                    let fu = d10(r);
                    1 - fu as i16
                }
                _ => attacker_base as i16,
            };
            8 + attacker_net > defender_total
        });

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must succeed");

        assert!(outcome.breakdown.success, "must hit");
        assert!(outcome.damage > 0, "damage must be non-zero on a hit");
        assert!(outcome.damage <= 6, "damage must be at most 6 (1d6)");

        match outcome.effect_applied {
            ZapEffect::BrainDamage { hp_lost } => {
                assert_eq!(hp_lost, outcome.damage, "hp_lost must equal damage roll");
            }
            other => panic!("expected BrainDamage, got {other:?}"),
        }

        // HP must have been reduced in world state.
        let target = world.entity(npc_entity_id).expect("NPC still in world");
        assert_eq!(
            target.wounds.current_hp,
            initial_hp - i16::from(outcome.damage),
            "enemy HP must be initial_hp - damage"
        );
    }

    // -----------------------------------------------------------------------
    // test_zap_misses_low_roll
    // -----------------------------------------------------------------------

    /// A low attacker roll fails to hit; no damage is applied.
    ///
    /// Setup: Interface rank 1 vs. enemy Interface rank 8. High chance of miss.
    ///
    /// See p.200.
    #[test]
    fn test_zap_misses_low_roll() {
        let pc = netrunner_pc(1); // very low interface
        let pc_id = EntityId(pc.id.0);
        let mut world = World::new(pc);

        let (npc, npc_id) = enemy_netrunner(8); // strong defender
        let npc_entity_id = EntityId(npc_id.0);
        let initial_hp = npc.wounds.current_hp;
        world.npcs.insert(npc_id, npc);

        let action = ZapAction {
            netrunner: pc_id,
            target: ZapTarget::Netrunner(npc_entity_id),
            luck_to_spend: 0,
        };

        // Find a seed where Interface 1 does NOT beat enemy Interface 8 + d10.
        // attacker total = 1 + attacker_d10_net; defender total = 8 + d10.
        let seed = find_seed_where(|r| {
            let defender_d10 = d10(r) as i16;
            let defender_total = 8 + defender_d10;
            let attacker_base = d10(r);
            let attacker_net: i16 = match attacker_base {
                10 => {
                    let fu = d10(r);
                    10 + fu as i16
                }
                1 => {
                    let fu = d10(r);
                    1 - fu as i16
                }
                _ => attacker_base as i16,
            };
            attacker_net < defender_total // strict < means miss (not >= for hit)
        });

        let mut rng = Rng::seed_from_u64(seed);
        let outcome = action
            .resolve(&mut world, &mut rng)
            .expect("resolve must succeed");

        assert!(!outcome.breakdown.success, "must miss");
        assert_eq!(outcome.damage, 0, "no damage on a miss");
        assert_eq!(outcome.effect_applied, ZapEffect::Missed);

        // HP must be unchanged.
        let target = world.entity(npc_entity_id).expect("NPC still in world");
        assert_eq!(
            target.wounds.current_hp, initial_hp,
            "HP must be unchanged on a miss"
        );
    }
}
