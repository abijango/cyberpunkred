//! Campaign log digest generator for LLM prompts (WP-608).
//!
//! Takes a [`CampaignLog`] and produces a compact **fact-bullet** summary
//! bounded by a configurable character limit. The LLM receiving this digest
//! is expected to convert the fact-bullets into narrative prose.
//!
//! ## Design goals
//!
//! - **Deterministic:** same log + same request → identical output string.
//!   No `Instant::now()`, no randomness. Time display is derived from the
//!   latest event's `at` timestamp.
//! - **Bounded:** output is truncated at the nearest line boundary when
//!   `max_chars` is exceeded, and a `"..."` suffix is appended.
//! - **Configurable:** callers can select which NPC relationships, factions,
//!   and promise state to include; they can also cap the number of recent
//!   events.
//!
//! ## Output format
//!
//! ```text
//! - On Day 12, killed Garcia (NpcKilled, by player). Padre witnessed this.
//! - Padre (disposition: -3). Knows: you killed Garcia. Owes you a favor.
//! - Open promise to Padre: deliver datachip by Day 14.
//! - Faction: Maelstrom -7 (hostile). Tyger Claws +2 (neutral).
//! - 2 days ago, completed gig "hot_property" (success, ¥1500, 25 IP).
//! ```
//!
//! See `IMPLEMENTATION_PLAN.md` lines 3235–3262.

#![forbid(unsafe_code)]

use crate::log::types::{
    CampaignLog, GameClockSnapshot, GigOutcome, KnowledgeFlag, LogEventKind, NpcRelationship,
};
use crate::npc::entity::NpcTemplateId;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// DigestRequest
// ---------------------------------------------------------------------------

/// Configuration for a [`generate_digest`] call.
///
/// Controls which parts of the campaign log are included in the output and
/// how many characters the result may occupy.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DigestRequest {
    /// How many of the most-recent log events to include in the "Recent"
    /// section. Set to `0` to omit recent events entirely.
    pub recent_event_limit: usize,
    /// NPC template slugs whose relationship state should be included.
    /// Leave empty to omit NPC sections.
    pub include_npcs: Vec<NpcTemplateId>,
    /// When `true`, all faction standings are included.
    pub include_factions: bool,
    /// When `true`, open (unfulfilled) promises are included.
    pub include_open_promises: bool,
    /// Hard upper bound on the output string length (in bytes / UTF-8 chars).
    /// The output is truncated at the nearest preceding newline and `"..."`
    /// is appended when the full digest would exceed this limit. The final
    /// string is always `<= max_chars` in length.
    pub max_chars: usize,
}

// ---------------------------------------------------------------------------
// generate_digest
// ---------------------------------------------------------------------------

/// Generate a compact text digest of the campaign log for an LLM prompt.
///
/// The output is **fact-bullets**, NOT narrative. The LLM converts to
/// narrative. Output is bounded by `req.max_chars` (truncated with an
/// ellipsis `"..."` at the nearest line boundary if needed).
///
/// ## Determinism
///
/// Given the same `log` and `req`, this function always returns an identical
/// `String`. It does not read wall-clock time or any external state.
///
/// ## Time display
///
/// All event timestamps are displayed relative to the latest event's `at`
/// value (the "now" reference point). If the log is empty, the reference is
/// `GameClockSnapshot(0)`.
///
/// - Same day → "On Day N"
/// - 1 day ago → "Yesterday (Day N)"
/// - N days ago → "N days ago (Day N)"
pub fn generate_digest(log: &CampaignLog, req: &DigestRequest) -> String {
    // Reference "now" = the latest event's timestamp (or 0 if log is empty).
    let now = log
        .events
        .last()
        .map(|e| e.at)
        .unwrap_or(GameClockSnapshot(0));

    let mut lines: Vec<String> = Vec::new();

    // -----------------------------------------------------------------------
    // 1. Recent events
    // -----------------------------------------------------------------------
    if req.recent_event_limit > 0 {
        let start = log.events.len().saturating_sub(req.recent_event_limit);
        for event in &log.events[start..] {
            if let Some(line) = format_event_line(&event.kind, event.at, now) {
                lines.push(format!("- {line}"));
            }
        }
    }

    // -----------------------------------------------------------------------
    // 2. NPC memory sections
    // -----------------------------------------------------------------------
    for npc_id in &req.include_npcs {
        if let Some(rel) = log.npc_memory.get(npc_id) {
            let npc_lines = format_npc_section(npc_id, rel, log, now);
            lines.extend(npc_lines);
        }
    }

    // -----------------------------------------------------------------------
    // 3. Open promises
    // -----------------------------------------------------------------------
    if req.include_open_promises {
        let promise_lines = collect_open_promises(log, now);
        lines.extend(promise_lines);
    }

    // -----------------------------------------------------------------------
    // 4. Faction standings
    // -----------------------------------------------------------------------
    if req.include_factions && !log.faction_standing.is_empty() {
        // Sort by faction slug for deterministic output.
        let mut standings: Vec<_> = log.faction_standing.iter().collect();
        standings.sort_by_key(|(fid, _)| fid.as_str());

        let faction_bullets: Vec<String> = standings
            .iter()
            .map(|(fid, &standing)| {
                let label = faction_label(standing);
                format!("{} {} ({label})", fid.as_str(), fmt_standing(standing))
            })
            .collect();

        lines.push(format!("- Factions: {}", faction_bullets.join(". ")));
    }

    // -----------------------------------------------------------------------
    // Assemble and truncate
    // -----------------------------------------------------------------------
    let full = lines.join("\n");
    truncate_to_max_chars(full, req.max_chars)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Format a single [`LogEventKind`] as a human-readable fact string.
///
/// Returns `None` for event kinds that do not need a fact-bullet
/// (e.g., `Custom` events, which may have arbitrary payloads).
fn format_event_line(
    kind: &LogEventKind,
    at: GameClockSnapshot,
    now: GameClockSnapshot,
) -> Option<String> {
    let when = relative_time(at, now);
    let s = match kind {
        LogEventKind::GigStarted { gig, fixer } => {
            format!("{when}, started gig \"{}\" (contracted by {}).", gig, fixer)
        }
        LogEventKind::GigCompleted {
            gig,
            outcome,
            payment,
            ip_awarded,
        } => {
            let outcome_str = match outcome {
                GigOutcome::Success => "success",
                GigOutcome::PartialSuccess => "partial success",
                GigOutcome::Failure => "failure",
            };
            format!(
                "{when}, completed gig \"{}\" ({outcome_str}, \u{00a5}{}, {ip_awarded} IP).",
                gig, payment.0
            )
        }
        LogEventKind::BeatEntered { beat } => {
            format!("{when}, entered beat \"{}\".", beat)
        }
        LogEventKind::NpcMet { npc, beat, .. } => {
            format!("{when}, met {} (in beat \"{}\").", npc, beat)
        }
        LogEventKind::NpcKilled {
            npc,
            by_player,
            witnesses,
        } => {
            let actor = if *by_player { "killed" } else { "NPC death:" };
            if witnesses.is_empty() {
                format!("{when}, {actor} {} (no witnesses).", npc)
            } else {
                let wit_list: Vec<_> = witnesses.iter().map(|w| w.as_str()).collect();
                format!(
                    "{when}, {actor} {}. Witnessed by: {}.",
                    npc,
                    wit_list.join(", ")
                )
            }
        }
        LogEventKind::PromiseMade { to, promise, due } => {
            if let Some(due_time) = due {
                let due_day = snapshot_to_day(*due_time);
                format!("{when}, promised {} \"{promise}\" (due Day {due_day}).", to)
            } else {
                format!("{when}, promised {} \"{promise}\" (no deadline).", to)
            }
        }
        LogEventKind::PromiseBroken { to, promise } => {
            format!("{when}, broke promise to {to}: \"{promise}\".")
        }
        LogEventKind::PromiseKept { to, promise } => {
            format!("{when}, kept promise to {to}: \"{promise}\".")
        }
        LogEventKind::HumanityLossEvent { delta, .. } => {
            if *delta < 0 {
                format!("{when}, humanity loss: {delta}.")
            } else {
                format!("{when}, humanity gain: +{delta}.")
            }
        }
        LogEventKind::CyberwareInstalled {
            id,
            ripperdoc,
            hl_paid,
        } => {
            format!(
                "{when}, installed cyberware \"{}\" at {} (HL -{hl_paid}).",
                id.0, ripperdoc
            )
        }
        LogEventKind::CombatResolved {
            summary, side_won, ..
        } => {
            use crate::log::types::Side;
            let result = match side_won {
                Side::Player => "player won",
                Side::Adversary => "adversary won",
                Side::Neutral => "stalemate",
            };
            format!("{when}, combat ({result}): {summary}")
        }
        LogEventKind::NetrunCompleted {
            architecture,
            files_extracted,
            viruses_left,
        } => {
            format!(
                "{when}, netrun on \"{}\": {} files extracted, {viruses_left} viruses left.",
                architecture.0,
                files_extracted.len()
            )
        }
        LogEventKind::Shopped { vendor, items } => {
            format!("{when}, shopped at {} ({} items).", vendor, items.len())
        }
        LogEventKind::LocationVisited {
            location,
            first_time,
        } => {
            let note = if *first_time { " (first visit)" } else { "" };
            format!("{when}, visited \"{}\"{note}.", location.0)
        }
        LogEventKind::Custom { tag, .. } => {
            // Custom events may have arbitrary payloads; emit minimal info.
            format!("{when}, custom event \"{tag}\".")
        }
    };
    Some(s)
}

/// Format the NPC memory section for one NPC.
///
/// Produces lines of the form:
/// ```text
/// - Padre (disposition: -3). Knows: you killed Garcia. Owes you a favor.
/// ```
/// Followed by any NPC-specific events from their event index list.
fn format_npc_section(
    npc_id: &NpcTemplateId,
    rel: &NpcRelationship,
    log: &CampaignLog,
    now: GameClockSnapshot,
) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    parts.push(format!("{} (disposition: {}).", npc_id, rel.disposition));

    // Knowledge flags
    for flag in &rel.knows_about {
        let flag_str = match flag {
            KnowledgeFlag::KnowsPlayerKilled(target) => {
                format!("Knows: you killed {}.", target)
            }
            KnowledgeFlag::KnowsPlayerLies(topic) => {
                format!("Knows: you lied about \"{topic}\".")
            }
            KnowledgeFlag::OwedFavor => "You owe them a favor.".to_string(),
            KnowledgeFlag::OwesFavor => "Owes you a favor.".to_string(),
            KnowledgeFlag::InDebt(amount) => {
                format!("You are in debt to them: \u{00a5}{}.", amount.0)
            }
        };
        parts.push(flag_str);
    }

    // NPC-specific events from their event index list
    for &idx in &rel.events {
        if let Some(event) = log.events.get(idx) {
            if let Some(line) = format_event_line(&event.kind, event.at, now) {
                parts.push(format!("Event: {line}"));
            }
        }
    }

    vec![format!("- {}", parts.join(" "))]
}

/// Scan all log events for open (unfulfilled) `PromiseMade` entries.
///
/// A promise is open if no `PromiseKept` or `PromiseBroken` entry matches
/// both the same `to` NPC and the same `promise` string.
fn collect_open_promises(log: &CampaignLog, now: GameClockSnapshot) -> Vec<String> {
    use std::collections::HashSet;

    // Build the set of resolved promises: (to, promise).
    let mut resolved: HashSet<(String, String)> = HashSet::new();
    for event in &log.events {
        match &event.kind {
            LogEventKind::PromiseKept { to, promise }
            | LogEventKind::PromiseBroken { to, promise } => {
                resolved.insert((to.as_str().to_string(), promise.clone()));
            }
            _ => {}
        }
    }

    let mut lines = Vec::new();
    for event in &log.events {
        if let LogEventKind::PromiseMade { to, promise, due } = &event.kind {
            let key = (to.as_str().to_string(), promise.clone());
            if !resolved.contains(&key) {
                let line = if let Some(due_time) = due {
                    let due_day = snapshot_to_day(*due_time);
                    format!("- Open promise to {to}: \"{promise}\" (due Day {due_day}).")
                } else {
                    format!("- Open promise to {to}: \"{promise}\".")
                };
                let _ = now; // used for context in some callers but not needed here
                lines.push(line);
            }
        }
    }
    lines
}

/// Convert a `GameClockSnapshot` to an in-fiction day number (1-based).
///
/// Minutes are stored as total minutes since campaign start (day 1, 00:00).
/// Day N begins at `(N-1) * 1440` minutes.
fn snapshot_to_day(t: GameClockSnapshot) -> u64 {
    t.0 / 1440 + 1
}

/// Express a past timestamp relative to `now` as a human-readable string.
///
/// ```text
/// Same day   → "On Day 12"
/// 1 day ago  → "Yesterday (Day 11)"
/// N days ago → "N days ago (Day 8)"
/// ```
fn relative_time(at: GameClockSnapshot, now: GameClockSnapshot) -> String {
    let at_day = snapshot_to_day(at);
    let now_day = snapshot_to_day(now);

    // Guard against future timestamps (shouldn't happen but be safe).
    let days_ago = now_day.saturating_sub(at_day);
    match days_ago {
        0 => format!("On Day {at_day}"),
        1 => format!("Yesterday (Day {at_day})"),
        n => format!("{n} days ago (Day {at_day})"),
    }
}

/// Format a standing value with an explicit `+` sign for positive values.
fn fmt_standing(v: i8) -> String {
    if v >= 0 {
        format!("+{v}")
    } else {
        format!("{v}")
    }
}

/// Return the faction label for a given standing value.
///
/// - `<= -5` → `"hostile"`
/// - `-4..=+4` → `"neutral"`
/// - `>= +5` → `"friendly"`
fn faction_label(v: i8) -> &'static str {
    if v <= -5 {
        "hostile"
    } else if v >= 5 {
        "friendly"
    } else {
        "neutral"
    }
}

/// Truncate `s` to at most `max_chars` characters, cutting at a line boundary
/// and appending `"..."` if truncation occurred.
///
/// The returned string is always `<= max_chars` bytes in length.
fn truncate_to_max_chars(s: String, max_chars: usize) -> String {
    if s.len() <= max_chars {
        return s;
    }

    // "..." is 3 chars; we need to fit within max_chars total.
    let budget = max_chars.saturating_sub(3);

    // Find the last newline within the budget.
    let truncated = &s[..budget.min(s.len())];
    let cut = truncated.rfind('\n').unwrap_or(0);
    if cut == 0 {
        // No newline found — hard-cut at budget.
        let safe_end = s
            .char_indices()
            .take_while(|(i, _)| *i < budget)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &s[..safe_end])
    } else {
        format!("{}...", &s[..cut])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{BeatId, FactionId, GigId};
    use crate::log::types::{
        CampaignLog, GameClockSnapshot, GigOutcome, KnowledgeFlag, LogEventKind, NpcImpression,
        NpcRelationship,
    };
    use crate::npc::entity::NpcTemplateId;
    use cpr_rules::types::{CharacterId, Eurobucks};
    use uuid::Uuid;

    fn char_id() -> CharacterId {
        CharacterId(Uuid::from_u128(0x608_0000_0000_0000))
    }

    fn padre() -> NpcTemplateId {
        NpcTemplateId::from("padre")
    }

    fn beat() -> BeatId {
        BeatId::from("beat-hook")
    }

    // -----------------------------------------------------------------------
    // test_digest_respects_max_chars
    //
    // With max_chars = 100 and a log full of events, the output must be
    // <= 100 bytes and must end with "...".
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_respects_max_chars() {
        let mut log = CampaignLog::new(char_id());
        let fixer = NpcTemplateId::from("rogue");

        // Record many events so the digest would be long without truncation.
        for i in 0_u64..20 {
            log.record(
                GameClockSnapshot(i * 1440),
                LogEventKind::GigStarted {
                    gig: GigId::from(format!("gig_{i}")),
                    fixer: fixer.clone(),
                },
            );
        }
        log.record(
            GameClockSnapshot(20 * 1440),
            LogEventKind::NpcMet {
                npc: padre(),
                beat: beat(),
                impression: NpcImpression::Friendly,
            },
        );

        let req = DigestRequest {
            recent_event_limit: 15,
            include_npcs: vec![],
            include_factions: true,
            include_open_promises: true,
            max_chars: 100,
        };

        let digest = generate_digest(&log, &req);
        assert!(
            digest.len() <= 100,
            "digest length {} exceeds max_chars 100",
            digest.len()
        );
        assert!(
            digest.ends_with("..."),
            "truncated digest must end with '...', got: {digest:?}"
        );
    }

    // -----------------------------------------------------------------------
    // test_digest_includes_npc_memory
    //
    // A log with NpcMet and NpcKilled events involving "padre"; requesting
    // include_npcs: [padre] must produce a digest that mentions Padre and
    // their disposition.
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_includes_npc_memory() {
        let mut log = CampaignLog::new(char_id());

        let garcia = NpcTemplateId::from("garcia");

        // Event 0: meet padre
        log.record(
            GameClockSnapshot(0),
            LogEventKind::NpcMet {
                npc: padre(),
                beat: beat(),
                impression: NpcImpression::Neutral,
            },
        );

        // Event 1: kill garcia, padre witnesses
        log.record(
            GameClockSnapshot(60),
            LogEventKind::NpcKilled {
                npc: garcia.clone(),
                by_player: true,
                witnesses: vec![padre()],
            },
        );

        // NpcRelationship for padre
        let rel = NpcRelationship {
            disposition: -3,
            events: vec![0, 1],
            knows_about: vec![KnowledgeFlag::KnowsPlayerKilled(garcia.clone())],
        };
        log.npc_memory.insert(padre(), rel);

        let req = DigestRequest {
            recent_event_limit: 5,
            include_npcs: vec![padre()],
            include_factions: false,
            include_open_promises: false,
            max_chars: 2000,
        };

        let digest = generate_digest(&log, &req);

        assert!(
            digest.contains("padre"),
            "digest must mention Padre; got:\n{digest}"
        );
        assert!(
            digest.contains("-3"),
            "digest must include padre's disposition -3; got:\n{digest}"
        );
        // Should mention the kill event from padre's event indices.
        assert!(
            digest.contains("garcia"),
            "digest must reference garcia (from NpcKilled event in padre's memory); got:\n{digest}"
        );
    }

    // -----------------------------------------------------------------------
    // test_digest_open_promises
    //
    // Log with PromiseMade("deliver_datachip") to Padre, no resolution:
    // with include_open_promises = true, digest must mention the open promise.
    // When PromiseKept matches, it must NOT appear.
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_open_promises() {
        // ---- part 1: open promise ----
        let mut log = CampaignLog::new(char_id());
        log.record(
            GameClockSnapshot(0),
            LogEventKind::PromiseMade {
                to: padre(),
                promise: "deliver_datachip".to_string(),
                due: Some(GameClockSnapshot(1440 * 14)), // Day 14
            },
        );

        let req = DigestRequest {
            recent_event_limit: 5,
            include_npcs: vec![],
            include_factions: false,
            include_open_promises: true,
            max_chars: 2000,
        };
        let digest = generate_digest(&log, &req);
        assert!(
            digest.contains("deliver_datachip"),
            "open promise must appear in digest; got:\n{digest}"
        );
        assert!(
            digest.contains("padre"),
            "open promise must name the NPC; got:\n{digest}"
        );

        // ---- part 2: kept promise must NOT appear ----
        let mut log2 = CampaignLog::new(char_id());
        log2.record(
            GameClockSnapshot(0),
            LogEventKind::PromiseMade {
                to: padre(),
                promise: "deliver_datachip".to_string(),
                due: None,
            },
        );
        log2.record(
            GameClockSnapshot(720),
            LogEventKind::PromiseKept {
                to: padre(),
                promise: "deliver_datachip".to_string(),
            },
        );

        let req2 = DigestRequest {
            recent_event_limit: 0,
            include_npcs: vec![],
            include_factions: false,
            include_open_promises: true,
            max_chars: 2000,
        };
        let digest2 = generate_digest(&log2, &req2);
        assert!(
            !digest2.contains("deliver_datachip"),
            "kept promise must NOT appear in open-promises section; got:\n{digest2}"
        );

        // ---- part 3: broken promise must also NOT appear ----
        let mut log3 = CampaignLog::new(char_id());
        log3.record(
            GameClockSnapshot(0),
            LogEventKind::PromiseMade {
                to: padre(),
                promise: "deliver_datachip".to_string(),
                due: None,
            },
        );
        log3.record(
            GameClockSnapshot(720),
            LogEventKind::PromiseBroken {
                to: padre(),
                promise: "deliver_datachip".to_string(),
            },
        );

        let req3 = DigestRequest {
            recent_event_limit: 0,
            include_npcs: vec![],
            include_factions: false,
            include_open_promises: true,
            max_chars: 2000,
        };
        let digest3 = generate_digest(&log3, &req3);
        assert!(
            !digest3.contains("deliver_datachip"),
            "broken promise must NOT appear in open-promises section; got:\n{digest3}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional: verify faction label thresholds
    // -----------------------------------------------------------------------
    #[test]
    fn test_faction_labels() {
        assert_eq!(faction_label(-10), "hostile");
        assert_eq!(faction_label(-5), "hostile");
        assert_eq!(faction_label(-4), "neutral");
        assert_eq!(faction_label(0), "neutral");
        assert_eq!(faction_label(4), "neutral");
        assert_eq!(faction_label(5), "friendly");
        assert_eq!(faction_label(10), "friendly");
    }

    // -----------------------------------------------------------------------
    // Additional: verify truncation never exceeds max_chars
    // -----------------------------------------------------------------------
    #[test]
    fn test_truncate_exact_boundary() {
        let s = "hello\nworld\nfoo".to_string();
        // max_chars = 8 → budget = 5 → truncated slice = "hello" → last '\n' not found
        // so we hard-cut at position 5: "hello" + "..." = 8 chars total.
        let result = truncate_to_max_chars(s.clone(), 8);
        assert!(result.len() <= 8, "result={result:?}");
        assert!(result.ends_with("..."));
    }

    // -----------------------------------------------------------------------
    // Additional: empty log produces empty digest
    // -----------------------------------------------------------------------
    #[test]
    fn test_empty_log_empty_digest() {
        let log = CampaignLog::new(char_id());
        let req = DigestRequest {
            recent_event_limit: 10,
            include_npcs: vec![],
            include_factions: true,
            include_open_promises: true,
            max_chars: 4096,
        };
        let digest = generate_digest(&log, &req);
        assert!(
            digest.is_empty(),
            "empty log must produce empty digest; got: {digest:?}"
        );
    }

    // -----------------------------------------------------------------------
    // Additional: determinism — same inputs yield identical output
    // -----------------------------------------------------------------------
    #[test]
    fn test_digest_is_deterministic() {
        let mut log = CampaignLog::new(char_id());
        log.record(
            GameClockSnapshot(0),
            LogEventKind::NpcMet {
                npc: padre(),
                beat: beat(),
                impression: NpcImpression::Friendly,
            },
        );
        log.record(
            GameClockSnapshot(1440),
            LogEventKind::GigCompleted {
                gig: GigId::from("hot_property"),
                outcome: GigOutcome::Success,
                payment: Eurobucks(1_500),
                ip_awarded: 25,
            },
        );
        let rel = NpcRelationship {
            disposition: 2,
            events: vec![0],
            knows_about: vec![],
        };
        log.npc_memory.insert(padre(), rel);
        log.set_faction_standing(FactionId::from("maelstrom"), -7);
        log.set_faction_standing(FactionId::from("tyger_claws"), 2);

        let req = DigestRequest {
            recent_event_limit: 10,
            include_npcs: vec![padre()],
            include_factions: true,
            include_open_promises: true,
            max_chars: 4096,
        };

        let d1 = generate_digest(&log, &req);
        let d2 = generate_digest(&log, &req);
        assert_eq!(d1, d2, "digest must be deterministic");
    }
}
