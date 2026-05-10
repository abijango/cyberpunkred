//! Beat Chart — schema, loader / validator, and runtime state machine.
//!
//! Populated by Phase 6 work packages:
//! - WP-602 — Beat Chart schema (`schema.rs`)
//! - WP-603 — Beat Chart loader and validator (`loader.rs`)
//! - WP-604 — Beat state machine (`state.rs`)
//!
//! See `IMPLEMENTATION_PLAN.md` §6 and rulebook pp.395–408.

pub mod loader;
pub mod schema;
pub mod state;

pub use loader::*;
pub use schema::*;
pub use state::*;
