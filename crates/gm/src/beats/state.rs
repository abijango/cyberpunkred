//! Beat Chart runtime state machine — WP-604.
//!
//! This module tracks the live state of an active [`Gig`]: which [`Beat`] the
//! player is currently in, the traversal history, which [`MechanicalHook`]s
//! have fired and what their outcomes were, and arbitrary game flags set by the
//! GM layer.
//!
//! ## Determinism
//!
//! The state machine is **fully deterministic given its inputs**: the same
//! sequence of hook outcomes and player choices always produces the same beat
//! path. The LLM cannot move the machine; it can only narrate after a
//! transition has been applied by the engine.
//!
//! ## `GameClockSnapshot` derivation
//!
//! Timestamps in [`BeatTraversal`] are stored as [`GameClockSnapshot`] —
//! total in-fiction minutes since campaign start — derived from
//! [`cpr_rules::world::GameClock`] via:
//!
//! ```text
//! (clock.day as u64 - 1) * 1440 + clock.minutes_into_day as u64
//! ```
//!
//! This is the same derivation documented in `cpr_gm::log::types` (WP-607).
//!
//! ## Flag-key convention
//!
//! String flags use a colon-separated namespace convention:
//!
//! | `TransitionCondition` variant | Flag key |
//! |---|---|
//! | `EncounterResolvedSilently` | `"encounter:silent"` |
//! | `EncounterResolvedLoud`     | `"encounter:loud"` |
//! | `AlarmRaised`               | `"alarm_raised"` |
//! | `PromiseBroken(s)`          | `"promise:{s}:broken"` |
//! | `PromiseKept(s)`            | `"promise:{s}:kept"` |
//! | `NpcKilled(id)`             | `"npc:{id}:killed"` |
//! | `NpcAlly(id)`               | `"npc:{id}:ally"` |
//!
//! All flags are expected to carry a [`FlagValue::Bool(true)`] when set by the
//! engine. Other `FlagValue` variants are available for future extensibility.
//!
//! ## Rulebook references
//!
//! - pp.395–396: Beat Chart structure — Hook, Development/Cliffhanger,
//!   Climax, Resolution. One chart per Gig; deterministic once hooks resolve.
//! - pp.396–408: All Beat sub-types and their narrative roles.

#![forbid(unsafe_code)]

use crate::beats::schema::{Beat, Gig, Transition, TransitionCondition};
use crate::error::GmError;
use crate::ids::{BeatId, GigId, MechanicalHookId};
use crate::log::types::GameClockSnapshot;
use crate::npc::entity::NpcTemplateId;
use cpr_rules::resolution::CheckBreakdown;
use cpr_rules::types::EntityId;
use cpr_rules::world::World;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Public types ─────────────────────────────────────────────────────────────

/// Live runtime state of an active Gig.
///
/// Tracks the current beat, full traversal history, all hook outcomes, and
/// arbitrary flag state. This is the struct the UI / engine queries to
/// determine what the player can do next.
///
/// # Invariants
///
/// - `current_beat` always refers to a beat that exists in the companion [`Gig`].
/// - `history` is non-empty after construction (the start beat is always pushed).
/// - `history.last().beat == current_beat` at all times.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GigState {
    /// Slug of the Gig this state belongs to.
    pub gig: GigId,
    /// Slug of the beat the player is currently on.
    pub current_beat: BeatId,
    /// Full ordered history of every beat entered. Never shrinks; beats are
    /// appended on each successful [`GigState::transition`] call and on
    /// [`GigState::start`].
    pub history: Vec<BeatTraversal>,
    /// Outcome of every [`MechanicalHook`] that has fired in this gig.
    pub hook_outcomes: HashMap<MechanicalHookId, HookOutcome>,
    /// Arbitrary key–value flags set by the GM layer. Used to gate flag-based
    /// [`TransitionCondition`] variants. See the flag-key convention in the
    /// module doc comment.
    pub flags: HashMap<String, FlagValue>,
    /// NPCs instantiated for this gig, keyed by their template slug.
    /// Populated by the NPC instantiation layer (WP-606 / WP-612).
    pub temp_npcs: HashMap<NpcTemplateId, EntityId>,
}

/// A record of one beat entered during a gig.
///
/// Every time the player enters a beat (including the start beat) a
/// `BeatTraversal` is appended to [`GigState::history`]. The `exited_at`
/// field is filled in when the player leaves the beat via
/// [`GigState::transition`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BeatTraversal {
    /// The beat that was entered.
    pub beat: BeatId,
    /// In-fiction time the beat was entered (minutes since campaign start).
    pub entered_at: GameClockSnapshot,
    /// In-fiction time the beat was exited, or `None` if the player is still
    /// in this beat.
    pub exited_at: Option<GameClockSnapshot>,
    /// The condition whose satisfaction caused this beat to be entered, or
    /// `None` for the start beat.
    pub via: Option<TransitionCondition>,
}

/// The resolved outcome of a [`MechanicalHook`] that has fired during this gig.
///
/// Variants correspond to the three possible outcomes: the hook succeeded, it
/// failed, or it was skipped (e.g. the player elected not to attempt it).
/// [`HookOutcome`] is recorded via [`GigState::record_hook`] and consulted by
/// [`GigState::available_transitions`] to evaluate
/// [`TransitionCondition::HookSucceeded`] / [`TransitionCondition::HookFailed`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HookOutcome {
    /// The hook's check was attempted and succeeded.
    Succeeded {
        /// Detailed breakdown of the roll (stat + skill + modifiers + d10).
        breakdown: CheckBreakdown,
    },
    /// The hook's check was attempted and failed.
    Failed {
        /// Detailed breakdown of the failed roll.
        breakdown: CheckBreakdown,
    },
    /// The hook was not attempted (player chose to skip, or it was bypassed).
    Skipped,
}

/// An arbitrary flag value stored in [`GigState::flags`].
///
/// The GM layer uses `Bool(true)` for most event flags (encounter:silent,
/// alarm_raised, npc:{id}:killed, etc.). `Int` and `Str` are provided for
/// future extensibility (e.g. tracking counts or string payloads).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlagValue {
    /// A boolean flag (true / false).
    Bool(bool),
    /// An integer counter or threshold.
    Int(i64),
    /// A string payload (e.g. an NPC slug, a quest note).
    Str(String),
}

// ─── Helper ──────────────────────────────────────────────────────────────────

/// Derive a [`GameClockSnapshot`] from the world's current game clock.
///
/// Formula: `(day - 1) * 1440 + minutes_into_day`, where 1440 = 24 × 60.
/// Day 1, 00:00 → snapshot `0`. Day 2, 01:00 → snapshot `1500`.
fn clock_snapshot(world: &World) -> GameClockSnapshot {
    let day = world.clock.day as u64;
    let minutes = world.clock.minutes_into_day as u64;
    GameClockSnapshot((day.saturating_sub(1)) * 1440 + minutes)
}

// ─── GigState implementation ──────────────────────────────────────────────────

impl GigState {
    /// Start a new gig at its `start_beat`.
    ///
    /// Creates a [`GigState`] with `current_beat` set to `gig.start_beat` and
    /// pushes the initial [`BeatTraversal`] for that beat. The traversal has
    /// `via: None` (no condition caused the first entry) and
    /// `exited_at: None` (the player is still in the start beat).
    ///
    /// # Panics
    ///
    /// Does **not** panic on its own — `validate_gig` (WP-603) should be
    /// called before starting a gig to guarantee structural soundness. If the
    /// gig is invalid (e.g. `start_beat` doesn't resolve), later calls such as
    /// [`GigState::current_beat`] will panic.
    pub fn start(gig: &Gig, world: &mut World) -> Self {
        let now = clock_snapshot(world);
        let start = gig.start_beat.clone();
        let traversal = BeatTraversal {
            beat: start.clone(),
            entered_at: now,
            exited_at: None,
            via: None,
        };
        GigState {
            gig: gig.id.clone(),
            current_beat: start,
            history: vec![traversal],
            hook_outcomes: HashMap::new(),
            flags: HashMap::new(),
            temp_npcs: HashMap::new(),
        }
    }

    /// Return a reference to the current [`Beat`] in this gig.
    ///
    /// # Panics
    ///
    /// Panics if `current_beat` doesn't resolve to any beat in `gig`. This
    /// should never happen when `validate_gig` was called before starting.
    pub fn current_beat<'g>(&self, gig: &'g Gig) -> &'g Beat {
        gig.beats
            .iter()
            .find(|b| b.id == self.current_beat)
            .unwrap_or_else(|| {
                panic!(
                    "current beat '{}' not found in gig '{}' — was validate_gig called?",
                    self.current_beat, self.gig
                )
            })
    }

    /// Return the transitions whose conditions are currently satisfied.
    ///
    /// Evaluates each [`Transition`] in the current beat against the live
    /// `hook_outcomes` and `flags`. Returns all transitions whose conditions
    /// are met. The caller should invoke [`GigState::transition`] with one of
    /// the returned conditions.
    ///
    /// [`TransitionCondition::PlayerChoice`] is **always** returned — the UI
    /// surfaces the choice label; the engine does not auto-fire these.
    ///
    /// # Flag-key convention
    ///
    /// See the module-level doc comment for the full key table. All flag checks
    /// look for `FlagValue::Bool(true)` unless otherwise noted.
    pub fn available_transitions<'g>(&self, gig: &'g Gig) -> Vec<&'g Transition> {
        let beat = self.current_beat(gig);
        beat.transitions
            .iter()
            .filter(|t| self.condition_satisfied(&t.condition))
            .collect()
    }

    /// Apply a transition by its condition.
    ///
    /// Validates that `condition` is in [`GigState::available_transitions`].
    /// If satisfied:
    ///
    /// 1. Marks the current beat's traversal `exited_at` with the current
    ///    clock snapshot.
    /// 2. Pushes a new [`BeatTraversal`] for the target beat with
    ///    `entered_at = now` and `via = Some(condition)`.
    /// 3. Updates `current_beat` to the transition target.
    ///
    /// # Errors
    ///
    /// Returns [`GmError::InvalidTransition`] if `condition` is not currently
    /// satisfied (i.e. not in `available_transitions`).
    pub fn transition(
        &mut self,
        condition: TransitionCondition,
        gig: &Gig,
        world: &mut World,
    ) -> Result<(), GmError> {
        // Find the matching available transition.
        let beat = self.current_beat(gig);
        let target = beat
            .transitions
            .iter()
            .find(|t| t.condition == condition && self.condition_satisfied(&t.condition))
            .map(|t| t.target.clone());

        let target = target.ok_or_else(|| GmError::InvalidTransition {
            gig: self.gig.clone(),
            from: self.current_beat.clone(),
        })?;

        let now = clock_snapshot(world);

        // Mark the current traversal as exited.
        if let Some(last) = self.history.last_mut() {
            last.exited_at = Some(now);
        }

        // Advance to the target beat.
        self.current_beat = target.clone();
        self.history.push(BeatTraversal {
            beat: target,
            entered_at: now,
            exited_at: None,
            via: Some(condition),
        });

        Ok(())
    }

    /// Record the outcome of a mechanical hook.
    ///
    /// Called by the UI / engine when a hook fires (e.g. a skill check
    /// resolves). The outcome is stored in [`GigState::hook_outcomes`] and
    /// consulted by [`GigState::available_transitions`] to evaluate
    /// [`TransitionCondition::HookSucceeded`] and
    /// [`TransitionCondition::HookFailed`].
    pub fn record_hook(&mut self, id: MechanicalHookId, outcome: HookOutcome) {
        self.hook_outcomes.insert(id, outcome);
    }

    /// Set an arbitrary game flag.
    ///
    /// Used by the hook-effect application layer (WP-613) to record events
    /// such as encounter outcomes, promise keeping/breaking, NPC state
    /// changes, and alarm triggers. See the flag-key convention in the module
    /// doc comment.
    pub fn set_flag(&mut self, key: String, value: FlagValue) {
        self.flags.insert(key, value);
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    /// Evaluate whether a single [`TransitionCondition`] is currently satisfied
    /// given the live hook outcomes and flags.
    fn condition_satisfied(&self, condition: &TransitionCondition) -> bool {
        match condition {
            TransitionCondition::Always => true,

            TransitionCondition::PlayerChoice(_) => {
                // Always listed; the caller (UI) picks one. Engine does not
                // auto-fire player choices.
                true
            }

            TransitionCondition::HookSucceeded(id) => {
                matches!(
                    self.hook_outcomes.get(id),
                    Some(HookOutcome::Succeeded { .. })
                )
            }

            TransitionCondition::HookFailed(id) => {
                matches!(self.hook_outcomes.get(id), Some(HookOutcome::Failed { .. }))
            }

            TransitionCondition::EncounterResolvedSilently => self.flag_is_true("encounter:silent"),

            TransitionCondition::EncounterResolvedLoud => self.flag_is_true("encounter:loud"),

            TransitionCondition::AlarmRaised => self.flag_is_true("alarm_raised"),

            TransitionCondition::PromiseBroken(s) => {
                self.flag_is_true(&format!("promise:{s}:broken"))
            }

            TransitionCondition::PromiseKept(s) => self.flag_is_true(&format!("promise:{s}:kept")),

            TransitionCondition::NpcKilled(id) => self.flag_is_true(&format!("npc:{id}:killed")),

            TransitionCondition::NpcAlly(id) => self.flag_is_true(&format!("npc:{id}:ally")),
        }
    }

    /// Return `true` iff the flag at `key` is set to `FlagValue::Bool(true)`.
    fn flag_is_true(&self, key: &str) -> bool {
        matches!(self.flags.get(key), Some(FlagValue::Bool(true)))
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beats::schema::{
        BeatKind, LocationRef, MechanicalHook, MechanicalHookKind, PaymentTier, TransitionCondition,
    };
    use crate::ids::{BeatId, GigId, MechanicalHookId};
    use cpr_rules::dice::{CritD10, D10Outcome};
    use cpr_rules::types::{Eurobucks, DV};
    use cpr_rules::world::{GameClock, LocationId};
    use std::collections::HashMap;
    use uuid::Uuid;

    // ── Fixture helpers ───────────────────────────────────────────────────────

    fn loc(s: &str) -> LocationId {
        LocationId(s.to_string())
    }

    fn beat_id(s: &str) -> BeatId {
        BeatId::from(s)
    }

    fn gig_id(s: &str) -> GigId {
        GigId::from(s)
    }

    fn hook_id(s: &str) -> MechanicalHookId {
        MechanicalHookId::from(s)
    }

    fn make_transition(condition: TransitionCondition, target: &str) -> Transition {
        Transition {
            condition,
            target: beat_id(target),
        }
    }

    fn minimal_beat(id: &str, kind: BeatKind, transitions: Vec<Transition>) -> Beat {
        Beat {
            id: beat_id(id),
            kind,
            location: loc("loc-a"),
            present: vec![],
            intent: "test".to_string(),
            mechanical_hooks: vec![],
            encounter: None,
            transitions,
        }
    }

    /// Build a minimal but valid Gig: Hook → (HookSucceeded | Always) → Climax → Resolution.
    fn make_test_gig_with_hook() -> Gig {
        let hid = hook_id("skill-check");

        let hook_beat = Beat {
            id: beat_id("hook"),
            kind: BeatKind::Hook,
            location: loc("loc-a"),
            present: vec![],
            intent: "hook intent".to_string(),
            mechanical_hooks: vec![MechanicalHook {
                id: hid.clone(),
                kind: MechanicalHookKind::Search {
                    dv: DV(13),
                    finds: vec![],
                },
            }],
            encounter: None,
            transitions: vec![
                make_transition(
                    TransitionCondition::HookSucceeded(hid.clone()),
                    "dev-success",
                ),
                make_transition(TransitionCondition::HookFailed(hid.clone()), "dev-fail"),
                make_transition(TransitionCondition::Always, "climax"),
            ],
        };

        let beats = vec![
            hook_beat,
            minimal_beat(
                "dev-success",
                BeatKind::Development,
                vec![make_transition(TransitionCondition::Always, "climax")],
            ),
            minimal_beat(
                "dev-fail",
                BeatKind::Development,
                vec![make_transition(TransitionCondition::Always, "climax")],
            ),
            minimal_beat(
                "climax",
                BeatKind::Climax,
                vec![make_transition(TransitionCondition::Always, "resolution")],
            ),
            minimal_beat("resolution", BeatKind::Resolution, vec![]),
        ];

        let mut locations = HashMap::new();
        locations.insert(
            loc("loc-a"),
            LocationRef {
                map: None,
                description: "test location".to_string(),
            },
        );

        Gig {
            id: gig_id("test-gig"),
            title: "Test Gig".to_string(),
            fixer: "test_fixer".to_string(),
            payment: PaymentTier::Cheap(Eurobucks(200)),
            setting: "A test setting.".to_string(),
            scope_hours: 2,
            npcs: HashMap::new(),
            locations,
            beats,
            start_beat: beat_id("hook"),
        }
    }

    /// Construct a minimal [`World`] with a fixed clock state.
    fn make_world(day: u32, minutes_into_day: u16) -> World {
        use cpr_rules::character::Character;
        use cpr_rules::character::{
            Inventory, Lifepath, Role, SkillSet, StatBlock, WornArmor, Wounds,
        };
        use cpr_rules::effects::EffectStack;
        use cpr_rules::types::{CharacterId, Eurobucks};

        let pc = Character {
            id: CharacterId(Uuid::from_u128(0xC0FFEE)),
            name: "Test PC".to_string(),
            handle: None,
            role: Role::Solo,
            role_rank: 4,
            stats: StatBlock {
                int: 5,
                r#ref: 6,
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
            complementary_bonuses: vec![],
        };

        let mut world = World::new(pc);
        world.clock = GameClock {
            day,
            minutes_into_day,
        };
        world
    }

    /// Construct a dummy [`CheckBreakdown`] for use in test outcomes.
    fn dummy_breakdown(success: bool) -> CheckBreakdown {
        let raw = if success { 8u8 } else { 2u8 };
        let d10 = CritD10 {
            base: raw,
            follow_up: None,
            outcome: D10Outcome::Normal,
            net: raw as i16,
        };
        CheckBreakdown::new(4, 3, 0, 0, d10, DV(13))
    }

    // ── Acceptance test 1: test_start_at_hook ────────────────────────────────

    /// Starting a Gig places `current_beat == gig.start_beat` and records one
    /// [`BeatTraversal`] in `history`.
    #[test]
    fn test_start_at_hook() {
        let gig = make_test_gig_with_hook();
        let mut world = make_world(1, 0);

        let state = GigState::start(&gig, &mut world);

        assert_eq!(
            state.current_beat, gig.start_beat,
            "current_beat must equal gig.start_beat after start()"
        );
        assert_eq!(
            state.history.len(),
            1,
            "history must contain exactly one traversal after start()"
        );
        assert_eq!(
            state.history[0].beat, gig.start_beat,
            "the traversal must record the start beat"
        );
        assert!(
            state.history[0].via.is_none(),
            "start beat has no entry condition"
        );
        assert!(
            state.history[0].exited_at.is_none(),
            "start beat has not been exited yet"
        );
    }

    // ── Acceptance test 2: test_transition_always ─────────────────────────────

    /// A transition with `TransitionCondition::Always` is always in
    /// `available_transitions` and `transition` succeeds, updating `current_beat`.
    #[test]
    fn test_transition_always() {
        let gig = make_test_gig_with_hook();
        let mut world = make_world(1, 30);
        let mut state = GigState::start(&gig, &mut world);

        // The hook beat has an Always transition to "climax".
        let available = state.available_transitions(&gig);
        assert!(
            available
                .iter()
                .any(|t| t.condition == TransitionCondition::Always),
            "Always transition must be available"
        );

        // Apply the Always transition.
        let result = state.transition(TransitionCondition::Always, &gig, &mut world);
        assert!(result.is_ok(), "Always transition must succeed: {result:?}");
        assert_eq!(
            state.current_beat,
            beat_id("climax"),
            "current_beat must be 'climax' after Always transition from hook"
        );
    }

    // ── Acceptance test 3: test_transition_hook_succeeded ────────────────────

    /// A transition gated on `HookSucceeded(id)` is NOT available before the
    /// hook fires, IS available after `record_hook(id, Succeeded)`, and is NOT
    /// available after `record_hook(id, Failed)`.
    #[test]
    fn test_transition_hook_succeeded() {
        let gig = make_test_gig_with_hook();
        let hid = hook_id("skill-check");
        let mut world = make_world(1, 0);
        let mut state = GigState::start(&gig, &mut world);

        // Before any hook fires, HookSucceeded transition must NOT be available.
        let available = state.available_transitions(&gig);
        assert!(
            !available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookSucceeded(hid.clone())),
            "HookSucceeded transition must not be available before hook fires"
        );

        // After recording a Success, the HookSucceeded transition IS available.
        state.record_hook(
            hid.clone(),
            HookOutcome::Succeeded {
                breakdown: dummy_breakdown(true),
            },
        );
        let available = state.available_transitions(&gig);
        assert!(
            available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookSucceeded(hid.clone())),
            "HookSucceeded transition must be available after recording Succeeded"
        );
        assert!(
            !available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookFailed(hid.clone())),
            "HookFailed transition must NOT be available after recording Succeeded"
        );

        // Overwrite with Failed — now HookSucceeded is unavailable again.
        state.record_hook(
            hid.clone(),
            HookOutcome::Failed {
                breakdown: dummy_breakdown(false),
            },
        );
        let available = state.available_transitions(&gig);
        assert!(
            !available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookSucceeded(hid.clone())),
            "HookSucceeded transition must NOT be available after recording Failed"
        );
        assert!(
            available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookFailed(hid.clone())),
            "HookFailed transition must be available after recording Failed"
        );
    }

    // ── Acceptance test 4: test_orphan_transition_unavailable ────────────────

    /// A transition whose condition isn't met is not in `available_transitions`.
    /// Calling `transition` with an unmet condition returns `Err(InvalidTransition)`.
    #[test]
    fn test_orphan_transition_unavailable() {
        let gig = make_test_gig_with_hook();
        let hid = hook_id("skill-check");
        let mut world = make_world(1, 0);
        let mut state = GigState::start(&gig, &mut world);

        // HookSucceeded is not met (no hook recorded yet).
        let available = state.available_transitions(&gig);
        assert!(
            !available
                .iter()
                .any(|t| t.condition == TransitionCondition::HookSucceeded(hid.clone())),
            "HookSucceeded should not be available"
        );

        // Attempting to apply the unmet condition must return Err.
        let result = state.transition(
            TransitionCondition::HookSucceeded(hid.clone()),
            &gig,
            &mut world,
        );
        assert!(
            matches!(result, Err(GmError::InvalidTransition { .. })),
            "transition with unmet condition must return InvalidTransition, got: {result:?}"
        );
    }

    // ── Acceptance test 5: test_history_records_traversal ────────────────────

    /// Every `transition` call appends a [`BeatTraversal`] with `entered_at`
    /// set; the previous beat's traversal has `exited_at` set.
    #[test]
    fn test_history_records_traversal() {
        let gig = make_test_gig_with_hook();
        let mut world = make_world(1, 0);
        let mut state = GigState::start(&gig, &mut world);

        // Advance the clock so the transition snapshot is distinguishable.
        world.clock.minutes_into_day = 30;

        // Transition: hook → climax (via Always).
        state
            .transition(TransitionCondition::Always, &gig, &mut world)
            .expect("Always transition must succeed");

        assert_eq!(state.history.len(), 2, "history must have 2 traversals");

        // The first traversal (hook) must now have exited_at set.
        let hook_traversal = &state.history[0];
        assert_eq!(hook_traversal.beat, beat_id("hook"));
        assert!(
            hook_traversal.exited_at.is_some(),
            "hook traversal must have exited_at set after transition"
        );

        // The second traversal (climax) must have entered_at set and exited_at = None.
        let climax_traversal = &state.history[1];
        assert_eq!(climax_traversal.beat, beat_id("climax"));
        assert_eq!(
            climax_traversal.via,
            Some(TransitionCondition::Always),
            "climax traversal must record the condition used"
        );
        assert!(
            climax_traversal.exited_at.is_none(),
            "climax traversal must not have exited_at yet"
        );
        assert_eq!(
            state.current_beat,
            beat_id("climax"),
            "current_beat must be 'climax'"
        );

        // Transition again: climax → resolution.
        world.clock.minutes_into_day = 60;
        state
            .transition(TransitionCondition::Always, &gig, &mut world)
            .expect("second Always transition must succeed");

        assert_eq!(state.history.len(), 3, "history must have 3 traversals");
        assert!(
            state.history[1].exited_at.is_some(),
            "climax traversal must have exited_at set after second transition"
        );
        assert_eq!(state.current_beat, beat_id("resolution"));
    }

    // ── Additional: flag-based transitions ───────────────────────────────────

    /// A transition gated on a flag is not available before the flag is set,
    /// and is available after `set_flag` is called with `Bool(true)`.
    #[test]
    fn test_flag_based_transition() {
        // Build a minimal gig with an AlarmRaised transition.
        let beats = vec![
            Beat {
                id: beat_id("hook"),
                kind: BeatKind::Hook,
                location: loc("loc-a"),
                present: vec![],
                intent: "test".to_string(),
                mechanical_hooks: vec![],
                encounter: None,
                transitions: vec![
                    make_transition(TransitionCondition::AlarmRaised, "loud-climax"),
                    make_transition(TransitionCondition::Always, "quiet-climax"),
                ],
            },
            minimal_beat(
                "loud-climax",
                BeatKind::Climax,
                vec![make_transition(TransitionCondition::Always, "resolution")],
            ),
            minimal_beat(
                "quiet-climax",
                BeatKind::Climax,
                vec![make_transition(TransitionCondition::Always, "resolution")],
            ),
            minimal_beat("resolution", BeatKind::Resolution, vec![]),
        ];

        let mut locations = HashMap::new();
        locations.insert(
            loc("loc-a"),
            LocationRef {
                map: None,
                description: "test location".to_string(),
            },
        );

        let gig = Gig {
            id: gig_id("flag-gig"),
            title: "Flag Gig".to_string(),
            fixer: "fixer".to_string(),
            payment: PaymentTier::Cheap(Eurobucks(100)),
            setting: "test".to_string(),
            scope_hours: 1,
            npcs: HashMap::new(),
            locations,
            beats,
            start_beat: beat_id("hook"),
        };

        let mut world = make_world(1, 0);
        let mut state = GigState::start(&gig, &mut world);

        // AlarmRaised not yet available.
        let available = state.available_transitions(&gig);
        assert!(
            !available
                .iter()
                .any(|t| t.condition == TransitionCondition::AlarmRaised),
            "AlarmRaised must not be available before flag is set"
        );

        // Set the flag.
        state.set_flag("alarm_raised".to_string(), FlagValue::Bool(true));

        let available = state.available_transitions(&gig);
        assert!(
            available
                .iter()
                .any(|t| t.condition == TransitionCondition::AlarmRaised),
            "AlarmRaised must be available after flag is set"
        );

        // Transition via AlarmRaised.
        state
            .transition(TransitionCondition::AlarmRaised, &gig, &mut world)
            .expect("AlarmRaised transition must succeed");
        assert_eq!(state.current_beat, beat_id("loud-climax"));
    }

    // ── Additional: GameClockSnapshot derivation ──────────────────────────────

    /// The `clock_snapshot` helper correctly converts `GameClock` to minutes
    /// since campaign start.
    #[test]
    fn test_clock_snapshot_derivation() {
        let mut world = make_world(1, 0);
        // Day 1 / 00:00 → 0 minutes since campaign start.
        assert_eq!(clock_snapshot(&world), GameClockSnapshot(0));

        world.clock = GameClock {
            day: 1,
            minutes_into_day: 60,
        };
        // Day 1 / 01:00 → 60 minutes.
        assert_eq!(clock_snapshot(&world), GameClockSnapshot(60));

        world.clock = GameClock {
            day: 2,
            minutes_into_day: 0,
        };
        // Day 2 / 00:00 → 1440 minutes.
        assert_eq!(clock_snapshot(&world), GameClockSnapshot(1440));

        world.clock = GameClock {
            day: 2,
            minutes_into_day: 90,
        };
        // Day 2 / 01:30 → 1530 minutes.
        assert_eq!(clock_snapshot(&world), GameClockSnapshot(1530));
    }
}
