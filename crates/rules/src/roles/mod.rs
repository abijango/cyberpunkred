//! Role Ability mechanics — one sub-module per Role.
//!
//! See `IMPLEMENTATION_PLAN.md` §4 (Phase 5, WP-510 through WP-519) and
//! rulebook pp.142–157 (Role Abilities).
//!
//! Sub-modules are added per Work Package:
//!
//! - [`combat_sense`] (WP-510, Solo) — Initiative + Awareness/Perception bonus. See p.146.
//! - [`interface`] (WP-511, Netrunner) — NET Action count + Interface check rolls. See pp.144, 199.
//! - [`maker`] (WP-512, Tech) — gadget crafting. See p.144+.
//! - [`medicine`] (WP-513, Medtech) — surgery + Critical Injury treatment. See p.142+.
//! - [`credibility`] (WP-514, Media) — narrative-conviction bonus. See p.142+.
//! - [`backup`] (WP-515, Lawman) — call NPC allies. See p.142+.
//! - [`resources`] (WP-516, Exec) — corporate resource pool. See p.142+.
//! - [`operator`] (WP-517, Fixer) — Night-Market procurement. See p.66, p.376.
//! - [`charismatic_impact`] (WP-518, Rockerboy) — performance-driven sway. See p.142+.
//! - [`moto`] (WP-519, Nomad) — vehicle handling bonus. See p.142+.

pub mod combat_sense;
pub mod interface;
