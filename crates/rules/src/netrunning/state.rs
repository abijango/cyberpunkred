//! Active netrun state — what is happening right now inside a NET Architecture.
//!
//! [`NetrunState`] captures everything about an in-progress netrun: which
//! architecture, which floor the Netrunner is on, what programs are rezzed,
//! what control nodes they hold, and what viruses they have queued to leave
//! when they reach the bottom floor and jack out.
//!
//! ## Rulebook references
//!
//! - pp.197–200: NET Actions, Interface Abilities (Pathfinder, Control, Virus,
//!   Cloak), jack-in/jack-out semantics.
//! - p.198: Jacking Out resets defences; all programs leave with the Netrunner.
//! - p.199: Pathfinder reveals floors; Control seizes control nodes; Cloak
//!   hides traces; Virus is queued until jack-out from the bottom floor.
//! - p.200: Virus Effect examples (alter icons, deactivate ICE, malfunction
//!   control nodes, etc.).
//! - p.197 (NET Actions table, also found at p.144): Interface Rank → NET
//!   Actions per Turn mapping: 1–3 → 2, 4–6 → 3, 7–9 → 4, 10 → 5.
//!
//! ## Program instance IDs
//!
//! [`ProgramInstanceId`] wraps a [`uuid::Uuid`] produced **deterministically**
//! from an internal counter stored on [`NetrunState`]. Each call to
//! [`NetrunState::rez_program`] increments the counter and produces:
//!
//! ```text
//! Uuid::from_u128(0x0000_5250_474D_0000_0000_0000_0000_NNNN)
//! ```
//!
//! where `NNNN` is the little-endian counter value (u128). The magic prefix
//! `5250474D` is ASCII `RPGM` (for "rezzed program"), making hand-inspection
//! of save files easy. This approach avoids OS entropy (`uuid::v4`) so the
//! rules crate stays deterministic and WASM-compatible.

use crate::effects::ProgramId;
use crate::netrunning::architecture::NetArchId;
use crate::types::{EntityId, DV};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// ProgramInstanceId
// ---------------------------------------------------------------------------

/// A unique identifier for one activated ("rezzed") instance of a program.
///
/// Multiple copies of the same program can be rezzed simultaneously (p.201:
/// "You can run multiple copies of the same Program on your Cyberdeck if you
/// wish"). `ProgramInstanceId` distinguishes them.
///
/// IDs are minted deterministically by [`NetrunState::rez_program`] using an
/// incrementing counter — see the module-level docs for the encoding.
///
/// See pp.201–203 (Programs, Rezzed state).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProgramInstanceId(pub Uuid);

impl ProgramInstanceId {
    /// Magic prefix bytes in the UUID (positions 4–7, big-endian).
    /// ASCII "RPGM" — marks this as a rezzed-program instance ID.
    const MAGIC: u128 = 0x0000_5250_474D_0000_0000_0000_0000_0000;

    /// Mint a new `ProgramInstanceId` from a counter value.
    ///
    /// Encoding: `0x0000_5250_474D_0000_0000_0000_0000_NNNN` where `NNNN`
    /// is the counter as a `u128` in the low 64 bits (little-endian within
    /// the 128-bit integer). This is deterministic and needs no OS entropy.
    ///
    /// See module-level documentation for rationale.
    fn from_counter(counter: u64) -> Self {
        ProgramInstanceId(Uuid::from_u128(Self::MAGIC | counter as u128))
    }
}

// ---------------------------------------------------------------------------
// RezzedProgram
// ---------------------------------------------------------------------------

/// One program that is currently rezzed (activated) in an Architecture.
///
/// A program's REZ is its Hit Points while running (p.202: "REZ: The
/// Program's Hit Points, or the amount of damage it can sustain while
/// Rezzed before it is Derezzed"). `current_rez` starts at the program's
/// catalog `max_rez` value and decreases as it takes damage in NET combat.
///
/// See pp.201–203 (Programs) and p.201 (Defeating a Program).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RezzedProgram {
    /// Unique ID for this specific rezzed instance.
    pub instance_id: ProgramInstanceId,
    /// The program catalog slug (e.g. `"eraser"`, `"hellbolt"`).
    pub program: ProgramId,
    /// Current REZ (hit points). Starts at `max_rez` passed to
    /// [`NetrunState::rez_program`]. Reaches 0 when Derezzed.
    ///
    /// See p.201 (Defeating a Program) and p.202 (REZ column).
    pub current_rez: u8,
}

// ---------------------------------------------------------------------------
// Virus
// ---------------------------------------------------------------------------

/// A Virus queued to be installed in a NET Architecture.
///
/// Viruses are created at the bottom floor and persist in the Architecture
/// after the Netrunner jacks out. A Virus is the only way to make permanent
/// changes to an Architecture (p.200: "Using this ability is the only way a
/// Netrunner can make a change to a NET Architecture that persists after they
/// Jack Out").
///
/// See p.200 (Virus Interface Ability) and the Virus Examples sidebar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Virus {
    /// Human-readable description of what this Virus does.
    /// Example: "Changes all passwords every 5 minutes." See p.200.
    pub description: String,
    /// The structured effect this Virus applies to the Architecture.
    pub effect: VirusEffect,
    /// The DV the Netrunner must beat with an Interface check to install
    /// this Virus. Higher DV = more powerful Virus (p.200: "A more powerful
    /// Virus will require a higher DV to leave in the Architecture").
    pub dv_to_install: DV,
    /// How many NET Actions it costs to install this Virus. The GM sets this
    /// (p.200: "Depending on what you want to do, this can require as many
    /// NET Actions as the GM determines").
    pub net_actions_to_install: u8,
}

/// The structured effect a [`Virus`] applies to a NET Architecture.
///
/// Variants correspond to the example Viruses listed on p.200.
///
/// See p.200 (Virus Interface Ability, Virus Examples sidebar).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VirusEffect {
    /// Alter the icon of a Black ICE program (e.g. from "fierce serpent" to
    /// "cute sneks wearing tiny party hats"). The String names the ICE
    /// whose icon is changed.
    ///
    /// Example from p.200: DV6; 1 NET Action.
    AlterIcon(String),
    /// Completely deactivate a particular Black ICE installed in the
    /// Architecture until the Virus is destroyed. The String names the ICE.
    ///
    /// Example from p.200: DV10; 2 NET Actions.
    DeactivateIce(String),
    /// Cause a Control Node (identified by floor index) to malfunction
    /// until the Virus is destroyed.
    ///
    /// Example from p.200: DV10; 2 NET Actions.
    MalfunctionNode(usize),
    /// Any other Virus effect not covered by the above variants, described
    /// as a free-form string. This is the catch-all for GM-invented Viruses.
    ///
    /// See p.200: "Describe to the GM what you want the virus to do."
    Custom(String),
}

// ---------------------------------------------------------------------------
// NetrunState
// ---------------------------------------------------------------------------

/// All mutable state for a Netrunner currently jacked into a NET Architecture.
///
/// Created by [`NetrunState::start`] when a Netrunner uses the Jack In NET
/// Action (p.198). Dropped (replaced with `None` in `World.netrun`) when they
/// jack out, either safely or by leaving the access-point's range.
///
/// ## Floor indexing
///
/// Floors are 0-indexed from the top. `current_floor = 0` means the Netrunner
/// is at the Lobby (the topmost floor). `current_floor = revealed_floors - 1`
/// means they are at the deepest revealed floor. They cannot advance past
/// `revealed_floors - 1` without revealing more floors (Pathfinder ability,
/// p.199).
///
/// ## NET Actions per turn
///
/// The number of NET Actions available each turn is determined by the
/// Netrunner's Interface rank (p.197):
///
/// | Interface Rank | NET Actions |
/// |----------------|-------------|
/// | 1–3            | 2           |
/// | 4–6            | 3           |
/// | 7–9            | 4           |
/// | 10             | 5           |
///
/// WP-403 will expose a standalone `net_actions_per_turn(rank)` function;
/// this state struct embeds the computed value directly so each action
/// site does not need to recompute it.
///
/// See pp.197–200 (NET Actions, Interface Abilities), p.198 (Jack In/Out).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetrunState {
    /// The Netrunner entity (maps to a `Character` in `World::npcs` or
    /// `World::pc`). See `World::entity`.
    pub netrunner: EntityId,
    /// Which architecture is being netrun. Resolves into a `NetArchitecture`
    /// in the GM or scene layer.
    pub architecture: NetArchId,
    /// The floor the Netrunner is currently on. 0 = top (Lobby).
    ///
    /// Per p.198: "You can move as much as you want in a NET Architecture
    /// on your Turn." Movement between floors is free (not a NET Action)
    /// unless blocked by an obstruction.
    pub current_floor: usize,
    /// How many floors have been revealed by the Pathfinder ability (p.199).
    ///
    /// Starts at `1` (the Netrunner can see the Lobby floor they enter on).
    /// Pathfinder extends this. The Netrunner cannot advance past
    /// `revealed_floors - 1` (can_advance_floor returns false otherwise).
    pub revealed_floors: usize,
    /// Programs currently rezzed (activated) in this Architecture.
    ///
    /// All programs leave the Architecture with the Netrunner on Jack Out
    /// (p.198: "All your Programs leave the Architecture with you when
    /// you Jack Out").
    pub rezzed_programs: Vec<RezzedProgram>,
    /// Floor indices of Control Nodes currently held by this Netrunner.
    ///
    /// The Netrunner loses all held control nodes when they jack out
    /// (p.199: "You lose control of any Control Nodes you hold in an
    /// Architecture when you Jack Out"). [`release_all_control_nodes`]
    /// enforces this at jack-out.
    pub controlled_nodes: Vec<usize>,
    /// Viruses queued to be left in the Architecture upon jack-out from
    /// the bottom floor.
    ///
    /// Viruses are the only mechanism for persistent post-jack-out changes
    /// (p.200). They are drained via [`drain_viruses_for_jackout`], which
    /// the jack-out resolution code calls when the Netrunner exits from the
    /// bottom floor.
    pub queued_viruses: Vec<Virus>,
    /// The DV a hostile Pathfinder check must beat to detect this Netrunner's
    /// Cloak (p.199: "Pathfinder DV … is equal to the Cloak Check"). `None`
    /// if Cloak is not active.
    pub cloak_dv: Option<DV>,
    /// How many NET Actions this Netrunner has consumed this turn.
    /// Reset to 0 by [`reset_turn`].
    pub net_actions_used_this_turn: u8,
    /// Maximum NET Actions this Netrunner can take this turn.
    /// Set by [`start`] and updated by [`reset_turn`] based on Interface rank.
    ///
    /// See p.197 (NET Actions table).
    pub net_actions_max_this_turn: u8,
    /// Monotonically incrementing counter used to mint deterministic
    /// [`ProgramInstanceId`]s. Never exposed publicly — mint via
    /// [`rez_program`] only.
    #[serde(default)]
    program_id_counter: u64,
}

impl NetrunState {
    /// Begin a new netrun.
    ///
    /// Called when a Netrunner succeeds at the Jack In NET Action within
    /// 6 m/yds of an access point (p.198). Sets up an empty state with one
    /// revealed floor (the Lobby) and computes the initial NET Action budget
    /// from `interface_rank`.
    ///
    /// # Arguments
    ///
    /// - `netrunner` — the entity that is jacking in.
    /// - `architecture` — the target NET Architecture.
    /// - `interface_rank` — the Netrunner's current Interface ability rank
    ///   (1–10). Determines `net_actions_max_this_turn` per p.197.
    ///
    /// See p.198 (Jack In/Out), p.197 (NET Actions table).
    pub fn start(netrunner: EntityId, architecture: NetArchId, interface_rank: u8) -> Self {
        NetrunState {
            netrunner,
            architecture,
            current_floor: 0,
            revealed_floors: 1,
            rezzed_programs: Vec::new(),
            controlled_nodes: Vec::new(),
            queued_viruses: Vec::new(),
            cloak_dv: None,
            net_actions_used_this_turn: 0,
            net_actions_max_this_turn: net_actions_for_rank(interface_rank),
            program_id_counter: 0,
        }
    }

    /// Returns `true` if the Netrunner can move to the next deeper floor.
    ///
    /// The Netrunner can advance only when their current floor is strictly
    /// less than the number of revealed floors minus one — i.e. there is at
    /// least one revealed floor below their current position.
    ///
    /// Pathfinder (p.199) is the ability that increases `revealed_floors`.
    ///
    /// See p.199 (Pathfinder), p.198 (Moving in a NET Architecture).
    pub fn can_advance_floor(&self) -> bool {
        self.current_floor < self.revealed_floors.saturating_sub(1)
    }

    /// Rez (activate) a program, giving it `max_rez` hit points.
    ///
    /// Activating a program costs one NET Action (p.198: "Activate/Deactivate
    /// Program — Activate or Deactivate one of your Programs"). This method
    /// only mutates state; the caller is responsible for spending the NET
    /// Action token.
    ///
    /// Returns the fresh [`ProgramInstanceId`] assigned to this activation,
    /// which the caller must retain to derez the program later.
    ///
    /// See p.201 (Programs, Defeating a Program), p.202 (REZ column).
    pub fn rez_program(&mut self, program: ProgramId, max_rez: u8) -> ProgramInstanceId {
        self.program_id_counter += 1;
        let instance_id = ProgramInstanceId::from_counter(self.program_id_counter);
        self.rezzed_programs.push(RezzedProgram {
            instance_id: instance_id.clone(),
            program,
            current_rez: max_rez,
        });
        instance_id
    }

    /// Derez (remove) a program by its instance ID.
    ///
    /// Returns the [`RezzedProgram`] entry if found, or `None` if no program
    /// with that instance ID is rezzed. A program is explicitly derezzed by
    /// the player (Deactivate, costing one NET Action) or implicitly when
    /// its REZ drops to 0 (p.201: "A Program is Derezzed when it is lowered
    /// to 0 REZ").
    ///
    /// See p.201 (Defeating a Program, Activate/Deactivate Program).
    pub fn derez_program(&mut self, instance: ProgramInstanceId) -> Option<RezzedProgram> {
        if let Some(pos) = self
            .rezzed_programs
            .iter()
            .position(|p| p.instance_id == instance)
        {
            Some(self.rezzed_programs.swap_remove(pos))
        } else {
            None
        }
    }

    /// Mark a Control Node floor as held by this Netrunner.
    ///
    /// Does not deduplicate — callers should check `controlled_nodes.contains`
    /// before calling if idempotency matters. Floor index must be a valid
    /// `Floor::ControlNode` in the architecture; validation is the caller's
    /// responsibility.
    ///
    /// See p.199 (Control Interface Ability): "Allows you to control things
    /// attached to the NET Architectures like cameras, drones, turrets…"
    pub fn take_control_node(&mut self, floor_idx: usize) {
        self.controlled_nodes.push(floor_idx);
    }

    /// Release all held Control Nodes.
    ///
    /// Must be called on jack-out (p.199: "You lose control of any Control
    /// Nodes you hold in an Architecture when you Jack Out"). Also call when
    /// the Netrunner is forcibly ejected (unsafe jack-out).
    ///
    /// See p.199 (Control), p.198 (Jack In/Out).
    pub fn release_all_control_nodes(&mut self) {
        self.controlled_nodes.clear();
    }

    /// Queue a Virus to be installed when jacking out from the bottom floor.
    ///
    /// The Virus interface ability requires the Netrunner to be at the lowest
    /// floor of the Architecture before they can install a Virus (p.200:
    /// "Once you have reached the lowest level of the NET Architecture you
    /// can leave your own Virus in the Architecture"). This method just
    /// enqueues — the jack-out resolution code calls [`drain_viruses_for_jackout`]
    /// to consume the queue.
    ///
    /// See p.200 (Virus Interface Ability).
    pub fn queue_virus(&mut self, virus: Virus) {
        self.queued_viruses.push(virus);
    }

    /// Drain the queued Virus list on jack-out.
    ///
    /// Returns all queued [`Virus`]es (the caller is responsible for
    /// installing them into the Architecture's persistent state) and leaves
    /// `queued_viruses` empty. If the Netrunner did not reach the bottom
    /// floor, the caller should discard these instead of installing them.
    ///
    /// See p.200 (Virus), p.198 (Jacking Out resets defences; Virus
    /// requires reaching the bottom floor to persist).
    pub fn drain_viruses_for_jackout(&mut self) -> Vec<Virus> {
        std::mem::take(&mut self.queued_viruses)
    }

    /// Reset NET Action accounting for a new turn.
    ///
    /// Sets `net_actions_used_this_turn` to 0 and recomputes
    /// `net_actions_max_this_turn` from `interface_rank`. Recomputing on
    /// each turn reset allows the max to change mid-netrun (e.g. if a Virus
    /// reduces the Netrunner's NET Actions via Vrizzbolt's effect, p.204).
    ///
    /// The `interface_rank → net_actions_max` mapping per p.197:
    ///
    /// | Interface Rank | NET Actions |
    /// |----------------|-------------|
    /// | 1–3            | 2           |
    /// | 4–6            | 3           |
    /// | 7–9            | 4           |
    /// | 10             | 5           |
    ///
    /// WP-403 will expose this as a standalone public function
    /// `net_actions_per_turn(rank)` to avoid duplication. This call site will
    /// delegate to it once WP-403 lands.
    ///
    /// See p.197 (NET Actions table), p.198 (each Turn).
    pub fn reset_turn(&mut self, interface_rank: u8) {
        self.net_actions_used_this_turn = 0;
        self.net_actions_max_this_turn = net_actions_for_rank(interface_rank);
    }
}

// ---------------------------------------------------------------------------
// Internal helper: Interface rank → NET Actions
// ---------------------------------------------------------------------------

/// Map an Interface rank to the number of NET Actions per turn.
///
/// Per p.197 (NET Actions table). WP-403 will expose an identical public
/// function `net_actions_per_turn(rank) -> u8` and both sites will then
/// refer to that single source of truth. For now we embed the table here
/// to keep WP-402 self-contained.
///
/// | Interface Rank | NET Actions |
/// |----------------|-------------|
/// | 1–3            | 2           |
/// | 4–6            | 3           |
/// | 7–9            | 4           |
/// | 10+            | 5           |
///
/// Rank 0 is treated as rank 1 (minimum 2 actions) since Interface is at
/// minimum rank 1 to Netrun at all (p.197: "Without it, you cannot Netrun").
///
/// See p.197 (NET Actions table).
fn net_actions_for_rank(rank: u8) -> u8 {
    match rank {
        0..=3 => 2,
        4..=6 => 3,
        7..=9 => 4,
        _ => 5, // rank 10+
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_entity(n: u128) -> EntityId {
        EntityId(Uuid::from_u128(n))
    }

    fn make_arch(s: &str) -> NetArchId {
        NetArchId(s.to_string())
    }

    fn make_program(s: &str) -> ProgramId {
        ProgramId(s.to_string())
    }

    /// `test_netrun_state_initializes_correctly`:
    /// `start()` must populate all fields with the expected initial values.
    ///
    /// Verified against: p.198 (jack-in starts at top, no programs rezzed),
    /// p.197 (Interface rank 4 → 3 NET Actions).
    #[test]
    fn test_netrun_state_initializes_correctly() {
        let entity = make_entity(0xAB);
        let arch = make_arch("test-arch");
        let state = NetrunState::start(entity, arch.clone(), 5); // rank 5 → 3 actions

        assert_eq!(state.netrunner, entity);
        assert_eq!(state.architecture, arch);
        assert_eq!(state.current_floor, 0, "start at floor 0 (Lobby)");
        assert_eq!(state.revealed_floors, 1, "one floor revealed at start");
        assert!(
            state.rezzed_programs.is_empty(),
            "no programs rezzed at start"
        );
        assert!(state.controlled_nodes.is_empty(), "no nodes held at start");
        assert!(
            state.queued_viruses.is_empty(),
            "no viruses queued at start"
        );
        assert_eq!(state.cloak_dv, None, "Cloak inactive at start");
        assert_eq!(
            state.net_actions_used_this_turn, 0,
            "zero actions used at start"
        );
        assert_eq!(
            state.net_actions_max_this_turn, 3,
            "rank 5 → 3 NET Actions per p.197"
        );
    }

    /// `test_can_advance_floor_when_revealed`:
    /// `can_advance_floor` returns `true` iff `revealed_floors > current_floor + 1`.
    ///
    /// See p.199 (Pathfinder reveals floors).
    #[test]
    fn test_can_advance_floor_when_revealed() {
        let entity = make_entity(0x01);
        let arch = make_arch("arch-a");
        let mut state = NetrunState::start(entity, arch, 3);

        // At start: current_floor=0, revealed_floors=1 → cannot advance.
        assert!(
            !state.can_advance_floor(),
            "cannot advance when current_floor == revealed_floors - 1"
        );

        // Reveal a second floor via Pathfinder → can advance.
        state.revealed_floors = 2;
        assert!(
            state.can_advance_floor(),
            "can advance when revealed_floors > current_floor + 1"
        );

        // Advance; now at floor 1, revealed=2 → cannot advance again.
        state.current_floor = 1;
        assert!(
            !state.can_advance_floor(),
            "cannot advance again at deepest revealed floor"
        );

        // Reveal a third floor → can advance once more.
        state.revealed_floors = 3;
        assert!(
            state.can_advance_floor(),
            "can advance after Pathfinder reveals more"
        );
    }

    /// `test_rez_and_derez_program`:
    /// `rez_program` adds a `RezzedProgram`; `derez_program` removes it and
    /// returns the correct entry; derezzing an unknown ID returns `None`.
    ///
    /// See p.201 (Programs, Defeating a Program).
    #[test]
    fn test_rez_and_derez_program() {
        let entity = make_entity(0x02);
        let arch = make_arch("arch-b");
        let mut state = NetrunState::start(entity, arch, 6);

        // Rez a program.
        let prog_id = make_program("eraser");
        let instance_id = state.rez_program(prog_id.clone(), 7);

        assert_eq!(state.rezzed_programs.len(), 1, "one program rezzed");
        assert_eq!(state.rezzed_programs[0].program, prog_id);
        assert_eq!(state.rezzed_programs[0].current_rez, 7);
        assert_eq!(state.rezzed_programs[0].instance_id, instance_id);

        // Derez it — round-trip.
        let removed = state.derez_program(instance_id.clone());
        assert!(removed.is_some(), "derez must return the program entry");
        let removed = removed.unwrap();
        assert_eq!(removed.program, prog_id);
        assert_eq!(removed.current_rez, 7);
        assert_eq!(removed.instance_id, instance_id);
        assert!(
            state.rezzed_programs.is_empty(),
            "list must be empty after derez"
        );

        // Derezzing a missing ID returns None — no panic.
        let missing = state.derez_program(instance_id.clone());
        assert_eq!(missing, None, "derez of missing ID must return None");
    }

    /// `test_rez_program_gives_unique_ids`:
    /// Repeated calls to `rez_program` produce distinct instance IDs.
    #[test]
    fn test_rez_program_gives_unique_ids() {
        let entity = make_entity(0x03);
        let arch = make_arch("arch-c");
        let mut state = NetrunState::start(entity, arch, 4);

        let id_a = state.rez_program(make_program("eraser"), 7);
        let id_b = state.rez_program(make_program("eraser"), 7);
        let id_c = state.rez_program(make_program("flak"), 7);

        assert_ne!(
            id_a, id_b,
            "two rezzed copies of the same program get distinct IDs"
        );
        assert_ne!(id_b, id_c, "different programs also get distinct IDs");
        assert_ne!(id_a, id_c);
    }

    /// `test_take_and_release_control_nodes`:
    /// Taking 3 nodes populates `controlled_nodes`; `release_all` empties it.
    ///
    /// See p.199 (Control) and p.198 (Jack Out releases all nodes).
    #[test]
    fn test_take_and_release_control_nodes() {
        let entity = make_entity(0x04);
        let arch = make_arch("arch-d");
        let mut state = NetrunState::start(entity, arch, 7);

        state.take_control_node(2);
        state.take_control_node(5);
        state.take_control_node(8);

        assert_eq!(
            state.controlled_nodes,
            vec![2, 5, 8],
            "three control nodes recorded"
        );

        state.release_all_control_nodes();
        assert!(
            state.controlled_nodes.is_empty(),
            "all nodes released on jack-out"
        );
    }

    /// `test_queue_and_drain_viruses`:
    /// Queue 3 viruses; drain returns all 3 and leaves the queue empty.
    ///
    /// See p.200 (Virus Interface Ability).
    #[test]
    fn test_queue_and_drain_viruses() {
        let entity = make_entity(0x05);
        let arch = make_arch("arch-e");
        let mut state = NetrunState::start(entity, arch, 8);

        let v1 = Virus {
            description: "Alter Asp icon".into(),
            effect: VirusEffect::AlterIcon("asp".into()),
            dv_to_install: DV(6),
            net_actions_to_install: 1,
        };
        let v2 = Virus {
            description: "Deactivate Hellhound".into(),
            effect: VirusEffect::DeactivateIce("hellhound".into()),
            dv_to_install: DV(10),
            net_actions_to_install: 2,
        };
        let v3 = Virus {
            description: "Malfunction camera node".into(),
            effect: VirusEffect::MalfunctionNode(3),
            dv_to_install: DV(10),
            net_actions_to_install: 2,
        };

        state.queue_virus(v1.clone());
        state.queue_virus(v2.clone());
        state.queue_virus(v3.clone());
        assert_eq!(state.queued_viruses.len(), 3, "three viruses queued");

        let drained = state.drain_viruses_for_jackout();
        assert_eq!(drained.len(), 3, "drain returns all three viruses");
        assert_eq!(drained[0], v1);
        assert_eq!(drained[1], v2);
        assert_eq!(drained[2], v3);
        assert!(
            state.queued_viruses.is_empty(),
            "queue is empty after drain"
        );
    }

    /// `test_reset_turn_resets_actions_used`:
    /// After using some actions, `reset_turn(5)` → used=0, max=3 (rank 5).
    ///
    /// See p.197 (NET Actions table), p.198 (each Turn).
    #[test]
    fn test_reset_turn_resets_actions_used() {
        let entity = make_entity(0x06);
        let arch = make_arch("arch-f");
        let mut state = NetrunState::start(entity, arch, 5); // rank 5 → max 3

        // Simulate having spent actions.
        state.net_actions_used_this_turn = 2;

        state.reset_turn(5);
        assert_eq!(state.net_actions_used_this_turn, 0, "used reset to 0");
        assert_eq!(
            state.net_actions_max_this_turn, 3,
            "rank 5 → max 3 per p.197"
        );
    }

    /// `test_net_actions_per_interface_rank_matches_table`:
    /// Verify rank 1, 4, 7, 10 against the NET Actions table on p.197.
    ///
    /// Table (p.197):
    /// | Interface Rank | 1-3 | 4-6 | 7-9 | 10 |
    /// | NET Actions    |  2  |  3  |  4  |  5 |
    #[test]
    fn test_net_actions_per_interface_rank_matches_table() {
        // rank 1 → 2 actions
        assert_eq!(
            net_actions_for_rank(1),
            2,
            "rank 1 must give 2 NET Actions (p.197)"
        );
        // rank 4 → 3 actions
        assert_eq!(
            net_actions_for_rank(4),
            3,
            "rank 4 must give 3 NET Actions (p.197)"
        );
        // rank 7 → 4 actions
        assert_eq!(
            net_actions_for_rank(7),
            4,
            "rank 7 must give 4 NET Actions (p.197)"
        );
        // rank 10 → 5 actions
        assert_eq!(
            net_actions_for_rank(10),
            5,
            "rank 10 must give 5 NET Actions (p.197)"
        );

        // Spot-check additional ranks.
        assert_eq!(net_actions_for_rank(2), 2, "rank 2 → 2");
        assert_eq!(net_actions_for_rank(3), 2, "rank 3 → 2");
        assert_eq!(net_actions_for_rank(5), 3, "rank 5 → 3");
        assert_eq!(net_actions_for_rank(6), 3, "rank 6 → 3");
        assert_eq!(net_actions_for_rank(8), 4, "rank 8 → 4");
        assert_eq!(net_actions_for_rank(9), 4, "rank 9 → 4");
    }

    /// `test_program_instance_id_deterministic`:
    /// The same counter always produces the same `ProgramInstanceId`.
    #[test]
    fn test_program_instance_id_deterministic() {
        let id_a = ProgramInstanceId::from_counter(1);
        let id_b = ProgramInstanceId::from_counter(1);
        let id_c = ProgramInstanceId::from_counter(2);
        assert_eq!(id_a, id_b, "same counter → same ID");
        assert_ne!(id_a, id_c, "different counter → different ID");
    }

    /// Serialise and deserialise a full `NetrunState` to verify RON
    /// round-trip compatibility.
    #[test]
    fn test_netrun_state_ron_round_trip() {
        let entity = make_entity(0x99);
        let arch = make_arch("arasaka-lvl2");
        let mut state = NetrunState::start(entity, arch, 9); // rank 9 → 4 actions

        // Add some data.
        let id = state.rez_program(make_program("armor"), 7);
        state.take_control_node(3);
        state.queue_virus(Virus {
            description: "Custom virus".into(),
            effect: VirusEffect::Custom("halve floor count".into()),
            dv_to_install: DV(12),
            net_actions_to_install: 10,
        });
        state.cloak_dv = Some(DV(16));
        state.net_actions_used_this_turn = 2;
        state.current_floor = 1;
        state.revealed_floors = 3;

        let serialised =
            ron::ser::to_string(&state).expect("NetrunState serialisation must succeed");
        let restored: NetrunState =
            ron::de::from_str(&serialised).expect("NetrunState deserialisation must succeed");

        assert_eq!(state, restored, "RON round-trip must be identity");

        // ProgramInstanceId survives round-trip.
        assert_eq!(
            restored.rezzed_programs[0].instance_id, id,
            "instance ID must survive serialisation"
        );
    }
}
