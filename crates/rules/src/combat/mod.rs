//! Combat subsystem modules.
//!
//! Each submodule is responsible for a discrete slice of combat mechanics as
//! specified in the Work Packages for Phase 3. See `IMPLEMENTATION_PLAN.md`
//! §3 for the phase plan and the relevant WPs for each module's public API.
//!
//! ## Sub-modules
//!
//! - [`damage`] (WP-303) — damage pipeline and armor ablation.
//! - [`dodge`] (WP-316) — REF≥8 dodge election helper.

pub mod damage;
pub mod dodge;
