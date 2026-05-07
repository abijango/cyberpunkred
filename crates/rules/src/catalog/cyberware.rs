//! Cyberware catalog (WP-204).
//!
//! Defines the [`Cyberware`] catalog row, the supporting enums
//! ([`CyberwareCategory`], [`InstallLocation`], [`HumanityLossSpec`],
//! [`DiceSpec`], [`DieKind`]) and the RON loader [`load_cyberware_catalog`].
//!
//! Rulebook references:
//! - **pp.94, 107–116:** abbreviated cyberware tables (the "Putting the Cyber
//!   into the Punk" chapter), one table per category. Used to cross-check
//!   counts and naming.
//! - **pp.358–365 (more precisely pp.358–367 in the v1.25 PDF):** the Night
//!   Market detailed entries for every cyberware item — the canonical source
//!   for Description & Data, Cost, HL, Install, and prerequisite rules. The
//!   loader pulls per-category RON files keyed off this table.
//! - **p.110:** the canonical definitions of `Mall` / `Clinic` / `Hospital`
//!   for [`InstallLocation`] and the shape of the cyberware row (Name,
//!   Install, Description, Cost, HL).
//! - **p.111:** "After Character Generation, Humanity Loss is determined
//!   by the dice in parentheses following the preset number." This sentence
//!   is the basis for [`HumanityLossSpec::Rolled`] vs [`HumanityLossSpec::Fixed`].
//! - **p.226:** "Medical-Grade Cyberware" — replacement parts for body parts
//!   lost to a Critical Injury have **no Humanity Loss** ([`HumanityLossSpec::None`]).
//! - **p.227:** the at-creation vs in-play HL distinction restated in
//!   the *Therapy and You!* / *Replacement Parts* sidebar.
//!
//! ## Public API contract
//!
//! Per the WP-204 spec the public surface is:
//!
//! ```ignore
//! pub struct Cyberware { … }
//! pub enum CyberwareCategory { Fashionware, Neuralware, Cyberoptics,
//!     Cyberaudio, InternalBody, ExternalBody, Cyberlimb, Borgware }
//! pub enum InstallLocation { Mall, Clinic, Hospital }
//! pub enum HumanityLossSpec { Fixed(u8), Rolled { fixed: u8, dice: DiceSpec },
//!     None }
//! pub fn load_cyberware_catalog(path: &Path) -> Result<Catalog<Cyberware>, RulesError>;
//! ```
//!
//! ## Deviation from WP-204 spec — `DiceSpec` not `DamageDice`
//!
//! WP-204's spec writes the in-play HL roll as `DamageDice` (the type
//! WP-202 introduces). At the time WP-204 was implemented WP-202 had not
//! merged into `main`, so per WP-204's *Workaround* note this module
//! defines a local [`DiceSpec`] / [`DieKind`] of the same shape. When
//! WP-202 lands a follow-up PR will collapse the two into a single type
//! (or alias one to the other) — both share the same `(n, die)` shape so
//! the migration is mechanical.
//!
//! ## Catalog file layout
//!
//! Authored content lives under `content/catalogs/cyberware/`, one RON
//! file per category (`fashionware.ron`, `neuralware.ron`, …). The
//! per-category split keeps each file under ~300 lines, which scans well
//! in a code review and makes a missing-line diff actually readable. The
//! loader merges every `*.ron` file in the directory in alphabetical
//! order — collisions between files are a hard error (duplicate slug).

use crate::catalog::Catalog;
use crate::effects::modifier::EffectModifier;
use crate::effects::CyberwareId;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Category
// ---------------------------------------------------------------------------

/// One of the eight cyberware categories listed on p.110.
///
/// Per p.110: "There are 8 types of Cyberware. Fashionware, Neuralware,
/// Cyberoptics, Cyberaudio, Internal Body Cyberware, External Body
/// Cyberware, Cyberlimbs, Borgware." Variant naming normalises the book's
/// display names to `UpperCamelCase` and drops the redundant "Cyberware"
/// suffix where the category banner already implies it
/// (`InternalBody`, not `InternalBodyCyberware`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CyberwareCategory {
    /// Personal-adornment cyberware. See p.111 / p.358.
    Fashionware,
    /// Reflex / mental-augmentation cyberware. Foundational piece is
    /// Neural Link. See p.112 / p.359.
    Neuralware,
    /// Visual cyberware. Foundational piece is Cybereye. See p.112 / p.360.
    Cyberoptics,
    /// Auditory cyberware. Foundational piece is Cyberaudio Suite. See
    /// p.113 / p.361.
    Cyberaudio,
    /// Internal organs and systemic improvements. See p.114 / p.362.
    InternalBody,
    /// On / under-the-skin cyberware (armor, holsters, pockets). See
    /// p.114 / p.364.
    ExternalBody,
    /// Cybernetic arms and legs. Foundational pieces are Cyberarm /
    /// Cyberleg. See p.115 / p.364.
    Cyberlimb,
    /// Heavy body-replacement cyberware (linear frames, mounts, sensor
    /// arrays). See p.116 / p.367.
    Borgware,
}

// ---------------------------------------------------------------------------
// Install location
// ---------------------------------------------------------------------------

/// Where a cyberware install can take place. See p.110.
///
/// Per p.110: *"Mall* means you can literally get the installation done in
/// any mall or street corner bio-mod shop … *Clinic* means an actual
/// Medtech in a medical surgery clinic … *Hospital* means the work
/// requires major surgery and a Medtech capable of doing this kind of
/// work."* The Hospital DV / cost ladder on p.226 also keys off these
/// three names.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InstallLocation {
    /// Walk-in mall / street-corner bio-mod shop. See p.110.
    Mall,
    /// Medtech-run surgery clinic. See p.110.
    Clinic,
    /// Full hospital with a major-surgery-capable Medtech. See p.110.
    Hospital,
}

// ---------------------------------------------------------------------------
// Dice spec (local stand-in for WP-202's DamageDice)
// ---------------------------------------------------------------------------

/// Which die a [`DiceSpec`] uses.
///
/// Cyberware HL on pp.358–367 only ever uses d6 (and the halved-rounded-up
/// "1d6/2 round up" pattern is encoded as `DiceSpec { n: 1, die: D6 }`
/// plus a `divisor` field). To keep the API shape compatible with
/// WP-202's planned `DieKind`, the variants are kept distinct rather than
/// collapsing into a single struct.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DieKind {
    /// Six-sided die. The only kind cyberware HL ever calls for.
    D6,
    /// Ten-sided die. Reserved for parity with WP-202's `DieKind`.
    D10,
}

/// A "roll N dice of kind K" specification.
///
/// **Local duplicate of WP-202's `DamageDice`.** WP-204 implementation
/// notes call out this duplication — when WP-202 lands, a follow-up PR
/// will merge the two. Same `(n, die)` shape, same serde representation;
/// the migration is mechanical.
///
/// Used inside [`HumanityLossSpec::Rolled`] to encode the in-play HL
/// roll printed in parentheses on pp.358–367 (e.g. "7 (2d6)" → fixed 7,
/// `DiceSpec { n: 2, die: D6, divisor: 1 }`).
///
/// `divisor` accommodates the "1d6/2 round up" pattern used by the
/// peripheral-cyberware HL entries (Anti-Dazzle, Standard Hand, etc., on
/// pp.360–367). The roll is `ceil(roll(n,die) / divisor)`. For the
/// common `2d6` / `4d6` / `1d6` HL entries, `divisor` is `1`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DiceSpec {
    /// Number of dice to roll.
    pub n: u8,
    /// Which die to roll.
    pub die: DieKind,
    /// Divisor applied to the sum, rounded up. `1` is the no-divisor
    /// case. The book uses `2` for *"1d6/2 round up"* peripherals.
    pub divisor: u8,
}

// ---------------------------------------------------------------------------
// Humanity loss spec
// ---------------------------------------------------------------------------

/// Humanity-Loss rule for an installed cyberware item.
///
/// Per p.110 (How to Read the Cyberware Tables) and p.111 ("HL"):
///
/// > "At Character Generation, Humanity Loss is preset. *After Character
/// > Generation, Humanity Loss is determined by the dice in parentheses
/// > following the preset number.*"
///
/// The catalog encodes both numbers from the tables (the preset and the
/// dice expression). At-creation paths use [`fixed_value`](Self::fixed_value);
/// in-play install paths roll [`Rolled::dice`] and add to
/// [`Rolled::fixed`].
///
/// [`HumanityLossSpec::None`] is reserved for items that explicitly carry
/// `0 (N/A)` in the table — every Fashionware item, Memory Chip, Plastic
/// Covering, Realskinn Covering, Superchrome Covering, Standard Hand /
/// Standard Foot when installed in a meat limb, and Contraceptive
/// Implant. Per p.226, *Medical-Grade Cyberware* (replacement parts for
/// a body part lost to a Critical Injury) also "does not cause Humanity
/// Loss" — the catalog only models the published item HL; whether a
/// **specific install** counts as medical-grade is a runtime concern
/// handled by the Character / GM crate, not by this catalog.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HumanityLossSpec {
    /// At-creation HL only. The full HL value is `n`. Used at character
    /// generation per p.111.
    Fixed(u8),
    /// In-play HL: the preset value plus a dice roll. e.g. `7 (2d6)` is
    /// `Rolled { fixed: 7, dice: DiceSpec { n: 2, die: D6, divisor: 1 } }`.
    /// At-creation a caller can still read [`Rolled::fixed`] as the
    /// preset value.
    Rolled {
        /// Preset HL printed before the parenthesis on pp.358–367.
        fixed: u8,
        /// Dice expression printed inside the parenthesis.
        dice: DiceSpec,
    },
    /// No Humanity Loss for this item — the table prints `0 (N/A)`.
    /// Also the right shape for medical-grade installs (p.226), though
    /// that determination is per-install, not per-catalog-row.
    None,
}

impl HumanityLossSpec {
    /// Return the at-character-generation HL value for this item.
    ///
    /// At creation HL is preset (p.111): `Fixed(n)` and `Rolled { fixed, .. }`
    /// both report `n`/`fixed`; `None` reports 0.
    pub fn fixed_value(&self) -> u8 {
        match self {
            HumanityLossSpec::Fixed(n) => *n,
            HumanityLossSpec::Rolled { fixed, .. } => *fixed,
            HumanityLossSpec::None => 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Cyberware
// ---------------------------------------------------------------------------

/// A single cyberware catalog entry — one row in the Night Market tables
/// on pp.358–367.
///
/// `id` doubles as the in-content stable handle. The slug used as the
/// `Catalog<Cyberware>` key is a snake_case form of the item name
/// (e.g. `neural_link`, `interface_plugs`); the loader enforces that
/// `id.0 == slug`.
///
/// `effects` carries the ongoing modifiers a piece of cyberware grants
/// while installed (e.g. Kerenzikov: `+2` to Initiative Rolls →
/// `EffectModifier::InitiativeBonus(2)`). Items whose printed text is
/// purely descriptive or whose effect is event-driven (e.g. Sandevistan's
/// activated `+3` to Initiative for one minute, or Pain Editor's
/// "ignore Seriously Wounded" toggle) carry an empty `effects` list —
/// the GM crate hooks those at the appropriate event sites.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cyberware {
    /// Stable identifier. The string inside is the slug (`snake_case`).
    pub id: CyberwareId,
    /// Display name as printed in the rulebook (e.g. "Neural Link",
    /// "Mr. Studd Sexual Implant").
    pub display_name: String,
    /// Which of the eight cyberware categories this item lives under
    /// (p.110).
    pub category: CyberwareCategory,
    /// Where the install must take place per the book's `Install` column
    /// (p.110).
    pub install_difficulty: InstallLocation,
    /// At-creation preset HL plus optional in-play dice roll. See p.111.
    pub humanity_loss: HumanityLossSpec,
    /// Price tier (p.110 referencing the Cost column).
    pub price: PriceTier,
    /// Eurobuck cost. Matches `price.canonical_cost()` for non-tiered
    /// outliers (e.g. Memory Chip at 10eb is `Cheap`); some items
    /// straddle two tiers (e.g. Skill Chip is 500eb or 1,000eb depending
    /// on whether the chipped skill is `(×2)`). The catalog records the
    /// **base** cost — the lower of the two for two-tier items — and the
    /// extended cost is computed at purchase time.
    pub price_eb: Eurobucks,
    /// How many option slots this item exposes (foundational pieces:
    /// Neural Link 5, Cybereye 3, Cyberaudio Suite 3, Cyberarm 4,
    /// Cyberleg 3; everything else 0). See p.111 ("Options Slots") and
    /// each foundational entry on pp.359–366.
    pub option_slots: u8,
    /// How many slots this item consumes when installed into its
    /// foundational piece. Most options take 1 slot; some take 2 or 3
    /// (e.g. MicroVideo, Popup Shield, Cyberdeck-in-arm). Foundational
    /// pieces and standalone items (Fashionware, External Body, etc.)
    /// have `slot_cost: 0`. See pp.358–367 for per-item annotations.
    pub slot_cost: u8,
    /// What this item requires to install — usually a foundational
    /// cyberware in the same category (Neural Link for Neuralware,
    /// Cybereye for Cyberoptics, …). `None` for foundational items and
    /// standalone categories.
    pub prerequisite: Option<CyberwareId>,
    /// Ongoing modifiers granted while this item is installed. Empty for
    /// items whose effect is descriptive, event-driven, or activated.
    pub effects: Vec<EffectModifier>,
    /// Free-form description distilled from the Description & Data
    /// column on pp.358–367. The LLM narrator uses this; the engine
    /// itself only cares about the structured fields.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// On-disk envelope for a per-category cyberware RON file.
///
/// One RON file per category, all under `content/catalogs/cyberware/`.
/// Splitting per category keeps each file under ~300 lines for review.
#[derive(Debug, Deserialize)]
struct CyberwareFile {
    cyberware: Vec<CyberwareFileEntry>,
}

/// One on-disk row inside a [`CyberwareFile`].
///
/// `slug` is the lookup key; the loader enforces `id.0 == slug` so the
/// catalog can't disagree with the structured id. `prerequisite_slug`
/// is the (optional) prerequisite cyberware's slug; the loader resolves
/// it into a `CyberwareId` and *checks the prerequisite exists in the
/// loaded set* — a missing prerequisite is a hard error.
#[derive(Debug, Deserialize)]
struct CyberwareFileEntry {
    slug: String,
    display_name: String,
    category: CyberwareCategory,
    install_difficulty: InstallLocation,
    humanity_loss: HumanityLossSpec,
    price: PriceTier,
    price_eb: i64,
    option_slots: u8,
    slot_cost: u8,
    #[serde(default)]
    prerequisite_slug: Option<String>,
    #[serde(default)]
    effects: Vec<EffectModifier>,
    description: String,
}

/// Load every per-category RON file under `dir` and merge into one
/// catalog.
///
/// `dir` should be the `content/catalogs/cyberware/` directory; the
/// loader reads every `*.ron` file inside (sorted alphabetically for
/// determinism) and merges. Cross-file slug collisions are a hard
/// error.
///
/// Invariants enforced by the loader:
/// 1. Every entry's `slug` is unique across the whole directory.
/// 2. If `prerequisite_slug` is set, the named slug exists somewhere in
///    the loaded set. (It does not have to be in the same file — a
///    cyberlimb option can declare its prerequisite as a cyberlimb.)
/// 3. `humanity_loss = HumanityLossSpec::None` agrees with
///    `display_name` of items the book explicitly marks `0 (N/A)`. We
///    do **not** enforce this in code — the RON file is the
///    transcription of the book and an integration test in
///    `mod tests` cross-checks it.
///
/// On any failure returns [`RulesError::CatalogLoadFailed`].
pub fn load_cyberware_catalog(dir: &Path) -> Result<Catalog<Cyberware>, RulesError> {
    let mut files: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| RulesError::CatalogLoadFailed {
            path: dir.to_path_buf(),
            source: format!("read_dir failed: {e}"),
        })?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
        .collect();
    files.sort();

    if files.is_empty() {
        return Err(RulesError::CatalogLoadFailed {
            path: dir.to_path_buf(),
            source: "no .ron files found in cyberware catalog directory".into(),
        });
    }

    let mut entries: HashMap<String, Cyberware> = HashMap::new();
    let mut all_slugs: Vec<String> = Vec::new();

    for path in &files {
        let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.clone(),
            source: format!("read failed: {e}"),
        })?;
        let parsed: CyberwareFile =
            ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
                path: path.clone(),
                source: format!("parse failed: {e}"),
            })?;

        for row in parsed.cyberware {
            if entries.contains_key(&row.slug) {
                return Err(RulesError::CatalogLoadFailed {
                    path: path.clone(),
                    source: format!("duplicate slug across cyberware files: '{}'", row.slug),
                });
            }
            let prerequisite = row
                .prerequisite_slug
                .as_ref()
                .map(|s| CyberwareId(s.clone()));
            let entry = Cyberware {
                id: CyberwareId(row.slug.clone()),
                display_name: row.display_name,
                category: row.category,
                install_difficulty: row.install_difficulty,
                humanity_loss: row.humanity_loss,
                price: row.price,
                price_eb: Eurobucks(row.price_eb),
                option_slots: row.option_slots,
                slot_cost: row.slot_cost,
                prerequisite,
                effects: row.effects,
                description: row.description,
            };
            all_slugs.push(row.slug.clone());
            entries.insert(row.slug, entry);
        }
    }

    // Second pass: every prerequisite_slug must resolve.
    for entry in entries.values() {
        if let Some(req) = &entry.prerequisite {
            if !entries.contains_key(&req.0) {
                return Err(RulesError::CatalogLoadFailed {
                    path: dir.to_path_buf(),
                    source: format!(
                        "cyberware '{}' references unknown prerequisite '{}'",
                        entry.id.0, req.0
                    ),
                });
            }
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

    /// Workspace-relative path to the canonical cyberware catalog
    /// directory (`content/catalogs/cyberware/`).
    fn catalog_dir() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("cyberware");
        p
    }

    /// Acceptance: every cyberware entry from pp.358–367 is present.
    ///
    /// Per-category counts (verified against the v1.25 PDF):
    /// - **Fashionware** (p.358): Biomonitor, Chemskin, EMP Threading,
    ///   Light Tattoo, Shift Tacts, Skinwatch, Techhair = **7**.
    /// - **Neuralware** (pp.359–360): Neural Link, Braindance Recorder,
    ///   Chipware Socket, Interface Plugs, Kerenzikov, Sandevistan,
    ///   Chemical Analyzer, Memory Chip, Olfactory Boost, Pain Editor,
    ///   Skill Chip, Tactile Boost = **12**.
    /// - **Cyberoptics** (pp.360–361): Cybereye, Anti-Dazzle, Chyron,
    ///   Color Shift, Dartgun, Image Enhance, Low Light/Infrared/UV,
    ///   MicroOptics, MicroVideo, Radiation Detector, Targeting Scope,
    ///   TeleOptics, Virtuality = **13**.
    /// - **Cyberaudio** (pp.361–362): Cyberaudio Suite, Amplified
    ///   Hearing, Audio Recorder, Bug Detector, Homing Tracer, Internal
    ///   Agent, Level Damper, Radio Communicator, Radio Scanner /
    ///   Music Player, Radar Detector, Scrambler / Descrambler,
    ///   Voice Stress Analyzer = **12**.
    /// - **Internal Body** (pp.362–363): AudioVox, Contraceptive
    ///   Implant, Enhanced Antibodies, Cybersnake, Gills, Grafted
    ///   Muscle and Bone Lace, Independent Air Supply, Midnight Lady
    ///   Sexual Implant, Mr. Studd Sexual Implant, Nasal Filters,
    ///   Radar/Sonar Implant, Toxin Binders, Vampyres = **13**.
    /// - **External Body** (p.364): Hidden Holster, Skin Weave,
    ///   Subdermal Armor, Subdermal Pocket = **4**.
    /// - **Cyberlimb** (pp.364–367): Cyberarm, Standard Hand, Big
    ///   Knucks, Cyberdeck, Grapple Hand, Medscanner, Popup Grenade
    ///   Launcher, Popup Melee Weapon, Popup Shield, Popup Ranged
    ///   Weapon, Quick Change Mount, Rippers, Scratchers, Shoulder Cam,
    ///   Slice 'N Dice, Subdermal Grip, Techscanner, Tool Hand,
    ///   Wolvers, Cyberleg, Standard Foot, Grip Foot, Jump Booster,
    ///   Skate Foot, Talon Foot, Web Foot, Hardened Shielding, Plastic
    ///   Covering, Realskinn Covering, Superchrome Covering = **30**.
    /// - **Borgware** (p.367): Artificial Shoulder Mount, Implanted
    ///   Linear Frame Sigma, Implanted Linear Frame Beta, MultiOptic
    ///   Mount, Sensor Array = **5**.
    ///
    /// Total: 7 + 12 + 13 + 12 + 13 + 4 + 30 + 5 = **96**.
    #[test]
    fn test_cyberware_catalog_complete() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            96,
            "expected 96 cyberware entries per pp.358-367 (got {})",
            cat.len()
        );

        let mut by_cat: HashMap<CyberwareCategory, usize> = HashMap::new();
        for (_, cw) in cat.iter() {
            *by_cat.entry(cw.category).or_insert(0) += 1;
        }
        assert_eq!(by_cat.get(&CyberwareCategory::Fashionware), Some(&7));
        assert_eq!(by_cat.get(&CyberwareCategory::Neuralware), Some(&12));
        assert_eq!(by_cat.get(&CyberwareCategory::Cyberoptics), Some(&13));
        assert_eq!(by_cat.get(&CyberwareCategory::Cyberaudio), Some(&12));
        assert_eq!(by_cat.get(&CyberwareCategory::InternalBody), Some(&13));
        assert_eq!(by_cat.get(&CyberwareCategory::ExternalBody), Some(&4));
        assert_eq!(by_cat.get(&CyberwareCategory::Cyberlimb), Some(&30));
        assert_eq!(by_cat.get(&CyberwareCategory::Borgware), Some(&5));
    }

    /// Acceptance: Neural Link is foundational — no prerequisite.
    /// See p.112 / p.359: "Wired artificial nervous system, required to
    /// use Neuralware and Subdermal Grips."
    #[test]
    fn test_neural_link_no_prereq() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let nl = cat.get("neural_link").expect("neural_link present");
        assert_eq!(nl.prerequisite, None, "Neural Link is foundational");
        assert_eq!(nl.option_slots, 5, "Neural Link has 5 Option Slots (p.359)");
    }

    /// Acceptance: Interface Plugs require Neural Link.
    /// See p.359: "Plugs in the wrist or head … *Requires Neural Link*."
    #[test]
    fn test_interface_plugs_require_neural_link() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let ip = cat.get("interface_plugs").expect("interface_plugs present");
        assert_eq!(
            ip.prerequisite,
            Some(CyberwareId("neural_link".into())),
            "Interface Plugs prerequisite must be Neural Link"
        );
    }

    /// Acceptance: items the book lists with `0 (N/A)` HL load as
    /// `HumanityLossSpec::None`.
    ///
    /// "Medical-grade" in the WP description maps to the rulebook's
    /// `0 (N/A)` HL annotation (the catalog row's preset HL — distinct
    /// from the per-install medical-grade replacement-parts rule on
    /// p.226, which is a runtime concern). Per pp.358–367 the
    /// always-zero-HL items are: every Fashionware item (Biomonitor,
    /// Chemskin, EMP Threading, Light Tattoo, Shift Tacts, Skinwatch,
    /// Techhair), Memory Chip, Contraceptive Implant, and the three
    /// no-Option-Slot cyberlimb coverings (Plastic, Realskinn,
    /// Superchrome). Spot-checking three: Biomonitor (Fashionware),
    /// Memory Chip (Neuralware peripheral), Contraceptive Implant
    /// (Internal Body).
    #[test]
    fn test_medical_grade_zero_hl() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        for slug in [
            "biomonitor",
            "chemskin",
            "emp_threading",
            "light_tattoo",
            "shift_tacts",
            "skinwatch",
            "techhair",
            "memory_chip",
            "contraceptive_implant",
            "plastic_covering",
            "realskinn_covering",
            "superchrome_covering",
        ] {
            let cw = cat
                .get(slug)
                .unwrap_or_else(|| panic!("expected '{slug}' in catalog"));
            assert_eq!(
                cw.humanity_loss,
                HumanityLossSpec::None,
                "{slug}: book lists HL as 0 (N/A) — must load as HumanityLossSpec::None"
            );
        }
    }

    /// Acceptance: at-creation HL is encoded as the preset; in-play HL
    /// is `Rolled { fixed, dice }`.
    ///
    /// See p.111: *"At Character Generation, Humanity Loss is preset.
    /// After Character Generation, Humanity Loss is determined by the
    /// dice in parentheses following the preset number."*
    ///
    /// Sample: Neural Link is `7 (2d6)` on p.359 — preset 7,
    /// post-creation `2d6`. Cybersnake / Pain Editor / Kerenzikov are
    /// `14 (4d6)`. Anti-Dazzle is `2 (1d6/2 round up)`.
    #[test]
    fn test_humanity_loss_creation_vs_play() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");

        let nl = cat.get("neural_link").expect("neural_link present");
        assert_eq!(
            nl.humanity_loss,
            HumanityLossSpec::Rolled {
                fixed: 7,
                dice: DiceSpec {
                    n: 2,
                    die: DieKind::D6,
                    divisor: 1
                }
            },
            "Neural Link HL must be 7 (2d6) per p.359"
        );
        assert_eq!(nl.humanity_loss.fixed_value(), 7);

        let pain = cat.get("pain_editor").expect("pain_editor present");
        assert_eq!(
            pain.humanity_loss,
            HumanityLossSpec::Rolled {
                fixed: 14,
                dice: DiceSpec {
                    n: 4,
                    die: DieKind::D6,
                    divisor: 1
                }
            },
            "Pain Editor HL must be 14 (4d6) per p.360"
        );

        let antidazzle = cat.get("anti_dazzle").expect("anti_dazzle present");
        assert_eq!(
            antidazzle.humanity_loss,
            HumanityLossSpec::Rolled {
                fixed: 2,
                dice: DiceSpec {
                    n: 1,
                    die: DieKind::D6,
                    divisor: 2
                }
            },
            "Anti-Dazzle HL must be 2 (1d6/2 round up) per p.360"
        );

        // Sanity: a Fixed-only encoding still reports its at-creation
        // value via fixed_value(). (None of the items on pp.358-367
        // load as Fixed today — they're all Rolled-or-None — but the
        // Fixed variant is part of the public API per WP-204.)
        assert_eq!(HumanityLossSpec::Fixed(3).fixed_value(), 3);
        assert_eq!(HumanityLossSpec::None.fixed_value(), 0);
    }

    /// Acceptance: a `Cyberware` round-trips through RON serialisation.
    /// Pins the serde shape so a future schema change can't silently
    /// break the on-disk catalog.
    #[test]
    fn test_cyberware_round_trip_ron() {
        let original = Cyberware {
            id: CyberwareId("kerenzikov".into()),
            display_name: "Kerenzikov".into(),
            category: CyberwareCategory::Neuralware,
            install_difficulty: InstallLocation::Clinic,
            humanity_loss: HumanityLossSpec::Rolled {
                fixed: 14,
                dice: DiceSpec {
                    n: 4,
                    die: DieKind::D6,
                    divisor: 1,
                },
            },
            price: PriceTier::Expensive,
            price_eb: Eurobucks(500),
            option_slots: 0,
            slot_cost: 1,
            prerequisite: Some(CyberwareId("neural_link".into())),
            effects: vec![EffectModifier::InitiativeBonus(2)],
            description: "Always-on Speedware that provides consistently improved \
                          reaction time. User adds +2 to their Initiative Rolls. Only \
                          a single piece of Speedware can be installed into a user at \
                          a time."
                .into(),
        };

        let serialised = ron::ser::to_string(&original).expect("serialise");
        let restored: Cyberware = ron::de::from_str(&serialised).expect("round-trip");
        assert_eq!(restored, original);
    }

    /// Regression: the loaded catalog actually has Kerenzikov's
    /// Initiative bonus encoded — the WP description requires effect
    /// transcription for items that grant ongoing modifiers.
    #[test]
    fn test_kerenzikov_initiative_bonus_loaded() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let k = cat.get("kerenzikov").expect("kerenzikov present");
        assert!(
            k.effects
                .iter()
                .any(|m| matches!(m, EffectModifier::InitiativeBonus(2))),
            "Kerenzikov must grant InitiativeBonus(+2) per p.359"
        );
    }

    /// Regression: every prerequisite_slug in the loaded catalog
    /// resolves. The loader enforces this at parse time; the test
    /// pins the invariant against accidental loader-bypass changes.
    #[test]
    fn test_every_prerequisite_resolves() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        for (slug, cw) in cat.iter() {
            if let Some(req) = &cw.prerequisite {
                assert!(
                    cat.get(&req.0).is_some(),
                    "cyberware '{slug}' references missing prerequisite '{}'",
                    req.0,
                );
            }
        }
    }

    /// Regression: foundational pieces declare their option slot counts
    /// per the rulebook. Pinned because downstream WPs (cyberware
    /// install action, character sheet rendering) key off these
    /// numbers.
    #[test]
    fn test_foundational_option_slots() {
        let cat = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        // Neural Link has 5 Option Slots (p.359).
        assert_eq!(cat.get("neural_link").unwrap().option_slots, 5);
        // Cybereye has 3 (p.360).
        assert_eq!(cat.get("cybereye").unwrap().option_slots, 3);
        // Cyberaudio Suite has 3 (p.361).
        assert_eq!(cat.get("cyberaudio_suite").unwrap().option_slots, 3);
        // Cyberarm has 4 (p.364).
        assert_eq!(cat.get("cyberarm").unwrap().option_slots, 4);
        // Cyberleg has 3 (p.366).
        assert_eq!(cat.get("cyberleg").unwrap().option_slots, 3);
    }
}
