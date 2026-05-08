//! Interface (Netrunner Role Ability) — WP-511.
//!
//! ## Rulebook mechanics (pp.144, 199)
//!
//! The **Interface** ability is the Netrunner's Role Ability. Its rank
//! (`role_rank`) determines:
//!
//! - How many NET Actions the Netrunner can take per Turn (p.144 / p.197 table;
//!   implemented in [`crate::netrunning::actions`]).
//! - The d10-additive bonus used in every Interface Ability check (pp.199+).
//!
//! p.144 states: "As a Netrunner, you have a special Interface ability that
//! only you possess, the ability to interact directly with the NET. The higher
//! your Interface ability, the more NET Actions you can take and the better
//! you are at using any of your Interface Abilities."
//!
//! p.199 confirms the NET Actions table used in WP-403.
//!
//! ## What this module provides
//!
//! - [`interface_rank`] — returns the effective Interface rank for a character.
//! - [`net_actions_per_turn`] — convenience wrapper over
//!   [`crate::netrunning::actions::net_actions_per_turn`].
//!
//! See pp.144, 199.

use crate::character::data::Role;
use crate::character::Character;

/// Returns the character's effective Interface rank.
///
/// Per pp.144, 199, Interface is the Netrunner's Role Ability and its rank
/// equals `character.role_rank` when the character is a [`Role::Netrunner`].
/// For any other role the ability does not apply; this function returns `0`
/// so callers can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::interface::interface_rank;
/// # use cpr_rules::character::Character;
/// // See test_interface_rank_for_netrunner.
/// ```
///
/// See pp.144, 199.
pub fn interface_rank(character: &Character) -> u8 {
    // See pp.144, 199.
    if character.role == Role::Netrunner {
        character.role_rank
    } else {
        0
    }
}

/// Number of NET Actions this Netrunner gets per Turn.
///
/// Convenience wrapper that calls
/// [`crate::netrunning::actions::net_actions_per_turn`] with the result of
/// [`interface_rank`], so callers that have a [`Character`] reference do not
/// need to extract the rank themselves.
///
/// Per p.144 / p.197 table: Interface Rank 1–3 → 2, 4–6 → 3, 7–9 → 4, 10 → 5.
/// Rank 0 (non-Netrunner) returns 2 (the floor per the existing WP-403
/// implementation which clamps rather than panics).
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::interface::net_actions_per_turn;
/// # use cpr_rules::character::Character;
/// // See test_net_actions_for_netrunner_rank_4.
/// ```
///
/// See pp.144, 199.
pub fn net_actions_per_turn(character: &Character) -> u8 {
    // See pp.144, 199.
    crate::netrunning::actions::net_actions_per_turn(interface_rank(character))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 7,
                r#ref: 6,
                dex: 6,
                tech: 6,
                cool: 6,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 6,
                emp: 5,
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
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    /// `test_interface_rank_for_netrunner`: Netrunner with role_rank 4 → 4.
    ///
    /// Per pp.144, 199: Interface rank equals role_rank for Netrunner characters.
    #[test]
    fn test_interface_rank_for_netrunner() {
        let character = make_character(Role::Netrunner, 4);
        assert_eq!(
            interface_rank(&character),
            4,
            "Netrunner rank 4 → 4 (pp.144, 199)"
        );
    }

    /// `test_interface_rank_zero_for_non_netrunner`: Solo role → 0.
    ///
    /// Per pp.144, 199: Interface belongs exclusively to the Netrunner role.
    #[test]
    fn test_interface_rank_zero_for_non_netrunner() {
        let character = make_character(Role::Solo, 5);
        assert_eq!(interface_rank(&character), 0, "Solo → 0 (pp.144, 199)");
    }

    /// `test_net_actions_for_netrunner_rank_4`: Netrunner role_rank 4 → 3 actions.
    ///
    /// Per p.144 / p.197 table: Interface Rank 4–6 → 3 NET Actions.
    #[test]
    fn test_net_actions_for_netrunner_rank_4() {
        let character = make_character(Role::Netrunner, 4);
        assert_eq!(
            net_actions_per_turn(&character),
            3,
            "Netrunner rank 4 → 3 NET actions (p.197 table)"
        );
    }

    /// `test_net_actions_zero_for_solo`: Solo role → 2 actions (rank 0 floor).
    ///
    /// Per p.197: rank 0 (non-Netrunner) maps to 2 — the floor of the table.
    /// Interface must be rank ≥ 1 to actually Netrun, but the function clamps
    /// rather than panics (consistent with WP-403 design).
    #[test]
    fn test_net_actions_zero_for_solo() {
        let character = make_character(Role::Solo, 5);
        assert_eq!(
            net_actions_per_turn(&character),
            2,
            "Solo (rank 0) → 2 NET actions (rank 0 floor, p.197)"
        );
    }
}
