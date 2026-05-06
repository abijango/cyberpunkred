//! The [`Resolution`] trait implemented by every probabilistic action in the
//! rules engine, plus a shared [`CheckBreakdown`] envelope for any roll-vs-DV
//! check (skill checks, attacks, NET actions).
//!
//! Determinism (see `IMPLEMENTATION_PLAN.md` §2.4) is the load-bearing property
//! here: every implementation of [`Resolution::resolve`] threads the same
//! `&mut Rng` through its dice rolls, never touches OS entropy or the clock,
//! and consumes the RNG in a fixed order so the replay tool can reproduce any
//! game from its seed and action log.

use crate::dice::CritD10;
use crate::rng::Rng;
use crate::types::DV;
use serde::{Deserialize, Serialize};

/// Anything in the rules engine that produces a probabilistic outcome
/// implements this trait.
///
/// # Determinism contract
///
/// Implementations **must** consume the RNG deterministically:
///
/// - never branch on wall-clock time (`Instant::now`, `SystemTime::now`),
/// - never read thread-local state or environment variables,
/// - never call `rand::thread_rng` or `rand::random`,
/// - take `rng: &mut Rng` as the sole source of randomness and consume it
///   in a fixed order for a given input.
///
/// These rules make every game replayable from its seed and action log. See
/// `IMPLEMENTATION_PLAN.md` §2.4.
pub trait Resolution {
    /// The structured result this resolution produces — typically a
    /// [`CheckBreakdown`] for roll-vs-DV checks, or a richer record for
    /// attacks and NET actions.
    type Outcome;

    /// Run the resolution against the mutable game `world`, drawing all
    /// randomness from `rng`.
    ///
    /// Implementations must obey the determinism contract documented on the
    /// trait: no clock reads, no thread-local RNG, no OS entropy.
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome;
}

/// Shared structured outcome record for any roll-vs-DV check.
///
/// Used by skill checks, attacks, and NET actions. The invariants
/// `final_value == stat_value + skill_value + modifier_total + luck_spent + d10.net`,
/// `margin == final_value - dv.0` and `success == (margin >= 0)` hold by
/// construction when the value is built via [`CheckBreakdown::new`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckBreakdown {
    /// The character's relevant STAT value at roll time, after effects.
    pub stat_value: i16,
    /// The character's relevant Skill rank at roll time, after effects.
    pub skill_value: i16,
    /// Sum of all situational modifiers contributed by the active effect
    /// stack (cover, range, wound penalties, etc.).
    pub modifier_total: i16,
    /// Points of LUCK the player committed to this check before the roll.
    pub luck_spent: u8,
    /// The exploding/imploding d10 result. See `dice::d10_with_crits`.
    pub d10: CritD10,
    /// `stat_value + skill_value + modifier_total + luck_spent + d10.net`.
    pub final_value: i16,
    /// The Difficulty Value the check was rolled against.
    pub dv: DV,
    /// `true` iff `margin >= 0`.
    pub success: bool,
    /// `final_value - dv.0`. Negative on failure; positive margin can be
    /// consumed by downstream rules (e.g., damage scaling, NET action effects).
    pub margin: i16,
}

impl CheckBreakdown {
    /// Build a [`CheckBreakdown`] from its raw inputs, deriving `final_value`,
    /// `margin`, and `success` so they are guaranteed consistent.
    ///
    /// `final_value = stat_value + skill_value + modifier_total + luck_spent + d10.net`,
    /// `margin = final_value - dv.0`, `success = margin >= 0`.
    pub fn new(
        stat_value: i16,
        skill_value: i16,
        modifier_total: i16,
        luck_spent: u8,
        d10: CritD10,
        dv: DV,
    ) -> Self {
        let final_value = stat_value + skill_value + modifier_total + luck_spent as i16 + d10.net;
        let margin = final_value - dv.0 as i16;
        let success = margin >= 0;
        Self {
            stat_value,
            skill_value,
            modifier_total,
            luck_spent,
            d10,
            final_value,
            dv,
            success,
            margin,
        }
    }
}

/// Mutable game state passed to every [`Resolution::resolve`] call.
///
/// Currently a placeholder — fields will be populated by a later work package
/// (the world container). Kept as a unit struct so downstream signatures can
/// stabilise now without blocking on world implementation.
#[derive(Debug, Default)]
pub struct World;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dice::{d10_with_crits, D10Outcome};
    use rand::SeedableRng;

    /// Minimal `CritD10` builder for tests that don't care about the roll
    /// history, only the `net` contribution to the check.
    fn fake_normal_d10(roll: u8) -> CritD10 {
        CritD10 {
            base: roll,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: roll as i16,
        }
    }

    #[test]
    fn test_check_breakdown_success_flag() {
        // Clear success: 5 + 4 + 0 + 0 + 7 = 16 vs DV 13 → margin 3, success.
        let success = CheckBreakdown::new(5, 4, 0, 0, fake_normal_d10(7), DV(13));
        assert_eq!(success.final_value, 16);
        assert_eq!(success.margin, 16 - 13);
        assert!(success.success);
        assert_eq!(success.success, success.margin >= 0);

        // Marginal success: total exactly equals DV → margin 0, success.
        let exact = CheckBreakdown::new(4, 4, 0, 0, fake_normal_d10(5), DV(13));
        assert_eq!(exact.final_value, 13);
        assert_eq!(exact.margin, 0);
        assert!(exact.success);
        assert_eq!(exact.success, exact.margin >= 0);

        // Failure: 2 + 2 + 0 + 0 + 4 = 8 vs DV 15 → margin -7, no success.
        let fail = CheckBreakdown::new(2, 2, 0, 0, fake_normal_d10(4), DV(15));
        assert_eq!(fail.final_value, 8);
        assert_eq!(fail.margin, 8 - 15);
        assert!(!fail.success);
        assert_eq!(fail.success, fail.margin >= 0);

        // Negative net (critical failure): base 1, follow-up 10 → net -9.
        let crit_fail_d10 = CritD10 {
            base: 1,
            follow_up: Some(10),
            outcome: D10Outcome::CriticalFailure,
            net: -9,
        };
        let crit_fail = CheckBreakdown::new(6, 6, 1, 0, crit_fail_d10, DV(13));
        assert_eq!(crit_fail.final_value, 6 + 6 + 1 - 9);
        assert_eq!(crit_fail.margin, crit_fail.final_value - 13);
        assert!(!crit_fail.success);
        assert_eq!(crit_fail.success, crit_fail.margin >= 0);

        // Modifier and luck both contribute. -2 cover, +3 LUCK.
        let with_mods = CheckBreakdown::new(7, 5, -2, 3, fake_normal_d10(6), DV(17));
        assert_eq!(with_mods.final_value, 7 + 5 - 2 + 3 + 6);
        assert_eq!(with_mods.margin, with_mods.final_value - 17);
        assert_eq!(with_mods.success, with_mods.margin >= 0);
    }

    #[test]
    fn test_check_breakdown_serializes() {
        let mut rng = Rng::seed_from_u64(42);
        let d10 = d10_with_crits(&mut rng);
        let original = CheckBreakdown::new(6, 4, -1, 2, d10, DV::PROFESSIONAL);

        let serialized = ron::ser::to_string(&original).expect("RON serialization failed");
        let deserialized: CheckBreakdown =
            ron::de::from_str(&serialized).expect("RON deserialization failed");

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_check_breakdown_constructor_invariants() {
        // All zeros, DV 0 → margin 0, success.
        let zero = CheckBreakdown::new(0, 0, 0, 0, fake_normal_d10(0), DV(0));
        assert_eq!(zero.final_value, 0);
        assert_eq!(zero.margin, 0);
        assert!(zero.success);

        // Maximum-ish luck (10), large stat/skill, large d10 net.
        let big = CheckBreakdown::new(8, 10, 0, 10, fake_normal_d10(10), DV::INCREDIBLE);
        assert_eq!(big.final_value, 8 + 10 + 10 + 10);
        assert_eq!(big.margin, big.final_value - DV::INCREDIBLE.0 as i16);
        assert!(big.success);
    }

    /// Minimal type implementing [`Resolution`] — exists to pin the trait
    /// signature so an accidental change here breaks the build instead of
    /// silently breaking downstream attack/skill-check WPs.
    struct MockCheck {
        result: CheckBreakdown,
    }

    impl Resolution for MockCheck {
        type Outcome = CheckBreakdown;

        fn resolve(&self, _world: &mut World, _rng: &mut Rng) -> Self::Outcome {
            self.result.clone()
        }
    }

    #[test]
    fn test_resolution_trait_can_be_implemented() {
        let mut world = World;
        let mut rng = Rng::seed_from_u64(0);
        let mock = MockCheck {
            result: CheckBreakdown::new(5, 5, 0, 0, fake_normal_d10(5), DV::EVERYDAY),
        };
        let outcome = mock.resolve(&mut world, &mut rng);
        assert_eq!(outcome.final_value, 15);
        assert!(outcome.success);
    }
}
