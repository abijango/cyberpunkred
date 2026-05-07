//! Interface Abilities — the Netrunner's toolkit for traversing and
//! exploiting NET Architectures.
//!
//! Each sub-module implements one Interface Ability as a [`Resolution`]-typed
//! action. All abilities (save for Scanner) consume at least one NET Action;
//! Scanner is the exception — it is a **Meat Action** (p.198).
//!
//! ## Roll formula (p.198–199)
//!
//! > "Resolution for using any of these abilities (save for Zap) is as follows:
//! > **Interface + 1d10 vs. DV**"
//!
//! "Interface" here is the Netrunner's Role Ability rank (`character.role_rank`
//! when `character.role == Role::Netrunner`). The full check value is:
//!
//! ```text
//! INT + Interface_rank + 1d10  vs.  DV
//! ```
//!
//! See p.199 for the full list of abilities.
//!
//! [`Resolution`]: crate::resolution::Resolution

pub mod scanner;
pub mod slide;
