//! Netrunning subsystem — NET architecture model, state, and procedural generation.
//!
//! This module implements the rules for jacking into and navigating NET
//! Architectures in *Cyberpunk RED*. See pp.209–218 for the rulebook foundation.
//!
//! ## Modules
//!
//! - [`architecture`] — NET architecture data model and procedural generator
//!   ([`architecture::generate_net_architecture`]). See pp.209–212, 217.
//! - [`state`] — Active netrun state: programs rezzed, floors revealed,
//!   control nodes held, viruses queued. See pp.197–200.
//! - [`abilities`] — Interface Abilities (Scanner, Backdoor, Cloak, etc.).
//!   See pp.198–199.

pub mod abilities;
pub mod actions;
pub mod architecture;
pub mod demon;
pub mod integration;
pub mod programs;
pub mod state;
pub mod virus;
