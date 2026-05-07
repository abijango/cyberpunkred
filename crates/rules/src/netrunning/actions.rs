//! NET Action accounting — Interface rank → action count.
//!
//! A Netrunner's Interface rank (their Netrunner Role Ability) determines how
//! many NET Actions they can take each Turn. This module exposes that mapping
//! as a single public function so all callers share one source of truth.
//!
//! ## Rulebook reference
//!
//! p.197 (NET Actions table):
//!
//! | Interface Rank | NET Actions |
//! |----------------|-------------|
//! | 1–3            | 2           |
//! | 4–6            | 3           |
//! | 7–9            | 4           |
//! | 10             | 5           |
//!
//! See also p.197 (rule box): "On your Turn, you can take either a Meat Action
//! or take as many NET Actions as your Interface (the Netrunner Role Ability)
//! allows."

/// Number of NET Actions a Netrunner gets per Turn from their Interface rank.
///
/// Per p.197: 1–3 → 2 actions, 4–6 → 3, 7–9 → 4, 10 → 5.
///
/// Rank 0 is treated identically to rank 1 (floor of 2 actions) because
/// Interface must be at least rank 1 to Netrun at all (p.197: "Without it,
/// you cannot Netrun"). Callers that need to enforce the rank ≥ 1 precondition
/// should do so before calling this function.
///
/// # Examples
///
/// ```
/// use cpr_rules::netrunning::actions::net_actions_per_turn;
///
/// assert_eq!(net_actions_per_turn(1), 2);
/// assert_eq!(net_actions_per_turn(5), 3);
/// assert_eq!(net_actions_per_turn(8), 4);
/// assert_eq!(net_actions_per_turn(10), 5);
/// ```
///
/// See p.197 (NET Actions table).
pub fn net_actions_per_turn(interface_rank: u8) -> u8 {
    // See p.197.
    match interface_rank {
        0..=3 => 2,
        4..=6 => 3,
        7..=9 => 4,
        _ => 5, // rank 10+
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// `test_actions_per_rank_1_through_3`: ranks 1, 2, 3 → 2 NET Actions.
    ///
    /// Per p.197 (NET Actions table): Interface Rank 1–3 → 2 NET Actions.
    #[test]
    fn test_actions_per_rank_1_through_3() {
        assert_eq!(net_actions_per_turn(1), 2, "rank 1 → 2 (p.197)");
        assert_eq!(net_actions_per_turn(2), 2, "rank 2 → 2 (p.197)");
        assert_eq!(net_actions_per_turn(3), 2, "rank 3 → 2 (p.197)");
    }

    /// `test_actions_per_rank_4_through_6`: ranks 4, 5, 6 → 3 NET Actions.
    ///
    /// Per p.197 (NET Actions table): Interface Rank 4–6 → 3 NET Actions.
    #[test]
    fn test_actions_per_rank_4_through_6() {
        assert_eq!(net_actions_per_turn(4), 3, "rank 4 → 3 (p.197)");
        assert_eq!(net_actions_per_turn(5), 3, "rank 5 → 3 (p.197)");
        assert_eq!(net_actions_per_turn(6), 3, "rank 6 → 3 (p.197)");
    }

    /// `test_actions_per_rank_7_through_9`: ranks 7, 8, 9 → 4 NET Actions.
    ///
    /// Per p.197 (NET Actions table): Interface Rank 7–9 → 4 NET Actions.
    #[test]
    fn test_actions_per_rank_7_through_9() {
        assert_eq!(net_actions_per_turn(7), 4, "rank 7 → 4 (p.197)");
        assert_eq!(net_actions_per_turn(8), 4, "rank 8 → 4 (p.197)");
        assert_eq!(net_actions_per_turn(9), 4, "rank 9 → 4 (p.197)");
    }

    /// `test_actions_per_rank_10`: rank 10 → 5 NET Actions.
    ///
    /// Per p.197 (NET Actions table): Interface Rank 10 → 5 NET Actions.
    #[test]
    fn test_actions_per_rank_10() {
        assert_eq!(net_actions_per_turn(10), 5, "rank 10 → 5 (p.197)");
    }

    /// `test_rank_zero_yields_2`: rank 0 → 2 NET Actions.
    ///
    /// Rank 0 is treated as the rank 1–3 floor (2 actions). Interface must be
    /// at least rank 1 to Netrun (p.197), so rank 0 is an underflow that we
    /// clamp rather than panic on.
    #[test]
    fn test_rank_zero_yields_2() {
        assert_eq!(
            net_actions_per_turn(0),
            2,
            "rank 0 → 2 (consistent with 1-3 floor)"
        );
    }
}
