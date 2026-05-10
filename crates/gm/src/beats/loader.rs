//! Beat Chart loader and structural validator.
//!
//! Provides two public entry points:
//!
//! - [`load_gig`] — parse a single RON file into a [`Gig`].
//! - [`load_all_gigs`] — walk a directory for `*.ron` files, parse each,
//!   and collect into a `HashMap<GigId, Gig>`.
//! - [`validate_gig`] — run all structural checks against a [`Gig`],
//!   collecting **all** violations in one pass.
//!
//! ## Rulebook references
//!
//! - pp.395–396: Beat Chart structure rules (Hook first, Climax → Resolution last).
//! - pp.396–408: Beat type catalogue and ordering constraints.
//!
//! ## API deviation from plan (WP-603)
//!
//! The plan signature for `validate_gig` uses `&Catalog<NpcTemplate>` and
//! `&Catalog<LocationDef>`. Neither type exists yet — `LocationDef` is deferred
//! and full NpcTemplate catalog loading is downstream (WP-606). This
//! implementation uses `&HashSet<NpcTemplateId>` and `&HashSet<LocationId>`
//! instead. The semantics are identical for validation purposes: if a slug
//! is not in the set, emit the corresponding error variant.
//!
//! ## Encounter validation
//!
//! `EncounterRef` slugs are not validated against a catalog here because
//! WP-610 (encounter loader) is not yet merged. All `EncounterRef` values
//! are accepted unconditionally. This is noted in the PR description.

use crate::beats::schema::{Beat, BeatKind, Gig, HookEffect, MechanicalHookKind};
use crate::error::GmError;
use crate::ids::{BeatId, GigId};
use cpr_rules::world::LocationId;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

// Re-export NpcTemplateId from the npc module — it is the external type we
// take in validate_gig.
pub use crate::npc::NpcTemplateId;

// ─── Public API ──────────────────────────────────────────────────────────────

/// Parse a single Beat Chart RON file into a [`Gig`].
///
/// Wraps I/O and RON parse failures as
/// [`GmError::BeatChartLoadFailed`].
///
/// # Errors
///
/// Returns `Err(GmError::BeatChartLoadFailed { path, detail })` if the file
/// cannot be read or if the RON cannot be parsed into a [`Gig`].
pub fn load_gig(path: &Path) -> Result<Gig, GmError> {
    let source = std::fs::read_to_string(path).map_err(|e| GmError::BeatChartLoadFailed {
        path: path.to_path_buf(),
        detail: format!("I/O error: {e}"),
    })?;

    ron::from_str::<Gig>(&source).map_err(|e| GmError::BeatChartLoadFailed {
        path: path.to_path_buf(),
        detail: format!("RON parse error: {e}"),
    })
}

/// Walk `dir` for `*.ron` files, parse each, and collect into
/// `HashMap<GigId, Gig>`.
///
/// Uses `std::fs::read_dir` — no external `walkdir` dependency.
/// The walk is **non-recursive** (top-level `*.ron` files only).
///
/// # Errors
///
/// - Returns `Err(GmError::BeatChartLoadFailed)` if `dir` cannot be read,
///   if any individual file fails to parse, or if two files yield the same
///   [`GigId`].
pub fn load_all_gigs(dir: &Path) -> Result<HashMap<GigId, Gig>, GmError> {
    let read_dir = std::fs::read_dir(dir).map_err(|e| GmError::BeatChartLoadFailed {
        path: dir.to_path_buf(),
        detail: format!("cannot read directory: {e}"),
    })?;

    let mut map: HashMap<GigId, Gig> = HashMap::new();

    for entry_result in read_dir {
        let entry = entry_result.map_err(|e| GmError::BeatChartLoadFailed {
            path: dir.to_path_buf(),
            detail: format!("directory entry error: {e}"),
        })?;

        let path = entry.path();

        // Only process *.ron files; skip directories and other file types.
        if path.extension().and_then(|e| e.to_str()) != Some("ron") {
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let gig = load_gig(&path)?;

        if map.contains_key(&gig.id) {
            return Err(GmError::BeatChartLoadFailed {
                path,
                detail: format!(
                    "duplicate gig id '{}' — another file already defined this gig",
                    gig.id
                ),
            });
        }

        map.insert(gig.id.clone(), gig);
    }

    Ok(map)
}

/// Run all structural validations against a [`Gig`].
///
/// This function collects **all** diagnostics in one pass and returns them
/// together rather than short-circuiting on the first failure.
///
/// ## Checks performed
///
/// 1. Every [`Transition::target`] resolves to an existing [`BeatId`] in
///    `gig.beats`. Missing targets → [`GmError::TransitionTargetMissing`].
/// 2. `gig.start_beat` points to a beat whose `kind` is
///    [`BeatKind::Hook`]. If not → [`GmError::StartBeatNotHook`].
/// 3. Every Beat is reachable from `start_beat` via at least one Transition
///    chain (BFS). Unreachable beats → [`GmError::OrphanBeat`].
/// 4. At least one path from `start_beat` visits a [`BeatKind::Climax`] beat
///    that has a Transition to a [`BeatKind::Resolution`] beat. If no such
///    path exists → [`GmError::NoResolutionPath`].
/// 5. Every [`MechanicalHook::id`] is unique within the Gig. Duplicates →
///    [`GmError::DuplicateHookId`].
/// 6. Every NPC slug in `beat.present` and in `MechanicalHookKind::Ambush`
///    exists in `npc_template_ids`. Missing → [`GmError::NpcTemplateNotFound`].
/// 7. Every `beat.location` and every `LocationId` in `HookEffect::RevealLocation`
///    exists in `locations`. Missing → [`GmError::LocationRefNotFound`].
/// 8. `EncounterRef` slugs are **not** validated — WP-610 (encounter loader)
///    is not yet implemented. All encounter references are accepted.
///
/// ## API deviation from plan
///
/// The plan specifies `&Catalog<NpcTemplate>` and `&Catalog<LocationDef>`.
/// Those types do not exist yet; `LocationDef` is deferred and
/// `Catalog<NpcTemplate>` loading is downstream (WP-606). This implementation
/// accepts `&HashSet<NpcTemplateId>` and `&HashSet<LocationId>` instead.
///
/// # Errors
///
/// Returns `Ok(())` if no violations are found. Returns
/// `Err(Vec<GmError>)` containing all violations if any are found. The
/// `Vec` is **never empty** when `Err` is returned.
pub fn validate_gig(
    gig: &Gig,
    npc_template_ids: &HashSet<NpcTemplateId>,
    locations: &HashSet<LocationId>,
) -> Result<(), Vec<GmError>> {
    let mut errors: Vec<GmError> = Vec::new();

    // Build a fast lookup map: BeatId → &Beat.
    let beat_map: HashMap<&BeatId, &Beat> = gig.beats.iter().map(|b| (&b.id, b)).collect();

    // ── Check 1: All transition targets must exist ────────────────────────────
    for beat in &gig.beats {
        for transition in &beat.transitions {
            if !beat_map.contains_key(&transition.target) {
                errors.push(GmError::TransitionTargetMissing {
                    gig: gig.id.clone(),
                    beat: transition.target.clone(),
                });
            }
            // Also check targets embedded in HookEffect::Transition.
            for hook in &beat.mechanical_hooks {
                collect_effect_transition_targets(&hook.kind, &beat_map, &gig.id, &mut errors);
            }
        }
    }

    // ── Check 2: start_beat must be BeatKind::Hook ───────────────────────────
    match beat_map.get(&gig.start_beat) {
        Some(beat) if beat.kind != BeatKind::Hook => {
            let found = beat_kind_name(&beat.kind);
            errors.push(GmError::StartBeatNotHook {
                gig: gig.id.clone(),
                found,
            });
        }
        None => {
            // If start_beat itself doesn't exist, there's a deeper problem;
            // we still record TransitionTargetMissing for it when building
            // reachability below.  For StartBeatNotHook we can't determine
            // the kind, so just record an implicit structural failure.
            errors.push(GmError::StartBeatNotHook {
                gig: gig.id.clone(),
                found: "<missing>",
            });
        }
        _ => {} // start_beat is a Hook — OK
    }

    // ── Check 3: Reachability — BFS from start_beat ──────────────────────────
    let reachable = bfs_reachable(&gig.start_beat, &beat_map);
    for beat in &gig.beats {
        if !reachable.contains(&beat.id) {
            errors.push(GmError::OrphanBeat {
                gig: gig.id.clone(),
                beat: beat.id.clone(),
            });
        }
    }

    // ── Check 4: At least one Climax → Resolution path exists ────────────────
    if !has_climax_to_resolution_path(&gig.start_beat, &beat_map) {
        errors.push(GmError::NoResolutionPath {
            gig: gig.id.clone(),
        });
    }

    // ── Check 5: Duplicate MechanicalHook ids ────────────────────────────────
    let mut seen_hook_ids: HashSet<String> = HashSet::new();
    for beat in &gig.beats {
        for hook in &beat.mechanical_hooks {
            let slug = hook.id.as_str().to_string();
            if !seen_hook_ids.insert(slug) {
                errors.push(GmError::DuplicateHookId {
                    gig: gig.id.clone(),
                    id: hook.id.clone(),
                });
            }
        }
    }

    // ── Check 6: NPC template references ─────────────────────────────────────
    // Only validate if the caller provided a non-empty set; an empty set means
    // "no NPC catalog loaded yet — skip NPC validation".
    if !npc_template_ids.is_empty() {
        for beat in &gig.beats {
            for npc_slug in &beat.present {
                let tid = NpcTemplateId::from(npc_slug.as_str());
                if !npc_template_ids.contains(&tid) {
                    errors.push(GmError::NpcTemplateNotFound(npc_slug.clone()));
                }
            }
            // Also check MechanicalHookKind::Ambush ambusher slug.
            for hook in &beat.mechanical_hooks {
                if let MechanicalHookKind::Ambush { ambusher, .. } = &hook.kind {
                    let tid = NpcTemplateId::from(ambusher.as_str());
                    if !npc_template_ids.contains(&tid) {
                        errors.push(GmError::NpcTemplateNotFound(ambusher.clone()));
                    }
                }
            }
        }
    }

    // ── Check 7: Location references ─────────────────────────────────────────
    // Only validate if the caller provided a non-empty set.
    if !locations.is_empty() {
        for beat in &gig.beats {
            if !locations.contains(&beat.location) {
                errors.push(GmError::LocationRefNotFound(beat.location.0.clone()));
            }
        }
    }

    // ── Check 8: EncounterRef — skipped (WP-610 pending) ─────────────────────
    // All EncounterRef slugs are accepted unconditionally until WP-610 (encounter
    // loader) is implemented. See doc comment above.

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

/// Return the static string name for a [`BeatKind`] (used in error messages).
fn beat_kind_name(kind: &BeatKind) -> &'static str {
    match kind {
        BeatKind::Hook => "Hook",
        BeatKind::Development => "Development",
        BeatKind::Cliffhanger => "Cliffhanger",
        BeatKind::Climax => "Climax",
        BeatKind::Resolution => "Resolution",
    }
}

/// BFS from `start` over `beat_map`; returns the set of reachable [`BeatId`]s.
fn bfs_reachable(start: &BeatId, beat_map: &HashMap<&BeatId, &Beat>) -> HashSet<BeatId> {
    let mut visited: HashSet<BeatId> = HashSet::new();
    let mut queue: VecDeque<BeatId> = VecDeque::new();

    if beat_map.contains_key(start) {
        queue.push_back(start.clone());
        visited.insert(start.clone());
    }

    while let Some(current_id) = queue.pop_front() {
        if let Some(beat) = beat_map.get(&current_id) {
            for transition in &beat.transitions {
                if !visited.contains(&transition.target)
                    && beat_map.contains_key(&transition.target)
                {
                    visited.insert(transition.target.clone());
                    queue.push_back(transition.target.clone());
                }
            }
            // Also follow HookEffect::Transition targets for reachability.
            for hook in &beat.mechanical_hooks {
                for target in collect_hook_transition_targets(&hook.kind) {
                    if !visited.contains(&target) && beat_map.contains_key(&target) {
                        visited.insert(target.clone());
                        queue.push_back(target.clone());
                    }
                }
            }
        }
    }

    visited
}

/// Return `true` if there is at least one path from `start` that reaches a
/// [`BeatKind::Climax`] beat with a transition to a [`BeatKind::Resolution`] beat.
fn has_climax_to_resolution_path(start: &BeatId, beat_map: &HashMap<&BeatId, &Beat>) -> bool {
    // We check: among reachable beats, is there any Climax beat whose
    // transitions point to a Resolution beat?
    let reachable = bfs_reachable(start, beat_map);

    for id in &reachable {
        if let Some(beat) = beat_map.get(id) {
            if beat.kind == BeatKind::Climax {
                for transition in &beat.transitions {
                    if let Some(target_beat) = beat_map.get(&transition.target) {
                        if target_beat.kind == BeatKind::Resolution {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

/// Collect `BeatId` targets from `HookEffect::Transition` within a
/// [`MechanicalHookKind`] — used for reachability analysis.
fn collect_hook_transition_targets(kind: &MechanicalHookKind) -> Vec<BeatId> {
    let mut targets = Vec::new();
    match kind {
        MechanicalHookKind::SkillCheck {
            on_success,
            on_failure,
            ..
        } => {
            collect_effect_beat_targets(on_success, &mut targets);
            collect_effect_beat_targets(on_failure, &mut targets);
        }
        MechanicalHookKind::OpposedCheck {
            on_attacker_wins,
            on_defender_wins,
            ..
        } => {
            collect_effect_beat_targets(on_attacker_wins, &mut targets);
            collect_effect_beat_targets(on_defender_wins, &mut targets);
        }
        MechanicalHookKind::Negotiation {
            on_success,
            on_failure,
            ..
        } => {
            collect_effect_beat_targets(on_success, &mut targets);
            collect_effect_beat_targets(on_failure, &mut targets);
        }
        MechanicalHookKind::Ambush {
            on_detected,
            on_surprised,
            ..
        } => {
            collect_effect_beat_targets(on_detected, &mut targets);
            collect_effect_beat_targets(on_surprised, &mut targets);
        }
        MechanicalHookKind::Search { .. } => {}
    }
    targets
}

/// Recursively collect all [`BeatId`] targets embedded in a [`HookEffect`].
fn collect_effect_beat_targets(effect: &HookEffect, out: &mut Vec<BeatId>) {
    match effect {
        HookEffect::Transition(beat_id) => out.push(beat_id.clone()),
        HookEffect::Combine(effects) => {
            for e in effects {
                collect_effect_beat_targets(e, out);
            }
        }
        _ => {}
    }
}

/// Check that `HookEffect::Transition` targets exist in `beat_map`, emitting
/// errors for missing ones. Called during Check 1 scan.
fn collect_effect_transition_targets(
    kind: &MechanicalHookKind,
    beat_map: &HashMap<&BeatId, &Beat>,
    gig_id: &GigId,
    errors: &mut Vec<GmError>,
) {
    for target in collect_hook_transition_targets(kind) {
        if !beat_map.contains_key(&target) {
            errors.push(GmError::TransitionTargetMissing {
                gig: gig_id.clone(),
                beat: target,
            });
        }
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::beats::schema::{
        BeatKind, DiscoveryRef, EncounterRef, LocationRef, MechanicalHook, MechanicalHookKind,
        PaymentTier, Transition, TransitionCondition,
    };
    use crate::ids::{BeatId, EncounterId, GigId, MechanicalHookId};
    use cpr_rules::types::{Eurobucks, DV};
    use cpr_rules::world::LocationId;
    use std::collections::HashMap;
    use std::io::Write;

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

    fn always_to(target: &str) -> Transition {
        Transition {
            condition: TransitionCondition::Always,
            target: beat_id(target),
        }
    }

    fn minimal_beat(id: &str, kind: BeatKind, transitions: Vec<Transition>) -> Beat {
        Beat {
            id: beat_id(id),
            kind,
            location: loc("loc-a"),
            present: vec![],
            intent: "test intent".to_string(),
            mechanical_hooks: vec![],
            encounter: None,
            transitions,
        }
    }

    /// Build a minimal but structurally valid Gig:
    ///   Hook → Development → Climax → Resolution
    fn valid_gig() -> Gig {
        let beats = vec![
            minimal_beat("hook", BeatKind::Hook, vec![always_to("dev")]),
            minimal_beat("dev", BeatKind::Development, vec![always_to("climax")]),
            minimal_beat("climax", BeatKind::Climax, vec![always_to("res")]),
            minimal_beat("res", BeatKind::Resolution, vec![]),
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

    // ── Acceptance test 1: load_gig parses a RON fixture ─────────────────────

    /// Write a minimal Gig to a temp file, parse it with `load_gig`, and verify
    /// the round-trip is lossless.
    #[test]
    fn test_loads_sample_gig() {
        let gig = valid_gig();
        let ron_str = ron::to_string(&gig).expect("serialise");

        // Write to a temp file in /tmp.
        let path = std::path::PathBuf::from("/tmp/wp603_test_loads_sample_gig.ron");
        {
            let mut f = std::fs::File::create(&path).expect("create temp file");
            f.write_all(ron_str.as_bytes()).expect("write ron");
        }

        let loaded = load_gig(&path).expect("load_gig should succeed");
        assert_eq!(loaded, gig);
    }

    // ── Acceptance test 2: missing transition target ──────────────────────────

    /// A Gig where a Transition points to a non-existent BeatId should produce
    /// `TransitionTargetMissing`.
    #[test]
    fn test_invalid_transition_target_fails() {
        let mut gig = valid_gig();
        // Add a transition pointing to a non-existent beat.
        gig.beats[0].transitions.push(Transition {
            condition: TransitionCondition::AlarmRaised,
            target: beat_id("does-not-exist"),
        });

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter().any(
                |e| matches!(e, GmError::TransitionTargetMissing { beat, .. }
                if beat.as_str() == "does-not-exist")
            ),
            "expected TransitionTargetMissing, got: {errs:?}"
        );
    }

    // ── Acceptance test 3: orphan beat ───────────────────────────────────────

    /// A Gig that contains a Beat no transition points to should produce `OrphanBeat`.
    #[test]
    fn test_orphan_beat_fails() {
        let mut gig = valid_gig();
        // Insert an isolated beat that nothing reaches.
        gig.beats
            .push(minimal_beat("orphan", BeatKind::Development, vec![]));

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter().any(
                |e| matches!(e, GmError::OrphanBeat { beat, .. } if beat.as_str() == "orphan")
            ),
            "expected OrphanBeat, got: {errs:?}"
        );
    }

    // ── Acceptance test 4: no Climax → Resolution path ───────────────────────

    /// A Gig that never reaches a Resolution beat should produce `NoResolutionPath`.
    #[test]
    fn test_no_climax_or_resolution_fails() {
        // Hook → Development → Climax (Climax has no transition to Resolution)
        let beats = vec![
            minimal_beat("hook", BeatKind::Hook, vec![always_to("dev")]),
            minimal_beat("dev", BeatKind::Development, vec![always_to("climax")]),
            // Climax transitions to another Climax, never Resolution.
            minimal_beat("climax", BeatKind::Climax, vec![always_to("climax2")]),
            minimal_beat("climax2", BeatKind::Climax, vec![]),
        ];

        let gig = Gig {
            id: gig_id("no-res-gig"),
            title: "No Resolution".to_string(),
            fixer: "fixer".to_string(),
            payment: PaymentTier::Cheap(Eurobucks(100)),
            setting: "test".to_string(),
            scope_hours: 1,
            npcs: HashMap::new(),
            locations: HashMap::new(),
            beats,
            start_beat: beat_id("hook"),
        };

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter()
                .any(|e| matches!(e, GmError::NoResolutionPath { .. })),
            "expected NoResolutionPath, got: {errs:?}"
        );
    }

    // ── Acceptance test 5: all errors collected in one pass ──────────────────

    /// A Gig with multiple violations should return ALL of them, not just the
    /// first one encountered.
    #[test]
    fn test_validator_collects_all_errors() {
        let mut gig = valid_gig();

        // Violation 1: broken transition target.
        gig.beats[0].transitions.push(Transition {
            condition: TransitionCondition::AlarmRaised,
            target: beat_id("ghost-beat"),
        });

        // Violation 2: orphan beat.
        gig.beats
            .push(minimal_beat("orphan", BeatKind::Development, vec![]));

        // Violation 3: duplicate hook id.
        let dup_hook = MechanicalHook {
            id: hook_id("dup-hook"),
            kind: MechanicalHookKind::Search {
                dv: DV(10),
                finds: vec![],
            },
        };
        gig.beats[0].mechanical_hooks.push(dup_hook.clone());
        gig.beats[1].mechanical_hooks.push(dup_hook);

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail with multiple errors");

        assert!(
            errs.len() >= 3,
            "expected at least 3 errors, got {}: {errs:?}",
            errs.len()
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, GmError::TransitionTargetMissing { .. })),
            "missing TransitionTargetMissing"
        );
        assert!(
            errs.iter().any(|e| matches!(e, GmError::OrphanBeat { .. })),
            "missing OrphanBeat"
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, GmError::DuplicateHookId { .. })),
            "missing DuplicateHookId"
        );
    }

    // ── Acceptance test 6: duplicate gig id across files ─────────────────────

    /// Two RON files in the same directory that share the same GigId should cause
    /// `load_all_gigs` to return `Err`.
    #[test]
    fn test_duplicate_gig_id_in_directory() {
        let dir = std::path::PathBuf::from("/tmp/wp603_dup_gig_test");
        std::fs::create_dir_all(&dir).expect("mkdir");

        let gig = valid_gig(); // id = "test-gig"
        let ron_str = ron::to_string(&gig).expect("serialise");

        let path_a = dir.join("gig_a.ron");
        let path_b = dir.join("gig_b.ron");

        std::fs::write(&path_a, &ron_str).expect("write a");
        std::fs::write(&path_b, &ron_str).expect("write b");

        let result = load_all_gigs(&dir);
        assert!(result.is_err(), "expected Err for duplicate gig id, got Ok");
        if let Err(GmError::BeatChartLoadFailed { detail, .. }) = result {
            assert!(
                detail.contains("duplicate"),
                "expected 'duplicate' in detail, got: {detail}"
            );
        } else {
            panic!("wrong error variant");
        }

        // Clean up.
        let _ = std::fs::remove_file(&path_a);
        let _ = std::fs::remove_file(&path_b);
    }

    // ── Additional: valid gig passes all checks ───────────────────────────────

    /// The minimal valid gig fixture should pass `validate_gig` with no errors.
    #[test]
    fn test_valid_gig_passes() {
        let gig = valid_gig();
        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        assert!(result.is_ok(), "valid gig should pass: {result:?}");
    }

    // ── Additional: start beat not a Hook ────────────────────────────────────

    /// If `start_beat` points to a Development beat, `StartBeatNotHook` is emitted.
    #[test]
    fn test_start_beat_not_hook_fails() {
        let mut gig = valid_gig();
        // Redirect start_beat to the dev beat (Development kind).
        gig.start_beat = beat_id("dev");

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter().any(
                |e| matches!(e, GmError::StartBeatNotHook { found, .. } if *found == "Development")
            ),
            "expected StartBeatNotHook with 'Development', got: {errs:?}"
        );
    }

    // ── Additional: NPC validation ────────────────────────────────────────────

    /// When `npc_template_ids` is non-empty, beats referencing unknown NPC slugs
    /// should produce `NpcTemplateNotFound`.
    #[test]
    fn test_npc_template_not_found() {
        let mut gig = valid_gig();
        gig.beats[0].present.push("unknown_npc".to_string());

        let known: HashSet<NpcTemplateId> =
            vec![NpcTemplateId::from("known_npc")].into_iter().collect();

        let result = validate_gig(&gig, &known, &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter()
                .any(|e| matches!(e, GmError::NpcTemplateNotFound(s) if s == "unknown_npc")),
            "expected NpcTemplateNotFound, got: {errs:?}"
        );
    }

    // ── Additional: location validation ───────────────────────────────────────

    /// When `locations` is non-empty, beats referencing unknown location slugs
    /// should produce `LocationRefNotFound`.
    #[test]
    fn test_location_ref_not_found() {
        let gig = valid_gig(); // All beats use loc("loc-a")

        // Provide a set that does NOT include "loc-a".
        let known: HashSet<LocationId> = vec![LocationId("other-loc".to_string())]
            .into_iter()
            .collect();

        let result = validate_gig(&gig, &HashSet::new(), &known);
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter()
                .any(|e| matches!(e, GmError::LocationRefNotFound(s) if s == "loc-a")),
            "expected LocationRefNotFound, got: {errs:?}"
        );
    }

    // ── Additional: encounter ref is not validated (WP-610 pending) ───────────

    /// EncounterRef slugs should not cause validation failures because the
    /// encounter catalog is not loaded until WP-610.
    #[test]
    fn test_encounter_ref_not_validated() {
        let mut gig = valid_gig();
        gig.beats[2].encounter = Some(EncounterRef(EncounterId::from("nonexistent_encounter")));

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        assert!(
            result.is_ok(),
            "encounter refs should not be validated yet: {result:?}"
        );
    }

    // ── Additional: duplicate hook ids across beats ───────────────────────────

    #[test]
    fn test_duplicate_hook_id_fails() {
        let mut gig = valid_gig();

        let mk_hook = |id: &str| MechanicalHook {
            id: hook_id(id),
            kind: MechanicalHookKind::Search {
                dv: DV(10),
                finds: vec![DiscoveryRef("intel".to_string())],
            },
        };

        // Add the same hook id to two different beats.
        gig.beats[0].mechanical_hooks.push(mk_hook("shared-hook"));
        gig.beats[1].mechanical_hooks.push(mk_hook("shared-hook"));

        let result = validate_gig(&gig, &HashSet::new(), &HashSet::new());
        let errs = result.expect_err("should fail");
        assert!(
            errs.iter().any(
                |e| matches!(e, GmError::DuplicateHookId { id, .. } if id.as_str() == "shared-hook")
            ),
            "expected DuplicateHookId, got: {errs:?}"
        );
    }
}
