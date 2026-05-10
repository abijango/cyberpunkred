#![forbid(unsafe_code)]

//! Cyberpunk RED Game Master layer — Beat Chart orchestration, NPC behaviour,
//! structured campaign log, encounter loading. Depends on `cpr_rules` only.
//! Synchronous (async lives at the edges in `cpr_server` / `cpr_web`).
//!
//! See `IMPLEMENTATION_PLAN.md` §1.2 (dependency graph) and §6 (Phase 6 WPs).
//!
//! ## Module map
//!
//! - [`beats`] — Beat Chart schema (WP-602), loader/validator (WP-603), runtime state (WP-604).
//! - [`npc`]   — NPC template / active model (WP-605), instantiation (WP-606), hiring (WP-612).
//! - [`log`]   — Structured campaign log (WP-607), digest generator (WP-608).
//! - [`encounter`] — Combat encounter loader (WP-610).
//! - [`faction`]   — Faction and reputation tracking (WP-611).
//! - [`hooks`]     — Mechanical hook resolver (WP-613).
//! - [`ip`]        — LLM-bonus IP awarding (WP-609).
//! - [`ids`]       — Slug-based newtype IDs shared across the GM layer.

pub mod beats;
pub mod encounter;
pub mod error;
pub mod faction;
pub mod hooks;
pub mod ids;
pub mod ip;
pub mod log;
pub mod npc;

pub use cpr_rules::character::Character;
pub use cpr_rules::types::{CharacterId, EntityId, Eurobucks, NpcId, Stat, DV};
pub use cpr_rules::world::{LocationId, World};
pub use cpr_rules::{Catalog, Rng, RulesError};

pub use error::GmError;
pub use ids::{BeatId, EncounterId, FactionId, GigId, MechanicalHookId};
