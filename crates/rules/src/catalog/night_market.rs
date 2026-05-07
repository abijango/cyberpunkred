//! Night Market gear catalog (WP-212).
//!
//! The Night Market Appendix on pp.340–380 of *Cyberpunk RED* lists every
//! item a Fixer might procure for the Crew. This catalog covers the slice
//! that no dedicated catalog owns: gadgets and gear (Master Gear List
//! pp.351–355), ammunition (pp.345–347), weapon attachments (pp.343–344),
//! fashion (p.356), cyberdeck hardware (p.368), home defenses
//! (pp.373–375), services and entertainment (p.376), and lifestyle &
//! housing (pp.377–379).
//!
//! Items already owned by other Phase 2 catalogs are intentionally
//! omitted:
//! - Melee, Ranged, and Exotic Weapons (pp.340–349) — owned by the
//!   weapons catalog WP.
//! - Armor (p.350) — owned by the armor catalog WP.
//! - Cyberware including Fashionware, Cyberoptics, Cyberaudio,
//!   Internal/External Body Cyberware, Cyberlimbs, Borgware
//!   (pp.358–367) — owned by the cyberware catalog WP.
//! - Street Drugs (p.357) — owned by the drugs catalog WP.
//! - Netrunner Programs and Black ICE (pp.368–371), and NET Architecture
//!   purchase tables (p.372) — owned by the programs catalog WP-208.
//!
//! Each catalog row also records the Fixer Operator rank (p.159) at which
//! the item becomes always-sourceable: Cheap and Everyday at Rank 1, up
//! to Expensive at Rank 3, Super Luxury at Rank 5 (only via a Night
//! Market the Fixer helps organise). Items not gated by the Operator
//! ladder use rank 1 — they're available to any street-level character.

use crate::catalog::Catalog;
use crate::effects::EffectModifier;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// MarketCategory
// ---------------------------------------------------------------------------

/// The Night Market category an item is shelved under.
///
/// The categories are derived from the Random Treasure Table on p.339
/// (which groups items by streetwise headings: Food and Drugs, Personal
/// Electronics, Survival Gear, …) and the explicit section headings of
/// the Night Market Appendix (pp.340–380). They drive shop-window
/// rendering and roleplaying ("the food stall", "the electronics stall",
/// "the fence selling military hardware") more than they drive
/// resolution mechanics.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MarketCategory {
    // -- Electronics — split by price band per the p.339 treasure table.
    /// Pocket-sized electronics: cell phone, smart glasses, virtuality
    /// goggles. Treasure table p.339; gear list pp.351–355.
    PersonalElectronics,
    /// Mid-tier electronics: medscanner, techscanner, braindance viewer.
    /// Treasure table p.339; gear list pp.351–355.
    MediumElectronics,
    /// Consumer-oriented electronics: computer, music player, drum
    /// synthesizer. Treasure table p.339; gear list pp.351–355.
    ConsumerElectronics,

    // -- Food & drink — split by quality per pp.376–377.
    /// Cheap-tier food: kibble, food stick, MRE. See pp.351, 376, 377.
    LowFood,
    /// Mid-tier food: a Good Bar drink, an Everyday restaurant meal,
    /// fresh fruits and vegetables. See pp.351, 376, 377.
    GoodFood,
    /// High-tier food and drink: Excellent Bar drink, World Class
    /// restaurant meal, exotic produce. See pp.351, 376, 377.
    ExcellentFood,

    /// Lodging — hotel rooms, real estate rents, lifestyle packages.
    /// See pp.376, 377, 378.
    Lodging,

    /// Ammunition for the various ranged weapons. See pp.344–347.
    Ammunition,

    /// Services and entertainment, including cyberware installations,
    /// hospital treatments, professional services, Trauma Team
    /// subscriptions, taxis, movies, and braindance. See p.376.
    Services,

    /// General-purpose tools: lock picks, tech tools, medtech bags,
    /// tents, ropes, and the like. See pp.351, 354.
    Tools,

    /// Survival and outdoor gear that doesn't fit a more specific bucket:
    /// road flares, glow sticks, anti-smog masks, sleeping bags. See
    /// pp.351–355.
    SurvivalGear,

    /// Fashion outfits priced by the Fashion table on p.356.
    Fashion,

    /// Bolt-on upgrades for an existing item: weapon attachments
    /// (pp.343–344) and cyberdeck hardware (p.368).
    GadgetUpgrade,

    /// Pre-built home and facility defenses purchased to wall off a safe
    /// house or NET Architecture. See pp.373–375.
    HomeDefenses,
}

// ---------------------------------------------------------------------------
// NightMarketItem
// ---------------------------------------------------------------------------

/// One row in the Night Market catalog.
///
/// `id` is the catalog slug — the same string used as the `Catalog<T>`
/// lookup key, surfaced on the struct so callers handed a
/// `&NightMarketItem` can identify it without carrying the slug
/// separately. `price_eb` is denormalised from `price` for convenience
/// at point-of-sale; the loader enforces the two agree
/// (`PriceTier::canonical_cost`).
///
/// `effects` is `None` for purely flavour items (a road flare, a kibble
/// pack) and `Some(_)` for items that grant a mechanical bonus while
/// equipped or active (an Agent's `+2` to Library Search and Wardrobe &
/// Style, a Medscanner's `+2` to First Aid and Paramedic). The exact
/// modifiers are surfaced as [`EffectModifier`]s so the effect-system
/// hook points (§2.6) can apply them directly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NightMarketItem {
    /// Catalog slug — also the `Catalog<NightMarketItem>` key.
    pub id: String,
    /// Display name as printed in the rulebook.
    pub display_name: String,
    /// Which Night Market stall this item shelves under.
    pub category: MarketCategory,
    /// Canonical price tier from p.339 / p.376 / inline catalog entries.
    pub price: PriceTier,
    /// Eurobuck cost, denormalised from `price`. The loader enforces
    /// `price_eb == price.canonical_cost()`.
    pub price_eb: Eurobucks,
    /// Minimum Fixer Operator rank (p.159) at which this item is
    /// always-sourceable. Items not gated by the Operator ladder use 1.
    pub min_fixer_rank: u8,
    /// 1–3 sentence summary of the item, paraphrased from pp.340–380.
    pub description: String,
    /// Mechanical effects applied while the item is equipped or active.
    /// `None` for items that are purely flavour.
    pub effects: Option<Vec<EffectModifier>>,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/night_market.ron`.
///
/// The file is `(items: [ ... ])`. Each entry is a flat record with a
/// `slug` field (the `Catalog<T>` key) plus the [`NightMarketItem`]
/// fields. Decoupling the on-disk schema from `Catalog<T>` lets the
/// authored content stay readable.
#[derive(Debug, Deserialize)]
struct NightMarketFile {
    items: Vec<NightMarketFileEntry>,
}

/// One row in the on-disk Night Market catalog file.
#[derive(Debug, Deserialize)]
struct NightMarketFileEntry {
    slug: String,
    display_name: String,
    category: MarketCategory,
    price: PriceTier,
    price_eb: Eurobucks,
    min_fixer_rank: u8,
    description: String,
    effects: Option<Vec<EffectModifier>>,
}

/// Load the Night Market catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<NightMarketItem>`] keyed by slug. On
/// failure returns [`RulesError::CatalogLoadFailed`] carrying the file
/// path and a stringified description of the underlying I/O or parse
/// error.
///
/// The loader enforces three invariants:
/// 1. Slugs are unique within the file.
/// 2. `price_eb` equals `price.canonical_cost()` for every entry —
///    keeping the denormalised eurobuck value in lockstep with the
///    canonical tier.
/// 3. `min_fixer_rank` is in the inclusive range 1..=10. The Operator
///    ladder caps at rank 10 (p.142+).
pub fn load_night_market_catalog(path: &Path) -> Result<Catalog<NightMarketItem>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: NightMarketFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, NightMarketItem> = HashMap::with_capacity(parsed.items.len());
    for row in parsed.items {
        if row.price_eb != row.price.canonical_cost() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "item '{}' has price_eb {:?} but tier {:?} canonicalises to {:?}",
                    row.slug,
                    row.price_eb,
                    row.price,
                    row.price.canonical_cost()
                ),
            });
        }
        if !(1..=10).contains(&row.min_fixer_rank) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "item '{}' has min_fixer_rank {} outside 1..=10",
                    row.slug, row.min_fixer_rank
                ),
            });
        }
        let item = NightMarketItem {
            id: row.slug.clone(),
            display_name: row.display_name,
            category: row.category,
            price: row.price,
            price_eb: row.price_eb,
            min_fixer_rank: row.min_fixer_rank,
            description: row.description,
            effects: row.effects,
        };
        if entries.insert(row.slug.clone(), item).is_some() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate slug: '{}'", row.slug),
            });
        }
    }

    Ok(Catalog::new(entries))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Workspace-relative path to the canonical Night Market catalog file.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("night_market.ron");
        p
    }

    /// Acceptance: the catalog has a substantial number of entries
    /// (50+). The Night Market Appendix lists hundreds of distinct
    /// SKUs across pp.340–380; with weapons, armor, cyberware, drugs,
    /// and netrunner programs intentionally omitted, the remainder
    /// still comfortably clears 50.
    #[test]
    fn test_night_market_catalog_complete() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        assert!(
            cat.len() >= 50,
            "expected 50+ Night Market items per pp.340-380; got {}",
            cat.len()
        );
    }

    /// Acceptance: at least one Low / Good / Excellent food entry is
    /// present, and prices match the rulebook (p.376 services table
    /// for restaurant meals, p.377 lifestyle table for monthly food
    /// budgets).
    #[test]
    fn test_food_categories() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        let low = cat
            .iter()
            .find(|(_, item)| item.category == MarketCategory::LowFood)
            .map(|(_, item)| item)
            .expect("at least one LowFood entry expected");
        let good = cat
            .iter()
            .find(|(_, item)| item.category == MarketCategory::GoodFood)
            .map(|(_, item)| item)
            .expect("at least one GoodFood entry expected");
        let excellent = cat
            .iter()
            .find(|(_, item)| item.category == MarketCategory::ExcellentFood)
            .map(|(_, item)| item)
            .expect("at least one ExcellentFood entry expected");

        // Every food entry's price must agree with its tier.
        for item in [low, good, excellent] {
            assert_eq!(
                item.price_eb,
                item.price.canonical_cost(),
                "{} price_eb disagrees with tier",
                item.id
            );
        }

        // Spot-check known prices: a Cheap fast-food meal is 10eb
        // (p.376), an Everyday Good restaurant meal is 20eb (p.376),
        // an Excellent restaurant meal is 50eb Costly (p.376).
        let fast = cat
            .get("restaurant_meal_fast_food")
            .expect("fast food meal expected");
        assert_eq!(fast.price, PriceTier::Cheap);
        assert_eq!(fast.price_eb, Eurobucks(10));

        let good_meal = cat
            .get("restaurant_meal_good")
            .expect("good restaurant meal expected");
        assert_eq!(good_meal.price, PriceTier::Everyday);
        assert_eq!(good_meal.price_eb, Eurobucks(20));

        let excellent_meal = cat
            .get("restaurant_meal_excellent")
            .expect("excellent restaurant meal expected");
        assert_eq!(excellent_meal.price, PriceTier::Costly);
        assert_eq!(excellent_meal.price_eb, Eurobucks(50));
    }

    /// Acceptance: at least one lodging entry from p.376 (hotel
    /// rooms) and one from p.378 (real estate rentals) is present.
    #[test]
    fn test_lodging_entries() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        let lodging: Vec<&NightMarketItem> = cat
            .iter()
            .filter(|(_, item)| item.category == MarketCategory::Lodging)
            .map(|(_, item)| item)
            .collect();
        assert!(
            !lodging.is_empty(),
            "expected at least one Lodging entry from pp.376-378"
        );

        // Spot-check: Hotel, Per Night is 100eb (Premium) on p.376.
        let hotel = cat
            .get("hotel_per_night")
            .expect("standard hotel per-night entry expected");
        assert_eq!(hotel.category, MarketCategory::Lodging);
        assert_eq!(hotel.price, PriceTier::Premium);
        assert_eq!(hotel.price_eb, Eurobucks(100));
    }

    /// Acceptance: at least one service from pp.376–378 is present,
    /// and a known service has the documented price.
    #[test]
    fn test_services_entries() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        let services: Vec<&NightMarketItem> = cat
            .iter()
            .filter(|(_, item)| item.category == MarketCategory::Services)
            .map(|(_, item)| item)
            .collect();
        assert!(
            !services.is_empty(),
            "expected at least one Services entry from pp.376-378"
        );

        // Spot-check: Trauma Team (Silver) is 500eb/month (p.376).
        let trauma = cat
            .get("trauma_team_silver")
            .expect("Trauma Team Silver service expected");
        assert_eq!(trauma.category, MarketCategory::Services);
        assert_eq!(trauma.price, PriceTier::Expensive);
        assert_eq!(trauma.price_eb, Eurobucks(500));

        // Spot-check: a Taxi is 20eb Everyday (p.376).
        let taxi = cat.get("taxi").expect("Taxi service expected");
        assert_eq!(taxi.category, MarketCategory::Services);
        assert_eq!(taxi.price, PriceTier::Everyday);
        assert_eq!(taxi.price_eb, Eurobucks(20));
    }

    /// Acceptance: every item round-trips through RON serialisation
    /// (`ser` → `de`) with bit-identical equality, including the
    /// `effects` field.
    #[test]
    fn test_night_market_round_trip_ron() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        for (slug, item) in cat.iter() {
            let s = ron::ser::to_string(item)
                .unwrap_or_else(|e| panic!("'{slug}' must serialise: {e}"));
            let restored: NightMarketItem =
                ron::de::from_str(&s).unwrap_or_else(|e| panic!("'{slug}' must deserialise: {e}"));
            assert_eq!(*item, restored, "'{slug}' must round-trip via RON");
        }
    }

    /// Regression: every entry's `price_eb` matches
    /// `price.canonical_cost()` — the loader rejects mismatches but
    /// the test pins the invariant against accidental loader-bypass
    /// changes.
    #[test]
    fn test_price_eb_matches_tier() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        for (slug, item) in cat.iter() {
            assert_eq!(
                item.price_eb,
                item.price.canonical_cost(),
                "'{slug}' price_eb {:?} disagrees with tier {:?} canonical {:?}",
                item.price_eb,
                item.price,
                item.price.canonical_cost(),
            );
        }
    }

    /// Regression: every `min_fixer_rank` is within the Operator
    /// ladder's 1..=10 range.
    #[test]
    fn test_min_fixer_rank_in_range() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        for (slug, item) in cat.iter() {
            assert!(
                (1..=10).contains(&item.min_fixer_rank),
                "'{slug}' min_fixer_rank {} outside 1..=10",
                item.min_fixer_rank
            );
        }
    }

    /// Regression: ammunition entries are present (pp.345–347 lists
    /// 12 base ammunition types).
    #[test]
    fn test_ammunition_present() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        let count = cat
            .iter()
            .filter(|(_, item)| item.category == MarketCategory::Ammunition)
            .count();
        assert!(
            count >= 8,
            "expected 8+ Ammunition entries per pp.344-347; got {count}"
        );
        // Spot-check Basic Ammunition: 10eb Cheap (p.345).
        let basic = cat
            .get("basic_ammunition")
            .expect("Basic Ammunition expected");
        assert_eq!(basic.category, MarketCategory::Ammunition);
        assert_eq!(basic.price, PriceTier::Cheap);
    }

    /// Regression: a known item with mechanical effects (Agent gives
    /// +2 to Library Search and Wardrobe & Style — p.352) carries
    /// the modifiers in `effects`.
    #[test]
    fn test_effects_field_populated_for_known_item() {
        let cat = load_night_market_catalog(&catalog_path()).expect("catalog must load");
        let agent = cat.get("agent").expect("Agent gear entry expected");
        let mods = agent
            .effects
            .as_ref()
            .expect("Agent should carry SkillBonus modifiers per p.352");
        assert!(
            !mods.is_empty(),
            "Agent effects vec must not be empty per p.352"
        );
    }
}
