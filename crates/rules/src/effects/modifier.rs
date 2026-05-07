//! The closed enum of every modifier the rules engine recognises.
//!
//! See `IMPLEMENTATION_PLAN.md` §2.6: this is the single chokepoint where
//! rules drift would creep in. Every new way the rules can change a query
//! result becomes a variant here. New variants are added carefully and
//! reviewed.

use crate::catalog::programs::BoostableCheck;
use crate::effects::SkillId;
use crate::types::Stat;
use serde::{Deserialize, Serialize};

/// A single, atomic change to some queried game value.
///
/// `EffectModifier` does **not** apply itself — it is data. Application is
/// the job of the query site (e.g. `character.current_dex()`, the combat
/// engine's autofire DV calculation, etc.). Effect-application code iterates
/// `EffectStack::iter_modifiers()` and matches.
///
/// The variants are grouped roughly by who consumes them:
///
/// - **Stat / skill query consumers:** `StatPenalty`, `StatBonus`,
///   `SkillPenalty`, `SkillBonus`.
/// - **Action-cost consumers (combat engine):** `AllActionsPenalty`,
///   `MovePenalty`, `MeleeAttackPenalty`, `HandActionsPenalty`,
///   `CannotTakeAction`, `CannotTakeMoveAction`, `CannotDodge`,
///   `AutofireDvDelta`, `InitiativeBonus`.
/// - **Lifecycle-event consumers (combat engine hook points,
///   §2.6 reverse-coupling):** `DamageOnMovementOver`, `DamagePerTurn`.
/// - **Death-save consumers:** `DeathSavePenaltyDelta`.
/// - **Netrunning consumers (WP-412):** `NetrunCheckBonus`,
///   `NetrunBrainDamageReduction`, `NetrunAttackerAtkNullified`,
///   `NetrunFirstEffectBlocked`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectModifier {
    /// Reduce the queried value of a stat. e.g. `Drunk` reduces REF by 2.
    StatPenalty { stat: Stat, by: i8 },
    /// Increase the queried value of a stat. e.g. a Sandevistan boosts REF.
    StatBonus { stat: Stat, by: i8 },
    /// Reduce a specific skill's effective rank.
    SkillPenalty { skill: SkillId, by: i8 },
    /// Increase a specific skill's effective rank.
    SkillBonus { skill: SkillId, by: i8 },

    /// Penalty applied to every Action Check this turn. e.g. Seriously
    /// Wounded applies `-2` (book p.186).
    AllActionsPenalty(i8),
    /// Reduce the character's MOVE. Floor at 1 is applied at query time.
    /// e.g. Mortally Wounded applies `-6` to MOVE (book p.186).
    MovePenalty(i8),
    /// Permanent change to the death-save penalty until the source injury is
    /// healed. Applied additively across multiple injuries.
    DeathSavePenaltyDelta(i8),
    /// Penalty applied to melee attacks specifically (e.g. Torn Muscle).
    MeleeAttackPenalty(i8),
    /// Penalty applied to actions taken with a particular hand.
    /// `Hand::Either` means either hand; e.g. Crushed Fingers Both = two
    /// effects each scoped to one hand, or one effect with `Either` if the
    /// rule treats it symmetrically.
    HandActionsPenalty { hand: Hand, by: i8 },

    /// Cannot take any Action this turn. e.g. spinal injury (next-turn).
    CannotTakeAction,
    /// Cannot take a Move Action. e.g. prone, dismembered legs.
    CannotTakeMoveAction,
    /// Cannot perform a Dodge reaction. e.g. dismembered leg, human-shielded.
    CannotDodge,

    /// Take damage if the character moves more than `threshold_m` metres in a
    /// single Action. e.g. Broken Ribs / Foreign Object — book p.187.
    /// The combat engine checks this at movement resolution.
    DamageOnMovementOver { threshold_m: u16, damage: HpDamage },
    /// Take damage at the start of every turn while this effect is active.
    /// e.g. burning, ongoing bleeding.
    DamagePerTurn(HpDamage),

    /// Adjust the DV of an autofire shot. e.g. Smartlinked Smartgun reduces.
    AutofireDvDelta(i8),
    /// Adjust initiative roll. e.g. Sandevistan grants a bonus.
    InitiativeBonus(i8),

    // ---- Netrunning (WP-412) ------------------------------------------------
    //
    // These variants are consumed by NET Architecture resolution code that
    // asks "what Interface-ability or speed bonus does the Netrunner have
    // active right now?" The consumer iterates `EffectStack::iter_modifiers`
    // and sums the `by` field for the relevant check. See p.201.
    /// Boosts one of the four boostable Interface Checks (or NET Speed) while
    /// a Booster program is Rezzed. Produced by programs with
    /// `ProgramEffect::BoostCheck` (Eraser, See Ya, Worm, Speedy Gonzalvez).
    ///
    /// For `BoostableCheck::Speed`, the consumer should add `by` to the
    /// Netrunner's derived NET Speed (MOVE-equivalent inside the NET
    /// Architecture). For the other three variants the consumer adds `by` to
    /// the relevant Interface-ability check roll.
    ///
    /// See p.201, p.203.
    NetrunCheckBonus {
        /// Which Interface Ability or derived value is being boosted.
        check: BoostableCheck,
        /// Magnitude of the bonus. `+2` for every published Booster (p.203).
        by: i8,
    },

    /// Reduces all brain damage received from Black ICE by `reduction` while
    /// the Armor Defender program is Rezzed. Consumed by the Black ICE combat
    /// resolution code (WP-414) when it applies brain damage to a Netrunner.
    ///
    /// See p.201, p.203: "Armor: Lowers all brain damage you would receive
    /// by 4."
    NetrunBrainDamageReduction(u8),

    /// Reduces the ATK bonus of every Non-Black-ICE Attacker Program run
    /// against the Netrunner to 0 while the Flak Defender program is Rezzed.
    /// Consumed by Attacker resolution code (WP-413).
    ///
    /// See p.201, p.203: "Flak: Reduces the ATK of non-Black-ICE Attacker
    /// Programs run against you to 0."
    NetrunAttackerAtkNullified,

    /// Stops the first successful Non-Black-ICE Program Effect from dealing
    /// brain damage; the program then auto-Derezzes. Set by Shield (Defender).
    /// Consumed by Attacker resolution code (WP-413) which must check for this
    /// modifier before applying brain damage and then derez Shield.
    ///
    /// See p.201, p.203: "Shield: Stops the first successful Non-Black ICE
    /// Program Effect from dealing brain damage."
    NetrunFirstEffectBlocked,
}

/// Which hand an action uses. See `EffectModifier::HandActionsPenalty`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum Hand {
    Left,
    Right,
    /// Symmetric: applies to whichever hand performs the action.
    Either,
}

/// Hit-point damage. Newtype so `DamageOnMovementOver`'s payload can't be
/// confused with metres-of-movement or other `u16` quantities.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct HpDamage(pub u16);
