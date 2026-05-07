//! The effect system — architectural keystone for transient/conditional
//! changes to character state.
//!
//! See `IMPLEMENTATION_PLAN.md` §2.6.
//!
//! Base character data (`stats`, `skills` ranks, etc.) is immutable after
//! creation except via explicit progression actions. All transient changes —
//! wound penalties, armor penalties, drug effects, critical-injury effects,
//! role buffs, environmental modifiers — flow through [`EffectStack`].
//!
//! Query sites (e.g. `character.current_dex()`) iterate
//! [`EffectStack::iter_modifiers`] and apply the relevant variants. The
//! stack itself does not apply modifiers; it stores them.

pub mod modifier;

pub use modifier::{EffectModifier, Hand, HpDamage};

use crate::types::{EffectInstanceId, DV};
use serde::{Deserialize, Serialize};

/// The stack of effects active on a character or entity.
///
/// `EffectStack` owns its [`ActiveEffect`]s and exposes lifecycle methods
/// (`tick_turn`, `end_round`, `end_gig`, `end_netrun`) that retire effects
/// whose duration has elapsed.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectStack {
    pub effects: Vec<ActiveEffect>,
}

impl EffectStack {
    /// Create an empty stack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Push an effect onto the stack. No de-duplication — callers (e.g. the
    /// combat engine when applying a Wound State, which should replace the
    /// previous Wound State per p.186) must remove the prior effect first.
    pub fn add(&mut self, effect: ActiveEffect) {
        self.effects.push(effect);
    }

    /// Remove the effect with the given id. `O(n)` linear scan — acceptable
    /// because typical stacks hold ~10 effects, not thousands.
    pub fn remove(&mut self, id: EffectInstanceId) -> Option<ActiveEffect> {
        let pos = self.effects.iter().position(|e| e.id == id)?;
        Some(self.effects.swap_remove(pos))
    }

    /// Iterate over the active effects in stack order.
    pub fn iter(&self) -> impl Iterator<Item = &ActiveEffect> {
        self.effects.iter()
    }

    /// Iterate flat over every modifier from every active effect, in stack
    /// order. This is the canonical query path for code that doesn't care
    /// which effect a modifier came from (e.g. summing all `StatPenalty`s
    /// for DEX).
    pub fn iter_modifiers(&self) -> impl Iterator<Item = &EffectModifier> {
        self.effects.iter().flat_map(|e| e.modifiers.iter())
    }

    /// Tick a Turn forward. Decrements the remaining count on every
    /// [`EffectDuration::Turns`] effect; drops effects whose count reaches 0.
    /// Returns the IDs of effects dropped this tick.
    ///
    /// Other duration kinds (`UntilEndOfRound`, `UntilGigEnd`, etc.) are
    /// untouched — that's the job of the corresponding lifecycle method.
    pub fn tick_turn(&mut self) -> Vec<EffectInstanceId> {
        let mut dropped = Vec::new();
        self.effects.retain_mut(|e| {
            if let EffectDuration::Turns(remaining) = &mut e.duration {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    dropped.push(e.id);
                    return false;
                }
            }
            true
        });
        dropped
    }

    /// End the current round. Drops every effect with
    /// [`EffectDuration::UntilEndOfRound`]; returns their IDs.
    pub fn end_round(&mut self) -> Vec<EffectInstanceId> {
        self.drop_with_duration(|d| matches!(d, EffectDuration::UntilEndOfRound))
    }

    /// End the current gig. Drops every effect with
    /// [`EffectDuration::UntilGigEnd`]; returns their IDs.
    pub fn end_gig(&mut self) -> Vec<EffectInstanceId> {
        self.drop_with_duration(|d| matches!(d, EffectDuration::UntilGigEnd))
    }

    /// End the current netrun. Drops every effect with
    /// [`EffectDuration::UntilEndOfNetrun`]; returns their IDs.
    pub fn end_netrun(&mut self) -> Vec<EffectInstanceId> {
        self.drop_with_duration(|d| matches!(d, EffectDuration::UntilEndOfNetrun))
    }

    fn drop_with_duration<F>(&mut self, pred: F) -> Vec<EffectInstanceId>
    where
        F: Fn(&EffectDuration) -> bool,
    {
        let mut dropped = Vec::new();
        self.effects.retain(|e| {
            if pred(&e.duration) {
                dropped.push(e.id);
                false
            } else {
                true
            }
        });
        dropped
    }
}

/// One active effect on a character or entity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActiveEffect {
    pub id: EffectInstanceId,
    pub source: EffectSource,
    pub modifiers: Vec<EffectModifier>,
    pub duration: EffectDuration,
}

/// Where an effect came from. Drives narration and informs the GM layer
/// when it asks "why does this character have a -2 to all actions?".
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectSource {
    /// A Critical Injury — see book pp.187–188. The kind enum is a stub
    /// until WP-205 lands the full 24-variant table.
    CriticalInjury(CriticalInjuryKind),
    /// A Wound State (Lightly / Seriously / Mortally Wounded; Dead).
    /// See book p.186.
    WoundState(WoundState),
    /// An installed cyberware item. Identified by content slug.
    Cyberware(CyberwareId),
    /// Worn armor. (No id — only one armor effect at a time per body location.)
    Armor,
    /// A drug currently in the character's system. See book p.227.
    Drug(DrugId),
    /// A Netrunner program currently rezzed and affecting this entity.
    Program(ProgramId),
    /// An environmental modifier (darkness, smoke, etc.).
    Environmental(EnvironmentalKind),
    /// Cyberpsychosis state (HUM < 0). See `IMPLEMENTATION_PLAN.md` §0.2.
    Cyberpsychosis,
    /// A Role Ability (e.g. Solo's Combat Awareness).
    RoleAbility(RoleAbilityId),
}

/// How long an effect lasts.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectDuration {
    /// Forever — survives gigs, netruns, healing.
    Permanent,
    /// Until specifically healed via a Quick Fix or Treatment check.
    /// Modeled on the Critical Injury healing rules (book pp.187, 223):
    /// some injuries have only Treatment (e.g. Dismembered Arm requires
    /// Surgery DV17), others have a Quick Fix that masks the Injury Effect
    /// until full Treatment is performed.
    UntilHealed {
        quick_fix: Option<DV>,
        treatment: DV,
    },
    /// N more combat turns.
    Turns(u16),
    /// Drops at end of the current combat round.
    UntilEndOfRound,
    /// Drops at end of the current gig.
    UntilGigEnd,
    /// Drops when the current netrun terminates.
    UntilEndOfNetrun,
}

/// Kind of critical injury.
///
/// **Stub.** WP-205 will replace this with a closed enum of 24 variants
/// drawn from the Critical Injuries to the Body table (p.187) and
/// Critical Injuries to the Head table (p.188).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CriticalInjuryKind {
    /// Until WP-205 lands the real variants. Carries the injury name as a
    /// free-form string so test fixtures and pre-WP-205 callers can construct
    /// concrete values.
    Placeholder(String),
}

/// Wound State per book p.186.
///
/// `None` is the unwounded state at full HP — the rulebook table on p.186
/// only defines wound states from "Less than Full HP" downward, so a
/// character at maximum HP has *no* wound effect. Modeling that explicitly
/// keeps query sites from having to reason about the absence of an entry.
///
/// `Dead` is the post-Mortally-Wounded state — when a Death Save fails
/// (or HP drops to a negative value beyond what the rules allow recovery
/// from). The plan classifies it as a Wound State for completeness.
#[derive(Copy, Clone, Default, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum WoundState {
    /// Full HP — no wound effect. See p.186.
    #[default]
    None,
    /// Less than Full HP — no penalty, but a Stabilization DV10 applies. See p.186.
    Lightly,
    /// Less than 1/2 HP (round up) — `-2` to all Actions. See p.186.
    Seriously,
    /// Less than 1 HP — `-4` to all Actions, `-6` to MOVE, Death Saves required. See p.186.
    Mortally,
    /// Failed Death Save (or HP below recoverable bound). See p.186.
    Dead,
}

/// Environmental modifier kind. See book pp.130 (Modifying the Attempt).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum EnvironmentalKind {
    Darkness,
    ExtremeStress,
    Exhausted,
    Drunk,
    Smoke,
    Stealth,
}

// ---- Stub catalog IDs ---------------------------------------------------
//
// Per WP-003's notes: "use String-newtype IDs as placeholders, then refactor
// once the catalog WPs land."

/// Cyberware catalog slug. Real catalog lands in WP-204.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct CyberwareId(pub String);

/// Drug catalog slug. Real catalog lands in WP-206.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct DrugId(pub String);

/// Program catalog slug. Real catalog lands in WP-208.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct ProgramId(pub String);

/// Open content slug naming a Role Ability *sub-feature* as the source of an
/// effect (e.g. `"combat_awareness.precision_attack"`,
/// `"moto.family_motorpool"`).
///
/// **Coexists with [`crate::catalog::roles::RoleAbilityKind`]** — see the
/// catalog module docs for the rationale. In short: `RoleAbilityKind` is the
/// closed-enum *catalog handle* (one of ten), while `RoleAbilityId` is the
/// *effect-source slug* the [`EffectSource::RoleAbility`] payload carries
/// to attribute a buff/debuff to a specific named sub-feature for narration
/// and UI. Sub-feature granularity (Solo's six Combat Awareness specialties,
/// Tech's four Maker specialties, etc.) is not yet a closed set, so it
/// stays as a string slug; closed-enum migration is left to a later WP if
/// the sub-feature set stabilises.
#[derive(Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct RoleAbilityId(pub String);

/// Skill identifier — see [`crate::catalog::skills::SkillId`].
///
/// Re-exported from the skill catalog so existing call sites
/// (`effects::SkillId`, `crate::effects::modifier::EffectModifier`) keep
/// compiling against the canonical type without taking a direct dependency
/// on `crate::catalog`. The catalog WP-201 supplies the real closed-enum
/// definition; this module owns only the re-export.
pub use crate::catalog::skills::SkillId;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Stat;
    use uuid::Uuid;

    fn id(n: u128) -> EffectInstanceId {
        EffectInstanceId(Uuid::from_u128(n))
    }

    fn drug_effect(name: &str, duration: EffectDuration) -> ActiveEffect {
        ActiveEffect {
            id: id(name.bytes().fold(0u128, |a, b| a * 31 + b as u128) + 1),
            source: EffectSource::Drug(DrugId(name.to_string())),
            modifiers: vec![EffectModifier::StatBonus {
                stat: Stat::Ref,
                by: 2,
            }],
            duration,
        }
    }

    #[test]
    fn test_effect_stack_add_remove() {
        let mut stack = EffectStack::new();
        let eid = id(1);
        let effect = ActiveEffect {
            id: eid,
            source: EffectSource::WoundState(WoundState::Lightly),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        };
        stack.add(effect.clone());
        assert_eq!(stack.iter().count(), 1);

        let removed = stack.remove(eid);
        assert_eq!(removed, Some(effect));
        assert_eq!(stack.iter().count(), 0);

        // Removing a missing id returns None — never panics.
        assert_eq!(stack.remove(id(999)), None);
    }

    #[test]
    fn test_tick_turn_decrements() {
        // Per the WP: "an effect with Turns(2) survives one tick, drops on
        // the second."
        let mut stack = EffectStack::new();
        stack.add(ActiveEffect {
            id: id(1),
            source: EffectSource::Drug(DrugId("synthcoke".into())),
            modifiers: vec![],
            duration: EffectDuration::Turns(2),
        });

        let dropped_first = stack.tick_turn();
        assert!(dropped_first.is_empty());
        assert_eq!(stack.iter().count(), 1);
        assert_eq!(
            stack.iter().next().unwrap().duration,
            EffectDuration::Turns(1),
            "first tick must decrement to Turns(1)"
        );

        let dropped_second = stack.tick_turn();
        assert_eq!(dropped_second, vec![id(1)]);
        assert_eq!(stack.iter().count(), 0);
    }

    #[test]
    fn test_tick_turn_returns_dropped_ids() {
        let mut stack = EffectStack::new();
        // Three effects: two with Turns(1), one with Permanent.
        stack.add(drug_effect("blue_glass", EffectDuration::Turns(1)));
        stack.add(drug_effect("smash", EffectDuration::Turns(1)));
        stack.add(ActiveEffect {
            id: id(99),
            source: EffectSource::Cyberware(CyberwareId("MNDARDR".into())),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });

        let dropped = stack.tick_turn();
        assert_eq!(dropped.len(), 2, "both Turns(1) effects must drop");
        assert_eq!(stack.iter().count(), 1, "Permanent effect must survive");
        assert_eq!(
            stack.iter().next().unwrap().source,
            EffectSource::Cyberware(CyberwareId("MNDARDR".into()))
        );
    }

    #[test]
    fn test_end_gig_drops_marker_effects() {
        let mut stack = EffectStack::new();
        let gig_id = id(1);
        let perm_id = id(2);
        let round_id = id(3);
        let netrun_id = id(4);

        stack.add(ActiveEffect {
            id: gig_id,
            source: EffectSource::Environmental(EnvironmentalKind::Stealth),
            modifiers: vec![],
            duration: EffectDuration::UntilGigEnd,
        });
        stack.add(ActiveEffect {
            id: perm_id,
            source: EffectSource::Cyberware(CyberwareId("CYBARM".into())),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });
        stack.add(ActiveEffect {
            id: round_id,
            source: EffectSource::RoleAbility(RoleAbilityId("combat_awareness".into())),
            modifiers: vec![],
            duration: EffectDuration::UntilEndOfRound,
        });
        stack.add(ActiveEffect {
            id: netrun_id,
            source: EffectSource::Program(ProgramId("eraser".into())),
            modifiers: vec![],
            duration: EffectDuration::UntilEndOfNetrun,
        });

        let dropped = stack.end_gig();
        assert_eq!(dropped, vec![gig_id], "only UntilGigEnd dropped");
        assert_eq!(
            stack.iter().count(),
            3,
            "Permanent, UntilEndOfRound, UntilEndOfNetrun all survive"
        );

        // Sanity: end_round only drops the UntilEndOfRound effect.
        let dropped_round = stack.end_round();
        assert_eq!(dropped_round, vec![round_id]);

        let dropped_netrun = stack.end_netrun();
        assert_eq!(dropped_netrun, vec![netrun_id]);

        // Permanent persists across all lifecycle hooks.
        assert_eq!(stack.iter().count(), 1);
        assert_eq!(stack.iter().next().unwrap().id, perm_id);
    }

    #[test]
    fn test_iter_modifiers_flat() {
        let mut stack = EffectStack::new();
        stack.add(ActiveEffect {
            id: id(1),
            source: EffectSource::Drug(DrugId("synthcoke".into())),
            modifiers: vec![
                EffectModifier::StatBonus {
                    stat: Stat::Ref,
                    by: 2,
                },
                EffectModifier::InitiativeBonus(1),
            ],
            duration: EffectDuration::Turns(10),
        });
        stack.add(ActiveEffect {
            id: id(2),
            source: EffectSource::WoundState(WoundState::Seriously),
            modifiers: vec![EffectModifier::AllActionsPenalty(-2)],
            duration: EffectDuration::Permanent,
        });
        stack.add(ActiveEffect {
            id: id(3),
            source: EffectSource::Environmental(EnvironmentalKind::Darkness),
            modifiers: vec![],
            duration: EffectDuration::UntilEndOfRound,
        });

        let mods: Vec<&EffectModifier> = stack.iter_modifiers().collect();
        assert_eq!(
            mods.len(),
            3,
            "flat: 2 + 1 + 0 = 3 modifiers across 3 effects"
        );

        // Order: stack order, then modifiers within each effect in declared order.
        assert!(matches!(
            mods[0],
            EffectModifier::StatBonus {
                stat: Stat::Ref,
                by: 2
            }
        ));
        assert!(matches!(mods[1], EffectModifier::InitiativeBonus(1)));
        assert!(matches!(mods[2], EffectModifier::AllActionsPenalty(-2)));
    }

    // Bonus: regression guard — adding then immediately removing the same
    // effect leaves the stack genuinely empty (not "logically empty" with
    // dangling capacity counted as a still-active effect).
    #[test]
    fn test_iter_modifiers_empty_after_full_cycle() {
        let mut stack = EffectStack::new();
        let eid = id(1);
        stack.add(ActiveEffect {
            id: eid,
            source: EffectSource::Drug(DrugId("blue_glass".into())),
            modifiers: vec![EffectModifier::StatBonus {
                stat: Stat::Cool,
                by: 2,
            }],
            duration: EffectDuration::Permanent,
        });
        stack.remove(eid);
        assert_eq!(stack.iter_modifiers().count(), 0);
    }
}
