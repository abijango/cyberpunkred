//! The single deterministic RNG used everywhere in `cpr_rules`.
//!
//! Every dice-rolling function takes `rng: &mut Rng` as its **last** parameter.
//! Seeds are explicit — the engine never seeds from the OS. Callers create an
//! `Rng` with [`SeedableRng::seed_from_u64`] (re-exported from `rand`) and
//! thread it through every roll site.
//!
//! See `IMPLEMENTATION_PLAN.md` §2.4.

pub use rand_chacha::ChaCha20Rng as Rng;
