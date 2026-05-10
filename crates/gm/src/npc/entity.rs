//! NPC entity model — templates and active runtime instances.
//!
//! Defines the two faces of an NPC:
//!
//! - [`NpcTemplate`] — authored content loaded from RON at startup. Describes
//!   what kind of NPC this is, how it behaves, and what equipment it carries.
//!   Identified by a [`NpcTemplateId`] slug (e.g. `"fixer_padre"`).
//!
//! - [`ActiveNpc`] — a live instance inside a gig scene, carrying a resolved
//!   [`Character`] and a per-gig disposition drift. Identified by a
//!   [`cpr_rules::types::EntityId`] UUID.
//!
//! Rulebook references:
//! - **pp.418–419:** Mook encounter archetypes used as the basis for
//!   [`MookArchetype`] — Goon / Edgerunner / gang-gang variants.
//!
//! ## Naming deviation from WP-605 spec
//!
//! The spec writes `NpcId` for template slugs. However, `cpr_rules::NpcId`
//! already exists as a UUID for runtime entity instances. This module
//! therefore defines [`NpcTemplateId`] for the slug-based template identifier
//! to avoid a type collision. See §5.2 "Coexist" deviation note in the PR.

#![forbid(unsafe_code)]

use crate::Character;
use cpr_rules::catalog::armor::ArmorId;
use cpr_rules::character::data::WeaponId;
use cpr_rules::effects::CyberwareId;
use cpr_rules::types::EntityId;
use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// NpcTemplateId
// ---------------------------------------------------------------------------

/// Slug-based identifier for an [`NpcTemplate`].
///
/// A template slug is a human-readable stable string such as `"fixer_padre"`
/// or `"maelstrom_grunt"` that addresses authored NPC content in the
/// `content/npcs/` directory. This is distinct from
/// [`cpr_rules::types::NpcId`], which is a UUID that identifies a live
/// runtime instance.
///
/// # Deviation
///
/// The WP-605 spec names this type `NpcId`. That name is taken by
/// `cpr_rules::types::NpcId` (a `Uuid`-wrapper for runtime instances).
/// Renamed to `NpcTemplateId` here to avoid a collision.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NpcTemplateId(pub String);

impl NpcTemplateId {
    /// Borrow the slug as a `&str`.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for NpcTemplateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for NpcTemplateId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for NpcTemplateId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ---------------------------------------------------------------------------
// Beacon
// ---------------------------------------------------------------------------

/// A short narrative label + explanatory note attached to a Narrative NPC.
///
/// Beacons give the LLM structured access to an NPC's personality hooks,
/// recurring motivations, and vocal tics without embedding freeform text in
/// the core data model.
///
/// Example:
/// ```text
/// Beacon { label: "Loyalty", note: "Will not betray contacts under any threat." }
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Beacon {
    /// Short label naming the personality trait or dramatic hook.
    pub label: String,
    /// Expanded note describing how the beacon manifests in play.
    pub note: String,
}

// ---------------------------------------------------------------------------
// Loadout
// ---------------------------------------------------------------------------

/// Equipment carried by a Mook NPC.
///
/// Slugs reference entries in the corresponding catalogs (`Catalog<Weapon>`,
/// `Catalog<Armor>`, and the cyberware catalog). WP-606 resolves these slugs
/// during instantiation; WP-605 only stores the references.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Loadout {
    /// Weapon slugs the mook carries. See `content/catalogs/weapons.ron`.
    pub weapons: Vec<WeaponId>,
    /// Optional armor slug. `None` means no armor worn. See `content/catalogs/armor.ron`.
    pub armor: Option<ArmorId>,
    /// Cyberware slugs installed on the mook. See `content/catalogs/cyberware/`.
    pub cyberware: Vec<CyberwareId>,
}

// ---------------------------------------------------------------------------
// MookArchetype
// ---------------------------------------------------------------------------

/// Broad combat-role archetype for a Mook NPC.
///
/// These archetypes correspond to the encounter tables on pp.418–419 of the
/// rulebook, where each entry represents a distinct threat type with
/// its own typical gear, behaviour, and stat profile. WP-606 maps each
/// variant to default stat tables and default loadouts.
///
/// Rulebook reference: **pp.418–419** (Encounter tables, Night City).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MookArchetype {
    /// Generic street muscle — the baseline mook. Encounters: local gangers,
    /// hired muscle. See p.418.
    Goon,
    /// A skilled independent operator, typically armed and cybered.
    /// Used for Edgerunner and solo-team encounter entries. See p.418.
    Edgerunner,
    /// Maelstrom gang member — chrome-obsessed, aggressive, often carrying
    /// heavy weaponry and extensive cyberware. See p.418.
    MaelstromGanger,
    /// Nomad Pack ganger — Road Gangers or Steel Vaquero equivalent,
    /// typically armed with rifles and crossbows. See pp.418–419.
    NomadGanger,
    /// Boostergang punk — Iron Sights-type gangers with SMGs, Rippers, and
    /// cyber. See p.419.
    BoosterGanger,
    /// Bozo gang clown — Piranhas / Red Chrome Legion street-level gang
    /// member. See p.418.
    BozoGanger,
    /// Corporate security operative — light armorjack, SMG or assault rifle.
    /// See pp.418–419 (Corporate Guards).
    SecurityOperative,
    /// Corporate security officer — heavier kit than the operative; used for
    /// Corporate backup and executive-zone enforcement. See pp.418–420.
    SecurityOfficer,
    /// Street punk / scavver — bottom-tier threat, improvised weapons and
    /// minimal armor. See p.419 (Street Punks, Scavvers).
    StreetPunk,
    /// Trauma Team combat medic — arrives in AV-4, Assault Rifle armed,
    /// Medium Armorjack. See p.419 (Trauma Team entry, pg.224 stat-block
    /// reference).
    TraumaTeamMedic,
    /// Cult / Reclaimer member — cultists and religious gang fighters with
    /// improvised weaponry. See p.418 (Culties, Reclaimers).
    Cultist,
    /// Private investigator — lightly armored, heavy pistol, detective tools.
    /// See pp.418–420.
    PrivateInvestigator,
}

// ---------------------------------------------------------------------------
// NpcTemplateKind
// ---------------------------------------------------------------------------

/// The structural kind of an [`NpcTemplate`], driving how it is instantiated.
///
/// - Narrative NPCs exist primarily as story fixtures for the LLM to voice.
/// - Mooks are procedural adversaries spawned in bulk from stat archetypes.
/// - Lieutenants and Bosses carry full hand-authored [`Character`] stat-blocks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NpcTemplateKind {
    /// A story NPC that the LLM voices — a fixer, a doc, a bartender.
    ///
    /// Narrative NPCs have minimal combat stats; they exist to interact with
    /// the player in dialogue and drive plot. `role_sketch` is a one-liner
    /// the LLM can use to anchor voice (e.g. `"Padre the Fixer, avuncular,
    /// deal-focused"`). `beacons` provide structured personality hooks.
    Narrative {
        /// Optional one-liner describing role and personality.
        role_sketch: Option<String>,
        /// Personality / narrative hooks exposed to the LLM. See [`Beacon`].
        beacons: Vec<Beacon>,
    },
    /// A procedural adversary — a crowd-filler with no individual identity.
    ///
    /// Mooks are instantiated in bulk from [`MookArchetype`] stat tables
    /// (pp.418–419) and the given [`Loadout`]. WP-606 handles the
    /// actual stat instantiation.
    Mook {
        /// Determines the default stat line for the mook. See pp.418–419.
        archetype: MookArchetype,
        /// Weapons, armor, and cyberware this mook type carries.
        loadout: Loadout,
    },
    /// A named adversary with a hand-authored stat block, intermediate
    /// threat level. The [`Character`] is taken verbatim at instantiation.
    Lieutenant {
        /// Full authored stat block for this Lieutenant.
        character: Character,
    },
    /// The primary antagonist of a gig — full authored stat block,
    /// typically with a unique loadout and role ability.
    Boss {
        /// Full authored stat block for this Boss.
        character: Character,
    },
}

// ---------------------------------------------------------------------------
// NpcTemplate
// ---------------------------------------------------------------------------

/// Authored description of an NPC type, loaded from `content/npcs/`.
///
/// An `NpcTemplate` is the static blueprint; [`ActiveNpc`] is the live
/// runtime instance spawned from it by WP-606's `instantiate_npc`.
///
/// Fields shared across all template kinds:
///
/// - `id` — slug key (`NpcTemplateId`), e.g. `"fixer_padre"`.
/// - `display_name` — human-readable name shown in the UI.
/// - `kind` — structural variant driving instantiation (see [`NpcTemplateKind`]).
/// - `description` — freeform text passed to the LLM for context.
/// - `initial_disposition` — starting attitude toward the player, −10..=+10.
/// - `voice_notes` — hints for the LLM when generating dialogue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NpcTemplate {
    /// Slug identifier for this template. See [`NpcTemplateId`].
    pub id: NpcTemplateId,
    /// Human-readable name displayed in the UI and used by the LLM.
    pub display_name: String,
    /// Structural kind that controls instantiation. See [`NpcTemplateKind`].
    pub kind: NpcTemplateKind,
    /// Freeform context text passed to the LLM for scene-setting.
    pub description: String,
    /// Starting attitude toward the player. −10 = hostile, +10 = devoted.
    ///
    /// Per-gig drift is tracked on [`ActiveNpc::gig_disposition`]; this
    /// field is the fresh-start default each time the NPC appears.
    pub initial_disposition: i8,
    /// Dialogue coaching notes for the LLM — vocal tics, speech patterns,
    /// recurring phrases, tone.
    pub voice_notes: String,
}

// ---------------------------------------------------------------------------
// ActiveNpc
// ---------------------------------------------------------------------------

/// A live NPC instance inside a gig scene.
///
/// `ActiveNpc` is produced by WP-606's `instantiate_npc` from an
/// [`NpcTemplate`]. It holds:
///
/// - `template` — the [`NpcTemplateId`] slug that spawned this instance,
///   used to look up authored content during the gig.
/// - `entity_id` — the [`EntityId`] UUID that addresses this instance on
///   the combat grid and in the effect system.
/// - `character` — a fully resolved [`Character`] (stat block, inventory,
///   effects) produced at instantiation time. For Mook templates WP-606
///   fills this from archetype tables + loadout; for Narrative templates
///   a minimal stub is used.
/// - `gig_disposition` — attitude that can drift per gig from NPC actions,
///   player choices, or faction events. Initialised from
///   [`NpcTemplate::initial_disposition`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActiveNpc {
    /// Slug of the [`NpcTemplate`] that produced this instance.
    pub template: NpcTemplateId,
    /// UUID identifying this entity on the combat grid and effect system.
    pub entity_id: EntityId,
    /// Fully instantiated character record. See WP-606 for how archetypes
    /// and loadouts are resolved into stats and inventory.
    pub character: Character,
    /// Per-gig attitude toward the player. Initialised from
    /// [`NpcTemplate::initial_disposition`]; may drift in response to
    /// in-gig events.
    pub gig_disposition: i8,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cpr_rules::character::data::{Inventory, Role, SkillSet, StatBlock, WornArmor, Wounds};
    use cpr_rules::effects::EffectStack;
    use cpr_rules::types::{CharacterId, EntityId, Eurobucks};
    use uuid::Uuid;

    /// Build a minimal `Character` suitable for tests.
    fn minimal_character(name: &str) -> Character {
        use cpr_rules::catalog::lifepath::Lifepath;
        Character {
            id: CharacterId(Uuid::nil()),
            name: name.to_string(),
            handle: None,
            role: Role::Solo,
            role_rank: 1,
            stats: StatBlock {
                int: 5,
                r#ref: 5,
                dex: 5,
                tech: 5,
                cool: 5,
                will: 5,
                luck: 5,
                r#move: 5,
                body: 5,
                emp: 5,
            },
            skills: SkillSet::default(),
            cyberware: vec![],
            armor: WornArmor::default(),
            wounds: Wounds::default(),
            effects: EffectStack::default(),
            inventory: Inventory::default(),
            money: Eurobucks(0),
            humanity: 50,
            luck_pool: 5,
            improvement_points: 0,
            complementary_bonuses: vec![],
            lifepath: Lifepath::default(),
        }
    }

    /// `test_npc_template_round_trips` — a Narrative NpcTemplate serialises
    /// to RON and deserialises back to an equal value.
    #[test]
    fn test_npc_template_round_trips() {
        let template = NpcTemplate {
            id: NpcTemplateId::from("fixer_padre"),
            display_name: "Padre".to_string(),
            kind: NpcTemplateKind::Narrative {
                role_sketch: Some(
                    "Avuncular Fixer, deal-focused, never betrays contacts".to_string(),
                ),
                beacons: vec![
                    Beacon {
                        label: "Loyalty".to_string(),
                        note: "Will not give up contacts even under threat.".to_string(),
                    },
                    Beacon {
                        label: "Greed".to_string(),
                        note: "Always angles for a cut of every deal.".to_string(),
                    },
                ],
            },
            description: "Padre is a veteran Fixer operating out of the Combat Zone.".to_string(),
            initial_disposition: 2,
            voice_notes: "Gravelly voice, avoids direct answers, speaks in metaphors.".to_string(),
        };

        let serialized = ron::to_string(&template).expect("RON serialization must succeed");
        let deserialized: NpcTemplate =
            ron::from_str(&serialized).expect("RON deserialization must succeed");

        assert_eq!(template, deserialized);
        assert_eq!(deserialized.id, NpcTemplateId::from("fixer_padre"));
        assert_eq!(deserialized.initial_disposition, 2);

        if let NpcTemplateKind::Narrative { beacons, .. } = &deserialized.kind {
            assert_eq!(beacons.len(), 2);
            assert_eq!(beacons[0].label, "Loyalty");
        } else {
            panic!("expected Narrative kind");
        }
    }

    /// `test_mook_instantiates_with_loadout` — construct an `ActiveNpc` from a
    /// Mook template (manually building the `Character` for now; full
    /// instantiation is WP-606) and assert the loadout's weapons appear on
    /// its character's inventory.
    ///
    /// This test validates the data model only: the `Loadout` fields round-trip
    /// correctly and can be read back out of an `ActiveNpc`.
    #[test]
    fn test_mook_instantiates_with_loadout() {
        let heavy_smg = WeaponId("heavy_smg".to_string());
        let kevlar = ArmorId("kevlar".to_string());
        let neural_link = CyberwareId("neural_link".to_string());

        let loadout = Loadout {
            weapons: vec![heavy_smg.clone()],
            armor: Some(kevlar.clone()),
            cyberware: vec![neural_link.clone()],
        };

        let template = NpcTemplate {
            id: NpcTemplateId::from("maelstrom_grunt"),
            display_name: "Maelstrom Grunt".to_string(),
            kind: NpcTemplateKind::Mook {
                archetype: MookArchetype::MaelstromGanger,
                loadout: loadout.clone(),
            },
            description: "Chrome-obsessed Maelstrom gang member. See p.418.".to_string(),
            initial_disposition: -5,
            voice_notes: "Aggressive, terse, refers to chrome as sacred.".to_string(),
        };

        // For WP-605 we build the Character manually; WP-606 will automate this
        // from the archetype stat tables.
        let mut character = minimal_character("Maelstrom Grunt");

        // Seed the inventory with the loadout's weapons so the assertion below
        // validates the round-trip, not just template shape.
        use cpr_rules::character::data::{ItemKind, ItemStack};
        character.inventory.items.push(ItemStack {
            kind: ItemKind::Weapon(heavy_smg.clone()),
            quantity: 1,
        });

        let active = ActiveNpc {
            template: NpcTemplateId::from("maelstrom_grunt"),
            entity_id: EntityId(Uuid::nil()),
            character,
            gig_disposition: template.initial_disposition,
        };

        // Verify template fields survived construction.
        assert_eq!(active.template, NpcTemplateId::from("maelstrom_grunt"));
        assert_eq!(active.gig_disposition, -5);

        // Verify the loadout's weapon appears on the character's inventory.
        let has_heavy_smg = active
            .character
            .inventory
            .items
            .iter()
            .any(|stack| matches!(&stack.kind, ItemKind::Weapon(wid) if wid.0 == "heavy_smg"));
        assert!(
            has_heavy_smg,
            "character inventory must contain the loadout's heavy_smg"
        );

        // Verify the loadout struct itself round-trips through RON.
        let loadout_ron = ron::to_string(&loadout).expect("loadout RON serialize");
        let loadout_back: Loadout = ron::from_str(&loadout_ron).expect("loadout RON deserialize");
        assert_eq!(
            loadout_back.weapons.first().map(|w| w.0.as_str()),
            Some("heavy_smg")
        );
        assert_eq!(
            loadout_back.armor.as_ref().map(|a| a.0.as_str()),
            Some("kevlar")
        );
        assert_eq!(
            loadout_back.cyberware.first().map(|c| c.0.as_str()),
            Some("neural_link")
        );

        // Verify the MookArchetype is correctly tagged.
        if let NpcTemplateKind::Mook {
            archetype,
            loadout: lout,
        } = &template.kind
        {
            assert_eq!(*archetype, MookArchetype::MaelstromGanger);
            assert_eq!(
                lout.weapons.first().map(|w| w.0.as_str()),
                Some("heavy_smg")
            );
        } else {
            panic!("expected Mook kind");
        }
    }
}
