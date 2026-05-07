//! Program-activation logic for Booster and Defender programs.
//!
//! This module handles the lifecycle of rezzed (activated) programs in a
//! NET Architecture. See pp.201–203 for the canonical rules.
//!
//! ## Modules
//!
//! - [`active`] — [`activate_booster_or_defender`]: the core activation
//!   routine that validates a program class, rezzes it in [`NetrunState`],
//!   and pushes its [`EffectModifier`]s onto the Netrunner's
//!   [`EffectStack`]. See p.201.

pub mod active;
