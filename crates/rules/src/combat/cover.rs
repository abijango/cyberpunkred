//! Cover interposition — WP-313.
//!
//! Applies cover material HP between an attack and a target, consuming cover
//! HP and returning how much damage was absorbed vs. passed through.
//!
//! ## Rulebook reference — pp.182–184
//!
//! From p.182 (*Cover Hit Points* and the *Cover Example* sidebar):
//!
//! > At 0 HP, cover is destroyed. If a cover's HP drops to 0, excess damage is
//! > lost and doesn't harm any targets hiding behind it. You can hurt them with
//! > your next Attack.
//!
//! This function implements the physical interposition step: how much of a
//! single attack's raw damage is soaked by the cover object, and how much (if
//! any) passes through to threaten the defender.
//!
//! **Note on the RAW "excess damage is lost" rule:** The rulebook states that
//! when cover is destroyed in a single hit, the excess damage does NOT harm the
//! target behind it on that same attack — the attacker must land a new attack
//! on the now-exposed defender next Turn. However, WP-313's public API is
//! defined to return `damage_through` (the excess) so that higher-level callers
//! (e.g. the explosives path — see p.174 and p.182's exception note) can handle
//! it appropriately. The standard attack path (WP-306/307) is responsible for
//! discarding `damage_through` when cover is destroyed, per RAW p.182. This
//! function is agnostic; it returns the arithmetic result and lets the caller
//! enforce the discard rule. See pp.182–184.

// See pp.182-184.

use crate::combat::grid::CoverInstance;

// ---------------------------------------------------------------------------
// CoverInterposition
// ---------------------------------------------------------------------------

/// Result of interposing a piece of cover between an attack and its target.
///
/// Returned by [`apply_cover`]. All fields are in hit-point / damage units.
///
/// ## Interpretation
///
/// - `damage_absorbed` is the portion of `raw_damage` that the cover soaked.
///   It equals `min(raw_damage, cover_hp_before)`.
/// - `damage_through` is the portion that passed through the cover (0 unless
///   the cover was destroyed in this hit). Per RAW p.182, the standard attack
///   path discards `damage_through` when `cover_destroyed` is `true`; the
///   field is exposed for the explosives exception (p.174) and future callers.
/// - `cover_destroyed` is `true` when `raw_damage >= cover_hp_before`.
///
/// See pp.182–184.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoverInterposition {
    /// Cover HP at the time the attack was resolved (before this call mutated
    /// the cover). See p.182.
    pub cover_hp_before: u16,
    /// Damage the cover absorbed (i.e. spent from `cover_hp_before`).
    /// Equals `min(raw_damage, cover_hp_before)`. See p.182.
    pub damage_absorbed: u16,
    /// Damage that passed through the cover after it was destroyed. Equals
    /// `raw_damage - cover_hp_before` when `cover_destroyed` is `true`,
    /// otherwise `0`. Per RAW p.182, the caller should discard this for
    /// normal attacks; it is returned for use by the explosives path (p.174).
    pub damage_through: u16,
    /// Cover HP remaining after the attack. `0` when `cover_destroyed` is
    /// `true`. See p.182.
    pub cover_hp_after: u16,
    /// `true` when the attack consumed all of the cover's remaining HP (i.e.
    /// `raw_damage >= cover_hp_before`). See p.182.
    pub cover_destroyed: bool,
}

// ---------------------------------------------------------------------------
// apply_cover
// ---------------------------------------------------------------------------

/// Apply cover material HP between an attack and a target.
///
/// Mutates `cover.current_hp` in place and returns a [`CoverInterposition`]
/// describing how the damage was split between the cover and whatever lies
/// behind it.
///
/// ## Rules summary (pp.182–184)
///
/// - Cover absorbs damage up to its current HP, decrementing `current_hp`
///   accordingly.
/// - If `raw_damage >= current_hp`, the cover is destroyed (`current_hp`
///   becomes `0`, `cover_destroyed = true`) and the leftover damage
///   (`raw_damage - hp_before`) is returned as `damage_through`.
/// - If `raw_damage < current_hp`, the cover remains intact with
///   `current_hp -= raw_damage` and `damage_through = 0`.
/// - If `current_hp` is already `0` (cover already destroyed), the cover
///   provides no protection at all: `damage_through == raw_damage`,
///   `damage_absorbed == 0`.
///
/// ## Caller responsibility
///
/// Per RAW p.182, when `cover_destroyed` is `true` the caller (the attack
/// path, e.g. WP-306/307) should discard `damage_through` — the target is
/// only exposed on the *next* Attack. The explosives exception (p.174) and
/// any future caller that needs the raw pass-through value may use it directly.
///
/// See pp.182–184.
pub fn apply_cover(cover: &mut CoverInstance, raw_damage: u16) -> CoverInterposition {
    // See pp.182-184.
    let cover_hp_before = cover.current_hp;

    if raw_damage >= cover_hp_before {
        // Cover is destroyed (or was already at 0 HP).
        let damage_absorbed = cover_hp_before;
        let damage_through = raw_damage - cover_hp_before;
        cover.current_hp = 0;

        CoverInterposition {
            cover_hp_before,
            damage_absorbed,
            damage_through,
            cover_hp_after: 0,
            cover_destroyed: true,
        }
    } else {
        // Cover survives; absorbs all damage.
        cover.current_hp -= raw_damage;

        CoverInterposition {
            cover_hp_before,
            damage_absorbed: raw_damage,
            damage_through: 0,
            cover_hp_after: cover.current_hp,
            cover_destroyed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: build a Thick Wood cover instance (20 HP per p.183).
    ///
    /// The *Bar*, *Log Cabin Wall*, and *Tree* entries on p.183 are all
    /// Thick Wood = 20 HP. We use "thick_wood" as the slug placeholder.
    fn thick_wood(current_hp: u16) -> CoverInstance {
        CoverInstance {
            material: "thick_wood".to_string(),
            current_hp,
            max_hp: 20,
        }
    }

    /// `test_cover_absorbs_up_to_hp` — 30 damage vs Thick Wood (20 HP) →
    /// 20 absorbed, 10 through, cover at 0 HP, cover_destroyed = true.
    ///
    /// Acceptance criterion per WP-313. See p.182.
    #[test]
    fn test_cover_absorbs_up_to_hp() {
        let mut cover = thick_wood(20);
        let result = apply_cover(&mut cover, 30);

        assert_eq!(result.cover_hp_before, 20);
        assert_eq!(result.damage_absorbed, 20);
        assert_eq!(result.damage_through, 10);
        assert_eq!(result.cover_hp_after, 0);
        assert!(result.cover_destroyed, "cover must be flagged as destroyed");
        // The cover's current_hp must be mutated to 0.
        assert_eq!(cover.current_hp, 0);
    }

    /// `test_cover_intact_partial` — 10 damage vs Thick Wood (20 HP) →
    /// 10 absorbed, 0 through, cover at 10 HP, cover_destroyed = false.
    ///
    /// Acceptance criterion per WP-313. See p.182.
    #[test]
    fn test_cover_intact_partial() {
        let mut cover = thick_wood(20);
        let result = apply_cover(&mut cover, 10);

        assert_eq!(result.cover_hp_before, 20);
        assert_eq!(result.damage_absorbed, 10);
        assert_eq!(result.damage_through, 0);
        assert_eq!(result.cover_hp_after, 10);
        assert!(!result.cover_destroyed, "cover must NOT be destroyed");
        // Mutation check.
        assert_eq!(cover.current_hp, 10);
    }

    /// `test_destroyed_cover_no_block` — cover already at 0 HP provides no
    /// protection; all damage passes through (`damage_through == raw_damage`).
    ///
    /// Acceptance criterion per WP-313. See p.182: "If a cover's HP drops to
    /// 0, excess damage is lost and doesn't harm any targets hiding behind it.
    /// You can hurt them with your **next Attack**." A cover that is already at
    /// 0 HP before the attack behaves identically — it cannot absorb anything.
    #[test]
    fn test_destroyed_cover_no_block() {
        let mut cover = thick_wood(0); // already destroyed
        let result = apply_cover(&mut cover, 15);

        assert_eq!(result.cover_hp_before, 0);
        assert_eq!(result.damage_absorbed, 0);
        assert_eq!(
            result.damage_through, 15,
            "destroyed cover must let all damage through"
        );
        assert_eq!(result.cover_hp_after, 0);
        assert!(
            result.cover_destroyed,
            "cover_destroyed must be true for a 0-HP cover"
        );
        assert_eq!(cover.current_hp, 0);
    }

    /// Extra: damage exactly equal to cover HP destroys cover with 0 through.
    ///
    /// Edge case — confirms the `>=` boundary: `raw_damage == current_hp`
    /// destroys the cover and leaves `damage_through = 0`. See p.182.
    #[test]
    fn test_exact_damage_destroys_cover() {
        let mut cover = thick_wood(20);
        let result = apply_cover(&mut cover, 20);

        assert_eq!(result.damage_absorbed, 20);
        assert_eq!(result.damage_through, 0);
        assert_eq!(result.cover_hp_after, 0);
        assert!(result.cover_destroyed);
        assert_eq!(cover.current_hp, 0);
    }

    /// Extra: zero damage against live cover leaves cover unchanged.
    ///
    /// A missed shot that ricochets and deals 0 raw damage should be a no-op.
    /// See pp.182–184.
    #[test]
    fn test_zero_damage_no_effect() {
        let mut cover = thick_wood(20);
        let result = apply_cover(&mut cover, 0);

        assert_eq!(result.cover_hp_before, 20);
        assert_eq!(result.damage_absorbed, 0);
        assert_eq!(result.damage_through, 0);
        assert_eq!(result.cover_hp_after, 20);
        assert!(!result.cover_destroyed);
        assert_eq!(cover.current_hp, 20);
    }
}
