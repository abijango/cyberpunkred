//! 2D combat grid — tile map, entity occupancy, line of sight, and movement.
//!
//! Cyberpunk RED uses a square grid where each cell represents 2 metres/yards
//! (p.126: "each 1-inch square corresponds to 2 meters/yards"). Movement on the
//! grid is measured in whole squares; diagonals cost the same as orthogonal
//! moves (p.126, p.169: "a number of squares equal to their MOVE, which can
//! include moving diagonally"). Characters "cannot stop in between the squares"
//! (p.169).
//!
//! Line of sight uses the Golden Rules of Cover (p.183): you are in cover if
//! fully behind something that could stop a bullet; if they have line of sight
//! on you, you are not in cover.
//!
//! **Rulebook:** pp.126–127 (distance and movement), pp.168–169 (combat grid,
//! move action), p.183 (cover, line of sight).

use crate::movement::METERS_PER_SQUARE;
use crate::types::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::fmt;

// ── TileKind ─────────────────────────────────────────────────────────────────

/// The kind of terrain in a grid cell.
///
/// Used to determine whether movement or line of sight is blocked.
/// See pp.168–169 for the combat grid rules; cover mechanics are on p.182–183.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TileKind {
    /// Passable, clear terrain. Neither movement nor LOS is impeded.
    Open,
    /// Impassable solid obstruction. Blocks movement and LOS completely.
    Wall,
    /// Difficult terrain — passable but costs extra effort narratively.
    /// In RAW Cyberpunk RED difficult terrain is not given a mechanical square
    /// cost on the grid, so we treat it as passable at normal cost (1 square).
    /// Flag is preserved for future GM houserule hooks.
    Difficult,
    /// Water terrain — narratively slow but mechanically treated as passable
    /// at normal cost on the grid. Preserved for future houserule hooks.
    Water,
}

// ── CoverInstance ─────────────────────────────────────────────────────────────

/// A piece of cover placed on a specific grid square.
///
/// The `material` string is a slug from the cover catalog (WP-207,
/// `content/catalogs/cover.ron`). HP tracks the remaining structural integrity;
/// at 0 HP the cover is destroyed and no longer provides protection (p.183).
///
/// Each cover object occupies exactly one 2 m × 2 m square (p.183: "a 2 m/yds
/// by 2 m/yds (1 square) section of it can be attacked").
///
/// See pp.182–183.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverInstance {
    /// Cover catalog slug, e.g. `"concrete_barricade"` or `"car_door"`.
    /// Corresponds to a [`crate::catalog::cover::CoverMaterial`] id.
    /// See p.183.
    pub material: String,
    /// Current hit points remaining. Cover is destroyed when this reaches 0.
    /// See p.183.
    pub current_hp: u16,
    /// Maximum hit points (from the cover material's HP per p.183).
    pub max_hp: u16,
}

// ── Grid ─────────────────────────────────────────────────────────────────────

/// 2D square combat grid.
///
/// The grid is `width × height` cells stored in **row-major** order:
/// `tiles[y * width + x]`. Coordinates are `(x, y)` with `(0, 0)` at the
/// top-left. `x` increases rightward, `y` increases downward.
///
/// Each cell is 2 metres/yards across (p.126). `width` and `height` are the
/// number of squares; the physical dimensions are `width × 2` metres and
/// `height × 2` metres.
///
/// ## Occupancy
///
/// At most one entity may occupy a given square at a time. The `occupants`
/// map stores which entity is in which square. An entity is either in the
/// `occupants` map (on the grid) or it is off-grid.
///
/// ## Cover
///
/// Cover objects are placed on specific squares via `cover_objects`. A covered
/// square that still has `current_hp > 0` causes line-of-sight rays through it
/// to return [`LosResult::ThroughCover`]. See p.183.
///
/// See pp.126–127, pp.168–169, pp.182–183.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grid {
    /// Number of columns (squares along the X axis).
    pub width: u16,
    /// Number of rows (squares along the Y axis).
    pub height: u16,
    /// Tile terrain, stored in row-major order: `tiles[y * width + x]`.
    pub tiles: Vec<TileKind>,
    /// Entities currently on the grid, keyed by their `(x, y)` position.
    pub occupants: HashMap<(u16, u16), EntityId>,
    /// Cover objects placed on the grid, keyed by their `(x, y)` position.
    pub cover_objects: HashMap<(u16, u16), CoverInstance>,
}

impl Grid {
    /// Create a new empty grid of `width × height` open squares.
    ///
    /// All tiles are initialised to [`TileKind::Open`]. `occupants` and
    /// `cover_objects` are empty.
    pub fn new(width: u16, height: u16) -> Self {
        let n = usize::from(width) * usize::from(height);
        Self {
            width,
            height,
            tiles: vec![TileKind::Open; n],
            occupants: HashMap::new(),
            cover_objects: HashMap::new(),
        }
    }

    // ── Tile access ───────────────────────────────────────────────────────────

    /// Return the [`TileKind`] at `(x, y)`, or `None` if out of bounds.
    pub fn tile_at(&self, pos: (u16, u16)) -> Option<TileKind> {
        let idx = self.tile_index(pos)?;
        Some(self.tiles[idx])
    }

    /// Set the [`TileKind`] at `(x, y)`.
    ///
    /// Does nothing if `pos` is out of bounds.
    pub fn set_tile(&mut self, pos: (u16, u16), kind: TileKind) {
        if let Some(idx) = self.tile_index(pos) {
            self.tiles[idx] = kind;
        }
    }

    /// Compute the flat `tiles` index for `(x, y)`, returning `None` if out
    /// of bounds.
    fn tile_index(&self, (x, y): (u16, u16)) -> Option<usize> {
        if x < self.width && y < self.height {
            Some(usize::from(y) * usize::from(self.width) + usize::from(x))
        } else {
            None
        }
    }

    // ── Entity placement ──────────────────────────────────────────────────────

    /// Place `entity` at `pos`, evicting any previous occupant at that square
    /// and removing the entity from its previous position (if any).
    ///
    /// This is an unconditional placement used for setup. Use
    /// [`Grid::move_entity`] for validated movement during combat.
    pub fn place(&mut self, entity: EntityId, pos: (u16, u16)) {
        // Remove from old position.
        self.occupants.retain(|_, v| *v != entity);
        // Insert at new position (overwrites any previous occupant).
        self.occupants.insert(pos, entity);
    }

    /// Return the current `(x, y)` position of `entity`, or `None` if the
    /// entity is not on the grid.
    pub fn position_of(&self, entity: EntityId) -> Option<(u16, u16)> {
        self.occupants
            .iter()
            .find_map(|(&pos, &e)| if e == entity { Some(pos) } else { None })
    }

    // ── Distance ──────────────────────────────────────────────────────────────

    /// Chebyshev distance between two squares, in squares.
    ///
    /// Diagonals cost 1 — per p.169 a character's Move Action covers "a number
    /// of squares equal to their MOVE, which can include moving diagonally".
    /// The Chebyshev metric (`max(|dx|, |dy|)`) implements this exactly.
    ///
    /// See p.169.
    pub fn distance_squares(&self, a: (u16, u16), b: (u16, u16)) -> u16 {
        let dx = a.0.abs_diff(b.0);
        let dy = a.1.abs_diff(b.1);
        dx.max(dy)
    }

    /// Distance between two squares in metres.
    ///
    /// Equals `distance_squares(a, b) × METERS_PER_SQUARE`. One square is
    /// 2 metres (p.126).
    ///
    /// See pp.126–127.
    pub fn distance_meters(&self, a: (u16, u16), b: (u16, u16)) -> u16 {
        self.distance_squares(a, b) * METERS_PER_SQUARE
    }

    // ── Line of sight ─────────────────────────────────────────────────────────

    /// Test line of sight between two squares using Bresenham's line algorithm.
    ///
    /// Walks all squares along the straight line from `from` to `to`
    /// (excluding the origin square, including the target square). Rules:
    ///
    /// - If any intermediate square has [`TileKind::Wall`], returns
    ///   [`LosResult::Blocked`].
    /// - If any square on the path (excluding `from`, including `to`) has a
    ///   [`CoverInstance`] with `current_hp > 0`, returns
    ///   [`LosResult::ThroughCover`] with the first such cover instance.
    /// - Otherwise returns [`LosResult::Clear`].
    ///
    /// The target square itself is checked for cover — shooting *through* the
    /// cover the defender is hiding behind is the main use-case (p.183).
    ///
    /// See pp.182–183 (The Golden Rules of Cover, Cover Hit Points).
    pub fn line_of_sight(&self, from: (u16, u16), to: (u16, u16)) -> LosResult {
        let cells = bresenham_line(from, to);

        // Skip the origin cell; check all others.
        let mut found_cover: Option<CoverInstance> = None;

        for pos in cells.into_iter().skip(1) {
            // Wall check first — walls block completely.
            if let Some(TileKind::Wall) = self.tile_at(pos) {
                return LosResult::Blocked;
            }

            // Cover check — record the first live cover instance encountered.
            if found_cover.is_none() {
                if let Some(cover) = self.cover_objects.get(&pos) {
                    if cover.current_hp > 0 {
                        found_cover = Some(cover.clone());
                    }
                }
            }
        }

        match found_cover {
            Some(cover) => LosResult::ThroughCover(cover),
            None => LosResult::Clear,
        }
    }

    // ── Movement ─────────────────────────────────────────────────────────────

    /// All squares reachable from `from` within `squares_budget` steps.
    ///
    /// Uses BFS over the 8-directional neighbourhood (including diagonals —
    /// see p.126, p.169). Each step costs 1 square regardless of direction.
    ///
    /// **Stopping rules** (p.169: "you cannot stop in between the squares"):
    /// - Wall squares are impassable; BFS does not enter them.
    /// - Squares occupied by another entity are impassable stopping points;
    ///   BFS does not add them to the result.
    /// - The origin square `from` is not included in the returned set.
    ///
    /// Returns a `Vec` of valid stopping positions in unspecified order.
    ///
    /// See pp.126–127, p.169.
    pub fn movement_options(&self, from: (u16, u16), squares_budget: u16) -> Vec<(u16, u16)> {
        // cost[pos] = cheapest number of steps to reach pos.
        let mut cost: HashMap<(u16, u16), u16> = HashMap::new();
        cost.insert(from, 0);

        let mut queue: VecDeque<(u16, u16)> = VecDeque::new();
        queue.push_back(from);

        let mut reachable: Vec<(u16, u16)> = Vec::new();

        while let Some(pos) = queue.pop_front() {
            let current_cost = cost[&pos];
            if current_cost >= squares_budget {
                continue;
            }

            for neighbour in eight_neighbours(pos, self.width, self.height) {
                // Walls are impassable.
                if self.tile_at(neighbour) == Some(TileKind::Wall) {
                    continue;
                }

                let new_cost = current_cost + 1;
                if new_cost > squares_budget {
                    continue;
                }

                if let std::collections::hash_map::Entry::Vacant(e) = cost.entry(neighbour) {
                    e.insert(new_cost);

                    // Only add to reachable if not occupied by another entity.
                    // The origin entity may be at `from`; treat neighbour as
                    // valid as long as no *other* entity is there.
                    let occupied_by_other = self
                        .occupants
                        .get(&neighbour)
                        .map(|&e| !self.position_of_origin_matches(from, e))
                        .unwrap_or(false);

                    if !occupied_by_other {
                        reachable.push(neighbour);
                        queue.push_back(neighbour);
                    }
                }
            }
        }

        reachable
    }

    /// Helper: return `true` if the entity at `from` is the same entity as `e`.
    ///
    /// This is used by `movement_options` to distinguish the moving entity
    /// (which may occupy `from`) from foreign occupants at reachable squares.
    fn position_of_origin_matches(&self, from: (u16, u16), e: EntityId) -> bool {
        self.occupants.get(&from) == Some(&e)
    }

    /// Validate and execute a movement path for `entity`.
    ///
    /// The `path` must be a sequence of squares starting at the entity's
    /// current position. Each consecutive pair must be a single-square move
    /// (Chebyshev distance = 1). No intermediate square (other than the
    /// origin) may be a Wall or occupied by another entity. The final square
    /// must not be occupied by another entity.
    ///
    /// On success: removes the entity from its original position and places it
    /// at the last square in `path`.
    ///
    /// On failure: the grid state is unchanged and an appropriate
    /// [`GridError`] is returned.
    ///
    /// See pp.126–127, p.169.
    pub fn move_entity(&mut self, entity: EntityId, path: &[(u16, u16)]) -> Result<(), GridError> {
        if path.is_empty() {
            return Ok(());
        }

        // Validate each step.
        for window in path.windows(2) {
            let a = window[0];
            let b = window[1];

            // Each step must be exactly 1 Chebyshev square.
            if self.distance_squares(a, b) != 1 {
                return Err(GridError::InvalidPath);
            }

            // The destination square of each step must not be a Wall.
            match self.tile_at(b) {
                None => return Err(GridError::OutOfBounds),
                Some(TileKind::Wall) => return Err(GridError::BlockedByWall),
                _ => {}
            }

            // The destination square must not be occupied by another entity
            // (the moving entity might still be at path[0] in occupants).
            if let Some(&occupant) = self.occupants.get(&b) {
                if occupant != entity {
                    return Err(GridError::Occupied);
                }
            }
        }

        let destination = *path.last().unwrap();

        // Remove from old position.
        self.occupants.retain(|_, v| *v != entity);
        // Place at new position.
        self.occupants.insert(destination, entity);

        Ok(())
    }

    // ── Area queries ─────────────────────────────────────────────────────────

    /// All entities within a 90° cone from `from` in `facing` direction, up
    /// to `max_meters` metres away.
    ///
    /// The cone extends `max_meters / METERS_PER_SQUARE` squares. Each entity
    /// on the grid is included if:
    /// 1. Its Chebyshev distance from `from` is ≤ the square budget.
    /// 2. The angle from `from` to the entity's square is within ±45° of the
    ///    facing direction's canonical angle.
    ///
    /// Entities at `from` itself are excluded (the attacker).
    ///
    /// **Implementation note:** angle comparison uses `f64` `atan2`. The
    /// ±45° boundary is inclusive: entities at exactly ±45° from the facing
    /// direction (i.e. on the cone edge — e.g. NE for a North-facing cone)
    /// are included. Documented in the PR for review.
    ///
    /// See pp.168–169 (movement), p.183 (cone weapons implied by cover rules).
    pub fn cone_targets(&self, from: (u16, u16), facing: Facing, max_meters: u16) -> Vec<EntityId> {
        let square_budget = max_meters / METERS_PER_SQUARE;
        let facing_angle = facing.angle_radians();

        self.occupants
            .iter()
            .filter_map(|(&pos, &entity)| {
                if pos == from {
                    return None;
                }

                // Distance check (Chebyshev).
                if self.distance_squares(from, pos) > square_budget {
                    return None;
                }

                // Angle check: must be within ±45° (π/4 radians) of facing.
                let dx = pos.0 as f64 - from.0 as f64;
                let dy = pos.1 as f64 - from.1 as f64;
                // atan2 in standard math coords: positive Y is up. On our
                // grid Y increases downward, so South is positive-dy.
                // We negate dy so that grid-South maps to math-angle -π/2,
                // which matches Facing::S.angle_radians() = -π/2.
                let angle = f64::atan2(-dy, dx);

                let diff = angle_diff(angle, facing_angle);
                // ≤ FRAC_PI_4: the 90° cone is half-open at exactly ±45°
                // (inclusive boundary). An entity at exactly NE of a North-
                // facing attacker is on the cone edge and is included.
                if diff <= std::f64::consts::FRAC_PI_4 {
                    Some(entity)
                } else {
                    None
                }
            })
            .collect()
    }

    /// All entities within a square area of effect centred on `center`.
    ///
    /// The area spans `[center.x - radius .. center.x + radius]` (inclusive)
    /// in both axes, giving a `(2*radius + 1) × (2*radius + 1)` square region.
    /// For the standard explosive 5×5 AoE (p.174: "10m × 10m area"), use
    /// `radius_squares = 2` (which yields a 5×5 = 25-square region).
    ///
    /// Squares outside the grid bounds are silently skipped.
    ///
    /// See p.174.
    pub fn square_aoe_targets(&self, center: (u16, u16), radius_squares: u16) -> Vec<EntityId> {
        let mut targets = Vec::new();

        let x_min = center.0.saturating_sub(radius_squares);
        let y_min = center.1.saturating_sub(radius_squares);
        let x_max = center.0.saturating_add(radius_squares).min(self.width - 1);
        let y_max = center.1.saturating_add(radius_squares).min(self.height - 1);

        for y in y_min..=y_max {
            for x in x_min..=x_max {
                if let Some(&entity) = self.occupants.get(&(x, y)) {
                    targets.push(entity);
                }
            }
        }

        targets
    }
}

// ── LosResult ────────────────────────────────────────────────────────────────

/// Result of a line-of-sight query between two squares.
///
/// See pp.182–183 (The Golden Rules of Cover).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LosResult {
    /// The line of sight is unobstructed.
    Clear,
    /// The line of sight is completely blocked by a [`TileKind::Wall`].
    Blocked,
    /// The line passes through a piece of live cover (HP > 0). Contains the
    /// first [`CoverInstance`] encountered along the ray.
    ///
    /// See p.183.
    ThroughCover(CoverInstance),
}

// ── Facing ───────────────────────────────────────────────────────────────────

/// Cardinal and diagonal directions, used for cone-attack orientation.
///
/// Matches the eight compass bearings. North is grid-up (decreasing Y).
///
/// See pp.168–169.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Facing {
    /// North — grid-up, decreasing Y.
    N,
    /// North-east.
    NE,
    /// East — increasing X.
    E,
    /// South-east.
    SE,
    /// South — grid-down, increasing Y.
    S,
    /// South-west.
    SW,
    /// West — decreasing X.
    W,
    /// North-west.
    NW,
}

impl Facing {
    /// Canonical angle in radians for this facing direction.
    ///
    /// Uses standard mathematical convention: positive X is East (0 rad),
    /// positive Y is North (π/2 rad). On the grid, North = decreasing Y,
    /// which maps to +π/2 after the Y-negation applied in [`Grid::cone_targets`].
    fn angle_radians(self) -> f64 {
        use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
        match self {
            Facing::E => 0.0,
            Facing::NE => FRAC_PI_4,
            Facing::N => FRAC_PI_2,
            Facing::NW => 3.0 * FRAC_PI_4,
            Facing::W => PI,
            Facing::SW => -(3.0 * FRAC_PI_4),
            Facing::S => -FRAC_PI_2,
            Facing::SE => -FRAC_PI_4,
        }
    }
}

// ── GridError ────────────────────────────────────────────────────────────────

/// Errors that can occur during grid operations.
///
/// Returned by [`Grid::move_entity`] and other validated grid mutations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GridError {
    /// The target position is outside the grid bounds.
    OutOfBounds,
    /// The target square is already occupied by another entity.
    Occupied,
    /// The path passes through or ends on a [`TileKind::Wall`] square.
    BlockedByWall,
    /// The path is invalid (non-adjacent steps, empty intermediate squares
    /// that violate the single-square-per-step rule).
    InvalidPath,
}

impl fmt::Display for GridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GridError::OutOfBounds => write!(f, "position is out of grid bounds"),
            GridError::Occupied => write!(f, "target square is occupied by another entity"),
            GridError::BlockedByWall => write!(f, "path is blocked by a wall"),
            GridError::InvalidPath => {
                write!(f, "path is invalid: steps must be exactly 1 square apart")
            }
        }
    }
}

impl std::error::Error for GridError {}

// ── Private helpers ───────────────────────────────────────────────────────────

/// Return all grid squares along the Bresenham line from `from` to `to`,
/// inclusive of both endpoints.
///
/// Uses the standard integer Bresenham algorithm. The returned `Vec` always
/// starts with `from` and ends with `to`.
fn bresenham_line(from: (u16, u16), to: (u16, u16)) -> Vec<(u16, u16)> {
    let mut cells = Vec::new();

    let mut x0 = from.0 as i32;
    let mut y0 = from.1 as i32;
    let x1 = to.0 as i32;
    let y1 = to.1 as i32;

    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx: i32 = if x0 < x1 { 1 } else { -1 };
    let sy: i32 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    loop {
        cells.push((x0 as u16, y0 as u16));
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }

    cells
}

/// Return all in-bounds 8-directional neighbours of `pos` within a grid of
/// `width × height`.
fn eight_neighbours(pos: (u16, u16), width: u16, height: u16) -> Vec<(u16, u16)> {
    let mut result = Vec::with_capacity(8);
    let (x, y) = (pos.0 as i32, pos.1 as i32);
    for dy in -1i32..=1 {
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            let nx = x + dx;
            let ny = y + dy;
            if nx >= 0 && ny >= 0 && nx < width as i32 && ny < height as i32 {
                result.push((nx as u16, ny as u16));
            }
        }
    }
    result
}

/// Absolute angular difference between two angles (in radians), normalised
/// to `[0, π]`.
fn angle_diff(a: f64, b: f64) -> f64 {
    use std::f64::consts::PI;
    let mut diff = (a - b).abs();
    // Wrap into [0, 2π].
    diff %= 2.0 * PI;
    // Fold into [0, π].
    if diff > PI {
        diff = 2.0 * PI - diff;
    }
    diff
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn eid(n: u128) -> EntityId {
        EntityId(Uuid::from_u128(n))
    }

    /// Helper: build a 10×10 open grid.
    fn open_grid() -> Grid {
        Grid::new(10, 10)
    }

    // ── Acceptance tests ──────────────────────────────────────────────────────

    /// `test_diagonal_costs_one` — Chebyshev diagonal distance is 1.
    ///
    /// Per p.169: moving diagonally costs the same as moving orthogonally —
    /// "a number of squares equal to their MOVE, which can include moving
    /// diagonally".
    #[test]
    fn test_diagonal_costs_one() {
        let g = open_grid();
        assert_eq!(
            g.distance_squares((0, 0), (1, 1)),
            1,
            "diagonal must cost 1 square (p.169)"
        );
    }

    /// `test_meters_between` — 3 squares at 2 m/square = 6 m. See p.126.
    #[test]
    fn test_meters_between() {
        let g = open_grid();
        assert_eq!(
            g.distance_meters((0, 0), (3, 0)),
            6,
            "3 squares × 2 m/square = 6 m (p.126)"
        );
    }

    /// `test_movement_options_with_budget` — a budget of 4 squares yields
    /// reachable squares in all clear directions.
    ///
    /// On an open 10×10 grid with a single entity at (5,5), budget=4, we
    /// expect all squares within Chebyshev distance 4 to be reachable (minus
    /// the origin itself). That's (2×4+1)² - 1 = 80 squares, all in-bounds.
    #[test]
    fn test_movement_options_with_budget() {
        let mut g = open_grid();
        let e = eid(1);
        g.place(e, (5, 5));

        let opts = g.movement_options((5, 5), 4);

        // Minimum: must have options in all 8 directions.
        assert!(
            !opts.is_empty(),
            "must return reachable squares with budget 4"
        );

        // No result square should exceed Chebyshev distance 4.
        for pos in &opts {
            let dist = g.distance_squares((5, 5), *pos);
            assert!(
                dist <= 4,
                "movement_options returned ({},{}) which is {} squares away — exceeds budget 4",
                pos.0,
                pos.1,
                dist
            );
        }

        // Must include squares at Chebyshev distance 1 in all 8 directions.
        for &expect in &[
            (5u16, 4u16),
            (5, 6),
            (4, 5),
            (6, 5),
            (4, 4),
            (6, 6),
            (4, 6),
            (6, 4),
        ] {
            assert!(
                opts.contains(&expect),
                "expected ({},{}) in movement_options",
                expect.0,
                expect.1
            );
        }
    }

    /// `test_los_blocked_by_wall` — a wall between two entities blocks LOS.
    ///
    /// Per p.183: "If they have line of sight on you, you aren't in cover."
    /// Conversely, a wall (impassable solid) fully blocks the ray.
    #[test]
    fn test_los_blocked_by_wall() {
        let mut g = open_grid();
        // Place a wall column at x=5.
        for y in 0..10 {
            g.set_tile((5, y), TileKind::Wall);
        }

        // LOS from (2,5) to (8,5) must cross the wall at x=5.
        assert_eq!(
            g.line_of_sight((2, 5), (8, 5)),
            LosResult::Blocked,
            "wall at x=5 must block LOS between (2,5) and (8,5)"
        );
    }

    /// `test_los_through_cover` — cover with HP > 0 along the way produces
    /// `ThroughCover`.
    ///
    /// See p.183.
    #[test]
    fn test_los_through_cover() {
        let mut g = open_grid();
        let cover = CoverInstance {
            material: "concrete_barricade".to_string(),
            current_hp: 25,
            max_hp: 25,
        };
        g.cover_objects.insert((5, 5), cover.clone());

        let result = g.line_of_sight((3, 5), (7, 5));
        assert_eq!(
            result,
            LosResult::ThroughCover(cover),
            "live cover at (5,5) must produce ThroughCover"
        );
    }

    /// `test_cone_targets_directional` — facing North, an entity due south is
    /// NOT in the cone.
    ///
    /// A North-facing cone spans roughly ±45° around straight up (decreasing
    /// Y). An entity to the south is at 180° from north, well outside the arc.
    #[test]
    fn test_cone_targets_directional() {
        let mut g = open_grid();
        let attacker = eid(1);
        let target_south = eid(2);
        g.place(attacker, (5, 5));
        g.place(target_south, (5, 8)); // due south of (5,5)

        let targets = g.cone_targets((5, 5), Facing::N, 8);
        assert!(
            !targets.contains(&target_south),
            "entity due south must NOT be in a North-facing cone"
        );
    }

    /// `test_cone_targets_within_arc` — facing North, an entity NE is in the
    /// cone.
    #[test]
    fn test_cone_targets_within_arc() {
        let mut g = open_grid();
        let attacker = eid(1);
        let target_ne = eid(2);
        g.place(attacker, (5, 5));
        g.place(target_ne, (6, 4)); // NE of (5,5): dx=+1, dy=-1 → angle NE

        let targets = g.cone_targets((5, 5), Facing::N, 8);
        assert!(
            targets.contains(&target_ne),
            "entity NE must be in a North-facing cone"
        );
    }

    /// `test_square_aoe_5x5` — radius_squares=2 produces a 5×5 area.
    ///
    /// Per p.174: explosives use a 10m × 10m (5-square × 5-square) AoE.
    /// With radius=2, the region is [center±2] which is 5 squares wide/tall.
    /// Placing one entity in every square and querying should return 25 entities.
    #[test]
    fn test_square_aoe_5x5() {
        // Need at least a 5×5 grid centred; use 10×10.
        let mut g = open_grid();
        let center = (5u16, 5u16);

        // Populate the 5×5 area (radius=2): x in [3,7], y in [3,7].
        let mut expected: Vec<EntityId> = Vec::new();
        let mut id_counter: u128 = 1;
        for y in 3u16..=7 {
            for x in 3u16..=7 {
                let e = eid(id_counter);
                id_counter += 1;
                g.place(e, (x, y));
                expected.push(e);
            }
        }

        let targets = g.square_aoe_targets(center, 2);

        assert_eq!(
            targets.len(),
            25,
            "5×5 AoE with one entity per square must return 25 entities (p.174)"
        );

        // Every placed entity must appear in the result.
        for e in &expected {
            assert!(
                targets.contains(e),
                "entity {:?} not found in AoE targets",
                e
            );
        }
    }

    /// `test_move_entity_valid_path` — moving along a valid path updates
    /// `occupants`.
    #[test]
    fn test_move_entity_valid_path() {
        let mut g = open_grid();
        let e = eid(1);
        g.place(e, (0, 0));

        let path = vec![(0, 0), (1, 0), (2, 0), (3, 0)];
        g.move_entity(e, &path).expect("valid path must succeed");

        assert_eq!(
            g.position_of(e),
            Some((3, 0)),
            "entity must be at end of path after move"
        );
        // Must not still be at the origin.
        assert!(
            !g.occupants.contains_key(&(0, 0)),
            "entity must no longer occupy origin after move"
        );
    }

    /// `test_move_entity_blocked_by_wall` — a wall in the path returns
    /// `Err(GridError::BlockedByWall)`.
    #[test]
    fn test_move_entity_blocked_by_wall() {
        let mut g = open_grid();
        let e = eid(1);
        g.place(e, (0, 0));
        g.set_tile((2, 0), TileKind::Wall);

        let path = vec![(0, 0), (1, 0), (2, 0)];
        let result = g.move_entity(e, &path);

        assert_eq!(
            result,
            Err(GridError::BlockedByWall),
            "path through wall must return BlockedByWall"
        );
        // Entity must remain at origin.
        assert_eq!(g.position_of(e), Some((0, 0)));
    }

    // ── Additional coverage tests ─────────────────────────────────────────────

    /// `distance_squares` for same-square is 0.
    #[test]
    fn test_distance_same_square() {
        let g = open_grid();
        assert_eq!(g.distance_squares((3, 3), (3, 3)), 0);
        assert_eq!(g.distance_meters((3, 3), (3, 3)), 0);
    }

    /// `distance_squares` for a cardinal direction is the manhattan component.
    #[test]
    fn test_distance_cardinal() {
        let g = open_grid();
        assert_eq!(g.distance_squares((0, 0), (5, 0)), 5);
        assert_eq!(g.distance_squares((0, 0), (0, 5)), 5);
        assert_eq!(g.distance_meters((0, 0), (5, 0)), 10);
    }

    /// Placing the same entity twice moves it to the new position.
    #[test]
    fn test_place_moves_entity() {
        let mut g = open_grid();
        let e = eid(42);
        g.place(e, (1, 1));
        g.place(e, (3, 3));
        assert_eq!(g.position_of(e), Some((3, 3)));
        // No longer at old pos.
        assert!(!g.occupants.contains_key(&(1, 1)));
    }

    /// `move_entity` with an out-of-bounds destination returns `OutOfBounds`.
    #[test]
    fn test_move_entity_out_of_bounds() {
        let mut g = Grid::new(3, 3);
        let e = eid(1);
        g.place(e, (2, 2));
        // Step to (3,3) which is out of the 3×3 grid (indices 0–2).
        let path = vec![(2, 2), (3, 3)];
        let result = g.move_entity(e, &path);
        assert_eq!(result, Err(GridError::OutOfBounds));
    }

    /// `move_entity` with a gap in the path returns `InvalidPath`.
    #[test]
    fn test_move_entity_invalid_gap() {
        let mut g = open_grid();
        let e = eid(1);
        g.place(e, (0, 0));
        // Jump of 2 squares — invalid.
        let path = vec![(0, 0), (2, 0)];
        assert_eq!(g.move_entity(e, &path), Err(GridError::InvalidPath));
    }

    /// `move_entity` blocked by another occupant returns `Occupied`.
    #[test]
    fn test_move_entity_blocked_by_occupant() {
        let mut g = open_grid();
        let mover = eid(1);
        let blocker = eid(2);
        g.place(mover, (0, 0));
        g.place(blocker, (1, 0));

        let path = vec![(0, 0), (1, 0)];
        assert_eq!(g.move_entity(mover, &path), Err(GridError::Occupied));
    }

    /// `LosResult::Clear` when nothing is in the way.
    #[test]
    fn test_los_clear() {
        let g = open_grid();
        assert_eq!(g.line_of_sight((0, 0), (5, 0)), LosResult::Clear);
    }

    /// Dead cover (0 HP) does not produce `ThroughCover`.
    #[test]
    fn test_los_through_dead_cover_is_clear() {
        let mut g = open_grid();
        g.cover_objects.insert(
            (5, 5),
            CoverInstance {
                material: "rubble".to_string(),
                current_hp: 0,
                max_hp: 20,
            },
        );
        // Dead cover should not block or return ThroughCover.
        let result = g.line_of_sight((3, 5), (7, 5));
        assert_eq!(
            result,
            LosResult::Clear,
            "destroyed cover (0 HP) must not produce ThroughCover"
        );
    }

    /// `movement_options` stops BFS at walls — wall squares are not returned.
    #[test]
    fn test_movement_options_respects_walls() {
        let mut g = open_grid();
        // Place a wall at (1,0).
        g.set_tile((1, 0), TileKind::Wall);

        let opts = g.movement_options((0, 0), 5);
        assert!(
            !opts.contains(&(1u16, 0u16)),
            "wall square must not appear in movement options"
        );
    }

    /// `square_aoe_targets` at grid edges does not panic (bounds clamping).
    #[test]
    fn test_square_aoe_edge_clamp() {
        let mut g = Grid::new(5, 5);
        let e = eid(1);
        g.place(e, (0, 0));
        // Center at (0,0) with radius 2 — should not panic even though the
        // area extends outside the grid.
        let targets = g.square_aoe_targets((0, 0), 2);
        assert!(targets.contains(&e));
    }

    /// `cone_targets` returns the attacker entity itself if it occupies `from`?
    /// No — entities at `from` are excluded.
    #[test]
    fn test_cone_excludes_origin() {
        let mut g = open_grid();
        let attacker = eid(1);
        let target = eid(2);
        g.place(attacker, (5, 5));
        g.place(target, (5, 3)); // due north

        let targets = g.cone_targets((5, 5), Facing::N, 8);
        assert!(
            !targets.contains(&attacker),
            "attacker at origin must not be in cone targets"
        );
        assert!(
            targets.contains(&target),
            "entity due north must be in North-facing cone"
        );
    }
}
