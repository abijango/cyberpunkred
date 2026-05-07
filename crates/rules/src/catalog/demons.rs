//! Demons catalog (WP-210).
//!
//! Demons are the large-scale Black ICE Intelligent Systems that defend a NET
//! Architecture in *Cyberpunk RED*. The book lists three named Demons — Imp,
//! Efreet, Balron — each with a fixed REZ / Interface / NET Actions / Combat
//! Number stat block on p.212. Demons cannot run Programs or Black ICE
//! themselves; they exist to operate Control Nodes (drones, turrets, …) and
//! defend their architecture against Netrunner intruders. See p.212 for the
//! full Demons table.
//!
//! The on-disk catalog file is `content/catalogs/demons.ron`; the loader
//! enforces slug uniqueness in the same style as
//! [`crate::catalog::skills::load_skills_catalog`]. See p.212.

use crate::catalog::Catalog;
use crate::error::RulesError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// DemonId
// ---------------------------------------------------------------------------

/// Stable string identifier for a [`Demon`].
///
/// The wrapped `String` is the same slug used as the lookup key in
/// [`Catalog<Demon>`]. Wrapping it in a newtype keeps `DemonId` distinct
/// from arbitrary strings — callers pass `DemonId` values around without
/// risk of collision with weapon / armor / skill ids.
///
/// See p.212.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct DemonId(pub String);

// ---------------------------------------------------------------------------
// Demon
// ---------------------------------------------------------------------------

/// A row in the canonical Demons catalog (`content/catalogs/demons.ron`).
///
/// One entry per Demon listed on p.212 — Imp, Efreet, Balron. Stats are
/// carried verbatim from the book's Demons table:
/// - `rez` — REZ score (the Demon's hit points / structural integrity).
/// - `interface` — the Demon's Interface ability rank, used for both Zap
///   (defending) and Control (operating Control Nodes). Note Demons only
///   have access to those two NET Actions per p.212.
/// - `net_actions_per_turn` — the number of NET Actions the Demon may take
///   on its Turn (p.212 column "NET Actions").
/// - `combat_number` — the Demon's Combat Number, used in cyberspace
///   combat as `combat_number + 1d10` for both attack and defence (p.212).
/// - `icon` — the flavour description of the Demon's appearance in the
///   NET, copied verbatim from the icon callouts on p.212.
///
/// See p.212 (Demons table and icon callouts).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Demon {
    /// Stable identifier — matches the catalog slug.
    pub id: DemonId,
    /// Display name as printed in the rulebook (p.212).
    pub display_name: String,
    /// REZ score per p.212 (`u16` because future expansions may exceed 255).
    pub rez: u16,
    /// Interface ability rank per p.212 (single-digit in RAW, `u8` is ample).
    pub interface: u8,
    /// NET Actions per Turn per p.212 (single-digit in RAW).
    pub net_actions_per_turn: u8,
    /// Combat Number used in cyberspace combat (p.212): the Demon adds
    /// `combat_number + 1d10` for attacks and defences.
    pub combat_number: u8,
    /// Flavour description of the Demon's NET icon, verbatim from p.212.
    pub icon: String,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/demons.ron`.
///
/// The file is a `(demons: [ ... ])` envelope where each entry is a
/// [`DemonsFileEntry`] carrying both a `slug` (the catalog key) and the
/// structured [`Demon`] fields. This matches the shape used by every Phase
/// 2 catalog (see [`crate::catalog::skills`]).
#[derive(Debug, Deserialize)]
struct DemonsFile {
    demons: Vec<DemonsFileEntry>,
}

/// One row in the on-disk demons catalog file.
///
/// `slug` is the lookup key inside the resulting `Catalog<Demon>`. All
/// other fields populate the [`Demon`] directly. The loader rebuilds the
/// [`DemonId`] from the slug so the file does not have to repeat it.
#[derive(Debug, Deserialize)]
struct DemonsFileEntry {
    slug: String,
    display_name: String,
    rez: u16,
    interface: u8,
    net_actions_per_turn: u8,
    combat_number: u8,
    icon: String,
}

/// Load the demons catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Demon>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and a
/// stringified description of the underlying I/O or parse error.
///
/// The loader enforces one invariant: slugs are unique within the file.
/// A duplicate slug fails the load.
///
/// See p.212 (Demons table) for the source data, and
/// `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader loading
/// conventions every Phase 2 catalog follows.
pub fn load_demons_catalog(path: &Path) -> Result<Catalog<Demon>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: DemonsFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Demon> = HashMap::with_capacity(parsed.demons.len());
    for row in parsed.demons {
        let demon = Demon {
            id: DemonId(row.slug.clone()),
            display_name: row.display_name,
            rez: row.rez,
            interface: row.interface,
            net_actions_per_turn: row.net_actions_per_turn,
            combat_number: row.combat_number,
            icon: row.icon,
        };
        if entries.insert(row.slug.clone(), demon).is_some() {
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

    /// Workspace-relative path to the canonical demons catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog lives
    /// two parents up at `content/catalogs/demons.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("demons.ron");
        p
    }

    /// Acceptance: Imp / Efreet / Balron all present with stats matching
    /// the table on p.212.
    ///
    /// p.212 Demons table:
    /// - Imp: REZ 15, Interface 3, NET Actions 2, Combat Number 14.
    /// - Efreet: REZ 25, Interface 4, NET Actions 3, Combat Number 14.
    /// - Balron: REZ 30, Interface 7, NET Actions 4, Combat Number 14.
    #[test]
    fn test_demon_catalog() {
        let cat = load_demons_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            3,
            "expected 3 demons per p.212 (got {})",
            cat.len()
        );

        let imp = cat.get("imp").expect("imp must be present");
        assert_eq!(imp.id, DemonId("imp".to_string()));
        assert_eq!(imp.display_name, "Imp");
        assert_eq!(imp.rez, 15);
        assert_eq!(imp.interface, 3);
        assert_eq!(imp.net_actions_per_turn, 2);
        assert_eq!(imp.combat_number, 14);
        assert!(!imp.icon.is_empty(), "imp icon flavour must be populated");

        let efreet = cat.get("efreet").expect("efreet must be present");
        assert_eq!(efreet.id, DemonId("efreet".to_string()));
        assert_eq!(efreet.display_name, "Efreet");
        assert_eq!(efreet.rez, 25);
        assert_eq!(efreet.interface, 4);
        assert_eq!(efreet.net_actions_per_turn, 3);
        assert_eq!(efreet.combat_number, 14);
        assert!(
            !efreet.icon.is_empty(),
            "efreet icon flavour must be populated"
        );

        let balron = cat.get("balron").expect("balron must be present");
        assert_eq!(balron.id, DemonId("balron".to_string()));
        assert_eq!(balron.display_name, "Balron");
        assert_eq!(balron.rez, 30);
        assert_eq!(balron.interface, 7);
        assert_eq!(balron.net_actions_per_turn, 4);
        assert_eq!(balron.combat_number, 14);
        assert!(
            !balron.icon.is_empty(),
            "balron icon flavour must be populated"
        );
    }

    /// Acceptance: a [`Demon`] round-trips through RON serialisation —
    /// the on-disk format is stable enough that we can serialise an
    /// in-memory `Demon` and parse it back into an equivalent value.
    #[test]
    fn test_demon_round_trip_ron() {
        let original = Demon {
            id: DemonId("imp".to_string()),
            display_name: "Imp".to_string(),
            rez: 15,
            interface: 3,
            net_actions_per_turn: 2,
            combat_number: 14,
            icon: "Small orange sphere of light with red horns.".to_string(),
        };
        let serialised = ron::ser::to_string(&original).expect("must serialise");
        let restored: Demon = ron::de::from_str(&serialised).expect("must round-trip");
        assert_eq!(restored, original);
    }

    /// Regression: the loader rejects a duplicate slug rather than
    /// silently keeping one of the two entries.
    #[test]
    fn test_loader_rejects_duplicate_slug() {
        let tmp = std::env::temp_dir().join("cpr_demons_dup_test.ron");
        let body = r#"DemonsFile(
    demons: [
        (
            slug: "imp",
            display_name: "Imp",
            rez: 15,
            interface: 3,
            net_actions_per_turn: 2,
            combat_number: 14,
            icon: "first.",
        ),
        (
            slug: "imp",
            display_name: "Imp",
            rez: 15,
            interface: 3,
            net_actions_per_turn: 2,
            combat_number: 14,
            icon: "second.",
        ),
    ],
)
"#;
        std::fs::write(&tmp, body).expect("write tmp file");
        let err = load_demons_catalog(&tmp).expect_err("must fail on duplicate slug");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(source.contains("duplicate slug"), "got: {source}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: the loader returns a structured error (not a panic)
    /// when the file does not exist.
    #[test]
    fn test_loader_missing_file() {
        let p = PathBuf::from("/definitely/not/a/real/demons.ron");
        let err = load_demons_catalog(&p).expect_err("must fail when path missing");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(source.contains("read failed"), "got: {source}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
