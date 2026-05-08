//! Maker (Tech Role Ability) — WP-512.
//!
//! **Rulebook reference:** pp.147–149.
//!
//! ## Overview
//!
//! The Tech's Role Ability is **Maker**. Whenever a Tech increases their Maker
//! Rank by 1, they gain 1 Rank in two different Maker Specialties of their
//! choice (Field Expertise, Upgrade Expertise, Fabrication Expertise, or
//! Invention Expertise). See p.147.
//!
//! ## The four sub-abilities (p.147–148)
//!
//! | Expertise       | Description                                                    |
//! |-----------------|----------------------------------------------------------------|
//! | Field           | Jury-rig repairs; add Rank to relevant Tech skill checks (p.147). |
//! | Upgrade         | Modify/improve an existing item (p.148).                       |
//! | Fabrication     | Craft an item from scratch using a spec or blueprint (p.148).  |
//! | Invention       | Invent an entirely new item/upgrade (pp.148–149).              |
//!
//! ## Crafting roll (pp.147–149)
//!
//! All four expertises share the same roll formula:
//!
//! > **TECH + the TECH Skill typically used to repair the item + Rank in the
//! > relevant Expertise + 1d10 vs DV**
//!
//! The DV and time cost are read from the "Upgrade/Fabricate/Invent DV/Time"
//! table on p.149, keyed on the item's price category. This module exposes
//! that table through [`maker_dv`] and the material cost through
//! [`maker_cost`].
//!
//! ## What this module provides
//!
//! - [`maker_rank`] — effective Maker rank for a character (0 for non-Techs).
//! - [`MakerExpertise`] — the four sub-ability discriminants.
//! - [`ItemDifficulty`] — price-category tiers used as DV/cost keys.
//! - [`maker_dv`] — DV lookup per expertise × difficulty (p.149).
//! - [`maker_cost`] — material cost per expertise × difficulty (p.149).
//!
//! ## Out-of-scope for this WP
//!
//! The full crafting [`Resolution`](crate::resolution::Resolution) (rolling
//! dice, checking success/failure, applying item state changes) is deferred
//! to a later WP. The caller is expected to assemble a
//! `TECH + skill_rank + maker_rank + d10` roll and compare it against the
//! [`DV`](crate::types::DV) returned by [`maker_dv`]. This is documented
//! here so a future WP can wrap it cleanly without re-reading the spec.
//!
//! See pp.147–149.

use crate::character::data::Role;
use crate::character::Character;
use crate::types::{Eurobucks, DV};
use serde::{Deserialize, Serialize};

// ── Rank accessor ──────────────────────────────────────────────────────────────

/// Returns the character's effective Maker rank.
///
/// Per p.147, Maker is the Tech's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Tech`].
/// For any other role the ability does not apply; this function returns `0`
/// so callers can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::maker::maker_rank;
/// # use cpr_rules::character::Character;
/// // See test_maker_rank_for_tech.
/// ```
///
/// See p.147.
pub fn maker_rank(character: &Character) -> u8 {
    // See p.147 — Maker belongs exclusively to the Tech role.
    if character.role == Role::Tech {
        character.role_rank
    } else {
        0
    }
}

// ── Sub-ability discriminant ───────────────────────────────────────────────────

/// One of the four Maker sub-abilities a Tech can invest in.
///
/// Per p.147: "Whenever a Tech increases their Maker Rank by 1, they gain 1
/// Rank in two different Maker Specialties (Field Expertise, Upgrade
/// Expertise, Fabrication Expertise, or Invention Expertise) of their choice."
///
/// See pp.147–148.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MakerExpertise {
    /// **Field Expertise** (p.147) — jury-rig repairs and adding Rank to
    /// Basic Tech, Cybertech, Electronics/Security Tech, Weaponstech, Land,
    /// Sea, or Air Vehicle Tech Skill Checks made for non-Maker purposes.
    Field,
    /// **Upgrade Expertise** (p.148) — improve an existing item in one of
    /// several ways (lower Humanity Loss, add option slots, conceal a
    /// weapon, raise SP, etc.).
    Upgrade,
    /// **Fabrication Expertise** (p.148) — craft an existing item or one
    /// invented by the Tech from materials whose price tier is one step below
    /// the finished item. Roll TECH + the associated TECH Skill + Rank + 1d10
    /// vs the DV for the item's price category.
    Fabrication,
    /// **Invention Expertise** (pp.148–149) — invent an entirely new item or
    /// upgrade that does not yet exist in the catalog. Once invented, the item
    /// can be fabricated using Fabrication Expertise or Upgrade Expertise.
    Invention,
}

// ── Item difficulty (price-category tiers used for DV / cost) ─────────────────

/// Price-category tier used to determine DV and material cost for Maker actions.
///
/// The Upgrade/Fabricate/Invent DV/Time table on p.149 has entries from
/// Cheap/Everyday through Super Luxury. This WP surfaces the tiers that the
/// WP-512 acceptance tests exercise; `Custom` covers the Invention path where
/// the GM assigns a price category (lowest is Expensive per p.149).
///
/// See p.149.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemDifficulty {
    /// Common gear — Cheap or Everyday tier. DV 9, 1 hour. See p.149.
    Everyday,
    /// Mid-tier — Expensive price category (500 eb). DV 21, 1 week. See p.149.
    Expensive,
    /// Top-tier — Very Expensive price category (1,000 eb). DV 24, 2 weeks. See p.149.
    VeryExpensive,
    /// Custom invention — price category assigned by the GM; the minimum the
    /// GM may assign is Expensive (p.149). DV 29 here models the Luxury/Super
    /// Luxury tier used for invented items with maximum complexity. See p.149.
    Custom,
}

// ── DV table ──────────────────────────────────────────────────────────────────

/// Returns the Difficulty Value for a Maker action at the given difficulty tier.
///
/// All four [`MakerExpertise`] variants use the **same** DV table on p.149
/// ("Upgrade/Fabricate/Invent DV/Time"). The DV is determined solely by the
/// price category of the item being repaired, upgraded, fabricated, or
/// invented — not by which sub-ability is used. The `expertise` parameter is
/// accepted for API clarity and symmetry with [`maker_cost`], where it
/// *does* affect the result (material costs differ per expertise).
///
/// | Difficulty      | DV  | Source (p.149) |
/// |-----------------|-----|----------------|
/// | Everyday        | 9   | Cheap/Everyday row |
/// | Expensive       | 21  | Expensive row   |
/// | VeryExpensive   | 24  | Very Expensive row |
/// | Custom          | 29  | Luxury row (GM-assigned minimum for inventions) |
///
/// See pp.147–149.
pub fn maker_dv(_expertise: MakerExpertise, item_difficulty: ItemDifficulty) -> DV {
    // See p.149 — Upgrade/Fabricate/Invent DV/Time table.
    // The DV is the same regardless of which expertise sub-ability is used.
    match item_difficulty {
        // Cheap/Everyday → DV 9.
        ItemDifficulty::Everyday => DV(9),
        // Costly → DV 13  (not exposed as a variant in this WP's API).
        // Premium → DV 17 (not exposed as a variant in this WP's API).
        // Expensive → DV 21.
        ItemDifficulty::Expensive => DV(21),
        // Very Expensive → DV 24.
        ItemDifficulty::VeryExpensive => DV(24),
        // Luxury / Super Luxury → DV 29. Custom inventions use this tier
        // (the table shows Luxury and Super Luxury both at DV 29). See p.149.
        ItemDifficulty::Custom => DV(29),
    }
}

// ── Cost table ────────────────────────────────────────────────────────────────

/// Returns the material cost in Eurobucks for a Maker action.
///
/// Material costs differ between sub-abilities:
///
/// - **Field Expertise** (p.147): jury-rig uses no raw material cost listed;
///   the Tech just needs their tools. This function returns `Eurobucks(0)` for
///   [`MakerExpertise::Field`] as a canonical sentinel — the GM may impose a
///   nominal materials cost in play, but the rulebook specifies none. Deviation
///   noted in PR.
///
/// - **Upgrade Expertise** (p.148): "purchase materials of the same price
///   category of the item being upgraded". Upgrade cost = item price tier.
///
/// - **Fabrication Expertise** (p.148): "purchase materials of one price
///   category lower than the price category of the item being fabricated
///   (except for Super Luxury items, which require materials equal to half their
///   Price to fabricate)". Fabrication cost = one tier below.
///
/// - **Invention Expertise** (pp.148–149): the GM sets the price category;
///   RAW says the cost to *invent* uses the same TECH roll but no explicit
///   materials cost is listed (materials are paid at fabrication time). This
///   function returns the Fabrication-equivalent cost (one tier below) as a
///   conservative estimate; the GM will adjudicate the final figure.
///
/// The cost ladder per p.149 / p.339:
///
/// | Difficulty    | Tier cost | Upgrade cost | Fabrication cost |
/// |---------------|-----------|--------------|-----------------|
/// | Everyday      | 20 eb     | 20 eb        | 10 eb (Cheap)   |
/// | Expensive     | 500 eb    | 500 eb       | 100 eb (Premium)|
/// | VeryExpensive | 1,000 eb  | 1,000 eb     | 500 eb (Expensive)|
/// | Custom        | 5,000 eb  | 5,000 eb     | 1,000 eb (V.Exp)|
///
/// See pp.147–149.
pub fn maker_cost(expertise: MakerExpertise, item_difficulty: ItemDifficulty) -> Eurobucks {
    // See p.148–149.
    match expertise {
        // Field Expertise — no explicit materials cost in the rulebook. See p.147.
        MakerExpertise::Field => Eurobucks(0),

        // Upgrade Expertise — materials at the same price tier as the item. See p.148.
        MakerExpertise::Upgrade => tier_cost(item_difficulty),

        // Fabrication Expertise — materials one price tier below the item. See p.148.
        MakerExpertise::Fabrication => one_tier_below(item_difficulty),

        // Invention Expertise — fabrication cost applies when making the
        // prototype (the GM may adjust). See pp.148–149.
        MakerExpertise::Invention => one_tier_below(item_difficulty),
    }
}

/// Returns the canonical Eurobuck cost for the given item difficulty tier.
///
/// Ladder per pp.339, 376 (Night Market canonical costs):
/// - Everyday   → 20 eb
/// - Expensive  → 500 eb
/// - VeryExpensive → 1,000 eb
/// - Custom     → 5,000 eb (Luxury tier, pp.376, 149)
///
/// See p.339.
fn tier_cost(difficulty: ItemDifficulty) -> Eurobucks {
    match difficulty {
        ItemDifficulty::Everyday => Eurobucks(20),
        ItemDifficulty::Expensive => Eurobucks(500),
        ItemDifficulty::VeryExpensive => Eurobucks(1_000),
        ItemDifficulty::Custom => Eurobucks(5_000),
    }
}

/// Returns the material cost one price tier below the given difficulty.
///
/// Per p.148: Fabrication requires "materials of one price category lower".
/// The canonical tier ladder (p.339) is: Cheap(10) → Everyday(20) →
/// Costly(50) → Premium(100) → Expensive(500) → Very Expensive(1,000) →
/// Luxury(5,000) → Super Luxury(10,000).
///
/// Mapping for the exposed [`ItemDifficulty`] variants:
/// - Everyday    → Cheap tier → 10 eb
/// - Expensive   → Premium tier → 100 eb
/// - VeryExpensive → Expensive tier → 500 eb
/// - Custom      → Very Expensive tier → 1,000 eb
///
/// See pp.148–149, 339.
fn one_tier_below(difficulty: ItemDifficulty) -> Eurobucks {
    match difficulty {
        // Everyday → one below is Cheap (10 eb). See p.339.
        ItemDifficulty::Everyday => Eurobucks(10),
        // Expensive → one below is Premium (100 eb). See p.339.
        ItemDifficulty::Expensive => Eurobucks(100),
        // Very Expensive → one below is Expensive (500 eb). See p.339.
        ItemDifficulty::VeryExpensive => Eurobucks(500),
        // Custom (Luxury) → one below is Very Expensive (1,000 eb). See p.339.
        ItemDifficulty::Custom => Eurobucks(1_000),
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

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

    /// Construct a minimal [`Character`] with the given role and role rank.
    fn make_character(role: Role, role_rank: u8) -> crate::character::Character {
        crate::character::Character {
            id: CharacterId(Uuid::from_u128(0xAB)),
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 7,
                cool: 5,
                will: 6,
                luck: 5,
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
                current_hp: 30,
                max_hp: 30,
                seriously_wounded_threshold: 15,
                death_save_base: 6,
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

    // ── Acceptance tests (WP-512) ──────────────────────────────────────────────

    #[test]
    fn test_maker_rank_for_tech() {
        // Per p.147: a Tech character's Maker rank equals their role_rank.
        let character = make_character(Role::Tech, 5);
        assert_eq!(maker_rank(&character), 5);
    }

    #[test]
    fn test_maker_rank_zero_for_non_tech() {
        // Non-Tech roles do not have the Maker ability — rank is 0. See p.147.
        let character = make_character(Role::Solo, 5);
        assert_eq!(maker_rank(&character), 0);
    }

    #[test]
    fn test_maker_dv_field_everyday() {
        // p.149: Cheap/Everyday row → DV 9.
        let dv = maker_dv(MakerExpertise::Field, ItemDifficulty::Everyday);
        assert_eq!(
            dv,
            DV(9),
            "Field Expertise + Everyday difficulty must be DV 9 (p.149)"
        );
    }

    #[test]
    fn test_maker_dv_upgrade_expensive() {
        // p.149: Expensive row → DV 21.
        let dv = maker_dv(MakerExpertise::Upgrade, ItemDifficulty::Expensive);
        assert_eq!(
            dv,
            DV(21),
            "Upgrade Expertise + Expensive difficulty must be DV 21 (p.149)"
        );
    }

    #[test]
    fn test_maker_cost_scales_with_difficulty() {
        // Costs must be strictly increasing with difficulty for Upgrade Expertise.
        // See p.148–149: materials = same price tier as the item.
        let everyday = maker_cost(MakerExpertise::Upgrade, ItemDifficulty::Everyday);
        let expensive = maker_cost(MakerExpertise::Upgrade, ItemDifficulty::Expensive);
        let very_expensive = maker_cost(MakerExpertise::Upgrade, ItemDifficulty::VeryExpensive);
        let custom = maker_cost(MakerExpertise::Upgrade, ItemDifficulty::Custom);

        assert!(
            everyday < expensive,
            "Everyday cost ({everyday:?}) must be less than Expensive cost ({expensive:?})"
        );
        assert!(
            expensive < very_expensive,
            "Expensive cost ({expensive:?}) must be less than VeryExpensive cost ({very_expensive:?})"
        );
        assert!(
            very_expensive < custom,
            "VeryExpensive cost ({very_expensive:?}) must be less than Custom cost ({custom:?})"
        );
    }

    // ── Additional coverage ────────────────────────────────────────────────────

    #[test]
    fn test_maker_rank_zero_for_netrunner() {
        // Any non-Tech role returns 0. See p.147.
        let character = make_character(Role::Netrunner, 8);
        assert_eq!(maker_rank(&character), 0);
    }

    #[test]
    fn test_maker_rank_for_tech_rank_10() {
        // Maximum rank. See p.147.
        let character = make_character(Role::Tech, 10);
        assert_eq!(maker_rank(&character), 10);
    }

    #[test]
    fn test_maker_dv_fabrication_very_expensive() {
        // p.149: Very Expensive row → DV 24.
        let dv = maker_dv(MakerExpertise::Fabrication, ItemDifficulty::VeryExpensive);
        assert_eq!(
            dv,
            DV(24),
            "Fabrication + VeryExpensive must be DV 24 (p.149)"
        );
    }

    #[test]
    fn test_maker_dv_invention_custom() {
        // p.149: Luxury/Super Luxury → DV 29.
        let dv = maker_dv(MakerExpertise::Invention, ItemDifficulty::Custom);
        assert_eq!(dv, DV(29), "Invention + Custom must be DV 29 (p.149)");
    }

    #[test]
    fn test_maker_dv_same_for_all_expertises() {
        // All four expertises use the same DV table (p.149). See module docs.
        let diff = ItemDifficulty::Expensive;
        let field = maker_dv(MakerExpertise::Field, diff);
        let upgrade = maker_dv(MakerExpertise::Upgrade, diff);
        let fabrication = maker_dv(MakerExpertise::Fabrication, diff);
        let invention = maker_dv(MakerExpertise::Invention, diff);
        assert_eq!(field, upgrade);
        assert_eq!(upgrade, fabrication);
        assert_eq!(fabrication, invention);
    }

    #[test]
    fn test_maker_cost_fabrication_one_tier_below() {
        // Fabrication materials = one tier lower than the item tier. See p.148.
        // Expensive item → Premium materials → 100 eb.
        let cost = maker_cost(MakerExpertise::Fabrication, ItemDifficulty::Expensive);
        assert_eq!(cost, Eurobucks(100));
    }

    #[test]
    fn test_maker_cost_upgrade_same_tier() {
        // Upgrade materials = same tier as the item. See p.148.
        // Expensive item → Expensive materials → 500 eb.
        let cost = maker_cost(MakerExpertise::Upgrade, ItemDifficulty::Expensive);
        assert_eq!(cost, Eurobucks(500));
    }

    #[test]
    fn test_maker_cost_field_zero() {
        // Field Expertise has no explicit materials cost in the rulebook. See p.147.
        let cost = maker_cost(MakerExpertise::Field, ItemDifficulty::VeryExpensive);
        assert_eq!(cost, Eurobucks(0));
    }

    #[test]
    fn test_item_difficulty_serde_round_trip() {
        // All ItemDifficulty variants must survive a RON round-trip.
        let variants = [
            ItemDifficulty::Everyday,
            ItemDifficulty::Expensive,
            ItemDifficulty::VeryExpensive,
            ItemDifficulty::Custom,
        ];
        for v in variants {
            let s = ron::ser::to_string(&v).expect("serialize");
            let r: ItemDifficulty = ron::de::from_str(&s).expect("deserialize");
            assert_eq!(v, r);
        }
    }

    #[test]
    fn test_maker_expertise_serde_round_trip() {
        // All MakerExpertise variants must survive a RON round-trip.
        let variants = [
            MakerExpertise::Field,
            MakerExpertise::Upgrade,
            MakerExpertise::Fabrication,
            MakerExpertise::Invention,
        ];
        for v in variants {
            let s = ron::ser::to_string(&v).expect("serialize");
            let r: MakerExpertise = ron::de::from_str(&s).expect("deserialize");
            assert_eq!(v, r);
        }
    }
}
