//! Weapon catalog (WP-202).
//!
//! Defines the [`Weapon`] entry shape — every weapon listed in the rulebook
//! Friday Night Firefight section (pp.170–179) and the Night Market
//! appendix (pp.340–344) — together with the supporting type ladder
//! ([`WeaponKind`], [`RangedKind`], [`MeleeKind`], [`WeaponFeature`],
//! [`DamageDice`], [`DieKind`], [`Magazine`], [`RangeBand`]) and the RON
//! loader [`load_weapons_catalog`].
//!
//! Rulebook references:
//! - **pp.170–171:** ranged weapon table (type, skill, single-shot damage,
//!   magazine, ROF, hands, conceal, alt-fire features, cost).
//! - **p.172:** "Single Shot DVs Based on Range" — the per-weapon-type
//!   range bands for ranged combat.
//! - **p.173:** "Autofire DVs Based on Range" — the SMG / Assault Rifle
//!   autofire range table, and the autofire damage cap (3 for SMGs, 4
//!   for Assault Rifles).
//! - **p.174:** Arrows, Suppressive Fire, Shotgun Shells, Explosives.
//! - **pp.175–176:** melee weapon table (Light, Medium, Heavy, Very Heavy)
//!   plus the resolution rules. Per p.176 melee weapons ignore half armor
//!   (round up); Very Heavy Melee Weapons can't attack twice in an Action.
//! - **pp.176–178:** Brawling, Grappling, Martial Arts (excluded from this
//!   catalog — Brawling and Martial Arts use BODY-scaled damage tables,
//!   not weapon entries).
//! - **pp.340–344:** Night Market appendix — same tables, plus the brand
//!   examples on p.342 and the Clip Chart / Ammunition section on p.344.
//!
//! The catalog file is `content/catalogs/weapons.ron`; the loader expects
//! one entry per slug and rejects duplicates.
//!
//! ## Open-ended catalog (vs. the closed [`crate::catalog::SkillId`])
//!
//! Unlike skills, weapons are **not** a closed enum: the rulebook lists
//! generic weapon *types* (Medium Pistol, Heavy SMG, …) on p.171 and only
//! gives brand names as flavour examples on p.342 ("Federated Arms X-9mm",
//! "Militech 'Dragon'"). Exotic weapons (p.347) are explicitly "of the
//! GM's choice". The catalog therefore keys on string slugs (the generic
//! type's normalised name) so DLC, brand variants, and homebrew can be
//! added without expanding a closed enum.

use crate::catalog::Catalog;
use crate::character::data::{AmmoKind, WeaponId};
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use crate::SkillId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Damage dice
// ---------------------------------------------------------------------------

/// Damage dice spec for a weapon's single-shot damage. See pp.171, 175.
///
/// In *Cyberpunk RED* every weapon's listed single-shot damage is `NdK`,
/// where `N` is the dice count and `K` is `D6` for single-shot weapons
/// and Brawling/Martial Arts (pp.171, 175–178). `D10` is included for
/// completeness — the `Death Save Modifier` and check rolls use d10s
/// elsewhere in the engine — but no rulebook weapon currently uses it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageDice {
    /// Number of dice rolled.
    pub n: u8,
    /// Die size.
    pub die: DieKind,
}

/// Die size used by [`DamageDice`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DieKind {
    /// d6 — every weapon's listed damage on pp.171, 175 is in d6s.
    D6,
    /// d10 — reserved for future weapon entries; no rulebook weapon uses
    /// it as of v1.25.
    D10,
}

// ---------------------------------------------------------------------------
// Weapon kind / sub-kind
// ---------------------------------------------------------------------------

/// Top-level discriminator for a weapon. See pp.170–179.
///
/// Ranged and melee weapons split into typed sub-kinds. Thrown weapons
/// (grenades thrown by hand, improvised throwables — p.177) are a flat
/// variant. Exotic weapons (p.347) are flat too because the rulebook
/// itself keeps them as "GM's choice" without further sub-typing.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponKind {
    /// A ranged weapon — see [`RangedKind`] for the listed types on p.171.
    Ranged(RangedKind),
    /// A melee weapon — see [`MeleeKind`] for the four classifications
    /// on p.175.
    Melee(MeleeKind),
    /// A weapon used by being thrown. See p.177 (Throw); grenades thrown
    /// by hand are also covered there.
    Thrown,
    /// An exotic ranged weapon of the GM's choice. See p.347.
    ExoticRanged,
    /// An exotic melee weapon of the GM's choice. See p.347.
    ExoticMelee,
}

/// The ranged weapon types listed in the table on p.171.
///
/// Variant order tracks the book's row order (Medium Pistol → Rocket
/// Launcher). All eleven types appear here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RangedKind {
    /// Medium Pistol — Handgun skill, 2d6, 12-round magazine. p.171.
    MediumPistol,
    /// Heavy Pistol — Handgun skill, 3d6, 8-round magazine. p.171.
    HeavyPistol,
    /// Very Heavy Pistol — Handgun skill, 4d6, 8-round magazine. p.171.
    VeryHeavyPistol,
    /// SMG — Handgun skill, 2d6, 30-round magazine, Autofire(3). p.171.
    SMG,
    /// Heavy SMG — Handgun skill, 3d6, 40-round magazine, Autofire(3). p.171.
    HeavySMG,
    /// Shotgun — Shoulder Arms skill, 5d6 (Slug), 4-round magazine,
    /// Shotgun Shell alt-fire. p.171.
    Shotgun,
    /// Assault Rifle — Shoulder Arms skill, 5d6, 25-round magazine,
    /// Autofire(4). p.171.
    AssaultRifle,
    /// Sniper Rifle — Shoulder Arms skill, 5d6, 4-round magazine. p.171.
    SniperRifle,
    /// Bows & Crossbows — Archery skill, 4d6, ammo: Arrow. p.171.
    BowCrossbow,
    /// Grenade Launcher — Heavy Weapons skill, 6d6, 2-round magazine,
    /// Explosive. p.171.
    GrenadeLauncher,
    /// Rocket Launcher — Heavy Weapons skill, 8d6, 1-round magazine,
    /// Explosive. p.171.
    RocketLauncher,
}

/// The four melee classifications listed on p.175.
///
/// Damage scales by classification: Light 1d6, Medium 2d6, Heavy 3d6,
/// Very Heavy 4d6. ROF is 2 for Light/Medium/Heavy and 1 for Very Heavy
/// (p.175). All four ignore half the defender's armor (p.176).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MeleeKind {
    /// Light Melee Weapon — 1d6, ROF 2, concealable. e.g. Combat Knife,
    /// Tomahawk. p.175.
    Light,
    /// Medium Melee Weapon — 2d6, ROF 2. e.g. Baseball Bat, Crowbar,
    /// Machete. p.175.
    Medium,
    /// Heavy Melee Weapon — 3d6, ROF 2. e.g. Lead Pipe, Sword, Spiked
    /// Bat. p.175.
    Heavy,
    /// Very Heavy Melee Weapon — 4d6, ROF 1. e.g. Chainsaw, Sledgehammer,
    /// Helicopter Blades, Naginata. p.175.
    VeryHeavy,
}

// ---------------------------------------------------------------------------
// Features
// ---------------------------------------------------------------------------

/// Alt-fire modes and special features listed on pp.171, 174.
///
/// The book pairs every ranged weapon with an "Alt. Fire Modes & Special
/// Features" row (p.171). Variants here correspond 1:1 to those entries
/// plus the bow-only `SilentNotSilenced` annotation called out in the
/// flavour text on p.174.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponFeature {
    /// Autofire mode — costs 10 rounds and an Action; the carried `u8`
    /// is the damage-multiplier cap (3 for SMG / Heavy SMG, 4 for Assault
    /// Rifle). See p.173.
    Autofire(u8),
    /// Suppressive Fire — costs 10 rounds and an Action; forces WILL +
    /// Concentration vs. REF + Autofire on everyone in line of sight
    /// within 25 m/yd. See p.174.
    SuppressiveFire,
    /// Shotgun shells — Shotguns can fire shells in addition to slugs.
    /// 3d6 to every target within a 6 m/yd cone, DV13 to hit. See p.174.
    ShotgunShell,
    /// Arrows — basic arrows can always be retrieved after firing, so
    /// bows / crossbows never need to Reload. See p.174.
    Arrows,
    /// Explosive — the weapon's damage hits everything in a 10×10 m/yd
    /// area, including terrain. See p.174.
    Explosive,
    /// Bow-only flavour: silent but not *silenced*. Called out in the
    /// p.174 sidebar describing why bows escape Reload constraints; the
    /// engine itself uses this to flag "no audio Detection check on
    /// firing".
    SilentNotSilenced,
}

// ---------------------------------------------------------------------------
// Magazine
// ---------------------------------------------------------------------------

/// A weapon's standard magazine. See p.171 ("Standard Magazine" column)
/// and p.344 (Clip Chart for Standard / Extended / Drum capacities).
///
/// `capacity` is the standard magazine size; the rulebook's Extended /
/// Drum upgrades on p.343 are weapon attachments and live in WP-209
/// (Weapon attachments), not here.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Magazine {
    /// Standard magazine size (rounds). 0 is reserved for weapons whose
    /// table entry reads "N/A" (Bows & Crossbows, p.171) — those have
    /// `capacity == 0` and the [`WeaponFeature::Arrows`] feature so the
    /// reload subsystem skips them.
    pub capacity: u8,
    /// Ammunition type the magazine is loaded with.
    pub ammo: AmmoKind,
}

// ---------------------------------------------------------------------------
// RangeBand
// ---------------------------------------------------------------------------

/// DV-by-range table for a ranged weapon. See pp.172–173.
///
/// `single_shot` is the list of `(max meters of band, single-shot DV)`
/// entries from the Single Shot table on p.172, in the order they appear
/// in the book (closest band first). For a weapon with no entry at a
/// given range (the table prints "N/A"), the band is **omitted** rather
/// than encoded — callers that need to ask "what's the DV at 500m for an
/// SMG?" will get `None` from [`RangeBand::single_shot_dv_at`] which the
/// combat engine should treat as out-of-range.
///
/// `autofire` is the `Some(_)` table for SMGs / Assault Rifles only
/// (p.173), and `None` for every other weapon. The Autofire table tops
/// out at 51–100 m/yd — Autofire beyond that is impossible per RAW.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RangeBand {
    /// Each entry: `(max_meters, dv)`. The max meters is the *upper*
    /// bound of that band — e.g. (6, 13) means "0 to 6 m/yd → DV 13".
    /// Bands are listed in ascending order of `max_meters`.
    pub single_shot: Vec<(u16, u8)>,
    /// Autofire DV table (p.173). `Some` only for weapons with an
    /// `Autofire` feature; `None` otherwise.
    pub autofire: Option<Vec<(u16, u8)>>,
}

impl RangeBand {
    /// Empty range band — used by every melee, thrown, and bare exotic
    /// entry where the range table is irrelevant.
    pub const fn none() -> Self {
        Self {
            single_shot: Vec::new(),
            autofire: None,
        }
    }

    /// Return the single-shot DV at `meters`, or `None` if the weapon's
    /// range table doesn't reach that far.
    ///
    /// Bands are inclusive on the upper bound: a `(6, 13)` band covers
    /// 0..=6 m/yd. The first band whose `max_meters >= meters` wins —
    /// matching how the rulebook's columns read on p.172.
    pub fn single_shot_dv_at(&self, meters: u16) -> Option<u8> {
        self.single_shot
            .iter()
            .find(|(max, _)| *max >= meters)
            .map(|(_, dv)| *dv)
    }

    /// Return the autofire DV at `meters`, or `None` if the weapon
    /// can't autofire at that range (or at all).
    pub fn autofire_dv_at(&self, meters: u16) -> Option<u8> {
        self.autofire
            .as_ref()?
            .iter()
            .find(|(max, _)| *max >= meters)
            .map(|(_, dv)| *dv)
    }
}

// ---------------------------------------------------------------------------
// Weapon
// ---------------------------------------------------------------------------

/// A weapon catalog entry. See pp.170–179, pp.340–344.
///
/// One entry per generic weapon type. Brand variants (p.342) are not
/// individually catalogued; the dice/skill/range data is the same and
/// the brand is purely flavour.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weapon {
    /// Catalog slug — the [`WeaponId`] used in `Inventory` items and
    /// equipped-weapon references.
    pub id: WeaponId,
    /// Display name as printed in the rulebook table (e.g. "Medium
    /// Pistol", "Very Heavy Melee Weapon").
    pub display_name: String,
    /// Top-level kind discriminator.
    pub kind: WeaponKind,
    /// The Skill used to attack with this weapon. See pp.171, 175 for
    /// the per-type assignment (Handgun, Shoulder Arms, Heavy Weapons,
    /// Archery, Melee Weapon, Brawling).
    pub skill: SkillId,
    /// Single-shot damage. See p.171 ("Single Shot Damage" column) and
    /// p.175 ("Damage" column).
    pub damage: DamageDice,
    /// Rate of Fire — `1` or `2`. See pp.171, 175.
    pub rof: u8,
    /// Number of hands required. `1` or `2`. See pp.171, 175.
    pub hands: u8,
    /// `true` if the "Can be Concealed?" column is YES (pp.171, 175).
    pub concealable: bool,
    /// Price tier (Cheap / Costly / Premium / Expensive / …).
    pub price: PriceTier,
    /// Canonical Eurobuck price — matches the table's Cost column on
    /// pp.171, 175 and `PriceTier::canonical_cost()`.
    pub price_eb: Eurobucks,
    /// Alt-fire modes / special features.
    pub features: Vec<WeaponFeature>,
    /// Magazine spec. `None` for melee, thrown, and weapons that
    /// genuinely have no magazine (some exotics).
    pub magazine: Option<Magazine>,
    /// DV-by-range table. Empty for melee / thrown / exotic-melee
    /// (use [`RangeBand::none`]).
    pub ranges: RangeBand,
}

// ---------------------------------------------------------------------------
// Range tables — single source of truth, p.172 / p.173
// ---------------------------------------------------------------------------

/// Single-shot range table for the "Pistol" row on p.172.
///
/// Used by Medium Pistol, Heavy Pistol, Very Heavy Pistol, and Heavy SMG
/// (the Heavy SMG fires a Heavy Pistol round and uses the Pistol row per
/// the table — RAW).
///
/// Bands per p.172: 0–6 → 13, 7–12 → 15, 13–25 → 20, 26–50 → 25,
/// 51–100 → 30, 101–200 → 30. (No entries for 201–400 / 401–800.)
pub const PISTOL_RANGES: &[(u16, u8)] =
    &[(6, 13), (12, 15), (25, 20), (50, 25), (100, 30), (200, 30)];

/// Single-shot range table for the "SMG" row on p.172.
///
/// Used by SMG only — Heavy SMG follows the Pistol row per p.172.
/// Bands: 0–6 → 15, 7–12 → 13, 13–25 → 15, 26–50 → 20, 51–100 → 25,
/// 101–200 → 25, 201–400 → 30. (No entry at 401–800.)
pub const SMG_RANGES: &[(u16, u8)] = &[
    (6, 15),
    (12, 13),
    (25, 15),
    (50, 20),
    (100, 25),
    (200, 25),
    (400, 30),
];

/// Single-shot range table for the "Shotgun (Slug)" row on p.172.
///
/// Bands: 0–6 → 13, 7–12 → 15, 13–25 → 20, 26–50 → 25, 51–100 → 30,
/// 101–200 → 35.
pub const SHOTGUN_SLUG_RANGES: &[(u16, u8)] =
    &[(6, 13), (12, 15), (25, 20), (50, 25), (100, 30), (200, 35)];

/// Single-shot range table for "Assault Rifle" on p.172.
///
/// Bands: 0–6 → 17, 7–12 → 16, 13–25 → 15, 26–50 → 13, 51–100 → 15,
/// 101–200 → 20, 201–400 → 25, 401–800 → 30.
pub const ASSAULT_RIFLE_RANGES: &[(u16, u8)] = &[
    (6, 17),
    (12, 16),
    (25, 15),
    (50, 13),
    (100, 15),
    (200, 20),
    (400, 25),
    (800, 30),
];

/// Single-shot range table for "Sniper Rifle" on p.172.
///
/// Note the inverted curve: short range is *harder* (DV 30 at 0–6 m/yd)
/// because sniper rifles aren't built for close work. Bands: 0–6 → 30,
/// 7–12 → 25, 13–25 → 25, 26–50 → 20, 51–100 → 15, 101–200 → 16,
/// 201–400 → 17, 401–800 → 20.
pub const SNIPER_RIFLE_RANGES: &[(u16, u8)] = &[
    (6, 30),
    (12, 25),
    (25, 25),
    (50, 20),
    (100, 15),
    (200, 16),
    (400, 17),
    (800, 20),
];

/// Single-shot range table for "Bows & Crossbow" on p.172.
///
/// Bands: 0–6 → 15, 7–12 → 13, 13–25 → 15, 26–50 → 17, 51–100 → 20,
/// 101–200 → 22.
pub const BOW_RANGES: &[(u16, u8)] = &[(6, 15), (12, 13), (25, 15), (50, 17), (100, 20), (200, 22)];

/// Single-shot range table for "Grenade Launcher" on p.172.
///
/// Bands: 0–6 → 16, 7–12 → 15, 13–25 → 15, 26–50 → 17, 51–100 → 20,
/// 101–200 → 22, 201–400 → 25.
pub const GRENADE_LAUNCHER_RANGES: &[(u16, u8)] = &[
    (6, 16),
    (12, 15),
    (25, 15),
    (50, 17),
    (100, 20),
    (200, 22),
    (400, 25),
];

/// Single-shot range table for "Rocket Launcher" on p.172.
///
/// Bands: 0–6 → 17, 7–12 → 16, 13–25 → 15, 26–50 → 15, 51–100 → 20,
/// 101–200 → 20, 201–400 → 25, 401–800 → 30.
pub const ROCKET_LAUNCHER_RANGES: &[(u16, u8)] = &[
    (6, 17),
    (12, 16),
    (25, 15),
    (50, 15),
    (100, 20),
    (200, 20),
    (400, 25),
    (800, 30),
];

/// Autofire DV table for "SMGs" on p.173.
///
/// Used for both SMG and Heavy SMG (autofire cap 3 per p.171).
/// Bands: 0–6 → 20, 7–12 → 17, 13–25 → 20, 26–50 → 25, 51–100 → 30.
pub const SMG_AUTOFIRE_RANGES: &[(u16, u8)] = &[(6, 20), (12, 17), (25, 20), (50, 25), (100, 30)];

/// Autofire DV table for "Assault Rifle" on p.173.
///
/// Cap 4 per p.171.
/// Bands: 0–6 → 22, 7–12 → 20, 13–25 → 17, 26–50 → 20, 51–100 → 25.
pub const ASSAULT_RIFLE_AUTOFIRE_RANGES: &[(u16, u8)] =
    &[(6, 22), (12, 20), (25, 17), (50, 20), (100, 25)];

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/weapons.ron`.
///
/// The file is a `WeaponsFile(weapons: [ ... ])` envelope where each
/// entry is a `Weapon` plus an explicit `slug` field (the `Catalog<T>`
/// key, also used as the [`WeaponId`]).
#[derive(Debug, Deserialize)]
struct WeaponsFile {
    weapons: Vec<WeaponsFileEntry>,
}

/// One row in the on-disk weapons catalog file.
///
/// `slug` is the lookup key; the loader checks that `slug == id.0`
/// (i.e. the [`WeaponId`] string matches the catalog key) so a
/// `cat.get("medium_pistol").unwrap().id` is always `WeaponId("medium_pistol")`.
#[derive(Debug, Deserialize)]
struct WeaponsFileEntry {
    slug: String,
    id: WeaponId,
    display_name: String,
    kind: WeaponKind,
    skill: SkillId,
    damage: DamageDice,
    rof: u8,
    hands: u8,
    concealable: bool,
    price: PriceTier,
    price_eb: Eurobucks,
    features: Vec<WeaponFeature>,
    magazine: Option<Magazine>,
    ranges: RangeBand,
}

/// Load the weapons catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Weapon>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] with the path and a
/// stringified description of the underlying I/O / parse / invariant
/// error.
///
/// The loader enforces three invariants:
/// 1. `slug == id.0` for every row — the [`WeaponId`] must agree with
///    the catalog key.
/// 2. Slugs are unique within the file.
/// 3. `hands` is `1` or `2` per p.171 / p.175. Other values fail the load.
pub fn load_weapons_catalog(path: &Path) -> Result<Catalog<Weapon>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: WeaponsFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Weapon> = HashMap::with_capacity(parsed.weapons.len());
    for row in parsed.weapons {
        if row.slug != row.id.0 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "weapon slug '{}' disagrees with WeaponId '{}'",
                    row.slug, row.id.0
                ),
            });
        }
        if row.hands != 1 && row.hands != 2 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "weapon '{}' has hands={} (must be 1 or 2 per pp.171, 175)",
                    row.slug, row.hands
                ),
            });
        }
        let weapon = Weapon {
            id: row.id,
            display_name: row.display_name,
            kind: row.kind,
            skill: row.skill,
            damage: row.damage,
            rof: row.rof,
            hands: row.hands,
            concealable: row.concealable,
            price: row.price,
            price_eb: row.price_eb,
            features: row.features,
            magazine: row.magazine,
            ranges: row.ranges,
        };
        if entries.insert(row.slug.clone(), weapon).is_some() {
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

    /// Workspace-relative path to the canonical weapons catalog file.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("weapons.ron");
        p
    }

    /// Acceptance: catalog has at least 30 entries — covering 11 ranged
    /// types (p.171), 4 melee classifications (p.175), plus several
    /// concrete melee examples per p.175 ("Combat Knife", "Baseball Bat",
    /// …) and Brawling/Martial Arts entries needed by combat tests.
    #[test]
    fn test_weapon_catalog_size() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        assert!(
            cat.len() >= 30,
            "expected >=30 weapons across pp.170-179 (got {}); verify the catalog RON file",
            cat.len()
        );
    }

    /// Acceptance (per WP-202): assault rifle DV at 13–25m is 15, at
    /// 401–800m is 30. Verified against p.172.
    #[test]
    fn test_assault_rifle_ranges() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        let rifle = cat
            .get("assault_rifle")
            .expect("assault_rifle must be in the catalog");

        // 13–25m band is the third entry; check at the band boundaries.
        assert_eq!(rifle.ranges.single_shot_dv_at(13), Some(15));
        assert_eq!(rifle.ranges.single_shot_dv_at(25), Some(15));

        // 401–800m band is the eighth entry.
        assert_eq!(rifle.ranges.single_shot_dv_at(401), Some(30));
        assert_eq!(rifle.ranges.single_shot_dv_at(800), Some(30));

        // Out of range.
        assert_eq!(rifle.ranges.single_shot_dv_at(801), None);
    }

    /// Acceptance (per WP-202): `medium_pistol.skill == SkillId::Handgun`.
    /// Verified against p.171 ("Medium Pistol — Handgun").
    #[test]
    fn test_handgun_skill_link() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        let pistol = cat
            .get("medium_pistol")
            .expect("medium_pistol must be in the catalog");
        assert_eq!(pistol.skill, SkillId::Handgun);
    }

    /// Acceptance (per WP-202): SMG autofire cap 3, Assault Rifle cap 4.
    /// Verified against p.171 (Alt. Fire Modes column) and p.173.
    #[test]
    fn test_autofire_caps() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");

        let smg = cat.get("smg").expect("smg must be in the catalog");
        let smg_cap = smg
            .features
            .iter()
            .find_map(|f| match f {
                WeaponFeature::Autofire(n) => Some(*n),
                _ => None,
            })
            .expect("smg must have an Autofire feature");
        assert_eq!(smg_cap, 3, "SMG autofire cap is 3 per p.171");

        let rifle = cat
            .get("assault_rifle")
            .expect("assault_rifle must be in the catalog");
        let rifle_cap = rifle
            .features
            .iter()
            .find_map(|f| match f {
                WeaponFeature::Autofire(n) => Some(*n),
                _ => None,
            })
            .expect("assault_rifle must have an Autofire feature");
        assert_eq!(rifle_cap, 4, "Assault Rifle autofire cap is 4 per p.171");

        // Heavy SMG also caps at 3 per p.171.
        let heavy_smg = cat
            .get("heavy_smg")
            .expect("heavy_smg must be in the catalog");
        let hsmg_cap = heavy_smg
            .features
            .iter()
            .find_map(|f| match f {
                WeaponFeature::Autofire(n) => Some(*n),
                _ => None,
            })
            .expect("heavy_smg must have an Autofire feature");
        assert_eq!(hsmg_cap, 3, "Heavy SMG autofire cap is 3 per p.171");
    }

    /// Acceptance: a representative weapon survives a RON round trip.
    /// Pins the on-disk schema so accidental field renames trip CI.
    #[test]
    fn test_weapon_round_trip_ron() {
        let original = Weapon {
            id: WeaponId("test_pistol".into()),
            display_name: "Test Pistol".to_string(),
            kind: WeaponKind::Ranged(RangedKind::MediumPistol),
            skill: SkillId::Handgun,
            damage: DamageDice {
                n: 2,
                die: DieKind::D6,
            },
            rof: 2,
            hands: 1,
            concealable: true,
            price: PriceTier::Costly,
            price_eb: Eurobucks(50),
            features: vec![],
            magazine: Some(Magazine {
                capacity: 12,
                ammo: AmmoKind::MPistol,
            }),
            ranges: RangeBand {
                single_shot: PISTOL_RANGES.to_vec(),
                autofire: None,
            },
        };
        let serialised = ron::ser::to_string(&original).expect("must serialise");
        let restored: Weapon = ron::de::from_str(&serialised).expect("must round-trip");
        assert_eq!(restored, original);
    }

    /// Regression: the SMG autofire band at 13–25m is DV 20 (p.173).
    /// The example on p.173 shows Royal hitting at 14m needing DV17, but
    /// that's the *Assault Rifle* table; the SMG table at 13–25 is 20.
    #[test]
    fn test_smg_autofire_range_band() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        let smg = cat.get("smg").expect("smg must be in the catalog");
        assert_eq!(smg.ranges.autofire_dv_at(14), Some(20));
        // Out of autofire range past 100m.
        assert_eq!(smg.ranges.autofire_dv_at(101), None);
    }

    /// Regression: sniper rifles preserve the *inverted* range curve
    /// from p.172 — close-range DV is high (30 at 0–6m), drops at long
    /// range (15 at 51–100m). Catches an accidental ascending sort.
    #[test]
    fn test_sniper_rifle_inverted_curve() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        let sniper = cat
            .get("sniper_rifle")
            .expect("sniper_rifle must be in the catalog");
        assert_eq!(sniper.ranges.single_shot_dv_at(3), Some(30));
        assert_eq!(sniper.ranges.single_shot_dv_at(60), Some(15));
    }

    /// Regression: all melee classifications are present and damage
    /// scales 1d6/2d6/3d6/4d6 with ROF 2/2/2/1 per p.175.
    #[test]
    fn test_melee_classifications_present() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");

        let light = cat.get("light_melee_weapon").expect("light melee");
        assert_eq!(
            light.damage,
            DamageDice {
                n: 1,
                die: DieKind::D6
            }
        );
        assert_eq!(light.rof, 2);
        assert!(light.concealable);

        let medium = cat.get("medium_melee_weapon").expect("medium melee");
        assert_eq!(
            medium.damage,
            DamageDice {
                n: 2,
                die: DieKind::D6
            }
        );
        assert_eq!(medium.rof, 2);

        let heavy = cat.get("heavy_melee_weapon").expect("heavy melee");
        assert_eq!(
            heavy.damage,
            DamageDice {
                n: 3,
                die: DieKind::D6
            }
        );
        assert_eq!(heavy.rof, 2);

        let very = cat
            .get("very_heavy_melee_weapon")
            .expect("very heavy melee");
        assert_eq!(
            very.damage,
            DamageDice {
                n: 4,
                die: DieKind::D6
            }
        );
        assert_eq!(
            very.rof, 1,
            "Very Heavy Melee Weapons attack at ROF 1 per p.176"
        );
    }

    /// Regression: every loaded weapon's `id.0` agrees with its catalog
    /// slug — pinning the loader's invariant.
    #[test]
    fn test_slug_id_agreement() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        for (slug, weapon) in cat.iter() {
            assert_eq!(slug, &weapon.id.0, "slug/id disagree for {slug}");
        }
    }

    /// Regression: every ranged weapon (`WeaponKind::Ranged(_)`) has a
    /// non-empty `single_shot` range band; every melee weapon has an
    /// empty one.
    #[test]
    fn test_ranges_match_kind() {
        let cat = load_weapons_catalog(&catalog_path()).expect("catalog must load");
        for (slug, weapon) in cat.iter() {
            match weapon.kind {
                WeaponKind::Ranged(_) => assert!(
                    !weapon.ranges.single_shot.is_empty(),
                    "ranged weapon '{slug}' has empty range band"
                ),
                WeaponKind::Melee(_) | WeaponKind::ExoticMelee => assert!(
                    weapon.ranges.single_shot.is_empty(),
                    "melee weapon '{slug}' must have empty range band"
                ),
                _ => {}
            }
        }
    }
}
