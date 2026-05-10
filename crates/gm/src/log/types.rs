//! Structured campaign log types for a Cyberpunk RED solo campaign.
//!
//! A [`CampaignLog`] records every significant in-game event in chronological
//! order, maintains a per-NPC relationship model, tracks faction standings,
//! stores completed-gig history, and flags major campaign turning points.
//!
//! The log is designed to be serialised/deserialised via RON and persisted by
//! the `persistence` crate. It also serves as the source material for the
//! digest generator (WP-608) that feeds narrative context to the LLM.
//!
//! ## Design note — `GameClockSnapshot`
//!
//! The spec references `GameClockSnapshot` without defining it. We define it
//! here as a `u64` of **total minutes elapsed since campaign start**. This
//! choice means:
//!
//! - Monotonically increasing — events can be ordered and compared with `<` /
//!   `>` without needing to decompose into days + minutes-into-day.
//! - Derived simply from [`cpr_rules::world::GameClock`]:
//!   `(day as u64 - 1) * 1440 + minutes_into_day as u64`.
//! - Not wall-clock time — it is in-fiction game time, so it is fully
//!   deterministic and replayable from the campaign seed.
//!
//! The alternative (storing `GameClock` directly) would work but requires a
//! compound comparison for ordering. The monotonic-minutes approach makes
//! sorting and range queries on the log trivial.
//!
//! ## `NpcId` → `NpcTemplateId` rename
//!
//! The WP-607 plan uses `NpcId` throughout. As documented in the WP-605 PR,
//! `cpr_rules::types::NpcId` is already a UUID for live runtime instances.
//! All occurrences of `NpcId` in the plan's API have been replaced with
//! [`NpcTemplateId`] (the slug-based template identifier) to avoid a type
//! collision. This is the same rename applied in WP-605.

#![forbid(unsafe_code)]

use crate::ids::{BeatId, FactionId, GigId};
use crate::npc::entity::NpcTemplateId;
use cpr_rules::character::data::ItemKind;
use cpr_rules::effects::CyberwareId;
use cpr_rules::netrunning::architecture::NetArchId;
use cpr_rules::types::{CharacterId, EntityId, Eurobucks};
use cpr_rules::world::LocationId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// GameClockSnapshot
// ---------------------------------------------------------------------------

/// A point in in-fiction campaign time, expressed as **total minutes elapsed
/// since the campaign started** (i.e., since day 1, 00:00).
///
/// ## Derivation from `GameClock`
///
/// Given a [`cpr_rules::world::GameClock`] `c`:
/// ```text
/// GameClockSnapshot((c.day as u64 - 1) * 1440 + c.minutes_into_day as u64)
/// ```
///
/// ## Why minutes-since-start?
///
/// A monotonic `u64` gives easy ordering (`<`, `>`, `==`) and range queries on
/// the event log without decomposing into days + intra-day minutes every time.
/// It is also identical to a timestamp in the rules engine's deterministic
/// time model — no wall-clock involved, fully replayable.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
pub struct GameClockSnapshot(pub u64);

// ---------------------------------------------------------------------------
// CampaignLog
// ---------------------------------------------------------------------------

/// The full persistent record of a solo campaign.
///
/// Stores a chronological [`Vec`] of [`LogEvent`]s, an NPC relationship map,
/// faction standings, completed gigs, and major turning-point events.
///
/// Faction standing values are clamped to `−10..=+10`; use
/// [`CampaignLog::set_faction_standing`] to update them safely.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CampaignLog {
    /// The player character this log belongs to.
    pub character_id: CharacterId,
    /// Chronological event stream. Never remove entries — indices are used as
    /// stable references by [`NpcRelationship::events`].
    pub events: Vec<LogEvent>,
    /// Per-NPC relationship state, keyed by template slug.
    pub npc_memory: HashMap<NpcTemplateId, NpcRelationship>,
    /// Faction standing values, clamped to `−10..=+10`.
    ///
    /// Use [`CampaignLog::set_faction_standing`] rather than inserting
    /// directly, so the clamp is always applied.
    pub faction_standing: HashMap<FactionId, i8>,
    /// History of every gig the player has finished.
    pub completed_gigs: Vec<CompletedGig>,
    /// Significant campaign turning points, surfaced for LLM context.
    pub major_events: Vec<MajorEvent>,
}

impl CampaignLog {
    /// Create an empty log for the given player character.
    pub fn new(character_id: CharacterId) -> Self {
        Self {
            character_id,
            events: Vec::new(),
            npc_memory: HashMap::new(),
            faction_standing: HashMap::new(),
            completed_gigs: Vec::new(),
            major_events: Vec::new(),
        }
    }

    /// Append an event to the log.
    ///
    /// The event index assigned is `self.events.len() - 1` after the push.
    /// Callers that also update [`NpcRelationship::events`] must push via
    /// this method first, then record the resulting index.
    pub fn record(&mut self, at: GameClockSnapshot, kind: LogEventKind) {
        self.events.push(LogEvent { at, kind });
    }

    /// Set a faction's standing, **clamping** the value to `−10..=+10`.
    ///
    /// Values outside the range are silently clamped:
    /// - `> +10` → `+10`
    /// - `< −10` → `−10`
    pub fn set_faction_standing(&mut self, faction: FactionId, value: i8) {
        self.faction_standing.insert(faction, value.clamp(-10, 10));
    }
}

// ---------------------------------------------------------------------------
// LogEvent
// ---------------------------------------------------------------------------

/// A single timestamped entry in the campaign log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LogEvent {
    /// In-fiction time at which this event occurred.
    pub at: GameClockSnapshot,
    /// What happened.
    pub kind: LogEventKind,
}

// ---------------------------------------------------------------------------
// LogEventKind
// ---------------------------------------------------------------------------

/// The full vocabulary of recordable campaign events.
///
/// Each variant corresponds to a distinct in-game occurrence. The LLM digest
/// (WP-608) summarises these variants into narrative context snippets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum LogEventKind {
    /// A new gig began, contracted by `fixer`.
    GigStarted {
        /// The gig that was started.
        gig: GigId,
        /// The fixer NPC who issued the contract.
        fixer: NpcTemplateId,
    },
    /// A gig ended with a specific outcome.
    GigCompleted {
        /// The gig that completed.
        gig: GigId,
        /// How the gig resolved.
        outcome: GigOutcome,
        /// Eurobucks paid to the player on completion.
        payment: Eurobucks,
        /// Improvement Points awarded at the end of this gig.
        ip_awarded: u32,
    },
    /// The campaign advanced into a new Beat.
    BeatEntered {
        /// The Beat the player entered.
        beat: BeatId,
    },
    /// The player met an NPC for the first time (or again).
    NpcMet {
        /// The NPC encountered.
        npc: NpcTemplateId,
        /// The Beat in which the meeting happened.
        beat: BeatId,
        /// The player's initial read of the NPC.
        impression: NpcImpression,
    },
    /// An NPC was killed during the campaign.
    NpcKilled {
        /// The NPC who died.
        npc: NpcTemplateId,
        /// Whether the player character was responsible.
        by_player: bool,
        /// Other NPCs who witnessed the killing.
        witnesses: Vec<NpcTemplateId>,
    },
    /// The player made a promise to an NPC.
    PromiseMade {
        /// The NPC to whom the promise was made.
        to: NpcTemplateId,
        /// A brief description of the promise.
        promise: String,
        /// Optional deadline (in-fiction time). `None` if open-ended.
        due: Option<GameClockSnapshot>,
    },
    /// The player broke a previously made promise.
    PromiseBroken {
        /// The NPC to whom the promise was made.
        to: NpcTemplateId,
        /// The promise that was broken (matching the `PromiseMade` entry).
        promise: String,
    },
    /// The player fulfilled a previously made promise.
    PromiseKept {
        /// The NPC to whom the promise was made.
        to: NpcTemplateId,
        /// The promise that was kept (matching the `PromiseMade` entry).
        promise: String,
    },
    /// The player's Humanity changed.
    HumanityLossEvent {
        /// Change in Humanity points (negative = loss, positive = gain from therapy).
        delta: i16,
        /// What caused the Humanity change.
        source: HumanitySource,
    },
    /// The player had cyberware installed.
    CyberwareInstalled {
        /// Slug of the cyberware item installed.
        id: CyberwareId,
        /// The ripperdoc NPC who performed the installation.
        ripperdoc: NpcTemplateId,
        /// Humanity Loss paid at installation.
        hl_paid: u8,
    },
    /// A combat encounter concluded.
    CombatResolved {
        /// All entities that participated in the combat.
        participants: Vec<EntityId>,
        /// Brief prose summary of what happened (for LLM context).
        summary: String,
        /// Which side came out on top.
        side_won: Side,
    },
    /// A Netrun through a specific architecture concluded.
    NetrunCompleted {
        /// The architecture that was run.
        architecture: NetArchId,
        /// Names / slugs of data files the netrunner extracted.
        files_extracted: Vec<String>,
        /// Number of viruses that remained active when the netrunner jacked out.
        viruses_left: u8,
    },
    /// The player bought items from a vendor NPC.
    Shopped {
        /// The vendor NPC.
        vendor: NpcTemplateId,
        /// Items purchased and their prices.
        items: Vec<(ItemKind, Eurobucks)>,
    },
    /// The player visited a location.
    LocationVisited {
        /// The location that was visited.
        location: LocationId,
        /// Whether this was the player's first time at this location.
        first_time: bool,
    },
    /// A GM-authored or mod-supplied event that does not fit a standard variant.
    Custom {
        /// Short machine-readable tag identifying the event type.
        tag: String,
        /// JSON or RON payload carrying event-specific data.
        payload: String,
    },
}

// ---------------------------------------------------------------------------
// GigOutcome
// ---------------------------------------------------------------------------

/// How a gig was resolved.
///
/// Corresponds to the three-tier outcome structure used in Beat Chart
/// resolution (see rulebook pp.395–408).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum GigOutcome {
    /// Gig completed fully; objectives met; full payment received.
    Success,
    /// Gig partially completed; some objectives failed; reduced payment.
    PartialSuccess,
    /// Gig failed; primary objectives unmet; no (or penalty) payment.
    Failure,
}

// ---------------------------------------------------------------------------
// NpcImpression
// ---------------------------------------------------------------------------

/// The player's initial impression of an NPC on first meeting.
///
/// This is a gut-read captured at `NpcMet` time; the tracked
/// [`NpcRelationship::disposition`] may diverge as events unfold.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum NpcImpression {
    /// NPC presents as warm, cooperative, or allied.
    Friendly,
    /// NPC is professional and transactional — no strong signal either way.
    Neutral,
    /// NPC is guarded, suspicious, or tense around the player.
    Wary,
    /// NPC is openly antagonistic or threatening.
    Hostile,
}

// ---------------------------------------------------------------------------
// HumanitySource
// ---------------------------------------------------------------------------

/// What caused a Humanity change recorded by [`LogEventKind::HumanityLossEvent`].
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HumanitySource {
    /// Humanity Loss from a cyberware installation (rulebook pp.228–229).
    CyberwareInstall(CyberwareId),
    /// Humanity Loss from witnessing or committing a traumatic act.
    TraumaticEvent(String),
    /// Humanity *gain* from therapy (rulebook pp.243–244).
    TherapyGain,
}

// ---------------------------------------------------------------------------
// Side
// ---------------------------------------------------------------------------

/// Which side won a combat encounter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum Side {
    /// The player character's side prevailed.
    Player,
    /// The opposing/enemy side prevailed.
    Adversary,
    /// The encounter ended without a clear victor (disengagement, stalemate).
    Neutral,
}

// ---------------------------------------------------------------------------
// NpcRelationship
// ---------------------------------------------------------------------------

/// The campaign-long relationship model between the player and one NPC.
///
/// `disposition` is the current attitude of the NPC toward the player on a
/// `−10..=+10` scale; `events` is a list of indices into
/// [`CampaignLog::events`] that are relevant to this NPC; `knows_about`
/// records what the NPC has learned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpcRelationship {
    /// NPC's current attitude: −10 (hates the player) to +10 (devoted ally).
    pub disposition: i8,
    /// Indices into [`CampaignLog::events`] that involve this NPC.
    ///
    /// Every index in this list must be `< CampaignLog::events.len()`.
    /// Callers are responsible for inserting the event via
    /// [`CampaignLog::record`] before adding the resulting index here.
    pub events: Vec<usize>,
    /// Facts this NPC knows that affect their behaviour.
    pub knows_about: Vec<KnowledgeFlag>,
}

// ---------------------------------------------------------------------------
// KnowledgeFlag
// ---------------------------------------------------------------------------

/// A discrete fact that an NPC has learned about the player.
///
/// Used by the LLM digest (WP-608) and the dialogue system to adapt NPC
/// behaviour based on past player actions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum KnowledgeFlag {
    /// NPC knows the player killed another specific NPC.
    KnowsPlayerKilled(NpcTemplateId),
    /// NPC knows the player lied about a specific topic.
    KnowsPlayerLies(String),
    /// The player owes this NPC a favor.
    OwedFavor,
    /// This NPC owes the player a favor.
    OwesFavor,
    /// The player is financially indebted to this NPC.
    InDebt(Eurobucks),
}

// ---------------------------------------------------------------------------
// CompletedGig
// ---------------------------------------------------------------------------

/// A record of a gig the player has finished.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompletedGig {
    /// Which gig this was.
    pub gig: GigId,
    /// When the gig ended (in-fiction time, minutes since campaign start).
    pub completed_at: GameClockSnapshot,
    /// How the gig resolved.
    pub outcome: GigOutcome,
}

// ---------------------------------------------------------------------------
// MajorEvent
// ---------------------------------------------------------------------------

/// A significant campaign milestone that the LLM and UI surfaces prominently.
///
/// Major events are separate from the event stream so they can be quickly
/// retrieved for recaps without scanning all log entries.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MajorEvent {
    /// When the event occurred (in-fiction time, minutes since campaign start).
    pub at: GameClockSnapshot,
    /// Brief headline shown in recaps and the UI timeline.
    pub headline: String,
    /// Structured category for the event.
    pub kind: MajorEventKind,
}

// ---------------------------------------------------------------------------
// MajorEventKind
// ---------------------------------------------------------------------------

/// The category of a [`MajorEvent`].
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum MajorEventKind {
    /// The player character died (and was later revived, or the session ended).
    CharacterDied,
    /// The player character was brought back from death / stabilised.
    CharacterRevived,
    /// The fixer who contracted the player betrayed them.
    BetrayedByFixer,
    /// The player gained a new family member or found a long-lost relative.
    GainedFamily,
    /// The player lost a family member (death, estrangement, etc.).
    LostFamily,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cpr_rules::types::CharacterId;
    use uuid::Uuid;

    fn test_character_id() -> CharacterId {
        CharacterId(Uuid::from_u128(0x607_0000_0000_0000))
    }

    fn test_faction() -> FactionId {
        FactionId::from("maelstrom")
    }

    /// `test_log_round_trips_ron` — construct a [`CampaignLog`] with several
    /// events spanning multiple [`LogEventKind`] variants and verify that
    /// RON serialisation → deserialisation produces an equal value.
    #[test]
    fn test_log_round_trips_ron() {
        use cpr_rules::effects::CyberwareId;
        use cpr_rules::netrunning::architecture::NetArchId;
        use cpr_rules::types::EntityId;
        use cpr_rules::world::LocationId;

        let char_id = test_character_id();
        let mut log = CampaignLog::new(char_id);

        let gig_id = GigId::from("hot_property");
        let fixer_id = NpcTemplateId::from("padre");
        let beat_id = BeatId::from("beat-hook");
        let npc_id = NpcTemplateId::from("corp_guard_lt");
        let faction = test_faction();

        // GigStarted
        log.record(
            GameClockSnapshot(0),
            LogEventKind::GigStarted {
                gig: gig_id.clone(),
                fixer: fixer_id.clone(),
            },
        );

        // LocationVisited
        log.record(
            GameClockSnapshot(5),
            LogEventKind::LocationVisited {
                location: LocationId("arasaka_lobby".to_string()),
                first_time: true,
            },
        );

        // NpcMet
        log.record(
            GameClockSnapshot(10),
            LogEventKind::NpcMet {
                npc: npc_id.clone(),
                beat: beat_id.clone(),
                impression: NpcImpression::Wary,
            },
        );

        // CyberwareInstalled
        log.record(
            GameClockSnapshot(20),
            LogEventKind::CyberwareInstalled {
                id: CyberwareId("neural_link".to_string()),
                ripperdoc: NpcTemplateId::from("doc_reza"),
                hl_paid: 2,
            },
        );

        // HumanityLossEvent
        log.record(
            GameClockSnapshot(20),
            LogEventKind::HumanityLossEvent {
                delta: -4,
                source: HumanitySource::CyberwareInstall(CyberwareId("neural_link".to_string())),
            },
        );

        // CombatResolved
        log.record(
            GameClockSnapshot(45),
            LogEventKind::CombatResolved {
                participants: vec![EntityId(Uuid::from_u128(0xABCD))],
                summary: "Lobby brawl — guards taken down without witnesses.".to_string(),
                side_won: Side::Player,
            },
        );

        // NetrunCompleted
        log.record(
            GameClockSnapshot(60),
            LogEventKind::NetrunCompleted {
                architecture: NetArchId("arch-corp01".to_string()),
                files_extracted: vec!["NCPD_blackmail_00.dat".to_string()],
                viruses_left: 1,
            },
        );

        // PromiseMade
        log.record(
            GameClockSnapshot(80),
            LogEventKind::PromiseMade {
                to: fixer_id.clone(),
                promise: "Return the data chip intact".to_string(),
                due: Some(GameClockSnapshot(120)),
            },
        );

        // GigCompleted
        log.record(
            GameClockSnapshot(120),
            LogEventKind::GigCompleted {
                gig: gig_id.clone(),
                outcome: GigOutcome::Success,
                payment: Eurobucks(1_500),
                ip_awarded: 20,
            },
        );

        // Custom
        log.record(
            GameClockSnapshot(125),
            LogEventKind::Custom {
                tag: "mood".to_string(),
                payload: r#"{"value": "melancholic"}"#.to_string(),
            },
        );

        // Add NPC relationship
        let rel = NpcRelationship {
            disposition: 3,
            events: vec![2, 5],
            knows_about: vec![KnowledgeFlag::OwedFavor],
        };
        log.npc_memory.insert(npc_id.clone(), rel);

        // Add faction standing
        log.set_faction_standing(faction.clone(), 5);

        // Add completed gig
        log.completed_gigs.push(CompletedGig {
            gig: gig_id.clone(),
            completed_at: GameClockSnapshot(120),
            outcome: GigOutcome::Success,
        });

        // Add major event
        log.major_events.push(MajorEvent {
            at: GameClockSnapshot(45),
            headline: "Player survived the Arasaka lobby ambush".to_string(),
            kind: MajorEventKind::CharacterRevived,
        });

        // RON round-trip
        let serialized = ron::to_string(&log).expect("RON serialization must succeed");
        let deserialized: CampaignLog =
            ron::from_str(&serialized).expect("RON deserialization must succeed");

        assert_eq!(log, deserialized);
        assert_eq!(deserialized.events.len(), 10);
        assert_eq!(deserialized.character_id, char_id);
        assert_eq!(deserialized.faction_standing.get(&faction), Some(&5i8));
        assert_eq!(deserialized.completed_gigs.len(), 1);
        assert_eq!(deserialized.major_events.len(), 1);
    }

    /// `test_npc_memory_event_indices_valid` — after recording events and
    /// building an [`NpcRelationship`], every index in
    /// `NpcRelationship::events` must be `< log.events.len()`.
    #[test]
    fn test_npc_memory_event_indices_valid() {
        let char_id = test_character_id();
        let mut log = CampaignLog::new(char_id);

        let beat_id = BeatId::from("beat-dev");
        let npc_id = NpcTemplateId::from("fixer_ryo");

        // Record three events
        log.record(
            GameClockSnapshot(0),
            LogEventKind::BeatEntered {
                beat: beat_id.clone(),
            },
        );
        log.record(
            GameClockSnapshot(5),
            LogEventKind::NpcMet {
                npc: npc_id.clone(),
                beat: beat_id.clone(),
                impression: NpcImpression::Friendly,
            },
        );
        log.record(
            GameClockSnapshot(30),
            LogEventKind::PromiseMade {
                to: npc_id.clone(),
                promise: "Bring back the goods by morning".to_string(),
                due: None,
            },
        );

        // NpcRelationship referencing event indices 1 and 2
        let rel = NpcRelationship {
            disposition: 2,
            events: vec![1, 2],
            knows_about: vec![KnowledgeFlag::OwesFavor],
        };
        log.npc_memory.insert(npc_id.clone(), rel);

        // Verify every referenced index is in range
        let event_count = log.events.len();
        for relationship in log.npc_memory.values() {
            for &idx in &relationship.events {
                assert!(
                    idx < event_count,
                    "NpcRelationship event index {idx} is out of bounds (events.len = {event_count})"
                );
            }
        }
    }

    /// `test_faction_standing_clamped` — values outside `−10..=+10` are
    /// clamped on insert.
    #[test]
    fn test_faction_standing_clamped() {
        let char_id = test_character_id();
        let mut log = CampaignLog::new(char_id);
        let faction = test_faction();

        // +20 must clamp to +10
        log.set_faction_standing(faction.clone(), 20);
        assert_eq!(
            *log.faction_standing.get(&faction).unwrap(),
            10i8,
            "standing of +20 must be clamped to +10"
        );

        // −50 must clamp to −10
        log.set_faction_standing(faction.clone(), -50);
        assert_eq!(
            *log.faction_standing.get(&faction).unwrap(),
            -10i8,
            "standing of −50 must be clamped to −10"
        );

        // In-range value must be stored as-is
        log.set_faction_standing(faction.clone(), -7);
        assert_eq!(*log.faction_standing.get(&faction).unwrap(), -7i8);

        log.set_faction_standing(faction.clone(), 8);
        assert_eq!(*log.faction_standing.get(&faction).unwrap(), 8i8);

        // Exact boundary values must not be clamped
        log.set_faction_standing(faction.clone(), 10);
        assert_eq!(*log.faction_standing.get(&faction).unwrap(), 10i8);

        log.set_faction_standing(faction.clone(), -10);
        assert_eq!(*log.faction_standing.get(&faction).unwrap(), -10i8);
    }
}
