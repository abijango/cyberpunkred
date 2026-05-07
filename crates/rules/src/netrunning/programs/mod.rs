//! Program activation — Netrunner programs that act during a netrun.
//!
//! Sub-modules implement each class of non-Black-ICE program:
//!
//! - [`attackers`] — Attacker programs (Banhammer, Sword, anti-personnel
//!   attackers such as Hellbolt / Vrizzbolt, etc.). See pp.201–204.
//!
//! Other program classes (Booster, Defender) do not use an activation
//! function — they apply their effects passively while Rezzed. Their effects
//! are applied by the effect-stack machinery rather than by an action
//! resolver here.
//!
//! See pp.201–204 (Programs) and p.202 (The Three Kinds of Non-Black ICE
//! Programs).

pub mod attackers;
