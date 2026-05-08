//! Combat Awareness (Solo Role Ability) — WP-510.
//!
//! **Rulebook name:** The rulebook (p.146) calls this ability
//! **"Combat Awareness"**, not "Combat Sense". The WP-510 spec uses "Combat
//! Sense" as its slug; this module honours the spec slug (`combat_sense`) in
//! the public API and `RoleAbilityId` but documents the correct rulebook name.
//!
//! ## Rulebook mechanics (p.146) vs. WP-510 contract
//!
//! The rulebook (p.146) describes a **point-pool** system: the Solo has
//! `role_rank` points to allocate across six sub-abilities at the start of
//! combat (Damage Deflection, Fumble Recovery, Initiative Reaction, Precision
//! Attack, Spot Weakness, Threat Detection). Each point spent in
//! *Initiative Reaction* adds +1 to Initiative rolls; each point spent in
//! *Threat Detection* adds +1 to Perception checks.
//!
//! **This WP implements a deliberate simplification** agreed in the WP-510
//! spec: the Solo receives an Initiative bonus and a Perception bonus both
//! equal to `role_rank`, as if all points were split evenly. The full
//! point-pool allocation mechanic is deferred to a later WP that can model
//! per-combat reallocations. Deviation flagged in PR.
//!
//! ## What this module provides
//!
//! - [`combat_sense_rank`] — returns the effective rank for a character.
//! - [`combat_sense_modifiers`] — builds the two-modifier list for a given rank.
//! - [`apply_combat_sense`] — pushes a permanent [`ActiveEffect`] onto a Solo's
//!   [`EffectStack`] if `role_rank > 0`.
//!
//! See p.146.

use crate::character::data::Role;
use crate::character::Character;
use crate::effects::{
    ActiveEffect, EffectDuration, EffectModifier, EffectSource, RoleAbilityId, SkillId,
};
use crate::types::EffectInstanceId;
use uuid::Uuid;

/// Returns the character's effective Combat Awareness rank.
///
/// Per p.146, Combat Awareness is the Solo's Role Ability and its rank
/// equals `character.role_rank` when the character is a [`Role::Solo`].
/// For any other role the ability does not apply; this function returns `0`
/// so callers can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::combat_sense::combat_sense_rank;
/// # use cpr_rules::character::Character;
/// // See test_combat_sense_rank_returns_role_rank.
/// ```
pub fn combat_sense_rank(character: &Character) -> u8 {
    // See p.146.
    if character.role == Role::Solo {
        character.role_rank
    } else {
        0
    }
}

/// Builds the list of [`EffectModifier`]s granted by Combat Awareness at
/// the given rank.
///
/// Returns two modifiers when `rank > 0`:
///
/// 1. [`EffectModifier::InitiativeBonus`]`(rank as i8)` — from the
///    *Initiative Reaction* sub-ability (p.146: "Each point adds a +1 to
///    Initiative rolls made").
/// 2. [`EffectModifier::SkillBonus`]`{ skill: SkillId::Perception, by: rank as i8 }` —
///    from the *Threat Detection* sub-ability (p.146: "Each point adds a +1 to
///    any Perception Checks made").
///
/// Returns an empty `Vec` for `rank == 0`.
///
/// **Simplification note:** in the rulebook the bonus depends on how many
/// points the Solo allocates to each sub-ability. This function models the
/// simplified contract from WP-510: both bonuses equal the full rank. See
/// module-level docs for the full deviation note.
///
/// See p.146.
pub fn combat_sense_modifiers(rank: u8) -> Vec<EffectModifier> {
    if rank == 0 {
        return Vec::new();
    }
    // See p.146: Initiative Reaction (+1 per point), Threat Detection (+1 per point).
    vec![
        EffectModifier::InitiativeBonus(rank as i8),
        EffectModifier::SkillBonus {
            skill: SkillId::Perception,
            by: rank as i8,
        },
    ]
}

/// Apply Combat Awareness to a Solo character.
///
/// Pushes a permanent [`ActiveEffect`] onto `character.effects` carrying the
/// modifiers from [`combat_sense_modifiers`]. The effect uses a deterministic
/// [`EffectInstanceId`] derived from the rank so it is stable across save
/// round-trips and reproducible from the character's seed.
///
/// **No-op** when:
/// - `character.role != Role::Solo` (silently returns; not an error per
///   WP-510 spec).
/// - `character.role_rank == 0` (no rank → no modifiers → nothing to push).
///
/// Callers are responsible for removing any prior Combat Awareness effect
/// (e.g. after a rank change) before calling this function. No de-duplication
/// is performed — see [`crate::effects::EffectStack::add`].
///
/// See p.146.
pub fn apply_combat_sense(character: &mut Character) {
    // See p.146 — ability belongs exclusively to the Solo role.
    if character.role != Role::Solo {
        return;
    }
    let rank = combat_sense_rank(character);
    if rank == 0 {
        return;
    }
    // Deterministic EffectInstanceId — stable across round-trips and seeds.
    // Base UUID chosen to be unique within the combat_sense namespace.
    let id = EffectInstanceId(Uuid::from_u128(
        0x00C0_C05E_55E0_0000_0000_0000_0000_0000 + rank as u128,
    ));
    let effect = ActiveEffect {
        id,
        source: EffectSource::RoleAbility(RoleAbilityId("combat_sense".into())),
        modifiers: combat_sense_modifiers(rank),
        duration: EffectDuration::Permanent,
    };
    character.effects.add(effect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::data::{
        Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds,
    };
    use crate::effects::{EffectStack, WoundState};
    use crate::types::{CharacterId, Eurobucks};
    use std::collections::HashMap;
    use uuid::Uuid;

    fn make_character(role: Role, role_rank: u8) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0xAB)),
            name: "Test".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 6,
                body: 7,
                emp: 5,
            },
            skills: SkillSet {
                ranks: HashMap::new(),
            },
            cyberware: vec![],
            armor: WornArmor {
                head: None,
                body: None,
            },
            inventory: Inventory { items: vec![] },
            wounds: Wounds {
                current_hp: 35,
                max_hp: 35,
                seriously_wounded_threshold: 18,
                death_save_base: 7,
                death_save_penalty: 0,
                current_state: WoundState::None,
            },
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    #[test]
    fn test_combat_sense_rank_returns_role_rank() {
        // Per p.146: Solo's Combat Awareness rank equals their Role Ability rank.
        let character = make_character(Role::Solo, 5);
        assert_eq!(combat_sense_rank(&character), 5);
    }

    #[test]
    fn test_combat_sense_rank_zero_for_non_solo() {
        // Non-Solo roles do not have Combat Awareness — rank is 0. See p.146.
        let character = make_character(Role::Netrunner, 5);
        assert_eq!(combat_sense_rank(&character), 0);
    }

    #[test]
    fn test_combat_sense_modifiers_at_rank_3() {
        // At rank 3: Initiative +3 (Initiative Reaction) and Perception +3
        // (Threat Detection). Both modifiers carry i8(3). See p.146.
        let mods = combat_sense_modifiers(3);
        assert_eq!(mods.len(), 2, "rank 3 must yield exactly 2 modifiers");
        assert!(
            mods.iter()
                .any(|m| matches!(m, EffectModifier::InitiativeBonus(3))),
            "must include InitiativeBonus(3)"
        );
        assert!(
            mods.iter().any(|m| matches!(
                m,
                EffectModifier::SkillBonus {
                    skill: SkillId::Perception,
                    by: 3
                }
            )),
            "must include SkillBonus {{ Perception, 3 }}"
        );
    }

    #[test]
    fn test_apply_combat_sense_adds_effect() {
        // apply_combat_sense on a Solo with rank > 0 must increase the stack
        // length by exactly 1. See p.146.
        let mut character = make_character(Role::Solo, 4);
        let before = character.effects.effects.len();
        apply_combat_sense(&mut character);
        assert_eq!(
            character.effects.effects.len(),
            before + 1,
            "stack must grow by 1 after apply_combat_sense"
        );
        // The pushed effect must carry both modifiers.
        let effect = character.effects.effects.first().unwrap();
        assert_eq!(effect.modifiers.len(), 2);
        assert!(matches!(effect.duration, EffectDuration::Permanent));
    }

    #[test]
    fn test_apply_combat_sense_no_op_for_non_solo() {
        // Non-Solo roles must not receive a Combat Awareness effect. See p.146.
        let mut character = make_character(Role::Netrunner, 5);
        let before = character.effects.effects.len();
        apply_combat_sense(&mut character);
        assert_eq!(
            character.effects.effects.len(),
            before,
            "stack must be unchanged for non-Solo roles"
        );
    }
}
