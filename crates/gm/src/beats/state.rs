//! Beat Chart runtime state machine — WP-604 stub for WP-612 compatibility.
//!
//! ## WP-604 dependency
//!
//! This file is a **minimal compilation stub** created by WP-612 so that
//! `crates/gm/src/npc/hiring.rs` compiles while WP-604 is still in flight.
//!
//! WP-604 (`beats/state.rs` — Beat state machine) will replace this stub with
//! the full `GigState` implementation including the Beat Chart progression
//! machine, hook resolution records, and encounter tracking.
//!
//! When WP-604 lands, its author should:
//! 1. Remove this stub file.
//! 2. Ensure the `GigState` they define retains the `temp_npcs` field (a
//!    `HashMap<NpcTemplateId, EntityId>`) so that WP-612's `hire()` function
//!    continues to compile. If the shape changes, update `hiring.rs` accordingly.
//!
//! Rulebook reference: **pp.395–408** (Beat Chart structure — WP-604 scope).

#![forbid(unsafe_code)]

use crate::npc::entity::NpcTemplateId;
use cpr_rules::types::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Runtime state for an active gig (Beat Chart session).
///
/// **Stub for WP-612 / WP-604 compatibility.** WP-604 will replace this with
/// the full Beat state machine. The `temp_npcs` field is owned by WP-612 and
/// must be preserved when WP-604 fills in the rest.
///
/// `temp_npcs` maps each hired ally's [`NpcTemplateId`] slug to the live
/// [`EntityId`] UUID assigned during instantiation. The Fixer hiring flow
/// writes to this map; combat and scene code reads it to resolve ally identity.
///
/// Rulebook reference: **p.140** (Fixer role — ally recruitment).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GigState {
    /// Hired NPC allies active in this gig, keyed by their template slug.
    ///
    /// Populated by [`crate::npc::hiring::hire`]. Empty at gig start.
    /// Cleared when the gig ends (allies are not carried between gigs).
    ///
    /// **WP-604 note:** this field is the only WP-612 addition to `GigState`.
    /// All other fields belong to the Beat state machine (WP-604 scope).
    pub temp_npcs: HashMap<NpcTemplateId, EntityId>,
}
