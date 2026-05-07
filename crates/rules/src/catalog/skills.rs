//! Skill catalog (WP-201).
//!
//! Defines the closed enum [`SkillId`] of every Skill in the rulebook
//! (pp.81–90), together with the metadata catalog [`SkillDefinition`] (linked
//! Stat, category, ×2 cost flag, description) and the RON loader
//! [`load_skills_catalog`].
//!
//! Rulebook references:
//! - **pp.81–84:** the canonical skill list grouped by category, each entry
//!   labelled with its linked Stat and (×2) marker if applicable.
//! - **pp.85–89:** Streetrat / Edgerunner skill tables — provide the basic
//!   skill set including `Language(Streetslang)` and `LocalExpert(Your Home)`.
//! - **pp.131–142:** rank-10 / 14 / 18 flavour examples, summarised into
//!   each entry's `description`.
//!
//! The catalog file is `content/catalogs/skills.ron`; the loader expects
//! one entry per slug. Parameterised skills (`Language`, `LocalExpert`,
//! `Science`, `MartialArts`, `PlayInstrument`) are catalogued under their
//! canonical default — campaign-specific instantiations
//! (e.g. `LocalExpert(Custom("Watson"))`) are constructed on the fly.

use crate::catalog::Catalog;
use crate::error::RulesError;
use crate::types::Stat;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Parameterised inner enums
// ---------------------------------------------------------------------------

/// Variants of the `Language` skill (rulebook p.83).
///
/// The book names *Streetslang* explicitly as a Basic Skill on p.85
/// ("`Language (Streetslang)`"), and `Custom(String)` allows any
/// campaign-specific language (English, Japanese, Klingon, …) without
/// expanding this enum every time.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LanguageKind {
    /// The shared underground argot of the Time of the Red. Basic Skill
    /// per p.85.
    Streetslang,
    /// Any other named language. Carries the language's display name.
    Custom(String),
}

/// Variants of the `Science` skill (rulebook p.83).
///
/// The book lists the named options explicitly: "Possible options include:
/// Geology, Mathematics, Physics, Zoology, Anthropology, Biology, Chemistry,
/// and History." (p.83 / p.135). `Custom(String)` accommodates any field
/// the GM allows.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ScienceField {
    Geology,
    Mathematics,
    Physics,
    Zoology,
    Anthropology,
    Biology,
    Chemistry,
    History,
    /// Any other field of science (e.g. "Astrophysics").
    Custom(String),
}

/// Variants of the `Martial Arts` skill (rulebook p.83 / p.137).
///
/// The book names "Karate, Taekwondo, Judo, Aikido" as the canonical
/// options on p.83 and references p.178 for the broader list (Boxing,
/// Capoeira, Wrestling, Animal Kung Fu). `Custom(String)` covers anything
/// the GM permits.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MartialArtsForm {
    Karate,
    Taekwondo,
    Judo,
    Aikido,
    Boxing,
    Capoeira,
    Wrestling,
    AnimalKungFu,
    /// Any other martial arts form.
    Custom(String),
}

/// Variants of the `Play Instrument` skill (rulebook p.83 / p.137).
///
/// The book lists "singing, guitar, drums, violin, piano, etc." as canonical
/// options on p.83. `Custom(String)` covers any other instrument.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Instrument {
    Singing,
    Guitar,
    Drums,
    Violin,
    Piano,
    /// Any other instrument.
    Custom(String),
}

/// Variants of the `Local Expert` skill (rulebook p.83 / p.135).
///
/// Per p.83: "You must choose a specific location whenever you increase
/// this Skill, which cannot be any larger than a single neighborhood or
/// community." Always parameterised — the book's only canonical default
/// is *"(Your Home)"* (p.85, Basic Skill list), which is itself a
/// content-defined string. We model that as `Custom("Your Home")` rather
/// than promoting it to a named variant; everything is a `Custom`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LocalArea {
    /// A specific named area — neighbourhood, community, or comparable.
    Custom(String),
}

// ---------------------------------------------------------------------------
// SkillId
// ---------------------------------------------------------------------------

/// Identifier for every Skill in the *Cyberpunk RED* rulebook.
///
/// This is a **closed enum**: every entry corresponds to a Skill listed on
/// pp.81–84. Parameterised skills (`Language`, `LocalExpert`, `Science`,
/// `MartialArts`, `PlayInstrument`) carry their inner choice as an enum
/// payload so the type system catches "you forgot to specify which
/// language" at compile time.
///
/// Variant naming follows the book's display names normalised to
/// `UpperCamelCase` (e.g. *Conceal/Reveal Object* → `ConcealRevealObject`,
/// *Drive Land Vehicle* → `DriveLandVehicle`, *Resist Torture/Drugs* →
/// `ResistTortureDrugs`). One concession to readability: the book's
/// *Accounting* (p.82) is encoded as `AccountingFinance` because the
/// official errata and Streetrat tables (p.86) refer to the same skill
/// either way and the broader name is more self-documenting in code; the
/// catalog's `display_name` field still reads "Accounting".
///
/// `Hash + Eq` enable use as a `HashMap` key (see
/// [`crate::character::SkillSet`]).
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum SkillId {
    // -- Awareness Skills (p.82) --
    Concentration,
    ConcealRevealObject,
    LipReading,
    Perception,
    Tracking,

    // -- Body Skills (p.82) --
    Athletics,
    Contortionist,
    Dance,
    Endurance,
    ResistTortureDrugs,
    Stealth,

    // -- Control Skills (p.82) --
    DriveLandVehicle,
    PilotAirVehicle,
    PilotSeaVehicle,
    Riding,

    // -- Education Skills (pp.82–83) --
    AccountingFinance,
    AnimalHandling,
    Bureaucracy,
    Business,
    Composition,
    Criminology,
    Cryptography,
    Deduction,
    Education,
    Gamble,
    Language(LanguageKind),
    LibrarySearch,
    LocalExpert(LocalArea),
    Science(ScienceField),
    Tactics,
    WildernessSurvival,

    // -- Fighting Skills (p.83) --
    Brawling,
    Evasion,
    MartialArts(MartialArtsForm),
    MeleeWeapon,

    // -- Performance Skills (p.83) --
    Acting,
    PlayInstrument(Instrument),

    // -- Ranged Weapon Skills (pp.83–84) --
    Archery,
    Autofire,
    Handgun,
    HeavyWeapons,
    ShoulderArms,

    // -- Social Skills (p.84) --
    Bribery,
    Conversation,
    HumanPerception,
    Interrogation,
    Persuasion,
    PersonalGrooming,
    Streetwise,
    Trading,
    WardrobeStyle,

    // -- Technique Skills (pp.84–85) --
    AirVehicleTech,
    BasicTech,
    Cybertech,
    DemolitionsTech,
    ElectronicsSecurityTech,
    FirstAid,
    Forgery,
    LandVehicleTech,
    PaintDrawSculpt,
    Paramedic,
    PhotographyFilm,
    PickLock,
    PickPocket,
    SeaVehicleTech,
    Weaponstech,
}

// ---------------------------------------------------------------------------
// Category
// ---------------------------------------------------------------------------

/// The nine Skill categories listed on p.81.
///
/// Categories drive flavour grouping (Skill sheet, training montages) and
/// are not used in dice resolution — but they're recorded so the GM /
/// frontend can render the standard CPR Skill sheet layout.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SkillCategory {
    Awareness,
    Body,
    Control,
    Education,
    Fighting,
    Performance,
    Ranged,
    Social,
    Technique,
}

// ---------------------------------------------------------------------------
// SkillDefinition
// ---------------------------------------------------------------------------

/// A row in the canonical skill catalog (`content/catalogs/skills.ron`).
///
/// Loaded by [`load_skills_catalog`]. The `id` carries the closed-enum
/// shape; the slug used as the `Catalog<T>` key is a normalised string
/// derived from the variant name (e.g. `concentration`,
/// `language_streetslang`). See pp.81–90 for the source data.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillDefinition {
    /// Closed-enum identifier — the canonical handle for this skill.
    pub id: SkillId,
    /// Display name as printed in the rulebook (p.81 onward), preserving
    /// the book's punctuation (e.g. "Conceal/Reveal Object").
    pub display_name: String,
    /// The Stat each Skill is linked to per the pp.81–84 table.
    pub linked_stat: Stat,
    /// Category banner under which the rulebook prints this skill.
    pub category: SkillCategory,
    /// `true` iff the skill carries the (×2) marker on pp.81–84 ("costs
    /// twice the number of points to buy").
    pub double_cost: bool,
    /// Human-readable summary, drawn from the Skill List section
    /// (pp.131–142). The LLM narrator uses this for flavour; the engine
    /// itself ignores the text.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Linked-stat lookup
// ---------------------------------------------------------------------------

/// Return the Stat each Skill is linked to per the rulebook (pp.81–84).
///
/// Replaces the WP-104 stub default of `Stat::Int` in
/// [`crate::character::Character::skill_base`]. This function is the single
/// source of truth for skill→stat mapping in the rules engine; the catalog
/// RON file's `linked_stat` field must agree (a regression test in this
/// module enforces the contract).
///
/// Parameterised skills inherit the parent skill's linked stat — every
/// `Language` regardless of language is INT, every `MartialArts` is DEX,
/// etc. — exactly per the table on pp.82–83.
pub fn linked_stat(skill: &SkillId) -> Stat {
    match skill {
        // Awareness — see p.82.
        SkillId::Concentration => Stat::Will,
        SkillId::ConcealRevealObject => Stat::Int,
        SkillId::LipReading => Stat::Int,
        SkillId::Perception => Stat::Int,
        SkillId::Tracking => Stat::Int,

        // Body — see p.82.
        SkillId::Athletics => Stat::Dex,
        SkillId::Contortionist => Stat::Dex,
        SkillId::Dance => Stat::Dex,
        SkillId::Endurance => Stat::Will,
        SkillId::ResistTortureDrugs => Stat::Will,
        SkillId::Stealth => Stat::Dex,

        // Control — see p.82.
        SkillId::DriveLandVehicle => Stat::Ref,
        SkillId::PilotAirVehicle => Stat::Ref,
        SkillId::PilotSeaVehicle => Stat::Ref,
        SkillId::Riding => Stat::Ref,

        // Education — see pp.82–83.
        SkillId::AccountingFinance => Stat::Int,
        SkillId::AnimalHandling => Stat::Int,
        SkillId::Bureaucracy => Stat::Int,
        SkillId::Business => Stat::Int,
        SkillId::Composition => Stat::Int,
        SkillId::Criminology => Stat::Int,
        SkillId::Cryptography => Stat::Int,
        SkillId::Deduction => Stat::Int,
        SkillId::Education => Stat::Int,
        SkillId::Gamble => Stat::Int,
        SkillId::Language(_) => Stat::Int,
        SkillId::LibrarySearch => Stat::Int,
        SkillId::LocalExpert(_) => Stat::Int,
        SkillId::Science(_) => Stat::Int,
        SkillId::Tactics => Stat::Int,
        SkillId::WildernessSurvival => Stat::Int,

        // Fighting — see p.83.
        SkillId::Brawling => Stat::Dex,
        SkillId::Evasion => Stat::Dex,
        SkillId::MartialArts(_) => Stat::Dex,
        SkillId::MeleeWeapon => Stat::Dex,

        // Performance — see p.83.
        SkillId::Acting => Stat::Cool,
        SkillId::PlayInstrument(_) => Stat::Tech,

        // Ranged Weapon — see pp.83–84.
        SkillId::Archery => Stat::Ref,
        SkillId::Autofire => Stat::Ref,
        SkillId::Handgun => Stat::Ref,
        SkillId::HeavyWeapons => Stat::Ref,
        SkillId::ShoulderArms => Stat::Ref,

        // Social — see p.84.
        SkillId::Bribery => Stat::Cool,
        SkillId::Conversation => Stat::Emp,
        SkillId::HumanPerception => Stat::Emp,
        SkillId::Interrogation => Stat::Cool,
        SkillId::Persuasion => Stat::Cool,
        SkillId::PersonalGrooming => Stat::Cool,
        SkillId::Streetwise => Stat::Cool,
        SkillId::Trading => Stat::Cool,
        SkillId::WardrobeStyle => Stat::Cool,

        // Technique — see pp.84–85.
        SkillId::AirVehicleTech => Stat::Tech,
        SkillId::BasicTech => Stat::Tech,
        SkillId::Cybertech => Stat::Tech,
        SkillId::DemolitionsTech => Stat::Tech,
        SkillId::ElectronicsSecurityTech => Stat::Tech,
        SkillId::FirstAid => Stat::Tech,
        SkillId::Forgery => Stat::Tech,
        SkillId::LandVehicleTech => Stat::Tech,
        SkillId::PaintDrawSculpt => Stat::Tech,
        SkillId::Paramedic => Stat::Tech,
        SkillId::PhotographyFilm => Stat::Tech,
        SkillId::PickLock => Stat::Tech,
        SkillId::PickPocket => Stat::Tech,
        SkillId::SeaVehicleTech => Stat::Tech,
        SkillId::Weaponstech => Stat::Tech,
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/skills.ron`.
///
/// The file is a `(skills: [ ... ])` envelope where each entry is a
/// `SkillDefinition` plus an explicit `slug` field (the `Catalog<T>` key).
/// Decoupling the on-disk schema from the in-memory `Catalog<T>` lets the
/// authored content stay readable (a flat list, not a map literal) while
/// the loader computes the lookup map.
#[derive(Debug, Deserialize)]
struct SkillsFile {
    skills: Vec<SkillsFileEntry>,
}

/// One row in the on-disk skills catalog file.
///
/// `slug` is the lookup key inside the resulting `Catalog<SkillDefinition>`.
/// All other fields populate the [`SkillDefinition`] directly.
#[derive(Debug, Deserialize)]
struct SkillsFileEntry {
    slug: String,
    id: SkillId,
    display_name: String,
    linked_stat: Stat,
    category: SkillCategory,
    double_cost: bool,
    description: String,
}

/// Load the skills catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<SkillDefinition>`] keyed by slug. On
/// failure returns [`RulesError::CatalogLoadFailed`] carrying the file path
/// and a stringified description of the underlying I/O or parse error.
///
/// The loader enforces two invariants:
/// 1. Every entry's `linked_stat` agrees with [`linked_stat`] for that
///    `id`. A mismatch fails the load — keeping the catalog file from
///    silently disagreeing with the in-code lookup.
/// 2. Slugs are unique within the file. A duplicate slug fails the load.
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader
/// loading conventions every Phase 2 catalog follows.
pub fn load_skills_catalog(path: &Path) -> Result<Catalog<SkillDefinition>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: SkillsFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, SkillDefinition> = HashMap::with_capacity(parsed.skills.len());
    for row in parsed.skills {
        let expected_stat = linked_stat(&row.id);
        if expected_stat != row.linked_stat {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "skill '{}' has linked_stat {:?} in file but {:?} in code (linked_stat lookup)",
                    row.slug, row.linked_stat, expected_stat
                ),
            });
        }
        let def = SkillDefinition {
            id: row.id,
            display_name: row.display_name,
            linked_stat: row.linked_stat,
            category: row.category,
            double_cost: row.double_cost,
            description: row.description,
        };
        if entries.insert(row.slug.clone(), def).is_some() {
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

    /// Workspace-relative path to the canonical skills catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog lives
    /// two parents up at `content/catalogs/skills.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("skills.ron");
        p
    }

    /// Acceptance: every Skill listed on pp.81–84 must appear in the
    /// catalog, and the catalog must contain exactly that many entries
    /// (no duplicates, no extras).
    ///
    /// Count derivation (verified against pp.81–84):
    /// - Awareness: 5 (Concentration, Conceal/Reveal Object, Lip Reading,
    ///   Perception, Tracking)
    /// - Body: 6 (Athletics, Contortionist, Dance, Endurance,
    ///   Resist Torture/Drugs, Stealth)
    /// - Control: 4 (Drive Land Vehicle, Pilot Air Vehicle, Pilot Sea
    ///   Vehicle, Riding)
    /// - Education: 16 (Accounting, Animal Handling, Bureaucracy,
    ///   Business, Composition, Criminology, Cryptography, Deduction,
    ///   Education, Gamble, Language [parameterised, one entry],
    ///   Library Search, Local Expert [parameterised], Science
    ///   [parameterised], Tactics, Wilderness Survival)
    /// - Fighting: 4 (Brawling, Evasion, Martial Arts [parameterised],
    ///   Melee Weapon)
    /// - Performance: 2 (Acting, Play Instrument [parameterised])
    /// - Ranged: 5 (Archery, Autofire, Handgun, Heavy Weapons, Shoulder Arms)
    /// - Social: 9 (Bribery, Conversation, Human Perception, Interrogation,
    ///   Persuasion, Personal Grooming, Streetwise, Trading, Wardrobe & Style)
    /// - Technique: 15 (Air Vehicle Tech, Basic Tech, Cybertech,
    ///   Demolitions, Electronics/Security Tech, First Aid, Forgery,
    ///   Land Vehicle Tech, Paint/Draw/Sculpt, Paramedic,
    ///   Photography/Film, Pick Lock, Pick Pocket, Sea Vehicle Tech,
    ///   Weaponstech)
    ///
    /// Total: 5 + 6 + 4 + 16 + 4 + 2 + 5 + 9 + 15 = **66**.
    #[test]
    fn test_all_skills_loaded() {
        let cat = load_skills_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            66,
            "expected 66 skills per pp.81-84 (got {}); verify the catalog RON file",
            cat.len()
        );
    }

    /// Acceptance: every (×2) skill flagged on pp.81–84 has
    /// `double_cost == true` in the loaded catalog.
    ///
    /// Per RAW (pp.82–84) the (×2) skills are: Pilot Air Vehicle,
    /// Martial Arts, Autofire, Heavy Weapons, Demolitions,
    /// Electronics/Security Tech, Paramedic. (See PR description for the
    /// note on why *Resist Torture/Drugs* is **not** in this list — it
    /// carries no (×2) marker on p.82.)
    #[test]
    fn test_double_cost_flagged() {
        let cat = load_skills_catalog(&catalog_path()).expect("catalog must load");

        let must_be_double = [
            "pilot_air_vehicle",
            "martial_arts_karate", // any MartialArts variant slug works
            "autofire",
            "heavy_weapons",
            "demolitions_tech",
            "electronics_security_tech",
            "paramedic",
        ];
        for slug in must_be_double {
            let def = cat
                .get(slug)
                .unwrap_or_else(|| panic!("missing (×2) skill: {slug}"));
            assert!(
                def.double_cost,
                "{slug} must be flagged double_cost (×2 per pp.82-84)"
            );
        }

        // And spot-check a non-(×2) skill stays false.
        let handgun = cat.get("handgun").expect("handgun must be present");
        assert!(
            !handgun.double_cost,
            "Handgun is not (×2); see p.84 — must not be flagged"
        );
    }

    /// Acceptance: `linked_stat` agrees with the rulebook for a sample
    /// of 10 skills covering 5 distinct stats (DEX, WILL, INT, REF, COOL).
    #[test]
    fn test_linked_stat_correct_sample() {
        // Exercise the in-code lookup directly — independent of the RON
        // file. Sample picks one skill per category where possible.
        assert_eq!(linked_stat(&SkillId::Athletics), Stat::Dex); // Body
        assert_eq!(linked_stat(&SkillId::Concentration), Stat::Will); // Awareness
        assert_eq!(linked_stat(&SkillId::Tactics), Stat::Int); // Education
        assert_eq!(linked_stat(&SkillId::Brawling), Stat::Dex); // Fighting
        assert_eq!(linked_stat(&SkillId::Education), Stat::Int); // Education
        assert_eq!(linked_stat(&SkillId::Stealth), Stat::Dex); // Body
        assert_eq!(linked_stat(&SkillId::Handgun), Stat::Ref); // Ranged
        assert_eq!(linked_stat(&SkillId::Persuasion), Stat::Cool); // Social
        assert_eq!(linked_stat(&SkillId::Conversation), Stat::Emp); // Social
        assert_eq!(linked_stat(&SkillId::FirstAid), Stat::Tech); // Technique
    }

    /// Acceptance: parameterised skills round-trip through RON with
    /// `Custom(String)` payloads.
    #[test]
    fn test_parameterised_skills_serializable() {
        let original = SkillId::Language(LanguageKind::Custom("Astrophysics".to_string()));
        let serialised = ron::ser::to_string(&original).expect("must serialise");
        let restored: SkillId = ron::de::from_str(&serialised).expect("must round-trip");
        assert_eq!(restored, original);

        // And the named variants.
        let karate = SkillId::MartialArts(MartialArtsForm::Karate);
        let s = ron::ser::to_string(&karate).expect("must serialise");
        let back: SkillId = ron::de::from_str(&s).expect("must round-trip");
        assert_eq!(back, karate);

        let physics = SkillId::Science(ScienceField::Physics);
        let s = ron::ser::to_string(&physics).expect("must serialise");
        let back: SkillId = ron::de::from_str(&s).expect("must round-trip");
        assert_eq!(back, physics);
    }

    /// Regression: `linked_stat` returns the documented Stat for a
    /// single canonical pairing. Pinned because the WP description
    /// mandates this exact mapping for `Handgun`.
    #[test]
    fn test_linked_stat_function() {
        assert_eq!(linked_stat(&SkillId::Handgun), Stat::Ref);
    }

    /// Regression: `SkillId` is `Hash + Eq`, so it can be used as a
    /// `HashMap` key (the storage in
    /// [`crate::character::SkillSet::ranks`] depends on this).
    #[test]
    fn test_skill_id_hash_works() {
        let mut m: HashMap<SkillId, u8> = HashMap::new();
        m.insert(SkillId::Handgun, 4);
        m.insert(SkillId::Stealth, 2);
        m.insert(SkillId::Language(LanguageKind::Streetslang), 6);
        m.insert(SkillId::LocalExpert(LocalArea::Custom("Watson".into())), 4);

        assert_eq!(m.get(&SkillId::Handgun).copied(), Some(4));
        assert_eq!(m.get(&SkillId::Stealth).copied(), Some(2));
        assert_eq!(
            m.get(&SkillId::Language(LanguageKind::Streetslang))
                .copied(),
            Some(6)
        );
        assert_eq!(
            m.get(&SkillId::LocalExpert(LocalArea::Custom("Watson".into())))
                .copied(),
            Some(4)
        );
        // Different LocalArea string → different key.
        assert!(!m.contains_key(&SkillId::LocalExpert(LocalArea::Custom("Heywood".into()))));
    }

    /// Regression: every `SkillDefinition` loaded from the RON file has
    /// `linked_stat` matching the in-code [`linked_stat`] lookup. The
    /// loader checks this at parse time, but the test pins the
    /// invariant so an accidental loader-bypass change can't slip past.
    #[test]
    fn test_catalog_linked_stats_agree_with_lookup() {
        let cat = load_skills_catalog(&catalog_path()).expect("catalog must load");
        for (slug, def) in cat.iter() {
            assert_eq!(
                def.linked_stat,
                linked_stat(&def.id),
                "catalog '{slug}' linked_stat disagrees with in-code lookup"
            );
        }
    }
}
