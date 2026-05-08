//! Character data — the central record for a player or NPC.
//!
//! See `IMPLEMENTATION_PLAN.md` §2.6: `Character` is a *pure data container*
//! at this layer. No methods compute current values — those derived queries
//! (current DEX after effects, current MOVE after wound penalties, etc.)
//! land in a later WP. Holding the line on "data only" here keeps the
//! effect-stack invariant clean: there is exactly one place transient
//! changes can affect a query, and it is `EffectStack`.
//!
//! Rulebook references: pp.71–80 (stats and derived stats), p.186 (wound
//! states), pp.81–90 (skills).

pub mod creation;
pub mod cyberware;
pub mod data;
pub mod derive;
pub mod hp;
pub mod lifepath;
pub mod luck;
pub mod progression;
pub mod wounds;

pub use data::{
    AmmoKind, ArmorKind, ArmorPiece, InstalledCyberware, Inventory, ItemKind, ItemStack, Lifepath,
    Role, SkillSet, StatBlock, WeaponId, WornArmor, Wounds,
};

use crate::checks::ComplementaryBonus;
use crate::effects::EffectStack;
use crate::types::{CharacterId, Eurobucks};
use serde::{Deserialize, Serialize};

/// A complete character record — the in-memory shape of a save.
///
/// Every transient adjustment (drug effects, wound penalties, drug-induced
/// stat shifts, role abilities) lives in [`Self::effects`], not in the
/// "base" fields below. Base fields (`stats`, `skills`, `cyberware`) only
/// change through explicit progression or installation actions.
///
/// `Eq` is intentionally not derived: it is rarely useful for a struct this
/// large, and keeping it off avoids accidentally making equality part of
/// the API contract for a type whose internals will keep growing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Character {
    /// Stable identifier. See `crate::types::CharacterId`.
    pub id: CharacterId,
    /// Real-world name on file. Distinct from `handle`.
    pub name: String,
    /// Street name / alias. Optional — not every NPC has one.
    pub handle: Option<String>,
    /// Role at character creation. See p.71 and the role chapter from p.36.
    pub role: Role,
    /// Role rank, 1..=10. Drives Role Ability scaling. See p.71.
    pub role_rank: u8,
    /// Base stats — immutable post-creation. Apply [`EffectStack`] modifiers
    /// at query time to obtain current values. See pp.72–73.
    pub stats: StatBlock,
    /// Base skill ranks. Apply effect modifiers at query time. See pp.81–90.
    pub skills: SkillSet,
    /// All installed cyberware. Each piece may emit a permanent effect into
    /// [`Self::effects`]. See p.94 (cyberware overview).
    pub cyberware: Vec<InstalledCyberware>,
    /// Currently worn armor. Head and body locations only — see p.184.
    pub armor: WornArmor,
    /// Owned items not currently worn or wielded.
    pub inventory: Inventory,
    /// Hit points and wound bookkeeping. See pp.79–80, p.186.
    pub wounds: Wounds,
    /// Humanity — signed because cyberpsychosis is HUM &lt; 0 territory and
    /// is a gameplay condition, not a struct invariant. See pp.226–230.
    pub humanity: i16,
    /// Remaining LUCK pool for the day. See p.130.
    pub luck_pool: u8,
    /// Eurobuck balance.
    pub money: Eurobucks,
    /// Improvement Points accumulated but not yet spent. See p.410.
    pub improvement_points: u32,
    /// Lifepath record. See pp.230–254 (Streetrat / Complete Lifepath).
    pub lifepath: Lifepath,
    /// All transient and permanent effects on this character. See
    /// `IMPLEMENTATION_PLAN.md` §2.6.
    pub effects: EffectStack,
    /// Pending one-shot Complementary Skill bonuses (rulebook p.130).
    /// Each entry is consumed by the next [`crate::checks::SkillCheck`]
    /// for its `target_skill`. Bonuses do not stack (p.130) — see
    /// [`Self::add_complementary_bonus`]. `#[serde(default)]` keeps
    /// pre-WP-102 saves loadable with an empty vec.
    #[serde(default)]
    pub complementary_bonuses: Vec<ComplementaryBonus>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::data::{ItemKind, ItemStack};
    use crate::effects::{
        ActiveEffect, CyberwareId, EffectDuration, EffectSource, SkillId, WoundState,
    };
    use crate::types::EffectInstanceId;
    use std::collections::HashMap;
    use uuid::Uuid;

    fn fully_populated() -> Character {
        let mut ranks = HashMap::new();
        ranks.insert(SkillId::Handgun, 4);
        ranks.insert(SkillId::Brawling, 2);

        let mut stack = EffectStack::new();
        stack.add(ActiveEffect {
            id: EffectInstanceId(Uuid::from_u128(0xE0)),
            source: EffectSource::Cyberware(CyberwareId("neural_link".into())),
            modifiers: vec![],
            duration: EffectDuration::Permanent,
        });

        Character {
            id: CharacterId(Uuid::from_u128(0xC0)),
            name: "V".to_string(),
            handle: Some("V".to_string()),
            role: Role::Solo,
            role_rank: 4,
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
            skills: SkillSet { ranks },
            cyberware: vec![InstalledCyberware {
                id: CyberwareId("neural_link".into()),
                options: vec![CyberwareId("interface_plugs".into())],
            }],
            armor: WornArmor {
                head: Some(ArmorPiece {
                    kind: ArmorKind::LightArmorjack,
                    current_sp: 11,
                    max_sp: 11,
                }),
                body: Some(ArmorPiece {
                    kind: ArmorKind::LightArmorjack,
                    current_sp: 10,
                    max_sp: 11,
                }),
            },
            inventory: Inventory {
                items: vec![
                    ItemStack {
                        kind: ItemKind::Weapon(WeaponId("medium_pistol".into())),
                        quantity: 1,
                    },
                    ItemStack {
                        kind: ItemKind::Ammo(AmmoKind::MPistol, 30),
                        quantity: 1,
                    },
                    ItemStack {
                        kind: ItemKind::Misc("medtech_bag".into()),
                        quantity: 1,
                    },
                ],
            },
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
            money: Eurobucks(1_500),
            improvement_points: 0,
            lifepath: Lifepath {
                cultural_region: "Sub-Saharan African".into(),
                ..Lifepath::default()
            },
            effects: stack,
            complementary_bonuses: Vec::new(),
        }
    }

    #[test]
    fn test_character_serializes_round_trip() {
        let c = fully_populated();
        let serialized = ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default())
            .expect("RON serialize must succeed for a well-formed Character");
        let restored: Character =
            ron::de::from_str(&serialized).expect("RON round-trip must deserialize cleanly");
        assert_eq!(c, restored);

        // Sanity: spot-check the populated fields actually round-tripped.
        assert_eq!(restored.skills.ranks.len(), 2);
        assert_eq!(restored.cyberware.len(), 1);
        assert_eq!(restored.inventory.items.len(), 3);
        assert!(restored.armor.head.is_some());
        assert!(restored.armor.body.is_some());
    }

    #[test]
    fn test_default_wound_state() {
        // Per book p.186: the wound state table starts at "Less than Full HP".
        // A character at full HP has no wound effect — `WoundState::None`.
        let c = fully_populated();
        assert_eq!(c.wounds.current_hp as u16, c.wounds.max_hp);
        assert_eq!(c.wounds.current_state, WoundState::None);
    }

    #[test]
    fn test_humanity_below_zero_legal() {
        // Cyberpsychosis (HUM < 0) is gameplay logic, not a struct invariant.
        // The struct must accept a negative humanity and still round-trip.
        let mut c = fully_populated();
        c.humanity = -1;

        let serialized = ron::ser::to_string_pretty(&c, ron::ser::PrettyConfig::default())
            .expect("negative humanity must serialize");
        let restored: Character =
            ron::de::from_str(&serialized).expect("negative humanity must deserialize");
        assert_eq!(restored.humanity, -1);
        assert_eq!(c, restored);
    }
}
