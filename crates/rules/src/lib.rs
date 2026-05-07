#![forbid(unsafe_code)]

//! Cyberpunk RED rules engine — pure logic. Dice, checks, combat, netrunning,
//! character derivation. WASM- and native-compatible. Zero feature flags.
//!
//! See `IMPLEMENTATION_PLAN.md` §1.4 (single-source-of-truth) and §2 (conventions).

pub mod character;
pub mod dice;
pub mod effects;
pub mod error;
pub mod resolution;
pub mod rng;
pub mod types;
pub mod world;

pub use error::RulesError;
pub use rng::Rng;
