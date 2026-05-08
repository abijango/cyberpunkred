//! Cyberware installation and Humanity Loss mechanics (WP-505).
//!
//! This module exposes [`install_cyberware`] — the single entry-point for
//! placing a piece of cyberware on a character. It validates prerequisites,
//! checks option-slot capacity, rolls (or reads the preset) Humanity Loss,
//! applies the loss, updates EMP, pushes the item's `EffectModifier`s onto
//! the character's [`EffectStack`], and records the installation.
//!
//! Rulebook references:
//! - **pp.94–116:** Cyberware overview and category tables.
//! - **p.111:** At-creation HL is preset; in-play HL uses the dice in
//!   parentheses following the preset number.
//! - **p.227:** Step-by-step install rules and cyberpsychosis trigger
//!   (`humanity_after <= 0`).
//! - **p.80:** `EMP = floor(max(humanity, 0) / 10)`.

use crate::catalog::cyberware::{Cyberware, HumanityLossSpec};
use crate::catalog::Catalog;
use crate::character::data::InstalledCyberware;
use crate::character::Character;
use crate::dice::ndn_d6;
use crate::effects::{ActiveEffect, CyberwareId, EffectDuration, EffectSource};
use crate::error::RulesError;
use crate::rng::Rng;
use crate::types::EffectInstanceId;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Result of a successful [`install_cyberware`] call.
///
/// Callers use this to narrate what happened: how much Humanity was lost,
/// whether EMP changed, which effects are now permanent, and whether the
/// character tipped into Cyberpsychosis.
///
/// See p.227 for the rulebook's description of each outcome field.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallOutcome {
    /// Humanity Points subtracted from the character this install. Zero for
    /// items with [`HumanityLossSpec::None`] and for items with a `Fixed(0)`
    /// value.
    pub humanity_loss: u8,
    /// The character's Humanity after the loss has been applied. May be
    /// negative — that is the Cyberpsychosis condition (p.230).
    pub humanity_after: i16,
    /// `Some((before, after))` when EMP crossed a tens-boundary as a result
    /// of this install. Both values are the `floor(humanity / 10)` integer.
    /// `None` when EMP was unchanged (either no HL was taken, or HL was small
    /// enough not to cross a boundary). See p.80.
    pub emp_change: Option<(u8, u8)>,
    /// The [`EffectInstanceId`]s of every permanent modifier pushed onto the
    /// character's [`crate::effects::EffectStack`] during this install. One
    /// entry per [`crate::effects::EffectModifier`] on the catalog row (items
    /// with an empty `effects` list produce an empty vec here).
    pub effects_added: Vec<EffectInstanceId>,
    /// `true` when `humanity_after <= 0`. See p.227 / p.230. The caller
    /// (or a later WP-506 call) should enter the Cyberpsychosis state.
    pub triggered_cyberpsychosis: bool,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Install a piece of cyberware on a character.
///
/// Performs all validation and side-effects described on p.227:
///
/// 1. Looks up `item` in `catalog`. Returns
///    [`RulesError::CatalogLoadFailed`] (reused as "not found") if missing.
///    **Note:** a more precise "item not found" variant does not exist in the
///    pre-staged error set — `CatalogLoadFailed` is the closest available
///    shape. The path field is set to the item slug for diagnostics.
/// 2. Validates prerequisites: if the item requires a foundational piece, it
///    must already be in `character.cyberware`.
/// 3. Validates option-slot capacity for items with `slot_cost > 0`.
/// 4. Rolls (or reads preset) Humanity Loss per `at_creation`:
///    - `at_creation = true` → always use the preset (`fixed_value()`).
///    - `at_creation = false` → use [`HumanityLossSpec::Rolled`]'s dice
///      expression when present; otherwise fall back to `fixed_value()`.
/// 5. Subtracts the HL from `character.humanity`.
/// 6. Recomputes EMP; records `Some((before, after))` when it changed.
/// 7. Pushes the catalog item's `effects` onto `character.effects` as
///    `EffectSource::Cyberware(id)` with `EffectDuration::Permanent`.
/// 8. Appends an [`InstalledCyberware`] entry to `character.cyberware`.
/// 9. Sets `triggered_cyberpsychosis` if `humanity_after <= 0`.
///
/// # Errors
///
/// - [`RulesError::CatalogLoadFailed`] — `item` slug not found in catalog.
/// - [`RulesError::MissingPrerequisite`] — prerequisite not installed.
/// - [`RulesError::OptionSlotsExhausted`] — no room in foundational piece.
///
/// # Determinism
///
/// `EffectInstanceId`s are derived from the item slug via a deterministic
/// hash with a per-modifier index offset. No OS entropy is used. See p.227.
pub fn install_cyberware(
    character: &mut Character,
    item: CyberwareId,
    catalog: &Catalog<Cyberware>,
    rng: &mut Rng,
    at_creation: bool,
) -> Result<InstallOutcome, RulesError> {
    // --- 1. Catalog lookup --------------------------------------------------
    // See p.227. We re-use CatalogLoadFailed (with the slug as path) as the
    // "not found" error because no dedicated "CyberwareNotFound" variant was
    // pre-staged in error.rs.
    let cw = catalog
        .get(&item.0)
        .ok_or_else(|| RulesError::CatalogLoadFailed {
            path: std::path::PathBuf::from(&item.0),
            source: format!("cyberware '{}' not found in catalog", item.0),
        })?;

    // --- 2. Prerequisite validation -----------------------------------------
    // See p.227 / pp.111–116: foundational pieces (Neural Link, Cybereye, etc.)
    // must be installed before their options can be added.
    if let Some(req) = &cw.prerequisite {
        let has_prereq = character.cyberware.iter().any(|ic| ic.id == *req);
        if !has_prereq {
            return Err(RulesError::MissingPrerequisite {
                item: item.0.clone(),
                prerequisite: req.0.clone(),
            });
        }
    }

    // --- 3. Option-slot validation ------------------------------------------
    // See p.111 ("Option Slots"): each foundational piece advertises how many
    // option slots it offers; the total `slot_cost` of installed options must
    // not exceed that budget. Items with `slot_cost == 0` (foundational pieces,
    // standalone categories like Fashionware / ExternalBody) skip this check.
    if cw.slot_cost > 0 {
        if let Some(req) = &cw.prerequisite {
            // Find the foundational piece and count how many slots are already used.
            let foundation = catalog
                .get(&req.0)
                .ok_or_else(|| RulesError::CatalogLoadFailed {
                    path: std::path::PathBuf::from(&req.0),
                    source: format!("prerequisite '{}' not found in catalog", req.0),
                })?;

            let used_slots: u8 = character
                .cyberware
                .iter()
                .filter_map(|ic| {
                    // Count slot_cost of every option already under this foundational piece.
                    // An option is "under" the foundation if its own prerequisite matches.
                    if ic.id == *req {
                        return None; // the foundational piece itself costs 0 slots
                    }
                    let ic_def = catalog.get(&ic.id.0)?;
                    if ic_def.prerequisite.as_ref() == Some(req) {
                        Some(ic_def.slot_cost)
                    } else {
                        None
                    }
                })
                .fold(0u8, |acc, cost| acc.saturating_add(cost));

            let available = foundation.option_slots.saturating_sub(used_slots);
            if cw.slot_cost > available {
                return Err(RulesError::OptionSlotsExhausted {
                    available,
                    needed: cw.slot_cost,
                });
            }
        }
        // If slot_cost > 0 but prerequisite is None, the catalog is malformed.
        // The loader enforces well-formedness, so this path should not be reached
        // in production. We proceed rather than hard-error, consistent with the
        // "engine trusts the catalog" principle.
    }

    // --- 4. Humanity Loss roll ----------------------------------------------
    // See p.111: at creation use the preset; in play roll the dice expression.
    let humanity_loss: u8 = if at_creation {
        // At character generation HL is always the preset value (p.111).
        cw.humanity_loss.fixed_value()
    } else {
        match &cw.humanity_loss {
            HumanityLossSpec::None => 0,
            HumanityLossSpec::Fixed(n) => *n,
            HumanityLossSpec::Rolled { fixed, dice } => {
                // Roll N dice of kind K, sum, then apply divisor (round up).
                // See p.111 and the "1d6/2 round up" pattern on pp.358–367.
                let raw_rolls = ndn_d6(dice.n, rng);
                let raw_sum: u32 = raw_rolls.iter().map(|&v| v as u32).sum();
                let divided = raw_sum.div_ceil(dice.divisor as u32);
                // fixed + roll result, saturating at u8::MAX.
                (*fixed as u32).saturating_add(divided).min(u8::MAX as u32) as u8
            }
        }
    };

    // --- 5. Apply Humanity Loss ---------------------------------------------
    // See p.227. humanity is i16 — it can go negative (cyberpsychosis territory).
    let humanity_before = character.humanity;
    let emp_before = emp_from_humanity(humanity_before);
    character.humanity -= i16::from(humanity_loss);
    let humanity_after = character.humanity;

    // --- 6. EMP change ------------------------------------------------------
    // See p.80: EMP = floor(max(HUM, 0) / 10). Only report when it crosses a
    // tens boundary as a result of this install.
    let emp_after = emp_from_humanity(humanity_after);
    let emp_change = if emp_after != emp_before {
        Some((emp_before, emp_after))
    } else {
        None
    };

    // --- 7. Push effects onto EffectStack -----------------------------------
    // See p.227. Each effect modifier gets its own ActiveEffect with a
    // deterministic EffectInstanceId derived from the item slug and index.
    // No OS entropy is used — the slug bytes are hashed with the modifier
    // index for uniqueness within the install.
    let mut effects_added = Vec::with_capacity(cw.effects.len());
    for (idx, modifier) in cw.effects.iter().enumerate() {
        let effect_id = deterministic_effect_id(&item.0, idx);
        let active = ActiveEffect {
            id: effect_id,
            source: EffectSource::Cyberware(item.clone()),
            modifiers: vec![modifier.clone()],
            duration: EffectDuration::Permanent,
        };
        character.effects.add(active);
        effects_added.push(effect_id);
    }

    // --- 8. Record installation --------------------------------------------
    // See p.94. Options lists are populated by subsequent install calls
    // (e.g. installing Interface Plugs after Neural Link).
    character.cyberware.push(InstalledCyberware {
        id: item,
        options: vec![],
    });

    // --- 9. Cyberpsychosis trigger -----------------------------------------
    // See pp.227–230. humanity_after <= 0 is the trigger per RAW.
    let triggered_cyberpsychosis = humanity_after <= 0;

    Ok(InstallOutcome {
        humanity_loss,
        humanity_after,
        emp_change,
        effects_added,
        triggered_cyberpsychosis,
    })
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Compute EMP from humanity per p.80:
/// `EMP = floor(max(humanity, 0) / 10)`.
///
/// Returns `u8`: the maximum humanity is 10 × 10 = 100, so EMP ≤ 10 which
/// fits comfortably.
fn emp_from_humanity(humanity: i16) -> u8 {
    (humanity.max(0) / 10) as u8
}

/// Produce a deterministic [`EffectInstanceId`] for the `idx`-th effect of
/// the cyberware item named by `slug`.
///
/// Uses a simple FNV-1a-inspired byte mix over the slug bytes, then XORs in
/// the index. No OS entropy, no `Instant::now()` — WASM-safe. See the
/// WP-505 spec note: "Generate a deterministic `EffectInstanceId` from a
/// counter or hash (no OS entropy)."
///
/// The magic constant `0xCBER_WARE_CBCB_CBCB` is an arbitrary non-zero
/// salt that namespaces these IDs away from the netrunning program IDs
/// (which use `0xEFF0_EFF0_EFF0_EFF0`) and the Black ICE entity IDs.
fn deterministic_effect_id(slug: &str, idx: usize) -> EffectInstanceId {
    // FNV-1a 64-bit over the slug bytes.
    const FNV_OFFSET: u64 = 14_695_981_039_346_656_037;
    const FNV_PRIME: u64 = 1_099_511_628_211;
    let mut h: u64 = FNV_OFFSET;
    for b in slug.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    // Fold in the index so separate modifiers on the same item get distinct IDs.
    h ^= idx as u64;
    h = h.wrapping_mul(FNV_PRIME);

    // Pack into a u128 with a distinguishing salt in the high bits.
    // 0xC5BE_WARE is a mnemonic for "CyBErWARE" in hex-ish notation.
    const SALT: u128 = 0x0000_C5BE_0000_0000_0000_0000_0000_0000;
    EffectInstanceId(Uuid::from_u128(SALT | h as u128))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::cyberware::load_cyberware_catalog;
    use crate::character::{Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use crate::effects::EffectStack;
    use crate::types::{CharacterId, Eurobucks};
    use rand::SeedableRng;
    use std::path::PathBuf;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn catalog_dir() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("catalogs");
        p.push("cyberware");
        p
    }

    fn fresh_character(humanity: i16) -> Character {
        Character {
            id: CharacterId(Uuid::from_u128(0x505_0001)),
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
            humanity,
            luck_pool: 6,
            money: Eurobucks(0),
            improvement_points: 0,
            lifepath: Lifepath::default(),
            effects: EffectStack::new(),
            complementary_bonuses: Vec::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Acceptance tests
    // -----------------------------------------------------------------------

    /// Acceptance: the catalog loads and a known item (Neural Link) can be
    /// installed on a fresh character. See p.227.
    #[test]
    fn test_cyberware_catalog_complete() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50);
        let mut rng = Rng::seed_from_u64(42);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("neural_link".into()),
            &catalog,
            &mut rng,
            true, // at_creation: preset HL
        )
        .expect("Neural Link install must succeed on empty character");

        // Neural Link preset HL is 7 (p.359).
        assert_eq!(outcome.humanity_loss, 7);
        assert_eq!(outcome.humanity_after, 43);
        assert!(!outcome.triggered_cyberpsychosis);

        // The install must be recorded.
        assert_eq!(character.cyberware.len(), 1);
        assert_eq!(character.cyberware[0].id, CyberwareId("neural_link".into()));
    }

    /// Acceptance: Neural Link is a foundational piece — it has no prerequisite
    /// and installs cleanly on an empty character. See p.112 / p.359.
    #[test]
    fn test_neural_link_no_prereq() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50);
        let mut rng = Rng::seed_from_u64(1);

        let result = install_cyberware(
            &mut character,
            CyberwareId("neural_link".into()),
            &catalog,
            &mut rng,
            true,
        );
        // Must succeed — no prerequisite check applies.
        assert!(
            result.is_ok(),
            "Neural Link must install without prerequisite: {result:?}"
        );
    }

    /// Acceptance: installing Interface Plugs without a Neural Link must
    /// return `Err(MissingPrerequisite)`. See p.359.
    #[test]
    fn test_interface_plugs_require_neural_link() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50);
        let mut rng = Rng::seed_from_u64(2);

        let result = install_cyberware(
            &mut character,
            CyberwareId("interface_plugs".into()),
            &catalog,
            &mut rng,
            true,
        );

        match result {
            Err(RulesError::MissingPrerequisite { item, prerequisite }) => {
                assert_eq!(item, "interface_plugs");
                assert_eq!(prerequisite, "neural_link");
            }
            other => panic!("expected MissingPrerequisite, got: {other:?}"),
        }
    }

    /// Acceptance: medical-grade (HumanityLossSpec::None) items produce
    /// `humanity_loss = 0`. See p.226.
    #[test]
    fn test_medical_grade_zero_hl() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50);
        let mut rng = Rng::seed_from_u64(3);

        // Biomonitor is Fashionware with HumanityLossSpec::None (p.358).
        let outcome = install_cyberware(
            &mut character,
            CyberwareId("biomonitor".into()),
            &catalog,
            &mut rng,
            false, // in-play install; HL should still be 0
        )
        .expect("Biomonitor install must succeed");

        assert_eq!(outcome.humanity_loss, 0, "Biomonitor has no HL");
        assert_eq!(outcome.humanity_after, 50, "humanity unchanged");
        assert_eq!(outcome.emp_change, None, "EMP unchanged");
    }

    /// Acceptance: `at_creation = true` always uses the preset (fixed) HL.
    /// Neural Link preset is 7 (p.359). See p.111.
    #[test]
    fn test_humanity_loss_at_creation_fixed() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(100);
        let mut rng = Rng::seed_from_u64(4);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("neural_link".into()),
            &catalog,
            &mut rng,
            true, // at_creation
        )
        .expect("install must succeed");

        // Neural Link is Rolled { fixed: 7, dice: 2d6 }.
        // at_creation = true must use fixed = 7.
        assert_eq!(
            outcome.humanity_loss, 7,
            "at_creation must use preset HL of 7"
        );
        assert_eq!(outcome.humanity_after, 93);
    }

    /// Acceptance: `at_creation = false` uses the rolled value for
    /// `HumanityLossSpec::Rolled` items. Neural Link is `7 (2d6)` — the
    /// in-play HL is `7 + 2d6` which is always ≥ 9 and ≤ 19. See p.111.
    #[test]
    fn test_humanity_loss_in_play_rolled() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(100);
        let mut rng = Rng::seed_from_u64(0);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("neural_link".into()),
            &catalog,
            &mut rng,
            false, // in-play
        )
        .expect("install must succeed");

        // Neural Link in-play HL = 7 + roll(2d6). Minimum = 7+2 = 9, max = 7+12 = 19.
        assert!(
            outcome.humanity_loss >= 9 && outcome.humanity_loss <= 19,
            "in-play rolled HL must be 9..=19 for Neural Link (7+2d6), got {}",
            outcome.humanity_loss
        );
        // humanity_after = 100 - loss.
        assert_eq!(
            outcome.humanity_after,
            100 - i16::from(outcome.humanity_loss)
        );
    }

    /// Acceptance: installing enough cyberware to drop humanity to ≤ 0
    /// sets `triggered_cyberpsychosis = true`. See p.227 / p.230.
    #[test]
    fn test_humanity_below_zero_triggers_cyberpsychosis() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        // Start with humanity = 2 so that any non-zero HL drops it to ≤ 0.
        // Pain Editor preset is 14 (p.360), which requires Neural Link.
        // Use kerenzikov (preset 14, also requires Neural Link).
        // To avoid prerequisite issues, manually set up Neural Link.
        let mut character = fresh_character(2);
        // Pre-install Neural Link so prerequisite check passes.
        character.cyberware.push(InstalledCyberware {
            id: CyberwareId("neural_link".into()),
            options: vec![],
        });
        // Deduct Neural Link's HL from humanity manually so the character's state
        // is internally consistent (humanity already reflects prior installs).
        // We leave humanity at 2 to force cyberpsychosis on the next install.

        let mut rng = Rng::seed_from_u64(5);

        // Kerenzikov preset HL is 14. humanity(2) - 14 = -12 → cyberpsychosis.
        let outcome = install_cyberware(
            &mut character,
            CyberwareId("kerenzikov".into()),
            &catalog,
            &mut rng,
            true, // at_creation uses preset = 14
        )
        .expect("install must succeed");

        assert!(
            outcome.triggered_cyberpsychosis,
            "humanity_after = {} must trigger cyberpsychosis",
            outcome.humanity_after
        );
        assert!(outcome.humanity_after <= 0);
    }

    /// Regression: installing an item with effects records `EffectInstanceId`s
    /// and the modifiers appear on the character's effect stack.
    /// Kerenzikov grants `InitiativeBonus(2)` (p.359).
    #[test]
    fn test_effects_pushed_onto_stack() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50);
        // Pre-install Neural Link prerequisite.
        character.cyberware.push(InstalledCyberware {
            id: CyberwareId("neural_link".into()),
            options: vec![],
        });

        let mut rng = Rng::seed_from_u64(6);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("kerenzikov".into()),
            &catalog,
            &mut rng,
            true,
        )
        .expect("install must succeed");

        // Kerenzikov has one effect: InitiativeBonus(2).
        assert_eq!(outcome.effects_added.len(), 1);
        let eid = outcome.effects_added[0];

        // The ID must appear in the effect stack.
        let found = character.effects.iter().any(|e| e.id == eid);
        assert!(found, "effect ID must appear in character.effects");

        // The modifier must be InitiativeBonus(2).
        use crate::effects::EffectModifier;
        let has_bonus = character
            .effects
            .iter_modifiers()
            .any(|m| matches!(m, EffectModifier::InitiativeBonus(2)));
        assert!(
            has_bonus,
            "Kerenzikov must grant InitiativeBonus(2) (p.359)"
        );
    }

    /// Regression: `emp_change` reports `Some((before, after))` when EMP
    /// crosses a tens boundary. humanity 50 → EMP 5; after 14 HL → 36 → EMP 3.
    /// Tests Kerenzikov (preset 14, Neural Link pre-installed).
    #[test]
    fn test_emp_change_reported_on_boundary_cross() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(50); // EMP = 5
        character.cyberware.push(InstalledCyberware {
            id: CyberwareId("neural_link".into()),
            options: vec![],
        });

        let mut rng = Rng::seed_from_u64(7);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("kerenzikov".into()),
            &catalog,
            &mut rng,
            true, // preset HL = 14: 50 - 14 = 36 → EMP 3
        )
        .expect("install must succeed");

        // EMP before = 50/10 = 5; after = 36/10 = 3 → crossed a boundary.
        assert_eq!(
            outcome.emp_change,
            Some((5, 3)),
            "EMP must drop from 5 to 3 after 14 HL (50→36)"
        );
    }

    /// Regression: `emp_change` is `None` when HL is small enough that EMP
    /// stays in the same tens bucket.
    #[test]
    fn test_emp_change_none_when_no_boundary_cross() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        // Humanity 50 → EMP 5. Interface Plugs preset HL = 3 → 47 → EMP 4.
        // That *is* a boundary cross. Use a lower HL item: Braindance Recorder
        // preset is 7 → 50-7 = 43 → EMP 4. That still crosses.
        // Use any Fashionware item with HL None (no loss → no boundary cross).
        let mut character = fresh_character(50);
        let mut rng = Rng::seed_from_u64(8);

        let outcome = install_cyberware(
            &mut character,
            CyberwareId("biomonitor".into()),
            &catalog,
            &mut rng,
            false,
        )
        .expect("Biomonitor install must succeed");

        assert_eq!(outcome.emp_change, None, "no HL → no EMP change");
    }

    /// Regression: option-slot exhaustion returns `OptionSlotsExhausted`.
    /// Neural Link has 5 slots. Kerenzikov (1 slot), Sandevistan (1 slot),
    /// Interface Plugs (1 slot), Chipware Socket (1 slot), Braindance Recorder
    /// (1 slot) = 5 used. Installing a sixth option must fail.
    #[test]
    fn test_option_slots_exhausted() {
        let catalog = load_cyberware_catalog(&catalog_dir()).expect("catalog must load");
        let mut character = fresh_character(200); // high humanity so HL doesn't matter
        let mut rng = Rng::seed_from_u64(9);

        // Install Neural Link first.
        install_cyberware(
            &mut character,
            CyberwareId("neural_link".into()),
            &catalog,
            &mut rng,
            true,
        )
        .expect("Neural Link");

        // Install 5 options that each cost 1 slot.
        for slug in [
            "kerenzikov",
            "sandevistan",
            "interface_plugs",
            "chipware_socket",
            "braindance_recorder",
        ] {
            install_cyberware(
                &mut character,
                CyberwareId(slug.into()),
                &catalog,
                &mut rng,
                true,
            )
            .unwrap_or_else(|e| panic!("install of '{slug}' must succeed: {e}"));
        }

        // Neural Link now has 5/5 slots used. Adding one more must fail.
        let result = install_cyberware(
            &mut character,
            CyberwareId("olfactory_boost".into()),
            &catalog,
            &mut rng,
            true,
        );

        match result {
            Err(RulesError::OptionSlotsExhausted { available, needed }) => {
                assert_eq!(available, 0, "no slots left");
                assert_eq!(needed, 1, "Olfactory Boost costs 1 slot");
            }
            other => panic!("expected OptionSlotsExhausted, got {other:?}"),
        }
    }

    /// Regression: deterministic IDs — same slug always produces the same
    /// EffectInstanceId regardless of call order.
    #[test]
    fn test_effect_ids_are_deterministic() {
        let id_a = deterministic_effect_id("kerenzikov", 0);
        let id_b = deterministic_effect_id("kerenzikov", 0);
        assert_eq!(id_a, id_b, "same slug + same index must produce same ID");

        let id_c = deterministic_effect_id("kerenzikov", 1);
        assert_ne!(id_a, id_c, "different index must produce different ID");

        let id_d = deterministic_effect_id("sandevistan", 0);
        assert_ne!(id_a, id_d, "different slug must produce different ID");
    }
}
