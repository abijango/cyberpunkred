//! Streetrat character creation — roll-on-template method.
//!
//! The Streetrat method (Method #1) lets a player roll 1d10 and copy the
//! adjacent stats from the role's template table. Skills, gear, and cyberware
//! are all predetermined by the role.
//!
//! # Pipeline
//! 1. [`streetrat_stats`] — roll 1d10, look up the role's STAT template row.
//! 2. [`create_streetrat`] — roll stats, derive HP/Humanity, assign starting
//!    skills, gear, and cyberware, build a complete [`Character`].
//!
//! See pp.73–78 (STAT templates), pp.86–87 (starting skills), p.98
//! (starting weapons and armor).

use crate::catalog::lifepath::Lifepath;
use crate::catalog::skills::{LanguageKind, LocalArea, SkillId};
use crate::character::{
    data::{
        AmmoKind, ArmorKind, ArmorPiece, InstalledCyberware, Inventory, ItemKind, ItemStack, Role,
        SkillSet, StatBlock, WornArmor, Wounds,
    },
    Character,
};
use crate::dice::d10;
use crate::effects::EffectStack;
use crate::error::RulesError;
use crate::rng::Rng;
use crate::types::{CharacterId, Eurobucks};
use std::collections::HashMap;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// STAT template tables (pp.73–76)
// ---------------------------------------------------------------------------

/// One row from a role's Streetrat STAT template. See pp.73–76.
///
/// Fields match the rulebook column order: INT, REF, DEX, TECH, COOL,
/// WILL, LUCK, MOVE, BODY, EMP.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
struct TemplateRow {
    int: u8,
    r#ref: u8,
    dex: u8,
    tech: u8,
    cool: u8,
    will: u8,
    luck: u8,
    r#move: u8,
    body: u8,
    emp: u8,
}

impl TemplateRow {
    /// Convert this row into the canonical [`StatBlock`].
    fn into_stat_block(self) -> StatBlock {
        StatBlock {
            int: self.int,
            r#ref: self.r#ref,
            dex: self.dex,
            tech: self.tech,
            cool: self.cool,
            will: self.will,
            luck: self.luck,
            r#move: self.r#move,
            body: self.body,
            emp: self.emp,
        }
    }
}

/// Return the 10-row STAT template for `role`. See pp.73–76.
///
/// The returned array is indexed 0..9; row index 0 corresponds to die
/// result 1, row index 9 corresponds to die result 10.
///
/// Rockerboy table: p.74. Solo: p.74. Netrunner: p.74.
/// Tech: p.75. Medtech: p.75. Media: p.75.
/// Lawman: p.76. Exec: p.76. Fixer: p.76. Nomad: p.77.
fn stat_template(role: Role) -> [TemplateRow; 10] {
    match role {
        // Rockerboy — p.74
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   7   6   6    5    6    8    7    7    3   8
        //    2 |   3   7   7    7    7    6    7    7    5   8
        //    3 |   4   5   7    7    6    6    7    7    5   8
        //    4 |   4   5   7    7    6    8    7    6    3   8
        //    5 |   3   7   7    7    6    8    6    5    4   7
        //    6 |   5   6   7    5    7    8    5    7    3   7
        //    7 |   5   6   6    7    7    8    7    6    3   6
        //    8 |   5   7   7    5    6    6    6    6    4   8
        //    9 |   3   5   5    6    7    8    7    5    5   7
        //   10 |   4   5   6    5    8    8    7    6    4   7
        Role::Rockerboy => [
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 7,
                r#move: 7,
                body: 3,
                emp: 8,
            },
            TemplateRow {
                int: 3,
                r#ref: 7,
                dex: 7,
                tech: 7,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 4,
                r#ref: 5,
                dex: 7,
                tech: 7,
                cool: 6,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 4,
                r#ref: 5,
                dex: 7,
                tech: 7,
                cool: 6,
                will: 8,
                luck: 7,
                r#move: 6,
                body: 3,
                emp: 8,
            },
            TemplateRow {
                int: 3,
                r#ref: 7,
                dex: 7,
                tech: 7,
                cool: 6,
                will: 8,
                luck: 6,
                r#move: 5,
                body: 4,
                emp: 7,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 7,
                will: 8,
                luck: 5,
                r#move: 7,
                body: 3,
                emp: 7,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 6,
                tech: 7,
                cool: 7,
                will: 8,
                luck: 7,
                r#move: 6,
                body: 3,
                emp: 6,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 4,
                emp: 8,
            },
            TemplateRow {
                int: 3,
                r#ref: 5,
                dex: 5,
                tech: 6,
                cool: 7,
                will: 8,
                luck: 7,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 4,
                r#ref: 5,
                dex: 6,
                tech: 5,
                cool: 8,
                will: 8,
                luck: 7,
                r#move: 6,
                body: 4,
                emp: 7,
            },
        ],

        // Solo — p.74
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   6   7   7    3    8    6    5    5    6   5
        //    2 |   7   8   6    3    6    6    7    5    6   6
        //    3 |   5   8   7    4    7    7    6    7    8   5
        //    4 |   5   8   6    4    6    7    6    5    7   6
        //    5 |   6   6   7    5    7    6    7    6    8   4
        //    6 |   7   7   6    5    7    6    6    7    7   5
        //    7 |   7   7   6    5    6    7    7    6    6   6
        //    8 |   7   8   7    5    6    6    5    6    8   4
        //    9 |   7   7   6    4    6    6    6    5    6   5
        //   10 |   6   6   8    5    6    6    5    6    6   5
        Role::Solo => [
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 7,
                tech: 3,
                cool: 8,
                will: 6,
                luck: 5,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            TemplateRow {
                int: 7,
                r#ref: 8,
                dex: 6,
                tech: 3,
                cool: 6,
                will: 6,
                luck: 7,
                r#move: 5,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 7,
                tech: 4,
                cool: 7,
                will: 7,
                luck: 6,
                r#move: 7,
                body: 8,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 6,
                tech: 4,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 6,
                body: 8,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 7,
                will: 6,
                luck: 6,
                r#move: 7,
                body: 7,
                emp: 5,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 7,
                r#move: 6,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 7,
                r#ref: 8,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 6,
                luck: 5,
                r#move: 6,
                body: 8,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 6,
                tech: 4,
                cool: 6,
                will: 6,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 8,
                tech: 5,
                cool: 6,
                will: 6,
                luck: 5,
                r#move: 6,
                body: 6,
                emp: 5,
            },
        ],

        // Netrunner — p.74
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   5   8   7    7    7    4    8    7    7   4
        //    2 |   5   6   7    5    8    3    8    7    5   5
        //    3 |   5   6   8    6    6    4    7    6    7   4
        //    4 |   5   7   7    7    7    5    8    6    5   5
        //    5 |   5   8   8    5    7    3    7    5    5   6
        //    6 |   6   6   6    7    8    4    7    7    6   6
        //    7 |   6   6   6    7    6    5    7    7    7   6
        //    8 |   5   7   8    6    8    4    8    5    7   4
        //    9 |   7   6   7    7    6    3    6    5    6   5
        //   10 |   7   8   6    6    6    4    7    7    5   6
        Role::Netrunner => [
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 7,
                tech: 7,
                cool: 7,
                will: 4,
                luck: 8,
                r#move: 7,
                body: 7,
                emp: 4,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 8,
                will: 3,
                luck: 8,
                r#move: 7,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 8,
                tech: 6,
                cool: 6,
                will: 4,
                luck: 7,
                r#move: 6,
                body: 7,
                emp: 4,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 7,
                tech: 7,
                cool: 7,
                will: 5,
                luck: 8,
                r#move: 6,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 8,
                tech: 5,
                cool: 7,
                will: 3,
                luck: 7,
                r#move: 5,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 7,
                cool: 8,
                will: 4,
                luck: 7,
                r#move: 7,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 7,
                cool: 6,
                will: 5,
                luck: 7,
                r#move: 7,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 8,
                tech: 6,
                cool: 8,
                will: 4,
                luck: 8,
                r#move: 5,
                body: 7,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 7,
                tech: 7,
                cool: 6,
                will: 3,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            TemplateRow {
                int: 7,
                r#ref: 8,
                dex: 6,
                tech: 6,
                cool: 6,
                will: 4,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 6,
            },
        ],

        // Tech — p.75
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   6   7   7    8    4    4    5    5    7   6
        //    2 |   7   6   6    7    5    3    7    7    5   5
        //    3 |   8   6   5    7    5    4    7    7    5   7
        //    4 |   7   8   7    8    4    4    6    5    6   7
        //    5 |   6   6   7    6    4    3    7    7    6   6
        //    6 |   8   7   5    6    3    3    7    6    6   7
        //    7 |   8   6   7    8    4    4    7    6    7   6
        //    8 |   8   8   7    8    5    4    6    5    6   6
        //    9 |   6   6   7    8    3    3    5    7    7   7
        //   10 |   8   8   5    6    4    4    6    5    6   6
        Role::Tech => [
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 7,
                tech: 8,
                cool: 4,
                will: 4,
                luck: 5,
                r#move: 5,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 6,
                tech: 7,
                cool: 5,
                will: 3,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 5,
                tech: 7,
                cool: 5,
                will: 4,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 7,
                r#ref: 8,
                dex: 7,
                tech: 8,
                cool: 4,
                will: 4,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 6,
                cool: 4,
                will: 3,
                luck: 7,
                r#move: 7,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 5,
                tech: 6,
                cool: 3,
                will: 3,
                luck: 7,
                r#move: 6,
                body: 6,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 7,
                tech: 8,
                cool: 4,
                will: 4,
                luck: 7,
                r#move: 6,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 8,
                r#ref: 8,
                dex: 7,
                tech: 8,
                cool: 5,
                will: 4,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 8,
                cool: 3,
                will: 3,
                luck: 5,
                r#move: 7,
                body: 7,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 8,
                dex: 5,
                tech: 6,
                cool: 4,
                will: 4,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 6,
            },
        ],

        // Medtech — p.75
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   7   5   6    7    5    3    8    5    5   7
        //    2 |   6   7   7    7    4    4    6    7    7   7
        //    3 |   6   5   5    8    5    3    8    5    7   8
        //    4 |   8   7   6    8    3    5    6    6    5   7
        //    5 |   6   7   5    7    5    5    8    7    6   8
        //    6 |   8   5   5    8    5    5    6    6    5   6
        //    7 |   8   6   5    8    5    4    8    5    7   7
        //    8 |   6   5   7    7    3    5    8    5    5   8
        //    9 |   6   6   7    7    5    4    6    6    5   6
        //   10 |   8   7   6    6    3    4    8    7    6   7
        Role::Medtech => [
            TemplateRow {
                int: 7,
                r#ref: 5,
                dex: 6,
                tech: 7,
                cool: 5,
                will: 3,
                luck: 8,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 7,
                tech: 7,
                cool: 4,
                will: 4,
                luck: 6,
                r#move: 7,
                body: 7,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 5,
                dex: 5,
                tech: 8,
                cool: 5,
                will: 3,
                luck: 8,
                r#move: 5,
                body: 7,
                emp: 8,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 6,
                tech: 8,
                cool: 3,
                will: 5,
                luck: 6,
                r#move: 6,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 5,
                tech: 7,
                cool: 5,
                will: 5,
                luck: 8,
                r#move: 7,
                body: 6,
                emp: 8,
            },
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 5,
                tech: 8,
                cool: 5,
                will: 5,
                luck: 6,
                r#move: 6,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 5,
                tech: 8,
                cool: 5,
                will: 4,
                luck: 8,
                r#move: 5,
                body: 7,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 5,
                dex: 7,
                tech: 7,
                cool: 3,
                will: 5,
                luck: 8,
                r#move: 5,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 7,
                cool: 5,
                will: 4,
                luck: 6,
                r#move: 6,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 6,
                tech: 6,
                cool: 3,
                will: 4,
                luck: 8,
                r#move: 7,
                body: 6,
                emp: 7,
            },
        ],

        // Media — p.75
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   6   6   5    5    8    7    5    7    5   7
        //    2 |   8   7   7    3    6    6    6    5    6   8
        //    3 |   6   7   7    5    6    8    5    5    5   7
        //    4 |   6   5   7    5    6    7    5    5    6   6
        //    5 |   6   6   7    4    8    7    6    7    5   8
        //    6 |   7   5   5    4    8    7    6    7    5   8
        //    7 |   8   5   6    3    7    6    6    5    6   7
        //    8 |   6   5   6    5    6    8    6    6    7   8
        //    9 |   7   7   5    4    6    7    6    5    6   7
        //   10 |   7   6   6    3    7    6    7    6    7   6
        Role::Media => [
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 5,
                tech: 5,
                cool: 8,
                will: 7,
                luck: 5,
                r#move: 7,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 7,
                tech: 3,
                cool: 6,
                will: 6,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 8,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 5,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 5,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 5,
                r#move: 5,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 4,
                cool: 8,
                will: 7,
                luck: 6,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 7,
                r#ref: 5,
                dex: 5,
                tech: 4,
                cool: 8,
                will: 7,
                luck: 6,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 6,
                tech: 3,
                cool: 7,
                will: 6,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 5,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 6,
                r#move: 6,
                body: 7,
                emp: 8,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 5,
                tech: 4,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 7,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 6,
                tech: 3,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 6,
                body: 7,
                emp: 6,
            },
        ],

        // Lawman — p.76
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   5   6   7    5    7    8    5    6    5   6
        //    2 |   6   6   6    5    6    8    5    7    5   5
        //    3 |   5   7   7    7    6    7    5    5    7   6
        //    4 |   6   6   7    6    6    8    5    7    7   6
        //    5 |   6   6   7    6    7    7    6    5    5   6
        //    6 |   7   6   5    5    7    8    5    6    7   4
        //    7 |   7   8   7    5    6    8    7    6    5   4
        //    8 |   5   6   6    5    6    8    5    7    6   4
        //    9 |   7   7   5    5    7    7    6    5    5   6
        //   10 |   6   6   5    6    8    7    5    7    6   6
        Role::Lawman => [
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 7,
                will: 8,
                luck: 5,
                r#move: 6,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 5,
                r#move: 7,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 7,
                tech: 7,
                cool: 6,
                will: 7,
                luck: 5,
                r#move: 5,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 6,
                cool: 6,
                will: 8,
                luck: 5,
                r#move: 7,
                body: 7,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 6,
                cool: 7,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 5,
                tech: 5,
                cool: 7,
                will: 8,
                luck: 5,
                r#move: 6,
                body: 7,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 8,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 7,
                r#move: 6,
                body: 5,
                emp: 4,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 8,
                luck: 5,
                r#move: 7,
                body: 6,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 5,
                tech: 5,
                cool: 7,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 5,
                tech: 6,
                cool: 8,
                will: 7,
                luck: 5,
                r#move: 7,
                body: 6,
                emp: 6,
            },
        ],

        // Exec — p.76
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   8   5   5    3    8    6    6    5    5   7
        //    2 |   8   6   6    4    7    6    7    7    5   7
        //    3 |   8   7   6    3    8    6    7    6    4   5
        //    4 |   8   5   7    5    6    5    6    5    5   7
        //    5 |   7   7   6    5    8    5    7    7    5   6
        //    6 |   5   7   7    3    6    7    6    5    5   7
        //    7 |   6   6   7    5    8    7    6    7    4   6
        //    8 |   6   7   7    3    7    5    7    5    5   7
        //    9 |   7   6   7    5    7    5    7    6    5   5
        //   10 |   7   7   5    5    8    6    6    7    4   7
        Role::Exec => [
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 5,
                tech: 3,
                cool: 8,
                will: 6,
                luck: 6,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 6,
                tech: 4,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 6,
                tech: 3,
                cool: 8,
                will: 6,
                luck: 7,
                r#move: 6,
                body: 4,
                emp: 5,
            },
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 7,
                tech: 5,
                cool: 6,
                will: 5,
                luck: 6,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 8,
                will: 5,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 7,
                tech: 3,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 8,
                will: 7,
                luck: 6,
                r#move: 7,
                body: 4,
                emp: 6,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 7,
                tech: 3,
                cool: 7,
                will: 5,
                luck: 7,
                r#move: 5,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 7,
                tech: 5,
                cool: 7,
                will: 5,
                luck: 7,
                r#move: 6,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 5,
                tech: 5,
                cool: 8,
                will: 6,
                luck: 6,
                r#move: 7,
                body: 4,
                emp: 7,
            },
        ],

        // Fixer — p.76
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   8   5   7    4    6    5    8    5    5   8
        //    2 |   8   5   5    5    6    7    8    7    5   7
        //    3 |   6   6   6    4    5    6    8    6    3   8
        //    4 |   7   7   5    5    7    6    7    7    5   8
        //    5 |   8   6   6    3    6    5    8    7    5   6
        //    6 |   8   7   5    5    6    7    7    7    5   3
        //    7 |   8   6   6    5    6    5    6    7    5   8
        //    8 |   6   6   7    4    7    6    7    7    4   7
        //    9 |   8   7   7    5    5    5    7    6    5   7
        //   10 |   6   5   6    5    5    6    8    6    4   7
        Role::Fixer => [
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 7,
                tech: 4,
                cool: 6,
                will: 5,
                luck: 8,
                r#move: 5,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 8,
                r#ref: 5,
                dex: 5,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 8,
                r#move: 7,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 4,
                cool: 5,
                will: 6,
                luck: 8,
                r#move: 6,
                body: 3,
                emp: 8,
            },
            TemplateRow {
                int: 7,
                r#ref: 7,
                dex: 5,
                tech: 5,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 6,
                tech: 3,
                cool: 6,
                will: 5,
                luck: 8,
                r#move: 7,
                body: 5,
                emp: 6,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 5,
                tech: 5,
                cool: 6,
                will: 7,
                luck: 7,
                r#move: 7,
                body: 5,
                emp: 3,
            },
            TemplateRow {
                int: 8,
                r#ref: 6,
                dex: 6,
                tech: 5,
                cool: 6,
                will: 5,
                luck: 6,
                r#move: 7,
                body: 5,
                emp: 8,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 7,
                tech: 4,
                cool: 7,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 4,
                emp: 7,
            },
            TemplateRow {
                int: 8,
                r#ref: 7,
                dex: 7,
                tech: 5,
                cool: 5,
                will: 5,
                luck: 7,
                r#move: 6,
                body: 5,
                emp: 7,
            },
            TemplateRow {
                int: 6,
                r#ref: 5,
                dex: 6,
                tech: 5,
                cool: 5,
                will: 6,
                luck: 8,
                r#move: 6,
                body: 4,
                emp: 7,
            },
        ],

        // Nomad — p.77
        // Roll | INT REF DEX TECH COOL WILL LUCK MOVE BODY EMP
        //    1 |   6   6   8    3    6    7    6    6    6   4
        //    2 |   5   7   6    5    8    8    8    7    5   4
        //    3 |   5   8   6    3    8    7    6    5    6   5
        //    4 |   5   8   7    4    8    6    7    7    7   5
        //    5 |   6   6   6    3    6    7    6    7    7   4
        //    6 |   7   6   8    4    6    7    6    5    6   5
        //    7 |   6   7   8    4    6    6    7    5    7   5
        //    8 |   5   7   8    3    8    6    7    5    5   5
        //    9 |   6   7   6    4    8    6    6    6    6   6
        //   10 |   5   6   7    4    7    8    7    7    7   4
        Role::Nomad => [
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 8,
                tech: 3,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 6,
                body: 6,
                emp: 4,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 6,
                tech: 5,
                cool: 8,
                will: 8,
                luck: 8,
                r#move: 7,
                body: 5,
                emp: 4,
            },
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 6,
                tech: 3,
                cool: 8,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 8,
                dex: 7,
                tech: 4,
                cool: 8,
                will: 6,
                luck: 7,
                r#move: 7,
                body: 7,
                emp: 5,
            },
            TemplateRow {
                int: 6,
                r#ref: 6,
                dex: 6,
                tech: 3,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 7,
                body: 7,
                emp: 4,
            },
            TemplateRow {
                int: 7,
                r#ref: 6,
                dex: 8,
                tech: 4,
                cool: 6,
                will: 7,
                luck: 6,
                r#move: 5,
                body: 6,
                emp: 5,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 8,
                tech: 4,
                cool: 6,
                will: 6,
                luck: 7,
                r#move: 5,
                body: 7,
                emp: 5,
            },
            TemplateRow {
                int: 5,
                r#ref: 7,
                dex: 8,
                tech: 3,
                cool: 8,
                will: 6,
                luck: 7,
                r#move: 5,
                body: 5,
                emp: 5,
            },
            TemplateRow {
                int: 6,
                r#ref: 7,
                dex: 6,
                tech: 4,
                cool: 8,
                will: 6,
                luck: 6,
                r#move: 6,
                body: 6,
                emp: 6,
            },
            TemplateRow {
                int: 5,
                r#ref: 6,
                dex: 7,
                tech: 4,
                cool: 7,
                will: 8,
                luck: 7,
                r#move: 7,
                body: 7,
                emp: 4,
            },
        ],
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Roll 1d10 on the role's Streetrat STAT template and return the result.
///
/// The caller must supply a `rng` seeded from the game's deterministic seed.
/// To test a specific row (e.g. "roll 6 on the Solo table"), use
/// [`rand::SeedableRng::seed_from_u64`] and find a seed that produces 6.
///
/// See pp.73–76 for the full template tables.
pub fn streetrat_stats(role: Role, rng: &mut Rng) -> StatBlock {
    // Roll 1d10 → 1..=10; map to 0-based index.
    let roll = d10(rng);
    let table = stat_template(role);
    // Safety: d10 returns 1..=10; saturating sub gives 0..=9.
    let row = table[(roll - 1) as usize];
    row.into_stat_block()
}

/// Build a complete [`Character`] using the Streetrat (template) method.
///
/// Steps (pp.73–98):
/// 1. Roll stats via [`streetrat_stats`].
/// 2. Derive HP and Humanity from the stat block (p.79–80).
/// 3. Assign starting skills per the role's Streetrat skill table (p.86–87).
/// 4. Equip starting weapons, armor, and cyberware per the role's gear
///    list (p.98).
/// 5. Grant the 500 eb starting cash (p.98).
///
/// Returns `Err(RulesError)` only if an internal constraint is violated
/// (currently unused — reserved for future validation).
pub fn create_streetrat(role: Role, name: String, rng: &mut Rng) -> Result<Character, RulesError> {
    // 1. Roll stats.
    let stats = streetrat_stats(role, rng);

    // 2. Derive HP and Humanity.
    let max_hp = {
        let body = u16::from(stats.body);
        let will = u16::from(stats.will);
        10 + 5 * (body + will).div_ceil(2)
    };
    let seriously_wounded_threshold = max_hp.div_ceil(2);
    let death_save_base = stats.body;
    let starting_humanity = Character::calculate_starting_humanity(stats.emp);

    let wounds = Wounds {
        current_hp: max_hp as i16,
        max_hp,
        seriously_wounded_threshold,
        death_save_base,
        death_save_penalty: 0,
        current_state: crate::effects::WoundState::None,
    };

    // 3. Assign starting skills.
    let skills = create_streetrat_skills(role);

    // 4. Assign starting gear and cyberware.
    let (armor, inventory, cyberware) = create_streetrat_gear(role);

    // 5. Build the Character.
    // Derive a deterministic CharacterId from the RNG so the create pipeline
    // is fully reproducible from its seed. Two u64 words → 16 bytes → UUID.
    use rand::Rng as _;
    let id_hi: u64 = rng.random();
    let id_lo: u64 = rng.random();
    let mut id_bytes = [0u8; 16];
    id_bytes[..8].copy_from_slice(&id_hi.to_le_bytes());
    id_bytes[8..].copy_from_slice(&id_lo.to_le_bytes());
    let character = Character {
        id: CharacterId(Uuid::from_bytes(id_bytes)),
        name,
        handle: None,
        role,
        role_rank: 4,
        stats,
        skills,
        cyberware,
        armor,
        inventory,
        wounds,
        humanity: starting_humanity,
        luck_pool: stats.luck,
        // 500eb starting cash per p.98.
        money: Eurobucks(500),
        improvement_points: 0,
        lifepath: Lifepath::default(),
        effects: EffectStack::new(),
        complementary_bonuses: Vec::new(),
    };

    Ok(character)
}

// ---------------------------------------------------------------------------
// Starting skills (pp.86–87)
// ---------------------------------------------------------------------------

/// Build the Streetrat starting [`SkillSet`] for `role`. See pp.86–87.
///
/// Basic Skills (bolded on p.86) start at level 2. Role-specific skills
/// start at level 6 unless otherwise noted. All values are read directly
/// from the tables on pp.86–87.
///
/// Note: the rulebook also grants 4 ranks in a language based on Cultural
/// Origin (pp.45, 85). That language is determined by the Lifepath roll and
/// is therefore set to `Language(Streetslang)` at level 2 here (the
/// universal default) and can be updated by the Lifepath roller (WP-504)
/// once the cultural origin is known.
///
/// `pub(super)` so that sibling creation modules (e.g. `edgerunner`)
/// can reuse this skill package without duplicating the tables (§0.2
/// design simplification: Edgerunner uses Streetrat skill list).
pub(super) fn create_streetrat_skills(role: Role) -> SkillSet {
    let mut ranks: HashMap<SkillId, u8> = HashMap::new();

    // Helper to insert without overwriting a higher rank.
    let mut set = |skill: SkillId, lvl: u8| {
        ranks
            .entry(skill)
            .and_modify(|e| *e = (*e).max(lvl))
            .or_insert(lvl);
    };

    // Basic Skills — shared across all roles (p.85). Minimum 2 for every role.
    // See the bold rows on p.86.
    set(SkillId::Athletics, 2);
    set(SkillId::Brawling, 2);
    set(SkillId::Concentration, 2);
    set(SkillId::Conversation, 2);
    set(SkillId::Education, 2);
    set(SkillId::Evasion, 2);
    set(SkillId::FirstAid, 2);
    set(SkillId::HumanPerception, 2);
    set(SkillId::Language(LanguageKind::Streetslang), 2);
    set(
        SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
        2,
    );
    set(SkillId::Perception, 2);
    set(SkillId::Persuasion, 2);
    set(SkillId::Stealth, 2);

    // Role-specific skill overrides and additions.
    match role {
        // Rockerboy — p.86 (left column)
        // Athletics 2, Brawling 6, Concentration 2, Conversation 2,
        // Education 2, Evasion 6, First Aid 6, Human Perception 6,
        // Language(Streetslang) 2, Local Expert(Your Home) 4,
        // Perception 2, Persuasion 6, Stealth 2,
        // Composition 6, Handgun 6, Melee Weapon 6,
        // Personal Grooming 4, Play Instrument(choose 1) 6,
        // Streetwise 6, Wardrobe & Style 4
        Role::Rockerboy => {
            set(SkillId::Brawling, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::FirstAid, 6);
            set(SkillId::HumanPerception, 6);
            set(
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
                4,
            );
            set(SkillId::Persuasion, 6);
            set(SkillId::Composition, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::MeleeWeapon, 6);
            set(SkillId::PersonalGrooming, 4);
            // Default instrument: Guitar (common for a Rockerboy)
            set(
                SkillId::PlayInstrument(crate::catalog::skills::Instrument::Guitar),
                6,
            );
            set(SkillId::Streetwise, 6);
            set(SkillId::WardrobeStyle, 4);
        }

        // Solo — p.86 (second column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 2,
        // Education 2, Evasion 6, First Aid 6, Human Perception 2,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 6, Persuasion 2, Stealth 2,
        // Autofire 6, Handgun 6, Interrogation 6, Melee Weapon 6,
        // Resist Torture/Drugs 6, Shoulder Arms 6, Tactics 6
        Role::Solo => {
            set(SkillId::Evasion, 6);
            set(SkillId::FirstAid, 6);
            set(SkillId::Perception, 6);
            set(SkillId::Autofire, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::Interrogation, 6);
            set(SkillId::MeleeWeapon, 6);
            set(SkillId::ResistTortureDrugs, 6);
            set(SkillId::ShoulderArms, 6);
            set(SkillId::Tactics, 6);
        }

        // Netrunner — p.86 (third column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 2,
        // Education 6, Evasion 6, First Aid 2, Human Perception 2,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 2, Persuasion 2, Stealth 6,
        // Basic Tech 6, Conceal/Reveal Object 6, Cryptography 6,
        // Cybertech 6, Electronics/Security Tech 6, Handgun 6,
        // Library Search 6
        Role::Netrunner => {
            set(SkillId::Education, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::Stealth, 6);
            set(SkillId::BasicTech, 6);
            set(SkillId::ConcealRevealObject, 6);
            set(SkillId::Cryptography, 6);
            set(SkillId::Cybertech, 6);
            set(SkillId::ElectronicsSecurityTech, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::LibrarySearch, 6);
        }

        // Tech — p.86 (fourth column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 2,
        // Education 6, Evasion 6, First Aid 6, Human Perception 2,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 2, Persuasion 2, Stealth 2,
        // Basic Tech 6, Cybertech 6,
        // Electronics/Security Tech(x2) 6, Land Vehicle Tech 6,
        // Shoulder Arms 6, Science(choose 1) 6, Weaponstech 6
        Role::Tech => {
            set(SkillId::Education, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::FirstAid, 6);
            set(SkillId::BasicTech, 6);
            set(SkillId::Cybertech, 6);
            set(SkillId::ElectronicsSecurityTech, 6);
            set(SkillId::LandVehicleTech, 6);
            set(SkillId::ShoulderArms, 6);
            // Default science field: Electronics
            set(
                SkillId::Science(crate::catalog::skills::ScienceField::Physics),
                6,
            );
            set(SkillId::Weaponstech, 6);
        }

        // Medtech — p.86 (fifth column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 6,
        // Education 6, Evasion 6, First Aid 2, Human Perception 6,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 2, Persuasion 2, Stealth 2,
        // Basic Tech 6, Cybertech 4, Deduction 6, Paramedic 6,
        // Resist Torture/Drugs 4, Science(choose 1) 6, Shoulder Arms 6
        Role::Medtech => {
            set(SkillId::Conversation, 6);
            set(SkillId::Education, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::HumanPerception, 6);
            set(SkillId::BasicTech, 6);
            set(SkillId::Cybertech, 4);
            set(SkillId::Deduction, 6);
            set(SkillId::Paramedic, 6);
            set(SkillId::ResistTortureDrugs, 4);
            set(
                SkillId::Science(crate::catalog::skills::ScienceField::Biology),
                6,
            );
            set(SkillId::ShoulderArms, 6);
        }

        // Media — p.87 (left column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 6,
        // Education 2, Evasion 6, First Aid 2, Human Perception 6,
        // Language(Streetslang) 2, Local Expert(Your Home) 6,
        // Perception 6, Persuasion 6, Stealth 2,
        // Bribery 6, Composition 6, Deduction 6, Handgun 6,
        // Library Search 4, Lip Reading 4, Photography/Film 4
        Role::Media => {
            set(SkillId::Conversation, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::HumanPerception, 6);
            set(
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
                6,
            );
            set(SkillId::Perception, 6);
            set(SkillId::Persuasion, 6);
            set(SkillId::Bribery, 6);
            set(SkillId::Composition, 6);
            set(SkillId::Deduction, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::LibrarySearch, 4);
            set(SkillId::LipReading, 4);
            set(SkillId::PhotographyFilm, 4);
        }

        // Lawman — p.87 (second column)
        // Athletics 2, Brawling 6, Concentration 2, Conversation 6,
        // Education 2, Evasion 6, First Aid 2, Human Perception 2,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 2, Persuasion 2, Stealth 2,
        // Autofire 6, Criminology 6, Deduction 6, Handgun 6,
        // Interrogation 6, Shoulder Arms 6, Tracking 6
        Role::Lawman => {
            set(SkillId::Brawling, 6);
            set(SkillId::Conversation, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::Autofire, 6);
            set(SkillId::Criminology, 6);
            set(SkillId::Deduction, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::Interrogation, 6);
            set(SkillId::ShoulderArms, 6);
            set(SkillId::Tracking, 6);
        }

        // Exec — p.87 (third column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 6,
        // Education 6, Evasion 6, First Aid 2, Human Perception 6,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 2, Persuasion 6, Stealth 2,
        // Accounting 6, Bureaucracy 6, Business 6, Deduction 6,
        // Handgun 6, Lip Reading 6, Personal Grooming 4
        Role::Exec => {
            set(SkillId::Conversation, 6);
            set(SkillId::Education, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::HumanPerception, 6);
            set(SkillId::Persuasion, 6);
            set(SkillId::AccountingFinance, 6);
            set(SkillId::Bureaucracy, 6);
            set(SkillId::Business, 6);
            set(SkillId::Deduction, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::LipReading, 6);
            set(SkillId::PersonalGrooming, 4);
        }

        // Fixer — p.87 (fourth column)
        // Athletics 2, Brawling 2, Concentration 2, Conversation 6,
        // Education 2, Evasion 6, First Aid 2, Human Perception 6,
        // Language(Streetslang) 4, Local Expert(Your Home) 6,
        // Perception 2, Persuasion 4, Stealth 2,
        // Bribery 6, Business 6, Forgery 6, Handgun 6,
        // Pick Lock 4, Streetwise 6, Trading 6
        Role::Fixer => {
            set(SkillId::Conversation, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::HumanPerception, 6);
            set(SkillId::Language(LanguageKind::Streetslang), 4);
            set(
                SkillId::LocalExpert(LocalArea::Custom("Your Home".into())),
                6,
            );
            set(SkillId::Persuasion, 4);
            set(SkillId::Bribery, 6);
            set(SkillId::Business, 6);
            set(SkillId::Forgery, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::PickLock, 4);
            set(SkillId::Streetwise, 6);
            set(SkillId::Trading, 6);
        }

        // Nomad — p.87 (fifth column)
        // Athletics 2, Brawling 6, Concentration 2, Conversation 2,
        // Education 2, Evasion 6, First Aid 6, Human Perception 2,
        // Language(Streetslang) 2, Local Expert(Your Home) 2,
        // Perception 4, Persuasion 2, Stealth 6,
        // Animal Handling 6, Drive Land Vehicle 6, Handgun 6,
        // Melee Weapon 6, Tracking 6, Trading 6, Wilderness Survival 6
        Role::Nomad => {
            set(SkillId::Brawling, 6);
            set(SkillId::Evasion, 6);
            set(SkillId::FirstAid, 6);
            set(SkillId::Perception, 4);
            set(SkillId::Stealth, 6);
            set(SkillId::AnimalHandling, 6);
            set(SkillId::DriveLandVehicle, 6);
            set(SkillId::Handgun, 6);
            set(SkillId::MeleeWeapon, 6);
            set(SkillId::Tracking, 6);
            set(SkillId::Trading, 6);
            set(SkillId::WildernessSurvival, 6);
        }
    }

    SkillSet { ranks }
}

// ---------------------------------------------------------------------------
// Starting gear (p.98)
// ---------------------------------------------------------------------------

/// Build the starting armor, inventory, and cyberware for `role`. See p.98.
///
/// Where the book gives a choice ("Shotgun **or** Assault Rifle"), we pick
/// the first-listed option as the canonical default. The Edgerunner and
/// Complete Package creation paths (WP-502, WP-503) will expose the choice
/// to the player.
///
/// All characters receive:
/// - Light Armorjack Body (SP 11) and Light Armorjack Head (SP 11) — p.98.
/// - 500eb starting cash — handled in [`create_streetrat`].
///
/// Cyberware: the book lists a Cyberdeck for Netrunners only (p.98 notes).
/// No other role receives starting cyberware under the Streetrat option.
///
/// `pub(super)` so that sibling creation modules (e.g. `edgerunner`)
/// can reuse this gear package without duplicating the tables (§0.2
/// design simplification: Edgerunner uses Streetrat gear list).
pub(super) fn create_streetrat_gear(role: Role) -> (WornArmor, Inventory, Vec<InstalledCyberware>) {
    // Standard armor: Light Armorjack body + head. SP 11, no penalty.
    let armor = WornArmor {
        body: Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: 11,
            max_sp: 11,
        }),
        head: Some(ArmorPiece {
            kind: ArmorKind::LightArmorjack,
            current_sp: 11,
            max_sp: 11,
        }),
    };

    let mut items: Vec<ItemStack> = Vec::new();
    let cyberware: Vec<InstalledCyberware> = Vec::new();

    match role {
        // Rockerboy — p.98
        // Very Heavy Pistol
        // Basic VH Pistol Ammunition x50
        // Heavy Melee Weapon *or* Flashbang Grenade (pick: Heavy Melee)
        // Teargas Grenade x2
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Rockerboy => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "very_heavy_pistol".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::VHPistol, 50),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "heavy_melee_weapon".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("teargas_grenade".into()),
                quantity: 2,
            });
        }

        // Solo — p.98
        // Assault Rifle
        // Very Heavy Pistol
        // Heavy Melee Weapon *or* Bulletproof Shield (pick: Heavy Melee)
        // Basic VH Pistol Ammunition x30
        // Basic Rifle Ammunition x70
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Solo => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("assault_rifle".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "very_heavy_pistol".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "heavy_melee_weapon".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::VHPistol, 30),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::Rifle, 70),
                quantity: 1,
            });
        }

        // Netrunner — p.98
        // Very Heavy Pistol
        // Basic VH Pistol Ammunition x30
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        // (Cyberdeck is gear, not cyberware — stored as Misc per catalog)
        Role::Netrunner => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "very_heavy_pistol".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::VHPistol, 30),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("cyberdeck".into()),
                quantity: 1,
            });
        }

        // Tech — p.98
        // Shotgun *or* Assault Rifle (pick: Shotgun)
        // Basic Shotgun Shell Ammunition x100 *or* Basic Rifle Ammunition x100
        //   (matches weapon pick: Shotgun Shells x100)
        // Flashbang Grenade
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Tech => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("shotgun".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::Slug, 100),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("flashbang_grenade".into()),
                quantity: 1,
            });
        }

        // Medtech — p.98
        // Shotgun *or* Assault Rifle (pick: Shotgun)
        // Basic Shotgun Shell Ammunition x100 *or* Basic Rifle Ammunition x100
        //   (matches weapon pick: Shotgun Shells x100)
        // Incendiary Shotgun Shell Ammunition x10 *or* Incendiary Rifle Ammo x10
        // Smoke Grenade x2
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        // Bulletproof Shield
        Role::Medtech => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("shotgun".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::Slug, 100),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("incendiary_shotgun_shell".into()),
                quantity: 10,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("smoke_grenade".into()),
                quantity: 2,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("bulletproof_shield".into()),
                quantity: 1,
            });
        }

        // Media — p.98
        // Heavy Pistol *or* Very Heavy Pistol (pick: Heavy Pistol)
        // Basic H Pistol Ammunition x50 *or* Basic VH Pistol Ammunition x50
        //   (matches weapon pick: H Pistol Ammo x50)
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Media => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("heavy_pistol".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::HPistol, 50),
                quantity: 1,
            });
        }

        // Lawman — p.98
        // Assault Rifle *or* Shotgun (pick: Assault Rifle)
        // Heavy Pistol
        // Basic Rifle Ammunition x100 *or* Basic Shotgun Shell Ammunition x100
        //   *or* Basic Slug Ammunition x100 (match weapon: Rifle Ammo x100)
        // Basic H Pistol Ammunition x30
        // Bulletproof Shield *or* Smoke Grenade x2 (pick: Bulletproof Shield)
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Lawman => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("assault_rifle".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("heavy_pistol".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::Rifle, 100),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::HPistol, 30),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Misc("bulletproof_shield".into()),
                quantity: 1,
            });
        }

        // Exec — p.98
        // Very Heavy Pistol
        // Basic VH Pistol Ammunition x50
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Exec => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "very_heavy_pistol".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::VHPistol, 50),
                quantity: 1,
            });
        }

        // Fixer — p.98
        // Heavy Pistol *or* Very Heavy Pistol (pick: Heavy Pistol)
        // Heavy Pistol *or* Very Heavy Pistol (second copy, book lists it twice)
        // Light Melee Weapon
        // Basic H Pistol Ammunition x100 *or* Basic VH Pistol Ammunition x100
        //   (match: H Pistol Ammo x100)
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Fixer => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("heavy_pistol".into())),
                quantity: 2,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "light_melee_weapon".into(),
                )),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::HPistol, 100),
                quantity: 1,
            });
        }

        // Nomad — p.98
        // Heavy Pistol *or* Very Heavy Pistol (pick: Heavy Pistol)
        // Basic H Pistol Ammunition x100 *or* Basic VH Pistol Ammunition x100
        //   (match: H Pistol Ammo x100)
        // Heavy Melee Weapon *or* Heavy Pistol (pick: Heavy Melee Weapon)
        // Light Armorjack Body (SP11) + Head (SP11) [handled above]
        Role::Nomad => {
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId("heavy_pistol".into())),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Ammo(AmmoKind::HPistol, 100),
                quantity: 1,
            });
            items.push(ItemStack {
                kind: ItemKind::Weapon(crate::character::data::WeaponId(
                    "heavy_melee_weapon".into(),
                )),
                quantity: 1,
            });
        }
    }

    (armor, Inventory { items }, cyberware)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::skills::SkillId;
    use rand::SeedableRng;

    /// Find the first seed that makes `d10(rng)` return `target`. Used to
    /// force a specific template row in deterministic tests.
    fn seed_for_d10(target: u8) -> u64 {
        for seed in 0..1_000_000_u64 {
            let mut rng = Rng::seed_from_u64(seed);
            if d10(&mut rng) == target {
                return seed;
            }
        }
        panic!("no seed produces d10={target} within 1M tries");
    }

    /// Acceptance: forced d10=6 on Solo template → row 6 stats.
    ///
    /// Solo roll-6 row from p.74:
    ///   INT 7, REF 7, DEX 6, TECH 5, COOL 7, WILL 6, LUCK 6, MOVE 7, BODY 7, EMP 5
    #[test]
    fn test_streetrat_solo_stats_match_table() {
        let seed = seed_for_d10(6);
        let mut rng = Rng::seed_from_u64(seed);
        let stats = streetrat_stats(Role::Solo, &mut rng);
        assert_eq!(stats.int, 7, "INT");
        assert_eq!(stats.r#ref, 7, "REF");
        assert_eq!(stats.dex, 6, "DEX");
        assert_eq!(stats.tech, 5, "TECH");
        assert_eq!(stats.cool, 7, "COOL");
        assert_eq!(stats.will, 6, "WILL");
        assert_eq!(stats.luck, 6, "LUCK");
        assert_eq!(stats.r#move, 7, "MOVE");
        assert_eq!(stats.body, 7, "BODY");
        assert_eq!(stats.emp, 5, "EMP");
    }

    /// Acceptance: HP formula matches stats from the template.
    ///
    /// Uses Solo roll-6: BODY 7, WILL 6 → ceil((7+6)/2) = 7 → HP = 10 + 35 = 45.
    #[test]
    fn test_streetrat_starting_hp_correct() {
        let seed = seed_for_d10(6);
        let mut rng = Rng::seed_from_u64(seed);
        let mut rng2 = Rng::seed_from_u64(seed);

        let stats = streetrat_stats(Role::Solo, &mut rng);
        let body = u16::from(stats.body);
        let will = u16::from(stats.will);
        let expected_hp = 10 + 5 * (body + will).div_ceil(2);

        let character = create_streetrat(Role::Solo, "TestSolo".into(), &mut rng2)
            .expect("create_streetrat must succeed");

        assert_eq!(character.wounds.max_hp, expected_hp);
        assert_eq!(character.wounds.current_hp, expected_hp as i16);
    }

    /// Acceptance: starting Humanity == 10 × EMP.
    ///
    /// p.80: "your starting Humanity, before any Cyberware is added, is
    /// your EMP × 10."
    #[test]
    fn test_streetrat_starting_humanity_eq_10x_emp() {
        let seed = seed_for_d10(6);
        let mut rng = Rng::seed_from_u64(seed);
        let character = create_streetrat(Role::Solo, "TestSolo".into(), &mut rng)
            .expect("create_streetrat must succeed");

        let expected_humanity = i16::from(character.stats.emp) * 10;
        assert_eq!(character.humanity, expected_humanity);
    }

    /// Acceptance: Solo gets the Solo's starting skills at the correct ranks.
    ///
    /// Verified against the Solo column on pp.86–87:
    /// - Evasion 6, First Aid 6, Perception 6 (raised from Basic 2)
    /// - Autofire 6, Handgun 6, Interrogation 6, Melee Weapon 6,
    ///   Resist Torture/Drugs 6, Shoulder Arms 6, Tactics 6
    /// - Athletics 2, Brawling 2, Concentration 2, Conversation 2,
    ///   Education 2, Human Perception 2, Language(Streetslang) 2,
    ///   Local Expert(Your Home) 2, Persuasion 2, Stealth 2
    #[test]
    fn test_streetrat_role_skills_set() {
        let mut rng = Rng::seed_from_u64(0);
        let character = create_streetrat(Role::Solo, "TestSolo".into(), &mut rng)
            .expect("create_streetrat must succeed");

        let r = &character.skills.ranks;

        // Role-specific skill ranks.
        assert_eq!(r.get(&SkillId::Evasion).copied().unwrap_or(0), 6, "Evasion");
        assert_eq!(
            r.get(&SkillId::FirstAid).copied().unwrap_or(0),
            6,
            "First Aid"
        );
        assert_eq!(
            r.get(&SkillId::Perception).copied().unwrap_or(0),
            6,
            "Perception"
        );
        assert_eq!(
            r.get(&SkillId::Autofire).copied().unwrap_or(0),
            6,
            "Autofire"
        );
        assert_eq!(r.get(&SkillId::Handgun).copied().unwrap_or(0), 6, "Handgun");
        assert_eq!(
            r.get(&SkillId::Interrogation).copied().unwrap_or(0),
            6,
            "Interrogation"
        );
        assert_eq!(
            r.get(&SkillId::MeleeWeapon).copied().unwrap_or(0),
            6,
            "Melee Weapon"
        );
        assert_eq!(
            r.get(&SkillId::ResistTortureDrugs).copied().unwrap_or(0),
            6,
            "Resist Torture/Drugs"
        );
        assert_eq!(
            r.get(&SkillId::ShoulderArms).copied().unwrap_or(0),
            6,
            "Shoulder Arms"
        );
        assert_eq!(r.get(&SkillId::Tactics).copied().unwrap_or(0), 6, "Tactics");

        // Basic Skills at their Streetrat values.
        assert_eq!(
            r.get(&SkillId::Athletics).copied().unwrap_or(0),
            2,
            "Athletics"
        );
        assert_eq!(
            r.get(&SkillId::Brawling).copied().unwrap_or(0),
            2,
            "Brawling"
        );
        assert_eq!(
            r.get(&SkillId::Concentration).copied().unwrap_or(0),
            2,
            "Concentration"
        );
        assert_eq!(
            r.get(&SkillId::Education).copied().unwrap_or(0),
            2,
            "Education"
        );
        assert_eq!(
            r.get(&SkillId::HumanPerception).copied().unwrap_or(0),
            2,
            "Human Perception"
        );
        assert_eq!(r.get(&SkillId::Stealth).copied().unwrap_or(0), 2, "Stealth");

        // Solo should NOT have non-solo skills.
        assert_eq!(
            r.get(&SkillId::Composition).copied().unwrap_or(0),
            0,
            "Composition not in Solo"
        );
    }

    /// Sanity: all ten roles produce a valid StatBlock without panicking.
    #[test]
    fn test_all_roles_produce_valid_stat_blocks() {
        let all_roles = [
            Role::Rockerboy,
            Role::Solo,
            Role::Netrunner,
            Role::Tech,
            Role::Medtech,
            Role::Media,
            Role::Lawman,
            Role::Exec,
            Role::Fixer,
            Role::Nomad,
        ];
        for role in all_roles {
            let mut rng = Rng::seed_from_u64(42);
            let stats = streetrat_stats(role, &mut rng);
            assert!((1..=10).contains(&stats.int), "{role:?} INT out of range");
            assert!((1..=10).contains(&stats.r#ref), "{role:?} REF out of range");
            assert!((1..=10).contains(&stats.dex), "{role:?} DEX out of range");
            assert!((1..=10).contains(&stats.body), "{role:?} BODY out of range");
            assert!((1..=10).contains(&stats.emp), "{role:?} EMP out of range");
        }
    }

    /// Sanity: create_streetrat for every role succeeds and HP > 0.
    #[test]
    fn test_all_roles_create_streetrat_succeeds() {
        let all_roles = [
            Role::Rockerboy,
            Role::Solo,
            Role::Netrunner,
            Role::Tech,
            Role::Medtech,
            Role::Media,
            Role::Lawman,
            Role::Exec,
            Role::Fixer,
            Role::Nomad,
        ];
        for role in all_roles {
            let mut rng = Rng::seed_from_u64(7);
            let c = create_streetrat(role, format!("{role:?}"), &mut rng)
                .expect("create_streetrat must succeed");
            assert!(c.wounds.max_hp > 0, "{role:?} must have positive HP");
            assert!(
                c.humanity > 0,
                "{role:?} must have positive starting Humanity"
            );
            assert_eq!(c.money, Eurobucks(500), "{role:?} must start with 500eb");
        }
    }

    /// Sanity: Netrunner gets a Cyberdeck in their inventory.
    #[test]
    fn test_netrunner_gets_cyberdeck() {
        let mut rng = Rng::seed_from_u64(0);
        let c = create_streetrat(Role::Netrunner, "Runner".into(), &mut rng)
            .expect("create_streetrat must succeed");
        let has_cyberdeck = c
            .inventory
            .items
            .iter()
            .any(|stack| matches!(&stack.kind, ItemKind::Misc(s) if s == "cyberdeck"));
        assert!(has_cyberdeck, "Netrunner must start with a Cyberdeck");
    }

    /// Sanity: all roles start with Light Armorjack on body and head.
    #[test]
    fn test_all_roles_get_light_armorjack() {
        let all_roles = [
            Role::Rockerboy,
            Role::Solo,
            Role::Netrunner,
            Role::Tech,
            Role::Medtech,
            Role::Media,
            Role::Lawman,
            Role::Exec,
            Role::Fixer,
            Role::Nomad,
        ];
        for role in all_roles {
            let mut rng = Rng::seed_from_u64(99);
            let c = create_streetrat(role, format!("{role:?}"), &mut rng)
                .expect("create_streetrat must succeed");
            assert_eq!(
                c.armor.body.as_ref().map(|a| a.kind),
                Some(ArmorKind::LightArmorjack),
                "{role:?} body armor"
            );
            assert_eq!(
                c.armor.head.as_ref().map(|a| a.kind),
                Some(ArmorKind::LightArmorjack),
                "{role:?} head armor"
            );
        }
    }
}
