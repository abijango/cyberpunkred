//! Backup (Lawman Role Ability) — WP-515.
//!
//! **Rulebook name:** The rulebook (p.158) calls this ability **"Backup"**.
//! The Lawman's Role Ability is Backup. Lawmen can call upon a group of fellow
//! law enforcement officers whose size and capabilities scale with the Lawman's
//! Backup Rank.
//!
//! ## Rulebook mechanics (pp.158–159)
//!
//! When in danger, the Lawman can use their Action to attempt to call Backup.
//! They roll a d10 and succeed if the result is ≤ their Backup Rank. On
//! success, they roll 1d6 to find out how many Rounds until backup arrives on
//! scene (p.158). On a roll of 6, the backup that arrives is from one tier
//! higher than normal (unless rank 10, where two separate groups arrive).
//!
//! ## Backup Rank table (pp.158–159)
//!
//! | Rank | Group                           | Count | Notes                                    |
//! |------|---------------------------------|-------|------------------------------------------|
//! | 1–2  | Corporate Security              | 4     | Renta-cops on foot, Heavy Pistols, Kevlar |
//! | 3–4  | Local Beat Cops                 | 4     | 2 Compact Groundcars, Heavy Pistols, Kevlar |
//! | 5–7  | Sheriff's Department            | 2     | High Perf. Groundcar, Heavy Pistols + AR, Heavy Armorjack |
//! | 8    | Recovery Zone Marshal           | 1     | Superbike, VH Pistol + AR + Grenade Launcher, Flak Armor |
//! | 9    | C-SWAT                          | 2     | AV-4, Assault Rifles + Rocket Launchers, Metalgear |
//! | 10   | National Law Enforcement/Interpol| 2+   | AV-4, VH Pistols + ARs, Light Armorjack |
//!
//! ## Arrival time (pp.158–159) — deviation note
//!
//! The rulebook specifies that arrival time is a **random d6 roll** in Rounds
//! (1 Round = 3 seconds). This is a random mechanic, not a deterministic
//! rank-scaled value. The WP-515 spec asks for `backup_arrival_minutes(rank) -> u16`,
//! a deterministic function. This implementation encodes a **rank-scaled
//! expected arrival in minutes**: higher-rank units are better equipped and
//! faster-responding, so their expected arrival decreases as rank grows. The
//! d6-Rounds RAW mechanic is the GM-level resolution; this function provides
//! a planning/narrative estimate. Deviation flagged in PR.
//!
//! See pp.158–159.

use crate::character::data::Role;
use crate::character::Character;

// ──────────────────────────────────────────────────────────────────────────────
// Core rank query
// ──────────────────────────────────────────────────────────────────────────────

/// Returns the character's effective Backup rank.
///
/// Per p.158, Backup is the Lawman's Role Ability and its rank equals
/// `character.role_rank` when the character is a [`Role::Lawman`]. For any
/// other role the ability does not apply; this function returns `0` so callers
/// can skip application without special-casing the role check.
///
/// # Examples
///
/// ```
/// # use cpr_rules::roles::backup::backup_rank;
/// # use cpr_rules::character::Character;
/// // See test_backup_rank_for_lawman and test_backup_rank_zero_for_non_lawman.
/// ```
///
/// See p.158.
pub fn backup_rank(character: &Character) -> u8 {
    // See p.158: Backup is exclusively the Lawman Role Ability.
    if character.role == Role::Lawman {
        character.role_rank
    } else {
        0
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Backup size
// ──────────────────────────────────────────────────────────────────────────────

/// Size/tier of backup that responds to a Lawman's call.
///
/// Size scales with Backup Rank per pp.158–159. The labels correspond to the
/// named backup groups in the rulebook; the [`None`] variant is returned for
/// rank 0 (no Backup ability).
///
/// See pp.158–159.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BackupSize {
    /// Rank 0: no Backup ability — ability not available. See p.158.
    None,
    /// Rank 1–2: a single backup officer group — Corporate Security (4
    /// renta-cops on foot, Heavy Pistols, Kevlar). See p.158.
    Lone,
    /// Rank 3–4: Local Beat Cops (4 local cops in 2 Compact Groundcars,
    /// Heavy Pistols, Kevlar). See p.158.
    Pair,
    /// Rank 5–7: Sheriff's Department (2 "County Mounties" in a High
    /// Performance Groundcar, Heavy Pistols + Assault Rifles, Heavy
    /// Armorjack). See p.158.
    Squad,
    /// Rank 8: Recovery Zone Marshal (lone Lawman on Superbike, Very Heavy
    /// Pistol + Assault Rifle + Grenade Launcher, Flak Armor). See p.159.
    Platoon,
    /// Rank 9–10: elite response — C-SWAT at rank 9 (2 heavy hitters in
    /// AV-4, Assault Rifles + Rocket Launchers, Metalgear); National Law
    /// Enforcement / Interpol / FBI / Netwatch at rank 10 (pairs of serious
    /// hitters in AV-4, Very Heavy Pistols + Assault Rifles, Light
    /// Armorjack). See p.159.
    Strikeforce,
}

/// Returns the [`BackupSize`] tier corresponding to the given Backup rank.
///
/// Mapping per pp.158–159:
///
/// | Rank  | BackupSize    |
/// |-------|---------------|
/// | 0     | `None`        |
/// | 1–2   | `Lone`        |
/// | 3–4   | `Pair`        |
/// | 5–7   | `Squad`       |
/// | 8     | `Platoon`     |
/// | 9–10  | `Strikeforce` |
///
/// Ranks above 10 are treated as rank 10 (Strikeforce).
///
/// See pp.158–159.
pub fn backup_size(rank: u8) -> BackupSize {
    // See pp.158–159: Backup Rank table.
    match rank {
        0 => BackupSize::None,
        1..=2 => BackupSize::Lone,
        3..=4 => BackupSize::Pair,
        5..=7 => BackupSize::Squad,
        8 => BackupSize::Platoon,
        _ => BackupSize::Strikeforce, // rank 9, 10, or any value above 10
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Arrival time
// ──────────────────────────────────────────────────────────────────────────────

/// Expected time for backup to arrive on scene, in minutes.
///
/// The rulebook (p.158) specifies arrival as a d6 roll in Rounds (3 seconds
/// each). This function returns a **deterministic rank-scaled planning
/// estimate in minutes** instead, reflecting that higher-rank response units
/// are more capable and faster to deploy. This is a purposeful simplification
/// from the WP-515 spec — see the module-level deviation note.
///
/// Mapping per pp.158–159 (narrative context and transport mode inform
/// the estimate):
///
/// | Rank  | Arrival (minutes) | Rationale                                      |
/// |-------|-------------------|------------------------------------------------|
/// | 0     | 0                 | No Backup ability                              |
/// | 1–2   | 10                | Renta-cops on foot; slow response             |
/// | 3–4   | 8                 | Local Beat Cops in Compact Groundcars          |
/// | 5–7   | 6                 | Sheriff's Dept in High Performance Groundcar   |
/// | 8     | 4                 | Recovery Zone Marshal on Superbike             |
/// | 9–10  | 2                 | C-SWAT / Interpol via AV-4 (airborne)          |
///
/// See pp.158–159.
pub fn backup_arrival_minutes(rank: u8) -> u16 {
    // See pp.158–159: arrival time scales inversely with Backup Rank.
    // Higher-rank units have faster transport (AV-4 vs on foot).
    // RAW uses d6 Rounds (not minutes); this function provides a deterministic
    // planning estimate. Deviation noted in module docs and PR.
    match rank {
        0 => 0,
        1..=2 => 10,
        3..=4 => 8,
        5..=7 => 6,
        8 => 4,
        _ => 2, // rank 9, 10+: elite airborne response
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

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
            id: CharacterId(Uuid::from_u128(0xBACC_u128)),
            name: "Slack".to_string(),
            handle: None,
            role,
            role_rank,
            stats: StatBlock {
                int: 6,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 7,
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

    // ── backup_rank ───────────────────────────────────────────────────────────

    #[test]
    fn test_backup_rank_for_lawman() {
        // Per p.158: Lawman's Backup rank equals their Role Ability rank.
        let character = make_character(Role::Lawman, 4);
        assert_eq!(backup_rank(&character), 4);
    }

    #[test]
    fn test_backup_rank_zero_for_non_lawman() {
        // Non-Lawman roles do not have the Backup ability — rank is 0. See p.158.
        let character = make_character(Role::Solo, 4);
        assert_eq!(backup_rank(&character), 0);
    }

    #[test]
    fn test_backup_rank_zero_for_netrunner() {
        // Additional non-Lawman check: Netrunner returns 0. See p.158.
        let character = make_character(Role::Netrunner, 7);
        assert_eq!(backup_rank(&character), 0);
    }

    #[test]
    fn test_backup_rank_max() {
        // Role rank caps at 10 per rulebook p.71; backup_rank mirrors it directly.
        let character = make_character(Role::Lawman, 10);
        assert_eq!(backup_rank(&character), 10);
    }

    // ── backup_size ───────────────────────────────────────────────────────────

    #[test]
    fn test_backup_size_at_each_tier() {
        // Per pp.158–159: verify every tier boundary maps to the correct BackupSize.

        // Rank 0: no ability.
        assert_eq!(backup_size(0), BackupSize::None);

        // Rank 1: lone Corporate Security. See p.158.
        assert_eq!(backup_size(1), BackupSize::Lone);
        // Rank 2: same tier. See p.158.
        assert_eq!(backup_size(2), BackupSize::Lone);

        // Rank 3: Local Beat Cops. See p.158.
        assert_eq!(backup_size(3), BackupSize::Pair);
        // Rank 4: same tier. See p.158.
        assert_eq!(backup_size(4), BackupSize::Pair);

        // Rank 5: Sheriff's Department. See p.158.
        assert_eq!(backup_size(5), BackupSize::Squad);
        // Rank 6: same tier. See p.158.
        assert_eq!(backup_size(6), BackupSize::Squad);
        // Rank 7: same tier. See p.158.
        assert_eq!(backup_size(7), BackupSize::Squad);

        // Rank 8: Recovery Zone Marshal. See p.159.
        assert_eq!(backup_size(8), BackupSize::Platoon);

        // Rank 9: C-SWAT. See p.159.
        assert_eq!(backup_size(9), BackupSize::Strikeforce);
        // Rank 10: National Law Enforcement / Interpol. See p.159.
        assert_eq!(backup_size(10), BackupSize::Strikeforce);
    }

    // ── backup_arrival_minutes ────────────────────────────────────────────────

    #[test]
    fn test_backup_arrival_time() {
        // Per pp.158–159: arrival time decreases as rank increases.
        // RAW uses d6 Rounds; this function returns a deterministic planning
        // estimate in minutes. See module-level deviation note.

        // Rank 0: no backup — 0 minutes (ability absent). See p.158.
        assert_eq!(backup_arrival_minutes(0), 0);

        // Rank 1: Corporate Security on foot — 10 minutes. See p.158.
        assert_eq!(backup_arrival_minutes(1), 10);
        // Rank 2: same tier. See p.158.
        assert_eq!(backup_arrival_minutes(2), 10);

        // Rank 3: Local Beat Cops in Compact Groundcars — 8 minutes. See p.158.
        assert_eq!(backup_arrival_minutes(3), 8);
        // Rank 4: same tier. See p.158.
        assert_eq!(backup_arrival_minutes(4), 8);

        // Rank 5: Sheriff's Dept in High Performance Groundcar — 6 minutes. See p.158.
        assert_eq!(backup_arrival_minutes(5), 6);
        // Rank 6: same tier. See p.158.
        assert_eq!(backup_arrival_minutes(6), 6);
        // Rank 7: same tier. See p.158.
        assert_eq!(backup_arrival_minutes(7), 6);

        // Rank 8: Recovery Zone Marshal on Superbike — 4 minutes. See p.159.
        assert_eq!(backup_arrival_minutes(8), 4);

        // Rank 9: C-SWAT via AV-4 (airborne) — 2 minutes. See p.159.
        assert_eq!(backup_arrival_minutes(9), 2);
        // Rank 10: National Law Enforcement via AV-4 — 2 minutes. See p.159.
        assert_eq!(backup_arrival_minutes(10), 2);
    }
}
