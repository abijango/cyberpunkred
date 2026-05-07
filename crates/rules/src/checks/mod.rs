//! Skill check primitives.
//!
//! This module hosts the core *roll vs. number* primitives the rest of the
//! rules engine composes:
//!
//! - [`SkillCheck`] — `STAT + Skill + 1d10 vs. DV` (rulebook p.129).
//! - [`OpposedCheck`] — attacker vs. defender, ties favour the defender
//!   (rulebook p.129).
//!
//! Both implement [`crate::resolution::Resolution`]. Their
//! [`crate::resolution::Resolution::Outcome`] is a `Result` because LUCK
//! validation (p.130) and entity-lookup can fail before the dice are
//! rolled — see [`crate::error::RulesError`].

pub mod skill_check;

pub use skill_check::{NamedModifier, OpposedCheck, OpposedOutcome, SkillCheck};
