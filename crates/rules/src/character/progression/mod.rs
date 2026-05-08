//! Improvement Point earning and spending — character progression at the
//! mechanical level.
//!
//! See `IMPLEMENTATION_PLAN.md` §4 (Phase 5) and rulebook pp.408–411.
//!
//! Sub-modules are added per Work Package:
//!
//! - [`spend`] (WP-508) — spend IP on Skill / Role rank increases. See pp.408–411.
//! - [`earn`] (WP-509) — milestone IP awards (gig-completed, enemy-defeated, etc.).

pub mod earn;
pub mod spend;
