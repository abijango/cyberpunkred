//! Critical Injury tables — the Body table (p.187) and the Head table (p.188).
//!
//! When a Melee or Ranged Attack rolls two or more 6s on its damage dice,
//! a Critical Injury is inflicted (see p.187 — *Critical Injuries*). The
//! attacker rolls 2d6 on the appropriate table:
//!
//! - Critical Injuries to the **Body** (p.187), used unless the attack was
//!   an Aimed Shot to the head.
//! - Critical Injuries to the **Head** (p.188), used when an Aimed Shot
//!   targets the head.
//!
//! Every Critical Injury inflicts an *Injury Effect* (modeled here as a
//! `Vec<EffectModifier>` per the closed-enum from WP-003) plus **5 Bonus
//! Damage** that bypasses armor and hit-location modifiers (p.187 box text).
//! Some critical injuries also bump the target's Base Death Save Penalty
//! by 1 (the "Base Death Save Penalty is increased by 1" sentence on the
//! relevant row of pp.187–188).
//!
//! ## Loaders
//!
//! [`load_critical_injuries_body`] and [`load_critical_injuries_head`]
//! parse the corresponding RON file from `content/tables/`. Each returns
//! a [`Catalog<CriticalInjury>`] keyed by the slug `"crit_<table>_<roll>"`
//! (e.g. `"crit_body_05"` for the Broken Ribs row on the Body table). The
//! loader enforces:
//!
//! 1. Exactly 11 entries per table (one per roll on the 2..=12 range).
//! 2. Every entry's `table` matches the loader (Body / Head).
//! 3. Every entry's `bonus_damage == HpDamage(5)` (book p.187).
//! 4. Slugs are unique within the file.
//!
//! ## Rolling
//!
//! [`roll_critical_injury`] performs the 2d6 roll. Per the user-facing
//! semantics in WP-205: if the rolled kind is already in
//! `already_suffering`, the function returns `None` — the caller still
//! applies the 5 Bonus Damage but no *new* Critical Injury attaches. If
//! every kind on the table is already in `already_suffering`, the function
//! also returns `None`. (The rulebook on p.187 says "Roll 2d6 … until you
//! get a Critical Injury that the target isn't currently suffering"; the
//! WP simplifies this to a single roll-and-check, deferring the
//! duplicate-handling tax to the caller.)

use crate::catalog::Catalog;
use crate::dice::d6;
use crate::effects::modifier::{EffectModifier, HpDamage};
use crate::error::RulesError;
use crate::rng::Rng;
use crate::types::DV;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// CriticalInjuryKind
// ---------------------------------------------------------------------------

/// Closed enum of every distinct Critical Injury named on pp.187–188.
///
/// The Body table (p.187) and Head table (p.188) each list 11 rows
/// (one per roll on the 2..=12 inclusive range, the support of 2d6); the
/// `Foreign Object` row appears on both tables with the *same* mechanical
/// effect, so it shares a single variant here. All other variants are
/// table-unique.
///
/// Body table (p.187, rolls 2..=12):
/// 2 Dismembered Arm, 3 Dismembered Hand, 4 Collapsed Lung, 5 Broken Ribs,
/// 6 Broken Arm, 7 Foreign Object, 8 Broken Leg, 9 Torn Muscle,
/// 10 Spinal Injury, 11 Crushed Fingers, 12 Dismembered Leg.
///
/// Head table (p.188, rolls 2..=12):
/// 2 Lost Eye, 3 Brain Injury, 4 Damaged Eye, 5 Concussion,
/// 6 Broken Jaw, 7 Foreign Object, 8 Whiplash, 9 Cracked Skull,
/// 10 Damaged Ear, 11 Crushed Windpipe, 12 Lost Ear.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CriticalInjuryKind {
    // --- Body table (p.187) ---
    /// Body 2. The arm is gone; drop any items in that hand. Base Death
    /// Save Penalty is increased by 1. See p.187.
    DismemberedArm,
    /// Body 3. The hand is gone; drop any items in it. Base Death Save
    /// Penalty is increased by 1. See p.187.
    DismemberedHand,
    /// Body 4. -2 to MOVE (minimum 1). Base Death Save Penalty is
    /// increased by 1. See p.187.
    CollapsedLung,
    /// Body 5. At the end of every Turn where you move further than 4m on
    /// foot, you re-suffer this Critical Injury's Bonus Damage directly to
    /// your Hit Points. See p.187.
    BrokenRibs,
    /// Body 6. The Broken Arm cannot be used; drop any items in that hand.
    /// See p.187.
    BrokenArm,
    /// Body 7 / Head 7. At the end of every Turn where you move further
    /// than 4m on foot, you re-suffer this Critical Injury's Bonus Damage
    /// directly to your Hit Points. Quick Fix removes the Injury Effect
    /// permanently. See pp.187–188.
    ForeignObject,
    /// Body 8. -4 to MOVE (minimum 1). See p.187.
    BrokenLeg,
    /// Body 9. -2 to Melee Attacks. Quick Fix removes the Injury Effect
    /// permanently. See p.187.
    TornMuscle,
    /// Body 10. Next Turn, you cannot take an Action, but you can still
    /// take a Move Action. Base Death Save Penalty is increased by 1.
    /// See p.187.
    SpinalInjury,
    /// Body 11. -4 to all Actions involving that hand. See p.187.
    CrushedFingers,
    /// Body 12. The leg is gone; -6 to MOVE (minimum 1). You cannot dodge
    /// attacks. Base Death Save Penalty is increased by 1. See p.187.
    DismemberedLeg,

    // --- Head table (p.188) ---
    /// Head 2. The eye is gone; -4 to Ranged Attacks and Perception
    /// Checks involving vision. Base Death Save Penalty is increased by 1.
    /// See p.188.
    LostEye,
    /// Head 3. -2 to all Actions. Base Death Save Penalty is increased by
    /// 1. See p.188.
    BrainInjury,
    /// Head 4. -2 to Ranged Attacks and Perception Checks involving
    /// vision. See p.188.
    DamagedEye,
    /// Head 5. -2 to all Actions. Quick Fix removes the Injury Effect
    /// permanently. See p.188.
    Concussion,
    /// Head 6. -4 to all Actions involving speech. See p.188.
    BrokenJaw,
    /// Head 8. Base Death Save Penalty is increased by 1. See p.188.
    Whiplash,
    /// Head 9. Aimed Shots to your head multiply the damage that gets
    /// through your SP by 3 instead of 2. Base Death Save Penalty is
    /// increased by 1. See p.188.
    CrackedSkull,
    /// Head 10. Whenever you move further than 4m on foot in a Turn, you
    /// cannot take a Move Action on your next Turn. -2 to Perception
    /// Checks involving hearing. See p.188.
    DamagedEar,
    /// Head 11. You cannot speak. Base Death Save Penalty is increased by
    /// 1. See p.188.
    CrushedWindpipe,
    /// Head 12. The ear is gone. Whenever you move further than 4m on
    /// foot in a Turn, you cannot take a Move Action on your next Turn.
    /// -4 to Perception Checks involving hearing. Base Death Save Penalty
    /// is increased by 1. See p.188.
    LostEar,
}

// ---------------------------------------------------------------------------
// Healing structs
// ---------------------------------------------------------------------------

/// Method used to heal a Critical Injury, per the table on p.187 (Body)
/// and p.188 (Head). The "or" forms ("First Aid or Paramedic", "Paramedic
/// or Surgery") are encoded by storing the *minimally sufficient* method
/// (e.g. `FirstAid` covers any First-Aid-or-Paramedic row); the user
/// performing the check decides which skill to use.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum HealMethod {
    /// First Aid skill check (the lowest tier — any character with the
    /// skill can attempt). See p.223 for the broader healing rules.
    FirstAid,
    /// Paramedic skill check.
    Paramedic,
    /// Surgery procedure (typically performed at a Trauma Team / clinic).
    Surgery,
    /// Cell marked "N/A" on the table — no Quick Fix is possible
    /// (e.g. Dismembered Arm). Treatment is the only path.
    NotApplicable,
}

/// A Quick Fix entry — the field-medicine path for a Critical Injury.
/// See pp.187–188 (the "Quick Fix" column of each table) and p.223
/// (broader healing rules).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickFix {
    /// Skill / procedure used to attempt the Quick Fix.
    pub method: HealMethod,
    /// Difficulty Value of the Quick Fix check.
    pub dv: DV,
}

/// A Treatment entry — the long-term recovery path for a Critical
/// Injury. See pp.187–188 (the "Treatment" column of each table) and
/// p.223 (broader healing rules).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Treatment {
    /// Skill / procedure used to attempt Treatment.
    pub method: HealMethod,
    /// Difficulty Value of the Treatment check.
    pub dv: DV,
}

// ---------------------------------------------------------------------------
// CritTable + CriticalInjury
// ---------------------------------------------------------------------------

/// Which 11-entry critical injury table a `CriticalInjury` belongs to.
/// See p.187 (Body) and p.188 (Head).
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub enum CritTable {
    /// Critical Injuries to the Body (p.187). Used for any non-Aimed-Shot
    /// damage spike.
    Body,
    /// Critical Injuries to the Head (p.188). Used when an Aimed Shot
    /// targets the head.
    Head,
}

/// A single row of a Critical Injury table — the data for one named
/// injury. See p.187 (Body) and p.188 (Head).
///
/// `effects` are the closed-enum [`EffectModifier`]s the Injury Effect
/// translates into. `bonus_damage` is always `HpDamage(5)` per p.187 (the
/// box text "All Critical Injuries cause … 5 Bonus Damage"). `quick_fix`
/// is `None` for rows whose Quick Fix column reads "N/A" (e.g.
/// Dismembered Arm). `increases_death_save_penalty` is `true` for the
/// rows whose entry mentions "Base Death Save Penalty is increased by 1"
/// (p.187 / p.188).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticalInjury {
    /// Closed-enum identifier of this injury.
    pub kind: CriticalInjuryKind,
    /// Which table this row lives on (Body / Head).
    pub table: CritTable,
    /// The 2d6 roll value, in `2..=12`, that selects this row.
    pub d2d6_roll: u8,
    /// Display name as printed in the rulebook (e.g. "Dismembered Arm").
    pub display_name: String,
    /// The Injury Effect, decomposed into closed-enum modifiers from
    /// WP-003. Empty for entries whose only mechanical impact is the
    /// Death Save Penalty bump (e.g. Whiplash on the Head table).
    pub effects: Vec<EffectModifier>,
    /// Always `HpDamage(5)` per p.187 (the "5 Bonus Damage" callout).
    /// Stored explicitly so a regression test can pin the contract.
    pub bonus_damage: HpDamage,
    /// `Some` for rows with a Quick Fix column entry, `None` for rows
    /// whose Quick Fix column is "N/A".
    pub quick_fix: Option<QuickFix>,
    /// The Treatment column entry — every row has one (no "N/A"
    /// Treatments on either table).
    pub treatment: Treatment,
    /// `true` iff the row's text includes "Base Death Save Penalty is
    /// increased by 1" (pp.187–188).
    pub increases_death_save_penalty: bool,
}

// ---------------------------------------------------------------------------
// In-code table builders
// ---------------------------------------------------------------------------

/// Map a 2d6 roll to its `CriticalInjuryKind` on the Body table (p.187).
/// Returns `None` for out-of-range rolls (i.e. anything outside 2..=12).
pub fn body_kind_for_roll(roll: u8) -> Option<CriticalInjuryKind> {
    let kind = match roll {
        2 => CriticalInjuryKind::DismemberedArm,
        3 => CriticalInjuryKind::DismemberedHand,
        4 => CriticalInjuryKind::CollapsedLung,
        5 => CriticalInjuryKind::BrokenRibs,
        6 => CriticalInjuryKind::BrokenArm,
        7 => CriticalInjuryKind::ForeignObject,
        8 => CriticalInjuryKind::BrokenLeg,
        9 => CriticalInjuryKind::TornMuscle,
        10 => CriticalInjuryKind::SpinalInjury,
        11 => CriticalInjuryKind::CrushedFingers,
        12 => CriticalInjuryKind::DismemberedLeg,
        _ => return None,
    };
    Some(kind)
}

/// Map a 2d6 roll to its `CriticalInjuryKind` on the Head table (p.188).
/// Returns `None` for out-of-range rolls (i.e. anything outside 2..=12).
pub fn head_kind_for_roll(roll: u8) -> Option<CriticalInjuryKind> {
    let kind = match roll {
        2 => CriticalInjuryKind::LostEye,
        3 => CriticalInjuryKind::BrainInjury,
        4 => CriticalInjuryKind::DamagedEye,
        5 => CriticalInjuryKind::Concussion,
        6 => CriticalInjuryKind::BrokenJaw,
        7 => CriticalInjuryKind::ForeignObject,
        8 => CriticalInjuryKind::Whiplash,
        9 => CriticalInjuryKind::CrackedSkull,
        10 => CriticalInjuryKind::DamagedEar,
        11 => CriticalInjuryKind::CrushedWindpipe,
        12 => CriticalInjuryKind::LostEar,
        _ => return None,
    };
    Some(kind)
}

/// Iterate every kind on the given table (11 entries each — rolls 2..=12).
fn kinds_on_table(table: CritTable) -> Vec<CriticalInjuryKind> {
    (2..=12u8)
        .map(|r| match table {
            CritTable::Body => body_kind_for_roll(r).expect("2..=12 covers the body table"),
            CritTable::Head => head_kind_for_roll(r).expect("2..=12 covers the head table"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// roll_critical_injury
// ---------------------------------------------------------------------------

/// Roll 2d6 on the named table and return the resulting kind, applying
/// the WP-205 duplicate-handling rule.
///
/// **Semantics** (per WP-205):
///
/// 1. Roll two d6s and sum them (range 2..=12).
/// 2. Look up the corresponding `CriticalInjuryKind` on `table`.
/// 3. If the kind is already in `already_suffering`, return `None` — the
///    caller still applies the 5 Bonus Damage (per p.187 box text) but
///    no *new* Critical Injury attaches.
/// 4. If every kind on the table is already in `already_suffering`,
///    return `None` regardless of what the dice say.
///
/// The WP simplifies the rulebook's "Roll 2d6 … until you get a Critical
/// Injury that the target isn't currently suffering" (p.187) to a single
/// roll-and-check; this keeps the function deterministic in dice
/// consumption and makes it trivial to reason about replay logs.
///
/// # Determinism
///
/// Consumes exactly two `d6` calls from `rng` per invocation.
pub fn roll_critical_injury(
    table: CritTable,
    already_suffering: &[CriticalInjuryKind],
    rng: &mut Rng,
) -> Option<CriticalInjuryKind> {
    // Short-circuit when the entire table is already exhausted.
    let table_kinds = kinds_on_table(table);
    if table_kinds.iter().all(|k| already_suffering.contains(k)) {
        // Still consume two dice so seed-replay logs line up across
        // "all-exhausted" and "duplicate" branches.
        let _ = d6(rng);
        let _ = d6(rng);
        return None;
    }

    let a = d6(rng);
    let b = d6(rng);
    let roll = a + b;
    let kind = match table {
        CritTable::Body => body_kind_for_roll(roll),
        CritTable::Head => head_kind_for_roll(roll),
    }
    .expect("2d6 sum is always in 2..=12");

    if already_suffering.contains(&kind) {
        None
    } else {
        Some(kind)
    }
}

// ---------------------------------------------------------------------------
// Loaders
// ---------------------------------------------------------------------------

/// On-disk schema for `content/tables/critical_injuries_*.ron`.
///
/// `entries` is a flat list whose order is irrelevant; the loader keys
/// each entry by `slug` and validates roll coverage / table match.
#[derive(Debug, Deserialize, Serialize)]
struct CriticalInjuriesFile {
    table: CritTable,
    entries: Vec<CriticalInjuryFileEntry>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CriticalInjuryFileEntry {
    slug: String,
    kind: CriticalInjuryKind,
    table: CritTable,
    d2d6_roll: u8,
    display_name: String,
    effects: Vec<EffectModifier>,
    bonus_damage: HpDamage,
    quick_fix: Option<QuickFix>,
    treatment: Treatment,
    increases_death_save_penalty: bool,
}

/// Load the **Body** Critical Injuries table from a RON file at `path`.
/// See p.187 for the canonical table.
///
/// On success returns a `Catalog<CriticalInjury>` with exactly 11 entries,
/// one per roll on `2..=12`. On failure returns
/// [`RulesError::CatalogLoadFailed`].
///
/// Loader-enforced invariants:
/// 1. The file's top-level `table` field is `Body`.
/// 2. Exactly 11 entries appear, with rolls covering `2..=12` exactly
///    once each.
/// 3. Every entry's `table == Body` and `bonus_damage == HpDamage(5)`.
/// 4. Slugs are unique within the file.
pub fn load_critical_injuries_body(path: &Path) -> Result<Catalog<CriticalInjury>, RulesError> {
    load_critical_injuries(path, CritTable::Body)
}

/// Load the **Head** Critical Injuries table from a RON file at `path`.
/// See p.188 for the canonical table.
///
/// On success returns a `Catalog<CriticalInjury>` with exactly 11 entries,
/// one per roll on `2..=12`. On failure returns
/// [`RulesError::CatalogLoadFailed`].
///
/// Loader-enforced invariants:
/// 1. The file's top-level `table` field is `Head`.
/// 2. Exactly 11 entries appear, with rolls covering `2..=12` exactly
///    once each.
/// 3. Every entry's `table == Head` and `bonus_damage == HpDamage(5)`.
/// 4. Slugs are unique within the file.
pub fn load_critical_injuries_head(path: &Path) -> Result<Catalog<CriticalInjury>, RulesError> {
    load_critical_injuries(path, CritTable::Head)
}

fn load_critical_injuries(
    path: &Path,
    expected_table: CritTable,
) -> Result<Catalog<CriticalInjury>, RulesError> {
    let bytes = std::fs::read_to_string(path).map_err(|e| RulesError::CatalogLoadFailed {
        path: path.to_path_buf(),
        source: format!("read failed: {e}"),
    })?;
    let parsed: CriticalInjuriesFile =
        ron::de::from_str(&bytes).map_err(|e| RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!("parse failed: {e}"),
        })?;

    if parsed.table != expected_table {
        return Err(RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!(
                "file declares table {:?} but loader expected {:?}",
                parsed.table, expected_table
            ),
        });
    }

    if parsed.entries.len() != 11 {
        return Err(RulesError::CatalogLoadFailed {
            path: path.to_path_buf(),
            source: format!(
                "expected exactly 11 entries (rolls 2..=12 inclusive), got {}",
                parsed.entries.len()
            ),
        });
    }

    let mut entries: HashMap<String, CriticalInjury> = HashMap::with_capacity(11);
    let mut seen_rolls = [false; 13]; // index 2..=12 used

    for row in parsed.entries {
        if row.table != expected_table {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "entry '{}' declares table {:?} but loader is loading {:?}",
                    row.slug, row.table, expected_table
                ),
            });
        }
        if !(2..=12).contains(&row.d2d6_roll) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "entry '{}' has out-of-range d2d6_roll {} (must be 2..=12)",
                    row.slug, row.d2d6_roll
                ),
            });
        }
        if seen_rolls[row.d2d6_roll as usize] {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate d2d6_roll {} in file", row.d2d6_roll),
            });
        }
        seen_rolls[row.d2d6_roll as usize] = true;

        if row.bonus_damage != HpDamage(5) {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "entry '{}' has bonus_damage {:?} but every critical's bonus damage must be HpDamage(5) per p.187",
                    row.slug, row.bonus_damage
                ),
            });
        }

        let expected_kind = match expected_table {
            CritTable::Body => body_kind_for_roll(row.d2d6_roll),
            CritTable::Head => head_kind_for_roll(row.d2d6_roll),
        }
        .expect("d2d6_roll already validated to 2..=12");
        if row.kind != expected_kind {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!(
                    "entry '{}' (roll {}) declares kind {:?}, expected {:?} per pp.187-188",
                    row.slug, row.d2d6_roll, row.kind, expected_kind
                ),
            });
        }

        let entry = CriticalInjury {
            kind: row.kind,
            table: row.table,
            d2d6_roll: row.d2d6_roll,
            display_name: row.display_name,
            effects: row.effects,
            bonus_damage: row.bonus_damage,
            quick_fix: row.quick_fix,
            treatment: row.treatment,
            increases_death_save_penalty: row.increases_death_save_penalty,
        };
        if entries.insert(row.slug.clone(), entry).is_some() {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("duplicate slug: '{}'", row.slug),
            });
        }
    }

    // Final check: every roll 2..=12 was supplied.
    for (r, supplied) in seen_rolls.iter().enumerate().skip(2) {
        if !supplied {
            return Err(RulesError::CatalogLoadFailed {
                path: path.to_path_buf(),
                source: format!("missing entry for d2d6_roll {r}"),
            });
        }
    }

    Ok(Catalog::new(entries))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effects::modifier::Hand;
    use rand::SeedableRng;
    use std::path::PathBuf;

    fn body_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop(); // crates/rules -> crates
        p.pop(); // crates -> repo root
        p.push("content");
        p.push("tables");
        p.push("critical_injuries_body.ron");
        p
    }

    fn head_path() -> PathBuf {
        let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        p.pop();
        p.pop();
        p.push("content");
        p.push("tables");
        p.push("critical_injuries_head.ron");
        p
    }

    /// Acceptance: every roll 2..=12 on the Body table maps to a defined
    /// kind, the loaded catalog has 11 entries (one per 2d6 roll value
    /// in 2..=12 — the support of 2d6 has 11 values, *not* 12 as the
    /// WP-205 description's "12-entry" wording loosely says), and every
    /// roll/kind combination in the catalog round-trips through
    /// `body_kind_for_roll`.
    #[test]
    fn test_body_table_12_entries() {
        for r in 2..=12u8 {
            assert!(
                body_kind_for_roll(r).is_some(),
                "body table has no entry for roll {r}"
            );
        }
        assert!(body_kind_for_roll(1).is_none());
        assert!(body_kind_for_roll(13).is_none());

        let cat = load_critical_injuries_body(&body_path()).expect("body table must load");
        assert_eq!(
            cat.len(),
            11,
            "body table must load exactly 11 entries (one per roll on 2..=12)"
        );

        // Every entry's kind agrees with the in-code map.
        for (_, ci) in cat.iter() {
            assert_eq!(ci.table, CritTable::Body);
            assert_eq!(
                Some(ci.kind.clone()),
                body_kind_for_roll(ci.d2d6_roll),
                "kind/roll mismatch in body catalog at roll {}",
                ci.d2d6_roll
            );
        }
    }

    /// Acceptance: every roll 2..=12 on the Head table maps to a defined
    /// kind, the loaded catalog has 11 entries (see
    /// `test_body_table_12_entries` for the 11-vs-12 nomenclature note),
    /// and every roll/kind combination in the catalog round-trips
    /// through `head_kind_for_roll`.
    #[test]
    fn test_head_table_12_entries() {
        for r in 2..=12u8 {
            assert!(
                head_kind_for_roll(r).is_some(),
                "head table has no entry for roll {r}"
            );
        }
        assert!(head_kind_for_roll(1).is_none());
        assert!(head_kind_for_roll(13).is_none());

        let cat = load_critical_injuries_head(&head_path()).expect("head table must load");
        assert_eq!(
            cat.len(),
            11,
            "head table must load exactly 11 entries (one per roll on 2..=12)"
        );

        for (_, ci) in cat.iter() {
            assert_eq!(ci.table, CritTable::Head);
            assert_eq!(
                Some(ci.kind.clone()),
                head_kind_for_roll(ci.d2d6_roll),
                "kind/roll mismatch in head catalog at roll {}",
                ci.d2d6_roll
            );
        }
    }

    /// Acceptance: when every kind on the body table is already
    /// suffered, `roll_critical_injury` returns None (per WP-205
    /// semantics). The rulebook on p.187 says "Roll 2d6 … until you get
    /// a Critical Injury that the target isn't currently suffering";
    /// when nothing on the table can satisfy that predicate, no new
    /// critical attaches.
    #[test]
    fn test_reroll_if_already_suffering() {
        let all_body: Vec<CriticalInjuryKind> =
            (2..=12u8).map(|r| body_kind_for_roll(r).unwrap()).collect();

        // Try across many seeds — must always return None.
        for seed in 0..50u64 {
            let mut rng = Rng::seed_from_u64(seed);
            let r = roll_critical_injury(CritTable::Body, &all_body, &mut rng);
            assert_eq!(
                r, None,
                "all-suffered body table must yield None, got {r:?} at seed {seed}"
            );
        }

        // And the same property holds for Head.
        let all_head: Vec<CriticalInjuryKind> =
            (2..=12u8).map(|r| head_kind_for_roll(r).unwrap()).collect();
        for seed in 0..50u64 {
            let mut rng = Rng::seed_from_u64(seed);
            assert_eq!(
                roll_critical_injury(CritTable::Head, &all_head, &mut rng),
                None
            );
        }
    }

    /// Acceptance: `roll_critical_injury` returns None when the rolled
    /// kind is already in `already_suffering`, even if other kinds on
    /// the table remain available (per WP-205 semantics).
    #[test]
    fn test_roll_returns_none_for_duplicate_only() {
        // Find a seed where 2d6 rolls a 7 → ForeignObject on the Body
        // table. Then claim ForeignObject is already suffered.
        let seed = (0u64..1_000_000)
            .find(|&seed| {
                let mut r = Rng::seed_from_u64(seed);
                let a = d6(&mut r);
                let b = d6(&mut r);
                a + b == 7
            })
            .expect("must find a seed rolling 2d6 == 7");

        let mut rng = Rng::seed_from_u64(seed);
        let result = roll_critical_injury(
            CritTable::Body,
            &[CriticalInjuryKind::ForeignObject],
            &mut rng,
        );
        assert_eq!(
            result, None,
            "rolling a duplicate kind must yield None per WP-205"
        );

        // And conversely: with no overlap, the same seed yields Some.
        let mut rng = Rng::seed_from_u64(seed);
        let result = roll_critical_injury(CritTable::Body, &[], &mut rng);
        assert_eq!(result, Some(CriticalInjuryKind::ForeignObject));
    }

    /// Acceptance: per p.187, the row-2 Dismembered Arm flag
    /// `increases_death_save_penalty` is set in the loaded catalog.
    /// The test scans the entire catalog by roll value (rather than
    /// slug) so that a slug rename in the RON file doesn't mask a
    /// data regression.
    #[test]
    fn test_dismembered_arm_increases_death_save() {
        let cat = load_critical_injuries_body(&body_path()).expect("body table must load");
        let arm = cat
            .iter()
            .find(|(_, ci)| ci.kind == CriticalInjuryKind::DismemberedArm)
            .map(|(_, ci)| ci)
            .expect("DismemberedArm must be in the body catalog");
        assert_eq!(arm.d2d6_roll, 2, "Dismembered Arm is the 2 row (p.187)");
        assert!(
            arm.increases_death_save_penalty,
            "DismemberedArm must bump base Death Save Penalty (p.187)"
        );

        // And confirm the row carries a DeathSavePenaltyDelta(+1) modifier
        // — p.187: "Base Death Save Penalty is increased by 1."
        let bumps = arm
            .effects
            .iter()
            .filter(|m| matches!(m, EffectModifier::DeathSavePenaltyDelta(1)))
            .count();
        assert_eq!(
            bumps, 1,
            "DismemberedArm must include exactly one DeathSavePenaltyDelta(+1)"
        );
    }

    /// Acceptance: per p.187, Broken Ribs creates a movement-trigger
    /// effect — `DamageOnMovementOver { threshold_m: 4, damage: HpDamage(5) }`.
    #[test]
    fn test_broken_ribs_movement_trigger() {
        let cat = load_critical_injuries_body(&body_path()).expect("body table must load");
        let ribs = cat
            .iter()
            .find(|(_, ci)| ci.kind == CriticalInjuryKind::BrokenRibs)
            .map(|(_, ci)| ci)
            .expect("BrokenRibs must be in the body catalog");
        assert_eq!(ribs.d2d6_roll, 5);

        let trigger = ribs.effects.iter().find_map(|m| match m {
            EffectModifier::DamageOnMovementOver {
                threshold_m,
                damage,
            } => Some((*threshold_m, *damage)),
            _ => None,
        });
        assert_eq!(
            trigger,
            Some((4, HpDamage(5))),
            "Broken Ribs must declare DamageOnMovementOver{{4m, 5}} per p.187"
        );
    }

    /// Acceptance: per p.187 box text "All Critical Injuries cause … 5
    /// Bonus Damage", every entry in both tables has
    /// `bonus_damage == HpDamage(5)`.
    #[test]
    fn test_bonus_damage_5() {
        for path in [body_path(), head_path()] {
            let cat = if path == body_path() {
                load_critical_injuries_body(&path).expect("body load")
            } else {
                load_critical_injuries_head(&path).expect("head load")
            };
            for (slug, ci) in cat.iter() {
                assert_eq!(
                    ci.bonus_damage,
                    HpDamage(5),
                    "{slug}: every Critical Injury's bonus_damage is HpDamage(5) per p.187"
                );
            }
        }
    }

    /// Acceptance: a `CriticalInjury` round-trips through RON
    /// serialisation (write → read → equality).
    #[test]
    fn test_critical_round_trip_ron() {
        let original = CriticalInjury {
            kind: CriticalInjuryKind::BrokenRibs,
            table: CritTable::Body,
            d2d6_roll: 5,
            display_name: "Broken Ribs".to_string(),
            effects: vec![EffectModifier::DamageOnMovementOver {
                threshold_m: 4,
                damage: HpDamage(5),
            }],
            bonus_damage: HpDamage(5),
            quick_fix: Some(QuickFix {
                method: HealMethod::Paramedic,
                dv: DV(13),
            }),
            treatment: Treatment {
                method: HealMethod::Paramedic,
                dv: DV(15),
            },
            increases_death_save_penalty: false,
        };

        let s = ron::ser::to_string(&original).expect("serialise must succeed");
        let restored: CriticalInjury = ron::de::from_str(&s).expect("deserialise must succeed");
        assert_eq!(restored, original);
    }

    /// Regression: a 2d6 roll consumes exactly two dice from the RNG —
    /// callers replaying logs must be able to predict the dice budget.
    #[test]
    fn test_roll_consumes_exactly_two_d6() {
        // Compute a baseline of 5 d6 calls.
        let mut baseline = Rng::seed_from_u64(123);
        let baseline_seq: Vec<u8> = (0..5).map(|_| d6(&mut baseline)).collect();

        // A second RNG: do one roll_critical_injury (2 dice consumed),
        // then 3 more d6 calls. The trailing 3 must equal the trailing
        // 3 of the baseline (i.e. d6 #3, #4, #5).
        let mut other = Rng::seed_from_u64(123);
        let _ = roll_critical_injury(CritTable::Body, &[], &mut other);
        let trailing: Vec<u8> = (0..3).map(|_| d6(&mut other)).collect();

        assert_eq!(&trailing, &baseline_seq[2..]);
    }

    /// Regression: when called on an exhausted table, the function still
    /// consumes 2 dice (so the seed-replay log doesn't desync between
    /// "duplicate" and "exhausted" branches).
    #[test]
    fn test_exhausted_table_still_consumes_two_dice() {
        let all_body: Vec<CriticalInjuryKind> =
            (2..=12u8).map(|r| body_kind_for_roll(r).unwrap()).collect();

        let mut rng_a = Rng::seed_from_u64(7);
        let _ = roll_critical_injury(CritTable::Body, &all_body, &mut rng_a);
        let next_a = d6(&mut rng_a);

        let mut rng_b = Rng::seed_from_u64(7);
        let _ = d6(&mut rng_b);
        let _ = d6(&mut rng_b);
        let next_b = d6(&mut rng_b);

        assert_eq!(next_a, next_b);
    }

    /// Regression: every "Base Death Save Penalty is increased by 1"
    /// row on p.187 (Body table) is flagged in the loaded catalog. The
    /// rulebook lists these on rows 2 (Dismembered Arm), 3 (Dismembered
    /// Hand), 4 (Collapsed Lung), 10 (Spinal Injury), and 12
    /// (Dismembered Leg).
    #[test]
    fn test_body_death_save_rows() {
        let cat = load_critical_injuries_body(&body_path()).expect("body load");
        let expected_rolls: Vec<u8> = vec![2, 3, 4, 10, 12];
        for (_, ci) in cat.iter() {
            let should_bump = expected_rolls.contains(&ci.d2d6_roll);
            assert_eq!(
                ci.increases_death_save_penalty, should_bump,
                "body roll {} death-save flag mismatch",
                ci.d2d6_roll
            );
        }
    }

    /// Regression: every "Base Death Save Penalty is increased by 1"
    /// row on p.188 (Head table) is flagged in the loaded catalog. The
    /// rulebook lists these on rows 2 (Lost Eye), 3 (Brain Injury), 8
    /// (Whiplash), 9 (Cracked Skull), 11 (Crushed Windpipe), and 12
    /// (Lost Ear).
    #[test]
    fn test_head_death_save_rows() {
        let cat = load_critical_injuries_head(&head_path()).expect("head load");
        let expected_rolls: Vec<u8> = vec![2, 3, 8, 9, 11, 12];
        for (_, ci) in cat.iter() {
            let should_bump = expected_rolls.contains(&ci.d2d6_roll);
            assert_eq!(
                ci.increases_death_save_penalty, should_bump,
                "head roll {} death-save flag mismatch",
                ci.d2d6_roll
            );
        }
    }

    /// Regression: roll-distribution sanity for the body table. Across
    /// many seeds with no `already_suffering`, every kind on the table
    /// is rolled at least once.
    #[test]
    fn test_roll_covers_every_body_kind() {
        let mut seen: Vec<CriticalInjuryKind> = Vec::new();
        for seed in 0..5_000u64 {
            let mut rng = Rng::seed_from_u64(seed);
            if let Some(k) = roll_critical_injury(CritTable::Body, &[], &mut rng) {
                if !seen.contains(&k) {
                    seen.push(k);
                }
            }
        }
        assert_eq!(seen.len(), 11, "every body-table kind must appear");
    }

    /// Regression: the Head table's row-9 (Cracked Skull) carries the
    /// damage-multiplier text on p.188 ("Aimed Shots to your head
    /// multiply the damage that gets through your SP by 3 instead of
    /// 2"). The current `EffectModifier` enum from WP-003 has no
    /// dedicated variant for this; the catalog therefore stores it as
    /// the +1 Death Save Penalty bump (also on the row) and a flag,
    /// with the multiplier deferred to the combat engine reading the
    /// kind directly. This regression test pins the +1 DSP bump and
    /// ensures the row is loaded.
    #[test]
    fn test_cracked_skull_row() {
        let cat = load_critical_injuries_head(&head_path()).expect("head load");
        let cs = cat
            .iter()
            .find(|(_, ci)| ci.kind == CriticalInjuryKind::CrackedSkull)
            .map(|(_, ci)| ci)
            .expect("CrackedSkull must be in head catalog");
        assert_eq!(cs.d2d6_roll, 9);
        assert!(cs.increases_death_save_penalty);
    }

    /// Regression: HealMethod::NotApplicable rows have `quick_fix ==
    /// None` (the loader stores N/A as `None` rather than as a
    /// `HealMethod::NotApplicable` quick-fix entry). Spot-check
    /// Dismembered Arm.
    #[test]
    fn test_quick_fix_none_for_na() {
        let cat = load_critical_injuries_body(&body_path()).expect("body load");
        let arm = cat
            .iter()
            .find(|(_, ci)| ci.kind == CriticalInjuryKind::DismemberedArm)
            .map(|(_, ci)| ci)
            .expect("DismemberedArm row");
        assert!(arm.quick_fix.is_none(), "Dismembered Arm has no Quick Fix");
    }

    /// Regression: `CrushedFingers` on the body table emits a
    /// `HandActionsPenalty(-4)` per p.187.
    #[test]
    fn test_crushed_fingers_hand_penalty() {
        let cat = load_critical_injuries_body(&body_path()).expect("body load");
        let cf = cat
            .iter()
            .find(|(_, ci)| ci.kind == CriticalInjuryKind::CrushedFingers)
            .map(|(_, ci)| ci)
            .expect("CrushedFingers row");
        let penalty = cf.effects.iter().find_map(|m| match m {
            EffectModifier::HandActionsPenalty { hand, by } => Some((*hand, *by)),
            _ => None,
        });
        assert_eq!(penalty, Some((Hand::Either, -4)));
    }
}
