//! Core identifier and value types used across the rules engine.
//!
//! Everything here is `Copy` (small, by-value) and `Serialize`/`Deserialize`,
//! since these types appear in save files, RON content, and over-the-wire
//! messages. Identifiers are newtypes around `Uuid`; the rules crate never
//! generates a UUID itself — the surrounding system passes them in.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stable identifier for a player or NPC character.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct CharacterId(pub Uuid);

/// Stable identifier for any combat-grid entity (character, drone, vehicle, etc.).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct EntityId(pub Uuid);

/// Stable identifier for an NPC, distinct from the broader `CharacterId` so
/// callers can express "NPC-only" parameters at the type level.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct NpcId(pub Uuid);

/// Stable identifier for an `ActiveEffect` instance on a character's
/// `EffectStack`. See `IMPLEMENTATION_PLAN.md` §2.6.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct EffectInstanceId(pub Uuid);

/// Difficulty Value — the target a `STAT + Skill + d10` check must meet or beat.
///
/// See p.129 (Difficulty Values).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct DV(pub u8);

impl DV {
    /// "Most people can do this without thinking." See p.129.
    pub const SIMPLE: DV = DV(9);
    /// "Most people can do without a lot of special training." See p.129.
    pub const EVERYDAY: DV = DV(13);
    /// "Difficult to accomplish without training or natural talent." See p.129.
    pub const DIFFICULT: DV = DV(15);
    /// "Actual training; the user is a professional." See p.129.
    pub const PROFESSIONAL: DV = DV(17);
    /// "Top of the field." See p.129.
    pub const HEROIC: DV = DV(21);
    /// "Olympian mettle." See p.129.
    pub const INCREDIBLE: DV = DV(24);
}

/// Currency. `i64` accommodates negative balances (debts, refunds, in-flight
/// transactions) without saturating arithmetic surprises.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
pub struct Eurobucks(pub i64);

/// Canonical price tier from the Night Market (pp.339, 376) and inline catalog
/// listings throughout pp.340–380.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum PriceTier {
    /// 10eb. e.g. Poor Quality Alcohol, MRE, Road Flare. See p.339.
    Cheap,
    /// 20eb. e.g. Bags of Prepak, Duct Tape, Movie ticket. See pp.339, 376.
    Everyday,
    /// 50eb. e.g. Computer, Light Melee Weapon, Handcuffs. See p.339.
    Costly,
    /// 100eb. e.g. Bows, Heavy Melee Weapon, Medtech Bag. See p.339.
    Premium,
    /// 500eb. e.g. Cyberdeck, Sniper Rifle, Standard Bodysculpting. See pp.339, 376.
    Expensive,
    /// 1,000eb. e.g. Medscanner, Exotic Bodysculpting. See pp.339, 376.
    VeryExpensive,
    /// 5,000eb. e.g. World Class Professional Services. See p.376.
    Luxury,
    /// 10,000eb. e.g. Malorian Arms 3516, Aerodyne. See pp.340–380 catalog.
    SuperLuxury,
}

impl PriceTier {
    /// Returns the canonical Eurobuck cost for this tier.
    ///
    /// Values verified against the rulebook: the `Random Treasure Table` on
    /// p.339 (Cheap–Very Expensive), the `Services and Entertainment` table on
    /// p.376 (Luxury), and inline catalog entries on pp.340–380 (Super Luxury).
    pub fn canonical_cost(self) -> Eurobucks {
        match self {
            PriceTier::Cheap => Eurobucks(10),
            PriceTier::Everyday => Eurobucks(20),
            PriceTier::Costly => Eurobucks(50),
            PriceTier::Premium => Eurobucks(100),
            PriceTier::Expensive => Eurobucks(500),
            PriceTier::VeryExpensive => Eurobucks(1_000),
            PriceTier::Luxury => Eurobucks(5_000),
            PriceTier::SuperLuxury => Eurobucks(10_000),
        }
    }
}

/// The ten Cyberpunk RED character stats. See pp.72–73 for definitions and
/// pp.79–80 for derived statistics.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum Stat {
    Int,
    Ref,
    Dex,
    Tech,
    Cool,
    Will,
    Luck,
    Move,
    Body,
    Emp,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_copy<T: Copy>() {}
    fn assert_serde<T: Serialize + serde::de::DeserializeOwned>() {}

    #[test]
    fn test_dv_constants_match_book() {
        // p.129: Simple 9, Everyday 13, Difficult 15, Professional 17,
        //        Heroic 21, Incredible 24.
        assert_eq!(DV::SIMPLE, DV(9));
        assert_eq!(DV::EVERYDAY, DV(13));
        assert_eq!(DV::DIFFICULT, DV(15));
        assert_eq!(DV::PROFESSIONAL, DV(17));
        assert_eq!(DV::HEROIC, DV(21));
        assert_eq!(DV::INCREDIBLE, DV(24));
    }

    #[test]
    fn test_price_tier_canonical_costs() {
        // Verified against pp.339, 376, and the pp.340–380 catalog.
        assert_eq!(PriceTier::Cheap.canonical_cost(), Eurobucks(10));
        assert_eq!(PriceTier::Everyday.canonical_cost(), Eurobucks(20));
        assert_eq!(PriceTier::Costly.canonical_cost(), Eurobucks(50));
        assert_eq!(PriceTier::Premium.canonical_cost(), Eurobucks(100));
        assert_eq!(PriceTier::Expensive.canonical_cost(), Eurobucks(500));
        assert_eq!(PriceTier::VeryExpensive.canonical_cost(), Eurobucks(1_000));
        assert_eq!(PriceTier::Luxury.canonical_cost(), Eurobucks(5_000));
        assert_eq!(PriceTier::SuperLuxury.canonical_cost(), Eurobucks(10_000));

        // Strict monotonicity — a higher tier always costs more.
        let ladder = [
            PriceTier::Cheap,
            PriceTier::Everyday,
            PriceTier::Costly,
            PriceTier::Premium,
            PriceTier::Expensive,
            PriceTier::VeryExpensive,
            PriceTier::Luxury,
            PriceTier::SuperLuxury,
        ];
        for pair in ladder.windows(2) {
            assert!(
                pair[0].canonical_cost() < pair[1].canonical_cost(),
                "tier ladder must be strictly increasing: {:?} >= {:?}",
                pair[0],
                pair[1],
            );
        }
    }

    #[test]
    fn test_all_types_are_copy_and_serde() {
        // Compile-time checks — if any of these stop holding, the file won't build.
        assert_copy::<CharacterId>();
        assert_copy::<EntityId>();
        assert_copy::<NpcId>();
        assert_copy::<EffectInstanceId>();
        assert_copy::<DV>();
        assert_copy::<Eurobucks>();
        assert_copy::<PriceTier>();
        assert_copy::<Stat>();

        assert_serde::<CharacterId>();
        assert_serde::<EntityId>();
        assert_serde::<NpcId>();
        assert_serde::<EffectInstanceId>();
        assert_serde::<DV>();
        assert_serde::<Eurobucks>();
        assert_serde::<PriceTier>();
        assert_serde::<Stat>();
    }
}
