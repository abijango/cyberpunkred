//! Improvement Point spending — raise Skill and Role Ability ranks.
//!
//! Players spend accumulated I.P. between sessions to advance their
//! character. This module implements the spend actions and the cost
//! formulae per p.411.
//!
//! # Cost formulae (p.411)
//!
//! The rulebook's p.411 tables are **linear**, not quadratic:
//!
//! | Table          | Cost to reach level N |
//! |----------------|------------------------|
//! | Normal Skill   | `N × 20`               |
//! | Difficult (×2) | `N × 40`               |
//! | Role Ability   | `N × 60`               |
//!
//! The WP spec doc proposed `next_rank²` but the RAW table does not match
//! that formula. Per the CLAUDE.md rule "Rulebook ambiguity: default to RAW;
//! comment the tension; flag in PR." — this implementation follows the
//! printed p.411 tables. Deviation is documented in the PR description.
//!
//! Skills and Role Abilities cap at rank 10 (p.411 sidebar: "Even if you
//! have the Improvement Points, you can't skip Levels/Ranks").
//!
//! See p.411 for the full cost tables; see p.408 for an overview of the
//! Improvement Point system.

use crate::catalog::skills::SkillDefinition;
use crate::catalog::Catalog;
use crate::character::Character;
use crate::effects::SkillId;
use crate::error::RulesError;

/// Maximum rank for Skills and Role Abilities. See p.411.
const MAX_RANK: u8 = 10;

/// Returns the IP cost to raise a skill from `current_rank` to
/// `current_rank + 1`.
///
/// Per p.411 ("Typical Skill Improvement" and "Difficult (×2) Skill
/// Improvement" tables):
/// - Normal skill cost  = `next_rank × 20`
/// - Difficult (×2) cost = `next_rank × 40`
///
/// Note: the WP spec proposed `next_rank²` but the RAW table is linear —
/// see module-level doc for the full explanation. This function follows
/// the printed p.411 tables. Flag raised in PR.
///
/// # Panics
///
/// Does not panic. `next_rank` is computed from `current_rank + 1` as a
/// `u32` to avoid overflow before multiplication.
pub fn ip_cost_for_next_skill_rank(current_rank: u8, double_cost: bool) -> u32 {
    // See p.411 — cost is based on the *next* (target) rank.
    let next_rank = u32::from(current_rank) + 1;
    if double_cost {
        next_rank * 40
    } else {
        next_rank * 20
    }
}

/// Returns the IP cost to raise the Role Ability rank from `current` to
/// `current + 1`.
///
/// Per p.411 ("Role Ability Rank Improvement" table):
/// `cost = next_rank × 60`
///
/// Note: the WP spec proposed `next_rank²` — the RAW table is linear.
/// See module-level doc and PR description. // See p.411
pub fn ip_cost_for_next_role_rank(current_rank: u8) -> u32 {
    // See p.411 — cost is based on the *next* (target) rank.
    let next_rank = u32::from(current_rank) + 1;
    next_rank * 60
}

/// Spend IP to raise a skill by 1 rank. Returns the IP cost on success.
///
/// # Lookup
///
/// The `double_cost` flag is read from the catalog via
/// `catalog.get(slug).map(|d| d.double_cost).unwrap_or(false)`.
/// Because `SkillId` is a closed enum without a canonical slug embedded in
/// it, we convert the variant to a normalised slug string by serialising
/// with `ron` and lower-casing. If the skill is not in the catalog (e.g. a
/// parameterised variant not registered), `double_cost` defaults to `false`.
///
/// # Errors
///
/// - [`RulesError::RankCapReached`] — the skill is already at rank 10 (p.411).
/// - [`RulesError::IpInsufficient`] — the character does not have enough IP.
///
/// # Side-effects on success
///
/// - `character.improvement_points` is decremented by the cost.
/// - The skill's rank in `character.skills.ranks` is incremented by 1.
///
/// See p.411.
pub fn spend_ip_on_skill(
    character: &mut Character,
    skill: SkillId,
    catalog: &Catalog<SkillDefinition>,
) -> Result<u32, RulesError> {
    let current_rank = character.skills.ranks.get(&skill).copied().unwrap_or(0);

    // Skills cap at 10. See p.411.
    if current_rank >= MAX_RANK {
        return Err(RulesError::RankCapReached {
            current: current_rank,
            max: MAX_RANK,
        });
    }

    // Resolve double_cost from catalog. Slug is derived by ron-serialising the
    // SkillId and lower-casing; parameterised skills not in the catalog fall
    // back to false. See p.411.
    let double_cost = skill_double_cost(&skill, catalog);

    let cost = ip_cost_for_next_skill_rank(current_rank, double_cost);

    if character.improvement_points < cost {
        return Err(RulesError::IpInsufficient {
            required: cost,
            available: character.improvement_points,
        });
    }

    character.improvement_points -= cost;
    *character.skills.ranks.entry(skill).or_insert(0) += 1;

    Ok(cost)
}

/// Spend IP to raise the Role Ability rank by 1. Returns the IP cost.
///
/// # Errors
///
/// - [`RulesError::RankCapReached`] — the role rank is already at 10 (p.411).
/// - [`RulesError::IpInsufficient`] — the character does not have enough IP.
///
/// # Side-effects on success
///
/// - `character.improvement_points` is decremented by the cost.
/// - `character.role_rank` is incremented by 1.
///
/// See p.411.
pub fn spend_ip_on_role_ability(character: &mut Character) -> Result<u32, RulesError> {
    let current_rank = character.role_rank;

    // Role Abilities cap at 10. See p.411.
    if current_rank >= MAX_RANK {
        return Err(RulesError::RankCapReached {
            current: current_rank,
            max: MAX_RANK,
        });
    }

    let cost = ip_cost_for_next_role_rank(current_rank);

    if character.improvement_points < cost {
        return Err(RulesError::IpInsufficient {
            required: cost,
            available: character.improvement_points,
        });
    }

    character.improvement_points -= cost;
    character.role_rank += 1;

    Ok(cost)
}

/// Derive the `double_cost` flag for a `SkillId` from the catalog.
///
/// Strategy: serialise the skill id via `ron` to a string, strip the outer
/// enum wrapper to a normalised slug, and look it up in the catalog.
/// If the skill is absent (e.g. uncommon parameterised variant), returns
/// `false` — the safest default for progression cost.
fn skill_double_cost(skill: &SkillId, catalog: &Catalog<SkillDefinition>) -> bool {
    // First try: look the definition up by scanning catalog for a matching id.
    // This avoids having to replicate the slug-derivation logic here and works
    // correctly for all parameterised SkillId variants. See p.411.
    catalog
        .iter()
        .find(|(_, def)| &def.id == skill)
        .map(|(_, def)| def.double_cost)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::skills::SkillDefinition;
    use crate::catalog::Catalog;
    use crate::character::data::{Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::character::Character;
    use crate::effects::{EffectStack, SkillId};
    use crate::types::{CharacterId, Eurobucks};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn empty_catalog() -> Catalog<SkillDefinition> {
        Catalog::new(HashMap::new())
    }

    fn make_character_with_ip(ip: u32) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0x508)),
            name: "Test Runner".to_string(),
            handle: None,
            role: Role::Solo,
            role_rank: 1,
            stats: StatBlock {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 6,
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
            inventory: crate::character::data::Inventory { items: vec![] },
            wounds: Wounds::default(),
            humanity: 40,
            luck_pool: 6,
            money: Eurobucks(100),
            improvement_points: ip,
            lifepath: crate::character::data::Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // ip_cost_for_next_skill_rank
    // -----------------------------------------------------------------------

    /// Acceptance: normal skill costs match p.411 "Typical Skill Improvement" table.
    /// Level 1=20, 2=40, 9=180, 10=200.
    #[test]
    fn test_ip_cost_normal_skill() {
        // rank 0→1: next_rank=1, cost=20
        assert_eq!(ip_cost_for_next_skill_rank(0, false), 20);
        // rank 1→2: next_rank=2, cost=40
        assert_eq!(ip_cost_for_next_skill_rank(1, false), 40);
        // rank 2→3: next_rank=3, cost=60
        assert_eq!(ip_cost_for_next_skill_rank(2, false), 60);
        // rank 9→10: next_rank=10, cost=200 — matches p.411 table cell "Level 10 = 200"
        assert_eq!(ip_cost_for_next_skill_rank(9, false), 200);

        // NOTE: The WP spec doc says rank 1→2=4, 2→3=9, 9→10=100 (next_rank²).
        // The RAW p.411 table is linear (next_rank × 20), so the acceptance
        // test values here follow the rulebook table, not the spec formula.
        // Deviation flagged in PR description. See p.411.
    }

    /// Acceptance: ×2 skill costs match p.411 "Difficult (×2) Skill Improvement" table.
    /// Level 1=40, 2=80, …, 10=400.
    #[test]
    fn test_ip_cost_double_skill() {
        // rank 1→2: next_rank=2, cost=80 — matches p.411 "Difficult" table cell "Level 2 = 80"
        assert_eq!(ip_cost_for_next_skill_rank(1, true), 80);
        // rank 0→1: next_rank=1, cost=40
        assert_eq!(ip_cost_for_next_skill_rank(0, true), 40);
        // rank 9→10: next_rank=10, cost=400
        assert_eq!(ip_cost_for_next_skill_rank(9, true), 400);

        // NOTE: The WP spec says rank 1→2=8 (double of 4). The RAW table says
        // Level 2 for a ×2 skill costs 80, not 8. Implementation follows RAW.
        // Deviation flagged in PR. See p.411.
    }

    /// Acceptance: Role Ability costs match p.411 "Role Ability Rank Improvement" table.
    /// Rank 1=60, 2=120, …, 10=600.
    #[test]
    fn test_ip_cost_role_rank() {
        // rank 0→1: next_rank=1, cost=60
        assert_eq!(ip_cost_for_next_role_rank(0), 60);
        // rank 1→2: next_rank=2, cost=120
        assert_eq!(ip_cost_for_next_role_rank(1), 120);
        // rank 2→3: next_rank=3, cost=180
        assert_eq!(ip_cost_for_next_role_rank(2), 180);
        // rank 9→10: next_rank=10, cost=600
        assert_eq!(ip_cost_for_next_role_rank(9), 600);
    }

    // -----------------------------------------------------------------------
    // spend_ip_on_skill
    // -----------------------------------------------------------------------

    /// Acceptance: with sufficient IP, the skill rank increments and IP is deducted.
    #[test]
    fn test_spend_skill_succeeds() {
        // rank 0→1 costs 20 IP
        let mut c = make_character_with_ip(20);
        let result = spend_ip_on_skill(&mut c, SkillId::Handgun, &empty_catalog());
        assert!(result.is_ok());
        let cost = result.unwrap();
        assert_eq!(cost, 20, "rank 0→1 normal skill should cost 20 IP (p.411)");
        assert_eq!(
            c.skills.ranks.get(&SkillId::Handgun).copied(),
            Some(1),
            "Handgun rank must be 1 after spend"
        );
        assert_eq!(
            c.improvement_points, 0,
            "IP pool must be decremented by cost"
        );
    }

    /// Acceptance: insufficient IP returns IpInsufficient, no mutation.
    #[test]
    fn test_spend_skill_insufficient_ip() {
        let mut c = make_character_with_ip(10); // needs 20 for rank 0→1
        let result = spend_ip_on_skill(&mut c, SkillId::Handgun, &empty_catalog());
        assert!(
            matches!(
                result,
                Err(RulesError::IpInsufficient {
                    required: 20,
                    available: 10
                })
            ),
            "expected IpInsufficient, got {:?}",
            result
        );
        // Character must be unchanged
        assert_eq!(c.improvement_points, 10);
        assert!(c.skills.ranks.get(&SkillId::Handgun).is_none());
    }

    /// Acceptance: spending on a skill already at rank 10 returns RankCapReached.
    #[test]
    fn test_spend_skill_at_cap() {
        let mut c = make_character_with_ip(10_000);
        c.skills.ranks.insert(SkillId::Handgun, 10);
        let result = spend_ip_on_skill(&mut c, SkillId::Handgun, &empty_catalog());
        assert!(
            matches!(
                result,
                Err(RulesError::RankCapReached {
                    current: 10,
                    max: 10
                })
            ),
            "expected RankCapReached, got {:?}",
            result
        );
        // IP and rank unchanged
        assert_eq!(c.improvement_points, 10_000);
        assert_eq!(c.skills.ranks.get(&SkillId::Handgun).copied(), Some(10));
    }

    // -----------------------------------------------------------------------
    // spend_ip_on_role_ability
    // -----------------------------------------------------------------------

    /// Acceptance: spending on role at rank 10 returns RankCapReached.
    #[test]
    fn test_spend_role_at_cap() {
        let mut c = make_character_with_ip(10_000);
        c.role_rank = 10;
        let result = spend_ip_on_role_ability(&mut c);
        assert!(
            matches!(
                result,
                Err(RulesError::RankCapReached {
                    current: 10,
                    max: 10
                })
            ),
            "expected RankCapReached, got {:?}",
            result
        );
        assert_eq!(c.improvement_points, 10_000);
        assert_eq!(c.role_rank, 10);
    }

    /// Role spend with sufficient IP: role_rank increments, IP deducted.
    #[test]
    fn test_spend_role_succeeds() {
        // role_rank=1 → cost for rank 1→2 = next_rank(2) × 60 = 120
        let mut c = make_character_with_ip(120);
        c.role_rank = 1;
        let result = spend_ip_on_role_ability(&mut c);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        let cost = result.unwrap();
        assert_eq!(cost, 120, "rank 1→2 role should cost 120 IP (p.411)");
        assert_eq!(c.role_rank, 2);
        assert_eq!(c.improvement_points, 0);
    }

    /// Role spend insufficient IP returns IpInsufficient.
    #[test]
    fn test_spend_role_insufficient_ip() {
        let mut c = make_character_with_ip(50); // needs 60 for rank 0→1
        c.role_rank = 0;
        let result = spend_ip_on_role_ability(&mut c);
        assert!(
            matches!(
                result,
                Err(RulesError::IpInsufficient {
                    required: 60,
                    available: 50
                })
            ),
            "expected IpInsufficient, got {:?}",
            result
        );
        assert_eq!(c.improvement_points, 50);
        assert_eq!(c.role_rank, 0);
    }

    /// Verify that double_cost lookup from catalog works when a catalog entry is present.
    #[test]
    fn test_double_cost_from_catalog() {
        use crate::catalog::skills::{SkillCategory, SkillDefinition};
        use crate::types::Stat;

        let mut entries: HashMap<String, SkillDefinition> = HashMap::new();
        entries.insert(
            "autofire".to_string(),
            SkillDefinition {
                id: SkillId::Autofire,
                display_name: "Autofire".to_string(),
                linked_stat: Stat::Ref,
                category: SkillCategory::Ranged,
                double_cost: true,
                description: "Autofire (×2).".to_string(),
            },
        );
        let catalog = Catalog::new(entries);

        // rank 0→1 for Autofire (×2) should cost 40 (next_rank=1 × 40)
        let mut c = make_character_with_ip(40);
        let result = spend_ip_on_skill(&mut c, SkillId::Autofire, &catalog);
        assert!(result.is_ok(), "expected Ok, got {:?}", result);
        assert_eq!(
            result.unwrap(),
            40,
            "Autofire rank 0→1 costs 40 (×2 table, p.411)"
        );
        assert_eq!(c.skills.ranks.get(&SkillId::Autofire).copied(), Some(1));
        assert_eq!(c.improvement_points, 0);
    }

    /// Multiple successive spends accumulate correctly.
    #[test]
    fn test_multiple_skill_spends() {
        // rank 0→1 (cost 20) then 1→2 (cost 40) = total 60
        let mut c = make_character_with_ip(60);
        spend_ip_on_skill(&mut c, SkillId::Handgun, &empty_catalog()).unwrap();
        spend_ip_on_skill(&mut c, SkillId::Handgun, &empty_catalog()).unwrap();
        assert_eq!(c.skills.ranks.get(&SkillId::Handgun).copied(), Some(2));
        assert_eq!(c.improvement_points, 0);
    }
}
