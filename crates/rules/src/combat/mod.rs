//! Combat subsystem modules.
//!
//! Each submodule is responsible for a discrete slice of combat mechanics as
//! specified in the Work Packages for Phase 3. See `IMPLEMENTATION_PLAN.md`
//! §3 for the phase plan and the relevant WPs for each module's public API.
//!
//! ## Sub-modules
//!
//! - [`cover`] (WP-313) — cover interposition; absorb / pass-through split.
//! - [`critical_injury`] (WP-305) — critical-injury trigger and application.
//! - [`damage`] (WP-303) — damage pipeline and armor ablation.
//! - [`dodge`] (WP-316) — REF≥8 dodge election helper.
//! - [`grid`] (WP-302 placeholder) — 2D combat grid; replaced by WP-302.
//! - [`turn_engine`] (WP-301) — initiative rolling, queue management, round lifecycle.

pub mod cover;
pub mod critical_injury;
pub mod damage;
pub mod dodge;
pub mod grid;
pub mod turn_engine;

pub use turn_engine::{
    CombatState, CombatSummary, HeldAction, HoldTrigger, InitiativeEntry, PlannedAction,
    TurnEndEvents,
};
