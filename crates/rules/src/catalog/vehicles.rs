//! Vehicle catalog (WP-211).
//!
//! Defines [`Vehicle`], [`VehicleId`], and [`VehicleKind`], plus the RON
//! loader [`load_vehicles_catalog`]. The catalog covers every vehicle for
//! which the *Cyberpunk RED* core rulebook publishes concrete game-mechanic
//! statistics.
//!
//! Rulebook references:
//! - **p.190** — the canonical Land/Sea/Air vehicle statblock table that
//!   provides SDP, Seats, Speed (Combat MOVE), Speed (Narrative), and Cost
//!   for every named vehicle. Every entry on this page is in the catalog.
//! - **pp.191–192** — vehicle combat rules. Vehicles have an SDP (which we
//!   surface as `hp`); the rulebook does **not** publish per-vehicle SP or
//!   Combat-Number values for these civilian vehicles, so the catalog
//!   stores `0` for both fields. They exist so future authored content
//!   (military vehicles, GM-defined NPC vehicles, etc.) can carry them
//!   without a breaking schema change.
//! - **pp.322–325** — "How You Get Around" — the prose chapter that
//!   describes the same vehicles in flavour terms (and adds a handful of
//!   "mega" vehicles like the OTEC Hammerhead Minisub, Delta 4 Spaceplane,
//!   Light Rail Lev Train, CINO RELaCS Cargo Sub, and K151 AeroZep) which
//!   carry only SDP / speed / crew callouts but no full p.190-style
//!   statblock. These are catalogued as [`VehicleKind::Other`] so the data
//!   is preserved without claiming a Combat-MOVE the book doesn't grant.
//!
//! The catalog file is `content/catalogs/vehicles.ron`; the loader expects
//! one entry per slug. Slugs are stable identifiers (not localised display
//! names) — see the file for the canonical list.

use crate::catalog::Catalog;
use crate::error::RulesError;
use crate::types::{Eurobucks, PriceTier};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// VehicleId
// ---------------------------------------------------------------------------

/// Stable string identifier for a vehicle catalog entry.
///
/// The wrapped `String` is the same value used as the slug key in the
/// `Catalog<Vehicle>` map, so a `VehicleId` round-trips with the
/// `Catalog::get` lookup key.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct VehicleId(pub String);

// ---------------------------------------------------------------------------
// VehicleKind
// ---------------------------------------------------------------------------

/// Category banner under which the rulebook prints the vehicle.
///
/// `Bike`, `Car`, and `Truck` partition the **Land Vehicles** table on
/// p.190; `AV` and `Boat` correspond to the **Air Vehicles** and
/// **Sea Vehicles** tables on p.190; `Other` covers the additional
/// stat-blocked but non-tabular vehicles described on pp.322–325 (subs,
/// spaceplane, lev train, aerozeps, etc.) which fit none of the named
/// kinds cleanly.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VehicleKind {
    /// Two-wheeled motor vehicles — Roadbike, Superbike (p.190 Land table).
    Bike,
    /// Four-wheeled passenger ground vehicles — Compact / High Performance /
    /// Super Groundcars (p.190 Land table).
    Car,
    /// Larger ground vehicles — primarily cargo / utility. Reserved for
    /// content WPs adding pickups, semi-trailers, etc.; the core rulebook's
    /// p.190 statblock contains no truck entry but the kind is part of the
    /// public API per the WP-211 spec.
    Truck,
    /// Air vehicles — Gyrocopter, Helicopter, AV-4, AV-9, Aerozep
    /// (p.190 Air table).
    AV,
    /// Sea vehicles — Jetski, Speedboat, Cabin Cruiser, Yacht
    /// (p.190 Sea table).
    Boat,
    /// Anything that doesn't fit the named kinds — submersibles
    /// (OTEC Hammerhead, CINO RELaCS), spaceplanes (Delta 4),
    /// lev trains, etc. described on pp.322–325.
    Other,
}

// ---------------------------------------------------------------------------
// Vehicle
// ---------------------------------------------------------------------------

/// A row in the canonical vehicle catalog (`content/catalogs/vehicles.ron`).
///
/// Loaded by [`load_vehicles_catalog`]. The `id` field carries the same
/// string the catalog map is keyed by, so a `Vehicle` is self-describing
/// once detached from the catalog.
///
/// Field origins (per p.190):
/// - `hp` ← SDP column.
/// - `top_speed_kph` ← the KPH half of the *Speed (Narrative)* column.
/// - `combat_number` ← MOVE half of *Speed (Combat)*. The book labels this
///   "MOVE" (a numeric MOVE STAT used in vehicle combat); we surface it via
///   the WP-211-mandated `combat_number` field name.
///
/// Fields without a p.190 source (`sp` for civilian vehicles where the
/// book publishes no SP) default to `0`. See module docs for rationale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Vehicle {
    /// Catalog lookup key, redundantly stored on the value for callers
    /// that have a `&Vehicle` and need its identifier without going back
    /// through the map.
    pub id: VehicleId,
    /// Display name as printed in the rulebook (p.190 / pp.322–325).
    pub display_name: String,
    /// Category banner (Land/Sea/Air bucket on p.190; `Other` for the
    /// pp.322–325 extras).
    pub kind: VehicleKind,
    /// Number of seats — the **Seats** column on p.190. `2 per Room` /
    /// `4 per Room` entries (Cabin Cruiser, Yacht, Aerozep) are flattened
    /// to the per-room number; multi-room totals are not modelled in this
    /// catalog.
    pub seats: u8,
    /// Top speed in kilometres per hour — the KPH half of the
    /// **Speed (Narrative)** column on p.190. For pp.322–325 entries that
    /// quote MPH only, the value is `mph * 8 / 5` rounded to the nearest
    /// integer.
    pub top_speed_kph: u16,
    /// Hit points — the **SDP** column on p.190 (Structural Damage Points,
    /// see also p.191).
    pub hp: u16,
    /// Stop Power — the rulebook publishes no per-vehicle SP for civilian
    /// vehicles; defaults to `0`. Reserved for future authored content
    /// (armoured vehicles, GM-defined statblocks) per p.191.
    pub sp: u8,
    /// MOVE STAT used in vehicle combat — the **Speed (Combat)** column on
    /// p.190 with the trailing word `MOVE` stripped (e.g. `20 MOVE` → 20).
    pub combat_number: u8,
    /// Canonical price tier per the *Cyberpunk RED* tier ladder — every
    /// vehicle on p.190 lists `(Super Luxury)`.
    pub price: PriceTier,
    /// Concrete eurobuck price as printed on p.190 / pp.322–325. The
    /// `PriceTier::SuperLuxury` canonical cost is `10,000eb`; vehicles
    /// list higher concrete prices (20,000eb to 100,000eb), so both the
    /// tier *and* the explicit value are stored.
    pub price_eb: Eurobucks,
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Schema for the on-disk RON file `content/catalogs/vehicles.ron`.
///
/// The file is a `(vehicles: [ ... ])` envelope where each entry is a
/// `VehiclesFileEntry` that carries an explicit `slug` plus the fields of
/// [`Vehicle`].
#[derive(Debug, Deserialize)]
struct VehiclesFile {
    vehicles: Vec<VehiclesFileEntry>,
}

/// One row in the on-disk vehicles catalog file. The `slug` is the lookup
/// key inside the resulting `Catalog<Vehicle>`; every other field
/// populates a [`Vehicle`] directly.
#[derive(Debug, Deserialize)]
struct VehiclesFileEntry {
    slug: String,
    id: VehicleId,
    display_name: String,
    kind: VehicleKind,
    seats: u8,
    top_speed_kph: u16,
    hp: u16,
    sp: u8,
    combat_number: u8,
    price: PriceTier,
    price_eb: Eurobucks,
}

/// Load the vehicles catalog from a RON file at `path`.
///
/// On success returns a [`Catalog<Vehicle>`] keyed by slug. On failure
/// returns [`RulesError::CatalogLoadFailed`] carrying the file path and a
/// stringified description of the underlying I/O or parse error.
///
/// The loader enforces three invariants:
/// 1. Slugs are unique within the file.
/// 2. Every entry's `id` matches its `slug` (the `VehicleId` is the slug,
///    so the two cannot diverge silently).
/// 3. `price_eb` is non-negative — vehicles cost money.
pub fn load_vehicles_catalog(path: &Path) -> Result<Catalog<Vehicle>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: VehiclesFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    let mut entries: HashMap<String, Vehicle> = HashMap::with_capacity(parsed.vehicles.len());
    for row in parsed.vehicles {
        if row.id.0 != row.slug {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "vehicle slug '{}' disagrees with id '{}'",
                    row.slug, row.id.0
                ),
            });
        }
        if row.price_eb.0 < 0 {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "vehicle '{}' has negative price_eb {}",
                    row.slug, row.price_eb.0
                ),
            });
        }
        let v = Vehicle {
            id: row.id,
            display_name: row.display_name,
            kind: row.kind,
            seats: row.seats,
            top_speed_kph: row.top_speed_kph,
            hp: row.hp,
            sp: row.sp,
            combat_number: row.combat_number,
            price: row.price,
            price_eb: row.price_eb,
        };
        if entries.insert(row.slug.clone(), v).is_some() {
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

    /// Workspace-relative path to the canonical vehicles catalog file.
    fn catalog_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("vehicles.ron");
        p
    }

    /// Acceptance: every vehicle the rulebook publishes a statblock for
    /// (p.190 + the named extras on pp.322–325) appears in the catalog.
    ///
    /// p.190 statblock entries (14 total):
    ///  * Land (5): Roadbike, Superbike, Compact Groundcar, High Performance
    ///    Groundcar, Super Groundcar
    ///  * Sea (4): Jetski, Speedboat, Cabin Cruiser, Yacht
    ///  * Air (5): Gyrocopter, Helicopter, AV-4 Multipurpose Aerodyne,
    ///    AV-9 Super Aerodyne, Aerozep
    ///
    /// pp.322–325 named extras with concrete SDP / speed (5 total):
    ///  * OTEC Hammerhead Multipurpose Minisub (p.323)
    ///  * Delta 4 Spaceplane (p.324)
    ///  * Light Rail Lev Train (p.325)
    ///  * CINO RELaCS Cargo Sub (p.325)
    ///  * K151 AeroZep (p.325)
    ///
    /// Total: 14 + 5 = **19**.
    #[test]
    fn test_vehicle_catalog_complete() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(
            cat.len(),
            19,
            "expected 19 vehicles per p.190 + pp.322-325 (got {})",
            cat.len()
        );

        // Every p.190 statblock vehicle is present by slug.
        for slug in [
            // Land
            "roadbike",
            "superbike",
            "compact_groundcar",
            "high_performance_groundcar",
            "super_groundcar",
            // Sea
            "jetski",
            "speedboat",
            "cabin_cruiser",
            "yacht",
            // Air
            "gyrocopter",
            "helicopter",
            "av4_multipurpose_aerodyne",
            "av9_super_aerodyne",
            "aerozep",
            // pp.322-325 named extras
            "otec_hammerhead_minisub",
            "delta4_spaceplane",
            "light_rail_lev_train",
            "cino_relacs_cargo_sub",
            "k151_aerozep",
        ] {
            assert!(
                cat.get(slug).is_some(),
                "missing required vehicle slug: {slug}"
            );
        }
    }

    /// Acceptance: a Roadbike's stats match p.190 exactly.
    ///
    /// p.190 Land Vehicles, Roadbike row:
    ///   SDP 35, Seats 2, Speed (Combat) 20 MOVE,
    ///   Speed (Narrative) 100 MPH / 161 KPH, Cost 20,000eb (Super Luxury).
    #[test]
    fn test_motorcycle_stats() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        let bike = cat.get("roadbike").expect("roadbike must be present");
        assert_eq!(bike.display_name, "Roadbike");
        assert_eq!(bike.kind, VehicleKind::Bike);
        assert_eq!(bike.hp, 35, "Roadbike SDP per p.190");
        assert_eq!(bike.seats, 2, "Roadbike Seats per p.190");
        assert_eq!(bike.combat_number, 20, "Roadbike Speed (Combat) per p.190");
        assert_eq!(
            bike.top_speed_kph, 161,
            "Roadbike Speed (Narrative) KPH per p.190"
        );
        assert_eq!(
            bike.price,
            PriceTier::SuperLuxury,
            "Roadbike tier per p.190"
        );
        assert_eq!(
            bike.price_eb,
            Eurobucks(20_000),
            "Roadbike concrete price per p.190"
        );
    }

    /// Acceptance: every catalog entry round-trips through RON without
    /// loss. Catches accidental schema drift between the on-disk format
    /// and the in-memory `Vehicle` struct.
    #[test]
    fn test_vehicle_round_trip_ron() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        for (slug, original) in cat.iter() {
            let serialised = ron::ser::to_string(original)
                .unwrap_or_else(|e| panic!("serialise '{slug}' failed: {e}"));
            let restored: Vehicle = ron::de::from_str(&serialised)
                .unwrap_or_else(|e| panic!("deserialise '{slug}' failed: {e}"));
            assert_eq!(
                &restored, original,
                "round-trip mismatch for vehicle '{slug}'"
            );
        }
    }

    /// Regression: every entry's `id` agrees with its catalog slug. The
    /// loader checks this at parse time; the test pins the invariant so
    /// a loader-bypass refactor can't slip past.
    #[test]
    fn test_vehicle_id_matches_slug() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        for (slug, v) in cat.iter() {
            assert_eq!(
                &v.id.0, slug,
                "vehicle id '{}' disagrees with catalog slug '{}'",
                v.id.0, slug
            );
        }
    }

    /// Spot-check a representative entry from each `VehicleKind` so a
    /// kind-renaming refactor immediately fails at the catalog boundary.
    #[test]
    fn test_kind_distribution() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        assert_eq!(cat.get("superbike").unwrap().kind, VehicleKind::Bike);
        assert_eq!(cat.get("compact_groundcar").unwrap().kind, VehicleKind::Car);
        assert_eq!(cat.get("yacht").unwrap().kind, VehicleKind::Boat);
        assert_eq!(cat.get("helicopter").unwrap().kind, VehicleKind::AV);
        assert_eq!(
            cat.get("delta4_spaceplane").unwrap().kind,
            VehicleKind::Other
        );
    }

    /// Spot-check the AV-9 Super Aerodyne — top of the line vehicle and
    /// the most expensive air vehicle in the table on p.190.
    ///
    /// p.190 Air Vehicles, AV-9 row:
    ///   SDP 60, Seats 2, Speed (Combat) 60 MOVE,
    ///   Speed (Narrative) 300 MPH / 483 KPH, Cost 100,000eb (Super Luxury).
    #[test]
    fn test_av9_super_aerodyne_stats() {
        let cat = load_vehicles_catalog(&catalog_path()).expect("catalog must load");
        let av = cat.get("av9_super_aerodyne").expect("AV-9 must be present");
        assert_eq!(av.kind, VehicleKind::AV);
        assert_eq!(av.hp, 60);
        assert_eq!(av.seats, 2);
        assert_eq!(av.combat_number, 60);
        assert_eq!(av.top_speed_kph, 483);
        assert_eq!(av.price_eb, Eurobucks(100_000));
        assert_eq!(av.price, PriceTier::SuperLuxury);
    }
}
