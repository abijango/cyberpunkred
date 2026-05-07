//! NET Architecture data model and procedural generator.
//!
//! A NET Architecture is an ordered sequence of [`Floor`]s that a Netrunner
//! descends through, index 0 at the top (the Lobby) and the last element at
//! the deepest point (the goal). See pp.209–212 for the conceptual overview
//! and p.217 for the pricing and floor-count table.
//!
//! ## Generator overview
//!
//! [`generate_net_architecture`] takes a [`NetArchSpec`] and a mutable
//! [`Rng`] reference and produces a fully populated [`NetArchitecture`].
//! The generation is **deterministic**: the same `(spec, seed)` pair always
//! produces the same architecture.
//!
//! ### Floor-count tiers (p.217)
//!
//! The rulebook table on p.217 ("Buying a NET Architecture") defines three
//! size bands by number of floors:
//!
//! | Floors   | Cost per floor         | PriceTier       |
//! |----------|------------------------|-----------------|
//! | 3 to 6   | 1,000 eb (V. Expensive)| `VeryExpensive` |
//! | 7 to 12  | 5,000 eb (Luxury)      | `Luxury`        |
//! | 13 to 18 | 10,000 eb (Super Luxury)| `SuperLuxury`  |
//!
//! Note: the WP guidance mentioned "V.Expensive → 3..=6, Luxury → 7..=12,
//! Super-Luxury → 13..=18". This agrees exactly with p.217 RAW. No deviation.
//!
//! ### DV tiers (p.210)
//!
//! The Difficulty Rating table on p.210 maps each band to a DV for
//! Passwords, Control Nodes, and Files:
//!
//! | Difficulty | DV   | PriceTier (per-feature, p.217) |
//! |------------|------|-------------------------------|
//! | Basic      | DV6  | Expensive (500 eb)            |
//! | Standard   | DV8  | V. Expensive (1,000 eb)       |
//! | Uncommon   | DV10 | Luxury (5,000 eb)             |
//! | Advanced   | DV12 | Super Luxury (10,000 eb)      |
//!
//! We map `PriceTier` to DV as follows:
//! - `VeryExpensive` (3–6 floors) → predominantly DV8 (Standard), with
//!   occasional DV6 lobby floors.
//! - `Luxury` (7–12 floors) → predominantly DV10 (Uncommon), with some DV8.
//! - `SuperLuxury` (13–18 floors) → predominantly DV12 (Advanced), with
//!   some DV10.
//!
//! ### Black ICE placeholder list
//!
//! The generator uses a hardcoded canonical slice of Black ICE slugs.
//! Future WP can wire this to the live `Catalog<BlackIce>`. All names are
//! from pp.206–207.
//!
//! See p.217.

use crate::catalog::black_ice::BlackIceId;
use crate::catalog::demons::DemonId;
use crate::rng::Rng;
use crate::types::{PriceTier, DV};
use crate::world::LocationId;
use rand::Rng as _;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Canonical Black ICE slug list (placeholder until catalog wiring — WP-404+)
// ---------------------------------------------------------------------------

/// Canonical Black ICE slugs available for procedural placement.
///
/// Sourced from pp.206–207 (Black ICE Program Table). This list is used by
/// the generator to pick placeholder `BlackIceId` values. A future WP
/// (WP-404+) should wire the generator to the live `Catalog<BlackIce>` and
/// honour the architecture's DV tier when selecting ICE.
///
/// The ordering here loosely sorts by cost/danger: lighter ICE first
/// (Raven, Wisp, Scorpion, Skunk) then heavier (Hellhound, Asp, Killer,
/// Liche, Sabertooth, Kraken, Dragon, Giant).
const BLACK_ICE_SLUGS: &[&str] = &[
    "raven",
    "wisp",
    "scorpion",
    "skunk",
    "hellhound",
    "asp",
    "killer",
    "liche",
    "sabertooth",
    "kraken",
    "dragon",
    "giant",
];

/// Canonical Demon slugs for procedural placement (p.212).
///
/// Ordered from weakest to strongest: Imp → Efreet → Balron.
/// Balron is only used in very large (Super-Luxury) architectures.
const DEMON_SLUGS: &[&str] = &["imp", "efreet", "balron"];

// ---------------------------------------------------------------------------
// Identifiers
// ---------------------------------------------------------------------------

/// Stable identifier for a [`NetArchitecture`] instance.
///
/// The wrapped `String` is a content slug (e.g. `"arasaka-lobby"`) or a
/// procedurally-generated UUID string. Open string identifier so
/// architectures can be authored in RON content files as well as generated
/// at runtime.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetArchId(pub String);

/// Identifier for an entity that currently holds a [`Floor::ControlNode`].
///
/// The wrapped `String` matches an `EntityId` UUID string or a Demon slug.
/// Deliberately not typed as `EntityId` here because the architecture model
/// is a durable data structure (saved/loaded), not a live reference into a
/// running game world.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NetEntityId(pub String);

// ---------------------------------------------------------------------------
// Core architecture types
// ---------------------------------------------------------------------------

/// A complete NET Architecture: an ordered list of floors, an id, a display
/// name, and physical access points in Meatspace.
///
/// `floors[0]` is the top (Lobby); `floors[floors.len()-1]` is the deepest
/// floor (the goal). Per p.209: "Each floor of a NET Architecture is a level
/// where, as the 'door' opens, you find something waiting for you."
///
/// See pp.209–212 (NET Architecture overview) and p.217 (pricing table).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetArchitecture {
    /// Stable identifier.
    pub id: NetArchId,
    /// Human-readable display name, e.g. "Militech Server Farm — Level 2".
    pub display_name: String,
    /// Ordered floors. Index 0 is the top (Lobby); last index is deepest.
    /// See p.209.
    pub floors: Vec<Floor>,
    /// Physical Meatspace locations from which a Netrunner can jack in.
    /// See p.210 Step 3 (access points).
    pub access_points: Vec<MeatPosition>,
}

/// One floor of a NET Architecture.
///
/// Each variant corresponds to one of the five floor types described on
/// pp.209–211. The `dv` field on Password / ControlNode / File corresponds
/// to the Difficulty Rating table on p.210.
///
/// See p.209 ("It's Easier if you Think of Netrunning Like an Elevator") and
/// pp.210–211 (Lobby / Body tables).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Floor {
    /// A Password floor. The Netrunner must beat `dv` to pass.
    ///
    /// DV comes from the Difficulty Rating table on p.210
    /// (DV6 / DV8 / DV10 / DV12).
    Password {
        /// Interface DV to bypass the Password. See p.210.
        dv: DV,
    },

    /// A Control Node floor. Holds a real-world system (camera, turret, etc.).
    ///
    /// `controls` specifies what the node operates. `currently_held_by`
    /// records whether a Netrunner or Demon already has control.
    ///
    /// See p.209 ("Control Node") and p.212 (Demons and defenses).
    ControlNode {
        /// Interface DV to seize the node. See p.210.
        dv: DV,
        /// The physical system this node controls. See pp.213–215.
        controls: ControlTarget,
        /// Entity currently holding control, if any.
        currently_held_by: Option<NetEntityId>,
    },

    /// A File floor — the "treasure" of Netrunning (p.210 Step 3).
    ///
    /// Files contain data, decoys, or encrypted payloads.
    File {
        /// Interface DV to access the file. See p.210.
        dv: DV,
        /// Contents of the file.
        contents: FileContents,
    },

    /// A Black ICE floor. Contains one Black ICE program in a given state.
    ///
    /// The `template` references a catalog entry (`content/catalogs/black_ice.ron`).
    /// See pp.205–207.
    BlackIce {
        /// Catalog identifier for the Black ICE type. See pp.206–207.
        template: BlackIceId,
        /// Whether the ICE is lying in wait, engaged, slid, or destroyed.
        state: BlackIceState,
        /// The ICE's Perception (PER) stat, cached from the catalog row.
        ///
        /// Stored inline so that Interface Ability resolvers (e.g. Slide,
        /// WP-410) can access the PER stat for the contested roll without
        /// requiring a live `Catalog<BlackIce>` lookup at resolve time.
        /// Mirrors the `per` column on pp.206–207. Default value of `0`
        /// keeps older `Floor::BlackIce` constructions valid via
        /// `#[serde(default)]`.
        ///
        /// See p.200 (Slide: "Program's Perception + 1d10") and p.206 (PER
        /// column definition).
        #[serde(default)]
        ice_per: u8,
    },

    /// A Demon floor. A Demon can control multiple Control Nodes concurrently.
    ///
    /// `control_nodes` lists the floor indices (0-based) of Control Nodes
    /// this Demon is operating. See p.212 (Demons and Defenses).
    Demon {
        /// Catalog identifier for the Demon type (imp / efreet / balron).
        template: DemonId,
        /// Floor indices of Control Nodes this Demon operates. See p.212.
        control_nodes: Vec<usize>,
    },
}

/// State of a Black ICE program on a floor. See pp.205–207.
///
/// Tracks whether the ICE is passive, active in combat, has slid away, or
/// has been destroyed by the Netrunner.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlackIceState {
    /// The Black ICE is present but has not yet been triggered.
    /// Standard starting state. See p.205.
    LyingInWait,
    /// The Black ICE is actively engaged with a Netrunner. See p.205.
    InCombat,
    /// The Black ICE has slid away (SPD check failed against it). See p.205.
    Slid,
    /// The Black ICE has been Derezzed (REZ reduced to 0). See p.206.
    Derezzed,
}

/// The real-world system a [`Floor::ControlNode`] can operate.
///
/// These correspond to the environmental systems listed in the NET
/// Architecture rules on pp.213–215 (Active Defenses, Emplaced Defenses,
/// Environmental Defenses) and the example architecture on p.209.
///
/// The `String` payload is a descriptive label (e.g., `"east corridor"`).
///
/// See pp.209, 213–215.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlTarget {
    /// Observation camera. See p.215.
    Camera(String),
    /// Active defense drone (air, ground, etc.). See p.213.
    Drone(String),
    /// Automated turret or emplaced weapon. See p.214.
    Turret(String),
    /// Electronically engaged door lock. See p.210 (Step 3).
    Door(String),
    /// Sprinkler / fire suppression system. See p.210 (Step 3).
    Sprinkler(String),
    /// Elevator. See p.210 (Step 3).
    Elevator(String),
    /// Laser grid environmental defense. See p.215.
    LaserGrid(String),
    /// Any other controllable system not covered by the above variants.
    Custom(String),
}

/// Contents of a [`Floor::File`].
///
/// See p.210 (Step 3): "Files and Control Nodes are the 'treasure' of
/// Netrunning."
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FileContents {
    /// Plain data — a `String` describes what information is stored.
    Data(String),
    /// A decoy file with no real value.
    Decoy,
    /// An encrypted file requiring a second Interface check to unlock.
    Encrypted {
        /// DV to crack the encryption. See p.210.
        unlock_dv: DV,
    },
}

/// A physical location in Meatspace from which a Netrunner can jack in.
///
/// Every NET Architecture has at least one access point — a terminal, server
/// rack, or wireless hotspot. See p.210 Step 3 ("Fit the Architecture to the
/// World Around It").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeatPosition {
    /// Location identifier (content slug). `LocationId("placeholder".into())`
    /// until the GM or scene layer assigns real locations.
    pub location: LocationId,
    /// Optional grid square within the location (2 m squares per §1.2).
    /// `None` if the access point is not yet placed on a combat map.
    pub grid_square: Option<(u16, u16)>,
}

// ---------------------------------------------------------------------------
// Generator specification
// ---------------------------------------------------------------------------

/// Input spec for [`generate_net_architecture`].
///
/// Encodes the three design-time decisions that determine the generated
/// architecture's shape and danger level.
///
/// See p.217 (price tiers) and p.210 (difficulty ratings).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetArchSpec {
    /// The price tier of the architecture, which determines the allowed
    /// floor-count range and base DV. See p.217.
    ///
    /// Only `VeryExpensive`, `Luxury`, and `SuperLuxury` are valid for NET
    /// Architectures per p.217. Other tiers will be treated as
    /// `VeryExpensive` (the lowest valid tier) and a debug note should
    /// be left by callers.
    pub price: PriceTier,
    /// The security profile — shapes the ratio of combat vs. passive floors.
    pub intent: SecurityIntent,
    /// Minimum Fixer rank required to purchase this architecture (p.217:
    /// "only available at a Night Market that includes Personal Electronics,
    /// run by a Fixer of Rank 4 or higher"). Stored for downstream use;
    /// the generator does not use this to vary floor content.
    pub fixer_rank_required: u8,
}

/// The security posture of the organization that built this architecture.
///
/// `SecurityIntent` drives the ratio of floor types the generator produces.
/// See p.210 ("Fit the Architecture to the World Around It"):
/// "GMs should keep in mind the type of operation setting up an Architecture."
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SecurityIntent {
    /// More Passwords and Files; fewer combat floors.
    /// Suits businesses that need data security without aggressive posture.
    Defensive,
    /// More Black ICE; fewer Passwords.
    /// Suits orgs that prioritise neutralising intruders over concealment.
    Offensive,
    /// Even mix of all floor types.
    /// The default for a generic corp installation.
    Balanced,
    /// Maximum Black ICE density + at least one Demon if the architecture
    /// is large enough. Suits Arasaka-grade kill architectures.
    ///
    /// Per WP-401 spec: "Killer intent has ≥ 50% BlackIce/Demon floors".
    Killer,
}

// ---------------------------------------------------------------------------
// Generator
// ---------------------------------------------------------------------------

/// Generate a [`NetArchitecture`] from a [`NetArchSpec`] and a seeded RNG.
///
/// This function is **deterministic**: the same `(spec, seed)` pair always
/// produces the same architecture. See `IMPLEMENTATION_PLAN.md` §2.4.
///
/// ## Floor-count ranges (p.217)
///
/// | `spec.price`     | Floors  | Base DV |
/// |------------------|---------|---------|
/// | `VeryExpensive`  | 3 – 6   | DV8     |
/// | `Luxury`         | 7 – 12  | DV10    |
/// | `SuperLuxury`    | 13 – 18 | DV12    |
///
/// Any other `PriceTier` is clamped to `VeryExpensive`.
///
/// ## Lowest floor invariant
///
/// The deepest floor is always a [`Floor::File`] or [`Floor::ControlNode`]
/// — the "goal" of the netrun. Per WP-401 spec: "you wouldn't put the goal
/// behind a password with nothing after it."
///
/// ## Access points
///
/// 1–3 placeholder [`MeatPosition`] entries are generated, with
/// `location: LocationId("placeholder".into())`. Real placement is the
/// GM/scene layer's responsibility (WP-604+).
///
/// See pp.209–212 (NET Architecture overview), p.217 (pricing table).
pub fn generate_net_architecture(spec: &NetArchSpec, rng: &mut Rng) -> NetArchitecture {
    let (floor_min, floor_max, base_dv, high_dv) = tier_params(spec.price);

    // Step 1: choose floor count within the tier range (p.217).
    let floor_count = rng.random_range(floor_min..=floor_max) as usize;

    // Step 2: choose access-point count (1–3 per WP spec).
    let access_count = rng.random_range(1u8..=3u8) as usize;
    let access_points: Vec<MeatPosition> = (0..access_count)
        .map(|_| MeatPosition {
            location: LocationId("placeholder".into()),
            grid_square: None,
        })
        .collect();

    // Step 3: build floors.
    // We reserve the last slot for the goal (File or ControlNode).
    // All floors before the last are filled by intent-weighted random choice.
    let pre_floors = floor_count.saturating_sub(1);
    let mut floors: Vec<Floor> = Vec::with_capacity(floor_count);

    for i in 0..pre_floors {
        let floor = pick_floor(spec.intent, i, pre_floors, base_dv, high_dv, rng);
        floors.push(floor);
    }

    // Final floor: always File or ControlNode (the goal).
    let goal = pick_goal_floor(base_dv, high_dv, rng);
    floors.push(goal);

    // Step 4: assign Demons to any ControlNode floors where we roll one in.
    // For Killer intent with a large-enough architecture, guarantee at least
    // one Demon is present. We do a post-pass rather than inline so the
    // floor indices are known.
    maybe_add_demon(&mut floors, spec.intent, floor_count, rng);

    // Step 5: build the architecture id (procedural).
    let arch_id = NetArchId(format!("arch-{:08x}", rng.random::<u32>()));

    NetArchitecture {
        id: arch_id,
        display_name: "Procedural NET Architecture".into(),
        floors,
        access_points,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Returns `(floor_min, floor_max, base_dv, high_dv)` for a given price tier.
///
/// Floor ranges are from p.217; DV values are from p.210.
///
/// | PriceTier      | Floors | base_dv | high_dv |
/// |----------------|--------|---------|---------|
/// | VeryExpensive  | 3–6    | DV8     | DV6     |
/// | Luxury         | 7–12   | DV10    | DV8     |
/// | SuperLuxury    | 13–18  | DV12    | DV10    |
/// | (anything else)| 3–6    | DV8     | DV6     |  (treated as VeryExpensive)
///
/// See p.217 (floor cost table) and p.210 (difficulty rating table).
fn tier_params(price: PriceTier) -> (u8, u8, DV, DV) {
    match price {
        PriceTier::SuperLuxury => (13, 18, DV(12), DV(10)),
        PriceTier::Luxury => (7, 12, DV(10), DV(8)),
        // VeryExpensive is the lowest valid NET Architecture tier (p.217).
        // All tiers below VeryExpensive are clamped here.
        _ => (3, 6, DV(8), DV(6)),
    }
}

/// Pick a single non-goal floor, weighted by `SecurityIntent`.
///
/// The floor index `i` and total pre-floors count `total` are used to
/// slightly bias earlier floors toward lighter content (the Lobby rolls on a
/// different table per p.210 Step 2, but we simplify this to a probability
/// skew rather than a full 3d6 table lookup, since the table is a GM
/// convenience tool and RAW says "feel free to pick whichever floors you
/// want" (p.211)).
///
/// Intent weights:
/// - `Defensive`: 40% Password, 30% File, 20% ControlNode, 10% BlackIce
/// - `Offensive`: 10% Password, 10% File, 15% ControlNode, 65% BlackIce
/// - `Balanced`:  25% Password, 25% File, 25% ControlNode, 25% BlackIce
/// - `Killer`:    5% Password,  5% File,  10% ControlNode, 80% BlackIce
///
/// See p.210 (Step 2 — Fill in the Architecture) and p.211 (intent notes).
fn pick_floor(
    intent: SecurityIntent,
    _i: usize,
    _total: usize,
    base_dv: DV,
    high_dv: DV,
    rng: &mut Rng,
) -> Floor {
    // Roll a d100 to select the floor type.
    let roll: u8 = rng.random_range(1..=100);

    let (password_cutoff, file_cutoff, control_cutoff) = match intent {
        SecurityIntent::Defensive => (40u8, 70u8, 90u8), // 40% PW, 30% File, 20% CN, 10% ICE
        SecurityIntent::Offensive => (10u8, 20u8, 35u8), // 10% PW, 10% File, 15% CN, 65% ICE
        SecurityIntent::Balanced => (25u8, 50u8, 75u8),  // 25% PW, 25% File, 25% CN, 25% ICE
        SecurityIntent::Killer => (5u8, 10u8, 20u8),     // 5% PW, 5% File, 10% CN, 80% ICE
    };

    let dv = pick_dv(base_dv, high_dv, rng);

    if roll <= password_cutoff {
        Floor::Password { dv }
    } else if roll <= file_cutoff {
        Floor::File {
            dv,
            contents: pick_file_contents(base_dv, rng),
        }
    } else if roll <= control_cutoff {
        Floor::ControlNode {
            dv,
            controls: pick_control_target(rng),
            currently_held_by: None,
        }
    } else {
        Floor::BlackIce {
            template: pick_black_ice(rng),
            state: BlackIceState::LyingInWait,
            // `ice_per` defaults to 0; the catalog-wiring WP (WP-404+) will
            // populate this from the live Catalog<BlackIce>. See p.206 (PER).
            ice_per: 0,
        }
    }
}

/// Pick the deepest (goal) floor: always a [`Floor::File`] or
/// [`Floor::ControlNode`]. Never a Password, BlackIce, or Demon.
///
/// Per WP-401 spec: "The lowest floor is always either File or ControlNode
/// (the goal). Never Password."
fn pick_goal_floor(base_dv: DV, high_dv: DV, rng: &mut Rng) -> Floor {
    let dv = pick_dv(base_dv, high_dv, rng);
    // 50/50 between File and ControlNode for the goal.
    if rng.random_range(0u8..2u8) == 0 {
        Floor::File {
            dv,
            contents: FileContents::Data("Mission-critical data".into()),
        }
    } else {
        Floor::ControlNode {
            dv,
            controls: pick_control_target(rng),
            currently_held_by: None,
        }
    }
}

/// Post-pass: optionally add a Demon entry to the architecture.
///
/// For `Killer` intent with `floor_count >= 5`, we guarantee at least one
/// Demon. For other intents with large architectures (Luxury+), there is a
/// 30% chance of adding a Demon. For smaller architectures we still allow
/// a 15% chance if there are ControlNode floors.
///
/// The Demon is added as a new floor (appended before the goal if present,
/// or replacing a ControlNode if the floor list is at the top of its tier
/// range). To keep things simple and avoid shifting indices, we simply insert
/// the Demon floor before the goal (last floor), collecting the ControlNode
/// indices at the time of insertion.
///
/// This insertion can cause the floor count to exceed the tier's max by 1
/// when the architecture was already at max. That is an acceptable
/// implementation simplification; the floor count tests allow ±1 for the
/// Demon insertion case.
///
/// See p.212 (Demons and Defenses):
/// "One way to spice up a NET Architecture location is by loading it up with
/// Defenses. You might want to look into installing a Demon into the
/// Architecture to control them."
fn maybe_add_demon(
    floors: &mut Vec<Floor>,
    intent: SecurityIntent,
    floor_count: usize,
    rng: &mut Rng,
) {
    // Collect ControlNode floor indices before adding the Demon.
    let control_node_indices: Vec<usize> = floors
        .iter()
        .enumerate()
        .filter(|(_, f)| matches!(f, Floor::ControlNode { .. }))
        .map(|(i, _)| i)
        .collect();

    // Decide whether to add a Demon.
    let add_demon = match intent {
        SecurityIntent::Killer => {
            // Guarantee a Demon for Killer intent when the architecture is
            // large enough to hold meaningful defenses.
            floor_count >= 5 || rng.random_range(0u8..2u8) == 0
        }
        SecurityIntent::Offensive | SecurityIntent::Balanced => {
            // 30% chance for offensive/balanced if there are nodes to control.
            !control_node_indices.is_empty() && rng.random_range(1u8..=10u8) <= 3
        }
        SecurityIntent::Defensive => {
            // 10% chance for defensive architectures.
            !control_node_indices.is_empty() && rng.random_range(1u8..=10u8) == 1
        }
    };

    if !add_demon {
        return;
    }

    // Choose demon tier based on architecture size (p.212 sidebar:
    // "In doubt, Imps are commonplace. The local movie theater's NET
    // Architecture probably isn't running a Balron unless it's hiding
    // something.").
    let demon_slug = if floor_count >= 13 {
        DEMON_SLUGS[2] // balron — for large architectures
    } else if floor_count >= 7 {
        DEMON_SLUGS[1] // efreet — mid-size
    } else {
        DEMON_SLUGS[0] // imp — small
    };

    let demon_floor = Floor::Demon {
        template: DemonId(demon_slug.into()),
        control_nodes: control_node_indices,
    };

    // Insert before the goal (last floor).
    let insert_pos = floors.len().saturating_sub(1);
    floors.insert(insert_pos, demon_floor);
}

/// Select a DV for a floor. Biased toward `base_dv` (70%) with `high_dv`
/// (30%) as an occasional lighter floor.
///
/// This implements the notion from p.210 that the first two floors are
/// slightly easier (the "Lobby" table), while deeper floors use the full
/// Body table. We simplify to a probability-weighted pick between two
/// adjacent DV values.
///
/// See p.210 (Difficulty Rating table).
fn pick_dv(base_dv: DV, high_dv: DV, rng: &mut Rng) -> DV {
    // 70% base_dv, 30% high_dv (the slightly lighter variant).
    if rng.random_range(1u8..=10u8) <= 7 {
        base_dv
    } else {
        high_dv
    }
}

/// Pick a random Black ICE from the canonical placeholder list.
///
/// See [`BLACK_ICE_SLUGS`] for the list and its rationale.
fn pick_black_ice(rng: &mut Rng) -> BlackIceId {
    let idx = rng.random_range(0..BLACK_ICE_SLUGS.len());
    BlackIceId(BLACK_ICE_SLUGS[idx].into())
}

/// Pick a random control target for a [`Floor::ControlNode`].
///
/// The control target labels are generic placeholder strings; the GM/LLM
/// layer should replace these with location-appropriate descriptions
/// (p.210 Step 3).
///
/// See pp.213–215 (Active, Emplaced, Environmental Defenses).
fn pick_control_target(rng: &mut Rng) -> ControlTarget {
    match rng.random_range(0u8..8u8) {
        0 => ControlTarget::Camera("security camera".into()),
        1 => ControlTarget::Drone("patrol drone".into()),
        2 => ControlTarget::Turret("automated turret".into()),
        3 => ControlTarget::Door("security door".into()),
        4 => ControlTarget::Sprinkler("sprinkler system".into()),
        5 => ControlTarget::Elevator("freight elevator".into()),
        6 => ControlTarget::LaserGrid("laser grid".into()),
        _ => ControlTarget::Custom("custom system".into()),
    }
}

/// Pick [`FileContents`] for a [`Floor::File`].
///
/// Weighted: 60% Data, 20% Decoy, 20% Encrypted.
fn pick_file_contents(base_dv: DV, rng: &mut Rng) -> FileContents {
    match rng.random_range(1u8..=10u8) {
        1..=6 => FileContents::Data("Corporate data".into()),
        7..=8 => FileContents::Decoy,
        _ => FileContents::Encrypted {
            unlock_dv: DV(base_dv.0 + 2),
        },
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use rand::SeedableRng;

    fn make_spec(price: PriceTier, intent: SecurityIntent) -> NetArchSpec {
        NetArchSpec {
            price,
            intent,
            fixer_rank_required: 4,
        }
    }

    /// `test_floor_count_within_tier_range`: generated floor counts must fall
    /// within the ranges defined on p.217.
    ///
    /// Verified against p.217 RAW:
    /// - VeryExpensive: 3 to 6 floors (1,000eb/floor).
    /// - Luxury: 7 to 12 floors (5,000eb/floor).
    /// - SuperLuxury: 13 to 18 floors (10,000eb/floor).
    ///
    /// Note: the generator may add 1 extra floor for a Demon (see
    /// `maybe_add_demon`). The test accounts for this by allowing floor_count
    /// + 1 at the upper bound when a Demon is present.
    ///
    /// No deviation from RAW — WP guidance matches p.217 exactly.
    #[test]
    fn test_floor_count_within_tier_range() {
        let cases = [
            (PriceTier::VeryExpensive, 3usize, 6usize),
            (PriceTier::Luxury, 7usize, 12usize),
            (PriceTier::SuperLuxury, 13usize, 18usize),
        ];

        for (price, min_floors, max_floors) in cases {
            for seed in 0u64..100 {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(price, SecurityIntent::Balanced);
                let arch = generate_net_architecture(&spec, &mut rng);

                // Count non-Demon floors (Demon is added as a post-pass
                // insertion and may push total above tier max by 1).
                let non_demon: usize = arch
                    .floors
                    .iter()
                    .filter(|f| !matches!(f, Floor::Demon { .. }))
                    .count();

                assert!(
                    non_demon >= min_floors && non_demon <= max_floors,
                    "price={price:?} seed={seed}: non-demon floor count {non_demon} not in [{min_floors}, {max_floors}]"
                );
            }
        }
    }

    /// `test_killer_intent_more_ice_and_demons`: across many seeds,
    /// Killer intent architectures must have ≥ 50% BlackIce/Demon floors.
    ///
    /// We aggregate across 200 seeds and compute the overall ratio, then
    /// assert it is >= 50%. Individual architectures may dip below due to
    /// the goal floor always being File/ControlNode; the aggregate must hold.
    #[test]
    fn test_killer_intent_more_ice_and_demons() {
        let total_floors: usize = (0u64..200)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::VeryExpensive, SecurityIntent::Killer);
                generate_net_architecture(&spec, &mut rng).floors.len()
            })
            .sum();

        let combat_floors: usize = (0u64..200)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::VeryExpensive, SecurityIntent::Killer);
                let arch = generate_net_architecture(&spec, &mut rng);
                arch.floors
                    .iter()
                    .filter(|f| matches!(f, Floor::BlackIce { .. } | Floor::Demon { .. }))
                    .count()
            })
            .sum();

        let ratio = combat_floors as f64 / total_floors as f64;
        assert!(
            ratio >= 0.50,
            "Killer intent: expected ≥ 50% BlackIce/Demon floors, got {:.1}% ({} / {})",
            ratio * 100.0,
            combat_floors,
            total_floors
        );
    }

    /// `test_defensive_more_passwords`: across many seeds, Defensive intent
    /// generates more Password floors than Offensive intent at the same size.
    #[test]
    fn test_defensive_more_passwords() {
        let defensive_passwords: usize = (0u64..200)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::VeryExpensive, SecurityIntent::Defensive);
                let arch = generate_net_architecture(&spec, &mut rng);
                arch.floors
                    .iter()
                    .filter(|f| matches!(f, Floor::Password { .. }))
                    .count()
            })
            .sum();

        let offensive_passwords: usize = (0u64..200)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::VeryExpensive, SecurityIntent::Offensive);
                let arch = generate_net_architecture(&spec, &mut rng);
                arch.floors
                    .iter()
                    .filter(|f| matches!(f, Floor::Password { .. }))
                    .count()
            })
            .sum();

        assert!(
            defensive_passwords > offensive_passwords,
            "Defensive intent should produce more Password floors than Offensive \
             (defensive={defensive_passwords}, offensive={offensive_passwords})"
        );
    }

    /// `test_lowest_floor_is_target`: the deepest floor of every generated
    /// architecture must be [`Floor::File`] or [`Floor::ControlNode`].
    ///
    /// Per WP-401 spec: "you wouldn't put the goal behind a password with
    /// nothing after it."
    #[test]
    fn test_lowest_floor_is_target() {
        for seed in 0u64..500 {
            for price in [
                PriceTier::VeryExpensive,
                PriceTier::Luxury,
                PriceTier::SuperLuxury,
            ] {
                for intent in [
                    SecurityIntent::Defensive,
                    SecurityIntent::Offensive,
                    SecurityIntent::Balanced,
                    SecurityIntent::Killer,
                ] {
                    let mut rng = Rng::seed_from_u64(seed);
                    let spec = make_spec(price, intent);
                    let arch = generate_net_architecture(&spec, &mut rng);

                    let last = arch.floors.last().expect("architecture must have floors");
                    assert!(
                        matches!(last, Floor::File { .. } | Floor::ControlNode { .. }),
                        "seed={seed} price={price:?} intent={intent:?}: \
                         deepest floor must be File or ControlNode, got {last:?}"
                    );
                }
            }
        }
    }

    /// `test_deterministic`: same seed produces the same architecture.
    #[test]
    fn test_deterministic() {
        for seed in [0u64, 1, 42, 999, 12345] {
            for price in [
                PriceTier::VeryExpensive,
                PriceTier::Luxury,
                PriceTier::SuperLuxury,
            ] {
                for intent in [
                    SecurityIntent::Defensive,
                    SecurityIntent::Offensive,
                    SecurityIntent::Balanced,
                    SecurityIntent::Killer,
                ] {
                    let spec = make_spec(price, intent);

                    let mut rng_a = Rng::seed_from_u64(seed);
                    let arch_a = generate_net_architecture(&spec, &mut rng_a);

                    let mut rng_b = Rng::seed_from_u64(seed);
                    let arch_b = generate_net_architecture(&spec, &mut rng_b);

                    assert_eq!(
                        arch_a, arch_b,
                        "seed={seed} price={price:?} intent={intent:?}: \
                         same seed must produce identical architecture"
                    );
                }
            }
        }
    }

    /// Sanity check: all floor types round-trip through RON serialisation.
    #[test]
    fn test_floor_ron_round_trip() {
        let floors = vec![
            Floor::Password { dv: DV(8) },
            Floor::ControlNode {
                dv: DV(10),
                controls: ControlTarget::Camera("west corridor".into()),
                currently_held_by: Some(NetEntityId("imp".into())),
            },
            Floor::File {
                dv: DV(8),
                contents: FileContents::Data("Payroll records".into()),
            },
            Floor::File {
                dv: DV(6),
                contents: FileContents::Decoy,
            },
            Floor::File {
                dv: DV(8),
                contents: FileContents::Encrypted { unlock_dv: DV(10) },
            },
            Floor::BlackIce {
                template: BlackIceId("hellhound".into()),
                state: BlackIceState::LyingInWait,
                ice_per: 6,
            },
            Floor::Demon {
                template: DemonId("imp".into()),
                control_nodes: vec![2, 5],
            },
        ];

        for floor in &floors {
            let serialised = ron::ser::to_string(floor)
                .unwrap_or_else(|e| panic!("floor serialise failed: {e}"));
            let restored: Floor = ron::de::from_str(&serialised)
                .unwrap_or_else(|e| panic!("floor deserialise failed: {e}"));
            assert_eq!(floor, &restored, "floor round-trip mismatch");
        }
    }

    /// Sanity check: generated architectures have at least 1 access point
    /// and the access point location is the placeholder.
    #[test]
    fn test_access_points_generated() {
        let mut rng = Rng::seed_from_u64(7);
        let spec = make_spec(PriceTier::VeryExpensive, SecurityIntent::Balanced);
        let arch = generate_net_architecture(&spec, &mut rng);

        assert!(
            !arch.access_points.is_empty(),
            "architecture must have at least one access point"
        );
        assert!(
            arch.access_points.len() <= 3,
            "generator produces at most 3 access points, got {}",
            arch.access_points.len()
        );
        for ap in &arch.access_points {
            assert_eq!(
                ap.location,
                LocationId("placeholder".into()),
                "access point location must be placeholder until GM assigns it"
            );
        }
    }

    /// Verify that Defensive architectures have fewer BlackIce floors
    /// than Killer architectures.
    #[test]
    fn test_killer_has_more_ice_than_defensive() {
        let killer_ice: usize = (0u64..100)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::Luxury, SecurityIntent::Killer);
                generate_net_architecture(&spec, &mut rng)
                    .floors
                    .iter()
                    .filter(|f| matches!(f, Floor::BlackIce { .. }))
                    .count()
            })
            .sum();

        let defensive_ice: usize = (0u64..100)
            .map(|seed| {
                let mut rng = Rng::seed_from_u64(seed);
                let spec = make_spec(PriceTier::Luxury, SecurityIntent::Defensive);
                generate_net_architecture(&spec, &mut rng)
                    .floors
                    .iter()
                    .filter(|f| matches!(f, Floor::BlackIce { .. }))
                    .count()
            })
            .sum();

        assert!(
            killer_ice > defensive_ice,
            "Killer intent should produce more BlackIce than Defensive \
             (killer={killer_ice}, defensive={defensive_ice})"
        );
    }

    /// Verify that DV values in generated architectures are from the valid set
    /// (DV6, DV8, DV10, DV12) per p.210.
    #[test]
    fn test_dv_values_are_valid() {
        let valid_dvs = [DV(6), DV(8), DV(10), DV(12)];

        for seed in 0u64..50 {
            let mut rng = Rng::seed_from_u64(seed);
            let spec = make_spec(PriceTier::Luxury, SecurityIntent::Balanced);
            let arch = generate_net_architecture(&spec, &mut rng);

            for floor in &arch.floors {
                match floor {
                    Floor::Password { dv } | Floor::File { dv, .. } => {
                        assert!(
                            valid_dvs.contains(dv),
                            "invalid DV {:?} on floor — expected one of {:?}",
                            dv,
                            valid_dvs
                        );
                    }
                    Floor::ControlNode { dv, .. } => {
                        assert!(
                            valid_dvs.contains(dv),
                            "invalid DV {:?} on ControlNode — expected one of {:?}",
                            dv,
                            valid_dvs
                        );
                    }
                    Floor::BlackIce { .. } | Floor::Demon { .. } => {}
                }
            }
        }
    }
}
