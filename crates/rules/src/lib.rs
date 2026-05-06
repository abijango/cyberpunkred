#![forbid(unsafe_code)]

//! Cyberpunk RED rules engine — pure logic. Dice, checks, combat, netrunning,
//! character derivation. WASM- and native-compatible. Zero feature flags.
//!
//! See `IMPLEMENTATION_PLAN.md` §1.4 (single-source-of-truth) and §2 (conventions).

<<<<<<< wp-003-effect-system
pub mod effects;
=======
pub mod dice;
pub mod rng;
>>>>>>> main
pub mod types;

pub use rng::Rng;
