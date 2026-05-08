//! Complete Package character creation — Method #3: full point-buy.
//!
//! The Complete Package (p.78) gives the player total control: a 62-point
//! STAT budget, an 86-point Skill budget, a 2,550eb gear allowance, and a
//! player-chosen cyberware loadout. Validation is the core concern of this
//! module.
//!
//! # Pipeline
//!
//! 1. [`validate_complete_spec`] — validate all constraints up front.
//! 2. [`create_complete_package`] — build a [`Character`] from a valid
//!    [`CompleteSpec`], installing cyberware with Humanity Loss.
//!
//! Rulebook references: pp.78–79 (STAT budget), pp.85–90 (Skill rules),
//! pp.104–105 (gear and money), p.80 (Humanity / EMP derivation).

use crate::catalog::cyberware::Cyberware;
use crate::catalog::Catalog;
use crate::character::{
    cyberware::install_cyberware,
    data::{Role, SkillSet, StatBlock},
    Character, Inventory, Lifepath, WornArmor, Wounds,
};
use crate::effects::{CyberwareId, EffectStack, SkillId, WoundState};
use crate::error::RulesError;
use crate::rng::Rng;
use crate::types::{CharacterId, Eurobucks};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public spec struct
// ---------------------------------------------------------------------------

/// Full specification for creating a Complete Package character. See pp.78–79.
///
/// All fields are validated by [`validate_complete_spec`] before the character
/// is built. The caller fills these in; no dice are rolled for STATs or Skills
/// — those are player choices.
///
/// See pp.78–79 (STAT budget, stat caps, skill budget, skill caps).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompleteSpec {
    /// The character's role. Drives role rank and `role_rank` default. See p.71.
    pub role: Role,
    /// Real-world name on file.
    pub name: String,
    /// Player-chosen STAT block. Each individual STAT must be in `2..=8`
    /// and the total must be ≤ 62 (the Starting Character budget per p.78).
    ///
    /// # Rulebook note
    ///
    /// p.78: "The only limit is that no STAT may be higher than 8 or lower
    /// than 2." The budget is normally 62 (Starting Character rank). The table
    /// on p.78 lists higher budgets (70/75/80) for non-starting ranks — this
    /// implementation validates the 62-point starting budget only, consistent
    /// with the design decision to start players at Starting Character rank.
    pub stats: StatBlock,
    /// Player-chosen skill picks. Each entry is `(SkillId, rank)` where rank
    /// must be in `2..=6` (Basic Skills start at 2; cap is 6 at creation per
    /// p.90). The total weighted cost (×2 skills cost 2 points per rank) must
    /// be ≤ 86. The 13 Basic Skills **must** be present at rank ≥ 2.
    ///
    /// Language based on Cultural Origin is granted free at rank 4 (p.85);
    /// include it in this list at rank 4 and it will cost 0 points.
    pub skill_picks: Vec<(SkillId, u8)>,
    /// Free Cultural-Origin language. Its 4 ranks cost 0 points (p.85).
    /// Set to `None` to omit (the player may fold it into `skill_picks`
    /// manually if preferred, paying 0 points for the first 4 ranks).
    pub cultural_language: Option<SkillId>,
    /// Cyberware to install at creation, in order. Each item is installed
    /// using the preset (fixed) Humanity Loss (p.111 — "at-creation HL").
    /// Requires a [`Catalog<Cyberware>`] to be passed to
    /// [`create_complete_package`]. See pp.104–105.
    pub starting_cyberware: Vec<CyberwareId>,
    /// Starting Eurobucks. Per p.104 the Complete Package gets 2,550eb.
    /// The caller may pass a smaller value if any was spent prior to
    /// construction (e.g. in a character-builder UI), but the amount must
    /// be ≤ 2,550.
    pub starting_money: u32,
}

// ---------------------------------------------------------------------------
// Validation constants (pp.78, 90)
// ---------------------------------------------------------------------------

/// 62-point STAT budget for a Starting Character (p.78).
pub const STAT_BUDGET: u32 = 62;

/// No individual STAT may exceed 8 at creation (p.78).
pub const STAT_MAX: u8 = 8;

/// No individual STAT may be lower than 2 at creation (p.78).
pub const STAT_MIN: u8 = 2;

/// 86-point Skill budget shared with Edgerunner (p.90).
pub const SKILL_BUDGET: u32 = 86;

/// Skills cap at 6 at character creation (p.90).
pub const SKILL_RANK_MAX_AT_CREATION: u8 = 6;

/// Minimum rank for Basic Skills — must be ≥ 2 (p.85, p.90).
pub const BASIC_SKILL_MIN: u8 = 2;

/// Complete Package starting money in Eurobucks (p.104).
pub const STARTING_MONEY_POOL: u32 = 2_550;

/// The 13 Basic Skills every character must have at rank ≥ 2. See p.85.
fn basic_skills() -> Vec<SkillId> {
    use crate::catalog::skills::{LanguageKind, LocalArea};
    vec![
        SkillId::Athletics,
        SkillId::Brawling,
        SkillId::Concentration,
        SkillId::Conversation,
        SkillId::Education,
        SkillId::Evasion,
        SkillId::FirstAid,
        SkillId::HumanPerception,
        SkillId::Language(LanguageKind::Streetslang),
        SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
        SkillId::Perception,
        SkillId::Persuasion,
        SkillId::Stealth,
    ]
}

/// The double-cost (×2) skills as listed on pp.82–84. Each rank of these
/// costs 2 Skill Points instead of 1.
fn is_double_cost(skill: &SkillId) -> bool {
    matches!(
        skill,
        SkillId::PilotAirVehicle
            | SkillId::MartialArts(_)
            | SkillId::Autofire
            | SkillId::HeavyWeapons
            | SkillId::DemolitionsTech
            | SkillId::ElectronicsSecurityTech
            | SkillId::Paramedic
    )
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Validate a [`CompleteSpec`] against the Complete Package rules (pp.78–90).
///
/// Returns `Ok(())` if all constraints pass, or the first constraint violated.
///
/// # Validated constraints
///
/// - STAT total ≤ 62, each STAT in 2..=8 (p.78).
/// - All 13 Basic Skills present at rank ≥ 2 (p.85).
/// - No skill rank > 6 at creation (p.90).
/// - Weighted skill cost total ≤ 86 (×2 skills cost 2 pts/rank; p.90).
/// - Starting money ≤ 2,550eb (p.104).
///
/// # Errors
///
/// - [`RulesError::RankCapReached`] — a STAT exceeds 8 (`max: 8`) or a Skill
///   rank exceeds 6 (`max: 6`).
/// - [`RulesError::IpInsufficient`] — STAT total > 62 or Skill cost > 86
///   (reused: `required` = actual cost, `available` = budget).
///
/// # Note on error variant re-use
///
/// The WP brief forbids adding new `RulesError` variants. `RankCapReached`
/// is used for both STAT-too-high and Skill-rank-too-high. `IpInsufficient`
/// (semantically an IP-spend error) is re-used for budget overruns because
/// it carries `{required, available: u32}` which maps cleanly to
/// `{actual_cost, budget}`. This deviation is flagged in the PR.
///
/// See pp.78–79, pp.85–90.
pub fn validate_complete_spec(spec: &CompleteSpec) -> Result<(), RulesError> {
    // --- 1. Validate each individual STAT --------------------------------
    // p.78: "no STAT may be higher than 8 or lower than 2".
    let stats = &spec.stats;
    let stat_values: [u8; 10] = [
        stats.int,
        stats.r#ref,
        stats.dex,
        stats.tech,
        stats.cool,
        stats.will,
        stats.luck,
        stats.r#move,
        stats.body,
        stats.emp,
    ];
    for &v in &stat_values {
        if v > STAT_MAX {
            return Err(RulesError::RankCapReached {
                current: v,
                max: STAT_MAX,
            });
        }
        // Minimum is enforced structurally (u8 >= 0), but values below 2
        // are out-of-spec. We emit RankCapReached with current/max swapped
        // to signal "too low" — flag in PR. Rulebook prohibits < 2.
        // In practice the UI should prevent this, but we guard here.
        if v < STAT_MIN {
            return Err(RulesError::RankCapReached {
                current: v,
                max: STAT_MIN, // "cap" here means minimum floor
            });
        }
    }

    // --- 2. Validate STAT budget ------------------------------------------
    // p.78: 62 points to distribute across 10 STATs.
    let stat_total: u32 = stat_values.iter().map(|&v| u32::from(v)).sum();
    if stat_total > STAT_BUDGET {
        return Err(RulesError::IpInsufficient {
            required: stat_total,
            available: STAT_BUDGET,
        });
    }

    // --- 3. Validate skill picks: no rank > 6 ----------------------------
    // p.90: "No Skill can be higher than 6."
    let picks: HashMap<SkillId, u8> = {
        let mut m: HashMap<SkillId, u8> = HashMap::new();
        for (skill, rank) in &spec.skill_picks {
            // Take the highest rank if the same skill appears twice.
            m.entry(skill.clone())
                .and_modify(|e: &mut u8| *e = (*e).max(*rank))
                .or_insert(*rank);
        }
        m
    };

    for (skill, &rank) in &picks {
        if rank > SKILL_RANK_MAX_AT_CREATION {
            return Err(RulesError::RankCapReached {
                current: rank,
                max: SKILL_RANK_MAX_AT_CREATION,
            });
        }
        // Enforce rank >= 1 for any listed skill (rank 0 is a no-op pick).
        if rank < 1 {
            return Err(RulesError::RankCapReached { current: 0, max: 1 });
        }
        let _ = skill; // used via the HashMap key
    }

    // --- 4. Validate Basic Skills present at ≥ 2 -------------------------
    // p.85, p.90: Basic Skills must be at least level 2.
    for basic in basic_skills() {
        let rank = picks.get(&basic).copied().unwrap_or(0);
        if rank < BASIC_SKILL_MIN {
            // Map "Basic Skill missing / below minimum" to RankCapReached
            // with current=rank and max=BASIC_SKILL_MIN (meaning "must be
            // at least this high"). Flagged as a deviation in the PR.
            return Err(RulesError::RankCapReached {
                current: rank,
                max: BASIC_SKILL_MIN,
            });
        }
    }

    // --- 5. Validate skill budget ≤ 86 -----------------------------------
    // p.90: 86 Skill points. ×2 skills cost 2 points per rank, others cost 1.
    // The cultural-origin language (rank 4) is free (p.85).
    let mut cost: u32 = 0;
    for (skill, &rank) in &picks {
        // Cultural language: first 4 ranks free.
        let free_ranks: u8 = if spec
            .cultural_language
            .as_ref()
            .map(|cl| cl == skill)
            .unwrap_or(false)
        {
            4
        } else {
            0
        };
        let paid_ranks: u8 = rank.saturating_sub(free_ranks);
        let rank_cost: u32 = if is_double_cost(skill) { 2 } else { 1 };
        cost = cost.saturating_add(u32::from(paid_ranks) * rank_cost);
    }

    if cost > SKILL_BUDGET {
        return Err(RulesError::IpInsufficient {
            required: cost,
            available: SKILL_BUDGET,
        });
    }

    // --- 6. Validate starting money --------------------------------------
    // p.104: Complete Package gets exactly 2,550eb. Spending before play
    // starts is allowed; the remainder must not exceed the pool.
    if spec.starting_money > STARTING_MONEY_POOL {
        return Err(RulesError::IpInsufficient {
            required: spec.starting_money,
            available: STARTING_MONEY_POOL,
        });
    }

    Ok(())
}

/// Create a character from a valid [`CompleteSpec`].
///
/// Calls [`validate_complete_spec`] first; returns its error immediately if
/// the spec is invalid. Then:
///
/// 1. Derives `max_hp`, `seriously_wounded_threshold`, `death_save_base`
///    from BODY and WILL (p.79).
/// 2. Derives starting Humanity from EMP × 10 (p.80).
/// 3. Builds the [`SkillSet`] from `spec.skill_picks` plus the cultural
///    language at rank 4 (p.85).
/// 4. Installs each piece of cyberware in `spec.starting_cyberware` using
///    the preset (at-creation) Humanity Loss (p.111). Returns the first
///    [`RulesError`] from installation if any piece fails.
/// 5. Returns the fully-built [`Character`].
///
/// The `rng` is used to generate a deterministic [`CharacterId`] (two u64
/// draws from the RNG, converted to UUID bytes). No dice are rolled for
/// STATs or Skills in Complete Package — those are player choices.
///
/// # Errors
///
/// - Any error returned by [`validate_complete_spec`].
/// - Any error returned by [`install_cyberware`] for a cyberware item.
///
/// See pp.78–79, pp.85–90, pp.104–105.
pub fn create_complete_package(
    spec: CompleteSpec,
    cyberware_catalog: &Catalog<Cyberware>,
    rng: &mut Rng,
) -> Result<Character, RulesError> {
    // --- Validate first ---------------------------------------------------
    validate_complete_spec(&spec)?;

    // --- 1. Derive HP and Humanity ----------------------------------------
    // pp.79–80.
    let max_hp = {
        let body = u16::from(spec.stats.body);
        let will = u16::from(spec.stats.will);
        10 + 5 * (body + will).div_ceil(2)
    };
    let seriously_wounded_threshold = max_hp.div_ceil(2);
    let death_save_base = spec.stats.body;
    let starting_humanity = Character::calculate_starting_humanity(spec.stats.emp);

    let wounds = Wounds {
        current_hp: max_hp as i16,
        max_hp,
        seriously_wounded_threshold,
        death_save_base,
        death_save_penalty: 0,
        current_state: WoundState::None,
    };

    // --- 2. Build the SkillSet -------------------------------------------
    // p.85, p.90: all Basic Skills at ≥ 2; cultural language at 4 (free).
    // We use the caller's picks verbatim (validation already checked limits).
    let mut ranks: HashMap<SkillId, u8> = HashMap::new();
    for (skill, rank) in &spec.skill_picks {
        ranks
            .entry(skill.clone())
            .and_modify(|e| *e = (*e).max(*rank))
            .or_insert(*rank);
    }
    // Ensure cultural language at rank 4 is present (free, p.85).
    if let Some(cl) = &spec.cultural_language {
        ranks
            .entry(cl.clone())
            .and_modify(|e| *e = (*e).max(4))
            .or_insert(4);
    }
    let skills = SkillSet { ranks };

    // --- 3. Generate a deterministic CharacterId from the RNG ------------
    use rand::Rng as _;
    let id_hi: u64 = rng.random();
    let id_lo: u64 = rng.random();
    let mut id_bytes = [0u8; 16];
    id_bytes[..8].copy_from_slice(&id_hi.to_le_bytes());
    id_bytes[8..].copy_from_slice(&id_lo.to_le_bytes());
    let char_id = CharacterId(Uuid::from_bytes(id_bytes));

    // --- 4. Build the base Character -------------------------------------
    // Cyberware will be installed in the next step. Start with no cyberware
    // so install_cyberware can validate prerequisites in order.
    let mut character = Character {
        id: char_id,
        name: spec.name,
        handle: None,
        role: spec.role,
        role_rank: 4, // Starting Character rank per p.78.
        stats: spec.stats,
        skills,
        cyberware: vec![],
        armor: WornArmor::default(),
        inventory: Inventory::default(),
        wounds,
        humanity: starting_humanity,
        luck_pool: spec.stats.luck,
        // `starting_money` may be ≤ 2,550 (the complete pool); remainder
        // represents unspent eb the character keeps (p.104).
        money: Eurobucks(spec.starting_money.into()),
        improvement_points: 0,
        lifepath: Lifepath::default(),
        effects: EffectStack::new(),
        complementary_bonuses: Vec::new(),
    };

    // --- 5. Install cyberware at creation --------------------------------
    // p.104–105: cyberware is purchased from the 2,550eb pool. At-creation
    // installs use the preset (fixed) Humanity Loss per p.111.
    for cw_id in &spec.starting_cyberware {
        install_cyberware(&mut character, cw_id.clone(), cyberware_catalog, rng, true)?;
    }

    Ok(character)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::cyberware::load_cyberware_catalog;
    use crate::catalog::skills::LanguageKind;
    use rand::SeedableRng;
    use std::path::PathBuf;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn catalog_dir() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules → crates
        p.pop(); // crates → repo root
        p.push("content");
        p.push("catalogs");
        p.push("cyberware");
        p
    }

    fn load_catalog() -> Catalog<Cyberware> {
        load_cyberware_catalog(&catalog_dir()).expect("cyberware catalog must load")
    }

    /// Minimal valid 62-point stat block for tests (p.78).
    /// 7+7+6+6+6+6+6+6+6+6 = 62. Each STAT is 2..=8.
    fn valid_stats() -> StatBlock {
        StatBlock {
            int: 7,
            r#ref: 7,
            dex: 6,
            tech: 6,
            cool: 6,
            will: 6,
            luck: 6,
            r#move: 6,
            body: 6,
            emp: 6,
        }
    }

    /// Build the 13 Basic Skills at level 2, plus a few extra to spend the
    /// 86 skill points (p.90). Total cost = 26 (13 × 2) + 60 (30 extra
    /// non-×2 skills each at rank 2 would be 60 additional points) but we
    /// just need to stay ≤ 86 and satisfy all Basic Skills.
    ///
    /// Total: Basic 13 skills at 2 = 26 pts. Handgun 6 = 6, Brawling
    /// bumped to 6 = +4 (already at 2, so +4 more), Evasion bumped to 6 =
    /// +4, Perception 6 = +4, Athletics 6 = +4, Stealth 6 = +4, total = 52.
    /// Remaining 34 pts → Shoulder Arms 6 (4 pts), Melee Weapon 6 (4 pts),
    /// Handgun is already handled, FirstAid 6 (+4 pts), Education 6 (+4),
    /// Concentration 6 (+4), HumanPerception 6 (+4), Persuasion 6 (+4),
    /// Conversation 6 (+4), that is 52 + 36 = 88 — too much. Let's keep it
    /// simple: all 13 Basic Skills at 2 = 26 pts, Handgun at 6 = 4 extra,
    /// ShoulderArms 6 = 6, MeleeWeapon 6 = 6, Tactics 4 = 4, Tracking 4 =
    /// 4. Total = 26 + 4 + 6 + 6 + 4 + 4 = 50. Use remaining 36 pts
    /// on a mix. But let's just build a simple spec that passes.
    fn valid_skills() -> Vec<(SkillId, u8)> {
        use crate::catalog::skills::{LanguageKind, LocalArea};
        // All 13 Basic Skills at 2, which costs 26 points, then spend
        // the remaining 60 points on non-×2 skills at various ranks.
        // Total must not exceed 86.
        vec![
            (SkillId::Athletics, 2),
            (SkillId::Brawling, 2),
            (SkillId::Concentration, 2),
            (SkillId::Conversation, 2),
            (SkillId::Education, 2),
            (SkillId::Evasion, 2),
            (SkillId::FirstAid, 2),
            (SkillId::HumanPerception, 2),
            (SkillId::Language(LanguageKind::Streetslang), 2),
            (
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
                2,
            ),
            (SkillId::Perception, 2),
            (SkillId::Persuasion, 2),
            (SkillId::Stealth, 2),
            // Extra skills (non-×2) to spend remaining 60 points.
            (SkillId::Handgun, 6),            // 6 pts
            (SkillId::ShoulderArms, 6),       // 6 pts
            (SkillId::MeleeWeapon, 6),        // 6 pts
            (SkillId::Interrogation, 6),      // 6 pts
            (SkillId::Tactics, 6),            // 6 pts
            (SkillId::Deduction, 6),          // 6 pts
            (SkillId::LibrarySearch, 6),      // 6 pts
            (SkillId::Tracking, 6),           // 6 pts
            (SkillId::WildernessSurvival, 6), // 6 pts
            (SkillId::Streetwise, 6),         // 6 pts
                                              // Total extra: 60. Grand total: 26 + 60 = 86. Exactly on budget.
        ]
    }

    fn valid_spec() -> CompleteSpec {
        CompleteSpec {
            role: Role::Solo,
            name: "V".into(),
            stats: valid_stats(),
            skill_picks: valid_skills(),
            cultural_language: Some(SkillId::Language(LanguageKind::Streetslang)),
            starting_cyberware: vec![],
            starting_money: 2_550,
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (named per the WP brief)
    // -----------------------------------------------------------------------

    /// `test_complete_package_valid_spec` — a fully valid spec succeeds and
    /// the returned character has correct derived stats. See pp.78–80.
    #[test]
    fn test_complete_package_valid_spec() {
        let catalog = load_catalog();
        let mut rng = Rng::seed_from_u64(503_001);
        let spec = valid_spec();

        let character = create_complete_package(spec.clone(), &catalog, &mut rng)
            .expect("valid spec must produce a character");

        // Role and name are passed through.
        assert_eq!(character.role, Role::Solo);
        assert_eq!(character.name, "V");
        assert_eq!(character.role_rank, 4);

        // Stats survive unchanged.
        assert_eq!(character.stats.body, 6);
        assert_eq!(character.stats.emp, 6);

        // HP: BODY 6, WILL 6 → ceil((6+6)/2) = 6 → 10 + 5×6 = 40. See p.79.
        assert_eq!(character.wounds.max_hp, 40);
        assert_eq!(character.wounds.current_hp, 40);
        // Seriously Wounded threshold: ceil(40/2) = 20.
        assert_eq!(character.wounds.seriously_wounded_threshold, 20);

        // Humanity: EMP 6 → 60. See p.80.
        assert_eq!(character.humanity, 60);

        // LUCK pool equals LUCK stat.
        assert_eq!(character.luck_pool, 6);

        // Money is the passed value.
        assert_eq!(character.money, Eurobucks(2_550));

        // All Basic Skills present at ≥ 2.
        assert!(
            character
                .skills
                .ranks
                .get(&SkillId::Athletics)
                .copied()
                .unwrap_or(0)
                >= 2
        );
        assert!(
            character
                .skills
                .ranks
                .get(&SkillId::Brawling)
                .copied()
                .unwrap_or(0)
                >= 2
        );

        // Cultural language at rank 4.
        let lang_rank = character
            .skills
            .ranks
            .get(&SkillId::Language(LanguageKind::Streetslang))
            .copied()
            .unwrap_or(0);
        // The cultural language is Streetslang rank 4 (free); skill_picks
        // listed it at 2, but cultural_language bumps it to 4.
        assert_eq!(lang_rank, 4);

        // No cyberware installed.
        assert!(character.cyberware.is_empty());
    }

    /// `test_complete_package_invalid_stat_too_high` — a stat above 8 returns
    /// `RankCapReached`. See p.78.
    #[test]
    fn test_complete_package_invalid_stat_too_high() {
        let mut spec = valid_spec();
        // Push INT to 9 — over the cap of 8. Reduce LUCK to keep total ≤ 62:
        // original sum = 62. INT 7→9 (+2), so reduce LUCK 6→4 (−2). Still 62.
        spec.stats.int = 9;
        spec.stats.luck = 4;

        let err = validate_complete_spec(&spec).expect_err("INT=9 must fail validation");
        assert!(
            matches!(err, RulesError::RankCapReached { current: 9, max: 8 }),
            "expected RankCapReached {{current:9, max:8}}, got {err:?}"
        );
    }

    /// `test_complete_package_skill_budget` — total skill cost > 86 returns
    /// `IpInsufficient`. See p.90.
    #[test]
    fn test_complete_package_skill_budget() {
        let mut spec = valid_spec();
        // Add extra non-×2 skills to push cost over 86. Add 2 more skills at
        // rank 6 (cost 6 each = 12 extra → 86 + 12 = 98 > 86).
        spec.skill_picks.push((SkillId::Bribery, 6));
        spec.skill_picks.push((SkillId::Composition, 6));

        let err = validate_complete_spec(&spec).expect_err("over-budget skills must fail");
        assert!(
            matches!(err, RulesError::IpInsufficient { required, available: 86 } if required > 86),
            "expected IpInsufficient with available=86, got {err:?}"
        );
    }

    /// `test_complete_package_skill_at_cap` — a skill at rank 7 (above 6)
    /// returns `RankCapReached`. See p.90.
    #[test]
    fn test_complete_package_skill_at_cap() {
        let mut spec = valid_spec();
        // Replace Handgun rank 6 with rank 7.
        for (skill, rank) in &mut spec.skill_picks {
            if *skill == SkillId::Handgun {
                *rank = 7;
            }
        }

        let err = validate_complete_spec(&spec).expect_err("rank 7 skill must fail validation");
        assert!(
            matches!(err, RulesError::RankCapReached { current: 7, max: 6 }),
            "expected RankCapReached {{current:7, max:6}}, got {err:?}"
        );
    }

    /// `test_complete_package_cyberware_humanity_loss` — installed cyberware
    /// reduces humanity. See pp.104–105, p.111, p.80.
    #[test]
    fn test_complete_package_cyberware_humanity_loss() {
        let catalog = load_catalog();
        let mut rng = Rng::seed_from_u64(503_005);

        // Neural Link (slug "neural_link") has preset HL = 7. Starting
        // humanity for EMP 6 = 60. After install: 60 − 7 = 53.
        let mut spec = valid_spec();
        spec.starting_cyberware = vec![CyberwareId("neural_link".into())];

        let character = create_complete_package(spec, &catalog, &mut rng)
            .expect("valid spec with neural_link must succeed");

        // Humanity must be strictly less than 60 (the pre-install value).
        assert!(
            character.humanity < 60,
            "humanity should be reduced after cyberware install; got {}",
            character.humanity
        );
        // Neural Link is in the cyberware list.
        assert!(
            character
                .cyberware
                .iter()
                .any(|c| c.id == CyberwareId("neural_link".into())),
            "neural_link must appear in character.cyberware"
        );
    }

    // -----------------------------------------------------------------------
    // Additional validation tests
    // -----------------------------------------------------------------------

    /// Stat total over 62 → `IpInsufficient`. See p.78.
    #[test]
    fn test_complete_package_stat_budget_exceeded() {
        let mut spec = valid_spec();
        // valid_stats() sums to 62. Bump REF by 1 (7→8) without reducing
        // anything else → total = 63.
        spec.stats.r#ref = 8;

        let err = validate_complete_spec(&spec).expect_err("stat total 63 must fail");
        assert!(
            matches!(
                err,
                RulesError::IpInsufficient {
                    required: 63,
                    available: 62
                }
            ),
            "expected IpInsufficient {{required:63, available:62}}, got {err:?}"
        );
    }

    /// Missing a Basic Skill → `RankCapReached`. See p.85.
    #[test]
    fn test_complete_package_missing_basic_skill() {
        let mut spec = valid_spec();
        // Remove Athletics entirely.
        spec.skill_picks
            .retain(|(skill, _)| *skill != SkillId::Athletics);

        let err = validate_complete_spec(&spec).expect_err("missing Athletics must fail");
        assert!(
            matches!(err, RulesError::RankCapReached { current: 0, max: 2 }),
            "expected RankCapReached {{current:0, max:2}}, got {err:?}"
        );
    }

    /// Starting money > 2,550 → `IpInsufficient`. See p.104.
    #[test]
    fn test_complete_package_money_cap() {
        let mut spec = valid_spec();
        spec.starting_money = 3_000;

        let err = validate_complete_spec(&spec).expect_err("money > 2550 must fail");
        assert!(
            matches!(
                err,
                RulesError::IpInsufficient {
                    required: 3_000,
                    available: 2_550
                }
            ),
            "expected IpInsufficient for money cap, got {err:?}"
        );
    }

    /// Double-cost (×2) skills correctly consume 2 pts per rank. See pp.82–84.
    #[test]
    fn test_complete_package_double_cost_skill_counts() {
        // Build a spec that puts all 86 pts into Autofire (×2) and Basic
        // Skills at minimum. Basic 13 at 2 = 26 pts (Autofire is a Basic
        // replacement here but not actually a Basic Skill so we keep it
        // separate). Let's carefully compute:
        //
        // Basic Skills at 2 = 26 points.
        // Remaining budget = 60. Autofire (×2) at rank 6 costs 6 × 2 = 12.
        // That leaves 48 for other non-×2 skills (e.g. 8 non-×2 skills at 6
        // = 48 pts). Total = 26 + 12 + 48 = 86.
        use crate::catalog::skills::{LanguageKind, LocalArea};
        let picks = vec![
            (SkillId::Athletics, 2),
            (SkillId::Brawling, 2),
            (SkillId::Concentration, 2),
            (SkillId::Conversation, 2),
            (SkillId::Education, 2),
            (SkillId::Evasion, 2),
            (SkillId::FirstAid, 2),
            (SkillId::HumanPerception, 2),
            (SkillId::Language(LanguageKind::Streetslang), 2),
            (
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
                2,
            ),
            (SkillId::Perception, 2),
            (SkillId::Persuasion, 2),
            (SkillId::Stealth, 2),
            // Non-×2 extras: 8 × 6 pts = 48 pts.
            (SkillId::Handgun, 6),
            (SkillId::ShoulderArms, 6),
            (SkillId::MeleeWeapon, 6),
            (SkillId::Interrogation, 6),
            (SkillId::Tactics, 6),
            (SkillId::Deduction, 6),
            (SkillId::LibrarySearch, 6),
            (SkillId::Tracking, 6),
            // ×2 skill: Autofire rank 6 = 12 pts.
            (SkillId::Autofire, 6),
            // That's 26 + 48 + 12 = 86. Exactly on budget.
        ];

        let spec = CompleteSpec {
            role: Role::Solo,
            name: "Autofire Specialist".into(),
            stats: valid_stats(),
            skill_picks: picks,
            cultural_language: Some(SkillId::Language(LanguageKind::Streetslang)),
            starting_cyberware: vec![],
            starting_money: 0,
        };

        validate_complete_spec(&spec).expect("exactly 86 pts with a ×2 skill must pass");
    }

    /// ×2 skill one rank over the cap still triggers RankCapReached. See p.90.
    #[test]
    fn test_complete_package_double_cost_skill_rank_cap() {
        let mut spec = valid_spec();
        spec.skill_picks.push((SkillId::Autofire, 7));

        let err = validate_complete_spec(&spec).expect_err("Autofire rank 7 must fail");
        assert!(
            matches!(err, RulesError::RankCapReached { current: 7, max: 6 }),
            "expected RankCapReached {{current:7, max:6}}, got {err:?}"
        );
    }
}
