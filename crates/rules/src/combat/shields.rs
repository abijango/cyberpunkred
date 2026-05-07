//! Shields (regular and human) — WP-314.
//!
//! A shield is a moveable source of cover that a character carries in one hand.
//! When an attack is interposed through a shield, the shield takes the full hit
//! to its HP. At 0 HP the shield is destroyed and can no longer be used as
//! cover (though it remains equipped until explicitly dropped with an Action).
//!
//! Human shields are grappled defenders used as cover. They take damage as if
//! they had been shot normally (their own armor and HP apply). When a human
//! shield reaches 0 HP they automatically become a corpse shield whose HP
//! equals the victim's BODY stat.
//!
//! ## Rulebook reference — pp.183–184
//!
//! > **Using Shields** (p.183): "The shield takes the entire attack to its HP.
//! > If the shield hits 0 HP it is destroyed (until repaired if inorganic), and
//! > cannot be used as cover, though it still remains equipped to your hand
//! > until you use an Action to drop it."
//!
//! > **Shield types** (p.184 table):
//! > - Bulletproof Shield — 10 HP.
//! > - Corpse — BODY STAT the corpse had in life.
//!
//! > **Human Shields** (p.184): "When your Human Shield is shot, they take
//! > damage as if they had been shot normally. A Human Shield who dies while
//! > you have them equipped automatically becomes a shield with HP equal to
//! > their BODY."

// See pp.183-184.

use crate::effects::Hand;
use crate::types::EntityId;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// EquippedShield
// ---------------------------------------------------------------------------

/// A shield being actively wielded by a character.
///
/// Tracks the shield's current and maximum HP, which hand it occupies, and
/// the kind of shield (regular bulletproof or a grappled human shield).
///
/// While `current_hp > 0` the shield can be interposed against attacks. At
/// `current_hp == 0` it is destroyed and provides no cover (p.183).
///
/// See pp.183–184.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EquippedShield {
    /// What kind of shield this is. See [`ShieldKind`].
    pub kind: ShieldKind,
    /// Remaining hit points. Zero means the shield is destroyed and cannot
    /// block attacks. See p.183.
    pub current_hp: u16,
    /// Maximum hit points the shield started with. Used to track damage taken
    /// and for display purposes.
    pub max_hp: u16,
    /// Which hand the shield occupies. That hand cannot be used for anything
    /// else while the shield is wielded. See p.183.
    pub hand_in_use: Hand,
}

// ---------------------------------------------------------------------------
// ShieldKind
// ---------------------------------------------------------------------------

/// Discriminator for shield type.
///
/// See pp.183–184.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShieldKind {
    /// A transparent polycarbonate bulletproof shield (10 HP, 100 eb).
    ///
    /// The canonical non-human shield type. Takes the full attack to its HP.
    /// Cannot block Melee Attacks or Aimed Shots at the head (p.184 — those
    /// restrictions apply to Human Shields; regular shields block everything
    /// that can be interposed, per p.183).
    ///
    /// See p.184 (shield table).
    Bulletproof,
    /// A grappled human being used as a living (or soon-to-be-dead) shield.
    ///
    /// The wrapped [`EntityId`] identifies the victim entity in the world.
    /// When attacked through a Human Shield, the shield victim takes damage
    /// as if they had been shot normally (their own SP and HP apply — handled
    /// by the WP-303 damage pipeline, not by [`shield_takes_damage`]).
    ///
    /// When the victim's HP reaches 0, call [`human_shield_to_corpse`] to
    /// convert them to a corpse shield. See p.184.
    HumanShield(EntityId),
}

// ---------------------------------------------------------------------------
// ShieldOutcome
// ---------------------------------------------------------------------------

/// Result of interposing a shield between an attack and its wielder.
///
/// Returned by [`shield_takes_damage`]. The caller (attack WP) uses this to
/// apply leftover damage to the shield-wielder and to flag the shield as
/// destroyed.
///
/// See pp.183–184.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShieldOutcome {
    /// How much damage the shield absorbed (up to `shield.current_hp`).
    ///
    /// Always equals `min(raw_damage, current_hp_before)`. See p.183.
    pub damage_to_shield: u16,
    /// Damage that bleeds through to the shield's wielder.
    ///
    /// Per RAW p.183, the shield takes the **entire** attack to its HP — a
    /// regular shield does not let excess damage through during the same
    /// attack. This field is therefore always `0` for a [`ShieldKind::Bulletproof`]
    /// shield (excess is lost, consistent with the standard cover rule on
    /// p.182). It is returned here for completeness and future callers (e.g.
    /// an explosives exception analogous to p.174).
    ///
    /// For [`ShieldKind::HumanShield`] the damage is routed to the victim via
    /// the WP-303 pipeline instead; this field reflects what remains after the
    /// shield interaction (also `0` on the shield itself — the victim absorbs
    /// it all). See p.184.
    pub damage_through: u16,
    /// `true` when the shield's HP reaches 0 after this hit.
    ///
    /// A destroyed shield cannot be used as cover but remains in the hand
    /// until dropped with an Action. See p.183.
    pub shield_destroyed: bool,
}

// ---------------------------------------------------------------------------
// shield_takes_damage
// ---------------------------------------------------------------------------

/// Apply a shielded attack — damage routes to the shield first.
///
/// Mutates `shield.current_hp` in place and returns a [`ShieldOutcome`]
/// describing the result.
///
/// ## Rules summary (pp.183–184)
///
/// > "The shield takes the entire attack to its HP."
///
/// - If `raw_damage >= current_hp`, the shield is destroyed (`current_hp`
///   becomes `0`, `shield_destroyed = true`). Per RAW p.183, excess damage
///   is **not** passed through to the wielder on this attack (consistent with
///   the base cover rule on p.182 — `damage_through` is `0`).
/// - If `raw_damage < current_hp`, the shield absorbs all damage, `current_hp`
///   decrements, and `damage_through` is `0`.
/// - If `current_hp` is already `0`, the shield provides no protection
///   (`damage_to_shield = 0`, `damage_through = raw_damage`). The caller
///   should not interpose a destroyed shield (p.183); this case is handled
///   defensively.
///
/// ## Human Shields
///
/// For [`ShieldKind::HumanShield`], call this function to track shield HP
/// consumed by the attack, but also route the same `raw_damage` through the
/// victim entity's own armor and HP via [`crate::combat::damage::apply_damage`]
/// (WP-303). The victim "takes damage as if they had been shot normally"
/// (p.184). When the victim dies, convert with [`human_shield_to_corpse`].
///
/// See pp.183–184.
pub fn shield_takes_damage(shield: &mut EquippedShield, raw_damage: u16) -> ShieldOutcome {
    // See pp.183-184.
    let hp_before = shield.current_hp;

    if hp_before == 0 {
        // Shield already destroyed — no protection at all. Defensive path;
        // callers should not interpose a 0-HP shield (p.183).
        return ShieldOutcome {
            damage_to_shield: 0,
            damage_through: raw_damage,
            shield_destroyed: true,
        };
    }

    if raw_damage >= hp_before {
        // Shield destroyed by this hit. Per RAW p.183, excess damage is NOT
        // passed through to the wielder on the same attack — `damage_through`
        // is 0. Consistent with the base cover rule (p.182): "excess damage
        // is lost and doesn't harm any targets hiding behind it."
        shield.current_hp = 0;
        ShieldOutcome {
            damage_to_shield: hp_before,
            damage_through: 0,
            shield_destroyed: true,
        }
    } else {
        // Shield survives; absorbs all damage.
        shield.current_hp -= raw_damage;
        ShieldOutcome {
            damage_to_shield: raw_damage,
            damage_through: 0,
            shield_destroyed: false,
        }
    }
}

// ---------------------------------------------------------------------------
// human_shield_to_corpse
// ---------------------------------------------------------------------------

/// Convert a defeated human shield into a corpse shield.
///
/// Per RAW p.184: *"A Human Shield who dies while you have them equipped
/// automatically becomes a shield with HP equal to their BODY."*
///
/// The new shield has:
/// - `kind`: [`ShieldKind::Bulletproof`] — the corpse is now an inorganic
///   (well, organic-but-inert) object; the victim entity ID is no longer
///   relevant because they are dead. The rulebook's corpse entry in the shield
///   table (p.184) has no separate `ShieldKind` variant — it is simply a
///   destroyed-human-shield turned cover object. We use `Bulletproof` as the
///   nearest structural equivalent; callers may inspect the prior `ShieldKind`
///   before calling this function if they need to record the transition.
/// - `current_hp` and `max_hp`: both set to `victim_body` (p.184).
/// - `hand_in_use`: [`Hand::Either`] — the caller must fill in the actual hand
///   from the equipped shield context (or pass the original `hand_in_use`
///   value).
///
/// # Parameters
///
/// - `victim_body`: the BODY stat the human shield had in life (p.184).
///
/// See p.184.
pub fn human_shield_to_corpse(victim_body: u8) -> EquippedShield {
    // See p.184: "HP equal to their BODY."
    let hp = u16::from(victim_body);
    EquippedShield {
        kind: ShieldKind::Bulletproof,
        current_hp: hp,
        max_hp: hp,
        hand_in_use: Hand::Either,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    /// Helper: build a Bulletproof Shield at full HP (10 HP per p.184).
    fn bulletproof_shield() -> EquippedShield {
        EquippedShield {
            kind: ShieldKind::Bulletproof,
            current_hp: 10,
            max_hp: 10,
            hand_in_use: Hand::Right,
        }
    }

    /// Helper: build a Human Shield at the given HP.
    fn human_shield(current_hp: u16) -> EquippedShield {
        let victim_id = EntityId(Uuid::nil());
        EquippedShield {
            kind: ShieldKind::HumanShield(victim_id),
            current_hp,
            max_hp: current_hp,
            hand_in_use: Hand::Left,
        }
    }

    /// `test_shield_takes_full_attack` — 10 damage to a 10-HP Bulletproof
    /// Shield → shield takes 10 damage, 0 through, shield destroyed.
    ///
    /// Per p.183: "The shield takes the entire attack to its HP."
    /// With exactly 10 damage vs 10 HP, the shield reaches 0 HP and is
    /// destroyed; no damage passes through.
    ///
    /// Acceptance criterion per WP-314.
    #[test]
    fn test_shield_takes_full_attack() {
        let mut shield = bulletproof_shield();
        let outcome = shield_takes_damage(&mut shield, 10);

        assert_eq!(outcome.damage_to_shield, 10);
        assert_eq!(outcome.damage_through, 0);
        assert!(outcome.shield_destroyed, "shield must be destroyed at 0 HP");
        assert_eq!(shield.current_hp, 0, "current_hp must be mutated to 0");
    }

    /// `test_shield_destroyed_at_zero_hp` — 15 damage to a 10-HP shield →
    /// shield absorbs all 10 HP, shield is destroyed, 0 damage through.
    ///
    /// Per RAW p.183: excess damage is NOT passed through to the wielder on
    /// the same attack ("excess damage is lost and doesn't harm any targets
    /// hiding behind it" — base cover rule p.182, applied to shields p.183).
    ///
    /// Acceptance criterion per WP-314.
    #[test]
    fn test_shield_destroyed_at_zero_hp() {
        let mut shield = bulletproof_shield();
        let outcome = shield_takes_damage(&mut shield, 15);

        assert_eq!(
            outcome.damage_to_shield, 10,
            "shield absorbs all its remaining HP"
        );
        assert_eq!(
            outcome.damage_through, 0,
            "per RAW p.183, excess damage is lost — not passed through"
        );
        assert!(outcome.shield_destroyed, "shield must be flagged destroyed");
        assert_eq!(shield.current_hp, 0);
    }

    /// `test_human_shield_takes_damage_normally` — damage to a HumanShield
    /// routes through their HP normally.
    ///
    /// The human shield's HP decrements just like any other shield. (The
    /// victim's own armor/SP routing is handled by WP-303 and is outside this
    /// function's scope.) See p.184.
    ///
    /// Acceptance criterion per WP-314.
    #[test]
    fn test_human_shield_takes_damage_normally() {
        // Human shield with 8 HP (e.g. a standard NPC with low BODY).
        let mut shield = human_shield(8);
        let outcome = shield_takes_damage(&mut shield, 5);

        assert_eq!(outcome.damage_to_shield, 5);
        assert_eq!(outcome.damage_through, 0);
        assert!(!outcome.shield_destroyed, "shield survives with 3 HP");
        assert_eq!(shield.current_hp, 3);
        // Kind unchanged — still a HumanShield.
        assert!(matches!(shield.kind, ShieldKind::HumanShield(_)));
    }

    /// `test_human_shield_dies_becomes_corpse_shield` — human_shield_to_corpse
    /// with BODY=6 produces an EquippedShield with current_hp=6, max_hp=6.
    ///
    /// Per p.184: "automatically becomes a shield with HP equal to their BODY."
    ///
    /// Acceptance criterion per WP-314.
    #[test]
    fn test_human_shield_dies_becomes_corpse_shield() {
        let corpse = human_shield_to_corpse(6);

        assert_eq!(corpse.current_hp, 6, "HP must equal victim BODY (p.184)");
        assert_eq!(corpse.max_hp, 6);
        // The corpse shield can still absorb damage (it has HP).
        assert!(corpse.current_hp > 0);
    }

    // -----------------------------------------------------------------------
    // Additional edge-case tests
    // -----------------------------------------------------------------------

    /// Extra: zero damage against a live shield — shield unchanged.
    #[test]
    fn test_zero_damage_no_effect() {
        let mut shield = bulletproof_shield();
        let outcome = shield_takes_damage(&mut shield, 0);

        assert_eq!(outcome.damage_to_shield, 0);
        assert_eq!(outcome.damage_through, 0);
        assert!(!outcome.shield_destroyed);
        assert_eq!(shield.current_hp, 10, "HP must be unchanged");
    }

    /// Extra: attacking a shield already at 0 HP — all damage passes through.
    ///
    /// Defensive path; callers should not interpose a destroyed shield (p.183).
    #[test]
    fn test_already_destroyed_shield_passes_all_damage() {
        let mut shield = EquippedShield {
            kind: ShieldKind::Bulletproof,
            current_hp: 0,
            max_hp: 10,
            hand_in_use: Hand::Right,
        };
        let outcome = shield_takes_damage(&mut shield, 8);

        assert_eq!(outcome.damage_to_shield, 0);
        assert_eq!(outcome.damage_through, 8);
        assert!(outcome.shield_destroyed);
    }

    /// Extra: partial hit — shield survives, no bleed-through.
    #[test]
    fn test_partial_hit_shield_survives() {
        let mut shield = bulletproof_shield(); // 10 HP
        let outcome = shield_takes_damage(&mut shield, 4);

        assert_eq!(outcome.damage_to_shield, 4);
        assert_eq!(outcome.damage_through, 0);
        assert!(!outcome.shield_destroyed);
        assert_eq!(shield.current_hp, 6);
    }

    /// Extra: corpse shield from BODY=0 yields 0-HP shield (already destroyed).
    ///
    /// Edge case — a victim with BODY=0 becomes a corpse shield that
    /// immediately provides no cover.
    #[test]
    fn test_corpse_shield_body_zero() {
        let corpse = human_shield_to_corpse(0);

        assert_eq!(corpse.current_hp, 0);
        assert_eq!(corpse.max_hp, 0);
    }
}
