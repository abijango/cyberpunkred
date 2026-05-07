//! Street drugs catalog (WP-206).
//!
//! Catalogs every street drug listed in the rulebook's *Street Drugs*
//! section (pp.227–228), capturing each drug's Primary-Effect modifier
//! list, duration, addiction profile, and Night-Market price tier.
//!
//! Rulebook references:
//! - **pp.227–228 (with overflow onto p.229):** the canonical *Street
//!   Drugs* list. Five drugs in total: Black Lace, Blue Glass, Boost,
//!   Smash, Synthcoke.
//! - **p.227 (intro paragraph):** "When you take street drugs, you typically
//!   are doing so using an Action with an Airhypo … When you are dosed
//!   with one of these drugs, you are automatically affected by the drug's
//!   Primary Effect." This catalog stores the Primary Effect only;
//!   Secondary-Effect (addiction) modifiers are applied separately when
//!   the addiction-resistance roll fails.
//!
//! The catalog file is `content/catalogs/drugs.ron`; the loader expects
//! one entry per slug. Because drugs are content-extensible (homebrew
//! pharmacology is part of CPR's flavour), [`DrugId`] remains a
//! `String`-newtype rather than a closed enum — see
//! `crates/rules/src/effects/mod.rs` for the placeholder definition.
//!
//! ## Duration encoding
//!
//! The book gives drug durations in wall-clock hours (4h or 24h) but the
//! [`EffectDuration`] enum has no `Hours` variant; the closest faithful
//! RAW match is [`EffectDuration::UntilGigEnd`], which approximates "for
//! the rest of the current scene/gig." Every drug in this catalog uses
//! `UntilGigEnd`. A future WP that adds a wall-clock duration variant
//! should re-encode these entries; the change is purely additive
//! (descriptions already record the literal "Lasts 4 Hours" / "Lasts
//! 24 Hours" wording).
//!
//! ## Addiction
//!
//! All five canonical drugs from pp.227–228 are addictive per RAW (each
//! lists a Secondary Effect that triggers on a failed
//! `WILL + Resist Torture/Drugs + 1d10` roll vs. the drug's DV — see
//! p.227 intro). The `addictive` flag stays on the [`Drug`] struct so
//! homebrew non-addictive entries — and any future official supplement —
//! can be modelled without a schema change.

use crate::catalog::Catalog;
use crate::effects::{DrugId, EffectDuration, EffectModifier};
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Drug
// ---------------------------------------------------------------------------

/// A row in the canonical drugs catalog (`content/catalogs/drugs.ron`).
///
/// Loaded by [`load_drugs_catalog`]. The `id` carries the drug's stable
/// catalog slug (mirrored as the `Catalog<T>` key); `effects` lists the
/// modifiers that apply for the duration of the drug's *Primary Effect*
/// (book p.227 — "When you are dosed with one of these drugs, you are
/// automatically affected by the drug's Primary Effect"). Secondary
/// (addiction) effects are *not* stored here — they fire only after a
/// failed resistance roll and are applied as a separate [`crate::effects::ActiveEffect`]
/// by whichever subsystem implements the addiction resolution.
///
/// `price` records the Night-Market tier (p.339) and `price_eb` the
/// canonical eurobuck cost (`PriceTier::canonical_cost`); both are kept
/// to let UI tooling display the tier label *and* the literal cost
/// without an extra lookup.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Drug {
    /// Catalog slug — the canonical handle for this drug. Doubles as the
    /// key inside [`Catalog<Drug>`].
    pub id: DrugId,
    /// Display name as printed in the rulebook (pp.227–228).
    pub display_name: String,
    /// Modifiers applied for the duration of the drug's Primary Effect.
    /// Empty for drugs whose Primary Effect is purely narrative
    /// (e.g. Blue Glass's "flashing out" hallucinations) or whose
    /// rules-level effect is conditional and applied by a dedicated
    /// subsystem (e.g. Black Lace's "ignores Seriously Wounded").
    pub effects: Vec<EffectModifier>,
    /// How long the Primary Effect lasts. See the module docs for the
    /// encoding limitation (book uses wall-clock hours; the engine
    /// encodes them as [`EffectDuration::UntilGigEnd`]).
    pub duration: EffectDuration,
    /// `true` iff the drug carries a Secondary Effect that creates an
    /// addiction (every canonical drug from pp.227–228 sets this to
    /// `true`). See p.227 intro for the resistance roll.
    pub addictive: bool,
    /// Night-Market price tier — see p.339.
    pub price: PriceTier,
    /// Canonical eurobuck cost. Always equal to `price.canonical_cost()`;
    /// the loader enforces this invariant.
    pub price_eb: Eurobucks,
    /// Human-readable summary distilled from the rulebook entry. The LLM
    /// narrator uses this for flavour; the engine itself ignores it.
    pub description: String,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/drugs.ron`.
///
/// The file is a `(drugs: [ ... ])` envelope where each entry is a
/// [`DrugsFileEntry`] that mirrors [`Drug`] but pulls the slug out of
/// the `DrugId` payload into a top-level field. Decoupling the on-disk
/// schema from the in-memory `Catalog<T>` lets the authored content
/// stay readable (a flat list, not a map literal) while the loader
/// computes the lookup map.
#[derive(Debug, Deserialize)]
struct DrugsFile {
    drugs: Vec<DrugsFileEntry>,
}

/// One row in the on-disk drugs catalog file.
///
/// `slug` is the lookup key inside the resulting `Catalog<Drug>`. All
/// other fields populate the [`Drug`] directly. The slug is *also*
/// promoted to `Drug::id` (as `DrugId(slug.clone())`) to keep the
/// in-memory representation self-describing.
#[derive(Debug, Deserialize)]
struct DrugsFileEntry {
    slug: String,
    display_name: String,
    effects: Vec<EffectModifier>,
    duration: EffectDuration,
    addictive: bool,
    price: PriceTier,
    price_eb: Eurobucks,
    description: String,
}

/// Load the drugs catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Drug>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and
/// a stringified description of the underlying I/O or parse error.
///
/// The loader enforces two invariants:
/// 1. Slugs are unique within the file. A duplicate slug fails the load.
/// 2. Each entry's `price_eb` equals `price.canonical_cost()`. Mismatches
///    fail the load — keeping authored content from drifting away from
///    the Night-Market table on p.339.
///
/// See `IMPLEMENTATION_PLAN.md` §2.5 (content files) for the broader
/// loading conventions every Phase 2 catalog follows.
pub fn load_drugs_catalog(path: &Path) -> Result<Catalog<Drug>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: DrugsFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Drug> = HashMap::with_capacity(parsed.drugs.len());
    for row in parsed.drugs {
        let canonical = row.price.canonical_cost();
        if row.price_eb != canonical {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "drug '{}' has price_eb {:?} but tier {:?} canonical cost is {:?} (see p.339)",
                    row.slug, row.price_eb, row.price, canonical
                ),
            });
        }
        let drug = Drug {
            id: DrugId(row.slug.clone()),
            display_name: row.display_name,
            effects: row.effects,
            duration: row.duration,
            addictive: row.addictive,
            price: row.price,
            price_eb: row.price_eb,
            description: row.description,
        };
        if entries.insert(row.slug.clone(), drug).is_some() {
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

    /// Workspace-relative path to the canonical drugs catalog file.
    ///
    /// `CARGO_MANIFEST_DIR` resolves to `crates/rules/`; the catalog
    /// lives two parents up at `content/catalogs/drugs.ron`.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("drugs.ron");
        p
    }

    /// Acceptance: every drug listed in *Street Drugs* (pp.227–228) is
    /// present in the catalog, and the catalog contains exactly that many
    /// entries (no duplicates, no extras).
    ///
    /// Count derivation (verified against pp.227–228):
    ///   Black Lace, Blue Glass, Boost, Smash, Synthcoke — **5 drugs.**
    #[test]
    fn test_drug_catalog_complete() {
        let cat = load_drugs_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            5,
            "expected 5 drugs per pp.227-228 (got {}); verify the catalog RON file",
            cat.len()
        );

        for slug in ["black_lace", "blue_glass", "boost", "smash", "synthcoke"] {
            assert!(
                cat.get(slug).is_some(),
                "missing drug entry for slug '{slug}' (see pp.227-228)"
            );
        }
    }

    /// Acceptance: the `addictive` flag agrees with the rulebook for
    /// every catalogued drug.
    ///
    /// Per RAW (pp.227–228) every named street drug has a Secondary
    /// Effect that establishes addiction on a failed
    /// `WILL + Resist Torture/Drugs + 1d10` roll, so all five entries
    /// must be flagged addictive. The struct field nonetheless stays on
    /// [`Drug`] so homebrew or future-supplement non-addictive entries
    /// can be modelled without a schema change — see the module docs.
    ///
    /// The acceptance criterion as written ("at least one addictive and
    /// one non-addictive drug") cannot be satisfied with the canonical
    /// pp.227–228 list alone, so the test instead pins the stronger
    /// invariant: every official drug is addictive *and* the flag is
    /// honoured. A non-addictive fixture round-trip in
    /// `test_drug_round_trip_ron` proves the flag works in the
    /// false-direction too.
    #[test]
    fn test_addictive_flag() {
        let cat = load_drugs_catalog(&catalog_path()).expect("catalog must load");

        // Every canonical drug from pp.227–228 is addictive per RAW.
        for slug in ["black_lace", "blue_glass", "boost", "smash", "synthcoke"] {
            let d = cat
                .get(slug)
                .unwrap_or_else(|| panic!("missing drug: {slug}"));
            assert!(
                d.addictive,
                "drug '{slug}' must be flagged addictive per pp.227-228"
            );
        }

        // Cross-check: a synthetic non-addictive drug round-trips
        // through serialisation with the flag set to false. This proves
        // the catalog can represent both branches even though every
        // canonical entry happens to be addictive.
        let placebo = Drug {
            id: DrugId("placebo".into()),
            display_name: "Placebo".into(),
            effects: Vec::new(),
            duration: EffectDuration::UntilGigEnd,
            addictive: false,
            price: PriceTier::Cheap,
            price_eb: PriceTier::Cheap.canonical_cost(),
            description:
                "A homebrew non-addictive drug used to exercise the addictive=false branch.".into(),
        };
        let s = ron::ser::to_string(&placebo).expect("placebo must serialise");
        let back: Drug = ron::de::from_str(&s).expect("placebo must round-trip");
        assert!(!back.addictive);
        assert_eq!(back, placebo);
    }

    /// Acceptance: load → serialise → reload yields an identical catalog.
    ///
    /// The test serialises every loaded entry into a fresh RON document
    /// (matching the on-disk schema with its `slug` field), writes it to
    /// a temp file, reloads via the public loader, and asserts that the
    /// reloaded catalog equals the original entry-for-entry. This pins
    /// both the `serde` round-trip *and* the loader's slug→`DrugId`
    /// promotion.
    #[test]
    fn test_drug_round_trip_ron() {
        let original = load_drugs_catalog(&catalog_path()).expect("catalog must load");

        // Re-serialise via the on-disk schema. We can't reuse `Drug`
        // directly because the file format hoists `slug` out of the
        // `DrugId` payload.
        #[derive(Serialize)]
        struct OutEntry<'a> {
            slug: &'a str,
            display_name: &'a str,
            effects: &'a Vec<EffectModifier>,
            duration: &'a EffectDuration,
            addictive: bool,
            price: &'a PriceTier,
            price_eb: &'a Eurobucks,
            description: &'a str,
        }
        #[derive(Serialize)]
        struct OutFile<'a> {
            drugs: Vec<OutEntry<'a>>,
        }

        // Sort by slug for deterministic output ordering — `Catalog::iter`
        // is `HashMap`-order, which is not stable.
        let mut rows: Vec<(&String, &Drug)> = original.iter().collect();
        rows.sort_by_key(|(slug, _)| (*slug).clone());

        let out = OutFile {
            drugs: rows
                .iter()
                .map(|(slug, d)| OutEntry {
                    slug: slug.as_str(),
                    display_name: d.display_name.as_str(),
                    effects: &d.effects,
                    duration: &d.duration,
                    addictive: d.addictive,
                    price: &d.price,
                    price_eb: &d.price_eb,
                    description: d.description.as_str(),
                })
                .collect(),
        };
        let serialised = ron::ser::to_string_pretty(&out, ron::ser::PrettyConfig::default())
            .expect("must re-serialise");

        // Write to a unique temp path, reload, compare.
        let mut tmp = std::env::temp_dir();
        tmp.push(format!(
            "cpr_drugs_round_trip_{}_{}.ron",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::write(&tmp, &serialised).expect("must write temp file");
        let reloaded = load_drugs_catalog(&tmp).expect("reload must succeed");
        // Don't leave a temp file behind even if assertions fail —
        // remove before assertions.
        let _ = std::fs::remove_file(&tmp);

        assert_eq!(reloaded.len(), original.len(), "entry count must match");
        for (slug, drug) in original.iter() {
            let other = reloaded
                .get(slug)
                .unwrap_or_else(|| panic!("reloaded catalog missing slug '{slug}'"));
            assert_eq!(other, drug, "round-trip mismatch for slug '{slug}'");
        }
    }

    /// Regression: every loaded drug's `price_eb` matches the canonical
    /// cost for its tier (the loader enforces this; the test pins the
    /// invariant against accidental loader-bypass in the future).
    #[test]
    fn test_price_eb_matches_tier() {
        let cat = load_drugs_catalog(&catalog_path()).expect("catalog must load");
        for (slug, d) in cat.iter() {
            assert_eq!(
                d.price_eb,
                d.price.canonical_cost(),
                "drug '{slug}' price_eb disagrees with tier canonical cost (p.339)"
            );
        }
    }

    /// Regression: the loader rejects a duplicate slug.
    #[test]
    fn test_loader_rejects_duplicate_slug() {
        let dup = r#"
DrugsFile(
    drugs: [
        (
            slug: "x",
            display_name: "X",
            effects: [],
            duration: UntilGigEnd,
            addictive: false,
            price: Cheap,
            price_eb: (10),
            description: "dup test",
        ),
        (
            slug: "x",
            display_name: "X2",
            effects: [],
            duration: UntilGigEnd,
            addictive: false,
            price: Cheap,
            price_eb: (10),
            description: "dup test",
        ),
    ],
)
        "#;
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("cpr_drugs_dup_{}.ron", std::process::id()));
        std::fs::write(&tmp, dup).expect("must write temp file");
        let err = load_drugs_catalog(&tmp).expect_err("must reject duplicate slug");
        let _ = std::fs::remove_file(&tmp);
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("duplicate slug"),
                    "expected duplicate-slug error, got: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed, got: {other:?}"),
        }
    }

    /// Regression: the loader rejects a `price_eb` that disagrees with
    /// the tier's canonical cost on p.339.
    #[test]
    fn test_loader_rejects_price_mismatch() {
        let bad = r#"
DrugsFile(
    drugs: [
        (
            slug: "y",
            display_name: "Y",
            effects: [],
            duration: UntilGigEnd,
            addictive: false,
            price: Cheap,
            price_eb: (999),
            description: "price mismatch",
        ),
    ],
)
        "#;
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("cpr_drugs_pricebad_{}.ron", std::process::id()));
        std::fs::write(&tmp, bad).expect("must write temp file");
        let err = load_drugs_catalog(&tmp).expect_err("must reject price mismatch");
        let _ = std::fs::remove_file(&tmp);
        match err {
            RulesError::CatalogLoadFailed { source, .. } => {
                assert!(
                    source.contains("canonical cost"),
                    "expected price-mismatch error, got: {source}"
                );
            }
            other => panic!("expected CatalogLoadFailed, got: {other:?}"),
        }
    }
}
