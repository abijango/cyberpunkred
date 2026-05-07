//! Cover materials catalog (WP-207).
//!
//! Defines the [`CoverMaterial`] entry shape for the `Cover Material and
//! Thickness Examples` table (rulebook p.183), the supporting
//! [`MaterialKind`] / [`Thickness`] enums, and the RON loader
//! [`load_cover_catalog`].
//!
//! Rulebook references:
//! - **p.182:** *Cover Hit Points* — describes how cover HP is determined
//!   by material × thickness, the (Thin × material) / (Thick × material)
//!   table, and the rules around destruction at 0 HP. The 6 rows of the
//!   *Type of Cover* table on p.182 are the underlying matrix.
//! - **p.183:** *Cover Material and Thickness Examples* — the catalogued
//!   table this module loads: 24 named example objects, each tagged with
//!   its material/thickness and explicit HP. All entries here come from
//!   that table. See `// See p.183` markers below.
//!
//! The catalog file is `content/catalogs/cover.ron`; the loader expects
//! one entry per slug. Two RAW idiosyncrasies are preserved verbatim from
//! p.183 rather than normalised:
//! 1. **Metal Door** is listed on p.183 as *Thin Steel = 20 HP*, even
//!    though the *Type of Cover* table on p.182 gives Thin Steel as 25 HP.
//!    We honour the p.183 value (RAW); the catalog comment flags this.
//! 2. **Bulletproof Windshield** is listed on p.183 as *Thin or Thick
//!    Bulletproof Glass = 15 or 30 HP*. The catalog stores the canonical
//!    Thick form (30 HP); the `example` field documents the Thin variant.
//!
//! Both choices are noted in the RON file header for content reviewers.

use crate::catalog::Catalog;
use crate::error::RulesError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// MaterialKind
// ---------------------------------------------------------------------------

/// Cover material as enumerated in the *Type of Cover* table on p.182.
///
/// The six named rows of that table are `Steel`, `Stone`, `BulletproofGlass`,
/// `Concrete`, `Wood`, and `PlasterFoamPlastic`. We add `Glass` for the
/// distinction many GMs draw between bulletproof (rated, 15/30 HP) and
/// ordinary glass (functionally not-cover, captured by the
/// p.183 *Windshield = 0 HP (Not Cover)* row), and `Other` for example
/// rows that the rulebook tags with no material at all (e.g. the *Office
/// Cubicle* row on p.183, which is just *0 HP (Not Cover)*).
///
/// Variant naming follows the book's display strings normalised to
/// `UpperCamelCase`. The compound *Plaster/Foam/Plastic* row of p.182
/// becomes `PlasterFoamPlastic`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MaterialKind {
    /// Steel — the toughest entry in the p.182 matrix (Thick = 50 HP /
    /// Thin = 25 HP). Steel cover *cannot* be damaged by Martial Arts or
    /// Brawling attacks except from a Cyberarm or BODY 10+ character (p.182).
    Steel,
    /// Wood — Thick = 20 HP / Thin = 5 HP per p.182.
    Wood,
    /// Stone — Thick = 40 HP / Thin = 20 HP per p.182.
    Stone,
    /// Concrete — Thick = 25 HP / Thin = 10 HP per p.182.
    Concrete,
    /// Bulletproof Glass — Thick = 30 HP / Thin = 15 HP per p.182.
    /// Distinct from ordinary `Glass`, which p.182 does not list.
    BulletproofGlass,
    /// Plaster/Foam/Plastic — Thick = 15 HP / Thin = 0 HP (Not Cover) per
    /// p.182. The single-name `PlasterFoamPlastic` mirrors the book's
    /// compound row.
    PlasterFoamPlastic,
    /// Ordinary (non-bulletproof) glass. Not a row on the p.182 *Type of
    /// Cover* table; supplied for narrative completeness — every example
    /// in the catalog that uses this is a *0 HP (Not Cover)* row from p.183.
    Glass,
    /// Catch-all for example rows on p.183 that the rulebook does not
    /// tag with a specific material (e.g. *Office Cubicle*). HP for such
    /// rows is taken straight from the table.
    Other,
}

// ---------------------------------------------------------------------------
// Thickness
// ---------------------------------------------------------------------------

/// Thickness column of the *Type of Cover* table on p.182.
///
/// `Thin` cover can be moved by anyone in a pinch; `Thick` cover is too
/// unwieldy for characters without BODY 10+ to relocate without special
/// equipment (p.182 — *Cover Hit Points* sidebar).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Thickness {
    /// Thin cover — left column of the p.182 table (lower HP).
    Thin,
    /// Thick cover — right column of the p.182 table (higher HP).
    Thick,
}

// ---------------------------------------------------------------------------
// CoverMaterial
// ---------------------------------------------------------------------------

/// One row of the *Cover Material and Thickness Examples* table on p.183.
///
/// Loaded by [`load_cover_catalog`]. The `id` is the slug used as the
/// `Catalog<T>` key; `display_name` and `example` carry the book's display
/// strings (preserved verbatim, including punctuation). `hp` is read
/// straight from the rightmost column of p.183 — see the file header in
/// `content/catalogs/cover.ron` for the two RAW idiosyncrasies this
/// module preserves rather than normalising.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverMaterial {
    /// Slug identifier — also the catalog lookup key.
    pub id: String,
    /// Display name for the example object as printed on p.183
    /// (e.g. "Bank Vault Door").
    pub display_name: String,
    /// Free-form example string. Usually identical to `display_name`,
    /// but may carry an extra annotation when p.183 documents two
    /// thicknesses for the same example (see *Bulletproof Windshield*).
    pub example: String,
    /// Material classification per the p.182 *Type of Cover* table.
    pub material: MaterialKind,
    /// Thickness classification per the p.182 *Type of Cover* table.
    pub thickness: Thickness,
    /// Cover HP as printed in the rightmost column of p.183.
    pub hp: u16,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/cover.ron`.
///
/// The file is a `(covers: [ ... ])` envelope where each entry has the
/// fields of [`CoverMaterial`]. Decoupling the on-disk schema from the
/// in-memory `Catalog<T>` keeps the authored content readable as a flat
/// list while the loader builds the lookup map.
// See p.183.
#[derive(Debug, Deserialize)]
struct CoverFile {
    covers: Vec<CoverFileEntry>,
}

/// One row in the on-disk cover catalog file.
///
/// `id` doubles as the `Catalog<T>` key; the loader fails the load on a
/// duplicate id.
// See p.183.
#[derive(Debug, Deserialize)]
struct CoverFileEntry {
    id: String,
    display_name: String,
    example: String,
    material: MaterialKind,
    thickness: Thickness,
    hp: u16,
}

/// Load the cover catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<CoverMaterial>`] keyed by `id`. On
/// failure returns [`RulesError::CatalogLoadFailed`] carrying the file
/// path and a stringified description of the underlying I/O or parse
/// error.
///
/// The loader enforces one invariant: `id`s are unique within the file.
/// A duplicate id fails the load.
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader
/// loading conventions every Phase 2 catalog follows.
// See p.183.
pub fn load_cover_catalog(path: &Path) -> Result<Catalog<CoverMaterial>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: CoverFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, CoverMaterial> = HashMap::with_capacity(parsed.covers.len());
    for row in parsed.covers {
        let def = CoverMaterial {
            id: row.id.clone(),
            display_name: row.display_name,
            example: row.example,
            material: row.material,
            thickness: row.thickness,
            hp: row.hp,
        };
        if entries.insert(row.id.clone(), def).is_some() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate id: '{}'", row.id),
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

    /// Workspace-relative path to the canonical cover catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog lives
    /// two parents up at `content/catalogs/cover.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("cover.ron");
        p
    }

    /// Acceptance: every row of the *Cover Material and Thickness
    /// Examples* table on p.183 is present in the catalog.
    ///
    /// Count derivation (verified against p.183, 24 visible rows):
    /// Bank Vault Door, Bank Window Glass, Bar, Boulder,
    /// Bulletproof Windshield, Car Door, Data Term, Engine Block,
    /// Hydrant, Log Cabin Wall, Metal Door, Office Cubicle, Office Wall,
    /// Overturned Table, Prison Visitation Glass, Refrigerator,
    /// Shipping Container, Sofa, Statue, Tree, Utility Pole, Wardrobe,
    /// Windshield, Wooden Door = **24**.
    #[test]
    fn test_cover_table_complete() {
        let cat = load_cover_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            24,
            "expected 24 cover examples per p.183 (got {}); verify the catalog RON file",
            cat.len()
        );
    }

    /// Acceptance: the *Bank Vault Door* row of p.183 — Thick Steel —
    /// is recorded as 50 HP. Pinned per the WP-207 acceptance criterion
    /// `test_thick_steel_50_hp`.
    #[test]
    fn test_thick_steel_50_hp() {
        let cat = load_cover_catalog(&catalog_path()).expect("catalog must load");
        let bvd = cat
            .get("bank_vault_door")
            .expect("bank_vault_door must be present");
        assert_eq!(bvd.material, MaterialKind::Steel);
        assert_eq!(bvd.thickness, Thickness::Thick);
        assert_eq!(bvd.hp, 50, "Thick Steel = 50 HP per p.183");
        assert_eq!(bvd.display_name, "Bank Vault Door");
    }

    /// Acceptance: a `CoverMaterial` round-trips through RON without
    /// loss — covers the WP-207 `test_cover_round_trip_ron` criterion.
    #[test]
    fn test_cover_round_trip_ron() {
        let original = CoverMaterial {
            id: "test_door".to_string(),
            display_name: "Test Door".to_string(),
            example: "Test Door".to_string(),
            material: MaterialKind::Steel,
            thickness: Thickness::Thick,
            hp: 50,
        };
        let serialised = ron::ser::to_string(&original).expect("must serialise");
        let restored: CoverMaterial = ron::de::from_str(&serialised).expect("must round-trip");
        assert_eq!(restored, original);

        // And every variant of the supporting enums round-trips.
        for m in [
            MaterialKind::Steel,
            MaterialKind::Wood,
            MaterialKind::Stone,
            MaterialKind::Concrete,
            MaterialKind::BulletproofGlass,
            MaterialKind::PlasterFoamPlastic,
            MaterialKind::Glass,
            MaterialKind::Other,
        ] {
            let s = ron::ser::to_string(&m).expect("must serialise");
            let back: MaterialKind = ron::de::from_str(&s).expect("must round-trip");
            assert_eq!(back, m);
        }
        for t in [Thickness::Thin, Thickness::Thick] {
            let s = ron::ser::to_string(&t).expect("must serialise");
            let back: Thickness = ron::de::from_str(&s).expect("must round-trip");
            assert_eq!(back, t);
        }
    }

    /// Regression: every entry's `id` field equals the catalog slug under
    /// which the loader stores it. Future tooling
    /// (`tools/content-validator`) relies on this identity.
    #[test]
    fn test_catalog_id_matches_slug() {
        let cat = load_cover_catalog(&catalog_path()).expect("catalog must load");
        for (slug, def) in cat.iter() {
            assert_eq!(slug, &def.id, "slug '{slug}' disagrees with entry id");
        }
    }

    /// Regression: HP values for Thick Steel rows match the p.182 column
    /// (50 HP). p.183 lists three Thick-Steel examples — Bank Vault Door,
    /// Engine Block, Hydrant.
    #[test]
    fn test_all_thick_steel_50_hp() {
        let cat = load_cover_catalog(&catalog_path()).expect("catalog must load");
        for slug in ["bank_vault_door", "engine_block", "hydrant"] {
            let def = cat.get(slug).unwrap_or_else(|| panic!("missing: {slug}"));
            assert_eq!(def.material, MaterialKind::Steel);
            assert_eq!(def.thickness, Thickness::Thick);
            assert_eq!(def.hp, 50, "{slug} should be 50 HP per p.183");
        }
    }

    /// Regression: zero-HP "Not Cover" rows from p.183 are preserved
    /// (Office Cubicle, Windshield).
    #[test]
    fn test_not_cover_rows_zero_hp() {
        let cat = load_cover_catalog(&catalog_path()).expect("catalog must load");
        let oc = cat.get("office_cubicle").expect("office_cubicle present");
        assert_eq!(oc.hp, 0);
        let ws = cat.get("windshield").expect("windshield present");
        assert_eq!(ws.hp, 0);
    }
}
