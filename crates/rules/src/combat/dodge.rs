//! REF≥8 dodge-election helper for ranged combat.
//!
//! Per p.172, a defender with REF 8 or higher may choose to attempt to dodge
//! a ranged attack (replacing the static range-table DV with their own
//! DEX + Evasion + 1d10 check). The threshold is on *current* REF — after all
//! armour, wound, and other effect modifiers have been applied.
//!
//! See [`can_elect_dodge_ranged`] for the public entry point.

use crate::character::Character;

/// Returns `true` if `character` is eligible to elect a dodge reaction
/// against a ranged attack this round.
///
/// Per p.172 (Resolving Ranged Combat Attacks): "A Defender with a REF 8 or
/// higher can choose to attempt to dodge a Ranged Attack instead of using the
/// range table to determine the DV."
///
/// "Current" REF means the value *after* all active [`crate::effects::EffectModifier`]s
/// have been applied — e.g. a base-8 character wearing Heavy Flak armour that
/// carries a `StatPenalty { stat: Ref, by: 4 }` has `current_ref() == 4` and
/// therefore **cannot** elect to dodge.
///
/// # See also
/// - `crate::character::Character::current_ref` — the authoritative current-REF
///   query used here.
///
/// # Example
/// ```
/// # use cpr_rules::character::Character;
/// # use cpr_rules::combat::dodge::can_elect_dodge_ranged;
/// // (build a Character with current REF ≥ 8 somehow)
/// // assert!(can_elect_dodge_ranged(&character));
/// ```
// See p.172.
pub fn can_elect_dodge_ranged(character: &Character) -> bool {
    character.current_ref() >= 8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::EffectModifier;
    use crate::effects::{ActiveEffect, EffectDuration, EffectSource};
    use crate::types::{EffectInstanceId, Stat};
    use crate::world::test_support::fresh_pc;
    use uuid::Uuid;

    fn effect_id(n: u128) -> EffectInstanceId {
        EffectInstanceId(Uuid::from_u128(n))
    }

    /// A character whose current REF is exactly 8 may elect to dodge.
    ///
    /// `fresh_pc` has REF 7 by default; we bump it to 8 to hit the threshold.
    #[test]
    fn test_can_dodge_at_ref_8() {
        let mut pc = fresh_pc();
        // fresh_pc has base REF 7; raise it to exactly 8.
        pc.stats.r#ref = 8;
        assert!(
            can_elect_dodge_ranged(&pc),
            "current_ref == 8 should be eligible to dodge"
        );
    }

    /// A character with current REF 7 cannot elect to dodge.
    ///
    /// `fresh_pc` has REF 7 by default — no mutation needed.
    #[test]
    fn test_cannot_dodge_at_ref_7() {
        let pc = fresh_pc();
        // fresh_pc has base REF 7 with no active effects → current_ref == 7.
        assert_eq!(pc.current_ref(), 7);
        assert!(
            !can_elect_dodge_ranged(&pc),
            "current_ref == 7 must not be eligible to dodge"
        );
    }

    /// Central regression: armour penalty drives current REF below 8, blocking dodge.
    ///
    /// A character with base REF 8 and a `StatPenalty { stat: Ref, by: 4 }`
    /// active effect has `current_ref() == 4` and must return `false`.
    ///
    /// This validates that the helper uses *current* REF (post-effects), not
    /// the base stat. See p.172 footnote and `IMPLEMENTATION_PLAN.md` §2.6.
    #[test]
    fn test_armor_penalty_blocks_dodge() {
        let mut pc = fresh_pc();
        pc.stats.r#ref = 8; // base REF 8 — would be eligible without armour

        // Simulate Heavy Flak armour: -4 REF penalty (see p.172 sidebar / WP spec).
        pc.effects.add(ActiveEffect {
            id: effect_id(1),
            source: EffectSource::Armor,
            modifiers: vec![EffectModifier::StatPenalty {
                stat: Stat::Ref,
                by: 4,
            }],
            duration: EffectDuration::Permanent,
        });

        // base 8 − 4 = 4; well below the threshold.
        assert_eq!(pc.current_ref(), 4);
        assert!(
            !can_elect_dodge_ranged(&pc),
            "armour-reduced current_ref == 4 must block dodge election"
        );
    }
}
