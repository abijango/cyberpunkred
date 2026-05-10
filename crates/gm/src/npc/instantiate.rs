//! NPC instantiation — builds a live [`ActiveNpc`] from an [`NpcTemplate`].
//!
//! The entry point is [`instantiate_npc`]. It resolves the template's
//! [`NpcTemplateKind`] variant and returns a fully populated [`ActiveNpc`]
//! ready for use in a gig scene:
//!
//! - **Mook** — stat block from the [`MookStatline`] catalog keyed by
//!   [`MookArchetype`] (pp.412–413 Mook stat sheets), loadout items
//!   resolved from the weapon / armor / cyberware catalogs.
//! - **Lieutenant / Boss** — template's hand-authored [`Character`] cloned
//!   verbatim, fresh [`EntityId`] generated from the RNG.
//! - **Narrative** — minimal stub [`Character`] with 1 HP; the LLM
//!   voices the NPC; combat stats are irrelevant.
//!
//! ## EntityId generation — design choice
//!
//! `cpr_rules` intentionally does **not** link `uuid`'s `v4` (OS-entropy)
//! feature (see `crates/rules/Cargo.toml`), because the rules engine is
//! fully deterministic and no OS RNG must be touched. This module follows
//! the same contract: [`EntityId`]s are produced by drawing two `u64`s
//! from `rng` and assembling them into a 128-bit UUID via
//! `Uuid::from_u128`. This makes every instantiated NPC's identity
//! reproducible from the session seed + action log — important for the
//! replay tool.
//!
//! Rulebook references:
//! - **pp.412–413:** Bodyguard (Goon), Boosterganger, Road Ganger,
//!   Security Operative stat lines.
//! - **pp.414–416:** Security Officer, Reclaimer Chief, Outrider, Pyro,
//!   Cyberpsycho stat lines.
//! - **pp.418–419:** Encounter tables referencing these NPC archetypes.

#![forbid(unsafe_code)]

use crate::npc::entity::{ActiveNpc, Loadout, MookArchetype, NpcTemplate, NpcTemplateKind};
use crate::{EntityId, GmError, Rng};
use cpr_rules::catalog::armor::{Armor, ArmorId};
use cpr_rules::catalog::cyberware::Cyberware;
use cpr_rules::catalog::weapons::Weapon;
use cpr_rules::character::data::{
    ArmorPiece, InstalledCyberware, Inventory, ItemKind, ItemStack, Role, SkillSet, StatBlock,
    WornArmor, Wounds,
};
use cpr_rules::character::Character;
use cpr_rules::effects::EffectStack;
use cpr_rules::types::{CharacterId, Eurobucks};
use cpr_rules::Catalog;
use rand_core::RngCore;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// CatalogBundle
// ---------------------------------------------------------------------------

/// All catalogs needed to resolve a template into an [`ActiveNpc`].
///
/// Borrowed references so the bundle stays cheap to construct; the catalogs
/// themselves live for the duration of the gig session.
pub struct CatalogBundle<'a> {
    /// Weapon catalog — resolves [`Loadout::weapons`] slugs.
    pub weapons: &'a Catalog<Weapon>,
    /// Armor catalog — resolves [`Loadout::armor`] slugs.
    pub armor: &'a Catalog<Armor>,
    /// Cyberware catalog — resolves [`Loadout::cyberware`] slugs.
    pub cyberware: &'a Catalog<Cyberware>,
    /// Mook statline catalog — resolves [`MookArchetype`] keys.
    ///
    /// Keyed by `format!("{:?}", archetype)` (the `Debug` representation).
    /// See [`MookStatline`] and [`MookArchetype`].
    pub mooks: &'a Catalog<MookStatline>,
}

// ---------------------------------------------------------------------------
// MookStatline
// ---------------------------------------------------------------------------

/// Pre-computed stat line for one [`MookArchetype`].
///
/// Each entry records the printed stat block for a mook NPC type from the
/// "Mooks and Grunts" section of the rulebook (pp.412–413), together with
/// the archetype's default [`Loadout`].
///
/// GMs and content authors populate a `Catalog<MookStatline>` from RON (see
/// `content/` for the authored data). The catalog key is
/// `format!("{:?}", archetype)` — e.g. `"Goon"`, `"BoosterGanger"`.
///
/// Rulebook references: **pp.412–413** (stat sheets), **pp.418–419**
/// (encounter tables that reference these archetypes by name).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MookStatline {
    /// Which archetype this statline covers. See [`MookArchetype`].
    pub archetype: MookArchetype,
    /// Base stats for all instances of this archetype. See pp.72–73.
    pub stats: StatBlock,
    /// Base skill ranks (pre-combined STAT + Skill on the sheet). See pp.81–90.
    pub skills: SkillSet,
    /// Maximum (and starting) Hit Points. See p.79.
    ///
    /// Stored directly from the book's printed HP value rather than
    /// recomputing from the formula, because the book's stat sheets are the
    /// authoritative source and the formula-derived value might diverge from
    /// the printed entry (mooks have curated, not formulaic, stat blocks).
    pub hp: u16,
    /// Default loadout for this archetype. Weapon/armor/cyberware slugs are
    /// resolved against the weapon, armor, and cyberware catalogs by
    /// [`instantiate_npc`].
    pub default_loadout: Loadout,
}

// ---------------------------------------------------------------------------
// instantiate_npc
// ---------------------------------------------------------------------------

/// Instantiate a live [`ActiveNpc`] from an [`NpcTemplate`].
///
/// Resolves the template's kind, builds a [`Character`] with appropriate
/// stats and inventory, and returns the active instance.
///
/// # Errors
///
/// - [`GmError::MookArchetypeNotFound`] — the template's [`MookArchetype`]
///   has no entry in `catalog.mooks`.
/// - [`GmError::LoadoutItemNotFound`] — a loadout slug is missing from its
///   respective catalog (weapon, armor, or cyberware).
///
/// # EntityId generation
///
/// IDs are derived from `rng` (two `u64` draws assembled into a 128-bit
/// UUID) so that replay is possible from the session seed + action log.
/// See the module-level doc for the full rationale.
///
/// Rulebook references: **pp.412–413** (mook stat lines),
/// **pp.418–419** (encounter tables).
pub fn instantiate_npc(
    template: &NpcTemplate,
    catalog: &CatalogBundle<'_>,
    rng: &mut Rng,
) -> Result<ActiveNpc, GmError> {
    let entity_id = next_entity_id(rng);

    let character = match &template.kind {
        NpcTemplateKind::Mook { archetype, loadout } => {
            build_mook_character(*archetype, loadout, catalog, rng)?
        }
        NpcTemplateKind::Lieutenant { character } => character.clone(),
        NpcTemplateKind::Boss { character } => character.clone(),
        NpcTemplateKind::Narrative { .. } => minimal_narrative_character(),
    };

    Ok(ActiveNpc {
        template: template.id.clone(),
        entity_id,
        character,
        gig_disposition: template.initial_disposition,
    })
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Draw two `u64`s from `rng` and assemble a deterministic UUID.
///
/// `v4` (OS-entropy) is intentionally not used — see module-level doc.
fn next_entity_id(rng: &mut Rng) -> EntityId {
    let hi = rng.next_u64();
    let lo = rng.next_u64();
    let bits = (u128::from(hi) << 64) | u128::from(lo);
    EntityId(Uuid::from_u128(bits))
}

/// Build a [`Character`] for a Mook template.
///
/// Looks up the archetype's stat line, resolves all loadout items, and
/// returns a fully populated [`Character`].
fn build_mook_character(
    archetype: MookArchetype,
    loadout: &Loadout,
    catalog: &CatalogBundle<'_>,
    rng: &mut Rng,
) -> Result<Character, GmError> {
    // Resolve the archetype stat line from the mook catalog.
    let key = format!("{archetype:?}");
    let statline = catalog
        .mooks
        .get(&key)
        .ok_or(GmError::MookArchetypeNotFound(key))?;

    // Resolve loadout inventory items.
    let items = resolve_loadout_items(loadout, catalog)?;

    // Resolve worn armor from the loadout's armor slug.
    let worn_armor = resolve_worn_armor(loadout.armor.as_ref(), catalog)?;

    // Resolve cyberware installs from the loadout.
    let cyberware = resolve_cyberware(loadout, catalog)?;

    let max_hp = statline.hp;
    let sw_threshold = max_hp.div_ceil(2);

    // Death Save base = BODY (per p.79).
    let death_save_base = statline.stats.body;

    let char_id = next_char_id(rng);

    Ok(Character {
        id: char_id,
        name: format!("{archetype:?}"),
        handle: None,
        role: Role::Solo,
        role_rank: 1,
        stats: statline.stats,
        skills: statline.skills.clone(),
        cyberware,
        armor: worn_armor,
        inventory: Inventory { items },
        wounds: Wounds {
            current_hp: max_hp as i16,
            max_hp,
            seriously_wounded_threshold: sw_threshold,
            death_save_base,
            death_save_penalty: 0,
            current_state: cpr_rules::effects::WoundState::None,
        },
        humanity: 50,
        luck_pool: 0,
        money: Eurobucks(0),
        improvement_points: 0,
        lifepath: cpr_rules::Lifepath::default(),
        effects: EffectStack::default(),
        complementary_bonuses: vec![],
    })
}

/// Build a minimal stub [`Character`] for a Narrative NPC.
///
/// Narrative NPCs exist as LLM voicing targets — they have no meaningful
/// combat stats. We give them 1 HP so the effect system doesn't need to
/// special-case "no HP" and a zeroed stat block (default impl, all zeros).
///
/// **Design note:** `StatBlock` has no `Default` impl (it is a pure data
/// struct of `u8`s with no semantic default); we synthesise all-1 stats
/// so that any accidental stat query returns a non-zero value. A HP of 1
/// marks them as instantly Mortally Wounded if targeted — GM should
/// keep Narrative NPCs out of combat.
fn minimal_narrative_character() -> Character {
    let stats = StatBlock {
        int: 1,
        r#ref: 1,
        dex: 1,
        tech: 1,
        cool: 1,
        will: 1,
        luck: 1,
        r#move: 1,
        body: 1,
        emp: 1,
    };
    Character {
        id: CharacterId(Uuid::from_u128(0)),
        name: "NarrativeNpc".to_string(),
        handle: None,
        role: Role::Solo,
        role_rank: 1,
        stats,
        skills: SkillSet::default(),
        cyberware: vec![],
        armor: WornArmor::default(),
        inventory: Inventory::default(),
        wounds: Wounds {
            current_hp: 1,
            max_hp: 1,
            seriously_wounded_threshold: 1,
            death_save_base: 1,
            death_save_penalty: 0,
            current_state: cpr_rules::effects::WoundState::None,
        },
        humanity: 10,
        luck_pool: 0,
        money: Eurobucks(0),
        improvement_points: 0,
        lifepath: cpr_rules::Lifepath::default(),
        effects: EffectStack::default(),
        complementary_bonuses: vec![],
    }
}

/// Resolve loadout weapon slugs into [`ItemStack`]s for the inventory.
fn resolve_loadout_items(
    loadout: &Loadout,
    catalog: &CatalogBundle<'_>,
) -> Result<Vec<ItemStack>, GmError> {
    let mut items = Vec::new();

    for weapon_id in &loadout.weapons {
        if catalog.weapons.get(weapon_id.0.as_str()).is_none() {
            return Err(GmError::LoadoutItemNotFound {
                kind: "weapon",
                slug: weapon_id.0.clone(),
            });
        }
        items.push(ItemStack {
            kind: ItemKind::Weapon(weapon_id.clone()),
            quantity: 1,
        });
    }

    Ok(items)
}

/// Resolve the loadout's optional armor slug into [`WornArmor`].
///
/// If an armor slug is present, it is looked up in `catalog.armor` and its
/// SP is used to populate both the head and body slots (mooks wear their
/// armor on both locations per the book's stat sheets).
fn resolve_worn_armor(
    armor_id: Option<&ArmorId>,
    catalog: &CatalogBundle<'_>,
) -> Result<WornArmor, GmError> {
    let Some(aid) = armor_id else {
        return Ok(WornArmor::default());
    };

    let armor = catalog
        .armor
        .get(aid.0.as_str())
        .ok_or_else(|| GmError::LoadoutItemNotFound {
            kind: "armor",
            slug: aid.0.clone(),
        })?;

    let piece = ArmorPiece {
        kind: armor.kind,
        current_sp: armor.sp,
        max_sp: armor.sp,
    };

    Ok(WornArmor {
        head: Some(piece.clone()),
        body: Some(piece),
    })
}

/// Resolve the loadout's cyberware slugs into [`InstalledCyberware`] entries.
fn resolve_cyberware(
    loadout: &Loadout,
    catalog: &CatalogBundle<'_>,
) -> Result<Vec<InstalledCyberware>, GmError> {
    let mut installed = Vec::new();

    for cw_id in &loadout.cyberware {
        if catalog.cyberware.get(cw_id.0.as_str()).is_none() {
            return Err(GmError::LoadoutItemNotFound {
                kind: "cyberware",
                slug: cw_id.0.clone(),
            });
        }
        installed.push(InstalledCyberware {
            id: cw_id.clone(),
            options: vec![],
        });
    }

    Ok(installed)
}

/// Build a deterministic [`CharacterId`] from the RNG.
fn next_char_id(rng: &mut Rng) -> CharacterId {
    let hi = rng.next_u64();
    let lo = rng.next_u64();
    let bits = (u128::from(hi) << 64) | u128::from(lo);
    CharacterId(Uuid::from_u128(bits))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::npc::entity::{
        Beacon, Loadout, MookArchetype, NpcTemplate, NpcTemplateId, NpcTemplateKind,
    };
    use cpr_rules::catalog::armor::ArmorId;
    use cpr_rules::catalog::armor::{Armor, ArmorKind, ArmorLocation, ArmorPenalty};
    use cpr_rules::catalog::cyberware::{
        Cyberware, CyberwareCategory, HumanityLossSpec, InstallLocation,
    };
    use cpr_rules::catalog::weapons::{DamageDice, DieKind, MeleeKind, RangeBand, WeaponKind};
    use cpr_rules::character::data::WeaponId;
    use cpr_rules::effects::CyberwareId;
    use cpr_rules::types::PriceTier;
    use rand_core::SeedableRng;
    use std::collections::HashMap;
    use uuid::Uuid;

    // -----------------------------------------------------------------------
    // Catalog builders
    // -----------------------------------------------------------------------

    fn make_weapon_catalog(slugs: &[&str]) -> Catalog<Weapon> {
        let mut entries = HashMap::new();
        for slug in slugs {
            entries.insert(
                slug.to_string(),
                Weapon {
                    id: WeaponId(slug.to_string()),
                    display_name: slug.to_string(),
                    kind: WeaponKind::Melee(MeleeKind::Light),
                    damage: DamageDice {
                        n: 1,
                        die: DieKind::D6,
                    },
                    hands: 1,
                    concealable: false,
                    features: vec![],
                    magazine: None,
                    rof: 2,
                    skill: cpr_rules::catalog::skills::SkillId::MeleeWeapon,
                    price: PriceTier::Costly,
                    price_eb: cpr_rules::types::Eurobucks(50),
                    ranges: RangeBand {
                        single_shot: vec![],
                        autofire: None,
                    },
                },
            );
        }
        Catalog::new(entries)
    }

    fn make_armor_catalog(slugs: &[(&str, u8, ArmorKind)]) -> Catalog<Armor> {
        let mut entries = HashMap::new();
        for (slug, sp, kind) in slugs {
            entries.insert(
                slug.to_string(),
                Armor {
                    id: ArmorId(slug.to_string()),
                    display_name: slug.to_string(),
                    kind: *kind,
                    sp: *sp,
                    locations: vec![ArmorLocation::Body, ArmorLocation::Head],
                    penalty: ArmorPenalty::NONE,
                    price: PriceTier::Costly,
                    price_eb: cpr_rules::types::Eurobucks(50),
                },
            );
        }
        Catalog::new(entries)
    }

    fn make_empty_cyberware_catalog() -> Catalog<Cyberware> {
        Catalog::new(HashMap::new())
    }

    #[allow(dead_code)]
    fn make_cyberware_catalog(slugs: &[&str]) -> Catalog<Cyberware> {
        let mut entries = HashMap::new();
        for slug in slugs {
            entries.insert(
                slug.to_string(),
                Cyberware {
                    id: CyberwareId(slug.to_string()),
                    display_name: slug.to_string(),
                    description: String::new(),
                    category: CyberwareCategory::Neuralware,
                    install_difficulty: InstallLocation::Clinic,
                    humanity_loss: HumanityLossSpec::Fixed(2),
                    effects: vec![],
                    prerequisite: None,
                    option_slots: 0,
                    slot_cost: 0,
                    price: PriceTier::Expensive,
                    price_eb: cpr_rules::types::Eurobucks(500),
                },
            );
        }
        Catalog::new(entries)
    }

    fn goon_statline() -> MookStatline {
        // Bodyguard stat block, p.412:
        //   INT 3, REF 6, DEX 5, TECH 2, COOL 4, WILL 4, MOVE 4, BODY 6, EMP 3
        //   HP 35, SW 18, DS 6
        MookStatline {
            archetype: MookArchetype::Goon,
            stats: StatBlock {
                int: 3,
                r#ref: 6,
                dex: 5,
                tech: 2,
                cool: 4,
                will: 4,
                luck: 0,
                r#move: 4,
                body: 6,
                emp: 3,
            },
            skills: SkillSet::default(),
            hp: 35,
            default_loadout: Loadout {
                weapons: vec![],
                armor: None,
                cyberware: vec![],
            },
        }
    }

    fn make_mook_catalog(statlines: Vec<MookStatline>) -> Catalog<MookStatline> {
        let entries: HashMap<String, MookStatline> = statlines
            .into_iter()
            .map(|s| (format!("{:?}", s.archetype), s))
            .collect();
        Catalog::new(entries)
    }

    fn seeded_rng() -> Rng {
        Rng::seed_from_u64(42)
    }

    // -----------------------------------------------------------------------
    // test_goon_hp_matches_book
    // -----------------------------------------------------------------------

    /// Verify that a Goon mook's instantiated HP matches the rulebook
    /// value on p.412 (Bodyguard stat block: 35 HP).
    ///
    /// The WP-606 prompt suggests "likely 25 hp" for the Goon, but the
    /// Bodyguard stat sheet on p.412 clearly shows **35 HP**. The rulebook
    /// is authoritative (RAW). This test asserts 35.
    ///
    /// Deviation flagged: the WP prompt's "likely 25 hp" guess is wrong;
    /// the real RAW value from p.412 is 35 HP for the Bodyguard (Goon).
    #[test]
    fn test_goon_hp_matches_book() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[]);
        let cw_cat = make_empty_cyberware_catalog();

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("test_goon"),
            display_name: "Test Goon".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Goon,
                loadout: Loadout {
                    weapons: vec![],
                    armor: None,
                    cyberware: vec![],
                },
            },
            description: "Test goon".to_string(),
            initial_disposition: -3,
            voice_notes: String::new(),
        };

        let mut rng = seeded_rng();
        let active = instantiate_npc(&template, &catalog, &mut rng)
            .expect("Goon instantiation must succeed");

        // RAW: Bodyguard (Goon) HP = 35 per p.412.
        assert_eq!(
            active.character.wounds.current_hp, 35,
            "Goon HP must match p.412 RAW value of 35"
        );
        assert_eq!(active.character.wounds.max_hp, 35);
    }

    // -----------------------------------------------------------------------
    // test_lieutenant_uses_template_character
    // -----------------------------------------------------------------------

    /// Verify that instantiating a Lieutenant uses the template's `character`
    /// verbatim (stats are copied without modification).
    #[test]
    fn test_lieutenant_uses_template_character() {
        let known_stats = StatBlock {
            int: 6,
            r#ref: 8,
            dex: 7,
            tech: 4,
            cool: 6,
            will: 5,
            luck: 3,
            r#move: 5,
            body: 7,
            emp: 4,
        };

        let char_id = CharacterId(Uuid::from_u128(0xAB_CD));
        let lieutenant_char = Character {
            id: char_id,
            name: "Maelstrom Lt".to_string(),
            handle: Some("Razor".to_string()),
            role: Role::Solo,
            role_rank: 4,
            stats: known_stats,
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            inventory: Inventory::default(),
            wounds: Wounds {
                current_hp: 40,
                max_hp: 40,
                seriously_wounded_threshold: 20,
                death_save_base: 7,
                death_save_penalty: 0,
                current_state: cpr_rules::effects::WoundState::None,
            },
            humanity: 30,
            luck_pool: 3,
            money: Eurobucks(500),
            improvement_points: 0,
            lifepath: cpr_rules::Lifepath::default(),
            effects: EffectStack::default(),
            complementary_bonuses: vec![],
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("maelstrom_lt"),
            display_name: "Maelstrom Lieutenant".to_string(),
            kind: NpcTemplateKind::Lieutenant {
                character: lieutenant_char.clone(),
            },
            description: "A lieutenant of the Maelstrom gang.".to_string(),
            initial_disposition: -5,
            voice_notes: "Aggressive, terse.".to_string(),
        };

        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[]);
        let cw_cat = make_empty_cyberware_catalog();
        let mook_cat = make_mook_catalog(vec![]);

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let mut rng = seeded_rng();
        let active = instantiate_npc(&template, &catalog, &mut rng)
            .expect("Lieutenant instantiation must succeed");

        // The character must be copied verbatim — stats equal to template's.
        assert_eq!(
            active.character.stats, known_stats,
            "Lieutenant stats must match template character verbatim"
        );
        assert_eq!(active.character.wounds.current_hp, 40);
        assert_eq!(active.gig_disposition, -5);
    }

    // -----------------------------------------------------------------------
    // test_loadout_weapon_missing_errors
    // -----------------------------------------------------------------------

    /// Verify that a Mook with a loadout referencing a missing weapon slug
    /// returns `GmError::LoadoutItemNotFound { kind: "weapon", slug: ... }`.
    #[test]
    fn test_loadout_weapon_missing_errors() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        // Intentionally empty weapon catalog — the loadout slug won't resolve.
        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[]);
        let cw_cat = make_empty_cyberware_catalog();

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let missing_slug = "nonexistent_shotgun";
        let template = NpcTemplate {
            id: NpcTemplateId::from("test_goon_bad_loadout"),
            display_name: "Test Goon (bad loadout)".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Goon,
                loadout: Loadout {
                    weapons: vec![WeaponId(missing_slug.to_string())],
                    armor: None,
                    cyberware: vec![],
                },
            },
            description: "Goon with a missing weapon slug".to_string(),
            initial_disposition: -3,
            voice_notes: String::new(),
        };

        let mut rng = seeded_rng();
        let result = instantiate_npc(&template, &catalog, &mut rng);

        match result {
            Err(GmError::LoadoutItemNotFound { kind, slug }) => {
                assert_eq!(kind, "weapon");
                assert_eq!(slug, missing_slug);
            }
            other => panic!("expected LoadoutItemNotFound, got: {other:?}"),
        }
    }

    // -----------------------------------------------------------------------
    // Additional coverage tests
    // -----------------------------------------------------------------------

    /// Narrative NPC instantiation produces 1 HP and no loadout errors.
    #[test]
    fn test_narrative_npc_has_minimal_stats() {
        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[]);
        let cw_cat = make_empty_cyberware_catalog();
        let mook_cat = make_mook_catalog(vec![]);

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("fixer_padre"),
            display_name: "Padre".to_string(),
            kind: NpcTemplateKind::Narrative {
                role_sketch: Some("Avuncular Fixer".to_string()),
                beacons: vec![Beacon {
                    label: "Loyalty".to_string(),
                    note: "Won't betray contacts.".to_string(),
                }],
            },
            description: "Veteran Fixer in the Combat Zone.".to_string(),
            initial_disposition: 2,
            voice_notes: "Gravelly voice.".to_string(),
        };

        let mut rng = seeded_rng();
        let active = instantiate_npc(&template, &catalog, &mut rng)
            .expect("Narrative NPC instantiation must succeed");

        assert_eq!(active.character.wounds.current_hp, 1);
        assert_eq!(active.character.wounds.max_hp, 1);
        assert_eq!(active.gig_disposition, 2);
    }

    /// Mook with armor in loadout resolves worn armor on both head and body.
    #[test]
    fn test_mook_with_armor_loadout() {
        let mook_cat = make_mook_catalog(vec![goon_statline()]);
        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[("kevlar", 7, ArmorKind::Kevlar)]);
        let cw_cat = make_empty_cyberware_catalog();

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("armored_goon"),
            display_name: "Armored Goon".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Goon,
                loadout: Loadout {
                    weapons: vec![],
                    armor: Some(ArmorId("kevlar".to_string())),
                    cyberware: vec![],
                },
            },
            description: "Goon wearing Kevlar.".to_string(),
            initial_disposition: -2,
            voice_notes: String::new(),
        };

        let mut rng = seeded_rng();
        let active = instantiate_npc(&template, &catalog, &mut rng)
            .expect("Armored goon instantiation must succeed");

        let head = active
            .character
            .armor
            .head
            .as_ref()
            .expect("head armor must be set");
        let body = active
            .character
            .armor
            .body
            .as_ref()
            .expect("body armor must be set");
        assert_eq!(head.kind, ArmorKind::Kevlar);
        assert_eq!(head.current_sp, 7);
        assert_eq!(body.kind, ArmorKind::Kevlar);
        assert_eq!(body.current_sp, 7);
    }

    /// Missing archetype in mook catalog returns MookArchetypeNotFound.
    #[test]
    fn test_missing_mook_archetype_errors() {
        // Empty mook catalog.
        let mook_cat = make_mook_catalog(vec![]);
        let weapon_cat = make_weapon_catalog(&[]);
        let armor_cat = make_armor_catalog(&[]);
        let cw_cat = make_empty_cyberware_catalog();

        let catalog = CatalogBundle {
            weapons: &weapon_cat,
            armor: &armor_cat,
            cyberware: &cw_cat,
            mooks: &mook_cat,
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("mystery_mook"),
            display_name: "Mystery Mook".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::Cultist,
                loadout: Loadout {
                    weapons: vec![],
                    armor: None,
                    cyberware: vec![],
                },
            },
            description: "A cultist mook.".to_string(),
            initial_disposition: -4,
            voice_notes: String::new(),
        };

        let mut rng = seeded_rng();
        let result = instantiate_npc(&template, &catalog, &mut rng);

        assert!(
            matches!(result, Err(GmError::MookArchetypeNotFound(_))),
            "missing mook archetype must return MookArchetypeNotFound"
        );
    }
}
