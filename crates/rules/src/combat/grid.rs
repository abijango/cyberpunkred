//! Grid placeholder — WP-302 will replace this unit struct with the full
//! 2D-grid implementation (tile map, occupancy, LOS, pathfinding).
//!
//! **Do not add logic here.** This file exists solely so [`super::CombatState`]
//! compiles with a `grid: Grid` field before WP-302 lands. Once WP-302 is
//! merged, the `pub struct Grid;` here will be replaced by the real type
//! defined in WP-302's `crates/rules/src/combat/grid.rs`.
//!
//! See `IMPLEMENTATION_PLAN.md` §4 WP-302 for the full public API contract.

use serde::{Deserialize, Serialize};

/// Placeholder grid. Replaced entirely by WP-302.
///
/// WP-302 will expand this into:
/// ```ignore
/// pub struct Grid {
///     pub width: u16,
///     pub height: u16,
///     pub tiles: Vec<TileKind>,
///     pub occupants: HashMap<(u16, u16), EntityId>,
///     pub cover_objects: HashMap<(u16, u16), CoverInstance>,
/// }
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid;
