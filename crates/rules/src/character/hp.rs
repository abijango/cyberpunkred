//! Hit-Point and Humanity derivation for [`Character`].
//!
//! Cyberpunk RED defines two **Derived Statistics** computed from a
//! character's STAT block: Hit Points (HP) and Humanity. This module is the
//! single home for those formulas, plus the [`recompute_wounds`] helper that
//! pushes a fresh derivation back into [`Character::wounds`] after a
//! permanent change to BODY or WILL (e.g. a cyberlimb install).
//!
//! Following the architecture rule in `IMPLEMENTATION_PLAN.md` §1.4
//! ("Effects compose, base values don't mutate"), the derivation reads the
//! immutable BODY / WILL / EMP from [`crate::character::StatBlock`]; the
//! one exception is [`Character::calculate_death_save`], which uses the
//! *current* BODY (i.e. base BODY plus any active [`crate::effects::EffectStack`]
//! modifiers). See the per-method docs below for the rationale.
//!
//! Rulebook references:
//! - p.79  Hit Points formula and table; Seriously Wounded ("half of total
//!   HP, rounded up"); Death Save ("equal to their BODY Statistic").
//! - p.80  Humanity = 10 × EMP at character creation.
//! - p.186 Wound-state thresholds — restated in combat terms; the Seriously
//!   Wounded threshold ("Less than 1/2 HP, round up") matches p.79.

use crate::character::Character;

impl Character {
    /// Maximum Hit Points = `10 + 5 × ceil((BODY + WILL) / 2)`.
    ///
    /// The book gives the formula on p.79 ("You have Hit Points equal to
    /// 10 + (5\[BODY and WILL averaged, rounding up\])") and a lookup
    /// table immediately below it. The two agree exactly for every
    /// 2..=10 BODY/WILL pair in the table. Worked example from the
    /// table: BODY 4, WILL 5 → ceil((4+5)/2) = 5 → HP = 10 + 25 = **35**.
    ///
    /// Reads the *base* [`crate::character::StatBlock::body`] and
    /// [`crate::character::StatBlock::will`]. Permanent BODY / WILL gains
    /// (cyberware install, IP spend) bump the base; transient effects
    /// (drugs, wound penalties) do not affect maximum HP.
    ///
    /// See p.79.
    pub fn calculate_max_hp(&self) -> u16 {
        let body = u16::from(self.stats.body);
        let will = u16::from(self.stats.will);
        // Ceil-average. BODY and WILL each fit in u8; their sum fits in
        // u16 with vast headroom, so `div_ceil` cannot overflow.
        let avg_round_up = (body + will).div_ceil(2);
        10 + 5 * avg_round_up
    }

    /// Seriously Wounded threshold = `ceil(max_hp / 2)`.
    ///
    /// The character becomes Seriously Wounded when their current HP drops
    /// **below** this number (p.186: "Less than 1/2 HP, round up"). For an
    /// odd `max_hp` the threshold rounds up: max_hp 35 → 18.
    ///
    /// See p.79 ("Seriously Wounded Wound Threshold is half of their total
    /// HP (rounded up)") and p.186.
    pub fn calculate_seriously_wounded_threshold(&self) -> u16 {
        self.calculate_max_hp().div_ceil(2)
    }

    /// Base value for a Death Save = the character's **current** BODY.
    ///
    /// p.79 ("Their Death Save is equal to their BODY Statistic") and
    /// p.186 (Mortally Wounded entry, which references the same Death Save
    /// mechanic) both phrase the Death Save in terms of *the* BODY STAT,
    /// without distinguishing base from current. We resolve this in favor
    /// of *current* BODY because:
    ///
    /// - The rulebook treats the Death Save as a CHECK ("Roll 1d10 and
    ///   try to roll equal to or under your BODY", p.186), and every
    ///   other CHECK in the system is rolled against current STATs.
    /// - The combat engine applies BODY-shifting effects (e.g. a drug
    ///   that buffs BODY, or a critical injury that drops it) via the
    ///   [`crate::effects::EffectStack`]; those effects must reach the
    ///   Death Save or the system would be inconsistent.
    /// - This matches the WP-105 specification note ("Death save base =
    ///   current BODY (NOT base BODY)").
    ///
    /// In the pathological case where stacked penalties push current BODY
    /// negative, this clamps to 0 — a 0-BODY character will auto-fail any
    /// 1d10 Death Save, which is the right behaviour.
    ///
    /// See p.79 / p.186.
    pub fn calculate_death_save(&self) -> u8 {
        let cur = self.current_body();
        if cur <= 0 {
            0
        } else if cur > i16::from(u8::MAX) {
            // Effectively unreachable (BODY is u8 and the largest plausible
            // bonus stack is small) — but be explicit rather than truncate.
            u8::MAX
        } else {
            cur as u8
        }
    }

    /// Starting Humanity at character creation = `10 × EMP`.
    ///
    /// Stored as `i16` because Humanity can later be driven negative by
    /// cyberware installation and cyberpsychosis (p.230); the *starting*
    /// value computed here is always non-negative, since EMP is `u8`.
    ///
    /// This is an associated function, not a method, because the only input
    /// is the creation-time EMP — characters in flight no longer "have a
    /// starting humanity," they have a current humanity that drifts from
    /// this seed.
    ///
    /// See p.80 ("Your starting Humanity, before any Cyberware is added,
    /// is your EMP × 10.").
    pub fn calculate_starting_humanity(emp: u8) -> i16 {
        i16::from(emp) * 10
    }
}

/// Recompute the derived [`crate::character::Wounds`] fields after a
/// permanent change to BODY or WILL.
///
/// Updates `max_hp`, `seriously_wounded_threshold`, and `death_save_base`
/// in place. **`current_hp` is deliberately untouched** — a wounded
/// character whose BODY rises mid-game keeps their accumulated damage; the
/// raised BODY only widens the *ceiling*. (Healing is its own concern —
/// see WP-106.)
///
/// The same rationale applies to `death_save_penalty` and `current_state`:
/// those track in-fight bookkeeping, not derivation, and are therefore
/// not touched here.
///
/// Typical callers: a cyberlimb install (p.94), an IP spend that raises
/// BODY or WILL (p.410), a `+1 BODY` permanent levelling action.
///
/// See p.79 (HP table), p.186 (Seriously Wounded threshold).
pub fn recompute_wounds(character: &mut Character) {
    character.wounds.max_hp = character.calculate_max_hp();
    character.wounds.seriously_wounded_threshold =
        character.calculate_seriously_wounded_threshold();
    character.wounds.death_save_base = character.calculate_death_save();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{ActiveEffect, EffectDuration, EffectModifier, EffectSource};
    use crate::types::{EffectInstanceId, Stat};
    use crate::world::test_support::fresh_pc;
    use uuid::Uuid;

    /// Acceptance: BODY 4, WILL 5 → 35 HP per the p.79 table.
    ///
    /// **Reconciliation:** the WP-105 acceptance criterion in
    /// `IMPLEMENTATION_PLAN.md` reads "BODY 4, WILL 5 → 30 HP", but a
    /// direct read of the p.79 table shows the WILL-5/BODY-4 cell is
    /// **35**, not 30. The formula on the same page (`10 + 5 ×
    /// ceil((BODY+WILL)/2)`) gives 35 as well: `ceil((4+5)/2) = 5`,
    /// `10 + 5×5 = 35`. The "30" in the WP doc is a typo, flagged in the
    /// PR. The rulebook is authoritative.
    #[test]
    fn test_hp_formula_matches_table() {
        let mut pc = fresh_pc();
        pc.stats.body = 4;
        pc.stats.will = 5;
        assert_eq!(pc.calculate_max_hp(), 35);
    }

    /// Acceptance: max_hp 35 → Seriously Wounded threshold 18.
    /// `ceil(35 / 2) = 18`. See p.186.
    #[test]
    fn test_seriously_wounded_rounds_up() {
        let mut pc = fresh_pc();
        // BODY 4 + WILL 5 → max HP 35.
        pc.stats.body = 4;
        pc.stats.will = 5;
        assert_eq!(pc.calculate_max_hp(), 35);
        assert_eq!(pc.calculate_seriously_wounded_threshold(), 18);
    }

    /// Acceptance: EMP 5 → starting Humanity 50. See p.80.
    #[test]
    fn test_starting_humanity_eq_10x_emp() {
        assert_eq!(Character::calculate_starting_humanity(5), 50);
        // Sanity: the formula scales linearly across the legal EMP range.
        assert_eq!(Character::calculate_starting_humanity(0), 0);
        assert_eq!(Character::calculate_starting_humanity(10), 100);
    }

    /// Acceptance: a wounded character whose BODY rises (e.g. cyberlimb
    /// install) keeps their accumulated damage. Only the maximum widens.
    #[test]
    fn test_recompute_wounds_preserves_damage() {
        let mut pc = fresh_pc();
        // Stage a wound history: BODY 4, WILL 4 → max_hp 30.
        pc.stats.body = 4;
        pc.stats.will = 4;
        recompute_wounds(&mut pc);
        assert_eq!(pc.wounds.max_hp, 30);

        // Take damage down to 12 HP (Seriously Wounded — but we don't
        // touch wound state here; that's WP-106's job).
        pc.wounds.current_hp = 12;

        // Permanent BODY raise: 4 → 6 (e.g. cyberlimb).
        pc.stats.body = 6;
        recompute_wounds(&mut pc);

        // BODY 6 + WILL 4 → ceil((6+4)/2) = 5 → max_hp 35.
        assert_eq!(pc.wounds.max_hp, 35);
        // The whole point of this test: damage is preserved.
        assert_eq!(pc.wounds.current_hp, 12);
        // Threshold tracks the new max.
        assert_eq!(pc.wounds.seriously_wounded_threshold, 18);
    }

    /// Regression: the Death Save base reads *current* BODY, not base
    /// BODY. A transient `StatBonus { stat: Body, by: 2 }` lifts the
    /// Death Save target along with everything else BODY-shaped.
    #[test]
    fn test_death_save_base_uses_current_body() {
        let mut pc = fresh_pc();
        pc.stats.body = 4;
        // No effects yet — current BODY == base BODY.
        assert_eq!(pc.calculate_death_save(), 4);

        // Apply a transient +2 BODY effect (e.g. a hypothetical drug).
        // current_body() now reads 6; calculate_death_save() must follow.
        pc.effects.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(0xDB)),
            source: EffectSource::Drug(crate::effects::DrugId("synthcoke".into())),
            modifiers: vec![EffectModifier::StatBonus {
                stat: Stat::Body,
                by: 2,
            }],
            duration: EffectDuration::Permanent,
        });
        assert_eq!(pc.current_body(), 6);
        assert_eq!(pc.calculate_death_save(), 6);
        // The base STAT is unchanged — invariant from §2.6.
        assert_eq!(pc.stats.body, 4);
    }

    /// Sanity: a couple more cells from the p.79 table to guard against
    /// off-by-one rounding bugs in the ceil-average.
    #[test]
    fn test_hp_table_corner_cases() {
        let mut pc = fresh_pc();
        // BODY 2, WILL 2 → ceil((2+2)/2) = 2 → 20 HP.
        pc.stats.body = 2;
        pc.stats.will = 2;
        assert_eq!(pc.calculate_max_hp(), 20);

        // BODY 10, WILL 10 → ceil((10+10)/2) = 10 → 60 HP.
        pc.stats.body = 10;
        pc.stats.will = 10;
        assert_eq!(pc.calculate_max_hp(), 60);

        // BODY 5, WILL 4 → ceil((5+4)/2) = 5 → 35 HP (table cell).
        pc.stats.body = 5;
        pc.stats.will = 4;
        assert_eq!(pc.calculate_max_hp(), 35);
    }

    /// Sanity: `recompute_wounds` does not touch `death_save_penalty`
    /// or `current_state` — those are in-fight bookkeeping, not
    /// derivation.
    #[test]
    fn test_recompute_wounds_leaves_penalty_and_state_alone() {
        use crate::effects::WoundState;

        let mut pc = fresh_pc();
        pc.stats.body = 4;
        pc.stats.will = 4;
        pc.wounds.death_save_penalty = 2;
        pc.wounds.current_state = WoundState::Seriously;
        recompute_wounds(&mut pc);

        assert_eq!(pc.wounds.death_save_penalty, 2);
        assert_eq!(pc.wounds.current_state, WoundState::Seriously);
    }

    /// Edge: a stacked-penalty stack that drives current BODY to 0
    /// or below clamps the Death Save base to 0. A 0-BODY character
    /// auto-fails any 1d10 Death Save — the right behaviour.
    #[test]
    fn test_death_save_base_clamps_at_zero_when_current_body_negative() {
        let mut pc = fresh_pc();
        pc.stats.body = 2;
        pc.effects.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(0xDC)),
            source: EffectSource::Cyberpsychosis,
            modifiers: vec![EffectModifier::StatPenalty {
                stat: Stat::Body,
                by: 5,
            }],
            duration: EffectDuration::Permanent,
        });
        assert_eq!(pc.current_body(), -3);
        assert_eq!(pc.calculate_death_save(), 0);
    }
}
