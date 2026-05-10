//! Structured campaign log and LLM-prompt digest.
//!
//! Populated by Phase 6 work packages:
//! - WP-607 — Structured campaign log types (`types.rs`)
//! - WP-608 — Campaign log digest generator (`digest.rs`)
//!
//! See `IMPLEMENTATION_PLAN.md` §6.

pub mod digest;
pub mod types;

pub use digest::*;
pub use types::*;
