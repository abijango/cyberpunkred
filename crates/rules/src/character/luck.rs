//! LUCK pool lifecycle on [`Character`].
//!
//! Per rulebook p.130 ("Using Your LUCK"), every Character has a LUCK Pool
//! that holds a number of LUCK Points equal to their LUCK Statistic. The
//! pool refills at the beginning of each game session, and points are
//! spent before a roll to add `+1` per point to that Check.
//!
//! This module attaches three lifecycle methods to [`Character`]:
//!
//! - [`Character::refill_luck`] — top the pool back up to the LUCK STAT.
//! - [`Character::spend_luck`]  — debit the pool, returning an error
//!   when the request exceeds what is available.
//! - [`Character::luck_remaining`] — read-only accessor for the pool.
//!
//! The current implementation reads the *base* `stats.luck` value when
//! refilling. Once WP-104 lands its `current_luck()` accessor (which
//! walks the [`crate::effects::EffectStack`] for transient LUCK
//! adjustments), callers may prefer that — but WP-103 deliberately does
//! not depend on WP-104.

use crate::character::Character;
use crate::error::RulesError;

impl Character {
    /// Refill this character's LUCK Pool to their base LUCK Statistic.
    ///
    /// Called by the `gm` crate at the start of each game session
    /// ("gig start" in this codebase). Per rulebook p.130, the pool
    /// refills at the beginning of each game session — any unspent
    /// points from the prior session are not preserved or carried over.
    ///
    /// Reads `self.stats.luck` (the base STAT). Once WP-104 introduces
    /// a `current_luck()` accessor that folds in `EffectStack`
    /// modifiers, callers may prefer that helper.
    ///
    /// See rulebook p.130 ("Using Your LUCK").
    pub fn refill_luck(&mut self) {
        // See WP-104 — once current_luck() lands, callers may prefer that.
        self.luck_pool = self.stats.luck;
    }

    /// Attempt to spend `n` LUCK Points from this character's pool.
    ///
    /// On success the pool is decremented by `n` and `Ok(())` is
    /// returned. On failure (pool < `n`) the pool is left unchanged
    /// and [`RulesError::InsufficientLuck`] is returned, carrying both
    /// the requested and available counts so callers can surface a
    /// useful message.
    ///
    /// Spending zero is a successful no-op: the rules don't forbid
    /// "dedicate 0 LUCK to this Check," and treating it as Ok keeps
    /// caller code simple.
    ///
    /// See rulebook p.130 ("Using Your LUCK").
    pub fn spend_luck(&mut self, n: u8) -> Result<(), RulesError> {
        if n > self.luck_pool {
            return Err(RulesError::InsufficientLuck {
                requested: n,
                available: self.luck_pool,
            });
        }
        self.luck_pool -= n;
        Ok(())
    }

    /// Read the number of LUCK Points currently remaining in the pool.
    ///
    /// This is just a typed accessor for [`Character::luck_pool`]; it
    /// exists so callers reading "how much LUCK is left?" don't have
    /// to reach into the struct field directly.
    ///
    /// See rulebook p.130 ("Using Your LUCK").
    pub fn luck_remaining(&self) -> u8 {
        self.luck_pool
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::test_support::fresh_pc;

    #[test]
    fn test_refill_to_stat() {
        let mut pc = fresh_pc();
        // Drain to a known non-default value so the assertion is meaningful.
        pc.luck_pool = 0;
        pc.refill_luck();
        assert_eq!(pc.luck_pool, pc.stats.luck);
    }

    #[test]
    fn test_spend_decrements() {
        let mut pc = fresh_pc();
        pc.luck_pool = 5;
        pc.spend_luck(3).expect("3 of 5 must succeed");
        assert_eq!(pc.luck_pool, 2);
    }

    #[test]
    fn test_spend_more_than_available_errors() {
        let mut pc = fresh_pc();
        pc.luck_pool = 5;
        let err = pc.spend_luck(6).expect_err("6 of 5 must fail");
        assert_eq!(
            err,
            RulesError::InsufficientLuck {
                requested: 6,
                available: 5,
            }
        );
        // Pool must be unchanged on the error path.
        assert_eq!(pc.luck_pool, 5);
    }

    #[test]
    fn test_spend_zero_is_noop_and_succeeds() {
        let mut pc = fresh_pc();
        pc.luck_pool = 4;
        pc.spend_luck(0).expect("spending zero must succeed");
        assert_eq!(pc.luck_pool, 4);

        // And on an empty pool, spending zero must still succeed —
        // the predicate is `n > pool`, so 0 > 0 is false.
        pc.luck_pool = 0;
        pc.spend_luck(0)
            .expect("spending zero from empty pool must succeed");
        assert_eq!(pc.luck_pool, 0);
    }

    #[test]
    fn test_luck_remaining_matches_pool() {
        let mut pc = fresh_pc();
        pc.luck_pool = 6;
        assert_eq!(pc.luck_remaining(), 6);
        pc.spend_luck(2).expect("2 of 6 must succeed");
        assert_eq!(pc.luck_remaining(), 4);
        assert_eq!(pc.luck_remaining(), pc.luck_pool);
    }
}
