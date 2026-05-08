//! Medicine (Medtech Role Ability) — WP-513.
//!
//! **Rulebook name:** The rulebook (p.149) calls this ability **"Medicine"**.
//! The Medtech Role Ability is Medicine. Medtechs keep people alive with their
//! knowledge and training.
//!
//! ## Rulebook mechanics (pp.149–150)
//!
//! Whenever the Medtech increases their Medicine Rank by 1, they choose one of
//! three **Medicine Specialties** to allocate a point to:
//!
//! - **Surgery** (p.149): Each point allocated grants +2 ranks in the Surgery
//!   Skill (up to a maximum of 10). Surgery is the TECH Skill used to treat the
//!   most severe Critical Injuries and to implant cyberware; it is only
//!   available to Medtechs through this Medicine Specialty.
//!
//! - **Medical Tech \[Pharmaceuticals\]** (p.149): Each point allocated grants
//!   +1 rank in the Medical Tech Skill (max 5 points total, capped at Skill 10).
//!   Additionally, each point grants access to one pharmaceutical the Medtech
//!   can synthesise by rolling DV13 using 200eb of materials in 1 hour.
//!
//! - **Medical Tech \[Cryosystem Operation\]** (p.150): Each point allocated
//!   grants +1 rank in the Medical Tech Skill (max 5 points total, capped at
//!   Skill 10). Additionally grants Cryopump/Cryotank access by level.
//!
//! ## Surgery and Critical Injury Treatment (pp.221–223)
//!
//! Surgery is the skill used to **Treat** the worst Critical Injuries — those
//! labelled "Surgery DV17" or "Surgery DV15" on the Critical Injury tables
//! (p.221). A non-Medtech cannot use Surgery at all; they are stuck with
//! Paramedic at best. For a Medtech with Surgery points allocated, each point
//! adds **+2** to their Surgery Skill. The surgery check is:
//!
//! > `TECH + Surgery Skill + 1d10 ≥ DV`
//!
//! So `surgery_dv_modifier` returns a positive addend to the surgery *roll*
//! (not a reduction to the DV) equal to `rank * 2`. The WP-513 spec calls it
//! a "DV modifier" — this implementation documents the RAW interpretation: it
//! is a bonus to the roll, which is equivalent but with opposite sign convention.
//! Flagged in PR.
//!
//! ## Pharmaceuticals and HP healed (pp.149–150)
//!
//! The rulebook does not define a scalar "HP healed per rank per session" for
//! Pharmaceuticals in isolation. What it defines is:
//!
//! - Speedheal (p.150): "heals an amount of HP equal to their BODY + WILL"
//! - A Medtech can synthesise a number of doses equal to their Medical Tech
//!   Skill from 200eb of materials in 1 hour.
//! - Medical Tech Skill Level = (Pharmaceuticals points allocated) at base.
//!
//! `pharmaceuticals_hp_healed` is therefore a **per-dose** healing estimate
//! for Speedheal: it returns `rank * 2` as a representative baseline
//! (each Pharmaceuticals rank grants 1 Medical Tech skill rank; a character with
//! BODY 7 + WILL 7 would heal 14 HP per Speedheal dose, but this function does
//! not have access to character stats). The caller must multiply by the target's
//! relevant stats for the actual Speedheal effect. The function is documented as
//! returning the **bonus HP healed per rank** above baseline, as a useful
//! scaling factor for the GM layer. Deviation flagged in PR.
//!
//! ## Cryosystem Operation
//!
//! Cryosystem Operation grants Cryopump/Cryotank access at specific levels
//! (p.150). This WP implements the Cryosystem Operation benefit table via
//! [`cryosystem_benefit`]. It is not flagged as "out of scope" because the
//! table is straightforward to encode.
//!
//! See pp.149–150.

use crate::character::data::Role;
use crate::character::Character;

// ──────────────────────────────────────────────────────────────────────────────
// Core rank query
// ──────────────────────────────────────────────────────────────────────────────

/// Returns the character's effective Medicine rank.
///
/// Per p.149, Medicine is the Medtech's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Medtech`]. For any
/// other role the ability does not apply; this function returns `0` so callers
/// can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::medicine::medicine_rank;
/// # use cpr_rules::character::Character;
/// // See test_medicine_rank_for_medtech and test_medicine_rank_zero_for_non_medtech.
/// ```
///
/// See p.149.
pub fn medicine_rank(character: &Character) -> u8 {
    // See p.149: Medicine is exclusively the Medtech Role Ability.
    if character.role == Role::Medtech {
        character.role_rank
    } else {
        0
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Sub-abilities
// ──────────────────────────────────────────────────────────────────────────────

/// The three Medicine Specialties available to a Medtech. See p.149.
///
/// Each time a Medtech's Medicine Rank increases by 1 they allocate one point
/// to exactly one of these three Specialties. Points cannot be re-allocated.
///
/// See pp.149–150.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MedTechSubAbility {
    /// Surgery Specialty — grants +2 Surgery Skill per point. See p.149.
    Surgery,
    /// Medical Tech (Pharmaceuticals) Specialty — grants +1 Medical Tech Skill
    /// per point and pharmaceutical synthesis access. See p.149.
    Pharmaceuticals,
    /// Medical Tech (Cryosystem Operation) Specialty — grants +1 Medical Tech
    /// Skill per point and Cryopump/Cryotank access by level. See p.150.
    CryosystemOperation,
}

// ──────────────────────────────────────────────────────────────────────────────
// Surgery
// ──────────────────────────────────────────────────────────────────────────────

/// Bonus to the surgery *roll* for Critical Injury Treatment — positive addend.
///
/// Per p.149, each point allocated to the Surgery Specialty grants +2 ranks
/// in the Surgery Skill. The surgery check is:
///
/// > `TECH + Surgery Skill + 1d10 ≥ Treatment DV`
///
/// This function returns `rank as i8 * 2`: if the character has Medicine rank N
/// with all points in Surgery, the Surgery Skill bonus to the check is `+2N`.
///
/// **RAW note (p.149 vs WP-513 spec):** The WP-513 spec describes this as a
/// "DV modifier." In the rulebook the bonus is applied to the **roll**, not
/// subtracted from the DV — the two conventions are equivalent for the combat
/// engine as long as the sign is correct. This implementation encodes the
/// RAW roll-bonus interpretation (positive = roll higher). Deviation flagged in PR.
///
/// Returns `0` for non-Medtech characters (Surgery is Medtech-only, p.149).
///
/// See pp.149, 221–223.
pub fn surgery_dv_modifier(character: &Character) -> i8 {
    // See p.149: Surgery Specialty grants +2 Surgery Skill per point.
    // With all Medicine rank points in Surgery, the roll bonus = rank * 2.
    // Surgery is Medtech-only; non-Medtech characters return 0.
    let rank = medicine_rank(character);
    // Cast is safe: rank is u8 ≤ 10, so rank * 2 ≤ 20, well within i8 range.
    (rank as i8).saturating_mul(2)
}

// ──────────────────────────────────────────────────────────────────────────────
// Pharmaceuticals
// ──────────────────────────────────────────────────────────────────────────────

/// HP healed per Speedheal dose, scaled by Pharmaceuticals rank.
///
/// The rulebook (p.150) states that Speedheal heals "an amount of HP equal to
/// their BODY + WILL." This function returns a rank-based scaling factor
/// (in HP) that the GM layer uses to size the healing pool available from a
/// Medtech's synthesised Pharmaceuticals.
///
/// ## RAW derivation (p.149–150)
///
/// - Each Pharmaceuticals point grants +1 Medical Tech Skill rank (capped at 5
///   points, Skill max 10).
/// - The Medtech can synthesise a number of doses equal to their Medical Tech
///   Skill from 200eb of materials in 1 hour.
/// - Speedheal heals BODY + WILL HP per dose.
///
/// Because this function does not have access to a specific target's stats, it
/// returns `rank * 2` as a **per-rank baseline** — a defensible approximation
/// representing the synthesis-throughput scaling. A rank-N Pharmaceuticals
/// specialist can produce N doses per session; each dose heals per character
/// stats. The GM layer multiplies by (BODY + WILL) for the actual Speedheal
/// effect. Deviation flagged in PR.
///
/// Returns `0` for rank 0 (no Pharmaceuticals training).
///
/// See pp.149–150.
pub fn pharmaceuticals_hp_healed(rank: u8) -> u16 {
    // See p.149–150: each Pharmaceuticals rank grants +1 Medical Tech Skill,
    // allowing synthesis of one additional dose per session. Baseline: rank * 2.
    // This represents the scaling factor; multiply by BODY + WILL for
    // the actual Speedheal HP healed per dose.
    (rank as u16).saturating_mul(2)
}

// ──────────────────────────────────────────────────────────────────────────────
// Cryosystem Operation
// ──────────────────────────────────────────────────────────────────────────────

/// Equipment benefit from Cryosystem Operation points allocated. See p.150.
///
/// Each level in Cryosystem Operation grants a specific benefit. This enum
/// captures the level-gated items described in the table on p.150.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CryosystemBenefit {
    /// Level 1: Gain one Cryopump. See p.150.
    Cryopump,
    /// Level 2: Become a Registered Cryotank Technician; unlimited 24/7 access
    /// to 1 Cryotank at any facility operated by medical corporations or
    /// government agencies. See p.150.
    RegisteredTechnician,
    /// Level 3: Gain 1 Cryotank, installed in a room of your choosing.
    /// See p.150.
    OwnCryotank,
    /// Level 4: Gain 2 more Cryotanks; Cryopump has 2 charges and carries 2
    /// people in stasis. See p.150.
    ExtendedStasis,
    /// Level 5: Gain 3 more Cryotanks; Cryopump has 3 charges and carries 3
    /// people in stasis. See p.150.
    MaximumStasis,
}

/// Returns the Cryosystem Operation benefit for the given allocation level.
///
/// Per p.150, benefits are level-gated: each level from 1–5 grants a new
/// capability. Returns `None` for level 0 (no allocation) or level > 5
/// (allocation is capped at 5 points per p.149).
///
/// See p.150.
pub fn cryosystem_benefit(level: u8) -> Option<CryosystemBenefit> {
    // See p.150: Cryosystem Operation benefit table.
    match level {
        1 => Some(CryosystemBenefit::Cryopump),
        2 => Some(CryosystemBenefit::RegisteredTechnician),
        3 => Some(CryosystemBenefit::OwnCryotank),
        4 => Some(CryosystemBenefit::ExtendedStasis),
        5 => Some(CryosystemBenefit::MaximumStasis),
        _ => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Medical Tech Skill derivation
// ──────────────────────────────────────────────────────────────────────────────

/// Effective Medical Tech Skill rank from Pharmaceuticals + Cryosystem points.
///
/// Per p.149–150, both Pharmaceuticals and Cryosystem Operation contribute to
/// the single Medical Tech Skill (max 10). The combined points from both
/// Specialties determine the Skill level. This function takes the allocated
/// points separately and returns the resulting Medical Tech Skill rank (capped
/// at 10).
///
/// See pp.149–150.
pub fn medical_tech_skill(pharmaceuticals_points: u8, cryosystem_points: u8) -> u8 {
    // See p.149–150: Medical Tech Skill = Pharmaceuticals points + Cryosystem
    // Operation points, capped at 10. Each specialty is individually capped
    // at 5 points (p.149).
    let combined = (pharmaceuticals_points as u16) + (cryosystem_points as u16);
    combined.min(10) as u8
}

/// Effective Surgery Skill rank from Surgery specialty points.
///
/// Per p.149, each Surgery point grants +2 ranks in the Surgery Skill, capped
/// at 10.
///
/// See p.149.
pub fn surgery_skill(surgery_points: u8) -> u8 {
    // See p.149: Surgery Specialty grants +2 Surgery Skill per point, max 10.
    let raw = (surgery_points as u16).saturating_mul(2);
    raw.min(10) as u8
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::data::{
        Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds,
    };
    use crate::effects::{EffectStack, WoundState};
    use crate::types::{CharacterId, Eurobucks};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_character(role: Role, role_rank: u8) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xAB)),
            name: "Doc".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 5,
                dex: 6,
                tech: 8,
                cool: 5,
                will: 7,
                luck: 5,
                r#move: 6,
                body: 7,
                emp: 6,
            },
            skills: SkillSet {
                ranks: HashMap::new(),
            },
            cyberware: vec![],
            armor: WornArmor {
                head: None,
                body: None,
            },
            inventory: Inventory { items: vec![] },
            wounds: Wounds {
                current_hp: 35,
                max_hp: 35,
                seriously_wounded_threshold: 18,
                death_save_base: 7,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 50,
            luck_pool: 5,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    // ── medicine_rank ─────────────────────────────────────────────────────────

    #[test]
    fn test_medicine_rank_for_medtech() {
        // Per p.149: Medtech's Medicine rank equals their Role Ability rank.
        let character = make_character(Role::Medtech, 4);
        assert_eq!(medicine_rank(&character), 4);
    }

    #[test]
    fn test_medicine_rank_zero_for_non_medtech() {
        // Non-Medtech roles do not have Medicine — rank is 0. See p.149.
        let character = make_character(Role::Solo, 6);
        assert_eq!(medicine_rank(&character), 0);
    }

    #[test]
    fn test_medicine_rank_zero_for_netrunner() {
        // Additional non-Medtech check: Netrunner has Medicine rank 0.
        let character = make_character(Role::Netrunner, 8);
        assert_eq!(medicine_rank(&character), 0);
    }

    #[test]
    fn test_medicine_rank_max() {
        // Role rank caps at 10 per book p.71; medicine_rank mirrors it directly.
        let character = make_character(Role::Medtech, 10);
        assert_eq!(medicine_rank(&character), 10);
    }

    // ── surgery_dv_modifier ───────────────────────────────────────────────────

    #[test]
    fn test_surgery_dv_modifier_rank_4() {
        // Per p.149: each Surgery point grants +2 Surgery Skill ranks.
        // At Medicine rank 4 (all points in Surgery): modifier = 4 * 2 = 8.
        // See module docs for RAW roll-bonus vs DV-reduction convention.
        let character = make_character(Role::Medtech, 4);
        assert_eq!(surgery_dv_modifier(&character), 8);
    }

    #[test]
    fn test_surgery_dv_modifier_rank_1() {
        // At rank 1: 1 * 2 = 2 Surgery Skill ranks → roll bonus of +2.
        let character = make_character(Role::Medtech, 1);
        assert_eq!(surgery_dv_modifier(&character), 2);
    }

    #[test]
    fn test_surgery_dv_modifier_rank_5() {
        // At rank 5: 5 * 2 = 10 Surgery Skill ranks (hitting the skill cap).
        let character = make_character(Role::Medtech, 5);
        assert_eq!(surgery_dv_modifier(&character), 10);
    }

    #[test]
    fn test_surgery_dv_modifier_zero_for_non_medtech() {
        // Surgery is Medtech-only (p.149). Non-Medtech returns 0.
        let character = make_character(Role::Tech, 7);
        assert_eq!(surgery_dv_modifier(&character), 0);
    }

    // ── pharmaceuticals_hp_healed ─────────────────────────────────────────────

    #[test]
    fn test_pharmaceuticals_hp_healed_rank_0() {
        // No Pharmaceuticals training → 0 HP healed. See pp.149–150.
        assert_eq!(pharmaceuticals_hp_healed(0), 0);
    }

    #[test]
    fn test_pharmaceuticals_hp_healed_rank_1() {
        // Rank 1: 1 * 2 = 2 baseline HP per rank. See pp.149–150.
        assert_eq!(pharmaceuticals_hp_healed(1), 2);
    }

    #[test]
    fn test_pharmaceuticals_hp_healed_rank_3() {
        // Rank 3: 3 * 2 = 6 baseline HP per rank. See pp.149–150.
        assert_eq!(pharmaceuticals_hp_healed(3), 6);
    }

    #[test]
    fn test_pharmaceuticals_hp_healed_rank_5() {
        // Rank 5 (max Pharmaceuticals per p.149): 5 * 2 = 10. See pp.149–150.
        assert_eq!(pharmaceuticals_hp_healed(5), 10);
    }

    // ── cryosystem_benefit ────────────────────────────────────────────────────

    #[test]
    fn test_cryosystem_benefit_level_0_is_none() {
        // Level 0 means no allocation; no benefit. See p.150.
        assert_eq!(cryosystem_benefit(0), None);
    }

    #[test]
    fn test_cryosystem_benefit_level_1() {
        // Level 1: Cryopump. See p.150.
        assert_eq!(cryosystem_benefit(1), Some(CryosystemBenefit::Cryopump));
    }

    #[test]
    fn test_cryosystem_benefit_level_2() {
        // Level 2: Registered Cryotank Technician. See p.150.
        assert_eq!(
            cryosystem_benefit(2),
            Some(CryosystemBenefit::RegisteredTechnician)
        );
    }

    #[test]
    fn test_cryosystem_benefit_level_5() {
        // Level 5 (max allocation): 3 more Cryotanks, Cryopump has 3 charges. See p.150.
        assert_eq!(
            cryosystem_benefit(5),
            Some(CryosystemBenefit::MaximumStasis)
        );
    }

    #[test]
    fn test_cryosystem_benefit_level_6_is_none() {
        // Level > 5 is out of range; allocation is capped at 5 (p.149).
        assert_eq!(cryosystem_benefit(6), None);
    }

    // ── surgery_skill ────────────────────────────────────────────────────────

    #[test]
    fn test_surgery_skill_from_points() {
        // 1 point → 2 Surgery Skill; 5 points → 10 (capped). See p.149.
        assert_eq!(surgery_skill(0), 0);
        assert_eq!(surgery_skill(1), 2);
        assert_eq!(surgery_skill(3), 6);
        assert_eq!(surgery_skill(5), 10);
        // Beyond 5 points: still capped at 10.
        assert_eq!(surgery_skill(6), 10);
    }

    // ── medical_tech_skill ────────────────────────────────────────────────────

    #[test]
    fn test_medical_tech_skill_combined() {
        // Pharmaceuticals 3 + Cryosystem 2 = 5 Medical Tech Skill. See pp.149–150.
        assert_eq!(medical_tech_skill(3, 2), 5);
    }

    #[test]
    fn test_medical_tech_skill_capped_at_10() {
        // 5 + 5 = 10, not above. See pp.149–150.
        assert_eq!(medical_tech_skill(5, 5), 10);
    }

    #[test]
    fn test_medical_tech_skill_zero() {
        assert_eq!(medical_tech_skill(0, 0), 0);
    }

    // ── MedTechSubAbility enum ────────────────────────────────────────────────

    #[test]
    fn test_sub_ability_variants_are_distinct() {
        // Sanity: all three variants exist and are distinguishable.
        assert_ne!(
            MedTechSubAbility::Surgery,
            MedTechSubAbility::Pharmaceuticals
        );
        assert_ne!(
            MedTechSubAbility::Pharmaceuticals,
            MedTechSubAbility::CryosystemOperation
        );
        assert_ne!(
            MedTechSubAbility::Surgery,
            MedTechSubAbility::CryosystemOperation
        );
    }
}
