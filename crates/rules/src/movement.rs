//! Movement primitives — grid-agnostic distance helpers.
//!
//! Cyberpunk RED measures distance in **metres / yards** and treats them
//! interchangeably (the difference is "only about 2 inches", p.126). When a
//! game uses a grid, "each 1-inch square corresponds to 2 meters/yards"
//! (p.126). A character's Move Action covers either:
//!
//! - `MOVE × 2` metres / yards, or
//! - `MOVE` squares (which may include diagonals).
//!
//! This module exposes the two distances and the metre ↔ square conversions
//! the rest of the engine and UI consume. Higher-level concerns —
//! pathfinding, terrain costs, Run vs. Walk allocation across phases — do
//! **not** live here; see WP-301+ for combat / movement-action plumbing.
//!
//! Rulebook references: pp.126–127 ("Distance and Movement", "Literal
//! Movement").

use crate::character::Character;

/// One grid square equals two metres (or yards). Per p.126, "each 1-inch
/// square corresponds to 2 meters/yards". This constant is the single
/// source of truth for the conversion factor; do not inline `2` elsewhere.
pub const METERS_PER_SQUARE: u16 = 2;

/// Distance, in metres, a [`Character`] can cover with a single Move
/// Action. Per p.126, this is `MOVE × 2` metres.
///
/// MOVE is sourced from [`Character::current_move`] (WP-104), which already
/// applies stat-level modifiers, the dedicated `MovePenalty` (e.g. wound
/// states, p.186), and the rulebook floor of 1 for any still-acting
/// character. A pathological negative `current_move` — not reachable
/// through the WP-104 floor but defensively handled here — is clamped to 0
/// before the conversion.
pub fn move_distance_meters(character: &Character) -> u16 {
    move_distance_squares(character) * METERS_PER_SQUARE
}

/// Distance, in squares, a [`Character`] can cover with a single Move
/// Action. Per p.126, this equals their current MOVE.
///
/// MOVE is sourced from [`Character::current_move`] (WP-104), which floors
/// at 1 for a still-acting character. Should the value ever come back
/// non-positive (defensive guard against future regressions), this returns
/// `0` rather than wrapping around the unsigned cast.
pub fn move_distance_squares(character: &Character) -> u16 {
    let m = character.current_move();
    if m < 0 {
        0
    } else {
        m as u16
    }
}

/// Convert a distance in metres to whole grid squares.
///
/// **Rounding decision: floor.** Per p.126, movement on the grid is
/// measured in whole squares (one square = `METERS_PER_SQUARE` m). A
/// partial square is not traversable on the grid, so `5 m` resolves to
/// `2` squares (covering 4 m) with the remaining 1 m discarded — the
/// character cannot "carry over" a partial square. This is the natural
/// reading of the literal-movement rules and the choice the WP brief
/// asked us to pin.
///
/// `metres → squares` is therefore lossy; round-tripping
/// `squares_to_meters(meters_to_squares(m))` produces a value `≤ m`.
pub fn meters_to_squares(m: u16) -> u16 {
    m / METERS_PER_SQUARE
}

/// Convert a distance in whole grid squares to metres. Per p.126, one
/// square equals `METERS_PER_SQUARE` m. This conversion is exact.
pub fn squares_to_meters(s: u16) -> u16 {
    s * METERS_PER_SQUARE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{ActiveEffect, EffectDuration, EffectModifier, EffectSource, WoundState};
    use crate::types::EffectInstanceId;
    use crate::world::test_support::fresh_pc;
    use uuid::Uuid;

    #[test]
    fn test_move_distances() {
        // fresh_pc has MOVE 5; bump to 6 to match the WP brief's worked
        // example (MOVE 6 → 12 m / 6 squares).
        let mut pc = fresh_pc();
        pc.stats.r#move = 6;
        assert_eq!(move_distance_squares(&pc), 6);
        assert_eq!(move_distance_meters(&pc), 12);
    }

    #[test]
    fn test_meters_to_squares_rounds_down() {
        // Per p.126, partial squares aren't traversable on the grid; we
        // floor.
        assert_eq!(meters_to_squares(0), 0);
        assert_eq!(meters_to_squares(5), 2); // 5 m → 2 full squares (4 m) + 1 m discarded
        assert_eq!(meters_to_squares(6), 3);
    }

    #[test]
    fn test_squares_to_meters_round_trip() {
        // metres → squares → metres is lossy (≤ original metres).
        for m in 0u16..=64 {
            let round_tripped = squares_to_meters(meters_to_squares(m));
            assert!(
                round_tripped <= m,
                "squares_to_meters(meters_to_squares({m})) = {round_tripped}, expected ≤ {m}"
            );
        }
        // squares → metres → squares is exact for any square count.
        for s in 0u16..=64 {
            assert_eq!(meters_to_squares(squares_to_meters(s)), s);
        }
    }

    #[test]
    fn test_meters_per_square_constant() {
        // Regression guard against accidental change — the rulebook fixes
        // this conversion at 2 m/yds per square (p.126).
        assert_eq!(METERS_PER_SQUARE, 2);
    }

    #[test]
    fn test_move_distance_uses_current_move_with_floor() {
        // Mortally Wounded contributes MovePenalty(-10), but
        // current_move() floors at 1 for a still-acting character (p.186).
        // That floor must propagate through the movement helpers.
        let mut pc = fresh_pc();
        pc.effects.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(1)),
            source: EffectSource::WoundState(WoundState::Mortally),
            modifiers: vec![EffectModifier::MovePenalty(-10)],
            duration: EffectDuration::Permanent,
        });
        assert_eq!(pc.current_move(), 1);
        assert_eq!(move_distance_squares(&pc), 1);
        assert_eq!(move_distance_meters(&pc), 2);
    }
}
