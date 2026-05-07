//! Damage pipeline and armor ablation — WP-303.
//!
//! This module implements the single authoritative path for applying rolled
//! damage to a target entity in the world: subtract SP at the targeted
//! location, conditionally ablate the armor by 1, subtract remaining damage
//! from HP, observe wound-state transitions, and return a structured outcome
//! that upstream callers (attack WPs, the GM layer, the frontend) consume.
//!
//! ## Rulebook reference — p.186 (When Armor Doesn't Cut It)
//!
//! The three-step procedure from p.186:
//!
//! 1. **Roll damage** — the attacker rolls damage dice. *This is NOT done
//!    here.* Damage rolling belongs to the attack WPs (WP-306, WP-307, etc.).
//!    This module receives the already-rolled `raw_damage` in
//!    [`DamageApplication`].
//!
//! 2. **Subtract SP** — subtract the armor's Stopping Power in the targeted
//!    location from the raw damage; apply any remaining damage to Hit Points.
//!    Certain things that deal damage (poisons, fire — and bonus damage from
//!    criticals) bypass armor entirely (`bypass_armor = true`).
//!
//! 3. **Ablate armor** — *"If you ended up taking any damage, your armor on
//!    that location is ablated, reducing its SP by 1 point, until it is
//!    repaired."* (p.186) Ablation only happens when `raw_damage > current_sp`;
//!    if the armor fully stopped the hit (`raw_damage <= current_sp`) the SP
//!    is **not** decremented.
//!
//! ## `triggered_critical` flag
//!
//! `DamageApplication::triggered_critical` is set **by the caller** (the
//! attack WP) before the `apply_damage` call. WP-303 does not roll for
//! critical triggers — it only preserves the flag through to
//! `DamageOutcome::triggered_critical`. The critical-injury table roll itself
//! is the responsibility of WP-305.
//!
//! ## Unknown EntityId
//!
//! If the `DamageApplication::target` is not found in `world`, `apply_damage`
//! returns a no-op `DamageOutcome` with zeroed numeric fields and all `None`
//! optional fields. This is a defensive choice: the callers (attack WPs) are
//! responsible for validating that their target exists before calling this
//! function. An unknown `EntityId` at this layer is a logic error in the
//! calling code, not a rules case. Using a no-op return rather than a
//! `panic!` lets the test suite run safely with crafted scenarios while still
//! being auditable (the returned `DamageOutcome` will clearly show
//! `hp_lost = 0`, `final_hp = 0`, etc.). The calling code should never
//! observe this path in production.

use crate::effects::WoundState;
use crate::types::EntityId;
use crate::world::World;
use serde::{Deserialize, Serialize};

/// Input to [`apply_damage`]: everything needed to apply one hit.
///
/// The attacker assembles this after a hit is confirmed and damage is rolled.
/// The `raw_damage` field is the already-rolled total (e.g. the sum of 2d6
/// dice for a pistol or 5d6 for a shotgun). This module does not roll dice.
///
/// See p.186.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageApplication {
    /// The entity being hit. Must resolve via [`World::entity_mut`].
    pub target: EntityId,
    /// Damage as rolled by the attacker — before SP subtraction.
    pub raw_damage: u16,
    /// Which body location the hit lands on. See [`HitLocation`].
    pub location: HitLocation,
    /// When `true`, armor SP is ignored entirely (poisons, fire, bonus
    /// damage from critical injuries per p.186). See p.186 footnote:
    /// "Some things that cause damage, like poisons and fire, bypass armor."
    pub bypass_armor: bool,
    /// Free-text label for narration (e.g. `"9mm pistol"`, `"poison"`).
    /// Not used by the rules engine; passed through for the GM/LLM layer.
    pub source_label: String,
    /// When `true`, the caller (an attack WP) has already determined that
    /// this hit triggered a critical injury roll (two or more 6s on the
    /// damage dice, per WP-305). WP-303 **does not** verify or roll this;
    /// it only preserves the flag through to [`DamageOutcome`].
    pub triggered_critical: bool,
}

/// The body location targeted by an attack.
///
/// Cyberpunk RED resolves armor in exactly two locations: body and head.
/// A normal attack always hits the body; only an Aimed Shot (with the
/// appropriate DV penalty) can target the head. See p.184.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitLocation {
    /// Standard body hit — uses `WornArmor::body`. See p.184.
    Body,
    /// Aimed Shot to the head — uses `WornArmor::head`. See p.184.
    Head,
}

/// The structured outcome of one [`apply_damage`] call.
///
/// All fields that were not applicable (e.g. `armor_ablated_to` when
/// `bypass_armor` is true) are `None`. The outcome is `Serialize` and
/// `Deserialize` so it can be recorded in the game log and replayed.
///
/// See p.186.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageOutcome {
    /// The entity that was targeted.
    pub target: EntityId,
    /// The rolled damage before SP subtraction.
    pub raw_damage: u16,
    /// How many points of damage the armor stopped (`min(raw_damage, sp)`).
    /// Zero if armor was bypassed or there was no armor in the location.
    pub sp_blocked: u16,
    /// HP actually lost (`raw_damage - sp_blocked`).
    pub hp_lost: u16,
    /// The target's current HP after the hit.
    pub final_hp: i16,
    /// The armor's new SP after ablation, if ablation occurred. `None` if:
    /// - `bypass_armor` was true, or
    /// - no armor in the targeted location, or
    /// - `raw_damage <= current_sp` (no damage got through — no ablation;
    ///   p.186: "If you ended up taking any damage…").
    pub armor_ablated_to: Option<u8>,
    /// Previous and new wound state, if the wound state changed.
    /// `None` if the wound state was unchanged.
    pub wound_state_change: Option<(WoundState, WoundState)>,
    /// Preserved from [`DamageApplication::triggered_critical`]. This WP
    /// does not roll or compute this value; it is set by the attack WP and
    /// passed through for WP-305 (Critical Injury application) to act on.
    pub triggered_critical: bool,
    /// `true` if the target's wound state is now [`WoundState::Dead`].
    ///
    /// Note: the actual death-save mechanic (rolling d10 vs. BODY at the
    /// start of each turn while Mortally Wounded) is handled separately
    /// by WP-106 / the combat turn machinery. This flag signals only that
    /// the wound-state transition from the damage application landed on Dead
    /// (i.e., `update_wound_state()` returned `Some(WoundState::Dead)`).
    pub died: bool,
}

/// Apply `dmg` to the target entity in `world`, returning the full outcome.
///
/// # Application procedure (p.186)
///
/// 1. Resolve `dmg.target` to a mutable `&mut Character` via
///    [`World::entity_mut`]. If not found, return a zeroed no-op outcome.
/// 2. Select the armor piece at `dmg.location` (`armor.head` or `armor.body`).
/// 3. Compute `sp_blocked`:
///    - If `bypass_armor` or no armor: `sp_blocked = 0`.
///    - Otherwise: `sp_blocked = min(raw_damage, current_sp as u16)`.
/// 4. Compute `hp_lost = raw_damage - sp_blocked`.
/// 5. Ablate armor by 1 if and only if `hp_lost > 0` (i.e., damage got
///    through). Record the new SP in `armor_ablated_to`. See p.186.
/// 6. Apply `hp_lost` to `wounds.current_hp`.
/// 7. Call `update_wound_state()` to observe any transition.
/// 8. Return the [`DamageOutcome`].
///
/// # `triggered_critical`
///
/// The `triggered_critical` flag in the input is passed through unchanged to
/// the output. This WP does **not** roll or check for criticals; that is
/// WP-305's responsibility.
///
/// # Unknown `EntityId`
///
/// If `dmg.target` is not found in `world`, returns a no-op outcome with
/// `hp_lost = 0`, `final_hp = 0`, all optionals `None`. This is a logic
/// error in the caller; see module-level docs.
///
/// See p.186.
pub fn apply_damage(world: &mut World, dmg: DamageApplication) -> DamageOutcome {
    let no_op = DamageOutcome {
        target: dmg.target,
        raw_damage: dmg.raw_damage,
        sp_blocked: 0,
        hp_lost: 0,
        final_hp: 0,
        armor_ablated_to: None,
        wound_state_change: None,
        triggered_critical: dmg.triggered_critical,
        died: false,
    };

    // Step 1: Resolve target. Unknown EntityId → no-op (logic error in caller).
    let target = match world.entity_mut(dmg.target) {
        Some(c) => c,
        None => return no_op,
    };

    // Step 2: Select armor piece for the targeted location.
    // See p.184 — body is the default, head only on Aimed Shot.
    let armor_piece = match dmg.location {
        HitLocation::Body => target.armor.body.as_mut(),
        HitLocation::Head => target.armor.head.as_mut(),
    };

    // Steps 3–5: SP subtraction and conditional ablation. See p.186.
    let (sp_blocked, armor_ablated_to) = if dmg.bypass_armor {
        // Poisons, fire, bonus critical damage — armor is irrelevant. See p.186.
        (0u16, None)
    } else {
        match armor_piece {
            None => {
                // No armor in this location — no SP to subtract, no ablation.
                (0u16, None)
            }
            Some(piece) => {
                let sp = u16::from(piece.current_sp);
                let blocked = sp.min(dmg.raw_damage);

                // p.186: "If you ended up taking any damage, your armor on
                // that location is ablated, reducing its SP by 1 point."
                // Ablation only occurs when raw_damage > current_sp, meaning
                // some damage got through (hp_lost > 0).
                let ablated_to = if dmg.raw_damage > sp {
                    piece.current_sp = piece.current_sp.saturating_sub(1);
                    Some(piece.current_sp)
                } else {
                    None
                };

                (blocked, ablated_to)
            }
        }
    };

    // Step 6: Compute HP lost and apply to current HP.
    let hp_lost = dmg.raw_damage - sp_blocked;
    target.wounds.current_hp -= hp_lost as i16;

    // Step 7: Capture prior wound state, then recompute.
    let prior_state = target.wounds.current_state;
    let new_state_opt = target.update_wound_state();

    // Step 8: Build outcome.
    let final_hp = target.wounds.current_hp;

    let wound_state_change = new_state_opt.map(|new_state| (prior_state, new_state));

    let died = matches!(wound_state_change, Some((_, WoundState::Dead)))
        || target.wounds.current_state == WoundState::Dead && prior_state != WoundState::Dead;

    DamageOutcome {
        target: dmg.target,
        raw_damage: dmg.raw_damage,
        sp_blocked,
        hp_lost,
        final_hp,
        armor_ablated_to,
        wound_state_change,
        triggered_critical: dmg.triggered_critical,
        died,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::armor::ArmorKind;
    use crate::character::data::ArmorPiece;
    use crate::character::hp::recompute_wounds;
    use crate::effects::WoundState;
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use uuid::Uuid;

    // ---- Helpers ------------------------------------------------------------

    /// Build a `DamageApplication` targeting the PC with body-location defaults.
    fn body_hit(world: &World, raw_damage: u16) -> DamageApplication {
        DamageApplication {
            target: EntityId(world.pc.id.0),
            raw_damage,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "test".into(),
            triggered_critical: false,
        }
    }

    /// Construct a world with a PC that has body armor at `sp`.
    fn world_with_body_armor(sp: u8) -> World {
        let mut pc = fresh_pc();
        pc.armor.body = Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: sp,
            max_sp: sp,
        });
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        World::new(pc)
    }

    // ---- Acceptance tests ---------------------------------------------------

    /// Acceptance: 20 damage vs SP 11 body armor → 9 HP lost. See p.186.
    #[test]
    fn test_armor_subtracts_sp() {
        let mut world = world_with_body_armor(11);
        let dmg = body_hit(&world, 20);
        let outcome = apply_damage(&mut world, dmg);

        assert_eq!(outcome.sp_blocked, 11);
        assert_eq!(outcome.hp_lost, 9);
    }

    /// Acceptance: armor SP 11 → 10 after taking damage (ablation by 1). See p.186.
    #[test]
    fn test_armor_ablates_one() {
        let mut world = world_with_body_armor(11);
        let dmg = body_hit(&world, 20);
        let outcome = apply_damage(&mut world, dmg);

        // Ablated from 11 → 10.
        assert_eq!(outcome.armor_ablated_to, Some(10));
        // Confirm armor piece on the character also reflects the change.
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 10);
    }

    /// Acceptance: SP 11 vs 5 damage → no HP lost, no ablation.
    ///
    /// p.186: "If you ended up taking any damage, your armor on that
    /// location is ablated." When `raw_damage <= SP`, no damage gets
    /// through, so SP must NOT be decremented.
    #[test]
    fn test_armor_does_not_ablate_on_zero_through() {
        let mut world = world_with_body_armor(11);
        let dmg = body_hit(&world, 5);
        let outcome = apply_damage(&mut world, dmg);

        assert_eq!(outcome.hp_lost, 0);
        assert_eq!(outcome.sp_blocked, 5);
        assert_eq!(
            outcome.armor_ablated_to, None,
            "no ablation when armor fully stopped the hit"
        );
        // SP on character unchanged.
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 11);
    }

    /// Acceptance: bypass_armor=true applies damage fully, no SP blocked. See p.186.
    #[test]
    fn test_bypass_armor_skips_sp() {
        let mut world = world_with_body_armor(11);
        let pc_id = world.pc.id.0;
        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 8,
            location: HitLocation::Body,
            bypass_armor: true,
            source_label: "poison".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);

        assert_eq!(outcome.sp_blocked, 0);
        assert_eq!(outcome.hp_lost, 8);
        assert_eq!(outcome.armor_ablated_to, None);
        // SP on character unchanged (bypass → no ablation).
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 11);
    }

    /// Acceptance: HP drops to Seriously Wounded threshold →
    /// `wound_state_change = Some((Lightly, Seriously))`.
    ///
    /// fresh_pc has BODY 5 / WILL 5 → max_hp = 10 + 5×5 = 35,
    /// threshold = ceil(35/2) = 18. Starting at 35, deal 18 damage
    /// (no armor) → HP = 17, which is < threshold (18) → Seriously.
    /// But we first need to enter Lightly (HP < max), so start at 34
    /// and let update_wound_state be Lightly, then deal more.
    ///
    /// Simplest approach: start at full HP, deal one small hit to go
    /// Lightly, then confirm state, then deal damage to Seriously.
    #[test]
    fn test_wound_state_transition_seriously() {
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        // max_hp = 35, threshold = 18.
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        // No armor — all damage passes through.
        let mut world = World::new(pc);

        let pc_id = world.pc.id.0;

        // First hit: go to Lightly (HP = 34).
        let dmg1 = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 1,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "scratch".into(),
            triggered_critical: false,
        };
        let out1 = apply_damage(&mut world, dmg1);
        assert_eq!(
            out1.wound_state_change,
            Some((WoundState::None, WoundState::Lightly))
        );

        // Second hit: HP goes from 34 to 34-17=17, which is < threshold (18) → Seriously.
        let dmg2 = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 17,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "bullet".into(),
            triggered_critical: false,
        };
        let out2 = apply_damage(&mut world, dmg2);
        assert_eq!(
            out2.wound_state_change,
            Some((WoundState::Lightly, WoundState::Seriously))
        );
    }

    /// Acceptance: location=Head uses head armor only, not body armor.
    #[test]
    fn test_aimed_head_uses_head_armor() {
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        // Head armor SP 7, body armor SP 11.
        pc.armor.head = Some(ArmorPiece {
            kind: ArmorKind::Kevlar,
            current_sp: 7,
            max_sp: 7,
        });
        pc.armor.body = Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: 11,
            max_sp: 11,
        });
        let pc_id = pc.id.0;
        let mut world = World::new(pc);

        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 10,
            location: HitLocation::Head,
            bypass_armor: false,
            source_label: "aimed shot".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);

        // Head armor SP 7 blocks 7 → 3 HP lost.
        assert_eq!(outcome.sp_blocked, 7);
        assert_eq!(outcome.hp_lost, 3);
        // Head armor ablated (10 > 7) → 6.
        assert_eq!(outcome.armor_ablated_to, Some(6));
        // Body armor must be untouched.
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 11);
        // Head armor should now be 6.
        assert_eq!(world.pc.armor.head.as_ref().unwrap().current_sp, 6);
    }

    /// Acceptance: location=Head with body-only armor → no SP blocked. See p.184.
    #[test]
    fn test_no_armor_in_location() {
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        // Body armor only — no head armor.
        pc.armor.head = None;
        pc.armor.body = Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: 11,
            max_sp: 11,
        });
        let pc_id = pc.id.0;
        let mut world = World::new(pc);

        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 10,
            location: HitLocation::Head,
            bypass_armor: false,
            source_label: "aimed shot".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);

        // No head armor → 0 SP blocked, 10 HP lost.
        assert_eq!(outcome.sp_blocked, 0);
        assert_eq!(outcome.hp_lost, 10);
        assert_eq!(outcome.armor_ablated_to, None);
    }

    /// Regression: `triggered_critical: true` in input is preserved in output.
    /// WP-303 does not evaluate or change this flag — it belongs to the attack WP.
    #[test]
    fn test_triggered_critical_passed_through() {
        let mut world = world_with_body_armor(11);
        let pc_id = world.pc.id.0;
        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 15,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "shotgun".into(),
            triggered_critical: true,
        };
        let outcome = apply_damage(&mut world, dmg);
        assert!(
            outcome.triggered_critical,
            "triggered_critical must be passed through unchanged"
        );
    }

    /// Regression: `triggered_critical: false` is also passed through (not set to true).
    #[test]
    fn test_triggered_critical_false_passed_through() {
        let mut world = world_with_body_armor(11);
        let pc_id = world.pc.id.0;
        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 15,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "pistol".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);
        assert!(
            !outcome.triggered_critical,
            "triggered_critical false must not be changed to true"
        );
    }

    /// Regression: unknown EntityId returns a no-op outcome without panic.
    #[test]
    fn test_unknown_entity_id_returns_noop() {
        let mut world = World::new(fresh_pc());
        let unknown = EntityId(Uuid::from_u128(0xDEADBEEF_u128));
        let dmg = DamageApplication {
            target: unknown,
            raw_damage: 10,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "unknown".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);
        assert_eq!(outcome.hp_lost, 0);
        assert_eq!(outcome.sp_blocked, 0);
        assert_eq!(outcome.armor_ablated_to, None);
        assert_eq!(outcome.wound_state_change, None);
        assert!(!outcome.died);
    }

    /// Sanity: zero raw_damage causes no HP loss and no ablation.
    #[test]
    fn test_zero_damage_no_effect() {
        let mut world = world_with_body_armor(11);
        let dmg = body_hit(&world, 0);
        let max_hp_before = world.pc.wounds.max_hp;
        let hp_before = world.pc.wounds.current_hp;
        let outcome = apply_damage(&mut world, dmg);

        assert_eq!(outcome.hp_lost, 0);
        assert_eq!(outcome.sp_blocked, 0);
        assert_eq!(outcome.armor_ablated_to, None);
        assert_eq!(world.pc.wounds.current_hp, hp_before);
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 11);
        let _ = max_hp_before;
    }

    /// Sanity: armor ablated to 0 does not underflow (saturating_sub). See p.186.
    #[test]
    fn test_armor_ablates_at_zero_saturates() {
        // SP already at 0 (fully ablated).
        let mut pc = fresh_pc();
        pc.stats.body = 5;
        pc.stats.will = 5;
        recompute_wounds(&mut pc);
        pc.wounds.current_hp = pc.wounds.max_hp as i16;
        pc.armor.body = Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: 0,
            max_sp: 11,
        });
        let pc_id = pc.id.0;
        let mut world = World::new(pc);

        let dmg = DamageApplication {
            target: EntityId(pc_id),
            raw_damage: 5,
            location: HitLocation::Body,
            bypass_armor: false,
            source_label: "test".into(),
            triggered_critical: false,
        };
        let outcome = apply_damage(&mut world, dmg);

        // SP is 0 → blocked 0 → all 5 HP lost.
        assert_eq!(outcome.sp_blocked, 0);
        assert_eq!(outcome.hp_lost, 5);
        // Ablation: raw_damage (5) > sp (0) → decrement saturating to 0.
        assert_eq!(outcome.armor_ablated_to, Some(0));
        assert_eq!(world.pc.armor.body.as_ref().unwrap().current_sp, 0);
    }
}
