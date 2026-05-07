//! Lifepath tables (WP-214).
//!
//! Defines the structured Streetrat / Complete-Package Lifepath record
//! used by [`crate::character::Character::lifepath`] together with the
//! generic [`LifepathTable<T>`] over which every authored RON table is
//! parsed. Tables cover the pp.43–69 ("Tales from The Street") shared
//! lifepath plus the ten role-specific lifepaths (Rockerboy through
//! Nomad).
//!
//! Every public table loader takes a path to a RON file in
//! `content/tables/lifepath/` and yields a typed
//! [`LifepathTable<T>`]. Loaders enforce two invariants:
//! 1. The declared `die` size matches the inclusive max roll value
//!    (`d10` ⇒ entries cover 1..=10, `d6` ⇒ 1..=6).
//! 2. Every `(roll, value)` entry's roll is in `1..=die`, and rolls
//!    are unique within the table.
//!
//! Sampling (rolling on a table) is **not** implemented here — that
//! belongs to a downstream WP that wires lifepath generation into
//! the deterministic [`crate::Rng`]. The tables themselves are pure
//! data.
//!
//! Rulebook references:
//! - **pp.43–53:** Shared "Tales from The Street" lifepath — Cultural
//!   Origins, Personality, Dress and Personal Style (Clothing Style,
//!   Hairstyle, Affectation), Motivations and Relationships
//!   (motivation, attitude toward people, valued person, valued
//!   possession), Family Background, Childhood Environment, Family
//!   Crisis, Friends, Enemies, Sweet Revenge, Tragic Loves, Life
//!   Goals.
//! - **pp.54–69:** Role-specific lifepath tables — one section per
//!   role (Rockerboy, Solo, Netrunner, Tech, Medtech, Media, Exec,
//!   Lawman, Fixer, Nomad).
//!
//! See `IMPLEMENTATION_PLAN.md` §4 (WP-214) for the public-API
//! contract; this module is the canonical implementation.

use crate::error::RulesError;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

// ---------------------------------------------------------------------------
// LifepathTable<T>
// ---------------------------------------------------------------------------

/// One authored lifepath table — `die` size plus an entry per legal roll.
///
/// `die` is `6` or `10` for every printed table on pp.43–69. `entries`
/// is a flat `Vec<(roll, value)>`; the loader guarantees each roll in
/// `1..=die` is present exactly once. Storage as a flat `Vec` (rather
/// than a `HashMap<u8, T>`) keeps insertion order stable for snapshot
/// tests and makes RON content read top-down.
///
/// `Default` is implemented (and yields an empty `1d10` table) so a
/// downstream type that contains a `LifepathTable<T>` can derive
/// `Default` without pinning a `T: Default` bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LifepathTable<T> {
    /// Die size — the inclusive max of every legal roll in this table.
    /// Conventionally `6` or `10` per pp.43–69.
    pub die: u8,
    /// Ordered list of `(roll, value)` pairs, one per legal roll.
    pub entries: Vec<(u8, T)>,
}

impl<T> Default for LifepathTable<T> {
    fn default() -> Self {
        Self {
            die: 10,
            entries: Vec::new(),
        }
    }
}

impl<T> LifepathTable<T> {
    /// Look up the entry for a specific roll.
    ///
    /// Returns `None` if `roll` is outside `1..=die` or the table is
    /// missing an entry for that roll (which the loader normally
    /// prevents). Sampling code should treat `None` as a programmer
    /// error rather than a recoverable condition.
    pub fn lookup(&self, roll: u8) -> Option<&T> {
        self.entries
            .iter()
            .find_map(|(r, v)| (*r == roll).then_some(v))
    }

    /// Number of entries in the table.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` iff the table has zero entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Lifepath top-level record
// ---------------------------------------------------------------------------

/// One character's structured lifepath record.
///
/// Fields capture the Streetrat / Complete-Package output from
/// pp.43–69: cultural region, personality, look, motivations, family
/// background and crisis, relationship beacons (friends, enemies,
/// tragic loves), and the role-specific lifepath. Values are
/// human-readable strings as printed in the rulebook so the LLM
/// narrator can quote them verbatim; structured catalogs (cyberware
/// etc.) live in their own WPs.
///
/// `Default` produces an "empty" lifepath — every string blank,
/// `family_background` defaulted, all relationship vectors empty,
/// and `role_specific = RoleLifepath::Solo(Default::default())`.
/// This matches the constructor shape used in
/// `world::test_support::fresh_pc()` and lets callers build a
/// minimal Character without authoring a full lifepath roll.
///
/// See pp.43–53 for the shared fields and pp.54–69 for `role_specific`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lifepath {
    /// Cultural region (one of the ten on p.45's table). Empty string
    /// means unrolled.
    pub cultural_region: String,
    /// Native language tied to `cultural_region` — see the right-hand
    /// column of p.45. The book grants 4 ranks of `Language(<this>)`.
    pub language_spoken: String,
    /// Personality archetype — see p.46.
    pub personality: String,
    /// Clothing-style label — see p.47's left column.
    pub clothing_style: String,
    /// Hairstyle — see p.47's right column.
    pub hairstyle: String,
    /// Affectation worn always — see p.47's bottom table.
    pub affectations: String,
    /// What you value most — see p.48's left column.
    pub motivation: String,
    /// Long-term life goal — see p.53.
    pub life_goal: String,
    /// Original family background. See p.49.
    pub family_background: FamilyBackground,
    /// What happened to your family. See p.50's right table.
    pub family_crisis: String,
    /// Friends — relationship beacons. See p.51's top table.
    pub friends: Vec<RelationshipBeacon>,
    /// Enemies — relationship beacons. See p.51's bottom table.
    pub enemies: Vec<RelationshipBeacon>,
    /// Tragic love affairs — relationship beacons. See p.52.
    pub tragic_loves: Vec<RelationshipBeacon>,
    /// Role-specific lifepath roll-up. See pp.54–69.
    pub role_specific: RoleLifepath,
}

// ---------------------------------------------------------------------------
// FamilyBackground
// ---------------------------------------------------------------------------

/// Family background captured from pp.49–50.
///
/// `original_background` and `feelings_about_people` come from the
/// p.48 / p.49 tables; the book threads the *Most Valued Person*,
/// *Most Valued Possession*, and *Childhood Environment* tables through
/// the same section (pp.48, p.50). All fields are free-form strings —
/// the loader fills them from the relevant table after a roll, but
/// `Default` yields blanks.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyBackground {
    /// Family background label (e.g. "Corporate Execs"). See p.49.
    pub original_background: String,
    /// Long-form description of the background. See p.49.
    pub description: String,
    /// Childhood environment — see p.50's left table.
    pub childhood_environment: String,
    /// How you feel about most people — see p.48's right column.
    pub feelings_about_people: String,
    /// Most valued person in your life — see p.48's bottom-left table.
    pub most_valued_person: String,
    /// Most valued possession you own — see p.48's bottom-right table.
    pub most_valued_possession: String,
}

// ---------------------------------------------------------------------------
// RelationshipBeacon
// ---------------------------------------------------------------------------

/// One person tied to the character — a Friend, Enemy, or Lover.
///
/// `name` is the in-fiction handle; `kind` discriminates the table
/// the entry was rolled on; `note` carries the rolled descriptor
/// (e.g. "Childhood enemy — caused a major public humiliation. Just
/// themselves and a close friend." for an enemy, or "Lover died in an
/// accident." for a tragic love). The character creator fills `note`
/// from the relevant pp.51–52 sub-roll.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationshipBeacon {
    /// In-fiction name or handle. May be empty for a not-yet-named NPC.
    pub name: String,
    /// Kind of relationship — discriminates which roll table the
    /// `note` was drawn from. See pp.51–52.
    pub kind: BeaconKind,
    /// Free-form note — typically the rolled descriptor.
    pub note: String,
}

/// Discriminator for [`RelationshipBeacon`].
///
/// Three variants matching the three pp.51–52 tables: Friends (p.51),
/// Enemies (p.51), and Tragic Loves (p.52). `Lover` is the in-book
/// "Tragic Love Affair" entry — the variant name shortens it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BeaconKind {
    /// A friend. See p.51's top table.
    Friend,
    /// An enemy. See p.51's bottom table.
    Enemy,
    /// A tragic lover. See p.52's table.
    Lover,
}

// ---------------------------------------------------------------------------
// RoleLifepath
// ---------------------------------------------------------------------------

/// Role-specific lifepath roll-up (pp.54–69).
///
/// One variant per role; each carries a per-role struct holding the
/// rolled fields from that role's sub-tables. `Default` is
/// `RoleLifepath::Solo(SoloLifepath::default())` — Solo is the most
/// common starting role and the test fixtures use it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoleLifepath {
    /// Rockerboy lifepath. See p.54.
    Rockerboy(RockerboyLifepath),
    /// Solo lifepath. See p.55.
    Solo(SoloLifepath),
    /// Netrunner lifepath. See pp.56–57.
    Netrunner(NetrunnerLifepath),
    /// Tech lifepath. See pp.58–59.
    Tech(TechLifepath),
    /// Medtech lifepath. See pp.60–61.
    Medtech(MedtechLifepath),
    /// Media lifepath. See p.62.
    Media(MediaLifepath),
    /// Lawman lifepath. See p.65.
    Lawman(LawmanLifepath),
    /// Exec lifepath. See pp.63–64.
    Exec(ExecLifepath),
    /// Fixer lifepath. See pp.66–67.
    Fixer(FixerLifepath),
    /// Nomad lifepath. See pp.68–69.
    Nomad(NomadLifepath),
}

impl Default for RoleLifepath {
    fn default() -> Self {
        Self::Solo(SoloLifepath::default())
    }
}

/// Rockerboy lifepath rolled fields (p.54).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RockerboyLifepath {
    /// "What Kind of Rockerboy are You?" — 1d10. See p.54.
    pub rockerboy_type: String,
    /// "Are You in a Group or a Solo Act?" — choose. See p.54.
    pub group_or_solo: String,
    /// "Were You Once in a Group?" — choose; blank for "no". See p.54.
    pub were_in_group: String,
    /// "Why Did You Leave?" — 1d6 (only if were-in-group). See p.54.
    pub why_left: String,
    /// "Where Do You Perform?" — 1d6. See p.54.
    pub venue: String,
    /// "Who's Gunning for You/Your Group?" — 1d6. See p.54.
    pub whos_gunning: String,
}

/// Solo lifepath rolled fields (p.55).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoloLifepath {
    /// "What Kind of Solo are You?" — 1d6. See p.55.
    pub solo_type: String,
    /// "What's Your Moral Compass Like?" — 1d6. See p.55.
    pub moral_compass: String,
    /// "Who's Gunning for You?" — 1d6. See p.55.
    pub whos_gunning: String,
    /// "What's Your Operational Territory?" — 1d6. See p.55.
    pub operational_territory: String,
}

/// Netrunner lifepath rolled fields (pp.56–57).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetrunnerLifepath {
    /// "What Kind of Runner are You?" — 1d6. See p.56.
    pub runner_type: String,
    /// "Got a Partner, or Do You Work Alone?" — choose. See p.56.
    pub partner_or_alone: String,
    /// "If You Have a Partner, Who are They?" — 1d6. See p.56.
    pub partner: String,
    /// "What's Your Workspace Like?" — 1d6. See p.56.
    pub workspace: String,
    /// "Who are Some of Your Other Clients?" — 1d6. See p.57.
    pub other_clients: String,
    /// "Where Do You Get Your Programs?" — 1d6. See p.57.
    pub program_source: String,
    /// "Who's Gunning for You?" — 1d6. See p.57.
    pub whos_gunning: String,
}

/// Tech lifepath rolled fields (pp.58–59).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TechLifepath {
    /// "What Kind of Tech are You?" — 1d10. See p.58.
    pub tech_type: String,
    /// "What's Your Workspace Like?" — 1d6. See p.58.
    pub workspace: String,
    /// "Got a Partner, or Do You Work Alone?" — choose. See p.58.
    pub partner_or_alone: String,
    /// "If You Have a Partner, Who are They?" — 1d6. See p.58.
    pub partner: String,
    /// "Who are Your Main Clients?" — 1d6. See p.59.
    pub main_clients: String,
    /// "Where Do You Get Your Supplies?" — 1d6. See p.59.
    pub supply_source: String,
    /// "Who's Gunning For You?" — 1d6. See p.59.
    pub whos_gunning: String,
}

/// Medtech lifepath rolled fields (pp.60–61).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MedtechLifepath {
    /// "What Kind of Medtech are You?" — 1d10. See p.60.
    pub medtech_type: String,
    /// "Got a Partner, or Do You Work Alone?" — choose. See p.60.
    pub partner_or_alone: String,
    /// "Tell Us About Your Partner(s)." — 1d6. See p.60.
    pub partner: String,
    /// "What's Your Workspace Like?" — 1d6. See p.60.
    pub workspace: String,
    /// "Who are Your Main Clients?" — 1d6. See p.61.
    pub main_clients: String,
    /// "Where Do You Get Your Supplies?" — 1d6. See p.61.
    pub supply_source: String,
}

/// Media lifepath rolled fields (p.62).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaLifepath {
    /// "What Kind of Media are You?" — 1d6. See p.62.
    pub media_type: String,
    /// "How Does Your Work Reach the Public?" — 1d6. See p.62.
    pub how_published: String,
    /// "How Ethical are You?" — 1d6. See p.62.
    pub ethics: String,
    /// "What Types of Stories Do You Want to Tell?" — 1d6. See p.62.
    pub story_types: String,
}

/// Lawman lifepath rolled fields (p.65).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LawmanLifepath {
    /// "What is Your Position on the Force" — 1d6. See p.65.
    pub position: String,
    /// "How Wide is Your Group's Jurisdiction?" — 1d6. See p.65.
    pub jurisdiction: String,
    /// "How Corrupt is Your Group?" — 1d6. See p.65.
    pub corruption: String,
    /// "Who's Gunning for Your Group?" — 1d6. See p.65.
    pub whos_gunning: String,
    /// "Who is Your Group's Major Target?" — 1d6. See p.65.
    pub major_target: String,
}

/// Exec lifepath rolled fields (pp.63–64).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecLifepath {
    /// "What Kind of Corp Do You Work For?" — 1d10. See p.63.
    pub corp_type: String,
    /// "What Division Do You Work In?" — 1d6. See p.63.
    pub division: String,
    /// "How Good/Bad is Your Corp?" — 1d6. See p.63.
    pub corp_ethics: String,
    /// "Where is Your Corp Based?" — 1d6. See p.64.
    pub corp_location: String,
    /// "Who's Gunning for Your Group?" — 1d6. See p.64.
    pub whos_gunning: String,
    /// "Current State with Your Boss" — 1d6. See p.64.
    pub boss_relationship: String,
}

/// Fixer lifepath rolled fields (pp.66–67).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixerLifepath {
    /// "What Kind of Fixer are You?" — 1d10. See p.66.
    pub fixer_type: String,
    /// "Got a Partner or Work Alone?" — choose. See p.66.
    pub partner_or_alone: String,
    /// "Got a Partner? Who?" — 1d6. See p.66.
    pub partner: String,
    /// "What's Your 'Office' Like?" — 1d6. See p.66.
    pub office: String,
    /// "Who are Your Side Clients?" — 1d6. See p.67.
    pub side_clients: String,
    /// "Who's Gunning for You?" — 1d6. See p.67.
    pub whos_gunning: String,
}

/// Nomad lifepath rolled fields (pp.68–69).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NomadLifepath {
    /// "How Big is Your Pack?" — 1d6. See p.68.
    pub pack_size: String,
    /// "Is Your Pack Based on Land, Air, or Sea?" — choose. See p.68.
    pub land_air_sea: String,
    /// "If on Land/Air/Sea, What Do They Do?" — 1d10/1d6/1d6 (only the
    /// matching column is filled). See p.68.
    pub pack_business: String,
    /// "What Do You Do for Your Pack?" — 1d6. See p.69.
    pub personal_role: String,
    /// "What's Your Pack's Overall Philosophy?" — 1d6. See p.69.
    pub philosophy: String,
    /// "Who's Gunning for Your Pack?" — 1d6. See p.69.
    pub whos_gunning: String,
}

// ---------------------------------------------------------------------------
// On-disk RON shape + generic loader
// ---------------------------------------------------------------------------

/// RON envelope shared by every authored lifepath table file.
///
/// Each `content/tables/lifepath/*.ron` file is a `LifepathTableFile`
/// with a `die` size and a flat `entries` list of `(roll, value)`
/// pairs. The loader validates that rolls cover `1..=die` exactly
/// once (no gaps, no duplicates).
#[derive(Debug, Deserialize)]
struct LifepathTableFile<T> {
    die: u8,
    entries: Vec<(u8, T)>,
}

/// Generic lifepath-table loader.
///
/// Reads `path`, parses it as a `LifepathTableFile<T>`, and validates
/// that every roll in `1..=die` is present exactly once. On any
/// failure (I/O, parse, missing/duplicate roll) returns
/// [`RulesError::CatalogLoadFailed`]. Loader-level invariants:
/// - `die` must be `> 0`.
/// - Every entry's `roll` must be in `1..=die`.
/// - No two entries share the same `roll`.
/// - `entries.len() == die as usize`.
fn load_lifepath_table<T>(path: &Path) -> Result<LifepathTable<T>, RulesError>
where
    T: serde::de::DeserializeOwned,
{
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: LifepathTableFile<T> =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    if parsed.die == 0 {
        return Err(RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: "die must be > 0".into(),
        });
    }
    if parsed.entries.len() != parsed.die as usize {
        return Err(RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!(
                "expected {} entries for d{}, got {}",
                parsed.die,
                parsed.die,
                parsed.entries.len()
            ),
        });
    }
    let mut seen: HashSet<u8> = HashSet::with_capacity(parsed.entries.len());
    for (roll, _) in &parsed.entries {
        if *roll < 1 || *roll > parsed.die {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("roll {} outside 1..={}", roll, parsed.die),
            });
        }
        if !seen.insert(*roll) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate roll: {roll}"),
            });
        }
    }

    Ok(LifepathTable {
        die: parsed.die,
        entries: parsed.entries,
    })
}

// ---------------------------------------------------------------------------
// Cultural-origin row type — has both region and language list
// ---------------------------------------------------------------------------

/// One row of the Cultural Origins table (p.45).
///
/// `region` is the printed region label (e.g. "North American"),
/// `languages` is the comma-separated list from the right-hand
/// column (e.g. ["Chinese", "Cree", "Creole", "English", "French",
/// "Navajo", "Spanish"]). Character creation picks one language from
/// `languages` and gains 4 ranks of `Language(<that>)`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CulturalOriginRow {
    /// Cultural region label as printed on p.45.
    pub region: String,
    /// Languages listed beside the region — one of these is chosen.
    pub languages: Vec<String>,
}

// ---------------------------------------------------------------------------
// Family-background row type — has both label and description
// ---------------------------------------------------------------------------

/// One row of the Original Family Background table (p.49).
///
/// `label` is the printed name (e.g. "Combat Zoners"); `description`
/// is the prose paragraph beside it. Character creation copies both
/// into the [`FamilyBackground`] field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FamilyBackgroundRow {
    /// Background label (e.g. "Corporate Execs"). See p.49.
    pub label: String,
    /// Long-form description. See p.49.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Public per-table loaders
// ---------------------------------------------------------------------------

/// Load the Cultural Origins table from RON. See p.45.
pub fn load_cultural_origin_table(
    path: &Path,
) -> Result<LifepathTable<CulturalOriginRow>, RulesError> {
    load_lifepath_table::<CulturalOriginRow>(path)
}

/// Load the Personality table from RON. See p.46.
pub fn load_personality_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Clothing Style table from RON. See p.47.
pub fn load_clothing_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Hairstyle table from RON. See p.47.
pub fn load_hairstyle_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Affectations table from RON. See p.47.
pub fn load_affectations_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Motivations ("What Do You Value Most?") table from RON.
/// See p.48.
pub fn load_motivations_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Life Goals table from RON. See p.53.
pub fn load_life_goals_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Original Family Background table from RON. See p.49.
pub fn load_family_background_table(
    path: &Path,
) -> Result<LifepathTable<FamilyBackgroundRow>, RulesError> {
    load_lifepath_table::<FamilyBackgroundRow>(path)
}

/// Load the Family Crisis table from RON. See p.50.
pub fn load_family_crisis_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Friends ("Friend's Relationship to You") table from RON.
/// See p.51.
pub fn load_friends_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Enemies table from RON. See p.51.
///
/// The enemies table on p.51 has three columns — Enemy / Cause /
/// What Can They Throw — that share the same roll. The RON file
/// stores each row as a single semicolon-joined string covering all
/// three columns; the character creator splits as needed.
pub fn load_enemies_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

/// Load the Tragic Loves ("What Happened?") table from RON. See p.52.
pub fn load_tragic_loves_table(path: &Path) -> Result<LifepathTable<String>, RulesError> {
    load_lifepath_table::<String>(path)
}

// --- Role-specific table loaders -------------------------------------------
//
// Each role's lifepath section on pp.54–69 has multiple tables (type,
// workspace, who's gunning, etc.). Rather than create N loaders per
// role, we expose a single per-role loader that reads a wrapper file
// keyed by sub-table name. The role-file shape is the same generic
// struct: a map of `String -> LifepathTable<String>`.

/// On-disk shape for a role-specific lifepath file.
///
/// Each role's RON file (`rockerboy.ron`, `solo.ron`, …) is a
/// `RoleLifepathFile { tables: { "name" : LifepathTableFile<String>, ... } }`.
/// The loader validates each contained table individually.
#[derive(Debug, Deserialize)]
struct RoleLifepathFile {
    tables: Vec<(String, LifepathTableFile<String>)>,
}

/// Loaded role-specific lifepath table set.
///
/// `tables` is keyed by sub-table name (e.g. `"rockerboy_type"`,
/// `"venue"`, `"whos_gunning"`). Lookups are by `&str`. The loader
/// guarantees each contained `LifepathTable<String>` satisfies the
/// same invariants as a top-level lifepath table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleLifepathTables {
    /// Sub-tables keyed by name.
    pub tables: Vec<(String, LifepathTable<String>)>,
}

impl RoleLifepathTables {
    /// Look up a sub-table by name.
    pub fn get(&self, name: &str) -> Option<&LifepathTable<String>> {
        self.tables
            .iter()
            .find_map(|(k, v)| (k == name).then_some(v))
    }

    /// Number of sub-tables.
    pub fn len(&self) -> usize {
        self.tables.len()
    }

    /// `true` iff the file declared no sub-tables.
    pub fn is_empty(&self) -> bool {
        self.tables.is_empty()
    }
}

/// Load a role-specific lifepath file from RON.
///
/// `path` points at one of `rockerboy.ron`, `solo.ron`,
/// `netrunner.ron`, `tech.ron`, `medtech.ron`, `media.ron`,
/// `lawman.ron`, `exec.ron`, `fixer.ron`, or `nomad.ron`. The loader
/// validates every contained sub-table the same way the generic
/// loader does (`die > 0`, full coverage, no duplicates).
pub fn load_role_lifepath(path: &Path) -> Result<RoleLifepathTables, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: RoleLifepathFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut tables: Vec<(String, LifepathTable<String>)> = Vec::with_capacity(parsed.tables.len());
    let mut seen_names: HashSet<String> = HashSet::with_capacity(parsed.tables.len());
    for (name, file_table) in parsed.tables {
        if !seen_names.insert(name.clone()) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate sub-table name: '{name}'"),
            });
        }
        if file_table.die == 0 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("sub-table '{name}': die must be > 0"),
            });
        }
        if file_table.entries.len() != file_table.die as usize {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "sub-table '{name}': expected {} entries for d{}, got {}",
                    file_table.die,
                    file_table.die,
                    file_table.entries.len()
                ),
            });
        }
        let mut seen_rolls: HashSet<u8> = HashSet::with_capacity(file_table.entries.len());
        for (roll, _) in &file_table.entries {
            if *roll < 1 || *roll > file_table.die {
                return Err(RulesError::CatalogLoadFailed {
                    path: path.to_path_buf(),
                    source: format!(
                        "sub-table '{name}': roll {roll} outside 1..={}",
                        file_table.die
                    ),
                });
            }
            if !seen_rolls.insert(*roll) {
                return Err(RulesError::CatalogLoadFailed {
                    path: path.to_path_buf(),
                    source: format!("sub-table '{name}': duplicate roll {roll}"),
                });
            }
        }
        tables.push((
            name,
            LifepathTable {
                die: file_table.die,
                entries: file_table.entries,
            },
        ));
    }

    Ok(RoleLifepathTables { tables })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Workspace-root-relative path to the lifepath content directory.
    fn lifepath_dir() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("tables");
        p.push("lifepath");
        p
    }

    fn shared_table(name: &str) -> PathBuf {
        let mut p = lifepath_dir();
        p.push(name);
        p
    }

    /// Acceptance: cultural-origin RON loads, has the expected ten
    /// d10 entries (one per pp.45 row), and the rolled language
    /// lists are non-empty.
    #[test]
    fn test_cultural_origin_table_loads() {
        let cat = load_cultural_origin_table(&shared_table("cultural_origin.ron"))
            .expect("cultural_origin must load");
        assert_eq!(cat.die, 10);
        assert_eq!(cat.len(), 10);

        // Spot-check: roll 1 is "North American" and roll 10 is
        // "Oceania/Pacific Islander", per p.45.
        let row1 = cat.lookup(1).expect("roll 1 must exist");
        assert_eq!(row1.region, "North American");
        assert!(!row1.languages.is_empty());
        let row10 = cat.lookup(10).expect("roll 10 must exist");
        assert_eq!(row10.region, "Oceania/Pacific Islander");
        assert!(!row10.languages.is_empty());
    }

    /// Acceptance: every shared lifepath table loads cleanly.
    #[test]
    fn test_shared_tables_load() {
        // d10 tables.
        for name in [
            "personality.ron",
            "clothing.ron",
            "hairstyle.ron",
            "affectations.ron",
            "motivations.ron",
            "life_goals.ron",
            "family_crisis.ron",
            "friends.ron",
            "enemies.ron",
            "tragic_loves.ron",
        ] {
            let t = load_lifepath_table::<String>(&shared_table(name))
                .unwrap_or_else(|e| panic!("{name} must load: {e}"));
            assert_eq!(t.die, 10, "{name} die must be 10");
            assert_eq!(t.len(), 10, "{name} must have 10 entries");
        }
        // family_background uses a struct row.
        let fb = load_family_background_table(&shared_table("family_background.ron"))
            .expect("family_background must load");
        assert_eq!(fb.die, 10);
        assert_eq!(fb.len(), 10);
    }

    /// Acceptance: every role-specific lifepath file loads.
    #[test]
    fn test_role_specific_loads() {
        for role in [
            "rockerboy",
            "solo",
            "netrunner",
            "tech",
            "medtech",
            "media",
            "lawman",
            "exec",
            "fixer",
            "nomad",
        ] {
            let mut p = lifepath_dir();
            p.push(format!("{role}.ron"));
            let r = load_role_lifepath(&p).unwrap_or_else(|e| panic!("{role} must load: {e}"));
            assert!(!r.is_empty(), "{role} must declare at least one sub-table");
        }
    }

    /// Acceptance: `Lifepath::default()` produces a value, and
    /// the role-specific default is `Solo` with empty fields.
    #[test]
    fn test_lifepath_default_compiles() {
        let lp = Lifepath::default();
        assert!(lp.cultural_region.is_empty());
        assert!(lp.language_spoken.is_empty());
        assert!(lp.friends.is_empty());
        assert!(matches!(lp.role_specific, RoleLifepath::Solo(_)));
    }

    /// Acceptance: `Lifepath` round-trips cleanly through RON.
    #[test]
    fn test_lifepath_round_trip_ron() {
        let lp = Lifepath {
            cultural_region: "Sub-Saharan African".into(),
            language_spoken: "Amharic".into(),
            personality: "Stable and serious".into(),
            clothing_style: "Businesswear (Leadership, Presence, Authority)".into(),
            hairstyle: "Short and curly".into(),
            affectations: "Nose rings".into(),
            motivation: "Knowledge".into(),
            life_goal: "Get off The Street no matter what it takes.".into(),
            family_background: FamilyBackground {
                original_background: "Corporate Technicians".into(),
                description: "Middle-middle class…".into(),
                childhood_environment: "In a Nomad pack with roots in transport".into(),
                feelings_about_people: "I stay neutral.".into(),
                most_valued_person: "A friend".into(),
                most_valued_possession: "A toy".into(),
            },
            family_crisis: "Your family lost everything through betrayal.".into(),
            friends: vec![RelationshipBeacon {
                name: "Maryam".into(),
                kind: BeaconKind::Friend,
                note: "An old childhood friend.".into(),
            }],
            enemies: vec![RelationshipBeacon {
                name: "rogue AI".into(),
                kind: BeaconKind::Enemy,
                note: "Boostergonger; one of you set the other up.".into(),
            }],
            tragic_loves: vec![RelationshipBeacon {
                name: "Kira".into(),
                kind: BeaconKind::Lover,
                note: "Your lover died in an accident.".into(),
            }],
            role_specific: RoleLifepath::Netrunner(NetrunnerLifepath {
                runner_type: "Freelancer who will hack for hire.".into(),
                partner_or_alone: "Work Alone".into(),
                partner: String::new(),
                workspace: "Minimalist, clean, and organized.".into(),
                other_clients: "Local Fixers who send you clients.".into(),
                program_source: "You hit the Night Markets.".into(),
                whos_gunning: "Rogue AI or NET Ghost.".into(),
            }),
        };

        let serialised = ron::ser::to_string_pretty(&lp, ron::ser::PrettyConfig::default())
            .expect("Lifepath must serialise");
        let restored: Lifepath = ron::de::from_str(&serialised).expect("Lifepath must round-trip");
        assert_eq!(restored, lp);
    }

    /// Regression: loader rejects a table whose entry count
    /// disagrees with `die`.
    #[test]
    fn test_loader_rejects_wrong_entry_count() {
        // Build a tempdir-based file that declares die = 10 but only
        // has two entries. Use the system temp dir via std::env.
        let dir = std::env::temp_dir().join("cpr_lifepath_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("bad.ron");
        std::fs::write(&path, r#"(die: 10, entries: [(1, "a"), (2, "b")])"#)
            .expect("write must succeed");
        let r = load_lifepath_table::<String>(&path);
        assert!(r.is_err(), "loader must reject mismatched entry count");
    }

    /// Regression: loader rejects a duplicate roll.
    #[test]
    fn test_loader_rejects_duplicate_roll() {
        let dir = std::env::temp_dir().join("cpr_lifepath_test");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("dup.ron");
        // Two entries on roll 1 and exactly two entries (so the
        // count check passes) — die=2.
        std::fs::write(&path, r#"(die: 2, entries: [(1, "a"), (1, "b")])"#)
            .expect("write must succeed");
        let r = load_lifepath_table::<String>(&path);
        assert!(r.is_err(), "loader must reject duplicate rolls");
    }
}
