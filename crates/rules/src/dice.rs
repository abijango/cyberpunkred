//! Dice helpers — single source of truth for every roll in the rules engine.
//!
//! The Cyberpunk RED critical mechanics on p.129–130 are encoded in
//! [`d10_with_crits`]. All other dice helpers are simple bounded uniform rolls.

use crate::rng::Rng;
use rand::Rng as _;
use serde::{Deserialize, Serialize};

/// Roll a single d10, returning a value in `1..=10`. No crit handling.
pub fn d10(rng: &mut Rng) -> u8 {
    rng.random_range(1..=10)
}

/// Roll a single d6, returning a value in `1..=6`.
pub fn d6(rng: &mut Rng) -> u8 {
    rng.random_range(1..=6)
}

/// Roll `n` d6s and return the individual values, in roll order.
///
/// Used for damage rolls (a 3d6 weapon, the 2d6 of an Asp Black ICE attack,
/// etc.). The caller decides how to aggregate the values.
pub fn ndn_d6(n: u8, rng: &mut Rng) -> Vec<u8> {
    (0..n).map(|_| d6(rng)).collect()
}

/// Roll a d10 with the Cyberpunk RED crit rules. See p.129–130.
///
/// - **Natural 10:** roll another d10 and add to the base.
///   The follow-up does not chain — even if it is also a 10,
///   no further explosion occurs.
/// - **Natural 1:** roll another d10 and subtract from the base.
///   The follow-up does not chain — even if it is also a 1,
///   no further fumble occurs.
///
/// The returned [`CritD10::net`] is what gets added to `STAT + Skill` for the
/// final check value: `final_check = stat + skill + net`. It can be negative
/// on a critical failure.
pub fn d10_with_crits(rng: &mut Rng) -> CritD10 {
    let base = d10(rng);
    match base {
        10 => {
            let follow_up = d10(rng);
            CritD10 {
                base,
                follow_up: Some(follow_up),
                outcome: D10Outcome::CriticalSuccess,
                net: 10_i16 + follow_up as i16,
            }
        }
        1 => {
            let follow_up = d10(rng);
            CritD10 {
                base,
                follow_up: Some(follow_up),
                outcome: D10Outcome::CriticalFailure,
                net: 1_i16 - follow_up as i16,
            }
        }
        _ => CritD10 {
            base,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: base as i16,
        },
    }
}

/// Result of a [`d10_with_crits`] roll.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CritD10 {
    /// Base roll, in `1..=10`.
    pub base: u8,
    /// The follow-up roll if `base` was 1 or 10. `None` for normal rolls.
    pub follow_up: Option<u8>,
    /// Whether the roll triggered a critical success or failure.
    pub outcome: D10Outcome,
    /// Net contribution to a check. `final_check = stat + skill + net`.
    /// May be negative on critical failure (e.g., base 1, follow-up 10 → -9).
    pub net: i16,
}

/// Result kind for [`CritD10`]. See p.129–130.
#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum D10Outcome {
    Normal,
    CriticalSuccess,
    CriticalFailure,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    /// Walk seeds in order until we find one whose initial RNG state satisfies
    /// `pred`. Used by the crit tests to find seeds that produce specific
    /// outcomes without hardcoding values that would break across `rand`
    /// patch versions.
    fn find_seed_where<F>(pred: F) -> u64
    where
        F: Fn(&mut Rng) -> bool,
    {
        for seed in 0..1_000_000 {
            let mut r = Rng::seed_from_u64(seed);
            if pred(&mut r) {
                return seed;
            }
        }
        panic!("no matching seed found within search bound");
    }

    #[test]
    fn test_d10_distribution() {
        // 100k rolls with a fixed seed: every value 1..=10 must appear, and
        // mean must converge near 5.5.
        let mut rng = Rng::seed_from_u64(0);
        let mut counts = [0u32; 11];
        let mut total: u64 = 0;
        let n: u64 = 100_000;
        for _ in 0..n {
            let v = d10(&mut rng);
            assert!((1..=10).contains(&v), "d10 returned out-of-range {v}");
            counts[v as usize] += 1;
            total += v as u64;
        }
        for (v, &count) in counts.iter().enumerate().skip(1) {
            assert!(count > 0, "value {v} never rolled in {n} samples");
        }
        let mean = total as f64 / n as f64;
        // 100k samples → SE of mean ≈ √(8.25/100k) ≈ 0.009; ±0.1 is well within tolerance.
        assert!(
            (mean - 5.5).abs() < 0.1,
            "mean {mean} not within 0.1 of 5.5"
        );
    }

    #[test]
    fn test_d10_with_crits_natural_10() {
        let seed = find_seed_where(|r| d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        let result = d10_with_crits(&mut rng);
        assert_eq!(result.base, 10);
        assert!(result.follow_up.is_some());
        let follow_up = result.follow_up.unwrap();
        assert!((1..=10).contains(&follow_up));
        assert_eq!(result.outcome, D10Outcome::CriticalSuccess);
        assert_eq!(result.net, 10_i16 + follow_up as i16);
    }

    #[test]
    fn test_d10_with_crits_natural_1() {
        let seed = find_seed_where(|r| d10(r) == 1);
        let mut rng = Rng::seed_from_u64(seed);
        let result = d10_with_crits(&mut rng);
        assert_eq!(result.base, 1);
        assert!(result.follow_up.is_some());
        let follow_up = result.follow_up.unwrap();
        assert!((1..=10).contains(&follow_up));
        assert_eq!(result.outcome, D10Outcome::CriticalFailure);
        assert_eq!(result.net, 1_i16 - follow_up as i16);
    }

    #[test]
    fn test_d10_with_crits_no_chained_crits() {
        // Find a seed where the first two d10 rolls are both 10 — then a single
        // call to d10_with_crits must consume *exactly* two rolls and return
        // net == 20 (not trigger a third roll).
        let seed = find_seed_where(|r| d10(r) == 10 && d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        let result = d10_with_crits(&mut rng);
        assert_eq!(result.base, 10);
        assert_eq!(result.follow_up, Some(10));
        assert_eq!(result.outcome, D10Outcome::CriticalSuccess);
        assert_eq!(result.net, 20, "follow-up of 10 must not chain");

        // Same idea for natural-1 → 1 (no chained fumble).
        let seed_11 = find_seed_where(|r| d10(r) == 1 && d10(r) == 1);
        let mut rng_11 = Rng::seed_from_u64(seed_11);
        let result_11 = d10_with_crits(&mut rng_11);
        assert_eq!(result_11.base, 1);
        assert_eq!(result_11.follow_up, Some(1));
        assert_eq!(result_11.outcome, D10Outcome::CriticalFailure);
        assert_eq!(result_11.net, 0, "follow-up of 1 must not chain");
    }

    #[test]
    fn test_seed_determinism() {
        let mut a = Rng::seed_from_u64(42);
        let mut b = Rng::seed_from_u64(42);
        for _ in 0..1000 {
            assert_eq!(d10(&mut a), d10(&mut b));
            assert_eq!(d6(&mut a), d6(&mut b));
        }
    }

    #[test]
    fn test_d6_range_and_distribution() {
        let mut rng = Rng::seed_from_u64(7);
        let mut counts = [0u32; 7];
        let n: u64 = 60_000;
        for _ in 0..n {
            let v = d6(&mut rng);
            assert!((1..=6).contains(&v));
            counts[v as usize] += 1;
        }
        for (v, &count) in counts.iter().enumerate().skip(1) {
            assert!(count > 0, "value {v} never rolled");
        }
    }

    #[test]
    fn test_ndn_d6() {
        let mut rng = Rng::seed_from_u64(7);
        let rolls = ndn_d6(5, &mut rng);
        assert_eq!(rolls.len(), 5);
        for v in rolls {
            assert!((1..=6).contains(&v));
        }

        // n = 0 returns an empty Vec — useful when a damage formula resolves
        // to zero dice for a given configuration.
        let empty = ndn_d6(0, &mut rng);
        assert!(empty.is_empty());
    }

    #[test]
    fn test_crit_failure_can_go_negative() {
        // The book explicitly notes that net can be negative on a crit
        // failure — a base of 1 with a follow-up of 10 gives -9.
        let seed = find_seed_where(|r| d10(r) == 1 && d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        let result = d10_with_crits(&mut rng);
        assert_eq!(result.base, 1);
        assert_eq!(result.follow_up, Some(10));
        assert_eq!(result.net, -9);
    }
}
