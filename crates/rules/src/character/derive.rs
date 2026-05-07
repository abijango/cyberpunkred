//! Current-value derivation queries on [`Character`].
//!
//! Base character data ([`crate::character::StatBlock`], [`SkillSet`])
//! is immutable post-creation; the *current* effective value of any stat or
//! skill is computed at query time by walking [`crate::effects::EffectStack`]
//! and applying the modifiers it carries. This file is the single home for
//! that derivation. There is no caching.
//!
//! Rulebook references:
//! - Stats: pp.72–73.
//! - Derived statistics (HP, Humanity / EMP relationship): pp.79–80.
//! - "When You Don't Have A Skill — STAT only": p.130.
//! - Wound-state movement penalty (clarifies floor): p.186.
//!
//! See `IMPLEMENTATION_PLAN.md` §2.6 — every transient adjustment lives in
//! the [`crate::effects::EffectStack`]; query sites such as the ones below
//! are the only place modifiers are *applied*.

use crate::catalog::skills::linked_stat;
use crate::character::Character;
use crate::effects::{EffectModifier, SkillId};
use crate::types::Stat;

impl Character {
    /// Current effective value of any [`Stat`] = base value (from
    /// [`Character::stats`]) + sum of [`EffectModifier::StatBonus`] − sum of
    /// [`EffectModifier::StatPenalty`] for matching `stat`.
    ///
    /// Returns `i16` because modifiers can in principle drive a stat below
    /// zero in pathological cases (multiple stacking penalties on a low
    /// base). Combat-resolution code is responsible for any clamping it
    /// needs at the point of use.
    ///
    /// `Stat::Move` and `Stat::Emp` have additional rules — see
    /// [`Self::current_move`] and [`Self::current_emp`]. Calling
    /// `current_stat(Stat::Move)` does *not* apply the [`MovePenalty`]
    /// modifier nor the floor; calling `current_stat(Stat::Emp)` returns the
    /// base stat-block EMP, not the Humanity-derived value. Use the
    /// dedicated accessors when you want the rulebook's full semantics.
    ///
    /// See pp.72–73.
    ///
    /// [`MovePenalty`]: crate::effects::EffectModifier::MovePenalty
    pub fn current_stat(&self, stat: Stat) -> i16 {
        let base = i16::from(self.base_stat_value(stat));
        let mut delta: i16 = 0;
        for m in self.effects.iter_modifiers() {
            match *m {
                EffectModifier::StatBonus { stat: s, by } if s == stat => {
                    delta += i16::from(by);
                }
                EffectModifier::StatPenalty { stat: s, by } if s == stat => {
                    delta -= i16::from(by);
                }
                _ => {}
            }
        }
        base + delta
    }

    /// Current INT. See p.72.
    pub fn current_int(&self) -> i16 {
        self.current_stat(Stat::Int)
    }

    /// Current REF. See p.72.
    pub fn current_ref(&self) -> i16 {
        self.current_stat(Stat::Ref)
    }

    /// Current DEX. See p.72.
    pub fn current_dex(&self) -> i16 {
        self.current_stat(Stat::Dex)
    }

    /// Current TECH. See p.72.
    pub fn current_tech(&self) -> i16 {
        self.current_stat(Stat::Tech)
    }

    /// Current COOL. See p.72.
    pub fn current_cool(&self) -> i16 {
        self.current_stat(Stat::Cool)
    }

    /// Current WILL. See p.73.
    pub fn current_will(&self) -> i16 {
        self.current_stat(Stat::Will)
    }

    /// Current LUCK. See p.73. This returns the *stat* (max LUCK pool
    /// size); spent LUCK is tracked separately on [`Character::luck_pool`].
    pub fn current_luck(&self) -> i16 {
        self.current_stat(Stat::Luck)
    }

    /// Current MOVE.
    ///
    /// Stat-level [`EffectModifier::StatBonus`] / [`EffectModifier::StatPenalty`]
    /// for `Stat::Move` are applied first (via [`Self::current_stat`]); the
    /// dedicated [`EffectModifier::MovePenalty`] (e.g. Mortally Wounded's
    /// `-6`, p.186) applies on top. Both kinds of modifier stack — they are
    /// not redundant.
    ///
    /// **Floor at 1.** A still-acting character has MOVE ≥ 1 in RAW: the
    /// Mortally Wounded entry on p.186 reads "MOVE −6" and the rules table
    /// implicitly assumes the character is still mobile (a Death Save is
    /// required, not auto-failure). The WP-104 acceptance test
    /// `test_move_floored_at_one` (base 5, −10 → 1) pins this choice. If the
    /// pre-floor value is already 0 or below (e.g. pathological stacking),
    /// we still report 1 here; the *Dead* state — which would set MOVE 0 —
    /// is conveyed by [`crate::effects::WoundState::Dead`] / a future
    /// `is_dead()` query, not by this number.
    pub fn current_move(&self) -> i16 {
        let base = self.current_stat(Stat::Move);
        let mut move_delta: i16 = 0;
        for m in self.effects.iter_modifiers() {
            if let EffectModifier::MovePenalty(by) = *m {
                // `by` is stored as a negative i8 by convention
                // (e.g. WoundState::Mortally → -6). Add it directly.
                move_delta += i16::from(by);
            }
        }
        let unfloored = base + move_delta;
        unfloored.max(1)
    }

    /// Current BODY. See p.73.
    pub fn current_body(&self) -> i16 {
        self.current_stat(Stat::Body)
    }

    /// Current EMP, derived from [`Character::humanity`] per p.80.
    ///
    /// `EMP = floor(max(humanity, 0) / 10)`. Page 80 is explicit: "a
    /// Character with 44 Humanity has an EMP of 4 until their Humanity is
    /// lowered to 39, at which point their EMP lowers to 3." Negative
    /// Humanity (cyberpsychosis territory, p.230) clamps EMP to 0; the
    /// cyberpsychosis state itself is modeled separately as a wound /
    /// effect concern, not as a negative EMP.
    ///
    /// Note: this deliberately ignores the base [`crate::character::StatBlock::emp`]
    /// field — that field tracks the *creation-time* EMP and is only useful
    /// as a max ceiling for future restoration mechanics. See WP-105 for
    /// max-EMP / max-HP derivation.
    pub fn current_emp(&self) -> i16 {
        // Use saturating arithmetic conceptually: Humanity is i16, dividing
        // a non-negative i16 by 10 always fits.
        let hum = self.humanity.max(0);
        hum / 10
    }

    /// Current effective rank for a skill = base rank (0 if the skill
    /// is not in [`Character::skills`]) + sum of
    /// [`EffectModifier::SkillBonus`] − sum of
    /// [`EffectModifier::SkillPenalty`] for matching `skill`.
    ///
    /// An unknown / untrained skill has base rank 0 (p.130: "When You Don't
    /// Have A Skill"). This method returns the *skill rank* alone, not the
    /// full check value — see [`Self::skill_base`] for STAT + skill.
    pub fn current_skill(&self, skill: &SkillId) -> i16 {
        let base = i16::from(self.skills.ranks.get(skill).copied().unwrap_or(0));
        let mut delta: i16 = 0;
        for m in self.effects.iter_modifiers() {
            match m {
                EffectModifier::SkillBonus { skill: s, by } if s == skill => {
                    delta += i16::from(*by);
                }
                EffectModifier::SkillPenalty { skill: s, by } if s == skill => {
                    delta -= i16::from(*by);
                }
                _ => {}
            }
        }
        base + delta
    }

    /// Skill base check value = `current_stat(linked_stat) + current_skill(skill)`.
    /// This is the value before adding 1d10 (or any DV modifiers).
    ///
    /// The linked stat is looked up via
    /// [`crate::catalog::skills::linked_stat`], which encodes the pp.81–84
    /// table. For an explicit-override (e.g. a campaign-specific Skill that
    /// the GM has linked to a different Stat) use
    /// [`Self::skill_base_with_stat`].
    ///
    /// See p.130 ("When You Don't Have A Skill — STAT only") for the
    /// untrained case.
    pub fn skill_base(&self, skill: &SkillId) -> i16 {
        self.skill_base_with_stat(skill, linked_stat(skill))
    }

    /// Skill base check value with an explicit linked stat.
    ///
    /// Most callers should use [`Self::skill_base`], which looks up the
    /// canonical linked Stat via [`crate::catalog::skills::linked_stat`].
    /// This helper is reserved for cases where the GM has explicitly
    /// re-linked a Skill (a rare, scenario-specific override).
    pub fn skill_base_with_stat(&self, skill: &SkillId, linked_stat: Stat) -> i16 {
        self.current_stat(linked_stat) + self.current_skill(skill)
    }

    /// Sum of [`EffectModifier::AllActionsPenalty`] across the effect stack.
    ///
    /// This is the modifier applied to *every* Action Check the character
    /// makes this turn — e.g. Seriously Wounded contributes `-2` (p.186).
    /// `HandActionsPenalty`, `MeleeAttackPenalty`, and `MovePenalty` are
    /// scoped narrower and are *not* summed here.
    ///
    /// Returned as `i8` for parity with the modifier's storage type;
    /// pathological stacks deeper than `i8::MIN` are not expected (the
    /// rulebook tops out at `-4`).
    pub fn all_actions_penalty(&self) -> i8 {
        let mut sum: i32 = 0;
        for m in self.effects.iter_modifiers() {
            if let EffectModifier::AllActionsPenalty(by) = *m {
                sum += i32::from(by);
            }
        }
        sum.clamp(i32::from(i8::MIN), i32::from(i8::MAX)) as i8
    }

    /// True if any active effect carries [`EffectModifier::CannotTakeAction`].
    /// e.g. a spinal injury preventing the next turn's actions (p.187).
    pub fn cannot_take_action(&self) -> bool {
        self.effects
            .iter_modifiers()
            .any(|m| matches!(m, EffectModifier::CannotTakeAction))
    }

    /// True if any active effect carries [`EffectModifier::CannotTakeMoveAction`].
    /// e.g. dismembered legs / prone state.
    pub fn cannot_take_move_action(&self) -> bool {
        self.effects
            .iter_modifiers()
            .any(|m| matches!(m, EffectModifier::CannotTakeMoveAction))
    }

    /// True if any active effect carries [`EffectModifier::CannotDodge`].
    /// e.g. dismembered leg, human-shielded — the character cannot use a
    /// Dodge reaction this round.
    pub fn cannot_dodge(&self) -> bool {
        self.effects
            .iter_modifiers()
            .any(|m| matches!(m, EffectModifier::CannotDodge))
    }

    /// Internal: project a [`Stat`] onto the corresponding base value from
    /// [`Character::stats`]. Kept private so the public surface is a single
    /// `current_stat` entry point.
    fn base_stat_value(&self, stat: Stat) -> u8 {
        match stat {
            Stat::Int => self.stats.int,
            Stat::Ref => self.stats.r#ref,
            Stat::Dex => self.stats.dex,
            Stat::Tech => self.stats.tech,
            Stat::Cool => self.stats.cool,
            Stat::Will => self.stats.will,
            Stat::Luck => self.stats.luck,
            Stat::Move => self.stats.r#move,
            Stat::Body => self.stats.body,
            Stat::Emp => self.stats.emp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::{ActiveEffect, EffectDuration, EffectSource, Hand, WoundState};
    use crate::types::EffectInstanceId;
    use crate::world::test_support::fresh_pc;
    use uuid::Uuid;

    fn id(n: u128) -> EffectInstanceId {
        EffectInstanceId(Uuid::from_u128(n))
    }

    /// Helper: push an effect carrying `modifiers` onto the PC's stack.
    /// `source` defaults to `Armor` since most fixtures here represent a
    /// gear-driven modifier; tests that care about source set it themselves.
    fn push_effect(
        pc: &mut Character,
        ord: u128,
        source: EffectSource,
        modifiers: Vec<EffectModifier>,
    ) {
        pc.effects.add(ActiveEffect {
            id: id(ord),
            source,
            modifiers,
            duration: EffectDuration::Permanent,
        });
    }

    #[test]
    fn test_current_dex_no_effects() {
        let pc = fresh_pc();
        // fresh_pc has DEX 6 and no effects → current = base.
        assert_eq!(pc.current_dex(), 6);
        assert_eq!(pc.current_stat(Stat::Dex), 6);
    }

    #[test]
    fn test_current_dex_with_armor_penalty() {
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::Armor,
            vec![EffectModifier::StatPenalty {
                stat: Stat::Dex,
                by: 2,
            }],
        );
        // base 6 - 2 = 4
        assert_eq!(pc.current_dex(), 4);
    }

    #[test]
    fn test_current_stat_combines_bonus_and_penalty() {
        // Sanity guard: a stacking bonus and penalty net out correctly.
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::Armor,
            vec![EffectModifier::StatPenalty {
                stat: Stat::Ref,
                by: 1,
            }],
        );
        push_effect(
            &mut pc,
            2,
            EffectSource::Cyberpsychosis,
            vec![EffectModifier::StatBonus {
                stat: Stat::Ref,
                by: 3,
            }],
        );
        // base 7, -1, +3 = 9
        assert_eq!(pc.current_ref(), 9);
    }

    #[test]
    fn test_move_floored_at_one() {
        // Acceptance: base MOVE 5 with `MovePenalty(-10)` → current_move == 1.
        let mut pc = fresh_pc();
        assert_eq!(pc.stats.r#move, 5);
        push_effect(
            &mut pc,
            1,
            EffectSource::WoundState(WoundState::Mortally),
            vec![EffectModifier::MovePenalty(-10)],
        );
        assert_eq!(pc.current_move(), 1);
    }

    #[test]
    fn test_move_applies_stat_and_move_penalties() {
        // Both StatPenalty{Move} and MovePenalty stack — they are NOT
        // redundant; the `EffectModifier` doc comment on `MovePenalty`
        // pins this distinction.
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::Armor,
            vec![EffectModifier::StatPenalty {
                stat: Stat::Move,
                by: 1,
            }],
        );
        push_effect(
            &mut pc,
            2,
            EffectSource::WoundState(WoundState::Mortally),
            vec![EffectModifier::MovePenalty(-2)],
        );
        // base 5 - 1 (stat) - 2 (move) = 2; not floored (positive).
        assert_eq!(pc.current_move(), 2);
    }

    #[test]
    fn test_emp_follows_humanity_tens() {
        // p.80: humanity 44 → EMP 4; humanity 39 → EMP 3.
        let mut pc = fresh_pc();
        pc.humanity = 44;
        assert_eq!(pc.current_emp(), 4);
        pc.humanity = 39;
        assert_eq!(pc.current_emp(), 3);
    }

    #[test]
    fn test_emp_floors_at_zero_for_negative_humanity() {
        let mut pc = fresh_pc();
        pc.humanity = -5;
        assert_eq!(pc.current_emp(), 0);
    }

    #[test]
    fn test_all_actions_sums_multiple_sources() {
        // Wound State (Seriously Wounded, p.186) contributes -2 via
        // AllActionsPenalty. A simultaneous HandActionsPenalty is NOT an
        // all-actions penalty and must NOT be summed in.
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::WoundState(WoundState::Seriously),
            vec![EffectModifier::AllActionsPenalty(-2)],
        );
        push_effect(
            &mut pc,
            2,
            EffectSource::CriticalInjury(crate::effects::CriticalInjuryKind::Placeholder(
                "crushed_fingers".into(),
            )),
            vec![EffectModifier::HandActionsPenalty {
                hand: Hand::Either,
                by: -2,
            }],
        );
        assert_eq!(pc.all_actions_penalty(), -2);
    }

    #[test]
    fn test_all_actions_sums_multiple_all_actions_modifiers() {
        // Two all-actions sources should add: e.g. Seriously Wounded -2
        // plus an environmental -1 = -3.
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::WoundState(WoundState::Seriously),
            vec![EffectModifier::AllActionsPenalty(-2)],
        );
        push_effect(
            &mut pc,
            2,
            EffectSource::Environmental(crate::effects::EnvironmentalKind::Smoke),
            vec![EffectModifier::AllActionsPenalty(-1)],
        );
        assert_eq!(pc.all_actions_penalty(), -3);
    }

    #[test]
    fn test_cannot_take_action_from_spinal_injury() {
        let mut pc = fresh_pc();
        // Absent → false.
        assert!(!pc.cannot_take_action());

        push_effect(
            &mut pc,
            1,
            EffectSource::CriticalInjury(crate::effects::CriticalInjuryKind::Placeholder(
                "spinal_injury".into(),
            )),
            vec![EffectModifier::CannotTakeAction],
        );
        assert!(pc.cannot_take_action());
    }

    #[test]
    fn test_cannot_take_move_and_dodge_default_false() {
        let pc = fresh_pc();
        assert!(!pc.cannot_take_move_action());
        assert!(!pc.cannot_dodge());
    }

    #[test]
    fn test_cannot_take_move_action_and_dodge_set() {
        let mut pc = fresh_pc();
        push_effect(
            &mut pc,
            1,
            EffectSource::CriticalInjury(crate::effects::CriticalInjuryKind::Placeholder(
                "dismembered_leg".into(),
            )),
            vec![
                EffectModifier::CannotTakeMoveAction,
                EffectModifier::CannotDodge,
            ],
        );
        assert!(pc.cannot_take_move_action());
        assert!(pc.cannot_dodge());
    }

    #[test]
    fn test_current_skill_no_rank_returns_zero() {
        let pc = fresh_pc();
        // fresh_pc has SkillSet::default() — no entries.
        assert_eq!(pc.current_skill(&SkillId::Handgun), 0);
    }

    #[test]
    fn test_current_skill_with_bonus() {
        let mut pc = fresh_pc();
        let handgun = SkillId::Handgun;
        pc.skills.ranks.insert(handgun.clone(), 4);
        push_effect(
            &mut pc,
            1,
            EffectSource::Cyberware(crate::effects::CyberwareId("smartlink".into())),
            vec![EffectModifier::SkillBonus {
                skill: handgun.clone(),
                by: 1,
            }],
        );
        assert_eq!(pc.current_skill(&handgun), 5);
    }

    #[test]
    fn test_current_skill_with_penalty_and_unrelated_modifier() {
        // SkillPenalty for the same skill subtracts; a SkillBonus on a
        // *different* skill must not leak into this query.
        let mut pc = fresh_pc();
        let stealth = SkillId::Stealth;
        let other = SkillId::Brawling;
        pc.skills.ranks.insert(stealth.clone(), 3);
        push_effect(
            &mut pc,
            1,
            EffectSource::Armor,
            vec![EffectModifier::SkillPenalty {
                skill: stealth.clone(),
                by: 2,
            }],
        );
        push_effect(
            &mut pc,
            2,
            EffectSource::Drug(crate::effects::DrugId("synthcoke".into())),
            vec![EffectModifier::SkillBonus {
                skill: other.clone(),
                by: 5,
            }],
        );
        assert_eq!(pc.current_skill(&stealth), 1);
        // And the unrelated skill picks up its own bonus from base 0.
        assert_eq!(pc.current_skill(&other), 5);
    }

    #[test]
    fn test_skill_base_uses_catalog_linked_stat() {
        // Post-WP-201: skill_base() looks up the canonical linked Stat
        // via crate::catalog::skills::linked_stat. Education is INT-linked
        // (p.83). fresh_pc has INT 5; rank 4 → 5 + 4 = 9.
        let mut pc = fresh_pc();
        let s = SkillId::Education;
        pc.skills.ranks.insert(s.clone(), 4);
        assert_eq!(pc.skill_base(&s), 9);
    }

    #[test]
    fn test_skill_base_handgun_uses_ref() {
        // Regression: post-WP-201, Handgun is REF-linked (p.84) and
        // skill_base() picks that up automatically — callers no longer
        // need skill_base_with_stat for the canonical case. fresh_pc has
        // REF 7; rank 4 → 7 + 4 = 11.
        let mut pc = fresh_pc();
        let s = SkillId::Handgun;
        pc.skills.ranks.insert(s.clone(), 4);
        assert_eq!(pc.skill_base(&s), 11);
    }

    #[test]
    fn test_skill_base_with_stat_passes_explicit_link() {
        // skill_base_with_stat is reserved for explicit-override cases.
        // Verify that an arbitrary override Stat is honoured.
        let mut pc = fresh_pc();
        let s = SkillId::Handgun;
        pc.skills.ranks.insert(s.clone(), 4);
        // Forced INT-link (override): fresh_pc INT 5 + rank 4 = 9.
        assert_eq!(pc.skill_base_with_stat(&s, Stat::Int), 9);
    }

    #[test]
    fn test_current_emp_ignores_base_emp_field() {
        // Per WP-104 notes: current_emp derives from humanity, NOT from
        // stats.emp. fresh_pc has emp=5, humanity=50; if we drop humanity
        // to 19 the EMP should be 1 even though the base field is still 5.
        let mut pc = fresh_pc();
        assert_eq!(pc.stats.emp, 5);
        pc.humanity = 19;
        assert_eq!(pc.current_emp(), 1);
    }

    #[test]
    fn test_current_stat_each_variant_unwraps_correctly() {
        // Regression guard: every Stat variant must read the right
        // StatBlock field. `r#ref` and `r#move` use raw identifiers in
        // StatBlock — easy to typo.
        let pc = fresh_pc();
        assert_eq!(pc.current_stat(Stat::Int), i16::from(pc.stats.int));
        assert_eq!(pc.current_stat(Stat::Ref), i16::from(pc.stats.r#ref));
        assert_eq!(pc.current_stat(Stat::Dex), i16::from(pc.stats.dex));
        assert_eq!(pc.current_stat(Stat::Tech), i16::from(pc.stats.tech));
        assert_eq!(pc.current_stat(Stat::Cool), i16::from(pc.stats.cool));
        assert_eq!(pc.current_stat(Stat::Will), i16::from(pc.stats.will));
        assert_eq!(pc.current_stat(Stat::Luck), i16::from(pc.stats.luck));
        assert_eq!(pc.current_stat(Stat::Move), i16::from(pc.stats.r#move));
        assert_eq!(pc.current_stat(Stat::Body), i16::from(pc.stats.body));
        assert_eq!(pc.current_stat(Stat::Emp), i16::from(pc.stats.emp));
    }
}
