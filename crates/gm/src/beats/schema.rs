//! Beat Chart schema — the authored RON types that describe a Gig.
//!
//! A Beat Chart is the scripting framework for a Cyberpunk RED adventure (rulebook pp.395–408).
//! It organises scenes ("Beats") into a directed graph, each typed by its narrative role
//! (Hook → Development/Cliffhanger alternation → Climax → Resolution). Each Beat records
//! which NPCs are present, what mechanical checks are available, and what transitions are
//! possible depending on player actions.
//!
//! This module defines the complete RON-serialisable schema. It is the source of truth for
//! WP-603 (loader/validator) and WP-604 (runtime state machine).
//!
//! ## Rulebook references
//!
//! - pp.395–396: The three rules of Beat Charts; Hook always first, Climax + Resolution always last.
//! - pp.396–408: Beat type catalogue (Hook, Cliffhanger, Development, Climax, Resolution) and
//!   exhaustive list of named Beat sub-types (Ambush, Battle, Discovery, etc.).

use crate::ids::{BeatId, EncounterId, GigId, MechanicalHookId};
use cpr_rules::character::ItemKind;
use cpr_rules::types::{Eurobucks, Stat, DV};
use cpr_rules::world::LocationId;
use cpr_rules::SkillId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Top-level Gig ───────────────────────────────────────────────────────────

/// A complete authored Gig — one Beat Chart, all its Beats, NPCs, and locations.
///
/// Deserialised from `content/gigs/<slug>.ron`. The validator (WP-603) checks
/// that `start_beat` is a `BeatKind::Hook`, that every `Transition.target` exists,
/// that there is at least one path from `start_beat` to a Climax → Resolution chain,
/// and that all cross-referenced slugs resolve.
///
/// **Rulebook:** pp.395–396 (Beat Chart structure).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Gig {
    /// Unique slug identifying this Gig across content files (e.g. `"hot_property"`).
    pub id: GigId,
    /// Human-readable adventure title shown to the player/LLM.
    pub title: String,
    /// Slug of the Fixer NPC who contracts the crew.
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    pub fixer: String,
    /// Base payment bracket for this Gig. See [`PaymentTier`].
    pub payment: PaymentTier,
    /// Short prose description of the setting / atmosphere (e.g. "Corporate tower, high security").
    /// Passed to the LLM as scene context.
    pub setting: String,
    /// Approximate in-world duration of the gig in hours; used for scope checks.
    /// Rulebook p.396: "one Beat ≈ ½ hour of real-world play".
    pub scope_hours: u8,
    /// NPC templates referenced anywhere in this Gig, keyed by the NPC slug.
    ///
    /// TODO(WP-605): replace String keys with NpcTemplateId
    pub npcs: HashMap<String, NpcRef>,
    /// Location metadata for every `LocationId` referenced in the Beats.
    pub locations: HashMap<LocationId, LocationRef>,
    /// Ordered or unordered list of all Beats in this Gig.
    /// The graph topology is encoded by each Beat's `transitions`.
    pub beats: Vec<Beat>,
    /// The id of the first Beat. Must be a `BeatKind::Hook` (enforced by WP-603 validator).
    pub start_beat: BeatId,
}

// ─── Beat ─────────────────────────────────────────────────────────────────────

/// One scene in the Beat Chart.
///
/// **Rulebook:** p.395 — "Each 'chunk' of story should convey information, be entertaining,
/// and help provide excitement by pushing the plot along in some visible way."
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Beat {
    /// Unique slug within this Gig (e.g. `"hook-discovery"`, `"dev-1"`).
    pub id: BeatId,
    /// The dramatic type of this Beat. Constrains ordering rules (pp.395–396).
    pub kind: BeatKind,
    /// Location where this scene takes place.
    pub location: LocationId,
    /// NPC slugs that are physically present at the start of this Beat.
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    pub present: Vec<String>,
    /// Prose narrative intent for the LLM: what this Beat is trying to accomplish
    /// dramaturgically. Not shown to the player directly.
    pub intent: String,
    /// Mechanical interactions the GM engine can resolve in this Beat (skill checks,
    /// negotiations, searches, etc.). See [`MechanicalHook`].
    pub mechanical_hooks: Vec<MechanicalHook>,
    /// Optional combat encounter that may be triggered in this Beat.
    pub encounter: Option<EncounterRef>,
    /// All possible exits from this Beat; resolved by the runtime (WP-604).
    pub transitions: Vec<Transition>,
}

/// Narrative role / dramatic type of a Beat.
///
/// **Rulebook:** p.395 — five types, each with structural constraints:
/// - The chart always begins with a Hook.
/// - Developments and Cliffhangers alternate; never two in a row.
/// - The chart ends with a Climax followed immediately by a Resolution.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum BeatKind {
    /// Opening scene; hooks the crew into the story (p.395, p.397).
    Hook,
    /// Plot-forwarding non-combat scene: clues, revelations, conversations (p.395, pp.402–405).
    Development,
    /// Action scene: chases, dogfights, battles (p.395, p.399).
    Cliffhanger,
    /// Big finale — always followed by a Resolution (p.395, p.406).
    Climax,
    /// Tag-line denouement; ties up the plot (p.395, p.406–407).
    Resolution,
}

// ─── NPC & Location references ───────────────────────────────────────────────

/// A reference to an NPC template used in this Gig.
///
/// The `template` field is the slug of a RON file under `content/npcs/`.
/// WP-605 will introduce `NpcTemplateId`; until then the slug is untyped.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NpcRef {
    /// Slug of the NPC template (e.g. `"corp_guard_lt"`).
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    pub template: String,
}

/// Metadata for a location used in this Gig.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LocationRef {
    /// Optional grid-map asset slug (e.g. `"arasaka_lobby_grid"`).
    pub map: Option<String>,
    /// Short prose description for the LLM.
    pub description: String,
}

/// Reference to a pre-authored combat encounter by slug.
///
/// The slug identifies a RON file under `content/encounters/`.
/// WP-610 introduces the full encounter loader.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct EncounterRef(
    /// Slug of the encounter definition file.
    pub EncounterId,
);

// ─── Payment ─────────────────────────────────────────────────────────────────

/// Payment bracket for a Gig, binding the Eurobucks amount to a named tier.
///
/// The tiers follow Night Market price-tier conventions (p.340):
/// Cheap < Costly < Premium.  `Custom` allows arbitrary scripted amounts.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum PaymentTier {
    /// Quick street-level job (e.g. 200–500 eb).
    Cheap(Eurobucks),
    /// Mid-range corporate gig (e.g. 500–2 000 eb).
    Costly(Eurobucks),
    /// High-value extraction / sabotage (e.g. 2 000+ eb).
    Premium(Eurobucks),
    /// GM-scripted amount outside the standard ladder.
    Custom(Eurobucks),
}

// ─── Transitions ─────────────────────────────────────────────────────────────

/// A directed edge in the Beat graph: under `condition`, move to `target`.
///
/// Multiple transitions may be attached to a Beat; the runtime (WP-604) evaluates
/// them in order and takes the first whose condition is currently satisfied.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Transition {
    /// The logical predicate that must hold for this edge to be taken.
    pub condition: TransitionCondition,
    /// Id of the Beat to advance to.
    pub target: BeatId,
}

/// Predicate that gates a [`Transition`].
///
/// Variants correspond to things the rules engine can observe (encounter outcome,
/// hook resolution, NPC state) or that surface a UI choice to the player.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum TransitionCondition {
    /// Transition is always taken (unconditional / fallthrough).
    Always,
    /// The Beat's encounter was resolved without triggering a general alarm.
    EncounterResolvedSilently,
    /// The Beat's encounter was resolved, but an alarm was triggered (witnesses, gunfire, etc.).
    EncounterResolvedLoud,
    /// An alarm state has been raised in the current scene.
    AlarmRaised,
    /// The named [`MechanicalHook`] succeeded its check.
    HookSucceeded(MechanicalHookId),
    /// The named [`MechanicalHook`] failed its check.
    HookFailed(MechanicalHookId),
    /// An NPC the crew made a deal with broke that deal.
    PromiseBroken(String),
    /// An NPC the crew made a deal with kept that deal.
    PromiseKept(String),
    /// The named NPC was killed during this Beat.
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    NpcKilled(String),
    /// The named NPC became an ally of the crew.
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    NpcAlly(String),
    /// The player is presented with an explicit branching choice in the UI.
    /// The `String` is the choice label shown to the player.
    PlayerChoice(String),
}

// ─── Mechanical Hooks ─────────────────────────────────────────────────────────

/// A named mechanical interaction available in a Beat.
///
/// Hooks are resolved by WP-613 (hook resolver). The results feed into
/// `TransitionCondition::HookSucceeded` / `HookFailed` and `HookEffect`
/// applications.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct MechanicalHook {
    /// Unique slug for this hook within the Gig.
    pub id: MechanicalHookId,
    /// The concrete mechanic to resolve.
    pub kind: MechanicalHookKind,
}

/// The specific mechanical procedure for a [`MechanicalHook`].
///
/// **Rulebook references:**
/// - `SkillCheck` / `OpposedCheck`: p.130–131 (core resolution).
/// - `Negotiation`: p.381–382 (Fixer haggling, Credibility checks).
/// - `Ambush`: p.399 (Ambush Cliffhanger — Awareness vs Stealth).
/// - `Search`: various (Perception checks to find clues/items).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum MechanicalHookKind {
    /// A standard STAT + Skill vs DV check (p.130).
    SkillCheck {
        /// Linked stat for the roll.
        stat: Stat,
        /// Skill being tested.
        skill: SkillId,
        /// Target difficulty.
        dv: DV,
        /// Effect applied on a successful roll.
        on_success: HookEffect,
        /// Effect applied on a failed roll.
        on_failure: HookEffect,
    },
    /// An opposed check: attacker's STAT+Skill vs defender's STAT+Skill (p.131).
    OpposedCheck {
        /// Stat for the acting party.
        attacker_stat: Stat,
        /// Skill for the acting party.
        attacker_skill: SkillId,
        /// Stat for the resisting party.
        defender_stat: Stat,
        /// Skill for the resisting party.
        defender_skill: SkillId,
        /// Effect when the attacker wins.
        on_attacker_wins: HookEffect,
        /// Effect when the defender wins.
        on_defender_wins: HookEffect,
    },
    /// A negotiation scene — roleplayed, with a persuasion/trading check (p.381–382).
    Negotiation {
        /// Stat used (typically Cool or Empathy).
        stat: Stat,
        /// Skill used (e.g. Persuasion, Trading).
        skill: SkillId,
        /// Difficulty of the negotiation.
        dv: DV,
        /// Effect on a successful negotiation.
        on_success: HookEffect,
        /// Effect on a failed negotiation.
        on_failure: HookEffect,
    },
    /// An ambush scenario — crew tests Awareness vs NPC Stealth (p.399).
    Ambush {
        /// Slug of the NPC springing the ambush.
        ///
        /// TODO(WP-605): replace String with NpcTemplateId
        ambusher: String,
        /// Effect applied if the crew detects the ambush in time.
        on_detected: HookEffect,
        /// Effect applied if the ambush is sprung undetected.
        on_surprised: HookEffect,
    },
    /// The crew searches an area for hidden items or information (Perception check).
    Search {
        /// Difficulty to find anything at all.
        dv: DV,
        /// Items / information that can be found on a success.
        finds: Vec<DiscoveryRef>,
    },
}

/// Effect applied as a result of a [`MechanicalHookKind`] outcome.
///
/// Effects are composable via [`HookEffect::Combine`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HookEffect {
    /// Crew earns an extra payment on top of the base Gig payout.
    BonusPay(Eurobucks),
    /// Crew loses part of the negotiated payout.
    PenaltyPay(Eurobucks),
    /// An NPC that was previously hidden is revealed to the crew.
    ///
    /// TODO(WP-605): replace String with NpcTemplateId
    RevealNpc(String),
    /// A location that was previously unknown is revealed to the crew.
    RevealLocation(LocationId),
    /// The crew receives an item of the given kind.
    GrantItem(ItemKind),
    /// The LLM / campaign log receives an intel string (free-form narrative).
    AddIntel(String),
    /// Immediately advance to the named Beat (short-circuit normal flow).
    Transition(BeatId),
    /// Apply multiple effects simultaneously.
    Combine(Vec<HookEffect>),
}

/// A reference to something discoverable during a [`MechanicalHookKind::Search`].
///
/// The string is a content slug (an item slug, an intel string key, or a map
/// marker slug — resolved by WP-613).
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryRef(
    /// Content slug or prose description of the discoverable.
    pub String,
);

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use cpr_rules::types::Eurobucks;
    use cpr_rules::world::LocationId;

    fn sample_gig() -> Gig {
        let hook_id = BeatId::from("beat-hook");
        let dev_id = BeatId::from("beat-dev");
        let climax_id = BeatId::from("beat-climax");
        let resolution_id = BeatId::from("beat-resolution");
        let loc_id = LocationId("ncpd_hq".to_string());
        let hook_id2 = MechanicalHookId::from("hook-awareness");
        let encounter_id = EncounterId::from("lobby_fight");

        let hook_beat = Beat {
            id: hook_id.clone(),
            kind: BeatKind::Hook,
            location: loc_id.clone(),
            present: vec!["corpo_fixer".to_string()],
            intent: "Crew is hired via a tense meet in the Atlantis bar.".to_string(),
            mechanical_hooks: vec![MechanicalHook {
                id: hook_id2.clone(),
                kind: MechanicalHookKind::SkillCheck {
                    stat: Stat::Cool,
                    skill: SkillId::Perception,
                    dv: DV(13),
                    on_success: HookEffect::AddIntel(
                        "You notice the fixer has a Militech tattoo.".to_string(),
                    ),
                    on_failure: HookEffect::Combine(vec![]),
                },
            }],
            encounter: None,
            transitions: vec![Transition {
                condition: TransitionCondition::Always,
                target: dev_id.clone(),
            }],
        };

        let dev_beat = Beat {
            id: dev_id.clone(),
            kind: BeatKind::Development,
            location: loc_id.clone(),
            present: vec!["netrunner_contact".to_string()],
            intent: "Crew meets their contact who provides a data chip.".to_string(),
            mechanical_hooks: vec![],
            encounter: None,
            transitions: vec![
                Transition {
                    condition: TransitionCondition::HookFailed(hook_id2),
                    target: climax_id.clone(),
                },
                Transition {
                    condition: TransitionCondition::Always,
                    target: climax_id.clone(),
                },
            ],
        };

        let climax_beat = Beat {
            id: climax_id.clone(),
            kind: BeatKind::Climax,
            location: LocationId("arasaka_lobby".to_string()),
            present: vec!["corp_guard_lt".to_string(), "mook_01".to_string()],
            intent: "Final battle in the Arasaka lobby.".to_string(),
            mechanical_hooks: vec![MechanicalHook {
                id: MechanicalHookId::from("search-cache"),
                kind: MechanicalHookKind::Search {
                    dv: DV(15),
                    finds: vec![DiscoveryRef("weapons_cache_alpha".to_string())],
                },
            }],
            encounter: Some(EncounterRef(encounter_id)),
            transitions: vec![Transition {
                condition: TransitionCondition::EncounterResolvedLoud,
                target: resolution_id.clone(),
            }],
        };

        let resolution_beat = Beat {
            id: resolution_id.clone(),
            kind: BeatKind::Resolution,
            location: LocationId("arasaka_lobby".to_string()),
            present: vec![],
            intent: "Crew escapes with the data; Fixer pays up.".to_string(),
            mechanical_hooks: vec![],
            encounter: None,
            transitions: vec![],
        };

        let mut npcs = HashMap::new();
        npcs.insert(
            "corpo_fixer".to_string(),
            NpcRef {
                template: "fixer_johnny_midnite".to_string(),
            },
        );
        npcs.insert(
            "corp_guard_lt".to_string(),
            NpcRef {
                template: "arasaka_guard_lt".to_string(),
            },
        );

        let mut locations = HashMap::new();
        locations.insert(
            loc_id,
            LocationRef {
                map: None,
                description: "A neon-lit bar in Night City's combat zone.".to_string(),
            },
        );
        locations.insert(
            LocationId("arasaka_lobby".to_string()),
            LocationRef {
                map: Some("arasaka_lobby_grid".to_string()),
                description: "Corporate tower lobby; guards on every floor.".to_string(),
            },
        );

        Gig {
            id: GigId::from("hot_property"),
            title: "Hot Property".to_string(),
            fixer: "johnny_midnite".to_string(),
            payment: PaymentTier::Costly(Eurobucks(1_500)),
            setting: "Corporate espionage in Night City.".to_string(),
            scope_hours: 4,
            npcs,
            locations,
            beats: vec![hook_beat, dev_beat, climax_beat, resolution_beat],
            start_beat: hook_id,
        }
    }

    /// A sample `Gig` must serialise to RON and deserialise back to an identical value.
    #[test]
    fn test_beat_chart_round_trips() {
        let gig = sample_gig();
        let serialized = ron::to_string(&gig).expect("RON serialisation failed");
        let back: Gig = ron::from_str(&serialized).expect("RON deserialisation failed");
        assert_eq!(gig, back);
    }

    /// All five `BeatKind` variants must be exhaustively matchable.
    #[test]
    fn test_beat_kinds_complete() {
        let kinds = [
            BeatKind::Hook,
            BeatKind::Development,
            BeatKind::Cliffhanger,
            BeatKind::Climax,
            BeatKind::Resolution,
        ];
        for kind in &kinds {
            let _label = match kind {
                BeatKind::Hook => "Hook",
                BeatKind::Development => "Development",
                BeatKind::Cliffhanger => "Cliffhanger",
                BeatKind::Climax => "Climax",
                BeatKind::Resolution => "Resolution",
            };
        }
        assert_eq!(kinds.len(), 5);
    }

    /// Every `TransitionCondition` variant must round-trip through RON.
    #[test]
    fn test_transition_conditions_serializable() {
        let hook_id = MechanicalHookId::from("hook-x");
        let beat_id = BeatId::from("next-beat");

        let variants: Vec<TransitionCondition> = vec![
            TransitionCondition::Always,
            TransitionCondition::EncounterResolvedSilently,
            TransitionCondition::EncounterResolvedLoud,
            TransitionCondition::AlarmRaised,
            TransitionCondition::HookSucceeded(hook_id.clone()),
            TransitionCondition::HookFailed(hook_id),
            TransitionCondition::PromiseBroken("contact_promise".to_string()),
            TransitionCondition::PromiseKept("fixer_deal".to_string()),
            TransitionCondition::NpcKilled("corp_guard_lt".to_string()),
            TransitionCondition::NpcAlly("turncoat_guard".to_string()),
            TransitionCondition::PlayerChoice("Open fire or stand down?".to_string()),
        ];

        for variant in &variants {
            let s = ron::to_string(variant)
                .unwrap_or_else(|e| panic!("Serialize failed for {variant:?}: {e}"));
            let back: TransitionCondition = ron::from_str(&s)
                .unwrap_or_else(|e| panic!("Deserialize failed for {variant:?}: {e}"));
            assert_eq!(variant, &back);
        }

        // Verify via a Transition struct too
        let t = Transition {
            condition: TransitionCondition::PlayerChoice("Go loud or stealth?".to_string()),
            target: beat_id,
        };
        let s = ron::to_string(&t).expect("Transition serialize");
        let back: Transition = ron::from_str(&s).expect("Transition deserialize");
        assert_eq!(t, back);
    }
}
