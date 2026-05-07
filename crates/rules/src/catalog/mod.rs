//! Shared catalog infrastructure for Phase 2 data WPs.
//!
//! Every catalog (skills, weapons, armor, …) loads from a RON file at
//! startup into a [`Catalog<T>`] keyed by string slug. The slug doubles as
//! the in-content stable identifier — the loader for each catalog enforces
//! consistency between the slug and the entry's structured `id` field where
//! one exists (see [`skills::load_skills_catalog`]).
//!
//! See `IMPLEMENTATION_PLAN.md` §4 (Phase 2 — Data Catalogs) for the
//! shared shape every Phase 2 WP commits to.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod armor;
pub mod black_ice;
pub mod cover;
pub mod demons;
pub mod drugs;
pub mod programs;
pub mod skills;
pub mod vehicles;
pub mod weapons;

/// A read-only collection of catalog entries keyed by string slug.
///
/// Construction is via [`Catalog::new`] (typically called by a loader).
/// Lookups are by `&str` (no allocation needed). Iteration and length
/// queries are provided for tooling — `tools/content-validator` calls
/// [`Catalog::is_empty`] to flag empty catalog files in CI.
///
/// `Clone` and `serde` are derived so a catalog can be embedded inside
/// save files / over-the-wire messages if needed; the typical access
/// pattern is `&Catalog<T>` (immutable, shared).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Catalog<T> {
    entries: HashMap<String, T>,
}

impl<T> Catalog<T> {
    /// Build a `Catalog<T>` from an already-loaded entry map.
    ///
    /// Loaders own deduplication, slug-validation, and any other invariants
    /// before handing the map here. This constructor is intentionally
    /// trusting — there's no validation in `Catalog<T>` itself.
    pub fn new(entries: HashMap<String, T>) -> Self {
        Self { entries }
    }

    /// Look up an entry by its slug.
    ///
    /// Returns `None` for unknown slugs. Callers are responsible for
    /// converting `None` into the appropriate domain error
    /// (e.g. `RulesError::EntityNotFound` analogues to be added by
    /// downstream WPs).
    pub fn get(&self, id: &str) -> Option<&T> {
        self.entries.get(id)
    }

    /// Iterate `(&slug, &entry)` pairs in `HashMap`'s nondeterministic order.
    ///
    /// Order-sensitive callers (deterministic snapshot tests, replay) must
    /// sort the result themselves; the catalog itself promises nothing
    /// about iteration order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)> {
        self.entries.iter()
    }

    /// Number of entries in the catalog.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` iff this catalog has zero entries.
    ///
    /// `tools/content-validator` (Phase 0) and the Phase 2 exit gate both
    /// fail a build that produces an empty catalog.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_catalog_get_returns_inserted_value() {
        let mut entries = HashMap::new();
        entries.insert("foo".to_string(), 42i32);
        let cat: Catalog<i32> = Catalog::new(entries);
        assert_eq!(cat.get("foo"), Some(&42));
        assert_eq!(cat.get("missing"), None);
    }

    #[test]
    fn test_catalog_len_and_empty() {
        let cat: Catalog<i32> = Catalog::new(HashMap::new());
        assert!(cat.is_empty());
        assert_eq!(cat.len(), 0);

        let mut entries = HashMap::new();
        entries.insert("a".to_string(), 1);
        entries.insert("b".to_string(), 2);
        let cat = Catalog::new(entries);
        assert!(!cat.is_empty());
        assert_eq!(cat.len(), 2);
    }

    #[test]
    fn test_catalog_iter_visits_every_entry() {
        let mut entries = HashMap::new();
        entries.insert("a".to_string(), 1u8);
        entries.insert("b".to_string(), 2u8);
        entries.insert("c".to_string(), 3u8);
        let cat: Catalog<u8> = Catalog::new(entries);
        let mut seen: Vec<(&String, &u8)> = cat.iter().collect();
        seen.sort_by_key(|(k, _)| (*k).clone());
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0].0, "a");
        assert_eq!(*seen[0].1, 1);
        assert_eq!(seen[2].0, "c");
        assert_eq!(*seen[2].1, 3);
    }
}
