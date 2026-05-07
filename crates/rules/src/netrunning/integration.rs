//! Netrun integration with the combat queue (WP-417).
//!
//! This module wires the Netrunning subsystem into the same
//! [`CombatState`] queue as physical combatants. It provides:
//!
//! - [`NetrunnerActionChoice`] — the binary choice a Netrunner makes on their
//!   Turn: take a Meat Action *or* take their NET Actions this round.
//! - [`safe_jack_out`] — a Meat Action used while within 6 m of an access
//!   point. Releases control nodes, applies queued viruses to the
//!   Architecture, and clears `world.netrun`. See p.198.
//! - [`unsafe_jack_out`] — triggered when the Netrunner leaves access-point
//!   range while still jacked in. Every still-rezzed enemy Black ICE applies
//!   its effect to the Netrunner on exit. See p.198.
//! - [`insert_ice_in_queue`] — inserts a Black ICE entity at the top of the
//!   initiative queue, one above the current highest score. See p.205.
//!
//! ## Rulebook references
//!
//! - **p.197:** Meat Actions vs. NET Actions — "On your Turn, you can take
//!   either a Meat Action or take as many NET Actions as your Interface
//!   (the Netrunner Role Ability) allows."
//! - **p.198:** Jack In/Out — safe jack-out is a NET Action; leaving the
//!   access-point range forces an *unsafe* jack-out. Unsafe jack-out applies
//!   the effect of all remaining un-Derezzed enemy Black ICE encountered in
//!   the Architecture. "Jacking Out resets the defences of a NET Architecture."
//! - **p.199:** Control — "You lose control of any Control Nodes you hold in
//!   an Architecture when you Jack Out."
//! - **p.200:** Virus — queued Viruses persist in the Architecture after
//!   jack-out; applied on both safe and unsafe paths.
//! - **p.205:** Black ICE in the combat queue — "It is placed into the
//!   Initiative Queue at the top, one number above the entity with the
//!   previously highest Initiative."
//!
//! See pp.197–205.

use crate::catalog::black_ice::BlackIceId;
use crate::error::RulesError;
use crate::netrunning::architecture::{BlackIceState, Floor};
use crate::netrunning::state::Virus;
use crate::rng::Rng;
use crate::types::EntityId;
use crate::world::World;
use uuid::Uuid;

// ── Action choice ─────────────────────────────────────────────────────────────

/// The Netrunner's turn-level choice: act in Meatspace or act in the NET.
///
/// Per p.197: "On your Turn, you can take either a Meat Action or take as many
/// NET Actions as your Interface (the Netrunner Role Ability) allows."
///
/// This is a pure enum used by caller code (e.g. the turn engine or the UI
/// layer) to record which path the Netrunner chose. It does **not** itself
/// resolve any game effects — those are handled by the relevant action types
/// in [`crate::netrunning`] and [`crate::combat`].
///
/// See p.197 (Meat Actions vs. NET Actions).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum NetrunnerActionChoice {
    /// The Netrunner uses their non-Move Action as a Meat Action (physical
    /// world action — attack, interact, etc.). See p.197.
    Meat,
    /// The Netrunner spends their non-Move Action budget on NET Actions
    /// inside the current Architecture. The number of NET Actions available
    /// is determined by `Interface rank` (see p.197 table and
    /// [`crate::netrunning::actions::net_actions_per_turn`]).
    Net,
    /// The Netrunner jacks out safely — a Meat Action taken while within
    /// 6 m/yds of the access point they jacked in from. See p.198.
    JackOut,
}

// ── UnsafeJackoutEffect ───────────────────────────────────────────────────────

/// One effect applied by a still-rezzed enemy Black ICE on an unsafe jack-out.
///
/// Per p.198: "You suffer the effect of all remaining enemy Black ICE you've
/// encountered, but not Derezzed, in the NET Architecture before you get out."
///
/// Each entry represents one floor whose Black ICE was `InCombat` (rezzed and
/// not Derezzed) at the time of the unsafe jack-out. The `damage_to_netrunner`
/// field is a placeholder — the actual effect of each ICE is a rich
/// [`crate::catalog::black_ice::BlackIceEffect`] that WP-414+ resolves;
/// this module captures only the raw damage component here so the
/// acceptance tests can verify the count and identity of the ICE.
///
/// See p.198 (Jacking In or Out, unsafe path) and pp.206–207 (ICE effects).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnsafeJackoutEffect {
    /// Catalog identifier for the Black ICE that applies this effect.
    ///
    /// Sourced from [`Floor::BlackIce::template`] on floors whose
    /// [`BlackIceState`] is [`BlackIceState::InCombat`] at the time of
    /// jack-out.
    ///
    /// See pp.205–207 (Black ICE catalogue).
    pub from_ice: BlackIceId,
    /// Human-readable description of the effect applied (e.g. "brain damage",
    /// "forced unsafe jack-out"). Populated by this module as a placeholder;
    /// WP-414 will replace with the real [`crate::catalog::black_ice::BlackIceEffect`]
    /// resolution.
    ///
    /// See pp.206–207 (Effect column in the Black ICE table).
    pub kind: String,
    /// Direct damage dealt to the Netrunner's brain / body.
    ///
    /// This is a simplified placeholder until WP-414 supplies the full damage
    /// pipeline. RAW: each ICE effect is different (see pp.206–207); this
    /// captures the minimum viable signal for tests and the GM layer.
    pub damage_to_netrunner: u16,
}

// ── JackOutOutcome ────────────────────────────────────────────────────────────

/// Outcome produced by either [`safe_jack_out`] or [`unsafe_jack_out`].
///
/// On both jack-out paths:
/// - All held control nodes are released (p.199).
/// - All queued Viruses are drained and returned to the caller for
///   persistent installation into the Architecture (p.200).
/// - `world.netrun` is set to `None`.
///
/// On the *unsafe* path additionally:
/// - `unsafe_jackout_consequences` is populated with one entry per
///   still-rezzed (InCombat) enemy Black ICE floor encountered in the
///   Architecture. See p.198.
///
/// See pp.198–200.
#[derive(Clone, Debug, PartialEq)]
pub struct JackOutOutcome {
    /// Viruses drained from [`crate::netrunning::state::NetrunState::queued_viruses`]
    /// and returned to the caller for persistent installation into the
    /// Architecture state.
    ///
    /// Empty when no Viruses were queued. Applies on **both** jack-out paths.
    ///
    /// See p.200 (Virus — "the only way … that persists after Jack Out").
    pub viruses_applied: Vec<Virus>,
    /// Floor indices of Control Nodes that were released on this jack-out.
    ///
    /// Populated from [`crate::netrunning::state::NetrunState::controlled_nodes`]
    /// before the list is cleared. Applies on **both** jack-out paths.
    ///
    /// See p.199 (Control — "You lose control of any Control Nodes … when
    /// you Jack Out").
    pub control_nodes_released: Vec<usize>,
    /// Effects from still-rezzed enemy Black ICE, applied only on the *unsafe*
    /// jack-out path (p.198). Empty on a safe jack-out.
    ///
    /// One entry per `Floor::BlackIce` whose [`BlackIceState`] is
    /// [`BlackIceState::InCombat`] in `NetrunState::floors` at the moment
    /// of unsafe jack-out.
    ///
    /// See p.198 ("You suffer the effect of all remaining enemy Black ICE …").
    pub unsafe_jackout_consequences: Vec<UnsafeJackoutEffect>,
}

// ── safe_jack_out ─────────────────────────────────────────────────────────────

/// Perform a safe jack-out: a Meat Action taken within access-point range.
///
/// Per p.198: "Using a NET Action, you can Jack In to a NET Architecture …
/// Being jacked in is a prerequisite … Moving out of the access point's range
/// while jacked in … jacks you out of the NET Architecture automatically …
/// It is much safer to use a NET Action to Jack Out from within the access
/// point's range."
///
/// ## Steps
///
/// 1. Require `world.netrun` to be `Some` — returns
///    [`RulesError::NoActiveNetrun`] otherwise.
/// 2. Drain the queued Virus list (p.200).
/// 3. Record and release all held control nodes (p.199).
/// 4. Clear `world.netrun` to `None`.
///
/// No ICE effects are applied on the safe path (p.198: only the unsafe path
/// triggers them).
///
/// See p.198 (Jack In/Out), p.199 (Control), p.200 (Virus).
pub fn safe_jack_out(world: &mut World) -> Result<JackOutOutcome, RulesError> {
    // Step 1 — require an active netrun. See p.198.
    let netrun = world.netrun.as_mut().ok_or(RulesError::NoActiveNetrun)?;

    // Step 2 — drain queued Viruses for persistence in Architecture. See p.200.
    let viruses_applied = netrun.drain_viruses_for_jackout();

    // Step 3 — record and release all held Control Nodes. See p.199.
    let control_nodes_released = netrun.controlled_nodes.clone();
    netrun.release_all_control_nodes();

    // Step 4 — clear the active netrun state. See p.198.
    world.netrun = None;

    Ok(JackOutOutcome {
        viruses_applied,
        control_nodes_released,
        // Safe path: no ICE consequences. See p.198.
        unsafe_jackout_consequences: Vec::new(),
    })
}

// ── unsafe_jack_out ───────────────────────────────────────────────────────────

/// Perform an unsafe jack-out: leaving access-point range while still jacked in.
///
/// Per p.198: "Moving out of the access point's range while jacked in to the
/// Architecture jacks you out of the NET Architecture automatically, but leaves
/// you vulnerable: **You suffer the effect of all remaining enemy Black ICE
/// you've encountered, but not Derezzed, in the NET Architecture before you
/// get 'out.'**"
///
/// ## Steps
///
/// 1. Require `world.netrun` to be `Some` — returns
///    [`RulesError::NoActiveNetrun`] otherwise.
/// 2. Iterate `NetrunState::floors` and collect every `Floor::BlackIce` whose
///    [`BlackIceState`] is [`BlackIceState::InCombat`] (rezzed, not Derezzed,
///    not Slid). These are the ICE whose effects apply on exit. See p.198.
/// 3. Drain the queued Virus list (p.200).
/// 4. Record and release all held control nodes (p.199).
/// 5. Clear `world.netrun` to `None`.
/// 6. Return the [`JackOutOutcome`] with populated `unsafe_jackout_consequences`.
///
/// **Note on damage values:** the `damage_to_netrunner` field in each
/// [`UnsafeJackoutEffect`] is set to `0` as a placeholder in this WP.
/// WP-414 will supply the real [`crate::catalog::black_ice::BlackIceEffect`]
/// resolution (ATK + d10 vs Interface + d10; on hit, apply the effect). This
/// WP guarantees the *count* and *identity* of the applying ICE — the actual
/// effect resolution belongs to WP-414.
///
/// See p.198 (Jacking In or Out, unsafe path) and pp.205–207 (Black ICE
/// effects per ICE type).
pub fn unsafe_jack_out(world: &mut World) -> Result<JackOutOutcome, RulesError> {
    // Step 1 — require an active netrun. See p.198.
    let netrun = world.netrun.as_mut().ok_or(RulesError::NoActiveNetrun)?;

    // Step 2 — collect all still-rezzed (InCombat) enemy Black ICE floors.
    // These apply their effects when the Netrunner is forcibly ejected. p.198.
    let unsafe_jackout_consequences: Vec<UnsafeJackoutEffect> = netrun
        .floors
        .iter()
        .filter_map(|floor| match floor {
            Floor::BlackIce {
                template,
                state: BlackIceState::InCombat,
                ..
            } => Some(UnsafeJackoutEffect {
                from_ice: template.clone(),
                // Placeholder: WP-414 resolves the real effect via ATK + d10.
                // See pp.206–207 (Effect column) and p.205 (encountering ICE).
                kind: "ice_effect_placeholder".to_string(),
                // Placeholder: actual damage is computed by WP-414.
                // Set to 0 so this module does not speculate on damage values.
                damage_to_netrunner: 0,
            }),
            _ => None,
        })
        .collect();

    // Step 3 — drain queued Viruses for persistence in Architecture. See p.200.
    let viruses_applied = netrun.drain_viruses_for_jackout();

    // Step 4 — record and release all held Control Nodes. See p.199.
    let control_nodes_released = netrun.controlled_nodes.clone();
    netrun.release_all_control_nodes();

    // Step 5 — clear the active netrun state. See p.198.
    world.netrun = None;

    Ok(JackOutOutcome {
        viruses_applied,
        control_nodes_released,
        unsafe_jackout_consequences,
    })
}

// ── insert_ice_in_queue ───────────────────────────────────────────────────────

/// Insert a Black ICE entity into the active combat queue at the top.
///
/// Per p.205: "It is placed into the Initiative Queue at the top, one number
/// above the entity with the previously highest Initiative."
///
/// This is a thin wrapper around [`CombatState::insert_at_top`] that:
/// 1. Requires `world.combat` to be `Some` (panics otherwise — Black ICE can
///    only enter the queue during an active combat encounter per p.205).
/// 2. Mints a deterministic [`EntityId`] for the ICE entity from the
///    `ice_floor_index` so that the same floor always produces the same
///    entity ID within a given run (and thus a given seed).
/// 3. Delegates to [`CombatState::insert_at_top`].
///
/// ## EntityId minting
///
/// The minted ID uses the magic prefix `0x0000_424B_4943_0000` (ASCII
/// `BKIC` for "Black ICE") combined with the floor index in the low 64 bits:
///
/// ```text
/// Uuid::from_u128(0x0000_424B_4943_0000_0000_0000_0000_0000 | floor_index as u128)
/// ```
///
/// This is deterministic and WASM-compatible (no OS entropy). Multiple calls
/// for the same `ice_floor_index` produce the same `EntityId`, which is
/// intentional — the same ICE on the same floor is always the same entity.
///
/// # Panics
///
/// Panics if `world.combat` is `None`. Black ICE insertion only makes sense
/// during an active combat encounter (p.205).
///
/// See p.205 (Black ICE in the Initiative Queue).
pub fn insert_ice_in_queue(world: &mut World, ice_floor_index: usize, rng: &mut Rng) {
    // Magic prefix: ASCII "BKIC" in bytes 4–7 (big-endian within u128).
    // This marks the UUID as a Black ICE entity for hand-inspection of saves.
    const BLACK_ICE_MAGIC: u128 = 0x0000_424B_4943_0000_0000_0000_0000_0000;
    let entity_id = EntityId(Uuid::from_u128(BLACK_ICE_MAGIC | ice_floor_index as u128));

    // Borrow note: `CombatState::insert_at_top` accepts `&World` only to
    // satisfy its API contract — the WP-301 implementation discards it with
    // `let _ = (world, rng)` (see turn_engine.rs). The Rust borrow checker
    // does not allow us to pass `world` as both `&mut` (to get `combat`) and
    // `&` (to satisfy insert_at_top's parameter). We work around this by
    // temporarily taking the `CombatState` out of `world`, calling
    // `insert_at_top` with `world` (now safely immutable), and putting the
    // modified state back. This avoids a dummy allocation and preserves
    // semantics precisely. When WP-301 is updated to actually inspect `world`
    // during insertion, this call site will need reviewing.
    let mut combat = world.combat.take().expect(
        "insert_ice_in_queue called with no active CombatState — p.205 requires active combat",
    );

    // insert_at_top ignores `world` (WP-301). See borrow note above.
    combat.insert_at_top(entity_id, world, rng);

    // Restore the modified combat state.
    world.combat = Some(combat);
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::Role;
    use crate::combat::CombatState;
    use crate::netrunning::architecture::{BlackIceState, Floor, NetArchId};
    use crate::netrunning::state::{NetrunState, Virus, VirusEffect};
    use crate::types::{CharacterId, EntityId, DV};
    use crate::world::test_support::fresh_pc;
    use crate::world::World;
    use rand::SeedableRng;
    use uuid::Uuid;

    // ── helpers ───────────────────────────────────────────────────────────────

    fn eid(n: u128) -> EntityId {
        EntityId(Uuid::from_u128(n))
    }

    fn arch_id(s: &str) -> NetArchId {
        NetArchId(s.to_string())
    }

    fn make_virus(desc: &str) -> Virus {
        Virus {
            description: desc.into(),
            effect: VirusEffect::AlterIcon("asp".into()),
            dv_to_install: DV(6),
            net_actions_to_install: 1,
        }
    }

    /// Build a world with a Netrunner PC and an active [`NetrunState`].
    fn world_with_netrun(floors: Vec<Floor>) -> (World, EntityId) {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0x01));
        pc.role = Role::Netrunner;
        pc.role_rank = 4;
        let entity_id = EntityId(pc.id.0);

        let mut world = World::new(pc);
        let mut state = NetrunState::start(entity_id, arch_id("test-arch"), 4);
        state.floors = floors;
        world.netrun = Some(state);
        (world, entity_id)
    }

    /// Build a minimal `CombatState` with one participant.
    fn minimal_combat(world: &World, entity: EntityId, rng: &mut Rng) -> CombatState {
        CombatState::start(vec![entity], world, rng)
    }

    // ── test_netrunner_meat_or_net_action ─────────────────────────────────────

    /// `test_netrunner_meat_or_net_action` — on the Netrunner's turn, they can
    /// choose either [`NetrunnerActionChoice::Meat`] or
    /// [`NetrunnerActionChoice::Net`] (or [`NetrunnerActionChoice::JackOut`]).
    ///
    /// This test verifies the three variants are distinct, `Copy`/`Clone`-able,
    /// and comparable — it is primarily an enum exercise as described in the
    /// WP-417 acceptance criteria.
    ///
    /// See p.197 (Meat Actions vs. NET Actions).
    #[test]
    fn test_netrunner_meat_or_net_action() {
        let meat = NetrunnerActionChoice::Meat;
        let net = NetrunnerActionChoice::Net;
        let jack = NetrunnerActionChoice::JackOut;

        // All three variants are distinct.
        assert_ne!(meat, net, "Meat and Net must be different choices");
        assert_ne!(meat, jack, "Meat and JackOut must be different choices");
        assert_ne!(net, jack, "Net and JackOut must be different choices");

        // Clone / Copy work.
        let meat2 = meat;
        assert_eq!(meat, meat2, "Copy of Meat must equal Meat");

        // Debug is implemented (used by test output).
        let _ = format!("{:?}", net);
        let _ = format!("{:?}", jack);

        // The choice can be matched exhaustively.
        let chosen = NetrunnerActionChoice::Net;
        let label = match chosen {
            NetrunnerActionChoice::Meat => "meat",
            NetrunnerActionChoice::Net => "net",
            NetrunnerActionChoice::JackOut => "jackout",
        };
        assert_eq!(label, "net", "match on Net must select the net arm");
    }

    // ── test_safe_jackout_releases_control_nodes ──────────────────────────────

    /// `test_safe_jackout_releases_control_nodes` — after safe jack-out, all
    /// held control nodes are returned in the outcome and the netrun state is
    /// cleared.
    ///
    /// See p.199 (Control — "You lose control of any Control Nodes … when you
    /// Jack Out") and p.198 (Jack In/Out).
    #[test]
    fn test_safe_jackout_releases_control_nodes() {
        let (mut world, _entity) = world_with_netrun(Vec::new());

        // Give the netrunner some control nodes.
        {
            let netrun = world.netrun.as_mut().unwrap();
            netrun.take_control_node(2);
            netrun.take_control_node(5);
            netrun.take_control_node(8);
        }

        let outcome = safe_jack_out(&mut world).expect("safe jack-out must succeed");

        // All three nodes returned in outcome.
        assert_eq!(
            outcome.control_nodes_released.len(),
            3,
            "three control nodes must be returned"
        );
        assert!(
            outcome.control_nodes_released.contains(&2),
            "node 2 must be released"
        );
        assert!(
            outcome.control_nodes_released.contains(&5),
            "node 5 must be released"
        );
        assert!(
            outcome.control_nodes_released.contains(&8),
            "node 8 must be released"
        );

        // Netrun state cleared.
        assert!(
            world.netrun.is_none(),
            "world.netrun must be None after safe jack-out"
        );

        // Safe path: no ICE consequences.
        assert!(
            outcome.unsafe_jackout_consequences.is_empty(),
            "safe jack-out must have no ICE consequences"
        );
    }

    // ── test_safe_jackout_applies_viruses ─────────────────────────────────────

    /// `test_safe_jackout_applies_viruses` — after safe jack-out, all queued
    /// Viruses are drained and returned in the outcome.
    ///
    /// See p.200 (Virus — "the only way a Netrunner can make a change to a NET
    /// Architecture that persists after they Jack Out").
    #[test]
    fn test_safe_jackout_applies_viruses() {
        let (mut world, _entity) = world_with_netrun(Vec::new());

        let v1 = make_virus("first virus");
        let v2 = make_virus("second virus");

        {
            let netrun = world.netrun.as_mut().unwrap();
            netrun.queue_virus(v1.clone());
            netrun.queue_virus(v2.clone());
        }

        let outcome = safe_jack_out(&mut world).expect("safe jack-out must succeed");

        // Both viruses returned.
        assert_eq!(
            outcome.viruses_applied.len(),
            2,
            "two viruses must be applied on jack-out"
        );
        assert_eq!(outcome.viruses_applied[0], v1, "first virus must match");
        assert_eq!(outcome.viruses_applied[1], v2, "second virus must match");

        // Netrun state cleared.
        assert!(
            world.netrun.is_none(),
            "world.netrun must be None after safe jack-out"
        );
    }

    // ── test_unsafe_jackout_applies_all_pending_ice ───────────────────────────

    /// `test_unsafe_jackout_applies_all_pending_ice` — when there are 2
    /// still-rezzed (InCombat) enemy Black ICE floors, both apply their effects
    /// and appear in `unsafe_jackout_consequences`.
    ///
    /// ICE with [`BlackIceState::Derezzed`] or [`BlackIceState::Slid`] must
    /// NOT appear.
    ///
    /// See p.198 ("You suffer the effect of all remaining enemy Black ICE
    /// you've encountered, but not Derezzed …").
    #[test]
    fn test_unsafe_jackout_applies_all_pending_ice() {
        // Build floors: 2 InCombat ICE, 1 Derezzed, 1 Slid.
        let floors = vec![
            Floor::BlackIce {
                template: BlackIceId("hellhound".into()),
                state: BlackIceState::InCombat,
                ice_per: 6,
            },
            Floor::BlackIce {
                template: BlackIceId("asp".into()),
                state: BlackIceState::InCombat,
                ice_per: 4,
            },
            Floor::BlackIce {
                template: BlackIceId("raven".into()),
                state: BlackIceState::Derezzed, // must NOT apply
                ice_per: 2,
            },
            Floor::BlackIce {
                template: BlackIceId("wisp".into()),
                state: BlackIceState::Slid, // must NOT apply
                ice_per: 3,
            },
        ];

        let (mut world, _entity) = world_with_netrun(floors);

        let outcome = unsafe_jack_out(&mut world).expect("unsafe jack-out must succeed");

        // Exactly 2 ICE effects — only the InCombat ones.
        assert_eq!(
            outcome.unsafe_jackout_consequences.len(),
            2,
            "exactly 2 InCombat ICE must apply their effects (not Derezzed/Slid)"
        );

        // The two applying ICE are hellhound and asp (in floor order).
        assert_eq!(
            outcome.unsafe_jackout_consequences[0].from_ice,
            BlackIceId("hellhound".into()),
            "first InCombat ICE is hellhound"
        );
        assert_eq!(
            outcome.unsafe_jackout_consequences[1].from_ice,
            BlackIceId("asp".into()),
            "second InCombat ICE is asp"
        );

        // Netrun state cleared.
        assert!(
            world.netrun.is_none(),
            "world.netrun must be None after unsafe jack-out"
        );
    }

    // ── test_jackout_clears_netrun_state ─────────────────────────────────────

    /// `test_jackout_clears_netrun_state` — after jack-out (either path),
    /// `world.netrun` is `None`.
    ///
    /// See p.198 (Jacking Out clears the active netrun).
    #[test]
    fn test_jackout_clears_netrun_state() {
        // Safe path.
        {
            let (mut world, _) = world_with_netrun(Vec::new());
            assert!(
                world.netrun.is_some(),
                "netrun must be Some before jack-out"
            );
            safe_jack_out(&mut world).unwrap();
            assert!(
                world.netrun.is_none(),
                "safe jack-out must clear world.netrun"
            );
        }

        // Unsafe path.
        {
            let (mut world, _) = world_with_netrun(Vec::new());
            assert!(
                world.netrun.is_some(),
                "netrun must be Some before jack-out"
            );
            unsafe_jack_out(&mut world).unwrap();
            assert!(
                world.netrun.is_none(),
                "unsafe jack-out must clear world.netrun"
            );
        }
    }

    // ── test_safe_jackout_no_active_netrun ───────────────────────────────────

    /// Attempting safe jack-out with no active netrun returns
    /// [`RulesError::NoActiveNetrun`].
    ///
    /// See p.198 (Jack In required).
    #[test]
    fn test_safe_jackout_no_active_netrun() {
        let pc = fresh_pc();
        let mut world = World::new(pc);
        // world.netrun is None.

        let err = safe_jack_out(&mut world).expect_err("must fail without active netrun");
        assert!(
            matches!(err, RulesError::NoActiveNetrun),
            "expected NoActiveNetrun, got {err:?}"
        );
    }

    // ── test_unsafe_jackout_no_active_netrun ─────────────────────────────────

    /// Attempting unsafe jack-out with no active netrun returns
    /// [`RulesError::NoActiveNetrun`].
    #[test]
    fn test_unsafe_jackout_no_active_netrun() {
        let pc = fresh_pc();
        let mut world = World::new(pc);

        let err = unsafe_jack_out(&mut world).expect_err("must fail without active netrun");
        assert!(
            matches!(err, RulesError::NoActiveNetrun),
            "expected NoActiveNetrun, got {err:?}"
        );
    }

    // ── test_insert_ice_in_queue ──────────────────────────────────────────────

    /// Inserting a Black ICE entity places it at the top of the combat queue
    /// with score = (previous highest + 1). See p.205.
    #[test]
    fn test_insert_ice_in_queue() {
        let mut pc = fresh_pc();
        pc.id = CharacterId(Uuid::from_u128(0x01));
        let entity = eid(1);
        let world_ref = World::new(pc.clone());
        let mut rng = Rng::seed_from_u64(42);

        let mut world = World::new(pc);
        world.combat = Some(minimal_combat(&world_ref, entity, &mut rng));

        let original_highest = world.combat.as_ref().unwrap().queue[0].score;

        // Insert ICE for floor 3.
        let mut rng2 = Rng::seed_from_u64(99);
        insert_ice_in_queue(&mut world, 3, &mut rng2);

        let combat = world.combat.as_ref().unwrap();

        // ICE is at index 0 (top of queue). See p.205.
        assert_eq!(
            combat.queue[0].score,
            original_highest + 1,
            "ICE score must be previous highest + 1 (p.205)"
        );

        // The minted entity ID uses the floor-index scheme.
        const BLACK_ICE_MAGIC: u128 = 0x0000_424B_4943_0000_0000_0000_0000_0000;
        let expected_id = EntityId(Uuid::from_u128(BLACK_ICE_MAGIC | 3u128));
        assert_eq!(
            combat.queue[0].entity, expected_id,
            "ICE EntityId must encode the floor index"
        );

        // Participant set updated.
        assert!(
            combat.participants.contains(&expected_id),
            "ICE entity must be in participants"
        );
    }

    // ── test_unsafe_jackout_lying_in_wait_not_counted ────────────────────────

    /// ICE with [`BlackIceState::LyingInWait`] must NOT apply effects on
    /// unsafe jack-out — only `InCombat` ICE do.
    ///
    /// Rulebook p.198: "You suffer the effect of all remaining enemy Black ICE
    /// **you've encountered** …". "Encountered" means the ICE has been triggered
    /// (i.e., is `InCombat`), not merely lurking.
    ///
    /// RAW tension: the rulebook says "you've encountered" which could be
    /// interpreted as "seen" during the run, not necessarily InCombat. We
    /// default to `InCombat` = encountered and active, since LyingInWait ICE
    /// has not yet attacked the Netrunner. Flagged as a RAW deviation in PR.
    #[test]
    fn test_unsafe_jackout_lying_in_wait_not_counted() {
        let floors = vec![
            Floor::BlackIce {
                template: BlackIceId("scorpion".into()),
                state: BlackIceState::LyingInWait, // not yet encountered
                ice_per: 3,
            },
            Floor::BlackIce {
                template: BlackIceId("liche".into()),
                state: BlackIceState::InCombat, // encountered
                ice_per: 5,
            },
        ];

        let (mut world, _) = world_with_netrun(floors);
        let outcome = unsafe_jack_out(&mut world).unwrap();

        assert_eq!(
            outcome.unsafe_jackout_consequences.len(),
            1,
            "only InCombat ICE applies — LyingInWait does not count as encountered"
        );
        assert_eq!(
            outcome.unsafe_jackout_consequences[0].from_ice,
            BlackIceId("liche".into()),
            "only liche (InCombat) applies"
        );
    }
}
