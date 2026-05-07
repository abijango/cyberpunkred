//! Role catalog (WP-213).
//!
//! Defines [`RoleAbilityKind`] — the closed enum of every Role Ability in the
//! rulebook — plus the metadata catalog [`RoleDefinition`] (display name, the
//! Role's ability, suggested skill emphasis at character creation) and the
//! RON loader [`load_roles_catalog`].
//!
//! Rulebook references:
//! - **pp.36–69:** the ten Role chapters (Exec p.36, Lawman p.37, Fixer p.38,
//!   Nomad p.39, Media p.35, Rockerboy p.32, Solo p.33, Netrunner p.33,
//!   Tech p.34, Medtech p.34) — each chapter's "Role Ability:" section is
//!   the canonical name reference for the Role's ability.
//! - **pp.142–161:** the Role Abilities chapter (*"Roles And Role Abilities
//!   in the Time of the Red"*) listing each Role Ability in detail:
//!   Charismatic Impact (Rockerboy, p.144), Combat Awareness (Solo, p.146),
//!   Interface (Netrunner, p.147), Maker (Tech, p.147), Medicine (Medtech,
//!   p.149), Credibility (Media, p.151), Teamwork (Exec, p.153), Backup
//!   (Lawman, p.158), Operator (Fixer, p.159), Moto (Nomad, p.161).
//!
//! ## Public-API deviation from WP-213 spec
//!
//! The WP-213 description (`IMPLEMENTATION_PLAN.md` §4) names two variants
//! that disagree with RAW: it lists Solo's ability as `CombatSense` and
//! Exec's ability as `Resources`. The rulebook explicitly calls them
//! **Combat Awareness** (p.146) and **Teamwork** (p.153). Per CLAUDE.md
//! ("default to RAW; comment the tension; flag in PR") and the priority
//! order in `IMPLEMENTATION_PLAN.md` §0.4 ("trust the rulebook over this
//! document"), this module uses the rulebook's canonical names —
//! [`RoleAbilityKind::CombatAwareness`] and [`RoleAbilityKind::Teamwork`].
//!
//! ## `RoleAbilityId` vs `RoleAbilityKind`
//!
//! [`crate::effects::RoleAbilityId`] (a `String` newtype) and
//! [`RoleAbilityKind`] (a closed enum) coexist intentionally:
//!
//! - `RoleAbilityKind` is the **catalog handle** — a closed set, exhaustively
//!   pattern-matched by code that branches on which Role Ability is at play.
//! - `RoleAbilityId` is the [`crate::effects::EffectSource::RoleAbility`]
//!   payload — an open *content slug* the effect engine carries to attribute
//!   buffs/debuffs to a specific role-ability sub-feature (e.g.
//!   `"combat_awareness.precision_attack"`, `"moto.family_motorpool"`).
//!   Sub-feature granularity is not yet a closed set, and the effect engine
//!   only needs a stable identifier for narration / UI.
//!
//! The catalog file is `content/catalogs/roles.ron`; the loader expects
//! exactly ten entries (one per Role variant).

use crate::catalog::skills::SkillId;
use crate::catalog::Catalog;
use crate::character::data::Role;
use crate::error::RulesError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// RoleAbilityKind
// ---------------------------------------------------------------------------

/// Closed enum of every canonical Role Ability in *Cyberpunk RED*.
///
/// One variant per Role per pp.142–161. Variant names follow the rulebook's
/// display names normalised to `UpperCamelCase`. This is a closed set —
/// every Role in the book has exactly one Role Ability, and the book's
/// "Multiclassing" rules (p.143) compose two `RoleAbilityKind`s on the same
/// character without inventing a new ability.
///
/// Rule-engine code that branches on Role Ability (combat resolution,
/// netrunning, character creation) pattern-matches this enum exhaustively;
/// adding a new variant is a deliberate compile-time event.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RoleAbilityKind {
    /// Rockerboy Role Ability — see p.144. Influence others through sheer
    /// force of personality (+1d10 vs DV8/DV10/DV12 against fans).
    CharismaticImpact,
    /// Solo Role Ability — see p.146. Distribute Role-Rank points across
    /// Damage Deflection, Fumble Recovery, Initiative Reaction, Precision
    /// Attack, Spot Weakness, and Threat Detection.
    ///
    /// **Naming note:** WP-213's spec named this variant `CombatSense`; the
    /// rulebook calls it **Combat Awareness** on p.146. Implementation
    /// follows the rulebook (CLAUDE.md: "default to RAW").
    CombatAwareness,
    /// Netrunner Role Ability — see p.147. Determines NET Actions per turn
    /// and unlocks Interface Abilities (Backdoor, Cloak, Control, Eye-Dee,
    /// Pathfinder, Scanner, Slide, Virus, Zap).
    Interface,
    /// Tech Role Ability — see p.147. Each rank grants two Maker Specialty
    /// ranks (Field Expertise, Upgrade Expertise, Fabrication Expertise,
    /// Invention Expertise).
    Maker,
    /// Medtech Role Ability — see p.149. Each rank grants one Medicine
    /// Specialty rank (Surgery, Medical Tech [Pharmaceuticals], Medical
    /// Tech [Cryosystem Operation]).
    Medicine,
    /// Media Role Ability — see p.151. Drives Rumor passive/active DVs and
    /// Story Believability/Audience/Impact tiers.
    Credibility,
    /// Lawman Role Ability — see p.158. Call upon a tier of fellow law
    /// enforcement (Corp Security → Beat Cops → Sheriffs → Recovery Zone
    /// Marshal → C-SWAT → National/Interpol).
    Backup,
    /// Exec Role Ability — see p.153. Provides Signing Bonus, Corporate
    /// Housing, Corporate Health Insurance, and up to three Team Members
    /// (Bodyguard / Covert Operative / Driver / Netrunner / Technician).
    ///
    /// **Naming note:** WP-213's spec named this variant `Resources`; the
    /// rulebook calls it **Teamwork** on p.153. Implementation follows the
    /// rulebook (CLAUDE.md: "default to RAW").
    Teamwork,
    /// Fixer Role Ability — see p.159. Provides Contacts, Reach, Haggle,
    /// and Grease tiers; unlocks Night/Midnight Markets at higher ranks.
    Operator,
    /// Nomad Role Ability — see p.161. Adds rank to vehicle-related Skill
    /// Checks and grows the Family Motorpool.
    Moto,
}

// ---------------------------------------------------------------------------
// RoleDefinition
// ---------------------------------------------------------------------------

/// A row in the canonical Role catalog (`content/catalogs/roles.ron`).
///
/// Loaded by [`load_roles_catalog`]. The `role` field is the closed-enum
/// handle; the slug used as the [`Catalog<T>`] key is the lowercase Role
/// name (e.g. `solo`, `netrunner`, `medtech`).
///
/// `flavor_skill_emphasis` is the suggested-at-creation skill set drawn
/// from the role's chapter (pp.32–39) and the Streetrat / Edgerunner
/// templates (pp.86–88). It's narrative guidance, not a hard constraint —
/// character-creation code (Phase 5) reads this as the default "you might
/// want to bump these" starting point and the player is free to ignore it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleDefinition {
    /// Closed-enum identifier — the canonical handle for this Role.
    pub role: Role,
    /// Display name as printed in the rulebook (preserving capitalisation).
    pub display_name: String,
    /// The Role Ability — see [`RoleAbilityKind`].
    pub role_ability: RoleAbilityKind,
    /// Suggested-at-creation skill emphasis. Drawn from the role's chapter
    /// (pp.32–39) and the Streetrat templates (p.86+). The list is not
    /// exhaustive and not enforced — it's a hint to character-creation UI.
    pub flavor_skill_emphasis: Vec<SkillId>,
}

// ---------------------------------------------------------------------------
// Role → RoleAbilityKind lookup
// ---------------------------------------------------------------------------

/// Return the canonical [`RoleAbilityKind`] for a [`Role`] per pp.142–161.
///
/// This is the in-code source of truth for the Role → Role Ability mapping;
/// the catalog RON file's `role_ability` field must agree (the loader
/// enforces this contract — a mismatch fails the load with
/// [`RulesError::CatalogLoadFailed`]).
pub fn role_ability_for(role: Role) -> RoleAbilityKind {
    match role {
        Role::Rockerboy => RoleAbilityKind::CharismaticImpact, // p.144
        Role::Solo => RoleAbilityKind::CombatAwareness,        // p.146
        Role::Netrunner => RoleAbilityKind::Interface,         // p.147
        Role::Tech => RoleAbilityKind::Maker,                  // p.147
        Role::Medtech => RoleAbilityKind::Medicine,            // p.149
        Role::Media => RoleAbilityKind::Credibility,           // p.151
        Role::Lawman => RoleAbilityKind::Backup,               // p.158
        Role::Exec => RoleAbilityKind::Teamwork,               // p.153
        Role::Fixer => RoleAbilityKind::Operator,              // p.159
        Role::Nomad => RoleAbilityKind::Moto,                  // p.161
    }
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/roles.ron`.
///
/// The file is a `(roles: [ ... ])` envelope where each entry is a
/// [`RoleDefinition`] plus an explicit `slug` field (the [`Catalog<T>`]
/// key). Mirrors the layout of `skills.ron` — a flat list, the loader
/// computes the lookup map.
#[derive(Debug, Deserialize)]
struct RolesFile {
    roles: Vec<RolesFileEntry>,
}

/// One row in the on-disk roles catalog file.
#[derive(Debug, Deserialize)]
struct RolesFileEntry {
    slug: String,
    role: Role,
    display_name: String,
    role_ability: RoleAbilityKind,
    flavor_skill_emphasis: Vec<SkillId>,
}

/// Load the roles catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<RoleDefinition>`] keyed by slug. On
/// failure returns [`RulesError::CatalogLoadFailed`] carrying the file
/// path and a stringified description of the underlying I/O / parse /
/// invariant error.
///
/// The loader enforces three invariants:
/// 1. Every entry's `role_ability` agrees with [`role_ability_for`] for
///    that `role`. A mismatch fails the load — keeping the catalog file
///    from silently disagreeing with the in-code lookup.
/// 2. Slugs are unique within the file.
/// 3. Roles are unique within the file (no duplicate `Role::Solo`, etc.).
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader
/// loading conventions every Phase 2 catalog follows.
pub fn load_roles_catalog(path: &Path) -> Result<Catalog<RoleDefinition>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: RolesFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, RoleDefinition> = HashMap::with_capacity(parsed.roles.len());
    let mut seen_roles: Vec<Role> = Vec::with_capacity(parsed.roles.len());
    for row in parsed.roles {
        let expected_ability = role_ability_for(row.role);
        if expected_ability != row.role_ability {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "role '{}' has role_ability {:?} in file but {:?} in code (role_ability_for lookup)",
                    row.slug, row.role_ability, expected_ability
                ),
            });
        }
        if seen_roles.contains(&row.role) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate Role variant: {:?}", row.role),
            });
        }
        seen_roles.push(row.role);
        let def = RoleDefinition {
            role: row.role,
            display_name: row.display_name,
            role_ability: row.role_ability,
            flavor_skill_emphasis: row.flavor_skill_emphasis,
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
    use crate::catalog::skills::{LanguageKind, LocalArea};
    use std::path::PathBuf;

    /// Workspace-relative path to the canonical roles catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog lives
    /// two parents up at `content/catalogs/roles.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("roles.ron");
        p
    }

    /// Acceptance: every Role listed on pp.32–39 must appear in the
    /// catalog, and the catalog must contain exactly ten entries (one per
    /// `Role` variant — no duplicates, no extras).
    #[test]
    fn test_all_10_roles_loaded() {
        let cat = load_roles_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            10,
            "expected 10 roles per pp.32-39 (got {}); verify the catalog RON file",
            cat.len()
        );
    }

    /// Acceptance: the Role → Role Ability mapping matches the rulebook
    /// (pp.142–161) for every Role.
    ///
    /// Pinned values (page references in `role_ability_for`):
    /// - Rockerboy → Charismatic Impact (p.144)
    /// - Solo → Combat Awareness (p.146; WP spec said `CombatSense` — see
    ///   module docs for the deviation note)
    /// - Netrunner → Interface (p.147)
    /// - Tech → Maker (p.147)
    /// - Medtech → Medicine (p.149)
    /// - Media → Credibility (p.151)
    /// - Exec → Teamwork (p.153; WP spec said `Resources` — see module
    ///   docs for the deviation note)
    /// - Lawman → Backup (p.158)
    /// - Fixer → Operator (p.159)
    /// - Nomad → Moto (p.161)
    #[test]
    fn test_role_ability_mapping() {
        // Exercise the in-code lookup directly — independent of the RON
        // file. This pin makes a silent rename of the RAW abilities a
        // compile-time + test-time double failure.
        assert_eq!(
            role_ability_for(Role::Rockerboy),
            RoleAbilityKind::CharismaticImpact
        );
        assert_eq!(
            role_ability_for(Role::Solo),
            RoleAbilityKind::CombatAwareness
        );
        assert_eq!(
            role_ability_for(Role::Netrunner),
            RoleAbilityKind::Interface
        );
        assert_eq!(role_ability_for(Role::Tech), RoleAbilityKind::Maker);
        assert_eq!(role_ability_for(Role::Medtech), RoleAbilityKind::Medicine);
        assert_eq!(role_ability_for(Role::Media), RoleAbilityKind::Credibility);
        assert_eq!(role_ability_for(Role::Lawman), RoleAbilityKind::Backup);
        assert_eq!(role_ability_for(Role::Exec), RoleAbilityKind::Teamwork);
        assert_eq!(role_ability_for(Role::Fixer), RoleAbilityKind::Operator);
        assert_eq!(role_ability_for(Role::Nomad), RoleAbilityKind::Moto);

        // And cross-check via the loaded catalog: every entry's
        // `role_ability` agrees with the in-code lookup.
        let cat = load_roles_catalog(&catalog_path()).expect("catalog must load");
        for (slug, def) in cat.iter() {
            assert_eq!(
                def.role_ability,
                role_ability_for(def.role),
                "catalog '{slug}' role_ability disagrees with in-code lookup",
            );
        }
    }

    /// Acceptance: a [`RoleDefinition`] (and the [`RoleAbilityKind`] it
    /// carries) round-trips through RON serialisation. Pins the
    /// `Serialize`/`Deserialize` derives so save-file and content-loader
    /// callers can rely on them.
    #[test]
    fn test_role_round_trip_ron() {
        let original = RoleDefinition {
            role: Role::Solo,
            display_name: "Solo".to_string(),
            role_ability: RoleAbilityKind::CombatAwareness,
            flavor_skill_emphasis: vec![
                SkillId::Handgun,
                SkillId::ShoulderArms,
                SkillId::Brawling,
                SkillId::Evasion,
                SkillId::Stealth,
                SkillId::Tactics,
            ],
        };
        let serialised = ron::ser::to_string(&original).expect("must serialise");
        let restored: RoleDefinition = ron::de::from_str(&serialised).expect("must round-trip");
        assert_eq!(restored, original);

        // And spot-check `RoleAbilityKind` on its own (the closed enum
        // must round-trip with no payload).
        let team = RoleAbilityKind::Teamwork;
        let s = ron::ser::to_string(&team).expect("must serialise");
        let back: RoleAbilityKind = ron::de::from_str(&s).expect("must round-trip");
        assert_eq!(back, team);
    }

    /// Regression: every Role variant has a unique slug in the loaded
    /// catalog, and every catalog entry's `role` field is unique.
    #[test]
    fn test_catalog_role_uniqueness() {
        let cat = load_roles_catalog(&catalog_path()).expect("catalog must load");
        let mut seen: Vec<Role> = Vec::new();
        for (_slug, def) in cat.iter() {
            assert!(
                !seen.contains(&def.role),
                "duplicate Role variant in catalog: {:?}",
                def.role
            );
            seen.push(def.role);
        }
        assert_eq!(seen.len(), 10);
    }

    /// Regression: a catalog file whose `role_ability` disagrees with the
    /// in-code lookup must fail to load with [`RulesError::CatalogLoadFailed`].
    #[test]
    fn test_loader_rejects_role_ability_mismatch() {
        // Minimal RON: one entry where Solo claims `Interface` (not
        // `CombatAwareness`). Loader must reject.
        let ron = r#"RolesFile(
    roles: [
        (
            slug: "solo",
            role: Solo,
            display_name: "Solo",
            role_ability: Interface,
            flavor_skill_emphasis: [],
        ),
    ],
)"#;
        let tmp = std::env::temp_dir().join("wp213_bad_role_ability.ron");
        std::fs::write(&tmp, ron).expect("write tmp");
        let err = load_roles_catalog(&tmp).expect_err("must reject mismatch");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("role_ability"),
                    "error must mention role_ability mismatch; got: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed; got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: a catalog file with a duplicate slug must fail to load.
    #[test]
    fn test_loader_rejects_duplicate_slug() {
        // Two entries with the same slug "solo" but different Roles.
        let ron = r#"RolesFile(
    roles: [
        (
            slug: "solo",
            role: Solo,
            display_name: "Solo",
            role_ability: CombatAwareness,
            flavor_skill_emphasis: [],
        ),
        (
            slug: "solo",
            role: Netrunner,
            display_name: "Netrunner",
            role_ability: Interface,
            flavor_skill_emphasis: [],
        ),
    ],
)"#;
        let tmp = std::env::temp_dir().join("wp213_dup_slug.ron");
        std::fs::write(&tmp, ron).expect("write tmp");
        let err = load_roles_catalog(&tmp).expect_err("must reject duplicate slug");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("duplicate slug") || source.contains("duplicate"),
                    "error must mention duplicate; got: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed; got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: `flavor_skill_emphasis` accommodates parameterised skills
    /// (e.g. `Language(Streetslang)`). This pins the dependency on the
    /// catalog::skills public types so a future refactor of `SkillId`
    /// can't silently break role authoring.
    #[test]
    fn test_flavor_skill_emphasis_supports_parameterised_skills() {
        let nomad = RoleDefinition {
            role: Role::Nomad,
            display_name: "Nomad".to_string(),
            role_ability: RoleAbilityKind::Moto,
            flavor_skill_emphasis: vec![
                SkillId::DriveLandVehicle,
                SkillId::LandVehicleTech,
                SkillId::Language(LanguageKind::Streetslang),
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
            ],
        };
        let s = ron::ser::to_string(&nomad).expect("serialise");
        let back: RoleDefinition = ron::de::from_str(&s).expect("round-trip");
        assert_eq!(back, nomad);
    }
}
