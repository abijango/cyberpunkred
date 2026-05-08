//! Operator (Fixer Role Ability) — WP-517.
//!
//! ## Rulebook mechanics (pp.159–161)
//!
//! The Fixer's Role Ability is **Operator**. It has four facets:
//!
//! - **Contacts & Clients** — who the Fixer can reach out to; narrated by the GM.
//! - **Reach** — the highest Night Market price tier that the Fixer can
//!   *always* source (even when that tier is otherwise unavailable). This also
//!   gates which Night Market items a Fixer can procure for the crew. See
//!   [`max_procurable_fixer_rank`].
//! - **Haggle** — the ability to strike a better deal. At certain ranks this
//!   is a flat percentage off/on the item price; at other ranks it takes a
//!   different form (bulk discount, deferred payment, job-pay bonus). See
//!   [`operator_price_multiplier`].
//! - **Grease** — cultural fluency; handled narratively, not modelled here.
//!
//! ## Operator Rank table (pp.160–161)
//!
//! | Rank | Reach (always-sourceable tier) | Haggle (item-price effect)            |
//! |------|-------------------------------|---------------------------------------|
//! | 1–2  | Cheap / Everyday              | 10 % off or on (× 90 %)               |
//! | 3–4  | Expensive                     | Buy 5+, get 1 free (no % multiplier)  |
//! | 5–6  | Super Luxury (Night Market)   | +20 % job pay (no item % multiplier)  |
//! | 7–8  | Very Expensive                | Pay half now, half next month         |
//! | 9    | Luxury                        | 20 % off or on (× 80 %)               |
//! | 10   | Super Luxury (always)         | Double pay on Dangerous Jobs           |
//!
//! The `operator_price_multiplier` function encodes only the tiers where the
//! rulebook specifies a straightforward percentage discount on item prices:
//! Ranks 1–2 (10 % off → 90) and Rank 9 (20 % off → 80). Other haggle
//! effects are mechanical variants that do not map to a per-item multiplier;
//! those ranks return 100 (no discount). Deviation: the spec calls for a
//! multiplier-based API; non-percentage haggle effects are flagged here and
//! should be handled in a dedicated haggle action in a later WP.
//!
//! See pp.159–161.

use crate::character::data::Role;
use crate::character::Character;
use crate::types::Eurobucks;

// ---------------------------------------------------------------------------
// operator_rank
// ---------------------------------------------------------------------------

/// Returns the character's effective Operator rank.
///
/// Per p.159, Operator is the Fixer's Role Ability; its rank equals
/// `character.role_rank` when the character is a [`Role::Fixer`].
/// For all other roles the ability does not apply and this function
/// returns `0` so callers can gate on the result without special-casing
/// the role check.
///
/// See p.159.
pub fn operator_rank(character: &Character) -> u8 {
    // See p.159 — Operator belongs exclusively to the Fixer role.
    if character.role == Role::Fixer {
        character.role_rank
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// operator_price_multiplier
// ---------------------------------------------------------------------------

/// Haggle price multiplier × 100 for Night Market purchases at the given rank.
///
/// The return value is the percentage of the base price the Fixer pays (or
/// charges) after the Haggle ability is applied. For example:
/// - `90` → the Fixer pays 90 % of the listed price (10 % off).
/// - `80` → the Fixer pays 80 % of the listed price (20 % off).
/// - `100` → no item-price multiplier at this rank; see the module doc for
///   the alternative haggle effects at Ranks 3–4, 5–6, 7–8, and 10.
///
/// Tier breakdown (pp.160–161):
///
/// | Rank  | Multiplier | Rulebook Haggle text                              |
/// |-------|-----------|---------------------------------------------------|
/// | 1–2   | 90        | "get 10% more or less than market price"          |
/// | 3–4   | 100       | "buy 5 or more of same item, get 1 more for free" |
/// | 5–6   | 100       | "+20% job pay per person" (not an item discount)  |
/// | 7–8   | 100       | "pay half now and half in one month"              |
/// | 9     | 80        | "get 20% more or less than market price"          |
/// | 10    | 100       | "negotiate to double pay on a Dangerous Job"      |
///
/// Returns `100` for `rank == 0` (ability not active).
///
/// See pp.160–161.
pub fn operator_price_multiplier(rank: u8) -> u8 {
    // See p.160 (Ranks 1–2: 10 % off/on) and p.161 (Rank 9: 20 % off/on).
    match rank {
        0 => 100,
        1 | 2 => 90,
        3 | 4 => 100,
        5 | 6 => 100,
        7 | 8 => 100,
        9 => 80,
        _ => 100, // Rank 10+ — double-pay mechanic, not an item % discount.
    }
}

// ---------------------------------------------------------------------------
// discounted_price
// ---------------------------------------------------------------------------

/// Apply the Operator Haggle discount to a base price.
///
/// Computes `floor(base_price × multiplier / 100)` where `multiplier` is
/// [`operator_price_multiplier(rank)`]. The floor ensures we never round up
/// (the Fixer always benefits, never pays a fraction extra). Returns
/// [`Eurobucks`] so the result slots directly into the rest of the economy.
///
/// See pp.160–161.
pub fn discounted_price(rank: u8, base: Eurobucks) -> Eurobucks {
    // See pp.160–161.
    let multiplier = operator_price_multiplier(rank) as i64;
    Eurobucks(base.0 * multiplier / 100)
}

// ---------------------------------------------------------------------------
// max_procurable_fixer_rank
// ---------------------------------------------------------------------------

/// Maximum Night Market `min_fixer_rank` the Fixer can procure at the given
/// Operator rank.
///
/// This is the **Reach** facet of the Operator ability (p.159). It defines
/// the highest `min_fixer_rank` value on a [`NightMarketItem`] that the
/// Fixer can source as always-available. Items with a higher `min_fixer_rank`
/// require a Fixer of at least that rank.
///
/// Reach ladder (pp.160–161):
///
/// | Operator Rank | Reach tier (always-sourceable)             | Returns |
/// |---------------|--------------------------------------------|---------|
/// | 0             | None (no Operator ability)                 | 0       |
/// | 1–2           | Cheap / Everyday items (min_fixer_rank ≤ 2)| 2       |
/// | 3–4           | Up to Expensive (min_fixer_rank ≤ 4)       | 4       |
/// | 5–6           | Super Luxury via Night Market (≤ 6)        | 6       |
/// | 7–8           | Up to Very Expensive piece-by-piece (≤ 8)  | 8       |
/// | 9             | Up to Luxury piece-by-piece (≤ 9)          | 9       |
/// | 10+           | Up to Super Luxury piece-by-piece (≤ 10)   | 10      |
///
/// See pp.160–161.
pub fn max_procurable_fixer_rank(rank: u8) -> u8 {
    // See pp.160–161 Operator Rank ladder.
    match rank {
        0 => 0,
        1 | 2 => 2,
        3 | 4 => 4,
        5 | 6 => 6,
        7 | 8 => 8,
        9 => 9,
        _ => 10, // Rank 10+
    }
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
            id: CharacterId(Uuid::from_u128(0xBB)),
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 6,
                body: 7,
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

    /// Per p.159: the Fixer's Operator rank equals their role_rank.
    #[test]
    fn test_operator_rank_for_fixer() {
        let character = make_character(Role::Fixer, 7);
        assert_eq!(operator_rank(&character), 7);
    }

    /// Non-Fixer roles do not have the Operator ability; rank must be 0. See p.159.
    #[test]
    fn test_operator_rank_zero_for_non_fixer() {
        let character = make_character(Role::Solo, 7);
        assert_eq!(operator_rank(&character), 0);
    }

    /// Haggle discount multiplier must match the rulebook table at every tier.
    ///
    /// - Rank 0  → 100 (ability not active).
    /// - Ranks 1–2 → 90 (10% off per p.160).
    /// - Ranks 3–8 → 100 (non-percentage haggle effects; see module doc).
    /// - Rank 9  → 80 (20% off per p.161).
    /// - Rank 10 → 100 (double-pay mechanic, not an item discount per p.161).
    ///
    /// See pp.160–161.
    #[test]
    fn test_operator_discount_at_each_tier() {
        // Rank 0 — ability inactive.
        assert_eq!(operator_price_multiplier(0), 100, "rank 0 must be 100");

        // Ranks 1–2: 10% off → multiplier 90. See p.160.
        assert_eq!(operator_price_multiplier(1), 90, "rank 1 must be 90");
        assert_eq!(operator_price_multiplier(2), 90, "rank 2 must be 90");

        // Ranks 3–4: bulk-buy deal — no item % discount. See p.160.
        assert_eq!(operator_price_multiplier(3), 100, "rank 3 must be 100");
        assert_eq!(operator_price_multiplier(4), 100, "rank 4 must be 100");

        // Ranks 5–6: job-pay bonus — no item % discount. See p.160.
        assert_eq!(operator_price_multiplier(5), 100, "rank 5 must be 100");
        assert_eq!(operator_price_multiplier(6), 100, "rank 6 must be 100");

        // Ranks 7–8: deferred-payment deal — no item % discount. See p.161.
        assert_eq!(operator_price_multiplier(7), 100, "rank 7 must be 100");
        assert_eq!(operator_price_multiplier(8), 100, "rank 8 must be 100");

        // Rank 9: 20% off → multiplier 80. See p.161.
        assert_eq!(operator_price_multiplier(9), 80, "rank 9 must be 80");

        // Rank 10: double-pay on Dangerous Jobs — no item % discount. See p.161.
        assert_eq!(operator_price_multiplier(10), 100, "rank 10 must be 100");

        // Verify discounted_price arithmetic for the two active discount tiers.
        // Rank 1: 500 eb × 90% = 450 eb.
        assert_eq!(discounted_price(1, Eurobucks(500)), Eurobucks(450));
        // Rank 9: 500 eb × 80% = 400 eb.
        assert_eq!(discounted_price(9, Eurobucks(500)), Eurobucks(400));
        // Rank 3: no discount → 500 eb stays 500 eb.
        assert_eq!(discounted_price(3, Eurobucks(500)), Eurobucks(500));
    }

    /// Reach ladder: `max_procurable_fixer_rank` must match pp.160–161.
    #[test]
    fn test_max_procurable_fixer_rank() {
        // Rank 0 — no Operator ability → cannot procure anything via the ladder.
        assert_eq!(max_procurable_fixer_rank(0), 0);

        // Ranks 1–2: Cheap/Everyday items (min_fixer_rank ≤ 2). See p.160.
        assert_eq!(max_procurable_fixer_rank(1), 2);
        assert_eq!(max_procurable_fixer_rank(2), 2);

        // Ranks 3–4: up to Expensive (min_fixer_rank ≤ 4). See p.160.
        assert_eq!(max_procurable_fixer_rank(3), 4);
        assert_eq!(max_procurable_fixer_rank(4), 4);

        // Ranks 5–6: Super Luxury via Night Market (min_fixer_rank ≤ 6). See p.160.
        assert_eq!(max_procurable_fixer_rank(5), 6);
        assert_eq!(max_procurable_fixer_rank(6), 6);

        // Ranks 7–8: Very Expensive piece-by-piece (min_fixer_rank ≤ 8). See p.161.
        assert_eq!(max_procurable_fixer_rank(7), 8);
        assert_eq!(max_procurable_fixer_rank(8), 8);

        // Rank 9: Luxury piece-by-piece (min_fixer_rank ≤ 9). See p.161.
        assert_eq!(max_procurable_fixer_rank(9), 9);

        // Rank 10: Super Luxury piece-by-piece (min_fixer_rank ≤ 10). See p.161.
        assert_eq!(max_procurable_fixer_rank(10), 10);
    }
}
