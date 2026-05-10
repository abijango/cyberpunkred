//! Improvement Point (IP) awarding via the LLM-bonus path.
//!
//! Populated by Phase 6 work packages:
//! - WP-609 — IP awarding (LLM-bonus side, capped) (`llm_bonus.rs`)
//!
//! IP *spending* and the rule-defined IP earn milestones live in
//! `cpr_rules::character::progression` (WP-508 / WP-509). This module is
//! exclusively for the narrative-quality bonus the LLM returns.
//!
//! See `IMPLEMENTATION_PLAN.md` §6.
