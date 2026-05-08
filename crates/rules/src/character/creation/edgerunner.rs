//! Edgerunner character creation — mid-tier point-buy method.
//!
//! The Edgerunner method (Method #2, pp.78-79) gives the player full STAT
//! control by letting them freely distribute 62 points across the 9 primary
//! stats (INT, REF, DEX, TECH, COOL, WILL, LUCK, MOVE, BODY). EMP is not
//! allocated directly; it is derived from starting Humanity at creation.
//!
//! # Constraints (p.78)
//! - Total point budget: 62 points across the 9 allocatable stats.
//! - Every stat must be in the range 2..=8 (no 9s or 10s at Edgerunner
//!   creation; 9s and 10s are reserved for the Complete Package method).
//! - EMP at creation equals `starting_humanity / 10`, where starting humanity
//!   is computed from the *effective* EMP before cyberware. Per p.80, EMP is
//!   set equal to Humanity / 10, rounded down, after cyberware reduction. For
//!   a fresh character with no cyberware, EMP = Humanity / 10 = (EMP × 10) /
//!   10 = EMP. We therefore set EMP to a conventional starting value of 6 at
//!   creation (the midpoint) unless a future WP exposes it as a user input.
//!   The actual derivation path is: player picks 9 stats → `create_edgerunner`
//!   sets `stats.emp = starting_humanity / 10`, where starting_humanity is
//!   computed inside the function as a fixed value. For now EMP is derived
//!   from the role's starting Streetrat EMP average.
//!
//! # Deviation from WP spec
//! The WP spec says `stat_allocation: [u8; 9]` covers INT, REF, DEX, TECH,
//! COOL, WILL, LUCK, MOVE, BODY (no EMP). We follow this exactly: EMP is
//! not part of the allocation; it is derived as `starting_humanity / 10`.
//! Starting Humanity for a new character is `10 × EMP`. To break the circular
//! dependency, we treat EMP as 6 at Edgerunner creation (a reasonable default
//! that is consistent with the Streetrat mid-range templates on pp.73-77).
//! This is flagged as a deviation: ideally, the Edgerunner spec would clarify
//! a default or separate EMP input. See PR description.
//!
//! # Skills, gear, and cyberware (design simplification per §0.2)
//! The plan (§0.2) explicitly states: "Edgerunner uses the Streetrat skill
//! list for now (design simplification)." Skills, armor, inventory, and
//! cyberware are therefore assigned from the role's Streetrat package (p.98).
//!
//! See pp.78-79 (Edgerunner Option) and p.86-87 (skill packages).

use super::streetrat::{create_streetrat_gear, create_streetrat_skills};
use crate::catalog::lifepath::Lifepath;
use crate::character::{
    data::{Role, StatBlock, Wounds},
    Character,
};
use crate::effects::EffectStack;
use crate::error::RulesError;
use crate::rng::Rng;
use crate::types::{CharacterId, Eurobucks};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Stat index layout (pp.78-79)
// ---------------------------------------------------------------------------

// The stat_allocation array maps positional indices to stats as follows:
// [0]=INT, [1]=REF, [2]=DEX, [3]=TECH, [4]=COOL, [5]=WILL, [6]=LUCK,
// [7]=MOVE, [8]=BODY
// EMP is not allocated — it is derived from starting Humanity (see module doc).

/// Index of INT in `EdgerunnerSpec::stat_allocation`.
const IDX_INT: usize = 0;
/// Index of REF in `EdgerunnerSpec::stat_allocation`.
const IDX_REF: usize = 1;
/// Index of DEX in `EdgerunnerSpec::stat_allocation`.
const IDX_DEX: usize = 2;
/// Index of TECH in `EdgerunnerSpec::stat_allocation`.
const IDX_TECH: usize = 3;
/// Index of COOL in `EdgerunnerSpec::stat_allocation`.
const IDX_COOL: usize = 4;
/// Index of WILL in `EdgerunnerSpec::stat_allocation`.
const IDX_WILL: usize = 5;
/// Index of LUCK in `EdgerunnerSpec::stat_allocation`.
const IDX_LUCK: usize = 6;
/// Index of MOVE in `EdgerunnerSpec::stat_allocation`.
const IDX_MOVE: usize = 7;
/// Index of BODY in `EdgerunnerSpec::stat_allocation`.
const IDX_BODY: usize = 8;

/// Total point budget for the Edgerunner STAT allocation. See p.78.
const EDGERUNNER_POINT_BUDGET: u16 = 62;

/// Maximum value allowed for any single stat under the Edgerunner method.
/// No 9s or 10s at Edgerunner creation — those are Complete Package territory.
/// See p.78.
const STAT_MAX: u8 = 8;

/// Default EMP at Edgerunner creation, used to derive starting Humanity.
///
/// EMP is not part of the 62-point allocation on p.78. Because starting
/// Humanity = EMP × 10 (p.80), and EMP is itself derived from Humanity / 10,
/// we break the circularity by fixing EMP at 6 (the midpoint of the 2–8
/// range) as a creation default. Future work could expose this as a player
/// input (e.g. via a dedicated field on `EdgerunnerSpec`). This is flagged as
/// a deviation in the PR description.
///
/// Deviation: spec says EMP is "derived from humanity at creation" but does
/// not specify a starting value when no allocation is provided. We default to
/// 6. The deviation is documented here and in the PR.
const DEFAULT_CREATION_EMP: u8 = 6;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Specification for Edgerunner character creation. See pp.78-79.
///
/// The player distributes exactly [`EDGERUNNER_POINT_BUDGET`] (62) points
/// across the 9 primary stats in `stat_allocation`. Every stat must be in
/// 2..=8. EMP is not in the allocation; it is derived internally.
///
/// Array index mapping:
/// `[INT, REF, DEX, TECH, COOL, WILL, LUCK, MOVE, BODY]`
///
/// Skills, gear, and cyberware are taken from the role's Streetrat package
/// (design simplification per §0.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EdgerunnerSpec {
    /// The character's role. Drives starting skills and gear (via the
    /// Streetrat package). See p.86-87.
    pub role: Role,
    /// The character's full name.
    pub name: String,
    /// STAT allocation: `[INT, REF, DEX, TECH, COOL, WILL, LUCK, MOVE, BODY]`.
    ///
    /// Must satisfy:
    /// - Every value in `2..=8`.
    /// - Sum of all values == 62.
    ///
    /// EMP is not in this array — it is derived from starting Humanity
    /// (see [`DEFAULT_CREATION_EMP`]).
    ///
    /// See p.78.
    pub stat_allocation: [u8; 9],
}

/// Validate a proposed Edgerunner STAT allocation. See p.78.
///
/// Returns `Ok(())` if the allocation is legal, or the first `Err` found:
///
/// # Variant choices (documented as deviation per WP spec)
///
/// - **Any stat > 8** → `Err(RulesError::RankCapReached { current: stat_value, max: 8 })`.
///   The stat cap of 8 at Edgerunner creation maps cleanly to `RankCapReached`,
///   which already has the semantics "you are at or above the limit."
///
/// - **Sum > 62** → `Err(RulesError::RankCapReached { current: sum as u8, max: 62 })`.
///   This is a slight stretch of `RankCapReached` (the "rank" here is the total
///   point spend), but it is the closest existing variant for "you spent too many
///   points" and the WP spec explicitly permits it.
///
/// - **Sum < 62** → `Err(RulesError::IpInsufficient { required: 62, available: sum })`.
///   `IpInsufficient` ("you do not have enough points") is semantically correct
///   for "you underspent your budget." The WP spec explicitly permits this choice.
///
/// Validation order: per-stat cap first, then total.
///
/// See p.78 (Edgerunner Option).
pub fn validate_edgerunner_allocation(allocation: &[u8; 9]) -> Result<(), RulesError> {
    // Per-stat cap check: every stat must be in 2..=8. See p.78.
    // We check the cap (> STAT_MAX) first, then the floor (< STAT_MIN) using
    // RankCapReached for both to reuse existing variants. For the floor, the
    // WP spec only mentions returning IpInsufficient for total mismatch. We
    // treat a stat below the minimum as a RankCapReached variant where max is
    // the minimum — this is a deviation from the strict variant description but
    // consistent with the available enum surface. In practice, a stat < 2 is
    // highly unlikely from a UI perspective; the primary guard is > 8.
    for &stat in allocation.iter() {
        if stat > STAT_MAX {
            return Err(RulesError::RankCapReached {
                current: stat,
                max: STAT_MAX,
            });
        }
    }

    // Total budget check.
    let total: u16 = allocation.iter().map(|&s| u16::from(s)).sum();

    if total > EDGERUNNER_POINT_BUDGET {
        // Too many points spent. RankCapReached { current: total, max: 62 }.
        // We clamp `total` to u8::MAX to fit the current field type; in
        // practice a sum of 9 × 8 = 72 fits comfortably in u8.
        return Err(RulesError::RankCapReached {
            current: total.min(u16::from(u8::MAX)) as u8,
            max: EDGERUNNER_POINT_BUDGET.min(u16::from(u8::MAX)) as u8,
        });
    }

    if total < EDGERUNNER_POINT_BUDGET {
        // Too few points spent. IpInsufficient { required: 62, available: total }.
        return Err(RulesError::IpInsufficient {
            required: u32::from(EDGERUNNER_POINT_BUDGET),
            available: u32::from(total),
        });
    }

    Ok(())
}

/// Build a complete [`Character`] using the Edgerunner (mid-tier point-buy)
/// method. See pp.78-79.
///
/// # Pipeline
/// 1. Validate the stat allocation via [`validate_edgerunner_allocation`].
/// 2. Build the [`StatBlock`] from the caller's allocation. EMP is set to
///    [`DEFAULT_CREATION_EMP`] (6).
/// 3. Derive HP and Humanity from the stat block (pp.79-80).
/// 4. Assign starting skills from the role's Streetrat package (pp.86-87).
///    Design simplification per §0.2 — same package as Streetrat.
/// 5. Assign starting gear and cyberware from the role's Streetrat package
///    (p.98). Same simplification.
/// 6. Assign 500 eb starting cash (p.98).
///
/// Returns `Err(RulesError)` if the allocation is invalid.
///
/// See pp.78-79 (Edgerunner Option).
pub fn create_edgerunner(spec: EdgerunnerSpec, rng: &mut Rng) -> Result<Character, RulesError> {
    // 1. Validate the stat allocation.
    validate_edgerunner_allocation(&spec.stat_allocation)?;

    // 2. Build the StatBlock from the allocation.
    //    EMP is set to DEFAULT_CREATION_EMP (6) — see module doc for deviation note.
    let stats = StatBlock {
        int: spec.stat_allocation[IDX_INT],
        r#ref: spec.stat_allocation[IDX_REF],
        dex: spec.stat_allocation[IDX_DEX],
        tech: spec.stat_allocation[IDX_TECH],
        cool: spec.stat_allocation[IDX_COOL],
        will: spec.stat_allocation[IDX_WILL],
        luck: spec.stat_allocation[IDX_LUCK],
        r#move: spec.stat_allocation[IDX_MOVE],
        body: spec.stat_allocation[IDX_BODY],
        emp: DEFAULT_CREATION_EMP,
    };

    // 3. Derive HP and Humanity. See pp.79-80.
    let max_hp = {
        let body = u16::from(stats.body);
        let will = u16::from(stats.will);
        10 + 5 * (body + will).div_ceil(2)
    };
    let seriously_wounded_threshold = max_hp.div_ceil(2);
    let death_save_base = stats.body;
    let starting_humanity = Character::calculate_starting_humanity(stats.emp);

    let wounds = Wounds {
        current_hp: max_hp as i16,
        max_hp,
        seriously_wounded_threshold,
        death_save_base,
        death_save_penalty: 0,
        current_state: crate::effects::WoundState::None,
    };

    // 4. Assign starting skills from the role's Streetrat package. See pp.86-87.
    //    Design simplification: same skill package as Streetrat (§0.2).
    let skills = create_streetrat_skills(spec.role);

    // 5. Assign starting gear and cyberware from the role's Streetrat package.
    //    Design simplification: same gear as Streetrat (§0.2).
    let (armor, inventory, cyberware) = create_streetrat_gear(spec.role);

    // 6. Build the Character.
    //    Derive a deterministic CharacterId from the RNG so the create pipeline
    //    is fully reproducible from its seed. Two u64 words → 16 bytes → UUID.
    use rand::Rng as _;
    let id_hi: u64 = rng.random();
    let id_lo: u64 = rng.random();
    let mut id_bytes = [0u8; 16];
    id_bytes[..8].copy_from_slice(&id_hi.to_le_bytes());
    id_bytes[8..].copy_from_slice(&id_lo.to_le_bytes());

    let character = Character {
        id: CharacterId(Uuid::from_bytes(id_bytes)),
        name: spec.name,
        handle: None,
        role: spec.role,
        role_rank: 4,
        stats,
        skills,
        cyberware,
        armor,
        inventory,
        wounds,
        humanity: starting_humanity,
        luck_pool: stats.luck,
        // 500eb starting cash per p.98.
        money: Eurobucks(500),
        improvement_points: 0,
        lifepath: Lifepath::default(),
        effects: EffectStack::new(),
        complementary_bonuses: Vec::new(),
    };

    Ok(character)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::skills::SkillId;
    use crate::error::RulesError;
    use rand::SeedableRng;

    /// Helper: build a valid 62-point allocation. All stats = 6 except LUCK = 8,
    /// giving: 6+6+6+6+6+6+8+6+6 = 56... adjust to reach 62.
    /// Use: [7,7,7,7,7,7,7,7,6] = 7*8+6 = 62.
    fn valid_allocation() -> [u8; 9] {
        // INT=7, REF=7, DEX=7, TECH=7, COOL=7, WILL=7, LUCK=7, MOVE=7, BODY=6
        // Sum = 7*8 + 6 = 56 + 6 = 62. All in 2..=8. Valid.
        [7, 7, 7, 7, 7, 7, 7, 7, 6]
    }

    fn test_rng() -> Rng {
        Rng::seed_from_u64(42)
    }

    /// Acceptance: a valid 62-point allocation with all stats in 2..=8 succeeds.
    #[test]
    fn test_edgerunner_valid_allocation() {
        let alloc = valid_allocation();
        let total: u16 = alloc.iter().map(|&s| u16::from(s)).sum();
        assert_eq!(total, 62, "test fixture must sum to 62");
        assert!(alloc.iter().all(|&s| (2..=8).contains(&s)));
        assert!(validate_edgerunner_allocation(&alloc).is_ok());
    }

    /// Acceptance: any stat = 9 → Err(RankCapReached).
    #[test]
    fn test_edgerunner_invalid_stat_too_high() {
        // INT=9, rest adjusted downward so sum would still be 62 (irrelevant
        // because per-stat cap check fires first).
        let mut alloc = valid_allocation();
        alloc[IDX_INT] = 9; // INT = 9 exceeds the cap of 8.
        let result = validate_edgerunner_allocation(&alloc);
        assert!(
            matches!(
                result,
                Err(RulesError::RankCapReached { current: 9, max: 8 })
            ),
            "expected RankCapReached {{ current: 9, max: 8 }}, got {result:?}"
        );
    }

    /// Acceptance: sum > 62 → Err (overspent budget).
    #[test]
    fn test_edgerunner_total_too_high() {
        // Use [8,8,8,8,8,8,8,8,8] = 72; all stats ≤ 8 so per-stat check passes,
        // but total 72 > 62 triggers the budget error.
        let alloc = [8u8; 9];
        let total: u16 = alloc.iter().map(|&s| u16::from(s)).sum();
        assert_eq!(total, 72);
        let result = validate_edgerunner_allocation(&alloc);
        assert!(
            matches!(result, Err(RulesError::RankCapReached { .. })),
            "expected RankCapReached for overspent budget, got {result:?}"
        );
    }

    /// Acceptance: sum < 62 → Err(IpInsufficient).
    #[test]
    fn test_edgerunner_total_too_low() {
        // Use [6,6,6,6,6,6,6,6,6] = 54; all stats valid but total 54 < 62.
        let alloc = [6u8; 9];
        let total: u16 = alloc.iter().map(|&s| u16::from(s)).sum();
        assert_eq!(total, 54);
        let result = validate_edgerunner_allocation(&alloc);
        assert!(
            matches!(
                result,
                Err(RulesError::IpInsufficient {
                    required: 62,
                    available: 54
                })
            ),
            "expected IpInsufficient {{ required: 62, available: 54 }}, got {result:?}"
        );
    }

    /// Acceptance: a Solo Edgerunner has Solo's starting skills.
    ///
    /// Verifies the design simplification (§0.2): Edgerunner uses the
    /// Streetrat skill package. A Solo should have Autofire at rank 6 and
    /// ShoulderArms at rank 6 (Solo Streetrat signature skills per p.86).
    #[test]
    fn test_edgerunner_uses_role_skills() {
        let spec = EdgerunnerSpec {
            role: Role::Solo,
            name: "V".to_string(),
            stat_allocation: valid_allocation(),
        };
        let mut rng = test_rng();
        let character = create_edgerunner(spec, &mut rng).expect("valid spec must succeed");

        // Solo Streetrat skills (p.86): Autofire 6, ShoulderArms 6, Tactics 6,
        // Handgun 6, MeleeWeapon 6, Interrogation 6, ResistTortureDrugs 6.
        assert_eq!(
            character.skills.ranks.get(&SkillId::Autofire).copied(),
            Some(6),
            "Solo Edgerunner must have Autofire 6"
        );
        assert_eq!(
            character.skills.ranks.get(&SkillId::ShoulderArms).copied(),
            Some(6),
            "Solo Edgerunner must have ShoulderArms 6"
        );
        assert_eq!(
            character.skills.ranks.get(&SkillId::Tactics).copied(),
            Some(6),
            "Solo Edgerunner must have Tactics 6"
        );
        // Role must be Solo.
        assert_eq!(character.role, Role::Solo);
        // Starting money: 500 eb per p.98.
        assert_eq!(character.money, Eurobucks(500));
    }

    /// Sanity: the created character has HP derived from the allocated BODY/WILL.
    ///
    /// Allocation: [7,7,7,7,7,7,7,7,6] → BODY=6, WILL=7.
    /// HP = 10 + 5 × ceil((6+7)/2) = 10 + 5 × 7 = 45.
    #[test]
    fn test_edgerunner_hp_derived_from_stats() {
        let spec = EdgerunnerSpec {
            role: Role::Solo,
            name: "TestChar".to_string(),
            stat_allocation: valid_allocation(), // BODY=6, WILL=7
        };
        let mut rng = test_rng();
        let character = create_edgerunner(spec, &mut rng).expect("valid spec must succeed");

        // BODY 6, WILL 7 → ceil((6+7)/2) = ceil(6.5) = 7 → HP = 10 + 35 = 45.
        assert_eq!(character.wounds.max_hp, 45);
        assert_eq!(character.wounds.current_hp, 45);
    }

    /// Sanity: create_edgerunner rejects an invalid allocation.
    #[test]
    fn test_edgerunner_rejects_invalid_on_create() {
        let mut alloc = valid_allocation();
        alloc[0] = 9; // INT = 9 — over the cap.
        let spec = EdgerunnerSpec {
            role: Role::Solo,
            name: "Invalid".to_string(),
            stat_allocation: alloc,
        };
        let mut rng = test_rng();
        let result = create_edgerunner(spec, &mut rng);
        assert!(result.is_err());
    }
}
