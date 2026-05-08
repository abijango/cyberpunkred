//! Teamwork (Exec Role Ability) — WP-516.
//!
//! ## Name disambiguation
//!
//! The rulebook (pp.153–154) names this ability **"Teamwork"**. The WP-516
//! spec uses "Resources" as the module/API slug. This module honours the spec
//! slug (`resources`, `resources_rank`, `ResourcesPool`) in its public API
//! while documenting the correct rulebook name in every doc comment. The
//! engine slug is `resources` — callers should not rely on the string being
//! "teamwork". Deviation flagged in PR.
//!
//! ## Rulebook mechanics (pp.153–154)
//!
//! The Exec's Teamwork ability gives them corporate backing across several
//! tiers that scale with Rank:
//!
//! ### Team Members (p.154)
//! Team Members are NPC allies assigned to the Exec by their corporation.
//!
//! | Rank threshold | Total Team Members |
//! |---|---|
//! | < 3  | 0 |
//! | 3–4  | 1 |
//! | 5–8  | 2 |
//! | 9–10 | 3 (maximum) |
//!
//! ### Corporate Benefits (p.153)
//! Additional per-gig corporate resource access scales by rank:
//!
//! | Rank | Benefit |
//! |---|---|
//! | 1  | Signing bonus: Businesswear suit (not resellable without suspicion) |
//! | 2  | Corporate housing: Company Conapt (no rent) |
//! | 6  | Corporate health insurance: Trauma Team Silver |
//! | 7  | Upgraded housing: Beaverville Executive Zone house |
//! | 8  | Upgraded health insurance: Trauma Team Executive |
//! | 10 | Upgraded housing: McMansion or Luxury Penthouse |
//!
//! ### Money per gig
//!
//! The rulebook does not specify an explicit per-gig Eurobucks pool keyed on
//! Rank. The WP-516 spec contract requires a `money_per_gig` field on
//! [`ResourcesPool`]. This implementation models it as a corporate "pull
//! budget" — a reasonable extrapolation from the housing and benefit tiers:
//! each rank tier grants progressively larger discretionary funds the Exec can
//! draw on for legitimate corporate business. Deviation from strict RAW flagged
//! in PR; the exact table is GM-adjudicated in RAW.
//!
//! The approximation used (per rank, scaled by 500eb / rank tier) is:
//!
//! | Rank | money_per_gig |
//! |---|---|
//! | 1  | 500eb   |
//! | 2  | 1,000eb |
//! | 3  | 1,500eb |
//! | 4  | 2,000eb |
//! | 5  | 3,000eb |
//! | 6  | 4,000eb |
//! | 7  | 5,500eb |
//! | 8  | 7,000eb |
//! | 9  | 9,000eb |
//! | 10 | 12,000eb |
//!
//! See pp.153–154.

use crate::character::data::Role;
use crate::character::Character;
use crate::types::Eurobucks;

/// Returns the character's effective Teamwork (Resources) rank.
///
/// Per p.153, Teamwork is the Exec's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Exec`].
/// For any other role the ability does not apply; this function returns `0`
/// so callers can skip application without special-casing the role check.
///
/// See pp.153–154.
pub fn resources_rank(character: &Character) -> u8 {
    // See p.153 — Teamwork belongs exclusively to the Exec role.
    if character.role == Role::Exec {
        character.role_rank
    } else {
        0
    }
}

/// Per-gig corporate resource pool available to an Exec of a given Teamwork
/// rank.
///
/// ## Fields
///
/// - `money_per_gig` — discretionary corporate funds the Exec can draw on
///   each gig. **Note:** RAW (pp.153–154) does not specify a numeric
///   Eurobucks pool; this field is an engine extrapolation from the benefit
///   tier progression. See the module-level deviation note.
///
/// - `team_size` — number of NPC Team Members the corporation assigns.
///   Derived from the RAW thresholds on p.154:
///   - Rank 1–2: 0 members.
///   - Rank 3–4: 1 member ("Starting at Rank 3, Teamwork gives the Exec a
///     Team Member").
///   - Rank 5–8: 2 members ("Rank 5 … give[s] the Exec an additional Team
///     Member").
///   - Rank 9–10: 3 members ("Rank … 9 … give[s] the Exec an additional Team
///     Member, capping out at a maximum of 3 total Team Members at Rank 9").
///
/// See pp.153–154.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourcesPool {
    /// Monthly / per-gig corporate pull in Eurobucks. See module-level note.
    /// See pp.153–154 (extrapolated; RAW does not specify a numeric table).
    pub money_per_gig: Eurobucks,
    /// NPC Team Members allocated by the corporation. See p.154.
    pub team_size: u8,
}

/// Computes the [`ResourcesPool`] for a given Teamwork rank.
///
/// Returns the pool appropriate to `rank`, using the RAW Team Member
/// thresholds from p.154 and the extrapolated money table documented in the
/// module header.
///
/// Returns a zero pool (`money_per_gig = 0`, `team_size = 0`) for `rank == 0`.
///
/// See pp.153–154.
pub fn resources_pool(rank: u8) -> ResourcesPool {
    // Team member allocation per p.154:
    //   rank 0-2: 0 members
    //   rank 3-4: 1 member  ("Starting at Rank 3")
    //   rank 5-8: 2 members ("Ranks 5 … give the Exec an additional Team Member")
    //   rank 9+:  3 members ("Rank … 9 … capping out at a maximum of 3 … at Rank 9")
    let team_size = match rank {
        0..=2 => 0,
        3..=4 => 1,
        5..=8 => 2,
        _ => 3, // rank 9-10, max per p.154
    };

    // Per-gig money pool — extrapolated; RAW has no explicit Eurobucks table.
    // Calibrated to the housing / benefit tiers on pp.153-154 so that
    // higher-ranking Execs can afford proportionally larger corporate pulls.
    let money_per_gig = match rank {
        0 => 0,
        1 => 500,
        2 => 1_000,
        3 => 1_500,
        4 => 2_000,
        5 => 3_000,
        6 => 4_000,
        7 => 5_500,
        8 => 7_000,
        9 => 9_000,
        _ => 12_000, // rank 10
    };

    ResourcesPool {
        money_per_gig: Eurobucks(money_per_gig),
        team_size,
    }
}

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
            id: CharacterId(Uuid::from_u128(0xEC)),
            name: "TestExec".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 5,
                dex: 5,
                tech: 5,
                cool: 7,
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
            inventory: Inventory { items: vec![] },
            wounds: Wounds {
                current_hp: 35,
                max_hp: 35,
                seriously_wounded_threshold: 18,
                death_save_base: 6,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    #[test]
    fn test_resources_rank_for_exec() {
        // Per p.153: Exec's Teamwork rank equals their Role Ability rank.
        let character = make_character(Role::Exec, 5);
        assert_eq!(resources_rank(&character), 5);
    }

    #[test]
    fn test_resources_rank_zero_for_non_exec() {
        // Non-Exec roles do not have Teamwork -- rank must be 0. See p.153.
        let character = make_character(Role::Solo, 7);
        assert_eq!(resources_rank(&character), 0);
    }

    #[test]
    fn test_resources_pool_scales() {
        // Verify Team Member thresholds per p.154.
        assert_eq!(resources_pool(0).team_size, 0, "rank 0: no members");
        assert_eq!(resources_pool(1).team_size, 0, "rank 1: no members yet");
        assert_eq!(resources_pool(2).team_size, 0, "rank 2: no members yet");
        assert_eq!(
            resources_pool(3).team_size,
            1,
            "rank 3: first member per p.154"
        );
        assert_eq!(resources_pool(4).team_size, 1, "rank 4: still 1 member");
        assert_eq!(
            resources_pool(5).team_size,
            2,
            "rank 5: second member per p.154"
        );
        assert_eq!(resources_pool(8).team_size, 2, "rank 8: still 2 members");
        assert_eq!(
            resources_pool(9).team_size,
            3,
            "rank 9: max 3 members per p.154"
        );
        assert_eq!(
            resources_pool(10).team_size,
            3,
            "rank 10: still max 3 members"
        );

        // Money pool must be strictly increasing with rank (extrapolated table).
        for lo in 1u8..10 {
            let hi = lo + 1;
            assert!(
                resources_pool(lo).money_per_gig < resources_pool(hi).money_per_gig,
                "money_per_gig must increase: rank {} < rank {}",
                lo,
                hi
            );
        }

        // rank 0 must yield a zero pool.
        let zero = resources_pool(0);
        assert_eq!(zero.money_per_gig, Eurobucks(0));
        assert_eq!(zero.team_size, 0);
    }
}
