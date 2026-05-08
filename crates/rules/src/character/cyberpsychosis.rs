//! Cyberpsychosis state management (WP-506).
//!
//! Cyberpsychosis is a dissociative disorder that occurs when a character's
//! Humanity drops to 0 or below as a result of cumulative cyberware
//! installation. The engine represents it as a [`EffectSource::Cyberpsychosis`]
//! entry on the character's [`EffectStack`].
//!
//! Per design decision §0.2 ("Therapy-driven recovery — character is
//! recoverable, not lost"), the state can be cleared via
//! [`Character::exit_cyberpsychosis`] when WP-507 therapy restores Humanity
//! above 0.
//!
//! No specific stat modifiers are attached at the engine level — the LLM/GM
//! layer narrates and adjudicates gameplay impact (see §0.2 and pp.108–109,
//! 226–230). The engine only tracks *whether* the state is active.
//!
//! Rulebook references:
//! - **pp.108–109** — Cyberpsychosis overview: "a dissociative disorder which
//!   occurs when someone with preexisting psychopathic tendencies enhances
//!   themselves via cybernetics to the point they no longer see themselves or
//!   others as human."
//! - **pp.226–230** — Therapy and Humanity mechanics in play: Humanity tracked
//!   per p.229, cyberpsychosis trigger at HUM ≤ 0 per p.227.

use crate::character::Character;
use crate::effects::{ActiveEffect, EffectDuration, EffectSource};
use crate::types::EffectInstanceId;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Stable sentinel ID
// ---------------------------------------------------------------------------

/// The deterministic [`EffectInstanceId`] used for the Cyberpsychosis effect.
///
/// This constant is stable across runs — it is derived from a fixed `u128`
/// literal, not OS entropy. Stability means save files round-trip correctly
/// and `exit_cyberpsychosis` can always find and remove the effect by ID.
///
/// The mnemonic `0xC8BE_47C0_5575` is a hex-ish rendering of "CyBEr_PSYCHOSIS"
/// truncated to 48 bits, padded to 128 bits.
///
/// See pp.108–109, 226–230.
const CYBERPSYCHOSIS_EFFECT_ID: EffectInstanceId =
    EffectInstanceId(Uuid::from_u128(0x0000_C8BE_47C0_5575_0000_0000_0000_0000));

// ---------------------------------------------------------------------------
// impl Character
// ---------------------------------------------------------------------------

impl Character {
    /// Returns `true` if the character currently has an active Cyberpsychosis
    /// effect on their [`EffectStack`].
    ///
    /// Scans the stack for any effect whose source is
    /// [`EffectSource::Cyberpsychosis`]. Typically there is at most one such
    /// effect (the no-op guard in [`Self::enter_cyberpsychosis`] enforces
    /// this), but the method returns `true` on the first match regardless.
    ///
    /// See pp.108–109, 226–230.
    pub fn is_cyberpsychotic(&self) -> bool {
        self.effects
            .iter()
            .any(|e| matches!(e.source, EffectSource::Cyberpsychosis))
    }

    /// Push a Cyberpsychosis [`ActiveEffect`] onto the [`EffectStack`].
    ///
    /// No-op if a Cyberpsychosis effect is already present — the stack must
    /// contain at most one Cyberpsychosis effect at any time.
    ///
    /// The effect uses:
    /// - [`EffectSource::Cyberpsychosis`]
    /// - [`EffectDuration::Permanent`] — persists until explicitly removed via
    ///   [`Self::exit_cyberpsychosis`] (therapy mechanic, WP-507).
    /// - An empty modifiers list — gameplay impact is narrated by the LLM/GM
    ///   layer (design decision §0.2; pp.108–109, 226–230).
    ///
    /// See pp.108–109, 226–230.
    pub fn enter_cyberpsychosis(&mut self) {
        // Guard: do not push a second Cyberpsychosis effect.
        if self.is_cyberpsychotic() {
            return;
        }

        self.effects.add(ActiveEffect {
            id: CYBERPSYCHOSIS_EFFECT_ID,
            source: EffectSource::Cyberpsychosis,
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });
    }

    /// Remove the Cyberpsychosis effect from the [`EffectStack`].
    ///
    /// No-op if the character is not currently cyberpsychotic. If (contrary to
    /// the invariant enforced by [`Self::enter_cyberpsychosis`]) multiple
    /// Cyberpsychosis effects exist, all are removed.
    ///
    /// Called by the therapy mechanic (WP-507) once Humanity climbs back above
    /// 0. See pp.108–109, 226–230; design decision §0.2 ("recoverable, not
    /// lost").
    pub fn exit_cyberpsychosis(&mut self) {
        // Attempt the fast path first: the stable sentinel ID.
        // Then sweep for any remaining Cyberpsychosis effects (defensive,
        // handles hypothetical edge cases where the ID differs).
        self.effects.remove(CYBERPSYCHOSIS_EFFECT_ID);

        // Remove any remaining stray Cyberpsychosis effects by source match.
        // Using retain_mut on the underlying vec via the EffectStack's public
        // field (it is `pub effects: Vec<ActiveEffect>`).
        self.effects
            .effects
            .retain(|e| !matches!(e.source, EffectSource::Cyberpsychosis));
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::{Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::effects::EffectStack;
    use crate::types::{CharacterId, Eurobucks};
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Test helpers
    // -----------------------------------------------------------------------

    fn fresh_character() -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0x506_0001)),
            name: "Test Edgerunner".into(),
            handle: None,
            role: Role::Solo,
            role_rank: 4,
            stats: StatBlock {
                int: 5,
                r#ref: 7,
                dex: 6,
                tech: 4,
                cool: 5,
                will: 5,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds::default(),
            humanity: 50,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests (WP-506)
    // -----------------------------------------------------------------------

    /// A freshly created character is not cyberpsychotic.
    /// See pp.108–109, 226–230.
    #[test]
    fn test_is_cyberpsychotic_returns_false_initially() {
        let character = fresh_character();
        assert!(
            !character.is_cyberpsychotic(),
            "a fresh character must not be cyberpsychotic"
        );
    }

    /// Calling `enter_cyberpsychosis` adds a Cyberpsychosis effect to the stack.
    /// See pp.108–109, 226–230.
    #[test]
    fn test_enter_adds_effect() {
        let mut character = fresh_character();
        character.enter_cyberpsychosis();

        assert!(
            character.is_cyberpsychotic(),
            "character must be cyberpsychotic after enter_cyberpsychosis"
        );

        // Verify exactly one Cyberpsychosis effect is on the stack.
        let count = character
            .effects
            .iter()
            .filter(|e| matches!(e.source, EffectSource::Cyberpsychosis))
            .count();
        assert_eq!(
            count, 1,
            "exactly one Cyberpsychosis effect must be present"
        );

        // Verify the effect properties.
        let effect = character
            .effects
            .iter()
            .find(|e| matches!(e.source, EffectSource::Cyberpsychosis))
            .expect("Cyberpsychosis effect must exist");
        assert_eq!(effect.id, CYBERPSYCHOSIS_EFFECT_ID, "must use stable ID");
        assert!(effect.modifiers.is_empty(), "no modifiers — narrated by GM");
        assert_eq!(
            effect.duration,
            EffectDuration::Permanent,
            "Cyberpsychosis is permanent until therapy"
        );
    }

    /// Calling `enter_cyberpsychosis` twice must leave only one effect on the stack.
    /// See pp.108–109, 226–230.
    #[test]
    fn test_enter_is_idempotent() {
        let mut character = fresh_character();
        character.enter_cyberpsychosis();
        character.enter_cyberpsychosis(); // second call — must be a no-op

        let count = character
            .effects
            .iter()
            .filter(|e| matches!(e.source, EffectSource::Cyberpsychosis))
            .count();
        assert_eq!(
            count, 1,
            "calling enter_cyberpsychosis twice must result in exactly one effect"
        );
    }

    /// Calling `exit_cyberpsychosis` removes the Cyberpsychosis effect.
    /// See pp.108–109, 226–230; design decision §0.2.
    #[test]
    fn test_exit_removes_effect() {
        let mut character = fresh_character();
        character.enter_cyberpsychosis();
        assert!(character.is_cyberpsychotic(), "precondition: must be set");

        character.exit_cyberpsychosis();
        assert!(
            !character.is_cyberpsychotic(),
            "character must not be cyberpsychotic after exit_cyberpsychosis"
        );
        assert_eq!(
            character.effects.iter().count(),
            0,
            "effect stack must be empty after exit"
        );
    }

    /// Calling `exit_cyberpsychosis` on a character who is not cyberpsychotic
    /// must be a no-op (must not panic or corrupt state).
    /// See pp.108–109, 226–230; design decision §0.2.
    #[test]
    fn test_exit_is_idempotent() {
        let mut character = fresh_character();
        // Character was never cyberpsychotic.
        character.exit_cyberpsychosis(); // must not panic
        assert!(
            !character.is_cyberpsychotic(),
            "character must remain non-cyberpsychotic after no-op exit"
        );
        assert_eq!(
            character.effects.iter().count(),
            0,
            "effect stack must remain empty"
        );

        // Also test: enter, exit, exit again.
        character.enter_cyberpsychosis();
        character.exit_cyberpsychosis();
        character.exit_cyberpsychosis(); // second exit — must be a no-op
        assert!(
            !character.is_cyberpsychotic(),
            "double-exit must leave character non-cyberpsychotic"
        );
    }
}
