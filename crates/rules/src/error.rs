//! Errors raised by the `cpr_rules` crate.
//!
//! A single error type, [`RulesError`], covers all rules-engine failure
//! modes. Variants are added as later WPs introduce new failure cases
//! (e.g. invalid skill check inputs, illegal combat actions). Callers
//! pattern-match on the variant; the [`std::fmt::Display`] impl produces
//! a human-readable message for logs / UI.

use std::fmt;

/// Failure modes raised by the `cpr_rules` crate.
///
/// New variants will be added as later Work Packages introduce additional
/// rules-engine failure cases. Callers should `match` exhaustively and
/// expect the enum to grow over time.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RulesError {
    /// A LUCK spend was attempted but the remaining pool could not
    /// cover the request. The character's `luck_pool` is unchanged.
    /// See rulebook p.130 ("Using Your LUCK").
    InsufficientLuck {
        /// The number of LUCK Points the caller asked to spend.
        requested: u8,
        /// The number of LUCK Points actually in the pool at the time
        /// of the request.
        available: u8,
    },
}

impl fmt::Display for RulesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RulesError::InsufficientLuck {
                requested,
                available,
            } => write!(
                f,
                "insufficient LUCK: requested {requested}, available {available}"
            ),
        }
    }
}

impl std::error::Error for RulesError {}
