//! Wound-state transitions and Death Saves for [`Character`].
//!
//! Two responsibilities live here:
//!
//! 1. [`Character::update_wound_state`] — recomputes the current
//!    [`crate::effects::WoundState`] from `wounds.current_hp` and re-syncs
//!    the matching [`crate::effects::ActiveEffect`] on the character's
//!    [`crate::effects::EffectStack`]. Removes the previous wound-state
//!    effect (if any) before installing the new one. Returns the new state
//!    if it differs from the cached `wounds.current_state`.
//!
//! 2. [`Character::roll_death_save`] — performs one Death Save per p.186 /
//!    p.188 (rule wording is on p.188 but the table is on p.186). On
//!    failure, transitions the character to [`crate::effects::WoundState::Dead`]
//!    and removes the Mortally-Wounded effect.
//!
//! ## Rulebook references
//! - p.186 — Wound States table, Mortally Wounded line ("…their **Death
//!   Save Penalty** increases by 1.").
//! - p.188 — DEATH SAVES: "Roll a d10. If you roll under your BODY, you
//!   live… If you roll a 10, you automatically fail your Death Save. Every
//!   time you roll a Death Save, your Death Save Penalty increases,
//!   meaning each future Death Save you roll is made with an additional
//!   +1, making it progressively harder to stave off death."
//!
//! ## Deviations from the WP-106 spec (flagged in PR)
//!
//! The WP description says "death-save target = `base + penalty`, roll
//! `<= target` → survived." This is **not** what RAW says. p.188 has the
//! penalty add to the **d10 roll**, not to the target, and survival is
//! strict (`roll < BODY` after adding the penalty), with a natural 10 as an
//! automatic fail. We implement RAW. The semantic difference matters:
//!
//! - With WP wording (`roll <= target`, target = base + penalty), a
//!   character with BODY 6 / penalty 0 lives on a roll of 1..=6 and dies on
//!   7..=10 — survival probability 60%. RAW with the same character: lives
//!   on 1..=5 (`roll < 6`) and the natural 10 is an auto-fail, so survival
//!   probability is **50%**.
//! - With WP wording, a higher penalty makes survival *easier* (the target
//!   grows). With RAW, a higher penalty makes survival *harder* (the
//!   effective roll grows). RAW matches the rulebook prose ("…progressively
//!   harder to stave off death.").
//!
//! The [`DeathSaveOutcome`] payload still carries `target` for caller
//! ergonomics, but `target` is now the character's BODY (i.e.
//! `wounds.death_save_base`), not `base + penalty`. The penalty is added to
//! `roll` for the comparison.
//!
//! ## Per-turn vs per-event penalty increment
//!
//! p.186 ("Mortally Wounded Characters suffer a Critical Injury whenever
//! they are damaged by a Melee or Ranged Attack. In addition, their Death
//! Save Penalty increases by 1.") and p.188 ("Every time you roll a Death
//! Save, your Death Save Penalty increases…") are two **separate**
//! penalty-increment triggers:
//!
//! 1. **Per damage event while Mortally Wounded** — handled outside this
//!    module, by the damage pipeline (WP-303). Each Critical Injury also
//!    bumps `death_save_base` via [`crate::effects::EffectModifier::DeathSavePenaltyDelta`]
//!    on the injury's effect.
//! 2. **Per Death Save attempt** — handled here, in `roll_death_save`,
//!    incrementing `wounds.death_save_penalty` after each roll.
//!
//! These do not double-count: the damage-event bump happens in the damage
//! pipeline, the per-roll bump happens here.
//!
//! ## Effect-id stability
//!
//! Each wound-state effect needs a stable [`crate::types::EffectInstanceId`]
//! so the housekeeping path can find and remove the prior effect. The crate
//! disallows OS entropy (no `Uuid::new_v4`) and no [`crate::types::CharacterId`]
//! is plumbed in (the WP signature does not take one). We therefore mint a
//! deterministic UUID derived from the wound state itself. This is safe
//! because **at most one wound-state effect is ever active on a given
//! character at a time** — the housekeeping path drops the old effect
//! before adding the new one. The id never escapes the character; it is
//! consumed by [`crate::effects::EffectStack::remove`] on the same
//! character. See `wound_effect_id` for the bit layout.

use crate::character::Character;
use crate::dice::d10;
use crate::effects::{ActiveEffect, EffectDuration, EffectModifier, EffectSource, WoundState};
use crate::rng::Rng;
use crate::types::EffectInstanceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Result of one [`Character::roll_death_save`].
///
/// `target` is the character's BODY (i.e. `wounds.death_save_base` at the
/// moment of the roll). To win, `(roll + pre_roll_penalty) < target`. A
/// natural 10 is always a fail per p.188, regardless of `target`. The
/// payload carries the un-modified d10 face (`roll`) and the BODY target
/// for narration / replay.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeathSaveOutcome {
    /// The character lived through this Death Save. They take their Turn
    /// as usual (per p.188). The penalty has still been incremented.
    Survived {
        /// The d10 face value (`1..=10`).
        roll: u8,
        /// The character's BODY at the moment of the roll.
        target: u8,
    },
    /// The character died. The character's [`crate::effects::WoundState`]
    /// is now [`WoundState::Dead`].
    Died {
        /// The d10 face value (`1..=10`).
        roll: u8,
        /// The character's BODY at the moment of the roll.
        target: u8,
    },
}

impl Character {
    /// Recompute the current wound state from `wounds.current_hp` /
    /// `wounds.max_hp` / `wounds.seriously_wounded_threshold` and re-sync
    /// the matching effect on [`Character::effects`].
    ///
    /// Returns `Some(new_state)` if the state changed,
    /// `None` if it was unchanged.
    ///
    /// Transitions follow p.186:
    ///
    /// | HP                                            | State        | Effect                                         |
    /// |-----------------------------------------------|--------------|------------------------------------------------|
    /// | `current_hp == max_hp`                        | `None`       | none                                           |
    /// | `0 < current_hp < max_hp` and `> threshold`   | `Lightly`    | none (no all-actions penalty per p.186)        |
    /// | `0 < current_hp <= threshold`                 | `Seriously`  | `AllActionsPenalty(-2)`                        |
    /// | `current_hp <= 0` (when not already `Dead`)   | `Mortally`   | `[AllActionsPenalty(-4), MovePenalty(-6)]`     |
    ///
    /// Once the character is [`WoundState::Dead`], this method is a no-op:
    /// healing back to full HP does **not** revive. (Resurrection is a
    /// downstream concern — see WP-303.)
    ///
    /// Effect housekeeping: any prior `EffectSource::WoundState(_)` effects
    /// are removed before the new one (if any) is added. The wound effect's
    /// id is deterministic in the wound state — see [module docs].
    ///
    /// See p.186.
    pub fn update_wound_state(&mut self) -> Option<WoundState> {
        // Dead is terminal. WP-303 will introduce resurrection if ever.
        if self.wounds.current_state == WoundState::Dead {
            return None;
        }

        let new_state = compute_wound_state(
            self.wounds.current_hp,
            self.wounds.max_hp,
            self.wounds.seriously_wounded_threshold,
        );

        // Always re-sync the effect stack: callers may have edited
        // `current_state` directly without going through us, and the
        // invariant we promise is "after update_wound_state, the stack
        // matches the computed state."
        sync_wound_effect(self, new_state);

        if new_state == self.wounds.current_state {
            None
        } else {
            self.wounds.current_state = new_state;
            Some(new_state)
        }
    }

    /// Roll one Death Save per p.188.
    ///
    /// Procedure:
    ///
    /// 1. Read `target = wounds.death_save_base` (the character's BODY at
    ///    the time of the roll, populated by WP-105's `recompute_wounds`).
    /// 2. Roll a d10 (no crit explosion — Death Saves are simple d10s per
    ///    p.188).
    /// 3. Compare `(roll + wounds.death_save_penalty) < target`. If yes →
    ///    [`DeathSaveOutcome::Survived`]. If no → [`DeathSaveOutcome::Died`]
    ///    and the character transitions to [`WoundState::Dead`].
    /// 4. **Always** a natural 10 fails (p.188 explicit).
    /// 5. **Always** increment `wounds.death_save_penalty` by 1 after the
    ///    roll (p.188: "Every time you roll a Death Save, your Death Save
    ///    Penalty increases…"), saturating at `u8::MAX`.
    ///
    /// On a failed save, the Mortally Wounded effect is removed and a Dead
    /// effect (no modifiers — Dead is a state, not a modifier set) is
    /// installed. `wounds.current_state` becomes [`WoundState::Dead`].
    ///
    /// The caller is responsible for guarding "is this character actually
    /// Mortally Wounded?". The method does not check — that policy belongs
    /// to the combat / turn machinery (WP-303).
    ///
    /// See p.186, p.188.
    pub fn roll_death_save(&mut self, rng: &mut Rng) -> DeathSaveOutcome {
        let target = self.wounds.death_save_base;
        let penalty = self.wounds.death_save_penalty;
        let roll = d10(rng);

        // Compute survival in u16 to avoid u8 overflow when penalty is high.
        let modified = u16::from(roll) + u16::from(penalty);
        // p.188: a natural 10 is always a fail, even if BODY is high enough
        // that 10 + penalty would otherwise be under target.
        let survived = roll != 10 && modified < u16::from(target);

        // Per p.188: every Death Save attempt bumps the penalty.
        self.wounds.death_save_penalty = self.wounds.death_save_penalty.saturating_add(1);

        if survived {
            DeathSaveOutcome::Survived { roll, target }
        } else {
            // Transition to Dead and re-sync the effect stack.
            self.wounds.current_state = WoundState::Dead;
            sync_wound_effect(self, WoundState::Dead);
            DeathSaveOutcome::Died { roll, target }
        }
    }
}

/// Determine the wound state from current HP and the cached thresholds.
///
/// Pure function; takes the three relevant numbers so it can be unit-tested
/// without constructing a [`Character`]. The caller (`update_wound_state`)
/// is responsible for the "Dead is terminal" override.
fn compute_wound_state(current_hp: i16, max_hp: u16, threshold: u16) -> WoundState {
    if current_hp <= 0 {
        return WoundState::Mortally;
    }
    // current_hp > 0 here, so casting to u16 is safe.
    let hp = current_hp as u16;
    if hp >= max_hp {
        WoundState::None
    } else if hp <= threshold {
        // p.186 "Less than 1/2 HP (round up)" — `seriously_wounded_threshold`
        // is precomputed by WP-105 as `ceil(max_hp/2)`. The book reads
        // "Less than" but the threshold rounds *up*, which makes the
        // boundary inclusive: HP at the threshold is Seriously Wounded.
        // (Worked example: max_hp 35 → threshold 18; HP 18 is Seriously,
        // HP 17 is also Seriously. WP-105's `test_seriously_wounded_rounds_up`
        // and the WP-106 acceptance `test_seriously_at_half` both pin this.)
        WoundState::Seriously
    } else {
        WoundState::Lightly
    }
}

/// Replace any existing `EffectSource::WoundState(_)` effect on the
/// character's stack with the canonical effect for `state`.
///
/// `WoundState::None` and `WoundState::Lightly` install no effect; they
/// just clear any previous one. `WoundState::Dead` installs an
/// id-bearing marker with no modifiers — useful so downstream code can
/// detect the Dead state via the stack as well as the wound flag.
fn sync_wound_effect(character: &mut Character, state: WoundState) {
    // Drop every existing wound-state-sourced effect. There should be at
    // most one in practice, but be defensive: a corrupted save or a
    // double-add elsewhere should not leak modifiers.
    character
        .effects
        .effects
        .retain(|e| !matches!(e.source, EffectSource::WoundState(_)));

    if let Some(effect) = wound_state_effect(state) {
        character.effects.add(effect);
    }
}

/// Build the canonical [`ActiveEffect`] for a given wound state, or
/// `None` for states that have no associated effect (`None` and `Lightly`).
///
/// See p.186 wound-state table for the modifier values.
fn wound_state_effect(state: WoundState) -> Option<ActiveEffect> {
    let modifiers: Vec<EffectModifier> = match state {
        WoundState::None | WoundState::Lightly => return None,
        WoundState::Seriously => vec![EffectModifier::AllActionsPenalty(-2)],
        WoundState::Mortally => vec![
            EffectModifier::AllActionsPenalty(-4),
            EffectModifier::MovePenalty(-6),
        ],
        // Dead: no modifiers — the Dead state is consulted via
        // `wounds.current_state`, not via the modifier list. We still mint
        // an effect so downstream code that scans by `EffectSource` can
        // find it.
        WoundState::Dead => Vec::new(),
    };

    Some(ActiveEffect {
        id: wound_effect_id(state),
        source: EffectSource::WoundState(state),
        modifiers,
        duration: EffectDuration::Permanent,
    })
}

/// Deterministic [`EffectInstanceId`] for the wound-state effect of `state`.
///
/// Each wound state gets a fixed UUID. The id only needs to be stable
/// within a single character's stack lifetime, because the housekeeping
/// path drops the prior wound-state effect before adding a new one. Using
/// a deterministic id keeps wound transitions reproducible from a seed —
/// part of the project's replay-determinism contract.
///
/// The high 64 bits carry a magic marker (`0xC0DE_C0DE_0000_0000`) so a
/// human reading a save file can tell at a glance that an EffectInstanceId
/// originated from this module. The low 64 bits encode the `WoundState`
/// discriminant.
fn wound_effect_id(state: WoundState) -> EffectInstanceId {
    // We don't `repr(u8)` `WoundState`, so cast through a manual mapping.
    let disc: u128 = match state {
        WoundState::None => 0,
        WoundState::Lightly => 1,
        WoundState::Seriously => 2,
        WoundState::Mortally => 3,
        WoundState::Dead => 4,
    };
    const MARKER: u128 = 0xC0DE_C0DE_0000_0000_0000_0000_0000_0000;
    EffectInstanceId(Uuid::from_u128(MARKER | disc))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::EffectModifier;
    use crate::world::test_support::fresh_pc;
    use rand::SeedableRng;

    /// Walk seeds in order until we find one whose initial RNG state
    /// satisfies `pred`. Mirrors the helper in `dice::tests`.
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

    /// Helper: configure a PC's wound bookkeeping for a max_hp / threshold
    /// pair, matching the WP-105 invariants without re-running the full
    /// recompute. Caller sets `current_hp` afterward.
    fn pc_with_max_hp(max_hp: u16, body: u8) -> Character {
        let mut pc = fresh_pc();
        // Recompute through WP-105 to keep the relationship consistent.
        // Pick BODY/WILL such that calculate_max_hp() gives `max_hp`. We
        // only need the threshold to be ceil(max_hp/2); the easiest way is
        // to bypass `calculate_max_hp` and set the fields directly.
        pc.stats.body = body;
        pc.wounds.max_hp = max_hp;
        pc.wounds.seriously_wounded_threshold = max_hp.div_ceil(2);
        pc.wounds.death_save_base = body;
        pc.wounds.current_hp = max_hp as i16;
        pc.wounds.current_state = WoundState::None;
        pc
    }

    /// Acceptance: HP 29 / max 30 → Lightly Wounded with no penalty effect.
    /// p.186: Lightly Wounded's wound effect column reads "None".
    #[test]
    fn test_lightly_at_less_than_full_no_penalty() {
        let mut pc = pc_with_max_hp(30, 6);
        // threshold = ceil(30/2) = 15. HP 29 > threshold → Lightly.
        pc.wounds.current_hp = 29;
        let new_state = pc.update_wound_state();

        assert_eq!(new_state, Some(WoundState::Lightly));
        assert_eq!(pc.wounds.current_state, WoundState::Lightly);
        // No -2 / -4 penalty effect is installed for Lightly.
        assert_eq!(pc.all_actions_penalty(), 0);
        // No wound-state effect on the stack at all (Lightly has no effect).
        assert!(
            !pc.effects
                .iter()
                .any(|e| matches!(e.source, EffectSource::WoundState(_))),
            "Lightly Wounded must not install a wound-state effect"
        );
    }

    /// Acceptance: HP at the Seriously Wounded threshold → Seriously,
    /// `-2 to all Actions`, and the `-4` Mortally penalty is NOT yet
    /// applied. p.186 wound table.
    #[test]
    fn test_seriously_at_half() {
        let mut pc = pc_with_max_hp(30, 6);
        // threshold = 15.
        pc.wounds.current_hp = 15;
        let new_state = pc.update_wound_state();

        assert_eq!(new_state, Some(WoundState::Seriously));
        assert_eq!(pc.all_actions_penalty(), -2);
        // Specifically: no -4 (i.e. not Mortally) yet.
        let has_minus_four = pc
            .effects
            .iter_modifiers()
            .any(|m| matches!(m, EffectModifier::AllActionsPenalty(-4)));
        assert!(!has_minus_four, "Seriously must not stack the -4 penalty");
    }

    /// Acceptance: HP=0 → Mortally Wounded, `-4` to all Actions, `-6` to
    /// MOVE. p.186.
    #[test]
    fn test_mortally_at_zero() {
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        let new_state = pc.update_wound_state();

        assert_eq!(new_state, Some(WoundState::Mortally));
        assert_eq!(pc.all_actions_penalty(), -4);
        // current_move() applies the MovePenalty(-6) on top of the base
        // MOVE; fresh_pc has MOVE 5, so 5 - 6 = -1, floored to 1.
        assert_eq!(pc.current_move(), 1);
    }

    /// Acceptance: from Mortally, force a failed Death Save via seed
    /// search; state becomes Dead; the Mortally effect is removed.
    #[test]
    fn test_mortally_to_dead() {
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        pc.update_wound_state();
        assert_eq!(pc.wounds.current_state, WoundState::Mortally);

        // Find a seed whose first d10 is a 10 (auto-fail per p.188).
        let seed = find_seed_where(|r| d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);

        let outcome = pc.roll_death_save(&mut rng);
        assert!(matches!(outcome, DeathSaveOutcome::Died { .. }));

        assert_eq!(pc.wounds.current_state, WoundState::Dead);
        // The Mortally effect is gone.
        let still_mortally = pc
            .effects
            .iter()
            .any(|e| matches!(e.source, EffectSource::WoundState(WoundState::Mortally)));
        assert!(!still_mortally, "Mortally effect must be removed on death");
        // current_move(): no MovePenalty active (Dead has no modifiers),
        // so MOVE returns the base — 5.
        assert_eq!(pc.current_move(), 5);
        // No -4 active either.
        assert_eq!(pc.all_actions_penalty(), 0);
    }

    /// Acceptance: each call to `roll_death_save` increments
    /// `wounds.death_save_penalty` by 1, per p.188 ("Every time you roll
    /// a Death Save, your Death Save Penalty increases…").
    #[test]
    fn test_death_save_penalty_increments() {
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        pc.update_wound_state();

        // Use a seed where the first roll is a 1 — guarantees survival,
        // so the test exercises increment-on-survive across multiple calls.
        let seed = find_seed_where(|r| d10(r) == 1);
        let mut rng = Rng::seed_from_u64(seed);

        let starting = pc.wounds.death_save_penalty;
        let _ = pc.roll_death_save(&mut rng);
        assert_eq!(pc.wounds.death_save_penalty, starting + 1);

        // Force a survive again — penalty rises a second time. Pick a
        // fresh seed each time so we know the d10 value.
        let seed2 = find_seed_where(|r| d10(r) == 1);
        let mut rng2 = Rng::seed_from_u64(seed2);
        let _ = pc.roll_death_save(&mut rng2);
        assert_eq!(pc.wounds.death_save_penalty, starting + 2);
    }

    /// Acceptance: moving Lightly → Seriously leaves exactly one
    /// `WoundState`-sourced effect on the stack. (Lightly has no effect,
    /// so the precondition has zero. After the transition there is one
    /// — the Seriously -2.)
    #[test]
    fn test_state_replaces_previous() {
        let mut pc = pc_with_max_hp(30, 6);
        // First, Lightly.
        pc.wounds.current_hp = 29;
        pc.update_wound_state();
        assert_eq!(pc.wounds.current_state, WoundState::Lightly);

        // Then, Seriously.
        pc.wounds.current_hp = 10;
        pc.update_wound_state();
        assert_eq!(pc.wounds.current_state, WoundState::Seriously);

        let wound_effects = pc
            .effects
            .iter()
            .filter(|e| matches!(e.source, EffectSource::WoundState(_)))
            .count();
        assert_eq!(
            wound_effects, 1,
            "exactly one wound-state effect after the transition"
        );

        // And: a transition Seriously → Mortally also leaves exactly one.
        pc.wounds.current_hp = 0;
        pc.update_wound_state();
        let wound_effects = pc
            .effects
            .iter()
            .filter(|e| matches!(e.source, EffectSource::WoundState(_)))
            .count();
        assert_eq!(
            wound_effects, 1,
            "Seriously → Mortally also drops the prior"
        );
    }

    /// Regression: HP == max → state None, no wound-state effect.
    #[test]
    fn test_full_hp_no_state_no_effect() {
        let mut pc = pc_with_max_hp(30, 6);
        // current_hp already == max via pc_with_max_hp.
        let result = pc.update_wound_state();
        // No transition on first call (state was already None).
        assert_eq!(result, None);
        assert_eq!(pc.wounds.current_state, WoundState::None);
        let any_wound = pc
            .effects
            .iter()
            .any(|e| matches!(e.source, EffectSource::WoundState(_)));
        assert!(!any_wound, "no wound-state effect at full HP");
    }

    /// Regression: Dead is terminal. Healing back to full HP after death
    /// does NOT revive. Resurrection is WP-303's concern.
    #[test]
    fn test_dead_is_terminal() {
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        pc.update_wound_state();

        // Force a fail.
        let seed = find_seed_where(|r| d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        let outcome = pc.roll_death_save(&mut rng);
        assert!(matches!(outcome, DeathSaveOutcome::Died { .. }));
        assert_eq!(pc.wounds.current_state, WoundState::Dead);

        // Now "heal" back to full HP and recompute.
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        let result = pc.update_wound_state();
        assert_eq!(result, None, "Dead is terminal; no transition");
        assert_eq!(pc.wounds.current_state, WoundState::Dead);
    }

    /// Regression: a *survived* Death Save must not transition out of
    /// Mortally Wounded. Survivors stay Mortally per p.188 — they take
    /// their Turn but they are still Mortally Wounded until Stabilization.
    #[test]
    fn test_death_save_survived_keeps_mortally() {
        let mut pc = pc_with_max_hp(30, 8); // BODY 8 → easy to survive.
        pc.wounds.current_hp = 0;
        pc.update_wound_state();
        assert_eq!(pc.wounds.current_state, WoundState::Mortally);

        // Seed: a roll of 1, which always survives at any reasonable BODY.
        let seed = find_seed_where(|r| d10(r) == 1);
        let mut rng = Rng::seed_from_u64(seed);
        let outcome = pc.roll_death_save(&mut rng);
        assert!(
            matches!(outcome, DeathSaveOutcome::Survived { .. }),
            "BODY 8, roll 1 → must survive"
        );

        // Still Mortally — both as a flag and as an effect.
        assert_eq!(pc.wounds.current_state, WoundState::Mortally);
        assert_eq!(pc.all_actions_penalty(), -4);
        let still_mortally = pc
            .effects
            .iter()
            .any(|e| matches!(e.source, EffectSource::WoundState(WoundState::Mortally)));
        assert!(
            still_mortally,
            "Mortally effect persists on a survived save"
        );
    }

    // ---- Auxiliary regression / RAW-direction guards ---------------------

    /// Survival math: a roll of `BODY - 1 - penalty` lives,
    /// `BODY - penalty` dies. RAW: `(roll + penalty) < BODY`.
    #[test]
    fn test_death_save_survival_boundary() {
        let mut pc = pc_with_max_hp(30, 6); // BODY = 6.
        pc.wounds.current_hp = 0;
        pc.update_wound_state();
        // penalty starts at 0 → boundary is roll 5 lives, roll 6 dies.

        // Find a seed where the first d10 is exactly 5.
        let seed_5 = find_seed_where(|r| d10(r) == 5);
        let mut rng_5 = Rng::seed_from_u64(seed_5);
        let outcome_5 = pc.roll_death_save(&mut rng_5);
        assert!(matches!(
            outcome_5,
            DeathSaveOutcome::Survived { roll: 5, target: 6 }
        ));

        // After that survive, penalty == 1. Reset state for the second
        // roll: re-clear penalty to keep the test independent.
        let mut pc2 = pc_with_max_hp(30, 6);
        pc2.wounds.current_hp = 0;
        pc2.update_wound_state();
        let seed_6 = find_seed_where(|r| d10(r) == 6);
        let mut rng_6 = Rng::seed_from_u64(seed_6);
        let outcome_6 = pc2.roll_death_save(&mut rng_6);
        assert!(matches!(
            outcome_6,
            DeathSaveOutcome::Died { roll: 6, target: 6 }
        ));
        assert_eq!(pc2.wounds.current_state, WoundState::Dead);
    }

    /// p.188: "If you roll a 10, you automatically fail your Death Save."
    /// Even a character with BODY high enough that 10+penalty would
    /// otherwise be under target must still die on a natural 10.
    #[test]
    fn test_natural_ten_auto_fails_even_with_high_body() {
        // Pathological BODY = 12: 10 < 12, so without the auto-fail rule
        // the character would always live. With it, they die.
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        pc.update_wound_state();
        // Override death_save_base to 12 (we test the rule, not the
        // BODY-shaped clamp from WP-105).
        pc.wounds.death_save_base = 12;

        let seed = find_seed_where(|r| d10(r) == 10);
        let mut rng = Rng::seed_from_u64(seed);
        let outcome = pc.roll_death_save(&mut rng);
        assert!(matches!(
            outcome,
            DeathSaveOutcome::Died {
                roll: 10,
                target: 12
            }
        ));
    }

    /// The penalty makes survival *harder*, not easier (RAW direction).
    /// A character with BODY 6 / penalty 0 lives on a roll of 5; the same
    /// character with penalty 1 dies on a roll of 5 (5 + 1 == 6, not <
    /// BODY).
    #[test]
    fn test_penalty_makes_survival_harder() {
        let mut pc = pc_with_max_hp(30, 6);
        pc.wounds.current_hp = 0;
        pc.update_wound_state();

        // Pre-load the penalty.
        pc.wounds.death_save_penalty = 1;

        let seed_5 = find_seed_where(|r| d10(r) == 5);
        let mut rng_5 = Rng::seed_from_u64(seed_5);
        let outcome = pc.roll_death_save(&mut rng_5);
        // 5 + 1 == 6, not strictly less than BODY 6 → Died.
        assert!(matches!(
            outcome,
            DeathSaveOutcome::Died { roll: 5, target: 6 }
        ));
    }

    /// Regression on the deterministic id helper: the same wound state
    /// always yields the same id; different states yield different ids.
    #[test]
    fn test_wound_effect_id_deterministic_and_unique() {
        let states = [
            WoundState::None,
            WoundState::Lightly,
            WoundState::Seriously,
            WoundState::Mortally,
            WoundState::Dead,
        ];
        for s in states {
            assert_eq!(wound_effect_id(s), wound_effect_id(s));
        }
        // Pairwise unique.
        for (i, a) in states.iter().enumerate() {
            for b in states.iter().skip(i + 1) {
                assert_ne!(wound_effect_id(*a), wound_effect_id(*b));
            }
        }
    }

    /// Defensive: if a save somehow contains two wound-state effects, the
    /// next `update_wound_state` collapses them. (Belt-and-braces test —
    /// the production add path will never produce this.)
    #[test]
    fn test_update_wound_state_collapses_duplicate_wound_effects() {
        let mut pc = pc_with_max_hp(30, 6);
        // Manually wedge two wound-state effects in.
        pc.effects.add(ActiveEffect {
            id: wound_effect_id(WoundState::Seriously),
            source: EffectSource::WoundState(WoundState::Seriously),
            modifiers: vec![EffectModifier::AllActionsPenalty(-2)],
            duration: EffectDuration::Permanent,
        });
        pc.effects.add(ActiveEffect {
            id: wound_effect_id(WoundState::Lightly),
            source: EffectSource::WoundState(WoundState::Lightly),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });
        // HP back to full → expected state None.
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        pc.update_wound_state();

        let count = pc
            .effects
            .iter()
            .filter(|e| matches!(e.source, EffectSource::WoundState(_)))
            .count();
        assert_eq!(count, 0);
    }
}
