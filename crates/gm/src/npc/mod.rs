//! NPC entity model, instantiation, and ally hiring.
//!
//! Populated by Phase 6 work packages:
//! - WP-605 — NPC entity model (`entity.rs`)
//! - WP-606 — NPC instantiation from template (`instantiate.rs`)
//! - WP-612 — NPC ally hiring / Fixer integration (`hiring.rs`)
//!
//! See `IMPLEMENTATION_PLAN.md` §6.

pub mod entity;
pub mod hiring;
pub mod instantiate;

pub use entity::NpcTemplateId;
pub use hiring::*;
pub use instantiate::*;
