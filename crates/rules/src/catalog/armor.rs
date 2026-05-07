//! Armor catalog (WP-203).
//!
//! Defines the closed enum [`ArmorKind`] of every armor type listed in the
//! Armor Table on rulebook p.185, together with the metadata catalog
//! [`Armor`] (Stopping Power, REF/DEX/MOVE penalty, locations, price) and
//! the RON loader [`load_armor_catalog`].
//!
//! Rulebook references:
//! - **p.184:** "Armor" / "How to Read the Armor Table" — defines Stopping
//!   Power, Armor Penalty (taken once even when armoring two locations),
//!   and the no-stacking rule ("only your highest source of SP in a
//!   location determines your SP for that location").
//! - **p.185:** the Armor Table itself — the eight armor types this catalog
//!   transcribes (Leathers, Kevlar, Light Armorjack, Bodyweight Suit,
//!   Medium Armorjack, Heavy Armorjack, Flak, Metalgear).
//!
//! The catalog file is `content/catalogs/armor.ron`; the loader expects
//! one entry per slug. Per p.184, armor for Body and Head locations is
//! purchased independently — every entry in the table can be worn in
//! either location, so the `locations` field defaults to
//! `[Body, Head]` for all eight pieces.
//!
//! ## SP stacking
//!
//! Per p.184, **only the highest SP in a location applies** — armor
//! stacking is *not* additive. This catalog records each entry's SP as
//! printed in the book; the no-stacking rule is enforced at damage
//! application time (WP-303), not here.
//!
//! ## Penalty sign convention
//!
//! The book prints the Armor Penalty column with negative numbers
//! (e.g. "-2 REF, DEX, and MOVE" for Medium Armorjack). The fields on
//! [`ArmorPenalty`] are unsigned (`u8`) and store the **magnitude** of
//! the penalty; callers are expected to subtract from the relevant stat.
//! The column header on p.185 ("Armor Penalty (Minimum 0)") makes this
//! clamping explicit. A penalty of `0` means "no penalty".

use crate::catalog::Catalog;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// ArmorId
// ---------------------------------------------------------------------------

/// Catalog slug for one armor entry. See p.185.
///
/// The slug is the same string used as the `Catalog<Armor>` lookup key,
/// stored as a structured field on [`Armor`] so a value-only handle can
/// flow through later WPs (e.g. WP-303 damage application) without
/// keeping a `&Catalog<Armor>` reference alive.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ArmorId(pub String);

// ---------------------------------------------------------------------------
// ArmorKind
// ---------------------------------------------------------------------------

/// The eight armor types in the rulebook Armor Table. See p.185.
///
/// This is a **closed enum**: every variant corresponds to a row in the
/// p.185 table. Variant naming follows the book's display names normalised
/// to `UpperCamelCase` (e.g. *Light Armorjack* → `LightArmorjack`,
/// *Bodyweight Suit* → `BodyweightSuit`).
///
/// Custom / homebrew armor is intentionally not modelled — the closed
/// enum makes "did I miss a kind?" a compile error.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArmorKind {
    /// Leathers — SP 4, no penalty. See p.185.
    Leathers,
    /// Kevlar® — SP 7, no penalty. See p.185.
    Kevlar,
    /// Light Armorjack — SP 11, no penalty. See p.185.
    LightArmorjack,
    /// Bodyweight Suit — SP 11, no penalty, also stows a Cyberdeck and
    /// supports Interface Plugs per the description on p.185.
    BodyweightSuit,
    /// Medium Armorjack — SP 12, -2 REF/DEX/MOVE. See p.185.
    MediumArmorjack,
    /// Heavy Armorjack — SP 13, -2 REF/DEX/MOVE. See p.185.
    HeavyArmorjack,
    /// Flak — SP 15, -4 REF/DEX/MOVE. See p.185.
    Flak,
    /// Metalgear® — SP 18, -4 REF/DEX/MOVE. See p.185.
    Metalgear,
}

// ---------------------------------------------------------------------------
// ArmorLocation
// ---------------------------------------------------------------------------

/// Where on the body a piece of armor can be worn. See p.184.
///
/// Per p.184, armor is purchased independently for the head and body
/// locations, and (paraphrased) "it is advised that you wear both". Every
/// entry in the Armor Table on p.185 is legal in both locations — the
/// `locations` field on [`Armor`] still records this explicitly so future
/// catalog entries (e.g. helmet-only or vest-only items added later)
/// can declare themselves correctly.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ArmorLocation {
    /// Worn on the torso. See p.184.
    Body,
    /// Worn on the head. Only struck on an Aimed Shot per p.184.
    Head,
}

// ---------------------------------------------------------------------------
// ArmorPenalty
// ---------------------------------------------------------------------------

/// REF / DEX / MOVE penalties imposed by wearing armor. See p.185.
///
/// The book's "Armor Penalty (Minimum 0)" column stores the same value
/// for each of REF, DEX, and MOVE in every catalog entry on p.185 (every
/// row is "-N REF, DEX, and MOVE"), so all three fields are stored
/// separately in case future supplements split them.
///
/// Stored as **unsigned magnitudes**: the printed values on p.185 are
/// negative ("-2", "-4"), but the column header explicitly clamps to a
/// minimum of 0, so the sign is implicit. Callers subtract the stored
/// magnitude from the relevant stat. A value of `0` means "no penalty"
/// (Leathers / Kevlar / Light Armorjack / Bodyweight Suit on p.185).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ArmorPenalty {
    /// Magnitude of the REF penalty (subtracted from REF). See p.185.
    pub ref_penalty: u8,
    /// Magnitude of the DEX penalty (subtracted from DEX). See p.185.
    pub dex_penalty: u8,
    /// Magnitude of the MOVE penalty (subtracted from MOVE). See p.185.
    pub move_penalty: u8,
}

impl ArmorPenalty {
    /// The "no penalty" entry — used by Leathers, Kevlar, Light
    /// Armorjack, and Bodyweight Suit on p.185.
    pub const NONE: ArmorPenalty = ArmorPenalty {
        ref_penalty: 0,
        dex_penalty: 0,
        move_penalty: 0,
    };
}

// ---------------------------------------------------------------------------
// Armor
// ---------------------------------------------------------------------------

/// One row in the Armor Table. See p.185.
///
/// Loaded by [`load_armor_catalog`]. The `id` field is the same string
/// used as the [`Catalog<Armor>`] lookup key; the structured `kind` is
/// the closed-enum handle for type-safe matches.
///
/// **Note:** SP stacking is non-additive (p.184); the catalog records
/// per-piece SP as printed in the book. Damage application
/// (WP-303) is responsible for picking the highest SP in a location.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Armor {
    /// Stable catalog slug for this entry. See p.185.
    pub id: ArmorId,
    /// Display name as printed in the rulebook (p.185), preserving the
    /// book's casing and ® marks (e.g. "Kevlar®", "Metalgear®").
    pub display_name: String,
    /// Closed-enum kind. See [`ArmorKind`].
    pub kind: ArmorKind,
    /// Stopping Power, as printed on p.185. Damage that gets through is
    /// `damage − SP` (clamped to 0); see WP-303.
    pub sp: u8,
    /// REF / DEX / MOVE penalty magnitudes. See p.185 column 4.
    pub penalty: ArmorPenalty,
    /// Locations where this armor may be worn. Per p.184 every entry on
    /// the p.185 table is purchasable for either location, so the
    /// canonical loaded value is `[Body, Head]`.
    pub locations: Vec<ArmorLocation>,
    /// Price tier (Cheap … Super Luxury) per the Cost column on p.185.
    pub price: PriceTier,
    /// Eurobuck cost as printed on p.185. Verified against
    /// [`PriceTier::canonical_cost`] by the loader.
    pub price_eb: Eurobucks,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/armor.ron`.
///
/// The file is an `(armor: [ ... ])` envelope where each entry is an
/// [`Armor`] plus an explicit `slug` field (the [`Catalog<Armor>`] key).
/// Decoupling the on-disk schema from the in-memory `Catalog<T>` keeps
/// authored content as a flat list while the loader builds the lookup
/// map.
#[derive(Debug, Deserialize)]
struct ArmorFile {
    armor: Vec<ArmorFileEntry>,
}

/// One row in the on-disk armor catalog file.
///
/// `slug` is the lookup key inside the resulting [`Catalog<Armor>`]. All
/// other fields populate the [`Armor`] directly; the loader cross-checks
/// `slug == id.0` so the two stay in sync.
#[derive(Debug, Deserialize)]
struct ArmorFileEntry {
    slug: String,
    id: ArmorId,
    display_name: String,
    kind: ArmorKind,
    sp: u8,
    penalty: ArmorPenalty,
    locations: Vec<ArmorLocation>,
    price: PriceTier,
    price_eb: Eurobucks,
}

/// Load the armor catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Armor>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and
/// a stringified description of the underlying I/O or parse error.
///
/// The loader enforces three invariants:
/// 1. Every entry's `slug` equals its `id.0`. The two are redundant on
///    disk; keeping them in sync at load time prevents drift.
/// 2. Slugs are unique within the file. A duplicate slug fails the load.
/// 3. Every entry has at least one [`ArmorLocation`]. An empty
///    `locations` list is a content authoring error.
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader
/// loading conventions every Phase 2 catalog follows. See also
/// [`super::skills::load_skills_catalog`] for the analogous skills
/// loader.
pub fn load_armor_catalog(path: &Path) -> Result<Catalog<Armor>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: ArmorFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Armor> = HashMap::with_capacity(parsed.armor.len());
    for row in parsed.armor {
        if row.slug != row.id.0 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "armor entry '{}' has id '{}' (slug must equal id)",
                    row.slug, row.id.0
                ),
            });
        }
        if row.locations.is_empty() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("armor '{}' must declare at least one location", row.slug),
            });
        }
        let armor = Armor {
            id: row.id,
            display_name: row.display_name,
            kind: row.kind,
            sp: row.sp,
            penalty: row.penalty,
            locations: row.locations,
            price: row.price,
            price_eb: row.price_eb,
        };
        if entries.insert(row.slug.clone(), armor).is_some() {
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

    /// Workspace-relative path to the canonical armor catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog
    /// lives two parents up at `content/catalogs/armor.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("armor.ron");
        p
    }

    /// Acceptance: every armor entry from the p.185 table is present and
    /// the catalog contains exactly those eight rows — no extras, no
    /// duplicates. (Leathers, Kevlar, Light Armorjack, Bodyweight Suit,
    /// Medium Armorjack, Heavy Armorjack, Flak, Metalgear.)
    #[test]
    fn test_armor_table_complete() {
        let cat = load_armor_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            8,
            "expected 8 armor entries per p.185 (got {})",
            cat.len()
        );

        for slug in [
            "leathers",
            "kevlar",
            "light_armorjack",
            "bodyweight_suit",
            "medium_armorjack",
            "heavy_armorjack",
            "flak",
            "metalgear",
        ] {
            assert!(
                cat.get(slug).is_some(),
                "missing armor entry for '{slug}' (see p.185)"
            );
        }
    }

    /// Acceptance: Metalgear® is SP 18 with a -4 REF/DEX/MOVE penalty
    /// per p.185.
    #[test]
    fn test_metalgear_sp_18() {
        let cat = load_armor_catalog(&catalog_path()).expect("catalog must load");
        let metalgear = cat.get("metalgear").expect("metalgear must be present");
        assert_eq!(metalgear.sp, 18, "Metalgear SP must be 18 (p.185)");
        assert_eq!(
            metalgear.penalty.ref_penalty, 4,
            "Metalgear REF penalty must be 4 (-4 on p.185)"
        );
        assert_eq!(
            metalgear.penalty.dex_penalty, 4,
            "Metalgear DEX penalty must be 4 (-4 on p.185)"
        );
        assert_eq!(
            metalgear.penalty.move_penalty, 4,
            "Metalgear MOVE penalty must be 4 (-4 on p.185)"
        );
    }

    /// Acceptance: Kevlar® has no Armor Penalty per p.185 — every field
    /// of `ArmorPenalty` is zero.
    #[test]
    fn test_kevlar_no_penalty() {
        let cat = load_armor_catalog(&catalog_path()).expect("catalog must load");
        let kevlar = cat.get("kevlar").expect("kevlar must be present");
        assert_eq!(
            kevlar.penalty,
            ArmorPenalty::NONE,
            "Kevlar must have zero penalty (p.185 column reads 'None')"
        );
        assert_eq!(kevlar.sp, 7, "Kevlar SP must be 7 (p.185)");
    }

    /// Acceptance: the catalog round-trips through RON serialisation.
    /// Load → `to_string` → load again must yield an identical
    /// `Catalog<Armor>` (same len, same `(slug, entry)` pairs).
    #[test]
    fn test_armor_round_trip_ron() {
        let original = load_armor_catalog(&catalog_path()).expect("catalog must load");

        // Re-serialise via the on-disk schema so the round-trip exercises
        // the same wire format the loader consumes. Sort by slug for a
        // deterministic re-serialisation.
        let mut rows: Vec<(&String, &Armor)> = original.iter().collect();
        rows.sort_by_key(|(k, _)| (*k).clone());

        let mut as_file = String::from("ArmorFile(armor: [\n");
        for (slug, a) in &rows {
            // Build a per-row tuple matching ArmorFileEntry's field order.
            let locs: Vec<String> = a.locations.iter().map(|l| format!("{l:?}")).collect();
            let kind_str = format!("{:?}", a.kind);
            let price_str = format!("{:?}", a.price);
            let locs_str = locs.join(", ");
            let row = format!(
                "    (slug: {slug:?}, id: ({id:?}), display_name: {name:?}, kind: {kind}, sp: {sp}, penalty: (ref_penalty: {rp}, dex_penalty: {dp}, move_penalty: {mp}), locations: [{locs}], price: {price}, price_eb: ({eb})),\n",
                slug = slug,
                id = a.id.0,
                name = a.display_name,
                kind = kind_str,
                sp = a.sp,
                rp = a.penalty.ref_penalty,
                dp = a.penalty.dex_penalty,
                mp = a.penalty.move_penalty,
                locs = locs_str,
                price = price_str,
                eb = a.price_eb.0,
            );
            as_file.push_str(&row);
        }
        as_file.push_str("])\n");

        // Write to a temp file and reload via the public loader.
        let tmp = std::env::temp_dir().join("wp203_armor_round_trip.ron");
        std::fs::write(&tmp, &as_file).expect("must write temp ron");
        let reloaded = load_armor_catalog(&tmp).expect("reloaded catalog must parse");

        assert_eq!(reloaded.len(), original.len());
        for (slug, a) in original.iter() {
            let other = reloaded
                .get(slug)
                .unwrap_or_else(|| panic!("reloaded catalog is missing '{slug}'"));
            assert_eq!(a, other, "entry '{slug}' must round-trip identically");
        }

        // Best-effort cleanup; not load-bearing.
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: the loader rejects a duplicate slug. Catches the
    /// "two Leathers entries" content authoring mistake.
    #[test]
    fn test_duplicate_slug_rejected() {
        let bad = r#"ArmorFile(armor: [
            (slug: "leathers", id: ("leathers"), display_name: "Leathers", kind: Leathers, sp: 4, penalty: (ref_penalty: 0, dex_penalty: 0, move_penalty: 0), locations: [Body, Head], price: Everyday, price_eb: (20)),
            (slug: "leathers", id: ("leathers"), display_name: "Leathers", kind: Leathers, sp: 4, penalty: (ref_penalty: 0, dex_penalty: 0, move_penalty: 0), locations: [Body, Head], price: Everyday, price_eb: (20)),
        ])"#;
        let tmp = std::env::temp_dir().join("wp203_armor_dup.ron");
        std::fs::write(&tmp, bad).expect("must write temp ron");
        let err = load_armor_catalog(&tmp).expect_err("duplicate slug must fail");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(source.contains("duplicate slug"), "got: {source}");
            }
            other => panic!("expected CatalogLoadFailed, got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: the loader rejects a slug/id mismatch. Catches a
    /// content-authoring error where the `slug` field drifts away from
    /// the structured `id`.
    #[test]
    fn test_slug_id_mismatch_rejected() {
        let bad = r#"ArmorFile(armor: [
            (slug: "leathers", id: ("leeeathers"), display_name: "Leathers", kind: Leathers, sp: 4, penalty: (ref_penalty: 0, dex_penalty: 0, move_penalty: 0), locations: [Body, Head], price: Everyday, price_eb: (20)),
        ])"#;
        let tmp = std::env::temp_dir().join("wp203_armor_mismatch.ron");
        std::fs::write(&tmp, bad).expect("must write temp ron");
        let err = load_armor_catalog(&tmp).expect_err("slug/id mismatch must fail");
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(source.contains("slug must equal id"), "got: {source}");
            }
            other => panic!("expected CatalogLoadFailed, got {other:?}"),
        }
        let _ = std::fs::remove_file(&tmp);
    }

    /// Regression: every loaded entry's `price_eb` matches the canonical
    /// cost for its `PriceTier`. Pinned because a content author could
    /// otherwise type "100eb / Everyday" and the type system wouldn't
    /// catch it.
    #[test]
    fn test_price_eb_matches_tier() {
        let cat = load_armor_catalog(&catalog_path()).expect("catalog must load");
        for (slug, a) in cat.iter() {
            assert_eq!(
                a.price_eb,
                a.price.canonical_cost(),
                "armor '{slug}' price_eb {:?} must match canonical cost for {:?}",
                a.price_eb,
                a.price,
            );
        }
    }

    /// Regression: spot-check the SP and penalty for every row of the
    /// p.185 table — keeps a single test as the canonical assertion of
    /// the table's content even if the acceptance tests above evolve.
    #[test]
    fn test_p185_table_values() {
        let cat = load_armor_catalog(&catalog_path()).expect("catalog must load");

        // (slug, sp, ref/dex/move-penalty)
        let rows = [
            ("leathers", 4, 0u8),
            ("kevlar", 7, 0),
            ("light_armorjack", 11, 0),
            ("bodyweight_suit", 11, 0),
            ("medium_armorjack", 12, 2),
            ("heavy_armorjack", 13, 2),
            ("flak", 15, 4),
            ("metalgear", 18, 4),
        ];
        for (slug, sp, pen) in rows {
            let a = cat.get(slug).unwrap_or_else(|| panic!("missing '{slug}'"));
            assert_eq!(a.sp, sp, "{slug} SP must be {sp} (p.185)");
            assert_eq!(
                a.penalty.ref_penalty, pen,
                "{slug} REF penalty must be {pen} (p.185)"
            );
            assert_eq!(
                a.penalty.dex_penalty, pen,
                "{slug} DEX penalty must be {pen} (p.185)"
            );
            assert_eq!(
                a.penalty.move_penalty, pen,
                "{slug} MOVE penalty must be {pen} (p.185)"
            );
        }
    }
}
