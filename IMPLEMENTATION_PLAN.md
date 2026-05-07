# Cyberpunk Red CRPG — Implementation Handoff

> **Audience:** Claude Code agent teams working in parallel.
> **Status:** Plan-of-record. Treat the design decisions in §1–§3 as fixed unless the user changes them.
> **Source rulebook:** *Cyberpunk RED Core Rules* (R. Talsorian, 2020), 458 pages. The PDF is **not** checked into the repo; place your own copy at `rulebook/Cyberpunk_Red_Core-Digital_v1.25.pdf` (workspace root). All page references in this document use that PDF.

---

## Table of Contents

0. [Context for Agents](#0-context-for-agents)
1. [Architecture Overview](#1-architecture-overview)
2. [Conventions](#2-conventions)
3. [Phase Plan Overview](#3-phase-plan-overview)
4. [Work Packages](#4-work-packages)
   - [Phase 0 — Foundation](#phase-0--foundation)
   - [Phase 1 — Core Rules Mechanics](#phase-1--core-rules-mechanics)
   - [Phase 2 — Data Catalogs](#phase-2--data-catalogs)
   - [Phase 3 — Combat Subsystems](#phase-3--combat-subsystems)
   - [Phase 4 — Netrunning](#phase-4--netrunning)
   - [Phase 5 — Character & Progression](#phase-5--character--progression)
   - [Phase 6 — GM Layer](#phase-6--gm-layer)
   - [Phase 7 — LLM Layer](#phase-7--llm-layer)
   - [Phase 8 — Frontend (Leptos)](#phase-8--frontend-leptos)
   - [Phase 9 — Backend (Axum)](#phase-9--backend-axum)
   - [Phase 10 — Integration](#phase-10--integration)
5. [Coordination Protocols](#5-coordination-protocols)
6. [Verification Gates](#6-verification-gates)
7. [Appendix — Agent Onboarding (CLAUDE.md template)](#7-appendix--agent-onboarding-claudemd-template)

---

## 0. Context for Agents

### 0.1 What we're building

A **solo, story-driven CRPG** based on *Cyberpunk RED*, implemented in **Rust**, delivered as a **web frontend (Leptos/WASM) + Axum backend**. Faithful to the tabletop rules. The Game Master role is filled by an LLM (locally-hosted via LM Studio for dev, Anthropic / AWS Bedrock in production). All dice and rules adjudication is done by a deterministic rules engine — the LLM only narrates and selects from constrained choices.

### 0.2 Design decisions (already made — do not relitigate)

These were settled in conversation with the user. Implement against them.

| Decision | Choice |
|---|---|
| Game type | Solo story-driven CRPG |
| Frontend | Leptos (fine-grained signals, SSR-ready) |
| Backend | Axum + SQLite (sqlx) |
| Combat presentation | SVG grid, 2 m squares |
| Faithfulness | Strict RAW where rolls/numbers are involved |
| Determinism | Seedable RNG (`rand_chacha::ChaCha20Rng`) from day one, threaded through every roll site |
| GM authority | Rules engine is authoritative on dice and DV outcomes. LLM may **select** from the standard DV ladder (9, 13, 15, 17, 21, 24) and **select** modifiers from a constrained list. LLM **never invents numbers**. |
| Improvement Points | Hybrid: objective milestones award fixed IP; LLM awards a capped narrative bonus per session. |
| Solo adaptation | Solo PC + hireable NPC allies per gig (via Fixer). |
| Cyberpsychosis (HUM < 0) | Therapy-driven recovery — character is recoverable, not lost. |
| LUCK refill | Per-gig. |
| Critical Injuries | Faithful to the 12-entry Body and Head tables. Stateful effects, ongoing triggers, all of it. |
| LLM providers | LM Studio (browser-direct, no backend), Anthropic (backend-only), Bedrock (backend-only). Same `LlmProvider` trait. |
| Content format | RON — comments, trailing commas, native enum/struct serialisation. |
| Derived value caching | Recompute on read. No cache. |
| Sub-agent model | Always `sonnet` when spawning a sub-agent via the Agent tool. Do **not** inherit the orchestrator's model and do **not** use `opus` or `haiku`. Set `model: "sonnet"` explicitly on every Agent call. |

### 0.3 How to use this document

If you are an agent picking up a Work Package (WP):

1. Read **§1 (Architecture)** and **§2 (Conventions)** in full. Skim once, refer back as needed.
2. Read your assigned WP in §4.
3. Read the **rulebook page references** in your WP. Do not skim. Cyberpunk Red has subtle interactions; the page reference is your spec.
4. Read the **public API** of every WP your work `Depends on:`. You may assume those APIs are stable. You may *not* read implementation code beyond what's documented in their public API — if you need more, you have a coordination problem (see §5).
5. Implement, test, and verify against your WP's **acceptance criteria**.
6. Mark the WP done by populating the public API surface exactly as specified, including doc comments.

### 0.4 The rulebook

The PDF is **not** checked into the repo (licensing). Each developer / agent environment must provide a local copy at **`rulebook/Cyberpunk_Red_Core-Digital_v1.25.pdf`** (workspace root). All page references in this document are to that PDF; read the cited pages directly with the `Read` tool's `pages:` parameter. When in doubt, **trust the rulebook over this document** for mechanics — flag the discrepancy.

---

## 1. Architecture Overview

### 1.1 Workspace layout

```
cyberpunk-red/
├── Cargo.toml                     # workspace root
├── rust-toolchain.toml            # pin to stable
├── content/                       # authored game content (RON, hot-reloadable in dev)
│   ├── catalogs/                  # weapons, armor, cyberware, programs, etc.
│   ├── gigs/                      # Beat Charts
│   ├── npcs/                      # NPC templates
│   ├── locations/                 # places and grid maps
│   └── tables/                    # critical injury tables, lifepath, etc.
├── crates/
│   ├── rules/                     # pure logic, dice, combat, netrunning. WASM + native.
│   ├── gm/                        # Beat orchestration, NPCs, campaign log. WASM + native.
│   ├── llm/                       # LlmProvider trait + impls
│   ├── persistence/               # save/load schemas. native (sqlx) + wasm (serde-only).
│   ├── server/                    # Axum: cloud LLM proxy + save sync. Native only.
│   └── web/                       # Leptos frontend. WASM only.
├── tools/
│   ├── content-validator/         # CLI: lints content/ for schema correctness
│   └── replay/                    # CLI: replay a deterministic game from seed + actions
└── IMPLEMENTATION_PLAN.md         # this file
```

### 1.2 Crate dependency graph

```
                ┌──────────┐
                │  rules   │   pure logic, no I/O
                └────┬─────┘
                     │
       ┌─────────────┼─────────────┐
       │             │             │
   ┌───▼───┐    ┌────▼────┐   ┌────▼────────┐
   │  gm   │    │persist. │   │   tools/    │
   └───┬───┘    └────┬────┘   └─────────────┘
       │             │
   ┌───▼─────────────▼──────┐
   │         llm            │
   └───┬───────────────┬────┘
       │               │
   ┌───▼────┐     ┌────▼────┐
   │ server │     │   web   │
   └────────┘     └─────────┘
```

- `rules` has zero feature flags. Same code on native and WASM.
- `gm` may depend on `rules` only.
- `llm` exposes a trait; `rules` and `gm` define the prompt-input data structures (which the `llm` crate consumes).
- `web` and `server` both depend on `rules`, `gm`, `llm`, `persistence`.
- `server` is native-only (`#[cfg(not(target_arch = "wasm32"))]` not needed; just don't compile it for WASM).
- `web` is WASM-only.

### 1.3 Build targets

- Native: `cargo build` (used for tests, `server`, `tools/`).
- WASM: `wasm-pack build` or Trunk for `web`. CI must build both.

### 1.4 Single-source-of-truth principles

- **Dice and rules logic exist only in `rules`.** Never re-implement a roll in `web` or `server`.
- **Content lives in `content/`, not in code.** Catalogs, Beat Charts, NPCs, tables — all RON. Code defines the *schema*, content provides the *data*.
- **Outcomes are structured, not stringly-typed.** A combat resolution returns `AttackOutcome { hit: bool, damage_rolled: u16, sp_ablated: u8, wound_state_change: Option<WoundState>, criticals: Vec<CriticalInjuryKind>, ... }`. The UI animates the outcome. The LLM narrates the outcome. They consume the same data.
- **Effects compose, base values don't mutate.** A character's `stats.dex` is forever their base DEX. Active effects modify *queries*, never base data. See §2.6.

---

## 2. Conventions

### 2.1 Naming

- Crates and modules: `snake_case`.
- Types: `UpperCamelCase`. Use full words: `CriticalInjuryKind`, not `CritKind`.
- Enums for closed sets, newtypes for open identifiers (e.g. `pub struct CharacterId(pub Uuid);`).
- Use Rust 2021 idioms. No `r#try`, no `tokio` macros in `rules`/`gm` crates.
- Cyberpunk-Red-specific terms preserve book capitalisation: `Skill`, `Role`, `Beat`, `NET Architecture`, `Black ICE`. In code: `Skill`, `Role`, `Beat`, `NetArchitecture`, `BlackIce`.

### 2.2 Error handling

- Library crates (`rules`, `gm`, `llm`, `persistence`): `thiserror`-derived error enums per crate (`RulesError`, `GmError`, etc.). Functions that can fail return `Result<T, ThisCrateError>`. Never `unwrap`/`expect` outside tests.
- Binary crates (`server`, `tools/`): `anyhow` is fine.
- `web`: errors get logged and surfaced as user-visible messages. Don't `panic!` in event handlers — match and render a fallback.

### 2.3 Testing

- Every WP delivers tests in its module (`#[cfg(test)] mod tests`).
- Use `proptest` for dice properties (distributions, ranges, monotonicity).
- Use plain `#[test]` with explicit RNG seeds for scenario tests. Do not use `thread_rng()` anywhere — even tests use `Rng::seed_from_u64(...)`.
- Test fixtures (sample characters, sample weapons) live in `crates/<crate>/tests/fixtures/` as RON. Loaded via a shared helper in `crates/rules/src/test_support.rs` (gated behind `#[cfg(any(test, feature = "test-support"))]`).
- Acceptance criteria in §4 are written as test names. Implement them as such.

### 2.4 Determinism

- The single RNG type: `pub type Rng = rand_chacha::ChaCha20Rng;` re-exported from `rules::rng`.
- Every public function that rolls dice takes `rng: &mut Rng` as the **last** parameter.
- Never call `rand::thread_rng()`, `rand::random()`, or `Instant::now()` in `rules` or `gm`.
- Logging the seed at gig start is mandatory (M12 / WP-1002). This makes every game replayable.

### 2.5 Content files

- All authored content in RON. Use `serde` `Deserialize` derives on every catalog type.
- Content loaders live in the crate that defines the schema. Loaders return `Result<Catalog<T>, RulesError>`.
- The `tools/content-validator` CLI loads every file in `content/` and reports any schema errors. CI runs it.
- Hot-reload in dev: the `web` and `server` crates may reload content on file change. The `rules` crate itself is content-agnostic — it operates on already-loaded structures.

### 2.6 The effect system

This is the architectural keystone. Read this section in full before touching any character or combat code.

A `Character`'s base data (`stats`, `skills` ranks, etc.) is **immutable after creation** except via explicit progression actions (Improvement Points, cyberware install, levelling). All transient/conditional changes — wound penalties, armor penalties, drug effects, critical injury effects, role buffs, environmental modifiers — flow through `EffectStack`.

```rust
pub struct EffectStack {
    pub effects: Vec<ActiveEffect>,
}

pub struct ActiveEffect {
    pub id: EffectInstanceId,
    pub source: EffectSource,
    pub modifiers: Vec<EffectModifier>,
    pub duration: EffectDuration,
}
```

When code wants a character's *current* DEX or current MOVE, it calls `character.current_dex()` / `character.current_move()`. These functions iterate `effects`, sum the relevant modifiers, and apply floors (MOVE has min 1, etc.).

`EffectModifier` is a **closed enum** — every kind of modifier in the system is one of its variants. New variants are added carefully and reviewed; this is the place where rules drift would creep in.

Event-driven modifiers (`DamageOnMovementOver`, `DamagePerTurn`, `OnDamageTaken`) are checked at specific lifecycle points by the combat engine. The combat engine knows about these hook points; it does *not* know about specific injuries (Broken Ribs, Foreign Object). That coupling is reversed: the *injury data* declares which hook to use.

Full type definition is in WP-003.

### 2.7 Documentation

- Every public type and function has a doc comment (`///`).
- Doc comments cite rulebook pages where rules are involved: `/// See p.187 (Critical Injuries to the Body).`
- Use `#[doc = include_str!(...)]` for long-form rule explanations if useful.

### 2.8 No unsafe, no async in `rules`/`gm`

- No `unsafe` in any crate. If you think you need it, file a design question.
- `rules` and `gm` are synchronous. Async lives at the edges (`server`, `web`, `llm` provider impls).

### 2.9 Commit conventions

- One WP per branch: `wp-XXX-short-description`.
- Commit prefix: `[WP-XXX]`.
- One PR per WP. PR description must reference the WP ID and the rulebook pages consulted.

---

## 3. Phase Plan Overview

| Phase | Theme | Parallelism | Blocks |
|---|---|---|---|
| 0 | Foundation (workspace, core types, RNG, effect skeleton) | 1 agent, sequential | Everything |
| 1 | Core rules mechanics (dice, checks, derivation, wound states) | ~7 agents | Phases 3, 4, 5 |
| 2 | Data catalogs (weapons, armor, cyberware, ICE, etc.) | ~14 agents | Phases 3, 4, 5 (partially) |
| 3 | Combat subsystems (initiative, attacks, damage, criticals) | ~16 agents | Phases 6, 8 |
| 4 | Netrunning (architecture, abilities, ICE, demons) | ~17 agents | Phases 6, 8 |
| 5 | Character & progression (creation, lifepath, role abilities, IP) | ~19 agents | Phases 6, 8 |
| 6 | GM layer (Beat Charts, NPCs, campaign log) | ~13 agents | Phase 8 (gameplay) |
| 7 | LLM layer (provider trait, prompts) | ~10 agents | Phases 8, 9 (LLM features) |
| 8 | Frontend (Leptos UI) | ~15 agents | Phase 10 |
| 9 | Backend (Axum endpoints) | ~8 agents | Phase 10 |
| 10 | Integration (sample gig, smoke tests, perf, docs) | ~4 agents | — |

**Wave strategy.** You don't need to finish a phase before starting the next. Once Phase 0 lands, Phases 1 and 2 can run in parallel. Phase 3 starts as soon as the WPs in Phase 1 it depends on are merged — not when "Phase 1" is done. Read the `Depends on:` field of each WP carefully; that's the real graph.

**Critical path.** Phase 0 → WP-101 (dice) → WP-102 (skill check) → WP-303 (damage pipeline) → WP-306 (single-shot) → WP-1001 (sample gig). Everything else fans out from this spine.

---

## 4. Work Packages

### Conventions for each WP

```
### WP-XXX — Title
**Crate:** crate name
**Module:** path/to/file.rs (or files)
**Depends on:** WP-YYY, WP-ZZZ
**Blocks:** WP-AAA, WP-BBB
**Estimate:** Small (≤200 LOC) / Medium (200–600 LOC) / Large (>600 LOC, consider splitting)

**Rulebook:** page references

**Description:** what this WP does

**Public API to add:** Rust signatures the agent commits to

**Acceptance criteria:** named tests

**Notes:** edge cases, gotchas, references to other docs
```

Agents may add private helpers and additional tests. Agents may **not** change the public API without coordinating (see §5.2).

---

### Phase 0 — Foundation

**This phase is sequential.** One agent owns it end to end. Nothing in Phase 1+ can begin until Phase 0 is merged.

#### WP-000 — Workspace bootstrap
**Crate:** workspace root
**Module:** `Cargo.toml`, `rust-toolchain.toml`, `.github/workflows/ci.yml`, `crates/*/Cargo.toml`, empty `src/lib.rs` per crate
**Depends on:** —
**Blocks:** every other WP
**Estimate:** Small

**Description:** Set up the Cargo workspace with all crates listed in §1.1 as empty library/binary skeletons. Pin Rust to a recent stable. Add CI that runs `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace`, `wasm-pack build crates/web --target web`.

**Public API to add:** N/A — empty crates with `pub fn placeholder() {}` to satisfy compilation.

**Acceptance criteria:**
- `cargo build --workspace` succeeds.
- `cargo test --workspace` succeeds (zero tests run is fine).
- `wasm-pack build crates/web --target web` succeeds.
- CI green on a fresh checkout.

**Notes:** Use `resolver = "2"`. Set `rust-version = "1.75"` minimum. Forbid `unsafe_code` in `rules` and `gm` via `#![forbid(unsafe_code)]` at the crate root.

---

#### WP-001 — Core identifier and value types
**Crate:** `rules`
**Module:** `crates/rules/src/types.rs`
**Depends on:** WP-000
**Blocks:** every other rules WP
**Estimate:** Small

**Rulebook:** general — these are scaffolding types.

**Description:** Define ID newtypes and primitive value types used everywhere.

**Public API to add:**
```rust
use uuid::Uuid;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct CharacterId(pub Uuid);
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct EntityId(pub Uuid);
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct NpcId(pub Uuid);
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct EffectInstanceId(pub Uuid);

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Serialize, Deserialize)]
pub struct DV(pub u8);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Serialize, Deserialize)]
pub struct Eurobucks(pub i64);

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum PriceTier {
    Cheap, Everyday, Costly, Premium, Expensive, VeryExpensive, Luxury, SuperLuxury,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum Stat { Int, Ref, Dex, Tech, Cool, Will, Luck, Move, Body, Emp }

impl PriceTier {
    /// Returns the canonical Eurobuck cost for this tier (p.371).
    pub fn canonical_cost(self) -> Eurobucks { /* ... */ }
}

impl DV {
    pub const SIMPLE: DV = DV(9);
    pub const EVERYDAY: DV = DV(13);
    pub const DIFFICULT: DV = DV(15);
    pub const PROFESSIONAL: DV = DV(17);
    pub const HEROIC: DV = DV(21);
    pub const INCREDIBLE: DV = DV(24);
}
```

**Acceptance criteria:**
- `test_dv_constants_match_book`: each DV constant matches the values on p.129.
- `test_price_tier_canonical_costs`: each tier's canonical cost matches p.371 (50 / 100 / 500 / 1,000 / 5,000 / 10,000 / 100,000 — confirm against the rulebook).
- All types implement `Copy` where they reasonably can; all derive `Serialize`/`Deserialize`.

**Notes:** Add `serde` and `uuid` to `rules` deps. UUIDs are v4. Don't add a runtime UUID generator dependency to `rules` itself — pass UUIDs in.

---

#### WP-002 — Deterministic RNG and dice helpers
**Crate:** `rules`
**Module:** `crates/rules/src/rng.rs`, `crates/rules/src/dice.rs`
**Depends on:** WP-001
**Blocks:** WP-101 and everything downstream
**Estimate:** Small

**Rulebook:** dice are used everywhere. Critical mechanics on p.129.

**Description:** Wrap `rand_chacha::ChaCha20Rng` as the project's only RNG type. Provide dice helpers.

**Public API to add:**
```rust
pub use rand_chacha::ChaCha20Rng as Rng;

pub mod dice {
    use super::Rng;
    use rand::Rng as _;

    /// Roll a single d10 (1..=10), no crit handling.
    pub fn d10(rng: &mut Rng) -> u8;

    /// Roll a single d6 (1..=6).
    pub fn d6(rng: &mut Rng) -> u8;

    /// Roll N d6 and return the individual values, in roll order.
    pub fn ndn_d6(n: u8, rng: &mut Rng) -> Vec<u8>;

    /// Roll a d10 with Cyberpunk Red crit rules (p.129):
    /// - On natural 10, roll another d10 and add (no further crits).
    /// - On natural 1, roll another d10 and subtract (no further fumbles).
    /// Returns: (final value, breakdown) where breakdown shows base roll, modifier, and outcome kind.
    pub fn d10_with_crits(rng: &mut Rng) -> CritD10;
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct CritD10 {
    pub base: u8,                  // 1..=10
    pub follow_up: Option<u8>,     // 1..=10 if base was 1 or 10
    pub outcome: D10Outcome,
    /// Net contribution to a check (can be negative on critical failure).
    pub net: i16,
}

#[derive(Copy, Clone, Eq, PartialEq, Debug, Serialize, Deserialize)]
pub enum D10Outcome { Normal, CriticalSuccess, CriticalFailure }
```

**Acceptance criteria:**
- `test_d10_distribution` (proptest): 100k rolls, every value 1..=10 appears, mean ≈ 5.5.
- `test_d10_with_crits_natural_10`: forced seed where base = 10; net == 10 + follow-up; outcome == CriticalSuccess.
- `test_d10_with_crits_natural_1`: forced seed where base = 1; net == 1 - follow-up; outcome == CriticalFailure.
- `test_d10_with_crits_no_chained_crits`: even if follow-up is 10, no further explosion.
- `test_seed_determinism`: same seed → same sequence across two `Rng` instances.

**Notes:** `CritD10::net` is what gets added to `STAT + Skill` for the final check value. The convention is `final_check = stat + skill + crit_d10.net`. Can be negative on a critical failure. See p.129.

---

#### WP-003 — Effect system core types
**Crate:** `rules`
**Module:** `crates/rules/src/effects/mod.rs`, `crates/rules/src/effects/modifier.rs`
**Depends on:** WP-001
**Blocks:** WP-104, WP-303, WP-305, all combat & character WPs
**Estimate:** Medium

**Rulebook:** Critical Injuries pp.187–188, Wound States p.186, Cyberware HL p.227, drugs p.227.

**Description:** Define the `EffectStack`, `ActiveEffect`, `EffectModifier`, `EffectSource`, and `EffectDuration` types per §2.6. This is the keystone — every other character / combat WP queries these.

**Public API to add:**
```rust
pub struct EffectStack {
    pub effects: Vec<ActiveEffect>,
}

impl EffectStack {
    pub fn new() -> Self;
    pub fn add(&mut self, effect: ActiveEffect);
    pub fn remove(&mut self, id: EffectInstanceId) -> Option<ActiveEffect>;
    pub fn iter(&self) -> impl Iterator<Item = &ActiveEffect>;
    pub fn iter_modifiers(&self) -> impl Iterator<Item = &EffectModifier>;
    /// Tick a Turn forward. Decrement Turns durations, drop expired effects.
    /// Returns the IDs of effects that were dropped this tick.
    pub fn tick_turn(&mut self) -> Vec<EffectInstanceId>;
    pub fn end_round(&mut self) -> Vec<EffectInstanceId>;
    pub fn end_gig(&mut self) -> Vec<EffectInstanceId>;
    pub fn end_netrun(&mut self) -> Vec<EffectInstanceId>;
}

pub struct ActiveEffect {
    pub id: EffectInstanceId,
    pub source: EffectSource,
    pub modifiers: Vec<EffectModifier>,
    pub duration: EffectDuration,
}

pub enum EffectSource {
    CriticalInjury(CriticalInjuryKind),
    WoundState(WoundState),
    Cyberware(CyberwareId),
    Armor,
    Drug(DrugId),
    Program(ProgramId),
    Environmental(EnvironmentalKind),
    Cyberpsychosis,
    RoleAbility(RoleAbilityId),
}

pub enum EffectModifier {
    StatPenalty { stat: Stat, by: i8 },
    StatBonus { stat: Stat, by: i8 },
    SkillPenalty { skill: SkillId, by: i8 },
    SkillBonus { skill: SkillId, by: i8 },
    AllActionsPenalty(i8),               // wound state penalty
    MovePenalty(i8),                     // floor at 1 applied at query time
    DeathSavePenaltyDelta(i8),           // permanent until injury healed
    MeleeAttackPenalty(i8),              // torn muscle
    HandActionsPenalty { hand: Hand, by: i8 },  // crushed fingers
    CannotTakeAction,                    // spinal injury (next-turn)
    CannotTakeMoveAction,                // prone, dismembered legs
    CannotDodge,                         // dismembered leg, human-shielded
    DamageOnMovementOver { threshold_m: u16, damage: HpDamage },  // broken ribs, foreign object
    DamagePerTurn(HpDamage),
    AutofireDvDelta(i8),
    InitiativeBonus(i8),
}

pub enum EffectDuration {
    Permanent,
    UntilHealed { quick_fix: Option<DV>, treatment: DV },
    Turns(u16),
    UntilEndOfRound,
    UntilGigEnd,
    UntilEndOfNetrun,
}

pub enum CriticalInjuryKind { /* 24 variants — see WP-205 */ }
pub enum WoundState { Lightly, Seriously, Mortally, Dead }
pub enum Hand { Left, Right, Either }
pub struct HpDamage(pub u16);
pub enum EnvironmentalKind { Darkness, ExtremeStress, Exhausted, Drunk, Smoke, Stealth, /* ... */ }

// Stub IDs — concrete catalog WPs (Phase 2) will reference them.
pub struct CyberwareId(pub String);
pub struct DrugId(pub String);
pub struct ProgramId(pub String);
pub struct RoleAbilityId(pub String);
pub struct SkillId(pub String);  // refined in WP-201
```

**Acceptance criteria:**
- `test_effect_stack_add_remove`: round-trip.
- `test_tick_turn_decrements`: an effect with `Turns(2)` survives one tick, drops on the second.
- `test_tick_turn_returns_dropped_ids`: dropped effects are returned.
- `test_end_gig_drops_marker_effects`: `UntilGigEnd` durations expire on `end_gig`.
- `test_iter_modifiers_flat`: iterating modifiers across multiple effects is flat.

**Notes:**
- This WP defines the **shape** of `CriticalInjuryKind`, `SkillId`, etc. as stubs. WP-205 (Critical Injury Tables) and WP-201 (Skills) will define their actual variants and content. To avoid blocking, use `String`-newtype IDs as placeholders, then refactor once the catalog WPs land.
- `EffectStack` does **not** apply modifiers — it stores them. Application happens at query sites in the relevant character/combat code.
- `tick_turn`, `end_round`, `end_gig`, `end_netrun` are called by the combat / GM crates at the right lifecycle points.

---

#### WP-004 — Character data skeleton
**Crate:** `rules`
**Module:** `crates/rules/src/character/mod.rs`, `crates/rules/src/character/data.rs`
**Depends on:** WP-001, WP-003
**Blocks:** WP-104, WP-105, WP-501–503
**Estimate:** Medium

**Rulebook:** pp.71–80.

**Description:** Define the `Character` struct as a pure data container. **No methods that compute current values yet** — those are WP-104. This WP gives every other WP a stable type to reference.

**Public API to add:**
```rust
pub struct Character {
    pub id: CharacterId,
    pub name: String,
    pub handle: Option<String>,
    pub role: Role,
    pub role_rank: u8,                  // 1..=10
    pub stats: StatBlock,               // base, immutable post-creation
    pub skills: SkillSet,               // base ranks
    pub cyberware: Vec<InstalledCyberware>,
    pub armor: WornArmor,
    pub inventory: Inventory,
    pub wounds: Wounds,
    pub humanity: i16,
    pub luck_pool: u8,
    pub money: Eurobucks,
    pub improvement_points: u32,
    pub lifepath: Lifepath,
    pub effects: EffectStack,
}

pub struct StatBlock {
    pub int: u8, pub r#ref: u8, pub dex: u8, pub tech: u8, pub cool: u8,
    pub will: u8, pub luck: u8, pub r#move: u8, pub body: u8, pub emp: u8,
}

pub struct SkillSet { /* HashMap<SkillId, u8> */ }

pub enum Role { Rockerboy, Solo, Netrunner, Tech, Medtech, Media, Lawman, Exec, Fixer, Nomad }

pub struct Wounds {
    pub current_hp: i16,                // can go negative briefly during damage application
    pub max_hp: u16,
    pub seriously_wounded_threshold: u16,
    pub death_save_base: u8,            // = base BODY
    pub death_save_penalty: u8,
    pub current_state: WoundState,
}

pub struct WornArmor {
    pub head: Option<ArmorPiece>,
    pub body: Option<ArmorPiece>,
}

pub struct ArmorPiece {
    pub kind: ArmorKind,
    pub current_sp: u8,
    pub max_sp: u8,
}

// Stubs for catalog-defined types
pub struct InstalledCyberware { pub id: CyberwareId, pub options: Vec<CyberwareId> }
pub struct Inventory { pub items: Vec<ItemStack> }
pub struct ItemStack { pub kind: ItemKind, pub quantity: u32 }
pub enum ItemKind { Weapon(WeaponId), Ammo(AmmoKind, u32), Misc(String) }
pub struct WeaponId(pub String);
pub enum AmmoKind { Pistol, Rifle, Shotgun, /* ... */ }
pub enum ArmorKind { /* filled by WP-203 */ }
pub struct Lifepath { /* filled by WP-214 */ }
```

**Acceptance criteria:**
- `test_character_serializes_round_trip`: a Character serializes to RON and deserializes back identically.
- `test_default_wound_state`: a freshly-created character (full HP) is `WoundState::Lightly`. (Confirm — at full HP, RAW says "Less than Full HP" is Lightly; full HP is unwounded, no penalties. Make `Lightly` apply at <full and add `Unwounded` if useful.)
- `test_humanity_below_zero_legal`: setting `humanity = -1` is structurally valid (cyberpsychosis is gameplay, not a struct invariant).

**Notes:**
- Resolve the "Unwounded vs Lightly" naming based on RAW. Per p.186, "Less than Full HP" is Lightly Wounded with no penalty. Full HP is just full HP with no wound state. Add `WoundState::None` if it makes downstream code cleaner.
- All `Stub` types here will be replaced with concrete enums by Phase 2 catalog WPs. Keep the placeholders compileable.

---

#### WP-005 — Resolution trait and outcome envelope
**Crate:** `rules`
**Module:** `crates/rules/src/resolution.rs`
**Depends on:** WP-001, WP-002
**Blocks:** WP-102, all attack WPs
**Estimate:** Small

**Description:** Define the trait every dice-rolling action implements, plus shared outcome scaffolding.

**Public API to add:**
```rust
/// Anything in the rules engine that produces a probabilistic outcome implements this.
/// Implementations MUST consume the RNG deterministically — never branch on time, never read thread-local state.
pub trait Resolution {
    type Outcome;
    fn resolve(&self, world: &mut World, rng: &mut Rng) -> Self::Outcome;
}

/// Shared structured outcome record for any roll-vs-DV check.
/// Used by skill checks, attacks, NET actions.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CheckBreakdown {
    pub stat_value: i16,
    pub skill_value: i16,
    pub modifier_total: i16,         // sum of relevant EffectModifiers
    pub luck_spent: u8,
    pub d10: CritD10,
    pub final_value: i16,
    pub dv: DV,
    pub success: bool,
    pub margin: i16,                 // final_value - dv (can be negative)
}

/// `World` is the mutable game state passed to resolutions.
pub struct World { /* fields populated by WP-006 */ }
```

**Acceptance criteria:**
- `test_check_breakdown_success_flag`: `success == margin >= 0`.
- `test_check_breakdown_serializes`: round-trip via RON.

**Notes:**
- `World` is a sketch; WP-006 fleshes it out. For now, an empty struct is fine.

---

#### WP-006 — World container
**Crate:** `rules`
**Module:** `crates/rules/src/world.rs`
**Depends on:** WP-004, WP-005
**Blocks:** all combat WPs, all gm WPs
**Estimate:** Medium

**Description:** The `World` struct holds all live game state during a play session — the player character, NPC entities, current location, current combat or netrun state.

**Public API to add:**
```rust
pub struct World {
    pub pc: Character,
    pub npcs: HashMap<NpcId, Character>,    // allies and adversaries on-scene
    pub location: Option<LocationId>,
    pub combat: Option<CombatState>,        // populated when in combat
    pub netrun: Option<NetrunState>,        // populated during a netrun
    pub gig: Option<GigState>,              // populated during a gig
    pub clock: GameClock,
}

impl World {
    pub fn new(pc: Character) -> Self;
    pub fn entity(&self, id: EntityId) -> Option<&Character>;
    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Character>;
}

pub struct GameClock {
    pub day: u32,
    pub minutes_into_day: u16,              // 0..1440
}

// Stubs — populated by other WPs
pub struct CombatState;       // WP-301
pub struct NetrunState;       // WP-401
pub struct GigState;          // WP-604
pub struct LocationId(pub String);
```

**Acceptance criteria:**
- `test_world_construction`: `World::new(pc)` yields a world with the PC and no NPCs.
- `test_entity_lookup_pc`: looking up the PC's `EntityId` returns the PC.
- `test_entity_lookup_missing`: looking up an unknown id returns `None`.

**Notes:** `EntityId` and `NpcId` are distinct. The PC's `EntityId` is derived from their `CharacterId` once at world creation. NPCs get a fresh `EntityId` each scene.


---

### Phase 1 — Core Rules Mechanics

**Parallelism:** ~7 agents. All depend only on Phase 0.

#### WP-101 — Skill check resolution
**Crate:** `rules`
**Module:** `crates/rules/src/checks/skill_check.rs`
**Depends on:** WP-002, WP-003, WP-005, WP-006
**Blocks:** WP-102 (LUCK), all attack WPs, all NET ability WPs
**Estimate:** Medium

**Rulebook:** pp.128–130.

**Description:** Implement the core skill check primitives — both the DV variant and the opposed variant — with full crit handling, modifier application, and breakdown reporting.

**Public API to add:**
```rust
pub struct SkillCheck {
    pub actor: EntityId,
    pub stat: Stat,
    pub skill: SkillId,
    pub dv: DV,
    pub luck_to_spend: u8,
    pub additional_modifiers: Vec<NamedModifier>,  // GM-applied or Beat-applied modifiers
}

pub struct OpposedCheck {
    pub attacker: EntityId,
    pub attacker_stat: Stat,
    pub attacker_skill: SkillId,
    pub attacker_luck: u8,
    pub defender: EntityId,
    pub defender_stat: Stat,
    pub defender_skill: SkillId,
    pub defender_luck: u8,
    pub additional_attacker_modifiers: Vec<NamedModifier>,
    pub additional_defender_modifiers: Vec<NamedModifier>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NamedModifier { pub label: String, pub value: i8 }

pub struct OpposedOutcome {
    pub attacker_breakdown: CheckBreakdown,
    pub defender_breakdown: CheckBreakdown,
    pub attacker_wins: bool,    // tie → defender wins (p.129)
}

impl Resolution for SkillCheck { type Outcome = CheckBreakdown; /* ... */ }
impl Resolution for OpposedCheck { type Outcome = OpposedOutcome; /* ... */ }
```

**Acceptance criteria:**
- `test_simple_check_against_dv9`: STAT=5, Skill=4, no modifiers, seed forcing d10=5 → final=14, success.
- `test_critical_success_propagates`: forced d10=10 → CritD10 with follow-up, final reflects both.
- `test_critical_failure_can_negate_high_skill`: STAT=8, Skill=8, DV=15, forced d10=1, follow-up=10 → final=8+8+1-10 = 7, fails.
- `test_opposed_tie_defender_wins`: equal final values → `attacker_wins == false`.
- `test_modifier_total_applied`: a `-2` complex-task modifier reduces the final by 2.
- `test_no_skill_uses_stat_only`: passing a `SkillId` with rank 0 in the actor's `SkillSet` yields `skill_value == 0` (p.130: "When You Don't Have A Skill" — STAT only).
- `test_luck_spent_increases_check`: spending 3 LUCK points adds +3 to final and decrements `luck_pool` (mutated through `World`).

**Notes:**
- LUCK is **pre-commit** (p.130): `luck_to_spend` is fixed before the d10 is rolled. Decrement the actor's `luck_pool` *before* the d10 roll so seeding is deterministic.
- `luck_to_spend` MUST be ≤ actor's current `luck_pool`. Return `Err` if not. Define `RulesError::InsufficientLuck`.
- The `additional_modifiers` field is what the GM/Beat applies (e.g. "low light: -1", "complex task: -2"). The actor's *own* persistent modifiers (cyberware, wounds, drugs) are pulled from their `EffectStack` and applied automatically inside `resolve`.

---

#### WP-102 — Complementary skill check
**Crate:** `rules`
**Module:** `crates/rules/src/checks/complementary.rs`
**Depends on:** WP-101
**Blocks:** Beat hooks that use complementary checks
**Estimate:** Small

**Rulebook:** p.130 (Complementary Skills).

**Description:** Implement the complementary check rule: a successful related Skill grants +1 to the next use of a related Skill.

**Public API to add:**
```rust
/// A single-use +1 marker placed on the next use of a target skill.
pub struct ComplementaryBonus {
    pub target_skill: SkillId,
    pub granted_by: SkillId,
    pub consumed: bool,
}

impl Character {
    pub fn add_complementary_bonus(&mut self, bonus: ComplementaryBonus);
    /// Returns and consumes any pending complementary bonus for this skill.
    pub fn take_complementary_bonus(&mut self, skill: &SkillId) -> Option<ComplementaryBonus>;
}
```

**Acceptance criteria:**
- `test_complementary_grants_plus_one`: a skill check that has a pending bonus gets +1 to its breakdown.
- `test_complementary_consumed_after_one_use`: second use of the same skill does not get the bonus.
- `test_complementary_does_not_stack`: two pending bonuses to the same skill grant only +1 (p.130).

**Notes:** Storage of pending bonuses goes on `Character` (not in `EffectStack`) because they are skill-specific and one-shot.

---

#### WP-103 — LUCK pool lifecycle
**Crate:** `rules`
**Module:** `crates/rules/src/character/luck.rs`
**Depends on:** WP-004
**Blocks:** WP-101
**Estimate:** Small

**Rulebook:** p.130 (Using Your LUCK).

**Description:** Refill and spending of the LUCK pool, tied to gig boundaries.

**Public API to add:**
```rust
impl Character {
    /// Refill LUCK pool to its current LUCK STAT value. Called by gm crate at gig start.
    pub fn refill_luck(&mut self);
    /// Try to spend `n` luck. Returns Err if pool is insufficient.
    pub fn spend_luck(&mut self, n: u8) -> Result<(), RulesError>;
    pub fn luck_remaining(&self) -> u8;
}
```

**Acceptance criteria:**
- `test_refill_to_stat`: after `refill_luck()`, pool == LUCK STAT.
- `test_spend_decrements`: spending 3 from a pool of 5 leaves 2.
- `test_spend_more_than_available_errors`: spending 6 from a pool of 5 returns `Err(InsufficientLuck)`.

---

#### WP-104 — Character stat derivation (current values)
**Crate:** `rules`
**Module:** `crates/rules/src/character/derive.rs`
**Depends on:** WP-003, WP-004
**Blocks:** WP-101 (uses current_stat), all attack WPs
**Estimate:** Medium

**Rulebook:** stats pp.72–73, derived stats pp.79–80.

**Description:** Implement all the `current_X()` accessors. These walk the `EffectStack` to compute current effective values. **No caching.**

**Public API to add:**
```rust
impl Character {
    pub fn current_stat(&self, stat: Stat) -> i16;       // base + bonuses - penalties
    pub fn current_int(&self) -> i16;
    pub fn current_ref(&self) -> i16;
    pub fn current_dex(&self) -> i16;
    pub fn current_tech(&self) -> i16;
    pub fn current_cool(&self) -> i16;
    pub fn current_will(&self) -> i16;
    pub fn current_luck(&self) -> i16;
    pub fn current_move(&self) -> i16;                   // floored at 1 if positive penalties
    pub fn current_body(&self) -> i16;
    pub fn current_emp(&self) -> i16;                    // = floor(humanity / 10) — see notes

    pub fn current_skill(&self, skill: &SkillId) -> i16; // base rank + bonuses - penalties
    pub fn skill_base(&self, skill: &SkillId) -> i16;    // = current_stat(linked) + current_skill(skill)

    pub fn all_actions_penalty(&self) -> i8;             // sum of EffectModifier::AllActionsPenalty
    pub fn cannot_take_action(&self) -> bool;
    pub fn cannot_take_move_action(&self) -> bool;
    pub fn cannot_dodge(&self) -> bool;
}
```

**Acceptance criteria:**
- `test_current_dex_no_effects`: equals base.
- `test_current_dex_with_armor_penalty`: armor penalty effect of -2 → current_dex = base - 2.
- `test_move_floored_at_one`: base MOVE 5 with -10 of penalties → current_move = 1.
- `test_emp_follows_humanity_tens`: humanity 44 → EMP 4; humanity 39 → EMP 3 (p.80).
- `test_all_actions_sums_multiple_sources`: wound state -2 + crushed-fingers (which is hand-specific, NOT all actions) → all_actions = -2.
- `test_cannot_take_action_from_spinal_injury`: an effect with `CannotTakeAction` returns true.

**Notes:**
- **EMP rule (p.80):** EMP = floor(humanity / 10), but humanity floors at 0 for the purposes of EMP (negative humanity → EMP 0 + cyberpsychosis state separately). Confirm against rulebook.
- `current_emp` should derive from `humanity`, NOT from the base stat block's `emp` field. The `emp` field tracks max EMP (set at character creation = humanity / 10).
- Apply penalty-as-negative consistently. `StatPenalty { by: 2 }` means -2.

---

#### WP-105 — HP and Humanity derivation
**Crate:** `rules`
**Module:** `crates/rules/src/character/hp.rs`
**Depends on:** WP-004, WP-104
**Blocks:** WP-303
**Estimate:** Small

**Rulebook:** p.79 (Hit Points), p.80 (Humanity).

**Description:** Derive max HP, seriously-wounded threshold, base death save, and starting humanity from STATs.

**Public API to add:**
```rust
impl Character {
    /// HP = 10 + 5 × ceil((BODY + WILL) / 2)
    pub fn calculate_max_hp(&self) -> u16;
    /// Half of max HP, rounded up.
    pub fn calculate_seriously_wounded_threshold(&self) -> u16;
    /// Death save base = current BODY (NOT base BODY — confirm at p.79).
    pub fn calculate_death_save(&self) -> u8;
    /// Humanity = 10 × EMP at creation.
    pub fn calculate_starting_humanity(emp: u8) -> i16;
}

/// Recalculate Wounds after a permanent change to BODY/WILL (e.g. cyberlimb install).
pub fn recompute_wounds(character: &mut Character);
```

**Acceptance criteria:**
- `test_hp_formula_matches_table`: BODY 4, WILL 5 → 30 HP (cross-check against p.79 table).
- `test_seriously_wounded_rounds_up`: max_hp 35 → threshold 18.
- `test_starting_humanity_eq_10x_emp`: EMP 5 → humanity 50.
- `test_recompute_wounds_preserves_damage`: a wounded character with current_hp = 12 / max_hp = 30, BODY raised to 6, max_hp recomputed to 35; current_hp stays at 12.

**Notes:** The book gives both formula and table; table is authoritative on rounding. Cross-check 4 BODY × 5 WILL: avg = 4.5, ceil = 5, so HP = 10 + 5×5 = 35? But the table on p.79 reads BODY 4, WILL 5 → 35. Verify against the actual table in your read. Document the formula you settle on with a citation.

---

#### WP-106 — Wound states and Death Saves
**Crate:** `rules`
**Module:** `crates/rules/src/character/wounds.rs`
**Depends on:** WP-002, WP-003, WP-004, WP-105
**Blocks:** WP-303 (damage pipeline applies wound state changes)
**Estimate:** Medium

**Rulebook:** p.186 (Wound States), p.186 (Mortally Wounded death save mechanic).

**Description:** Apply wound state transitions when HP changes, and roll death saves.

**Public API to add:**
```rust
impl Character {
    /// Recompute current wound state from current_hp. Apply or remove the matching
    /// EffectStack effect (Lightly = no penalty effect, Seriously = -2 all actions, Mortally = -4/-6 + death save).
    /// Returns the new state if it changed.
    pub fn update_wound_state(&mut self) -> Option<WoundState>;

    /// Roll a death save. Mortally wounded only. Mutates death_save_penalty per RAW.
    pub fn roll_death_save(&mut self, rng: &mut Rng) -> DeathSaveOutcome;
}

pub enum DeathSaveOutcome {
    Survived { roll: u8, target: u8 },
    Died { roll: u8, target: u8 },
}
```

**Acceptance criteria:**
- `test_lightly_at_less_than_full_no_penalty`: HP=29, max=30 → Lightly, no all_actions penalty effect.
- `test_seriously_at_half`: HP at threshold → Seriously, -2 all actions, -4 not yet.
- `test_mortally_at_zero`: HP=0 → Mortally, -4 all actions, -6 MOVE.
- `test_mortally_to_dead`: failed death save → state Dead.
- `test_death_save_penalty_increments`: each turn at Mortally increments the penalty per RAW (verify on p.186).
- `test_state_replaces_previous`: moving Lightly → Seriously removes the Lightly effect (which is a no-op effect anyway).

**Notes:**
- Per p.186, "Mortally Wounded Characters suffer a Critical Injury whenever they are damaged by an Attack. In addition, their Death Save Penalty increases by 1." Confirm whether the penalty increases per *turn* or per *damage event*. Cite the rule.
- Death save target = current `death_save_base + death_save_penalty`. Roll d10 (no crits per p.186 — verify); if roll ≤ target → survived; if > target → died. Read carefully — Cyberpunk Red's death save direction is sometimes counterintuitive.
- Mark the Mortally Wounded effect with a `DeathSavePenaltyDelta(+1)` triggered on tick? Or update it manually? Decide and document.

---

#### WP-107 — Movement primitives (grid agnostic)
**Crate:** `rules`
**Module:** `crates/rules/src/movement.rs`
**Depends on:** WP-104
**Blocks:** WP-302
**Estimate:** Small

**Rulebook:** pp.126–127 (Walking/Running, m/yd ↔ square conversion).

**Description:** Distance and pace conversions independent of any grid implementation.

**Public API to add:**
```rust
pub fn move_distance_meters(character: &Character) -> u16 { /* current_move() * 2 */ }
pub fn move_distance_squares(character: &Character) -> u16 { /* current_move() */ }

/// 1 square = 2 m/yds (p.126).
pub const METERS_PER_SQUARE: u16 = 2;

pub fn meters_to_squares(m: u16) -> u16;
pub fn squares_to_meters(s: u16) -> u16;
```

**Acceptance criteria:**
- `test_move_distances`: MOVE 6 → 12m, 6 squares.
- `test_meters_to_squares_rounds_down`: 5m → 2 squares (or 3? Decide and document; 5m would naturally cover 2 full squares + 1m partial. RAW: see p.126 — "must move whole squares"). Document the choice.

**Notes:** The grid itself (pathfinding, line of sight, occupancy) is WP-302. This WP is just unit conversion.


---

### Phase 2 — Data Catalogs

**Parallelism:** ~14 agents fully parallel after Phase 0. Each WP defines a schema, writes the RON, and provides a loader. None depend on each other.

**Common pattern for every Phase 2 WP:**
- Schema lives in `crates/rules/src/catalog/<thing>.rs`.
- RON file lives in `content/catalogs/<thing>.ron`.
- Loader signature: `pub fn load_<thing>_catalog(path: &Path) -> Result<Catalog<Foo>, RulesError>`.
- Acceptance: round-trip RON, every entry from the rulebook is present, every entry validates.

```rust
pub struct Catalog<T> {
    entries: HashMap<String, T>,    // keyed by ID slug (e.g. "medium_pistol")
}
impl<T> Catalog<T> {
    pub fn get(&self, id: &str) -> Option<&T>;
    pub fn iter(&self) -> impl Iterator<Item = (&String, &T)>;
    pub fn len(&self) -> usize;
}
```

#### WP-201 — Skill catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/skills.rs`, `content/catalogs/skills.ron`
**Depends on:** WP-001
**Blocks:** WP-104, WP-501–503, WP-510–519
**Estimate:** Medium

**Rulebook:** pp.81–90 (full skill list), pp.130–142 (skill descriptions with rank examples).

**Description:** Catalog every skill in the book with its linked stat, x2 cost flag, parameterisation, and category.

**Public API to add:**
```rust
pub enum SkillId {
    // Awareness
    Concentration, ConcealRevealObject, LipReading, Perception, Tracking,
    // Body
    Athletics, Contortionist, Dance, Endurance, ResistTortureDrugs, Stealth,
    // Control
    DriveLandVehicle, PilotAirVehicle, PilotSeaVehicle, Riding,
    // Education
    AccountingFinance, AnimalHandling, Bureaucracy, Business, Composition,
    Criminology, Cryptography, Deduction, Education, Gamble,
    Language(LanguageKind), LibrarySearch, LocalExpert(LocalArea), Science(ScienceField),
    Tactics, WildernessSurvival,
    // Fighting
    Brawling, Evasion, MartialArts(MartialArtsForm), MeleeWeapon,
    // Performance
    Acting, PlayInstrument(Instrument),
    // Ranged
    Archery, Autofire, Handgun, HeavyWeapons, ShoulderArms,
    // Social
    Bribery, Conversation, HumanPerception, Interrogation, Persuasion,
    PersonalGrooming, Streetwise, Trading, WardrobeStyle,
    // Technique
    AirVehicleTech, BasicTech, Cybertech, DemolitionsTech, ElectronicsSecurityTech,
    FirstAid, Forgery, LandVehicleTech, PaintDrawSculpt, Paramedic,
    PhotographyFilm, PickLock, PickPocket, SeaVehicleTech, Weaponstech,
}

pub struct SkillDefinition {
    pub id: SkillId,
    pub display_name: String,
    pub linked_stat: Stat,
    pub category: SkillCategory,
    pub double_cost: bool,             // (×2) skills
    pub description: String,
}

pub enum SkillCategory { Awareness, Body, Control, Education, Fighting, Performance, Ranged, Social, Technique }

pub enum LanguageKind { /* well-known languages from rulebook flavour */ Streetslang, English, Japanese, /* ... */ Custom(String) }
pub enum ScienceField { Geology, Mathematics, Physics, Zoology, Anthropology, Biology, Chemistry, History, Custom(String) }
pub enum MartialArtsForm { Karate, Taekwondo, Judo, Aikido, Boxing, Capoeira, Wrestling, AnimalKungFu, Custom(String) }
pub enum Instrument { Singing, Guitar, Drums, Violin, Piano, Custom(String) }
pub enum LocalArea { Custom(String) }  // always parameterised by location
```

**Acceptance criteria:**
- `test_all_skills_loaded`: catalog count equals the book's skill count (verify by reading pp.81–90).
- `test_double_cost_flagged`: Autofire, Heavy Weapons, Martial Arts, and any other (×2) skill have `double_cost: true`.
- `test_linked_stat_correct_sample`: Athletics → DEX, Concentration → WILL, Tactics → INT (sample 10 skills).
- `test_parameterised_skills_serializable`: `Language(Japanese)`, `Science(Custom("Astrophysics"))`, `MartialArts(Karate)` round-trip via RON.

**Notes:** Use the per-skill rank descriptions in the book (Rank 10/14/18 examples) as flavour for the LLM's narration; include them in the `description` field even if the engine ignores them.

---

#### WP-202 — Weapon catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/weapons.rs`, `content/catalogs/weapons.ron`
**Depends on:** WP-001, WP-201
**Blocks:** WP-306, WP-307, WP-309
**Estimate:** Medium

**Rulebook:** pp.170–171 (ranged), pp.176–179 (melee), pp.340+ (Night Market detail).

**Description:** Every weapon in the book — ranged and melee — with damage dice, ROF, magazine, ammo type, hands, concealability, special features, cost.

**Public API to add:**
```rust
pub struct Weapon {
    pub id: WeaponId,
    pub display_name: String,
    pub kind: WeaponKind,
    pub skill: SkillId,
    pub damage: DamageDice,             // e.g. 2d6, 5d6, 3d6
    pub rof: u8,                        // 1 or 2
    pub hands: u8,                      // 1 or 2
    pub concealable: bool,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
    pub features: Vec<WeaponFeature>,
    pub magazine: Option<Magazine>,
    pub ranges: RangeBand,              // for ranged weapons
}

pub enum WeaponKind {
    Ranged(RangedKind),
    Melee(MeleeKind),
    Thrown,
    ExoticRanged,
    ExoticMelee,
}

pub enum RangedKind { MediumPistol, HeavyPistol, VeryHeavyPistol, SMG, HeavySMG, Shotgun, AssaultRifle, SniperRifle, BowCrossbow, GrenadeLauncher, RocketLauncher }
pub enum MeleeKind { Light, Medium, Heavy, VeryHeavy }

pub enum WeaponFeature {
    Autofire(u8),                       // capped multiplier (3 or 4)
    SuppressiveFire,
    ShotgunShell,
    Arrows,
    Explosive,
    SilentNotSilenced,                  // bows
}

pub struct DamageDice { pub n: u8, pub die: DieKind }
pub enum DieKind { D6, D10 }
pub struct Magazine { pub capacity: u8, pub ammo: AmmoKind }
pub enum AmmoKind { MPistol, HPistol, VHPistol, Slug, Rifle, Arrow, Grenade, Rocket }

/// DV-by-range table per weapon kind.
pub struct RangeBand {
    /// Each entry: (max meters of band, single-shot DV). Use u16::MAX for "no max".
    pub single_shot: Vec<(u16, u8)>,
    pub autofire: Option<Vec<(u16, u8)>>,
}
```

**Acceptance criteria:**
- `test_weapon_catalog_size`: matches the rulebook's table count.
- `test_assault_rifle_ranges`: assault rifle DV at 13–25m is 15, at 401–800m is 30 (verify against p.172).
- `test_handgun_skill_link`: medium pistol → SkillId::Handgun.
- `test_autofire_caps`: SMG cap 3, Assault Rifle cap 4 (p.173).

**Notes:** Cross-reference p.171 weapon table and p.172 range table. Some weapons (sniper rifle) have inverted ranges where short-range DV is high — preserve that.

---

#### WP-203 — Armor catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/armor.rs`, `content/catalogs/armor.ron`
**Depends on:** WP-001
**Blocks:** WP-303
**Estimate:** Small

**Rulebook:** p.185 (armor table).

**Description:** Every armor type with SP, penalty, locations it can be worn, cost.

**Public API to add:**
```rust
pub struct Armor {
    pub id: ArmorId,
    pub display_name: String,
    pub kind: ArmorKind,
    pub sp: u8,
    pub penalty: ArmorPenalty,
    pub locations: Vec<ArmorLocation>,  // [Body], [Head], [Body, Head]
    pub price: PriceTier,
    pub price_eb: Eurobucks,
}

pub struct ArmorPenalty { pub ref_penalty: u8, pub dex_penalty: u8, pub move_penalty: u8 }
pub enum ArmorLocation { Body, Head }
pub struct ArmorId(pub String);

pub enum ArmorKind { Leathers, Kevlar, LightArmorjack, BodyweightSuit, MediumArmorjack, HeavyArmorjack, Flak, Metalgear, BulletproofShield /* moved to shield catalog if separate */ }
```

**Acceptance criteria:**
- `test_armor_table_complete`: all 8 armor entries from p.185 present.
- `test_metalgear_sp_18`: Metalgear SP = 18, penalty -4 (p.185).
- `test_kevlar_no_penalty`: Kevlar penalty = 0 (p.185).

**Notes:** Per p.185, only the highest SP in a location applies; armor stacking is NOT additive. Note this in the docs but enforce it at WP-303 (damage application) not here.

---

#### WP-204 — Cyberware catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/cyberware.rs`, `content/catalogs/cyberware.ron`
**Depends on:** WP-001, WP-003 (for EffectModifier in cyberware effects)
**Blocks:** WP-501–503, WP-505
**Estimate:** Large

**Rulebook:** pp.110–124 abbreviated, pp.358+ detailed.

**Description:** Every cyberware item across the 8 categories (Fashionware, Neuralware, Cyberoptics, Cyberaudio, Internal Body, External Body, Cyberlimbs, Borgware). Includes install location, cost, HL (preset and dice), option slots.

**Public API to add:**
```rust
pub struct Cyberware {
    pub id: CyberwareId,
    pub display_name: String,
    pub category: CyberwareCategory,
    pub install_difficulty: InstallLocation,
    pub humanity_loss: HumanityLossSpec,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
    pub option_slots: u8,               // foundational pieces have slots; options use slots
    pub slot_cost: u8,                  // how many slots this item uses when installed
    pub prerequisite: Option<CyberwareId>,
    pub effects: Vec<EffectModifier>,   // ongoing modifiers while installed
    pub description: String,
}

pub enum CyberwareCategory { Fashionware, Neuralware, Cyberoptics, Cyberaudio, InternalBody, ExternalBody, Cyberlimb, Borgware }
pub enum InstallLocation { Mall, Clinic, Hospital }

pub enum HumanityLossSpec {
    Fixed(u8),                          // at character creation, this is what you pay
    Rolled { fixed: u8, dice: DamageDice },  // post-creation: e.g., 4 + 1d6
    None,                               // medical-grade or therapeutic
}
```

**Acceptance criteria:**
- `test_cyberware_catalog_complete`: all entries from pp.358–365 present.
- `test_neural_link_no_prereq`: Neural Link is foundational with no prerequisite.
- `test_interface_plugs_require_neural_link`: Interface Plugs prerequisite = Neural Link.
- `test_medical_grade_zero_hl`: medical-grade items have `HumanityLossSpec::None`.
- `test_humanity_loss_creation_vs_play`: at-creation HL is `Fixed`; in-play HL is `Rolled` (p.227).

**Notes:** This is a big catalog. Prefer to fully transcribe from the rulebook; the LLM cannot make up cyberware. Group RON files by category if a single 1000-line file is unwieldy (e.g. `cyberware/neuralware.ron`).

---

#### WP-205 — Critical Injury tables
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/critical_injuries.rs`, `content/tables/critical_injuries_body.ron`, `content/tables/critical_injuries_head.ron`
**Depends on:** WP-001, WP-003
**Blocks:** WP-305
**Estimate:** Medium

**Rulebook:** pp.187–188.

**Description:** Both 12-entry critical injury tables — Body (rolls 2–12) and Head (rolls 2–12). Each entry: name, narrative description, the EffectModifiers it applies, the bonus damage (always 5, ignores armor), the Quick Fix DV, the Treatment DV.

**Public API to add:**
```rust
pub enum CriticalInjuryKind {
    // Body table (p.187)
    DismemberedArm, DismemberedHand, CollapsedLung, BrokenRibs, BrokenArm,
    ForeignObject, BrokenLeg, TornMuscle, SpinalInjury, CrushedFingers, DismemberedLeg,
    // Head table (p.188)
    LostEye, BrainInjury, DamagedEye, ConcussionMild, BrokenJaw, /* etc. — confirm */
    WhiplashHead, CrackedSkull, /* ... */
}

pub struct CriticalInjury {
    pub kind: CriticalInjuryKind,
    pub table: CritTable,
    pub d2d6_roll: u8,                  // 2..=12
    pub display_name: String,
    pub effects: Vec<EffectModifier>,
    pub bonus_damage: HpDamage,         // always 5 per p.187
    pub quick_fix: Option<QuickFix>,
    pub treatment: Treatment,
    pub increases_death_save_penalty: bool,  // some criticals do (p.187)
}

pub struct QuickFix { pub method: HealMethod, pub dv: DV }
pub struct Treatment { pub method: HealMethod, pub dv: DV }
pub enum HealMethod { FirstAid, Paramedic, Surgery, NotApplicable }
pub enum CritTable { Body, Head }

pub fn roll_critical_injury(table: CritTable, already_suffering: &[CriticalInjuryKind], rng: &mut Rng) -> Option<CriticalInjuryKind>;
```

**Acceptance criteria:**
- `test_body_table_12_entries`: every roll 2..=12 returns a distinct kind.
- `test_head_table_12_entries`: same.
- `test_reroll_if_already_suffering`: if all 12 body criticals are already active, `roll_critical_injury` returns None (the rulebook says "until you get one not currently suffered" — see p.187).
- `test_dismembered_arm_increases_death_save`: per p.187.
- `test_broken_ribs_movement_trigger`: BrokenRibs creates an effect with `DamageOnMovementOver { threshold_m: 4, damage: HpDamage(5) }`.
- `test_bonus_damage_5`: every critical's `bonus_damage` is 5 (p.187).

**Notes:** This is the densest single-table WP. Read pp.187–188 cell by cell. Cross-check ongoing effects vs. one-time effects per entry.

---

#### WP-206 — Drugs and chemical effects catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/drugs.rs`, `content/catalogs/drugs.ron`
**Depends on:** WP-003
**Blocks:** Beat hooks involving drugs
**Estimate:** Small

**Rulebook:** pp.227–228 (Street Drugs).

**Description:** Catalog street drugs with their effects (as `EffectModifier` lists), durations, addiction profiles, costs.

**Public API to add:**
```rust
pub struct Drug {
    pub id: DrugId,
    pub display_name: String,
    pub effects: Vec<EffectModifier>,
    pub duration: EffectDuration,
    pub addictive: bool,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
    pub description: String,
}
```

**Acceptance criteria:**
- `test_drug_catalog_complete`: every drug from pp.227–228 present.
- `test_addictive_flag`: addictive drugs flagged correctly per book.

---

#### WP-207 — Cover materials catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/cover.rs`, `content/catalogs/cover.ron`
**Depends on:** WP-001
**Blocks:** WP-313
**Estimate:** Small

**Rulebook:** p.183 (Cover Material and Thickness Examples).

**Description:** Cover types with their HP values.

**Public API to add:**
```rust
pub struct CoverMaterial {
    pub id: String,
    pub display_name: String,
    pub example: String,                // "Bank Vault Door"
    pub material: MaterialKind,
    pub thickness: Thickness,
    pub hp: u16,
}

pub enum MaterialKind { Steel, Wood, Stone, Concrete, BulletproofGlass, PlasterFoamPlastic, Glass, Other }
pub enum Thickness { Thin, Thick }
```

**Acceptance criteria:**
- `test_cover_table_complete`: all entries from p.183 present.
- `test_thick_steel_50_hp`: matches rulebook.

---

#### WP-208 — Programs catalog (boosters, defenders, attackers)
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/programs.rs`, `content/catalogs/programs.ron`
**Depends on:** WP-001
**Blocks:** WP-412, WP-413
**Estimate:** Medium

**Rulebook:** pp.202–204 (Boosters/Defenders/Attackers). Black ICE in WP-209.

**Description:** Catalog all non-Black-ICE NET programs.

**Public API to add:**
```rust
pub struct Program {
    pub id: ProgramId,
    pub display_name: String,
    pub class: ProgramClass,
    pub atk: u8,
    pub def: u8,
    pub rez: u8,                        // program HP
    pub effect: ProgramEffect,
    pub icon: String,                   // descriptive — for narration
    pub price: PriceTier,
    pub price_eb: Eurobucks,
    pub slot_cost: u8,                  // 1 for normal, 2 for Black ICE
}

pub enum ProgramClass { Booster, Defender, AntiPersonnelAttacker, AntiProgramAttacker }

pub enum ProgramEffect {
    BoostCheck { check: BoostableCheck, by: i8 },
    BlockBlackIceDamage { reduction: u8 },        // Armor
    NullifyAttackerAtk,                           // Flak
    StopFirstNonBlackIceEffect,                   // Shield (one-shot, derezzes)
    AnyAttackerProgramDamage { dice_vs_non_black_ice: DamageDice, dice_vs_black_ice: DamageDice },  // Banhammer / Sword
    BrainDamageAndNetActionPenalty { dice: DamageDice, action_penalty: u8 },  // Vrizzbolt
    /* ... fill from rulebook */
}

pub enum BoostableCheck { Cloak, Pathfinder, Backdoor, Speed }
```

**Acceptance criteria:**
- `test_programs_catalog_complete`: all from pp.202–204.
- `test_eraser_boosts_cloak_by_2`: per p.203.
- `test_banhammer_dice`: 3d6 vs non-Black-ICE programs, 2d6 vs Black ICE (p.203).

---

#### WP-209 — Black ICE catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/black_ice.rs`, `content/catalogs/black_ice.ron`
**Depends on:** WP-001
**Blocks:** WP-414
**Estimate:** Medium

**Rulebook:** pp.205–207.

**Description:** All Black ICE programs with PER, SPD, ATK, DEF, REZ, effects.

**Public API to add:**
```rust
pub struct BlackIce {
    pub id: BlackIceId,
    pub display_name: String,
    pub class: BlackIceClass,
    pub per: u8,
    pub spd: u8,
    pub atk: u8,
    pub def: u8,
    pub rez: u8,
    pub effect: BlackIceEffect,
    pub icon: String,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
}

pub enum BlackIceClass { AntiPersonnel, AntiProgram, Demon }

pub enum BlackIceEffect {
    DestroyRandomProgram,
    BrainDamageAndForceJackOut { dice: DamageDice, jackout_consequence: JackOutKind },
    /* fill from rulebook */
}

pub struct BlackIceId(pub String);
```

**Acceptance criteria:**
- `test_black_ice_catalog_complete`: all entries from pp.205–207.
- `test_asp_destroys_program`: matches book.
- `test_hellhound_brain_damage`: matches book.

---

#### WP-210 — Demons catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/demons.rs`, `content/catalogs/demons.ron`
**Depends on:** WP-001
**Blocks:** WP-415
**Estimate:** Small

**Rulebook:** p.212 (Demons table).

**Description:** Imp, Efreet, Balron — large-scale Black ICE that defends a NET architecture.

**Public API to add:**
```rust
pub struct Demon {
    pub id: DemonId,
    pub display_name: String,
    pub rez: u16,
    pub interface: u8,
    pub net_actions_per_turn: u8,
    pub combat_number: u8,
    pub icon: String,
}
```

**Acceptance criteria:**
- `test_demon_catalog`: Imp/Efreet/Balron with stats from p.212.

---

#### WP-211 — Vehicle catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/vehicles.rs`, `content/catalogs/vehicles.ron`
**Depends on:** WP-001
**Blocks:** Vehicle combat (deferred per Phase 3 plan, but catalog can land now)
**Estimate:** Small

**Rulebook:** pp.323–325, p.190 (vehicle combat data).

**Description:** Cars, bikes, AVs with HP, SP, top speed, seats, cost.

**Public API to add:**
```rust
pub struct Vehicle {
    pub id: VehicleId,
    pub display_name: String,
    pub kind: VehicleKind,
    pub seats: u8,
    pub top_speed_kph: u16,
    pub hp: u16,
    pub sp: u8,
    pub combat_number: u8,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
}

pub enum VehicleKind { Bike, Car, Truck, AV, Boat, Other }
```

**Acceptance criteria:** all vehicles from pp.323–325 present.

---

#### WP-212 — Night Market gear catalog
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/night_market.rs`, `content/catalogs/night_market.ron`
**Depends on:** WP-001
**Blocks:** Economy / shopping flows
**Estimate:** Medium

**Rulebook:** pp.340–380 (Night Market Appendix).

**Description:** Gadgets, gear, services, ammunition, repair, drugs, food, lodging — everything sold at a Night Market that isn't already covered by weapons / armor / cyberware / drugs catalogs. Also each item's market category and Fixer rank required.

**Public API to add:**
```rust
pub struct NightMarketItem {
    pub id: String,
    pub display_name: String,
    pub category: MarketCategory,
    pub price: PriceTier,
    pub price_eb: Eurobucks,
    pub min_fixer_rank: u8,
    pub description: String,
    pub effects: Option<Vec<EffectModifier>>,    // for things like medkits, augments
}

pub enum MarketCategory {
    PersonalElectronics, MediumElectronics, ConsumerElectronics, /* ... */
    LowFood, GoodFood, ExcellentFood,
    Lodging,
    /* etc. */
}
```

**Acceptance criteria:** all items from pp.340–380 present.

---

#### WP-213 — Role definitions
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/roles.rs`, `content/catalogs/roles.ron`
**Depends on:** WP-001
**Blocks:** WP-501–503, WP-510–519
**Estimate:** Small

**Rulebook:** pp.29–42, p.142+ (Role Abilities).

**Description:** All ten roles with their core skill, role ability name, starting role rank.

**Public API to add:**
```rust
pub struct RoleDefinition {
    pub role: Role,
    pub display_name: String,
    pub role_ability: RoleAbilityKind,
    pub flavor_skill_emphasis: Vec<SkillId>,    // suggested at creation
}

pub enum RoleAbilityKind {
    CharismaticImpact,    // Rockerboy
    CombatSense,          // Solo
    Interface,            // Netrunner
    Maker,                // Tech
    Medicine,             // Medtech
    Credibility,          // Media
    Backup,               // Lawman
    Resources,            // Exec
    Operator,             // Fixer
    Moto,                 // Nomad
}
```

**Acceptance criteria:** all 10 roles with correct ability names per p.142+.

---

#### WP-214 — Lifepath tables
**Crate:** `rules`
**Module:** `crates/rules/src/catalog/lifepath.rs`, `content/tables/lifepath/*.ron`
**Depends on:** WP-001
**Blocks:** WP-504
**Estimate:** Medium

**Rulebook:** pp.43–70 (Tales from The Street).

**Description:** All lifepath tables — Cultural Origin, Personality, Clothing/Hairstyle, Affectations, Motivations, Life Goals, Family Background/Crisis, Friends/Enemies, Tragic Loves, plus role-specific lifepath tables (Rockerboy lifepath, Solo lifepath, etc.).

**Public API to add:**
```rust
pub struct LifepathTable<T> {
    pub die: u8,                        // d10
    pub entries: Vec<(u8, T)>,          // (roll, value)
}

pub struct Lifepath {
    pub cultural_region: String,
    pub language_spoken: String,
    pub personality: String,
    pub clothing_style: String,
    pub hairstyle: String,
    pub affectations: String,
    pub motivation: String,
    pub life_goal: String,
    pub family_background: FamilyBackground,
    pub family_crisis: String,
    pub friends: Vec<RelationshipBeacon>,
    pub enemies: Vec<RelationshipBeacon>,
    pub tragic_loves: Vec<RelationshipBeacon>,
    pub role_specific: RoleLifepath,    // enum per role
}

pub struct RelationshipBeacon {
    pub name: String,
    pub kind: BeaconKind,                // Friend, Enemy, Lover
    pub note: String,
}
```

**Acceptance criteria:** every lifepath table from pp.43–70 loadable. RON-driven random rolls produce valid `Lifepath` instances.

---


### Phase 3 — Combat Subsystems

**Parallelism:** ~16 agents. Most depend on Phase 1 + relevant Phase 2 catalogs.

#### WP-301 — Initiative and turn engine
**Crate:** `rules`
**Module:** `crates/rules/src/combat/turn_engine.rs`
**Depends on:** WP-002, WP-006, WP-104
**Blocks:** every attack WP, WP-417
**Estimate:** Medium

**Rulebook:** pp.126–127, p.168.

**Description:** Initiative roll, queue management, round wrap-around, top-of-queue insertion (for Black ICE activation).

**Public API to add:**
```rust
pub struct CombatState {
    pub round: u32,
    pub queue: Vec<InitiativeEntry>,
    pub turn_index: usize,
    pub grid: Grid,                     // populated by WP-302
    pub participants: HashSet<EntityId>,
}

pub struct InitiativeEntry {
    pub entity: EntityId,
    pub score: i16,                     // REF + d10 + modifiers
    pub move_used: bool,
    pub action_used: bool,
    pub held_action: Option<HeldAction>,
}

pub struct HeldAction { pub trigger: HoldTrigger, pub action: PlannedAction }
pub enum HoldTrigger { UntilInitiative(i16), UntilEvent(String) }
pub struct PlannedAction; // populated as actions WP land

impl CombatState {
    pub fn start(participants: Vec<EntityId>, world: &World, rng: &mut Rng) -> Self;
    pub fn current(&self) -> EntityId;
    pub fn end_turn(&mut self, world: &mut World) -> TurnEndEvents;
    pub fn insert_at_top(&mut self, entity: EntityId, world: &World, rng: &mut Rng);
    pub fn end_combat(self) -> CombatSummary;
}

pub struct TurnEndEvents { pub effects_dropped: Vec<EffectInstanceId> }
pub struct CombatSummary { pub rounds: u32, pub kills: Vec<EntityId> }
```

**Acceptance criteria:**
- `test_initiative_descending`: highest score goes first.
- `test_initiative_tiebreak_reroll`: ties resolved by re-rolling until distinct (p.168).
- `test_round_wraparound`: after the last entry, queue restarts at index 0 with `round + 1`.
- `test_insert_at_top_above_highest`: a Black ICE inserted at top has score = current_highest + 1 (p.205).
- `test_end_turn_ticks_effects`: Effect with `Turns(1)` drops on the actor's turn end.

**Notes:** Tie-break per RAW is "roll again". Implement that as a re-roll, not as a stat tiebreak. Don't include modifiers from `EffectStack` in the *initiative* roll — RAW p.168 says "REF + 1d10". If a cyberware grants `InitiativeBonus`, apply it; otherwise raw.

---

#### WP-302 — Grid, line of sight, and movement
**Crate:** `rules`
**Module:** `crates/rules/src/combat/grid.rs`
**Depends on:** WP-006, WP-107, WP-301
**Blocks:** WP-306, WP-307, WP-310, WP-311, WP-312
**Estimate:** Large

**Rulebook:** pp.126–127, pp.168–169, p.183 (cover).

**Description:** 2D grid (square cells, 2 m each), entity occupancy, pathfinding for movement, line-of-sight for ranged, line-of-effect for explosives, range queries in metres.

**Public API to add:**
```rust
pub struct Grid {
    pub width: u16,
    pub height: u16,
    pub tiles: Vec<TileKind>,           // row-major
    pub occupants: HashMap<(u16, u16), EntityId>,
    pub cover_objects: HashMap<(u16, u16), CoverInstance>,
}

pub enum TileKind { Open, Wall, Difficult, Water }

pub struct CoverInstance {
    pub material: String,                // CoverMaterial id from WP-207
    pub current_hp: u16,
    pub max_hp: u16,
}

impl Grid {
    pub fn place(&mut self, entity: EntityId, pos: (u16, u16));
    pub fn position_of(&self, entity: EntityId) -> Option<(u16, u16)>;
    pub fn move_entity(&mut self, entity: EntityId, path: &[(u16, u16)]) -> Result<(), GridError>;

    /// Available squares within a Move Action budget. Diagonals cost 1.
    pub fn movement_options(&self, from: (u16, u16), squares_budget: u16) -> Vec<(u16, u16)>;

    pub fn distance_squares(&self, a: (u16, u16), b: (u16, u16)) -> u16;
    pub fn distance_meters(&self, a: (u16, u16), b: (u16, u16)) -> u16;

    pub fn line_of_sight(&self, from: (u16, u16), to: (u16, u16)) -> LosResult;

    /// All entities within a cone (for shotgun shells). 6m/yd range, in front of attacker.
    pub fn cone_targets(&self, from: (u16, u16), facing: Facing, max_meters: u16) -> Vec<EntityId>;

    /// All entities in a 5x5 square area centered on `center` (for explosives).
    pub fn square_aoe_targets(&self, center: (u16, u16), radius_squares: u16) -> Vec<EntityId>;
}

pub enum LosResult { Clear, Blocked, ThroughCover(CoverInstance) }
pub enum Facing { N, NE, E, SE, S, SW, W, NW }
```

**Acceptance criteria:**
- `test_diagonal_costs_one`: moving from (0,0) to (1,1) costs 1 square (p.169 — diagonals included).
- `test_meters_between`: distance_meters((0,0),(3,0)) == 6m.
- `test_movement_options_with_budget`: a MOVE 4 character has 4 squares of options not blocked by walls.
- `test_los_blocked_by_wall`: a wall between two entities blocks LOS.
- `test_cone_targets_directional`: facing North, an entity due south is not in the cone.
- `test_square_aoe_5x5`: 10m × 10m AoE centered on a target picks up all 25 squares (p.174).

**Notes:** RAW p.169 — "you cannot stop in between the squares". So pathfinding can pass through squares but movement_options returns only positions where you can stop (i.e., unoccupied squares).

---

#### WP-303 — Damage pipeline and armor ablation
**Crate:** `rules`
**Module:** `crates/rules/src/combat/damage.rs`
**Depends on:** WP-002, WP-003, WP-105, WP-106, WP-203
**Blocks:** every attack WP, WP-305
**Estimate:** Medium

**Rulebook:** p.186 (When Armor Doesn't Cut It).

**Description:** Apply rolled damage to a target: subtract SP at the targeted location, ablate armor by 1, subtract remaining from HP, transition wound state, return a structured outcome.

**Public API to add:**
```rust
pub struct DamageApplication {
    pub target: EntityId,
    pub raw_damage: u16,
    pub location: HitLocation,           // Body or Head
    pub bypass_armor: bool,              // poisons, fire, bonus damage from criticals
    pub source_label: String,            // for narration
}

pub enum HitLocation { Body, Head }

pub struct DamageOutcome {
    pub target: EntityId,
    pub raw_damage: u16,
    pub sp_blocked: u16,
    pub hp_lost: u16,
    pub final_hp: i16,
    pub armor_ablated_to: Option<u8>,    // None if no armor in that location
    pub wound_state_change: Option<(WoundState, WoundState)>,
    pub triggered_critical: bool,        // attack-side rolled 2+ sixes — flag for caller to roll on table
    pub died: bool,
}

pub fn apply_damage(world: &mut World, dmg: DamageApplication) -> DamageOutcome;
```

**Acceptance criteria:**
- `test_armor_subtracts_sp`: 20 damage vs SP 11 body armor → 9 HP lost.
- `test_armor_ablates_one`: armor SP 11 → 10 after taking damage.
- `test_armor_does_not_ablate_on_zero_through`: SP 11 vs 5 damage → no HP lost, no ablation (p.186 — only ablates if "you ended up taking any damage").
- `test_bypass_armor_skips_sp`: poison damage applies fully.
- `test_wound_state_transition_seriously`: HP at threshold → Seriously, effect applied.
- `test_died_when_hp_zero_and_failed_save`: combine with wound state + death save (cross-WP integration test).

**Notes:** This is just the application step. The *roll* of damage dice (5d6, 2d6×margin, etc.) is the attack WP's responsibility; this WP receives a raw number. The `triggered_critical` flag is set by the attack WP and passed in — this WP doesn't roll it.

---

#### WP-304 — Wound state transitions and combat-time penalties
**Crate:** `rules`
**Module:** integrated into `WP-303` and `WP-106` — this WP is a small reconciliation pass.
**Depends on:** WP-303, WP-106
**Estimate:** Small

**Description:** Wire `apply_damage` to `update_wound_state`, and ensure wound effects (Seriously: -2 all actions; Mortally: -4 all actions, -6 MOVE) are added/removed via `EffectStack` correctly.

**Acceptance criteria:**
- `test_seriously_adds_minus_two_effect`: transition Lightly → Seriously adds an effect with `AllActionsPenalty(-2)`.
- `test_mortally_replaces_seriously`: transition Seriously → Mortally removes the -2 effect and adds the -4/-6 effect.
- `test_healing_back_removes_mortally`: heal up past threshold → wound effect removed.

---

#### WP-305 — Critical Injury application
**Crate:** `rules`
**Module:** `crates/rules/src/combat/critical_injury.rs`
**Depends on:** WP-205, WP-303
**Blocks:** WP-306, WP-307, WP-309
**Estimate:** Medium

**Rulebook:** pp.187–188.

**Description:** Trigger logic ("two or more 6s on damage dice"), table roll (filtered by already-suffered list), apply effects, apply 5 bonus damage that bypasses armor.

**Public API to add:**
```rust
pub fn check_critical_trigger(damage_rolls: &[u8]) -> bool;  // >=2 dice came up 6

pub fn apply_critical_injury(
    world: &mut World,
    target: EntityId,
    table: CritTable,
    rng: &mut Rng,
) -> Option<CriticalInjuryApplied>;

pub struct CriticalInjuryApplied {
    pub kind: CriticalInjuryKind,
    pub bonus_damage_outcome: DamageOutcome,
    pub effects_added: Vec<EffectInstanceId>,
    pub death_save_penalty_delta: i8,
}
```

**Acceptance criteria:**
- `test_critical_trigger_two_sixes`: damage rolls [6,6,3,2] → true.
- `test_critical_trigger_one_six`: [6,5,4,3] → false.
- `test_no_repeat_critical`: a target already suffering Broken Arm cannot get Broken Arm again — re-roll (p.187).
- `test_bonus_damage_bypasses_armor`: 5 bonus damage applied with `bypass_armor: true`.
- `test_aimed_head_uses_head_table`: caller passes `CritTable::Head`.
- `test_death_save_penalty_increments`: dismembered arm increments the death save penalty by 1.

**Notes:** Per p.187, criticals fire from MELEE or RANGED damage rolls. Autofire has its own crit trigger (both 2d6 = 6). Both call this same WP with `table: CritTable::Body` (unless head was aimed for).

---

#### WP-306 — Single-shot ranged attack
**Crate:** `rules`
**Module:** `crates/rules/src/combat/ranged_single.rs`
**Depends on:** WP-101, WP-202, WP-301, WP-302, WP-303, WP-305, WP-316
**Blocks:** WP-1001
**Estimate:** Medium

**Rulebook:** pp.170–172.

**Description:** Resolve a single-shot ranged attack — REF + Weapon Skill + d10 vs DV (from range table or defender's dodge if elected).

**Public API to add:**
```rust
pub struct RangedSingleAttack {
    pub attacker: EntityId,
    pub target: EntityId,
    pub weapon: WeaponId,
    pub aimed_shot: Option<AimedLocation>,
    pub luck_to_spend: u8,
    pub defender_dodges: bool,           // requires REF >= 8 (validated)
    pub additional_modifiers: Vec<NamedModifier>,
}

pub enum AimedLocation { Head, HeldItem, Leg }

pub struct RangedAttackOutcome {
    pub attack_breakdown: CheckBreakdown,
    pub defender_dodge_breakdown: Option<CheckBreakdown>,
    pub hit: bool,
    pub damage_rolls: Vec<u8>,
    pub damage_total: u16,
    pub damage_outcome: Option<DamageOutcome>,
    pub critical: Option<CriticalInjuryApplied>,
    pub aimed_shot_effect: Option<AimedShotEffect>,
}

pub enum AimedShotEffect { HeadDoubleDamage, DropHeldItem(WeaponId), BrokenLeg }

impl Resolution for RangedSingleAttack { type Outcome = RangedAttackOutcome; /* ... */ }
```

**Acceptance criteria:**
- `test_pistol_at_short_range_dv13`: a Medium Pistol at 4m → DV 13.
- `test_aimed_head_minus_8`: aimed shot subtracts 8 from the attack roll (p.170).
- `test_aimed_head_double_damage`: damage that gets through head SP is doubled.
- `test_dodge_rejected_below_ref_8`: defender_dodges = true with REF 7 → returns Err.
- `test_dodge_election_valid`: REF 8 dodge — Outcome includes the defender's dodge breakdown and the higher of (range DV, dodge result) is used.
- `test_critical_on_two_sixes`: damage [6,6,1,2,4] → triggers crit, outcome includes critical.
- `test_concealment_modifier_applied`: -4 stealth modifier in additional_modifiers reduces hit chance.

**Notes:**
- REF 8 dodge: the *defender* picks the higher of (range table DV) or (dodge roll). Implement as: roll dodge; if dodge result > range DV, use it; else use range DV.
- Aimed shot uses `Resolution`'s standard d10 — the -8 is in `additional_modifiers`.
- The single-shot DV table comes from the weapon's `RangeBand::single_shot`.

---

#### WP-307 — Melee attack
**Crate:** `rules`
**Module:** `crates/rules/src/combat/melee.rs`
**Depends on:** WP-101, WP-202, WP-302, WP-303, WP-305
**Blocks:** WP-1001
**Estimate:** Medium

**Rulebook:** pp.175–179.

**Description:** Melee attack — DEX + Melee Weapon (or Brawling, or Martial Arts) + d10 vs defender's DEX + Evasion + d10. Damage = weapon damage + (1 if BODY 5–7 / 2 if BODY 8–10 / 3 if BODY ≥11) (the BODY damage bonus, p.176 — verify).

**Public API to add:**
```rust
pub struct MeleeAttack {
    pub attacker: EntityId,
    pub target: EntityId,
    pub weapon_or_unarmed: MeleeWeaponChoice,
    pub luck_to_spend: u8,
    pub additional_modifiers: Vec<NamedModifier>,
    pub martial_arts_special_move: Option<MartialArtsSpecialMove>,
}

pub enum MeleeWeaponChoice {
    Weapon(WeaponId),
    Brawling,
    MartialArts(MartialArtsForm),
}

pub enum MartialArtsSpecialMove { Strike, Hold, Sweep, Throw, Disarm, Choke, Resolution }

pub struct MeleeAttackOutcome {
    pub attack_breakdown: CheckBreakdown,
    pub defender_breakdown: CheckBreakdown,
    pub hit: bool,
    pub damage_rolls: Vec<u8>,
    pub damage_total: u16,
    pub body_bonus: u8,
    pub damage_outcome: Option<DamageOutcome>,
    pub critical: Option<CriticalInjuryApplied>,
    pub special_move_effect: Option<SpecialMoveEffect>,
}
```

**Acceptance criteria:**
- `test_melee_opposed_check`: defender rolls DEX + Evasion.
- `test_body_bonus_applied`: BODY 6 → +1 damage.
- `test_brawling_uses_brawling_skill`: unarmed brawl uses `SkillId::Brawling`.
- `test_martial_arts_form_uses_specific_skill`: passing `MartialArts(Karate)` uses `SkillId::MartialArts(Karate)`.
- `test_defender_can_use_brawling_or_martial_arts`: per p.175, defender uses Evasion OR Brawling OR Martial Arts.

**Notes:** Per p.175, the defender on a melee attack uses Evasion OR Brawling OR Martial Arts (their choice). This is a pre-roll election. Add a field for the defender's choice or default to highest skill.

---

#### WP-308 — Aimed shots
**Crate:** `rules`
**Module:** integrated into WP-306 and WP-307; this WP is the data table.
**Depends on:** WP-306
**Estimate:** Small

**Description:** Tabulate the aimed shot effects (p.170). Already typed as `AimedShotEffect` in WP-306; this WP wires it up.

**Acceptance criteria:**
- `test_aimed_head_doubles_damage_through_head_sp`: 10 damage, head SP 4 → 6 gets through, doubled to 12 HP lost.
- `test_aimed_held_item_drops_on_one_through`: a single point of damage through body SP → drops one held item of attacker's choice.
- `test_aimed_leg_breaks_on_one_through`: same condition → Broken Leg crit applies (or skip if already broken).

---

#### WP-309 — Autofire
**Crate:** `rules`
**Module:** `crates/rules/src/combat/autofire.rs`
**Depends on:** WP-101, WP-202, WP-303, WP-305
**Blocks:** WP-1001
**Estimate:** Medium

**Rulebook:** pp.173–174.

**Description:** Autofire-specific resolution: REF + Autofire Skill + d10 vs autofire DV table; on hit, damage = 2d6 × min(beat_amount, weapon_cap); both d6 = 6 also triggers crit.

**Public API to add:**
```rust
pub struct AutofireAttack {
    pub attacker: EntityId,
    pub target: EntityId,
    pub weapon: WeaponId,                // must support Autofire feature
    pub luck_to_spend: u8,
    pub defender_dodges: bool,
    pub additional_modifiers: Vec<NamedModifier>,
}

pub struct AutofireOutcome {
    pub attack_breakdown: CheckBreakdown,
    pub beat_dv_by: u8,                   // capped at weapon's autofire cap
    pub damage_rolls: [u8; 2],
    pub damage_total: u16,
    pub damage_outcome: Option<DamageOutcome>,
    pub critical: Option<CriticalInjuryApplied>,
    pub bullets_consumed: u8,             // always 10 (p.173)
}
```

**Acceptance criteria:**
- `test_autofire_consumes_10_bullets`: bullets_consumed always 10.
- `test_autofire_requires_10_in_clip`: < 10 bullets → Err.
- `test_assault_rifle_cap_4`: beat by 7 capped to 4.
- `test_smg_cap_3`: beat by 5 capped to 3.
- `test_both_d6_six_triggers_crit`: damage_rolls [6,6] → critical applied.
- `test_autofire_cannot_aim`: aimed_shot rejected (p.173).

**Notes:** Autofire uses the *autofire DV table* (`RangeBand::autofire`), not the single-shot table. The damage cap per weapon is on `WeaponFeature::Autofire(cap)`.

---

#### WP-310 — Suppressive fire
**Crate:** `rules`
**Module:** `crates/rules/src/combat/suppressive.rs`
**Depends on:** WP-302, WP-303
**Estimate:** Small

**Rulebook:** p.174.

**Description:** Costs 10 bullets. Forces WILL + Concentration check from each affected enemy; failures must use their next Move Action to seek cover.

**Public API to add:**
```rust
pub struct SuppressiveFire {
    pub attacker: EntityId,
    pub area: SuppressiveArea,            // entities within 25m, in LOS, not in cover
    pub additional_modifiers: Vec<NamedModifier>,
}

pub struct SuppressiveOutcome {
    pub bullets_consumed: u8,             // 10
    pub forced_to_cover: Vec<EntityId>,
    pub resisted: Vec<EntityId>,
}
```

**Acceptance criteria:**
- `test_suppressive_consumes_10`: always 10 bullets.
- `test_only_in_los_no_cover`: targets behind cover or not in LOS are not affected.
- `test_failed_concentration_must_seek_cover`: failures appear in `forced_to_cover`.

---

#### WP-311 — Shotgun shells
**Crate:** `rules`
**Module:** `crates/rules/src/combat/shotgun_shell.rs`
**Depends on:** WP-302, WP-303
**Estimate:** Small

**Rulebook:** p.174.

**Description:** Shotgun shell mode: REF + Shoulder Arms + d10 vs DV13. If success, 3d6 damage to all targets in a 6m cone. Damage rolled once. Each target with REF≥8 may individually dodge.

**Public API to add:**
```rust
pub struct ShotgunShellAttack {
    pub attacker: EntityId,
    pub facing: Facing,
    pub luck_to_spend: u8,
}

pub struct ShotgunShellOutcome {
    pub attack_breakdown: CheckBreakdown,
    pub damage_rolls: Vec<u8>,            // 3d6 once
    pub damage_total: u16,
    pub per_target: Vec<(EntityId, ShotgunTargetOutcome)>,
}

pub enum ShotgunTargetOutcome { Dodged(CheckBreakdown), Hit(DamageOutcome) }
```

---

#### WP-312 — Explosives
**Crate:** `rules`
**Module:** `crates/rules/src/combat/explosives.rs`
**Depends on:** WP-302, WP-303
**Estimate:** Medium

**Rulebook:** p.174.

**Description:** Grenades and rockets. 10×10m square AoE centered on target square. Damage rolled once for all in area. Cover is destroyed if damage exceeds cover HP, otherwise cover blocks. REF≥8 individuals may dodge.

**Public API to add:**
```rust
pub struct ExplosiveAttack {
    pub attacker: EntityId,
    pub target_square: (u16, u16),
    pub weapon: WeaponId,                 // grenade launcher, rocket launcher
    pub luck_to_spend: u8,
}

pub struct ExplosiveOutcome {
    pub attack_breakdown: CheckBreakdown,
    pub final_blast_center: (u16, u16),   // may be off-target on miss
    pub damage_rolls: Vec<u8>,
    pub damage_total: u16,
    pub per_target: Vec<(EntityId, ExplosiveTargetOutcome)>,
    pub cover_destroyed: Vec<(u16, u16)>,
}

pub enum ExplosiveTargetOutcome { Dodged(CheckBreakdown), CoverBlocked, Hit(DamageOutcome) }
```

**Notes:** On miss, GM (LLM or fallback policy) picks the actual blast center within the original 10×10 square. Implement a deterministic fallback (e.g. roll a d8 for direction, scatter 1 square per missed-DV-by) and let the GM override.

---

#### WP-313 — Cover system
**Crate:** `rules`
**Module:** `crates/rules/src/combat/cover.rs`
**Depends on:** WP-207, WP-302, WP-303
**Estimate:** Small

**Description:** Apply cover material HP between an attack and a target. Cover absorbs damage up to its current HP, then breaks; remaining damage passes through.

**Acceptance criteria:**
- `test_cover_absorbs_up_to_hp`: 30 damage vs Thick Wood (20 HP) → 20 absorbed, 10 through, cover at 0.
- `test_cover_intact_partial`: 10 damage vs Thick Wood → 10 absorbed, 0 through, cover at 10 HP.
- `test_destroyed_cover_no_block`: cover at 0 HP → no protection.

---

#### WP-314 — Shields (regular and human)
**Crate:** `rules`
**Module:** `crates/rules/src/combat/shields.rs`
**Depends on:** WP-303, WP-313
**Estimate:** Medium

**Rulebook:** pp.183–184.

**Description:** Regular shields are equippable cover with HP. Human shields are grappled defenders used as cover.

**Public API to add:**
```rust
pub struct EquippedShield { pub kind: ShieldKind, pub current_hp: u16, pub max_hp: u16, pub hand_in_use: Hand }
pub enum ShieldKind { Bulletproof, HumanShield(EntityId) }
```

**Acceptance criteria:**
- `test_shield_takes_full_attack`: a shield interposed takes full damage to its HP.
- `test_shield_destroyed_at_zero_hp`: shield at 0 HP cannot be used as cover.
- `test_human_shield_takes_damage_normally`: damage to a human shield routes through their armor / HP normally.
- `test_human_shield_dies_becomes_corpse_shield`: human shield reaching 0 HP becomes a `HumanShield` with HP = their BODY (p.184).

---

#### WP-315 — Grappling, throwing, choking
**Crate:** `rules`
**Module:** `crates/rules/src/combat/grapple.rs`
**Depends on:** WP-101, WP-307
**Estimate:** Medium

**Rulebook:** p.177–178.

**Description:** Grapple as opposed Brawling/Martial Arts. Held targets are restricted (no actions / movement). Throw as Action; Choke as Action.

**Public API to add:**
```rust
pub struct GrappleAttempt { pub attacker: EntityId, pub target: EntityId }
pub struct ChokeAction { pub attacker: EntityId }     // implicit grappled target
pub struct ThrowAction { pub attacker: EntityId, pub target: ThrowTarget }
pub enum ThrowTarget { Square((u16, u16)), Object(WeaponId) }
```

---

#### WP-316 — REF≥8 dodge election
**Crate:** `rules`
**Module:** integrated into WP-306, WP-309, WP-311, WP-312.
**Description:** Common helper that surfaces the dodge choice point and validates the requirement (current_ref ≥ 8 — note: *current* REF, after armor penalties).

**Public API to add:**
```rust
pub fn can_elect_dodge_ranged(character: &Character) -> bool { character.current_ref() >= 8 }
```

**Acceptance criteria:**
- `test_armor_penalty_blocks_dodge`: a character with base REF 8 wearing Heavy Flak (-4 REF) has current_ref = 4 → cannot elect dodge.


---

### Phase 4 — Netrunning

**Parallelism:** ~17 agents. Most depend on Phase 1 + WP-208/209/210.

#### WP-401 — NET architecture model and procedural generator
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/architecture.rs`
**Depends on:** WP-001, WP-209, WP-210
**Blocks:** WP-404–417
**Estimate:** Medium

**Rulebook:** pp.209–212, pp.217–218.

**Description:** A NET Architecture is an ordered sequence of floors, each containing one of: Password, Control Node, File, Black ICE, or Demon. The model + a procedural generator that takes a `(price_tier, security_intent, fixer_rank)` and produces an architecture matching the cost rules on p.217.

**Public API to add:**
```rust
pub struct NetArchitecture {
    pub id: NetArchId,
    pub display_name: String,
    pub floors: Vec<Floor>,                 // index 0 = top, last = bottom (deepest)
    pub access_points: Vec<MeatPosition>,   // physical locations to jack in from
}

pub enum Floor {
    Password { dv: DV },
    ControlNode { dv: DV, controls: ControlTarget, currently_held_by: Option<NetEntityId> },
    File { dv: DV, contents: FileContents },
    BlackIce { template: BlackIceId, state: BlackIceState },
    Demon { template: DemonId, control_nodes: Vec<usize> }, // floor indices it owns
}

pub enum BlackIceState { LyingInWait, InCombat, Slid, Derezzed }

pub enum ControlTarget { Camera(String), Drone(String), Turret(String), Door(String), Sprinkler(String), Elevator(String), LaserGrid(String), Custom(String) }

pub enum FileContents { Data(String), Decoy, Encrypted { unlock_dv: DV } }

pub struct MeatPosition { pub location: LocationId, pub grid_square: Option<(u16, u16)> }

pub struct NetArchSpec {
    pub price: PriceTier,
    pub intent: SecurityIntent,
    pub fixer_rank_required: u8,
}

pub enum SecurityIntent { Defensive, Offensive, Balanced, Killer }

pub fn generate_net_architecture(spec: &NetArchSpec, rng: &mut Rng) -> NetArchitecture;
```

**Acceptance criteria:**
- `test_floor_count_within_tier_range`: V.Expensive tier produces 3..=6 floors, Luxury 7..=12, Super-Luxury 13..=18 (p.217).
- `test_killer_intent_more_ice_and_demons`: Killer intent has higher density of BlackIce/Demon floors.
- `test_defensive_more_passwords`: Defensive intent has more Password floors.
- `test_lowest_floor_is_target`: the deepest floor is always a File or Control Node, never a Password (you wouldn't put the goal behind a password with nothing after it).
- `test_deterministic`: same seed → same architecture.

**Notes:** The DV choices come from p.217's table (DV6 / DV8 / DV10 / DV12 with cost tiers). Generator should respect these.

---

#### WP-402 — Netrunner state
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/state.rs`
**Depends on:** WP-208, WP-401
**Blocks:** WP-404–417
**Estimate:** Medium

**Description:** The active state of a netrun: which architecture, which floor, what's revealed, what programs are rezzed, what control nodes the netrunner holds, what viruses they've left.

**Public API to add:**
```rust
pub struct NetrunState {
    pub netrunner: EntityId,
    pub architecture: NetArchId,
    pub current_floor: usize,                // start at 0 (top)
    pub revealed_floors: usize,              // pathfinder progress
    pub rezzed_programs: Vec<RezzedProgram>,
    pub controlled_nodes: Vec<usize>,        // floor indices
    pub queued_viruses: Vec<Virus>,          // applied on jack-out from bottom floor
    pub cloak_dv: Option<DV>,                // active Cloak's DV if running
    pub net_actions_used_this_turn: u8,
    pub net_actions_max_this_turn: u8,
}

pub struct RezzedProgram {
    pub instance_id: ProgramInstanceId,
    pub program: ProgramId,
    pub current_rez: u8,
}

pub struct Virus { pub description: String, pub effect: VirusEffect, pub dv_to_install: DV, pub net_actions_to_install: u8 }
pub enum VirusEffect { /* per p.200 — alter icons, deactivate ice, malfunction nodes */ }
```

---

#### WP-403 — NET Action accounting (Interface rank → action count)
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/actions.rs`
**Depends on:** WP-402
**Estimate:** Small

**Rulebook:** p.144 (Interface Rank), p.198.

**Description:** Compute how many NET Actions a netrunner gets per Turn from their Interface rank (1–3 → 2 actions, 4–6 → 3, 7–9 → 4, 10 → 5 — verify on p.144).

**Public API to add:**
```rust
pub fn net_actions_per_turn(interface_rank: u8) -> u8;
```

---

#### WP-404 — Interface ability: Scanner
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/scanner.rs`
**Depends on:** WP-101, WP-401
**Estimate:** Small

**Rulebook:** p.199.

**Description:** **Meat** action (not NET). Reveal nearby NET Architecture access points. Higher roll = more revealed.

**Public API to add:**
```rust
pub struct ScannerAction { pub netrunner: EntityId, pub luck_to_spend: u8 }
pub struct ScannerOutcome { pub breakdown: CheckBreakdown, pub access_points_found: Vec<NetArchId> }
```

---

#### WP-405 — Interface ability: Backdoor
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/backdoor.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.199.

**Description:** NET Action. Roll Interface + d10 vs Password DV. Success → traverse the password floor.

---

#### WP-406 — Interface ability: Cloak
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/cloak.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.199.

---

#### WP-407 — Interface ability: Control
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/control.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Medium

**Rulebook:** p.199.

**Description:** Take a Control Node (vs the node's DV; or vs another netrunner/Demon's previous Control roll). Once held, operate the controlled object as additional NET Actions, once per Turn per node. Lose all controlled nodes on Jack Out.

---

#### WP-408 — Interface ability: Eye-Dee
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/eye_dee.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.199.

---

#### WP-409 — Interface ability: Pathfinder
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/pathfinder.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.199.

**Description:** Reveal floors ahead. Reveal count = check result floors, capped at first password whose DV exceeds the check.

---

#### WP-410 — Interface ability: Slide
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/slide.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.200.

**Description:** Flee from a single Black ICE (not Demon) by rolling Slide vs ICE's PER + d10. Once per Turn. Cannot slide preemptively, cannot slide past a password.

---

#### WP-411 — Interface ability: Zap
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/abilities/zap.rs`
**Depends on:** WP-101, WP-402
**Estimate:** Small

**Rulebook:** p.200.

**Description:** Built-in attack. Damage to a target program's REZ or a netrunner's brain.

---

#### WP-412 — Program activation (Boosters & Defenders)
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/programs/active.rs`
**Depends on:** WP-208, WP-402
**Estimate:** Small

**Description:** Activate a Booster or Defender program — places it in `rezzed_programs`, applies its effect via `EffectStack` (with `EffectSource::Program`).

**Acceptance criteria:**
- `test_speedy_gonzalvez_increases_speed`: rezzing Speedy Gonzalvez adds an effect granting +2 to SPEED-related checks.
- `test_armor_reduces_brain_damage`: the Armor program reduces brain damage by 4 while rezzed.
- `test_one_per_round_constraint`: a program can only be activated once per Round (p.201).

---

#### WP-413 — Program activation (Attackers)
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/programs/attackers.rs`
**Depends on:** WP-208, WP-402
**Estimate:** Medium

**Description:** Attacker programs (Banhammer, Sword, Vrizzbolt, etc.) — make an attack vs target program's DEF + d10 or netrunner's Interface + d10. On hit, apply effect. Attacker derezzes itself after use (p.201).

---

#### WP-414 — Black ICE encounter & combat
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/black_ice.rs`
**Depends on:** WP-209, WP-301, WP-402
**Estimate:** Large

**Rulebook:** pp.205–207.

**Description:** Encountering Black ICE — opposed Interface (+ SPEED bonuses) vs ICE SPEED + d10. On loss, immediate effect. ICE inserts at top of initiative queue. Each Turn the ICE attacks (ATK + d10 vs Interface + d10 or program DEF + d10). Pursues across architecture until Derezzed or Slid.

**Public API to add:**
```rust
pub struct BlackIceEncounter {
    pub netrunner: EntityId,
    pub ice_template: BlackIceId,
    pub floor: usize,
}

pub struct BlackIceEncounterOutcome {
    pub speed_contest: OpposedOutcome,
    pub immediate_effect_applied: Option<BlackIceEffectApplied>,
    pub combat_started: bool,
}

pub struct BlackIceTurn {
    pub netrunner_target: bool,           // Anti-Personnel always; Anti-Program targets random rezzed program
    pub attack_breakdown: CheckBreakdown,
    pub effect_applied: Option<BlackIceEffectApplied>,
}
```

---

#### WP-415 — Demon behavior
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/demon.rs`
**Depends on:** WP-210, WP-402
**Estimate:** Medium

**Rulebook:** p.212.

**Description:** Demons hold control nodes, defend with Zap, can take multiple NET Actions per Turn. No SPD/PER → can't be Slid. Always aware of all netrunners present. Operate Control Nodes once each per Turn.

---

#### WP-416 — Virus deployment (jack-out persistence)
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/virus.rs`
**Depends on:** WP-402
**Estimate:** Small

**Rulebook:** p.200.

**Description:** At the bottom floor, a netrunner may deploy a Virus (DV-graded by power, taking N NET Actions to install). Effect persists after Jack Out. Tracks queued viruses; applies on jack-out.

---

#### WP-417 — Netrun integration with combat queue
**Crate:** `rules`
**Module:** `crates/rules/src/netrunning/integration.rs`
**Depends on:** WP-301, WP-402, WP-414
**Estimate:** Medium

**Description:** Wire the netrun into the same `CombatState::queue`. The netrunner takes Meat Actions or NET Actions on their Turn; Black ICE entities go into the queue; Demons too. Jack Out as a Meat Action. Unsafe Jack Out (leaving access point range while jacked in) applies all remaining un-derezzed enemy ICE effects (p.198).

**Acceptance criteria:**
- `test_netrunner_meat_or_net_action`: on the netrunner's turn, they can choose either path.
- `test_black_ice_in_queue_acts_each_round`: a rezzed ICE attacks every round.
- `test_unsafe_jackout_applies_all_pending_ice`: leaving range with un-derezzed ICE applies all their effects.
- `test_loss_of_control_nodes_on_jackout`: jacking out releases all controlled nodes.


---

### Phase 5 — Character & Progression

**Parallelism:** ~19 agents.

#### WP-501 — Streetrat character creation
**Crate:** `rules`
**Module:** `crates/rules/src/character/creation/streetrat.rs`
**Depends on:** WP-004, WP-105, WP-201, WP-202, WP-203, WP-204, WP-213, WP-214
**Blocks:** WP-1001
**Estimate:** Medium

**Rulebook:** pp.73–78.

**Description:** Streetrat Character Generation — roll on the Role-specific STAT template tables; assigned starting Skills, gear, and cyberware per role.

**Public API to add:**
```rust
pub fn create_streetrat(role: Role, name: String, rng: &mut Rng) -> Result<Character, RulesError>;

pub fn streetrat_stats(role: Role, rng: &mut Rng) -> StatBlock;     // rolls 1d10 on the template table
```

**Acceptance criteria:**
- `test_streetrat_solo_stats_match_table`: forced d10=6 on Solo template → stats match the row in the rulebook.
- `test_streetrat_starting_hp_correct`: HP from formula matches stats.
- `test_streetrat_starting_humanity_eq_10x_emp`.
- `test_streetrat_role_skills_set`: Solo gets the Solo's starting skills with appropriate ranks.

---

#### WP-502 — Edgerunner character creation
**Crate:** `rules`
**Module:** `crates/rules/src/character/creation/edgerunner.rs`
**Depends on:** WP-501
**Estimate:** Medium

**Rulebook:** pp.78–79+ (Edgerunner Option).

**Description:** Edgerunner — players spend points; mid-tier customisation between Streetrat and Complete Package.

---

#### WP-503 — Complete Package character creation
**Crate:** `rules`
**Module:** `crates/rules/src/character/creation/complete.rs`
**Depends on:** WP-501
**Estimate:** Large

**Rulebook:** Complete Package rules (pp.78+).

**Description:** Full point-buy creation — every STAT, Skill, gear choice is the player's. Includes validation against caps.

---

#### WP-504 — Lifepath roller
**Crate:** `rules`
**Module:** `crates/rules/src/character/lifepath.rs`
**Depends on:** WP-214
**Estimate:** Medium

**Description:** Roll lifepath tables for a character. Produces `Lifepath` struct with all backstory beacons.

**Acceptance criteria:**
- `test_lifepath_complete`: every required field is populated.
- `test_lifepath_role_specific_matches_role`: a Solo rolls the Solo lifepath table, not the Rockerboy's.
- `test_friends_enemies_have_names`: the LLM expects names; ensure beacons have generated names (pull from a names table or use the rulebook's flavour names).

---

#### WP-505 — Cyberware install + Humanity Loss
**Crate:** `rules`
**Module:** `crates/rules/src/character/cyberware.rs`
**Depends on:** WP-003, WP-004, WP-204
**Blocks:** WP-506
**Estimate:** Medium

**Rulebook:** p.227.

**Description:** Install a piece of cyberware on a character. Validate prerequisites. Validate option slots. Roll Humanity Loss (rolled spec for in-play installs, fixed for at-creation). Apply the cyberware's effects to `EffectStack` as `EffectSource::Cyberware`. Trigger EMP recalculation (which may drop EMP and reduce all skill bases derived from EMP).

**Public API to add:**
```rust
pub fn install_cyberware(
    character: &mut Character,
    item: CyberwareId,
    catalog: &Catalog<Cyberware>,
    rng: &mut Rng,
    at_creation: bool,
) -> Result<InstallOutcome, RulesError>;

pub struct InstallOutcome {
    pub humanity_loss: u8,
    pub humanity_after: i16,
    pub emp_change: Option<(u8, u8)>,    // (before, after)
    pub effects_added: Vec<EffectInstanceId>,
    pub triggered_cyberpsychosis: bool,
}
```

---

#### WP-506 — Cyberpsychosis state
**Crate:** `rules`
**Module:** `crates/rules/src/character/cyberpsychosis.rs`
**Depends on:** WP-003, WP-505
**Estimate:** Small

**Rulebook:** pp.108–109, p.230.

**Description:** When Humanity drops below 0, the character enters Cyberpsychosis. Add an `EffectSource::Cyberpsychosis` effect with appropriate flavour modifiers. Per design decision, this is **recoverable via therapy** — see WP-507. Block normal play actions until a therapy track is started? — actually, per design the LLM frames it as a story event. Engine just sets the flag.

**Public API to add:**
```rust
impl Character {
    pub fn is_cyberpsychotic(&self) -> bool;
    pub fn enter_cyberpsychosis(&mut self);
    pub fn exit_cyberpsychosis(&mut self);
}
```

---

#### WP-507 — Therapy mechanic
**Crate:** `rules`
**Module:** `crates/rules/src/character/therapy.rs`
**Depends on:** WP-506
**Estimate:** Medium

**Rulebook:** p.229.

**Description:** Therapy lets a character regain Humanity. Each session: a fixed amount of Humanity gained, costs eb, takes in-game time. Removes Cyberpsychosis when humanity climbs back ≥ 0.

**Public API to add:**
```rust
pub struct TherapySession { pub kind: TherapyKind, pub cost: Eurobucks, pub humanity_gain: u8, pub time_required: TimeUnit }
pub enum TherapyKind { Outpatient, Inpatient, Intensive }
pub fn run_therapy(character: &mut Character, session: TherapySession) -> Result<TherapyOutcome, RulesError>;
```

---

#### WP-508 — Improvement Point spending
**Crate:** `rules`
**Module:** `crates/rules/src/character/progression/spend.rs`
**Depends on:** WP-004, WP-201, WP-213
**Blocks:** Frontend progression UI
**Estimate:** Medium

**Rulebook:** pp.408–411.

**Description:** Spend IP to raise Skills (cost = next-level squared, doubled for x2 skills) or Role Ability rank. Validate bounds (skills cap at 10, etc.).

**Public API to add:**
```rust
pub fn ip_cost_for_next_skill_rank(current_rank: u8, double_cost: bool) -> u32;
pub fn ip_cost_for_next_role_rank(current_rank: u8) -> u32;
pub fn spend_ip_on_skill(character: &mut Character, skill: SkillId) -> Result<u32, RulesError>;
pub fn spend_ip_on_role_ability(character: &mut Character) -> Result<u32, RulesError>;
```

**Acceptance criteria:** verify the cost table on p.411.

---

#### WP-509 — Improvement Point earning (objective milestones)
**Crate:** `rules`
**Module:** `crates/rules/src/character/progression/earn.rs`
**Depends on:** WP-004
**Estimate:** Small

**Description:** Per the hybrid IP design — fixed IP for objective triggers (gig completed: +30, enemy defeated in unusual way: +50, etc.). The `gm` crate calls these at appropriate Beat transitions. The LLM-bonus side lives in WP-707.

**Public API to add:**
```rust
pub enum IpMilestone {
    GigStarted, GigCompletedSuccessfully, GigCompletedFailure,
    EnemyDefeatedNotably, NetrunCleared, BeatResolved, FirstTimeVisit,
}

pub fn milestone_ip(milestone: IpMilestone) -> u32;
```

---

#### WP-510 — Role Ability: Combat Sense (Solo)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/combat_sense.rs`
**Depends on:** WP-003, WP-004, WP-213
**Estimate:** Small

**Rulebook:** p.142.

**Description:** Combat Sense rank applies as a bonus to Initiative AND as a bonus to Awareness/Perception checks (verify). Implement as an effect emitted at combat start + a permanent perception bonus.

---

#### WP-511 — Role Ability: Interface (Netrunner)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/interface.rs`
**Depends on:** WP-403
**Estimate:** Small

**Rulebook:** pp.144, 199.

**Description:** Interface rank determines NET Actions per Turn (table on p.144) and is the d10-added value for every Interface ability. WP-403 already implements net_actions_per_turn; this WP wires it to the character's role rank and exposes a single `interface_rank(character)` accessor.

---

#### WP-512 — Role Ability: Maker (Tech)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/maker.rs`
**Estimate:** Medium

**Rulebook:** p.144+.

**Description:** Tech can build/modify gadgets. Implement crafting checks, available recipes, time cost. Reduce to mechanical hooks: "given a recipe and a check, success modifies/creates an item".

---

#### WP-513 — Role Ability: Medicine (Medtech)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/medicine.rs`
**Depends on:** WP-303, WP-305
**Estimate:** Medium

**Rulebook:** p.142+.

**Description:** Medtech heals HP and treats Critical Injuries above their normal Paramedic/Surgery DV. Specifically: +1 to surgery checks per rank, ability to perform Pharmaceuticals, Cryosystem Operation, etc. (per book).

---

#### WP-514 — Role Ability: Credibility (Media)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/credibility.rs`
**Estimate:** Small

**Rulebook:** p.142+.

**Description:** Media's Credibility allows them to convince others their reporting is true; ranks unlock effects on different scales (single person → small group → mob). Mostly LLM-narrated; implement check helpers and constraints.

---

#### WP-515 — Role Ability: Backup (Lawman)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/backup.rs`
**Estimate:** Small

**Rulebook:** p.142+.

**Description:** Lawman can call backup of a given size. Adapt for solo: backup arrives as NPC allies via the Fixer-style mechanic (WP-612), with size determined by rank.

---

#### WP-516 — Role Ability: Resources (Exec)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/resources.rs`
**Estimate:** Small

**Rulebook:** p.142+.

**Description:** Exec's Resources let them call on corporate backing for money / personnel / equipment. Implement as a per-rank pool of "pull" usable per gig.

---

#### WP-517 — Role Ability: Operator (Fixer)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/operator.rs`
**Depends on:** WP-201, WP-212
**Estimate:** Medium

**Rulebook:** pp.142+, also p.371 (Trading).

**Description:** Operator rank gives Fixer-only abilities: better trading prices, access to higher-rank Night Markets, contact lists. Implement as price modifiers + access gating.

---

#### WP-518 — Role Ability: Charismatic Impact (Rockerboy)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/charismatic_impact.rs`
**Estimate:** Medium

**Rulebook:** pp.218–.

**Description:** Influence fans by COOL + Charismatic Impact + d10 vs DV (8/10/12 by group size). Effect tier varies with Rank. Implement check helper; LLM-narrate the effect.

---

#### WP-519 — Role Ability: Moto (Nomad)
**Crate:** `rules`
**Module:** `crates/rules/src/roles/moto.rs`
**Depends on:** WP-211
**Estimate:** Small

**Rulebook:** p.144.

**Description:** Nomad's Moto gives access to Family vehicles + bonuses to Drive/Pilot. Implement vehicle access + check bonuses.


---

### Phase 6 — GM Layer

**Parallelism:** ~13 agents. Builds on `rules` crate. New crate `gm`.

#### WP-601 — gm crate scaffolding
**Crate:** `gm`
**Module:** `crates/gm/src/lib.rs`
**Depends on:** WP-006
**Blocks:** WP-602–613
**Estimate:** Small

**Description:** Set up the `gm` crate with module structure, error type (`GmError`), re-exports from `rules`. No business logic yet — this is just the scaffolding so other Phase 6 WPs can land in parallel.

**Public API to add:**
```rust
pub use cpr_rules::{Character, World, Rng, EntityId};

#[derive(thiserror::Error, Debug)]
pub enum GmError {
    #[error("rules error: {0}")] Rules(#[from] cpr_rules::RulesError),
    #[error("beat chart error: {0}")] BeatChart(String),
    #[error("npc not found: {0}")] NpcNotFound(NpcId),
    #[error("invalid transition: {0}")] InvalidTransition(String),
    /* ... */
}

pub mod beats;
pub mod npc;
pub mod log;
pub mod encounter;
pub mod faction;
pub mod hooks;
pub mod ip;
```

**Acceptance criteria:** `cargo build -p gm` succeeds with empty modules.

---

#### WP-602 — Beat Chart schema
**Crate:** `gm`
**Module:** `crates/gm/src/beats/schema.rs`
**Depends on:** WP-601
**Blocks:** WP-603, WP-604
**Estimate:** Medium

**Rulebook:** pp.395–408 (Beat Charts).

**Description:** Define the Beat Chart RON schema as Rust types. Beats are typed (Hook, Development, Cliffhanger, Climax, Resolution). Each Beat has a location, present NPCs, narrative intent, mechanical hooks, and possible transitions.

**Public API to add:**
```rust
pub struct Gig {
    pub id: GigId,
    pub title: String,
    pub fixer: NpcId,
    pub payment: PaymentTier,
    pub setting: String,
    pub scope_hours: u8,
    pub npcs: HashMap<NpcId, NpcRef>,
    pub locations: HashMap<LocationId, LocationRef>,
    pub beats: Vec<Beat>,
    pub start_beat: BeatId,
}

pub struct Beat {
    pub id: BeatId,
    pub kind: BeatKind,
    pub location: LocationId,
    pub present: Vec<NpcId>,
    pub intent: String,                          // narrative intent for the LLM
    pub mechanical_hooks: Vec<MechanicalHook>,
    pub encounter: Option<EncounterRef>,
    pub transitions: Vec<Transition>,
}

pub enum BeatKind { Hook, Development, Cliffhanger, Climax, Resolution }

pub struct GigId(pub String);
pub struct BeatId(pub String);

pub struct NpcRef { pub template: String }
pub struct LocationRef { pub map: Option<String>, pub description: String }
pub struct EncounterRef(pub String);

pub enum PaymentTier { Cheap(Eurobucks), Costly(Eurobucks), Premium(Eurobucks), Custom(Eurobucks) }

pub struct Transition {
    pub condition: TransitionCondition,
    pub target: BeatId,
}

pub enum TransitionCondition {
    Always,
    EncounterResolvedSilently,
    EncounterResolvedLoud,
    AlarmRaised,
    HookSucceeded(MechanicalHookId),
    HookFailed(MechanicalHookId),
    PromiseBroken(String),
    PromiseKept(String),
    NpcKilled(NpcId),
    NpcAlly(NpcId),
    PlayerChoice(String),                        // surfaces a UI choice
}

pub struct MechanicalHook {
    pub id: MechanicalHookId,
    pub kind: MechanicalHookKind,
}

pub struct MechanicalHookId(pub String);

pub enum MechanicalHookKind {
    SkillCheck { stat: Stat, skill: SkillId, dv: DV, on_success: HookEffect, on_failure: HookEffect },
    OpposedCheck { /* ... */ },
    Negotiation { /* ... */ },
    Ambush { /* ... */ },
    Search { dv: DV, finds: Vec<DiscoveryRef> },
}

pub enum HookEffect {
    BonusPay(Eurobucks),
    PenaltyPay(Eurobucks),
    RevealNpc(NpcId),
    RevealLocation(LocationId),
    GrantItem(ItemKind),
    AddIntel(String),
    Transition(BeatId),
    Combine(Vec<HookEffect>),
}

pub struct DiscoveryRef(pub String);
```

**Acceptance criteria:**
- `test_beat_chart_round_trips`: a sample Gig serializes to RON and deserializes identically.
- `test_beat_kinds_complete`: all 5 BeatKind variants present.
- `test_transition_conditions_serializable`: all variants RON round-trip.

---

#### WP-603 — Beat Chart loader and validator
**Crate:** `gm`
**Module:** `crates/gm/src/beats/loader.rs`, `tools/content-validator/src/beats.rs`
**Depends on:** WP-602
**Blocks:** WP-604, WP-1001
**Estimate:** Medium

**Description:** Load Gigs from RON files in `content/gigs/`. Validate structural correctness: every transition target exists; every NPC ref resolves against `content/npcs/`; every location ref resolves; first beat is `Hook`; at least one Climax → Resolution path exists; no orphan beats; mechanical hook IDs are unique within a Gig.

**Public API to add:**
```rust
pub fn load_gig(path: &Path) -> Result<Gig, GmError>;
pub fn load_all_gigs(dir: &Path) -> Result<HashMap<GigId, Gig>, GmError>;
pub fn validate_gig(gig: &Gig, npc_catalog: &Catalog<NpcTemplate>, locations: &Catalog<LocationDef>) -> Result<(), GmError>;
```

**Acceptance criteria:**
- `test_loads_sample_gig`: a fixture gig loads.
- `test_invalid_transition_target_fails`: a transition pointing to a missing beat ID returns `Err`.
- `test_orphan_beat_fails`: a beat unreachable from `start_beat` returns `Err`.
- `test_no_climax_or_resolution_fails`: a chart with no path to `Climax → Resolution` returns `Err`.
- `test_validator_cli_reports_all_errors`: the validator CLI surfaces every error in a single pass, not just the first.

**Notes:** Validator should produce structured diagnostics (file, beat id, error kind) so the user can fix in batch. CI runs `cargo run -p content-validator -- content/`.

---

#### WP-604 — Beat state machine
**Crate:** `gm`
**Module:** `crates/gm/src/beats/state.rs`
**Depends on:** WP-602, WP-603, WP-006
**Blocks:** WP-1001, WP-1002
**Estimate:** Large

**Description:** The active running state of a Gig: which beat we're on, which transitions are available, which mechanical hooks have fired, NPC dispositions in this gig. Drives the Beat → Beat progression. Surfaces "what can the player do here" to the UI.

**Public API to add:**
```rust
pub struct GigState {
    pub gig: GigId,
    pub current_beat: BeatId,
    pub history: Vec<BeatTraversal>,
    pub hook_outcomes: HashMap<MechanicalHookId, HookOutcome>,
    pub flags: HashMap<String, FlagValue>,         // arbitrary KV the GM uses for state
    pub temp_npcs: HashMap<NpcId, EntityId>,       // NPCs instantiated for this gig
}

pub struct BeatTraversal {
    pub beat: BeatId,
    pub entered_at: GameClockSnapshot,
    pub exited_at: Option<GameClockSnapshot>,
    pub via: Option<TransitionCondition>,
}

pub enum HookOutcome { Succeeded { breakdown: CheckBreakdown }, Failed { breakdown: CheckBreakdown }, Skipped }

pub enum FlagValue { Bool(bool), Int(i64), Str(String) }

impl GigState {
    pub fn start(gig: &Gig, world: &mut World) -> Self;
    pub fn current_beat<'g>(&self, gig: &'g Gig) -> &'g Beat;
    /// Returns the transitions whose conditions can currently be evaluated/triggered.
    pub fn available_transitions<'g>(&self, gig: &'g Gig) -> Vec<&'g Transition>;
    /// Apply a transition. Calls `enter_beat` on the target beat.
    pub fn transition(&mut self, condition: TransitionCondition, gig: &Gig, world: &mut World) -> Result<(), GmError>;
    /// Record a hook outcome (used by UI/engine when a hook fires).
    pub fn record_hook(&mut self, id: MechanicalHookId, outcome: HookOutcome);
    /// Resolve a flag set by a HookEffect.
    pub fn set_flag(&mut self, key: String, value: FlagValue);
}
```

**Acceptance criteria:**
- `test_start_at_hook`: starting a Gig places us at the `start_beat`.
- `test_transition_always`: a transition with `Always` is taken once its source beat is exited.
- `test_transition_hook_succeeded`: a transition gated on `HookSucceeded(id)` is available iff that hook recorded a Success.
- `test_orphan_transition_unavailable`: a transition whose condition is unmet is not in `available_transitions`.
- `test_history_records_traversal`: every transition appends to `history`.

**Notes:** The Beat state machine is **deterministic given inputs** — same hook outcomes + same player choices → same beat path. The LLM cannot move the state; it can only narrate.

---

#### WP-605 — NPC entity model
**Crate:** `gm`
**Module:** `crates/gm/src/npc/entity.rs`
**Depends on:** WP-006
**Blocks:** WP-606, WP-607, WP-612
**Estimate:** Medium

**Description:** NPC types — both narrative NPCs (Padre the Fixer) and adversary NPCs (Maelstrom Grunt). Adversaries are full `Character` instances. Narrative NPCs have lighter records. A single `Npc` enum unifies them.

**Public API to add:**
```rust
pub struct NpcTemplate {
    pub id: NpcId,                                // e.g. "fixer_padre", "maelstrom_grunt"
    pub display_name: String,
    pub kind: NpcTemplateKind,
    pub description: String,                       // for the LLM
    pub initial_disposition: i8,                  // -10..=+10
    pub voice_notes: String,                       // hints for LLM dialogue
}

pub enum NpcTemplateKind {
    Narrative {
        role_sketch: Option<String>,               // "Padre the Fixer", "Joss the doc"
        beacons: Vec<Beacon>,
    },
    Mook {
        archetype: MookArchetype,                  // pp.418–419 (Goons, Solos, etc.)
        loadout: Loadout,
    },
    Lieutenant {
        character: Character,                      // full stat block
    },
    Boss {
        character: Character,
    },
}

pub enum MookArchetype { Goon, Edgerunner, MaelstromGanger, /* ... see book pp.418–419 */ }

pub struct Loadout { pub weapons: Vec<WeaponId>, pub armor: Option<ArmorId>, pub cyberware: Vec<CyberwareId> }

pub struct Beacon { pub label: String, pub note: String }

pub struct ActiveNpc {
    pub template: NpcId,
    pub entity_id: EntityId,
    pub character: Character,                       // freshly instantiated
    pub gig_disposition: i8,                        // can drift per-gig
}
```

**Acceptance criteria:**
- `test_npc_template_round_trips`: RON.
- `test_mook_instantiates_with_loadout`: instantiating a Maelstrom Goon produces a Character with the loadout's weapons/armor.

---

#### WP-606 — NPC instantiation from template
**Crate:** `gm`
**Module:** `crates/gm/src/npc/instantiate.rs`
**Depends on:** WP-605
**Blocks:** WP-610, WP-612
**Estimate:** Medium

**Description:** Given an `NpcTemplate`, build an `ActiveNpc` for use in a scene. Mooks get statlines from the archetype tables (pp.418–419). Lieutenants/Bosses already have characters. Narrative NPCs get a thin `Character` for HP/stat references but mostly exist as data for the LLM.

**Public API to add:**
```rust
pub fn instantiate_npc(template: &NpcTemplate, catalog: &CatalogBundle, rng: &mut Rng) -> Result<ActiveNpc, GmError>;

pub struct CatalogBundle<'a> {
    pub weapons: &'a Catalog<Weapon>,
    pub armor: &'a Catalog<Armor>,
    pub cyberware: &'a Catalog<Cyberware>,
    pub mooks: &'a Catalog<MookStatline>,
}

pub struct MookStatline {
    pub archetype: MookArchetype,
    pub stats: StatBlock,
    pub skills: SkillSet,
    pub hp: u16,
    pub default_loadout: Loadout,
}
```

**Acceptance criteria:**
- `test_goon_hp_matches_book`: archetype Goon hp matches p.418.
- `test_lieutenant_uses_template_character`: instantiation copies the template's stats verbatim.

---

#### WP-607 — Structured campaign log
**Crate:** `gm`
**Module:** `crates/gm/src/log/types.rs`
**Depends on:** WP-605
**Blocks:** WP-608, WP-611
**Estimate:** Medium

**Description:** Define the campaign log as structured events, NPC memory, faction standing, completed gigs, major events. Persisted by the `persistence` crate.

**Public API to add:**
```rust
pub struct CampaignLog {
    pub character_id: CharacterId,
    pub events: Vec<LogEvent>,
    pub npc_memory: HashMap<NpcId, NpcRelationship>,
    pub faction_standing: HashMap<FactionId, i8>,    // -10..=+10
    pub completed_gigs: Vec<CompletedGig>,
    pub major_events: Vec<MajorEvent>,
}

pub struct LogEvent {
    pub at: GameClockSnapshot,
    pub kind: LogEventKind,
}

pub enum LogEventKind {
    GigStarted { gig: GigId, fixer: NpcId },
    GigCompleted { gig: GigId, outcome: GigOutcome, payment: Eurobucks, ip_awarded: u32 },
    BeatEntered { beat: BeatId },
    NpcMet { npc: NpcId, beat: BeatId, impression: NpcImpression },
    NpcKilled { npc: NpcId, by_player: bool, witnesses: Vec<NpcId> },
    PromiseMade { to: NpcId, promise: String, due: Option<GameClockSnapshot> },
    PromiseBroken { to: NpcId, promise: String },
    PromiseKept { to: NpcId, promise: String },
    HumanityLossEvent { delta: i16, source: HumanitySource },
    CyberwareInstalled { id: CyberwareId, ripperdoc: NpcId, hl_paid: u8 },
    CombatResolved { participants: Vec<EntityId>, summary: String, side_won: Side },
    NetrunCompleted { architecture: NetArchId, files_extracted: Vec<String>, viruses_left: u8 },
    Shopped { vendor: NpcId, items: Vec<(ItemKind, Eurobucks)> },
    LocationVisited { location: LocationId, first_time: bool },
    Custom { tag: String, payload: String },
}

pub enum GigOutcome { Success, PartialSuccess, Failure }
pub enum NpcImpression { Friendly, Neutral, Wary, Hostile }
pub enum HumanitySource { CyberwareInstall(CyberwareId), TraumaticEvent(String), TherapyGain }
pub enum Side { Player, Adversary, Neutral }

pub struct NpcRelationship {
    pub disposition: i8,                              // -10..=+10
    pub events: Vec<usize>,                           // indices into CampaignLog::events
    pub knows_about: Vec<KnowledgeFlag>,
}

pub enum KnowledgeFlag {
    KnowsPlayerKilled(NpcId),
    KnowsPlayerLies(String),
    OwedFavor,
    OwesFavor,
    InDebt(Eurobucks),
}

pub struct CompletedGig { pub gig: GigId, pub completed_at: GameClockSnapshot, pub outcome: GigOutcome }

pub struct MajorEvent { pub at: GameClockSnapshot, pub headline: String, pub kind: MajorEventKind }
pub enum MajorEventKind { CharacterDied, CharacterRevived, BetrayedByFixer, GainedFamily, LostFamily }
```

**Acceptance criteria:**
- `test_log_round_trips_ron`.
- `test_npc_memory_event_indices_valid`: every NpcRelationship.events index is in range.
- `test_faction_standing_clamped`: setting +20 clamps to +10.

---

#### WP-608 — Campaign log digest generator
**Crate:** `gm`
**Module:** `crates/gm/src/log/digest.rs`
**Depends on:** WP-607
**Blocks:** WP-705 (LLM prompts consume digests)
**Estimate:** Medium

**Description:** Generate a compact text digest for the LLM prompt: recent events, currently-relevant NPCs, current factions, current open promises. Bounded length. Configurable per use case.

**Public API to add:**
```rust
pub struct DigestRequest {
    pub recent_event_limit: usize,
    pub include_npcs: Vec<NpcId>,
    pub include_factions: bool,
    pub include_open_promises: bool,
    pub max_chars: usize,
}

pub fn generate_digest(log: &CampaignLog, req: &DigestRequest) -> String;
```

**Acceptance criteria:**
- `test_digest_respects_max_chars`.
- `test_digest_includes_npc_memory`: a digest with `include_npcs: [padre]` includes Padre's most relevant events.
- `test_digest_open_promises`: unfulfilled promises appear.

**Notes:** Output format is **fact-bullets** — "On Day 12, you killed Garcia (a Maelstrom Lieutenant). Padre witnessed this. Padre's disposition: -3." Not narrative. The LLM converts to narrative.

---

#### WP-609 — IP awarding (LLM-bonus side)
**Crate:** `gm`
**Module:** `crates/gm/src/ip/llm_bonus.rs`
**Depends on:** WP-509, WP-707
**Estimate:** Small

**Description:** The LLM scores narrative quality of a gig (creativity, RP, problem-solving) and awards capped bonus IP. Per design: hard cap (e.g. +30 IP per gig) so LLM cannot inflate. WP-707 builds the prompt; this WP defines the cap, validation, and integration with the gig-end event.

**Public API to add:**
```rust
pub struct IpBonusRequest { pub gig: GigId, pub log_excerpt: String, pub cap: u32 }
pub struct IpBonusResponse { pub awarded: u32, pub reason: String }

pub fn award_llm_bonus_ip(character: &mut Character, response: &IpBonusResponse) -> Result<u32, GmError>;
```

**Acceptance criteria:**
- `test_cap_enforced`: a response with `awarded: 50` and cap 30 → 30 IP added.
- `test_negative_rejected`: negative awarded values rejected.

---

#### WP-610 — Encounter loader
**Crate:** `gm`
**Module:** `crates/gm/src/encounter/loader.rs`
**Depends on:** WP-301, WP-302, WP-606
**Blocks:** WP-1001
**Estimate:** Medium

**Description:** Load combat encounter definitions from `content/encounters/`. An encounter has: a grid map reference, enemy NPC instantiations with starting positions, optional environmental effects (low light, smoke, difficult terrain), an optional stealth approach DV.

**Public API to add:**
```rust
pub struct Encounter {
    pub id: EncounterId,
    pub display_name: String,
    pub grid: GridDef,
    pub enemies: Vec<EnemyPlacement>,
    pub environment: Vec<EnvironmentalKind>,
    pub stealth_approach_dv: Option<DV>,
    pub allies_can_join: bool,
}

pub struct EnemyPlacement {
    pub template: NpcId,
    pub position: (u16, u16),
    pub initial_state: EnemyInitialState,
}

pub enum EnemyInitialState { Patrolling, OnGuard, Asleep, Surprised, Engaged }

pub struct GridDef { pub width: u16, pub height: u16, pub tiles: String, pub cover: Vec<(u16, u16, String)> }

pub fn load_encounter(path: &Path) -> Result<Encounter, GmError>;
pub fn instantiate_encounter(enc: &Encounter, world: &mut World, catalog: &CatalogBundle, rng: &mut Rng) -> Result<CombatState, GmError>;
```

**Acceptance criteria:**
- `test_load_encounter_round_trip`: RON ⇄ struct.
- `test_instantiate_places_enemies`: enemies appear on the grid at their declared positions.
- `test_stealth_approach_dv_threading`: when a player succeeds at a stealth approach, enemies start in `Surprised` (lose first turn — implement as initiative penalty per RAW p.169).

---

#### WP-611 — Faction and reputation tracking
**Crate:** `gm`
**Module:** `crates/gm/src/faction/mod.rs`
**Depends on:** WP-607
**Estimate:** Small

**Description:** Maintain faction standing as a function of LogEvents. Hostile actions against a faction's members lower standing; helping them raises it. Faction standing affects shop access, encounter generation, NPC dispositions.

**Public API to add:**
```rust
pub struct FactionId(pub String);
pub struct FactionDef { pub id: FactionId, pub display_name: String, pub members: Vec<NpcId> }

pub fn update_faction_standing(log: &mut CampaignLog, factions: &HashMap<FactionId, FactionDef>, event: &LogEvent);
```

**Acceptance criteria:**
- `test_killing_member_lowers_faction`: NpcKilled of a member with `by_player=true` drops faction by an amount.
- `test_floors_at_minus_10`: cannot drop below -10.

---

#### WP-612 — NPC ally hiring (Fixer integration)
**Crate:** `gm`
**Module:** `crates/gm/src/npc/hiring.rs`
**Depends on:** WP-606, WP-517
**Blocks:** WP-1001
**Estimate:** Medium

**Description:** Per the solo design decision: the player hires NPC allies through the Fixer mechanic for a gig. Each hireable NPC has a cost, a loadout, and a loyalty profile. They participate in combat as allies. They take their own initiative. They may refuse certain orders (loyalty check). They may die or quit.

**Public API to add:**
```rust
pub struct HireableNpc {
    pub id: NpcId,
    pub display_name: String,
    pub archetype: MookArchetype,
    pub cost: Eurobucks,
    pub loyalty: u8,                                 // 1..=10
    pub specialty: Specialty,
    pub min_fixer_rank: u8,
}

pub enum Specialty { Wheelman, Muscle, Hacker, Tech, Medic, Face }

pub fn list_available_hires(fixer_rank: u8, player_money: Eurobucks, region: &LocationId) -> Vec<HireableNpc>;
pub fn hire(world: &mut World, hire: &HireableNpc, gig: &mut GigState) -> Result<EntityId, GmError>;
pub fn loyalty_check(ally: &ActiveNpc, order: &Order, rng: &mut Rng) -> bool;
```

**Acceptance criteria:**
- `test_hire_deducts_money`: hiring costs are deducted.
- `test_hire_requires_fixer_rank`: a hireable below the player's Fixer rank is unavailable.
- `test_hireable_added_to_party`: post-hire, the ally is in the player's gig party.
- `test_dangerous_order_loyalty_check`: an order to "die for me" rolls loyalty.

**Notes:** Loyalty is an internal stat the LLM consults when deciding ally dialogue/actions. The engine enforces hard cases (e.g. ally won't suicide-charge below loyalty 8); the LLM colours softer cases.

---

#### WP-613 — Mechanical hook resolver
**Crate:** `gm`
**Module:** `crates/gm/src/hooks/resolver.rs`
**Depends on:** WP-101, WP-602, WP-604
**Estimate:** Medium

**Description:** Translate a `MechanicalHookKind` into engine calls. A `SkillCheck` hook → run a `SkillCheck` resolution → record the outcome on `GigState` → apply `HookEffect` (which might be `BonusPay`, `RevealNpc`, `Transition`, etc.).

**Public API to add:**
```rust
pub fn resolve_hook(
    hook: &MechanicalHook,
    gig_state: &mut GigState,
    world: &mut World,
    rng: &mut Rng,
) -> Result<HookResolutionOutcome, GmError>;

pub struct HookResolutionOutcome {
    pub hook_id: MechanicalHookId,
    pub outcome: HookOutcome,
    pub effects_applied: Vec<HookEffect>,
}
```

**Acceptance criteria:**
- `test_skill_check_hook_records_outcome`: a SkillCheck hook produces a `HookOutcome::Succeeded` or `Failed`.
- `test_bonus_pay_increases_payment`: a `BonusPay(100)` increments the gig's payment record.
- `test_reveal_npc_adds_to_log`: `RevealNpc(id)` adds an `NpcMet` event.
- `test_transition_advances_beat`: a `Transition(target)` moves the GigState's current_beat.
- `test_combine_applies_all`: `Combine([a, b, c])` applies all effects in order.


---

### Phase 7 — LLM Layer

**Parallelism:** ~10 agents. New crate `llm`. Most WPs are independent.

#### WP-701 — LlmProvider trait and message types
**Crate:** `llm`
**Module:** `crates/llm/src/provider.rs`, `crates/llm/src/message.rs`
**Depends on:** WP-000
**Blocks:** WP-702–710
**Estimate:** Medium

**Description:** The transport-agnostic provider trait. All providers implement this. Sync API on the call site (returns futures because impls do I/O). The crate compiles to both native and WASM, but provider impls are gated by target.

**Public API to add:**
```rust
#[async_trait::async_trait(?Send)]                  // ?Send because WASM single-threaded
pub trait LlmProvider {
    async fn complete(&self, request: ChatRequest) -> Result<ChatResponse, LlmError>;
    async fn stream(&self, request: ChatRequest) -> Result<BoxStream<'_, Result<ChatChunk, LlmError>>, LlmError>;
}

pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<Message>,
    pub temperature: f32,
    pub max_tokens: u32,
    pub stop: Vec<String>,
    pub response_schema: Option<ResponseSchema>,    // for constrained outputs
}

pub struct Message { pub role: Role, pub content: String }
pub enum Role { User, Assistant }

pub struct ChatResponse { pub content: String, pub finish_reason: FinishReason, pub usage: Usage }
pub struct ChatChunk { pub delta: String, pub finish_reason: Option<FinishReason> }
pub enum FinishReason { Stop, Length, StopSequence, Other(String) }
pub struct Usage { pub input_tokens: u32, pub output_tokens: u32 }

pub enum ResponseSchema {
    /// JSON schema. Provider attempts to constrain output; we always validate.
    Json(serde_json::Value),
    /// Strict regex. Provider may not constrain; we validate.
    Regex(String),
    /// One of N predefined values.
    Enum(Vec<String>),
}

#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("network error: {0}")] Network(String),
    #[error("provider error {status}: {body}")] Provider { status: u16, body: String },
    #[error("invalid response: {0}")] InvalidResponse(String),
    #[error("schema violation: {0}")] SchemaViolation(String),
    #[error("auth error")] Auth,
    #[error("rate limited, retry after {0}s")] RateLimited(u64),
}
```

**Acceptance criteria:**
- `test_chat_request_serializes`: round-trip via JSON.
- `test_response_schema_variants_serializable`: each variant round-trips.

**Notes:** No tokio. Use `futures` types and let each impl pick its runtime. Native uses `reqwest`; WASM uses `wasm-bindgen-futures` + `web_sys::fetch`.

---

#### WP-702 — LM Studio provider (browser-direct)
**Crate:** `llm`
**Module:** `crates/llm/src/providers/lmstudio.rs`
**Depends on:** WP-701
**Estimate:** Medium

**Description:** Calls LM Studio's OpenAI-compatible API at `http://localhost:1234/v1/chat/completions`. Works in both native and WASM. WASM impl uses `web_sys::fetch`; native uses `reqwest`. CORS on LM Studio is permissive by default.

**Public API to add:**
```rust
pub struct LmStudioProvider {
    pub base_url: String,                            // default "http://localhost:1234"
    pub model: String,                                // user-selected
}

impl LmStudioProvider {
    pub fn new(base_url: String, model: String) -> Self;
    pub async fn list_models(&self) -> Result<Vec<String>, LlmError>;
}

#[async_trait::async_trait(?Send)]
impl LlmProvider for LmStudioProvider { /* ... */ }
```

**Acceptance criteria:**
- `test_request_payload_openai_compatible`: with mocked HTTP, the outgoing payload matches OpenAI Chat Completions schema.
- `test_streaming_parses_sse`: SSE chunks parse into `ChatChunk` events.
- `test_error_400_returns_provider_error`.
- (Manual integration) connecting to a running LM Studio returns models.

**Notes:** `web_sys::fetch` has no timeout. Add a `setTimeout`/AbortController dance in the WASM path or rely on user cancel.

---

#### WP-703 — Anthropic provider (server-only)
**Crate:** `llm`
**Module:** `crates/llm/src/providers/anthropic.rs` (gated `#[cfg(not(target_arch = "wasm32"))]`)
**Depends on:** WP-701
**Estimate:** Medium

**Description:** Anthropic Messages API. Only compiled on native. Used by the Axum server (WP-904).

**Public API to add:**
```rust
#[cfg(not(target_arch = "wasm32"))]
pub struct AnthropicProvider {
    api_key: String,
    model: String,                                    // e.g. "claude-sonnet-4-20250514"
    base_url: String,
}

impl AnthropicProvider {
    pub fn new(api_key: String, model: String) -> Self;
}
```

**Acceptance criteria:**
- `test_anthropic_payload_shape`: matches Anthropic Messages schema (system, messages, max_tokens, temperature).
- `test_streaming_parses_anthropic_sse`: handles the `message_start`/`content_block_delta`/`message_stop` event sequence.
- Validate against current public Anthropic API docs at implementation time. If schema has changed, follow the docs over this WP.

**Notes:** API key never crosses to WASM. Anthropic responses include thinking blocks if extended thinking is enabled — for narration prompts, leave thinking off.

---

#### WP-704 — Bedrock provider (server-only)
**Crate:** `llm`
**Module:** `crates/llm/src/providers/bedrock.rs`
**Depends on:** WP-701
**Estimate:** Medium

**Description:** AWS Bedrock InvokeModel / InvokeModelWithResponseStream API for Anthropic models on Bedrock. Uses `aws-sdk-bedrockruntime`. Server-only.

**Public API to add:**
```rust
#[cfg(not(target_arch = "wasm32"))]
pub struct BedrockProvider {
    pub region: String,
    pub model_id: String,                            // e.g. "anthropic.claude-sonnet-4-20250514-v1:0"
    pub client: aws_sdk_bedrockruntime::Client,
}

impl BedrockProvider {
    pub async fn from_env(region: String, model_id: String) -> Result<Self, LlmError>;
}
```

**Acceptance criteria:**
- Mocked-client unit tests for request shape (Bedrock wraps the Anthropic schema in its own envelope).
- Streaming variant emits ChatChunks correctly.

**Notes:** Bedrock IAM credential resolution via the AWS SDK's default chain. Document the IAM policy needed (`bedrock:InvokeModel`, `bedrock:InvokeModelWithResponseStream`).

---

#### WP-705 — Prompt template: Beat narration
**Crate:** `llm`
**Module:** `crates/llm/src/prompts/beat.rs`
**Depends on:** WP-608, WP-602
**Blocks:** WP-807, WP-1001
**Estimate:** Medium

**Description:** Build the system + user prompt that asks the LLM to narrate a Beat. Inputs: current Beat (intent, present NPCs, location), recent campaign log digest, current world state summary, last player action and engine outcome. Output: narrative paragraph(s) + list of player choices (constrained to the available transitions).

**Public API to add:**
```rust
pub struct BeatNarrationRequest<'a> {
    pub gig: &'a Gig,
    pub beat: &'a Beat,
    pub world_summary: &'a str,
    pub log_digest: &'a str,
    pub last_action: Option<&'a LastActionSummary>,
    pub available_choices: &'a [PlayerChoice],
}

pub struct LastActionSummary { pub action: String, pub outcome_facts: Vec<String> }
pub struct PlayerChoice { pub id: String, pub label: String, pub mechanical_hook: Option<MechanicalHookId> }

pub fn build_beat_narration_prompt(req: &BeatNarrationRequest) -> ChatRequest;

pub struct BeatNarrationResponse {
    pub narration: String,
    pub presented_choices: Vec<PresentedChoice>,
}

pub struct PresentedChoice { pub id: String, pub blurb: String }

pub fn parse_beat_narration_response(raw: &str) -> Result<BeatNarrationResponse, LlmError>;
```

**Acceptance criteria:**
- `test_prompt_includes_beat_intent`.
- `test_prompt_lists_available_choices`: only the IDs in `available_choices` are presented.
- `test_response_parses_well_formed`.
- `test_response_rejects_invented_choices`: parse fails (or filters) any presented choice ID not in `available_choices`.

**Notes:** The system prompt **forbids the LLM from inventing dice outcomes, DVs, or modifiers**. It may suggest a check, but the engine resolves it. Bake this into the system message verbatim.

---

#### WP-706 — Prompt template: NPC dialogue
**Crate:** `llm`
**Module:** `crates/llm/src/prompts/dialogue.rs`
**Depends on:** WP-605, WP-607, WP-608
**Estimate:** Medium

**Description:** Build the prompt for an NPC dialogue exchange. Inputs: NPC template (voice notes, dispositions), relationship history (digest of past interactions), player's line. Output: NPC's response + optional disposition delta (constrained to ±2) + optional flag changes (knowledge gained).

**Public API to add:**
```rust
pub struct DialogueRequest<'a> {
    pub npc: &'a NpcTemplate,
    pub relationship: &'a NpcRelationship,
    pub recent_log: &'a str,
    pub player_line: &'a str,
}

pub fn build_dialogue_prompt(req: &DialogueRequest) -> ChatRequest;

pub struct DialogueResponse {
    pub spoken: String,
    pub disposition_delta: i8,                        // -2..=+2
    pub knowledge_gained: Vec<KnowledgeFlag>,
    pub knowledge_revealed: Vec<KnowledgeFlag>,        // NPC reveals something they know
}

pub fn parse_dialogue_response(raw: &str) -> Result<DialogueResponse, LlmError>;
```

**Acceptance criteria:**
- `test_disposition_delta_clamped`: a response with `disposition_delta: 5` is rejected or clamped.
- `test_response_validates_knowledge_flags`: a `KnowledgeFlag` referencing an unknown NpcId is rejected.

---

#### WP-707 — Prompt template: IP scoring
**Crate:** `llm`
**Module:** `crates/llm/src/prompts/ip_scoring.rs`
**Depends on:** WP-609, WP-608
**Estimate:** Small

**Description:** Build the prompt that asks the LLM to score narrative quality of a completed gig and award bonus IP up to the cap. Inputs: full gig log excerpt, the cap. Output: `IpBonusResponse { awarded: u32 (≤ cap), reason: String }`.

**Public API to add:**
```rust
pub fn build_ip_scoring_prompt(log_excerpt: &str, cap: u32) -> ChatRequest;
pub fn parse_ip_scoring_response(raw: &str, cap: u32) -> Result<IpBonusResponse, LlmError>;
```

**Acceptance criteria:**
- `test_response_clamped_to_cap`.
- `test_negative_rejected`.
- `test_reason_required`: empty reason rejected.

---

#### WP-708 — Prompt template: Humanity event judgment
**Crate:** `llm`
**Module:** `crates/llm/src/prompts/humanity.rs`
**Depends on:** WP-606
**Estimate:** Small

**Description:** When a narrative event happens that *might* trigger Humanity loss (witnessing extreme violence, betraying an ally, etc.), ask the LLM whether it does, and if so by how much (constrained 0..=4 per event). LLM may say "no impact" — engine respects.

**Public API to add:**
```rust
pub struct HumanityJudgmentRequest<'a> {
    pub event_description: &'a str,
    pub character_summary: &'a str,
    pub recent_humanity_events: &'a str,
}

pub struct HumanityJudgmentResponse { pub humanity_delta: i8, pub reason: String }    // -4..=0

pub fn build_humanity_prompt(req: &HumanityJudgmentRequest) -> ChatRequest;
pub fn parse_humanity_response(raw: &str) -> Result<HumanityJudgmentResponse, LlmError>;
```

**Acceptance criteria:**
- `test_delta_in_range`: -4..=0 only.
- `test_positive_rejected`: an LLM that suggests "+1 humanity" via this hook is rejected (use the therapy WP for gains).

---

#### WP-709 — Constrained output validation (DV/modifier selection)
**Crate:** `llm`
**Module:** `crates/llm/src/constrained.rs`
**Depends on:** WP-701
**Estimate:** Medium

**Description:** Helper for "select from a fixed set" outputs. The LLM is asked to pick a DV from the standard ladder (9, 13, 15, 17, 21, 24) and/or a list of named modifiers (each ±1 or ±2). The response is parsed as JSON and validated against the available set. Anything outside the set is rejected.

**Public API to add:**
```rust
pub fn build_dv_selection_prompt(situation_description: &str, allowed_dvs: &[DV]) -> ChatRequest;
pub fn parse_dv_selection(raw: &str, allowed: &[DV]) -> Result<DV, LlmError>;

pub fn build_modifier_selection_prompt(situation: &str, allowed: &[NamedModifier]) -> ChatRequest;
pub fn parse_modifier_selection(raw: &str, allowed: &[NamedModifier]) -> Result<Vec<NamedModifier>, LlmError>;
```

**Acceptance criteria:**
- `test_dv_outside_allowed_rejected`: a response of `12` when allowed = [9,13,15] returns Err.
- `test_modifier_outside_allowed_rejected`.
- `test_well_formed_json_parses`.
- `test_invalid_json_returns_invalid_response_error`.

**Notes:** This is the lever that prevents "rules drift". The LLM proposes; the engine disposes. Every numeric the LLM might emit goes through one of these constrained selectors.

---

#### WP-710 — Streaming response handling
**Crate:** `llm`
**Module:** `crates/llm/src/stream.rs`
**Depends on:** WP-701, WP-702
**Estimate:** Small

**Description:** Common helper to consume a `BoxStream<ChatChunk>` and emit progressive narration to a UI subscriber. Buffer-and-flush at sentence boundaries to avoid mid-word flicker. Cancellation token support.

**Public API to add:**
```rust
pub struct StreamConsumer<F> where F: FnMut(StreamEvent) {
    on_event: F,
}

pub enum StreamEvent {
    Chunk(String),
    SentenceBoundary,
    Done(ChatResponse),
    Error(LlmError),
}

pub async fn consume_stream<F: FnMut(StreamEvent)>(stream: BoxStream<'_, Result<ChatChunk, LlmError>>, on_event: F) -> Result<ChatResponse, LlmError>;
```

**Acceptance criteria:**
- `test_emits_at_sentence_boundary`: with input "Hello world. Goodbye.", the consumer emits one `SentenceBoundary` event.
- `test_done_event_carries_full_response`.


---

### Phase 8 — Frontend (Leptos)

**Parallelism:** ~15 agents. Most depend on a stable rules + gm + llm interface; can prototype against mocked data while those land.

**Common conventions:**
- Use Leptos signals (`create_signal`, `create_resource`). Avoid `RwSignal`'s ergonomic but coarse mutation patterns where a signal pair is clearer.
- All UI components are functions returning `impl IntoView`.
- Styling: Tailwind via `tailwindcss-cli`, scoped CSS via Leptos `<Style>` for component-specific rules.
- Tests: `wasm-bindgen-test` for component logic; visual checks deferred to Phase 10 (manual + Playwright).
- Components live in `crates/web/src/components/`. Pages in `crates/web/src/pages/`.

#### WP-801 — Leptos app scaffold
**Crate:** `web`
**Module:** `crates/web/src/lib.rs`, `crates/web/src/app.rs`, `crates/web/index.html`, `Trunk.toml`
**Depends on:** WP-000
**Blocks:** WP-802–815
**Estimate:** Small

**Description:** Trunk-based Leptos CSR scaffold (no SSR for now — keeps deployment simple). Tailwind hooked up. Hello-world render.

**Acceptance criteria:**
- `trunk serve` brings up a page on localhost.
- `wasm-pack build crates/web --target web` succeeds in CI.

---

#### WP-802 — Routing and main shell
**Crate:** `web`
**Module:** `crates/web/src/router.rs`, `crates/web/src/components/shell.rs`
**Depends on:** WP-801
**Blocks:** WP-803–815
**Estimate:** Small

**Description:** Routes: `/`, `/new`, `/character`, `/play`, `/settings`. Persistent shell with nav. Use `leptos_router`.

---

#### WP-803 — Character creation flow UI
**Crate:** `web`
**Module:** `crates/web/src/pages/character_creation.rs`
**Depends on:** WP-501, WP-502, WP-503, WP-504, WP-802
**Estimate:** Large

**Description:** Multi-step flow: pick method (Streetrat / Edgerunner / Complete) → role → STAT roll/assign → skill assignments → lifepath rolls → starting gear → confirm. Shows the rolled values, lets the user re-roll where allowed, surfaces validation errors.

**Acceptance criteria:**
- `wasm-test_streetrat_flow_completes`: clicking through default selections produces a valid Character.
- `wasm-test_lifepath_displayed`: lifepath beacons are visible and re-rollable per RAW.

---

#### WP-804 — Character sheet view
**Crate:** `web`
**Module:** `crates/web/src/pages/character_sheet.rs`, `crates/web/src/components/sheet/*`
**Depends on:** WP-104, WP-802
**Estimate:** Large

**Description:** Read-only-ish view of the active Character: stats (base + current with deltas), skills (rank + linked stat + total), gear, cyberware (with HL paid), lifepath beacons, current effects, IP balance. Spend-IP affordances. Shows wound state with visible damage.

**Acceptance criteria:**
- Effects with negative modifiers are visually distinguished.
- Spending IP on a skill updates the rank in-place.
- Spending IP beyond the available balance is blocked.

---

#### WP-805 — SVG combat grid component
**Crate:** `web`
**Module:** `crates/web/src/components/combat_grid.rs`
**Depends on:** WP-302, WP-802
**Blocks:** WP-1001
**Estimate:** Large

**Description:** SVG-rendered grid (2 m squares). Tiles, walls, cover, occupants. Click-to-move (highlights movement options from the engine). Click-to-target. Keyboard navigation. Accessibility labels.

**Acceptance criteria:**
- `wasm-test_grid_renders_basic`: a 10×10 grid with 2 entities renders without panicking.
- `wasm-test_movement_options_highlighted`: clicking an entity highlights its `movement_options(...)` squares.
- `wasm-test_target_selection_callback`: clicking a hostile entity invokes the parent's target-set callback.

**Notes:** This is the visual centrepiece. Spend the time making it crisp. SVG over Canvas because tile updates are sparse.

---

#### WP-806 — NET architecture viewer
**Crate:** `web`
**Module:** `crates/web/src/components/net_architecture.rs`
**Depends on:** WP-401, WP-402, WP-802
**Blocks:** WP-1001
**Estimate:** Medium

**Description:** Vertical "elevator" visualisation of floors. Revealed floors visible; unrevealed floors hidden. Current floor highlighted. Black ICE / Demons / control nodes / passwords visually distinguished. Click a floor to see details when revealed.

---

#### WP-807 — Scene panel with LLM streaming
**Crate:** `web`
**Module:** `crates/web/src/components/scene_panel.rs`
**Depends on:** WP-705, WP-710, WP-802
**Blocks:** WP-1001
**Estimate:** Medium

**Description:** The narrative reading area. Streams the LLM's narration character-by-character (or sentence-by-sentence). Below the narration: action affordances from the current Beat's available choices. Player choice → action sent to engine → result fed back into next narration prompt.

**Acceptance criteria:**
- `wasm-test_streaming_renders_progressive`: chunk events update the visible text incrementally.
- `wasm-test_choice_buttons_dispatched`: clicking a choice fires the choice ID upward.
- Cancellation: switching scenes mid-stream cancels cleanly.

---

#### WP-808 — Inventory UI
**Crate:** `web`
**Module:** `crates/web/src/components/inventory.rs`
**Depends on:** WP-202, WP-203, WP-204, WP-212, WP-802
**Estimate:** Medium

**Description:** Inventory list with categories. Equip/unequip (changes WornArmor / weapon-in-hand). Drop. Use (consumes a usable item). Trade at vendors. Encumbrance indicator (if implemented).

---

#### WP-809 — Action selector (combat actions)
**Crate:** `web`
**Module:** `crates/web/src/components/action_selector.rs`
**Depends on:** WP-301, WP-805, WP-806
**Estimate:** Medium

**Description:** During combat, surfaces the currently-allowed actions for the active entity: Move, Attack (single, autofire if available, suppressive, shotgun shell, melee), Aim, Reload, Use Cover, Hold Action, Pass. Disabled when invalid (e.g. Autofire disabled if not enough bullets).

**Acceptance criteria:**
- `wasm-test_autofire_disabled_under_10_bullets`.
- `wasm-test_dodge_election_only_visible_with_ref_8`.
- `wasm-test_action_dispatched_with_target_id`.

---

#### WP-810 — Dice roll display (deterministic)
**Crate:** `web`
**Module:** `crates/web/src/components/dice_roll.rs`
**Depends on:** WP-002, WP-101
**Estimate:** Small

**Description:** Show the seed, the d10, the crit follow-up if any, the modifier breakdown, the final value, the DV, and the success/failure verdict. The user should be able to verify any roll. Optionally, "Replay from seed" debug control.

---

#### WP-811 — Save / load UI
**Crate:** `web`
**Module:** `crates/web/src/pages/save_load.rs`
**Depends on:** WP-902, WP-903
**Estimate:** Medium

**Description:** List saved games (from local storage in pure-local mode, or from server in synced mode). Save / load / delete. Auto-save indicator.

---

#### WP-812 — Settings (LLM provider selection)
**Crate:** `web`
**Module:** `crates/web/src/pages/settings.rs`
**Depends on:** WP-702, WP-703, WP-704, WP-802
**Estimate:** Small

**Description:** Pick provider: LM Studio (default; works offline; no backend), Anthropic via server, Bedrock via server. For LM Studio: fetch model list, pick model, test connection. Persist selection.

---

#### WP-813 — Beat transition UI
**Crate:** `web`
**Module:** `crates/web/src/components/beat_transition.rs`
**Depends on:** WP-604, WP-807
**Estimate:** Small

**Description:** When a Beat transitions, show a brief separator with the new Beat's title (or none if the LLM is meant to thread it seamlessly). User-configurable: visible separator vs inline.

---

#### WP-814 — NPC dialog UI
**Crate:** `web`
**Module:** `crates/web/src/components/npc_dialog.rs`
**Depends on:** WP-706, WP-807
**Estimate:** Medium

**Description:** Modal-style dialogue with the active NPC. Player types a line; LLM responds. Disposition indicator (subtle — a colour band, not a number). Available exit options from the dialogue (terminate, threaten, leave).

---

#### WP-815 — Map / location selector
**Crate:** `web`
**Module:** `crates/web/src/pages/map.rs`
**Depends on:** WP-006, WP-802
**Estimate:** Medium

**Description:** Region/district selection between Beats or for travel. Lists known locations. Distance + travel time displayed.


---

### Phase 9 — Backend (Axum)

**Parallelism:** ~8 agents. New crate `server`. Native-only.

#### WP-901 — Axum scaffold
**Crate:** `server`
**Module:** `crates/server/src/main.rs`, `crates/server/src/app.rs`
**Depends on:** WP-000
**Blocks:** WP-902–908
**Estimate:** Small

**Description:** Axum app with structured config (env-driven), tracing/logging via `tracing-subscriber`, graceful shutdown, CORS configured for the web frontend's origin in dev.

**Public API to add:**
```rust
pub struct AppState {
    pub db: SqlitePool,
    pub anthropic: Option<AnthropicProvider>,
    pub bedrock: Option<BedrockProvider>,
    pub content: ContentBundle,
}

pub fn router(state: AppState) -> Router;
```

**Acceptance criteria:**
- Server starts, responds 200 to `/healthz`.
- Graceful SIGINT handling.

---

#### WP-902 — SQLite migrations + sqlx setup
**Crate:** `server`, `persistence`
**Module:** `crates/persistence/src/lib.rs`, `crates/server/migrations/`
**Depends on:** WP-901, WP-004, WP-607
**Blocks:** WP-903
**Estimate:** Medium

**Description:** Schema for: characters, gigs in progress, campaign log, save snapshots, RNG seed log. Migrations via `sqlx migrate`. `persistence` crate exposes typed save/load operations.

**Public API to add:**
```rust
pub struct SaveStore { pool: SqlitePool }

impl SaveStore {
    pub async fn new(database_url: &str) -> Result<Self, PersistenceError>;
    pub async fn migrate(&self) -> Result<(), PersistenceError>;
    pub async fn save_character(&self, c: &Character) -> Result<(), PersistenceError>;
    pub async fn load_character(&self, id: CharacterId) -> Result<Character, PersistenceError>;
    pub async fn save_log(&self, log: &CampaignLog) -> Result<(), PersistenceError>;
    pub async fn load_log(&self, character: CharacterId) -> Result<CampaignLog, PersistenceError>;
    pub async fn snapshot_world(&self, character: CharacterId, world: &World, seed: u64) -> Result<SnapshotId, PersistenceError>;
    pub async fn load_snapshot(&self, id: SnapshotId) -> Result<(World, u64), PersistenceError>;
}

pub struct SnapshotId(pub Uuid);
```

**Acceptance criteria:**
- `test_migrate_idempotent`: running migrations twice succeeds.
- `test_save_load_round_trip`: a Character round-trips through the store.
- `test_snapshot_includes_seed`: a snapshot stores both the World and the RNG seed used.

**Notes:** Snapshots use serde-RON or Postcard for compactness. Don't try to project Character into normalised tables — store as a serialised blob keyed by id. Indexed columns are: character_id, gig_id, created_at.

---

#### WP-903 — Save sync endpoints
**Crate:** `server`
**Module:** `crates/server/src/routes/saves.rs`
**Depends on:** WP-902
**Blocks:** WP-811
**Estimate:** Medium

**Description:** REST endpoints for the web frontend to push and pull saves.

```
GET    /api/characters                — list saved characters
POST   /api/characters                — create new
GET    /api/characters/:id            — load
PUT    /api/characters/:id            — save (full character + log)
DELETE /api/characters/:id            — delete
GET    /api/snapshots/:id             — load a world snapshot
POST   /api/snapshots                 — create snapshot
```

**Acceptance criteria:**
- `test_post_then_get_roundtrip`.
- `test_delete_then_get_404`.
- `test_invalid_json_400`.

---

#### WP-904 — LLM proxy: Anthropic
**Crate:** `server`
**Module:** `crates/server/src/routes/llm_anthropic.rs`
**Depends on:** WP-703, WP-901
**Estimate:** Medium

**Description:** Proxy `POST /api/llm/anthropic/messages` and `POST /api/llm/anthropic/stream`. The web client never sees the API key. Server signs and forwards. Streams as SSE.

**Acceptance criteria:**
- `test_request_relays_correctly`: with mocked Anthropic client, request body forwarded verbatim.
- `test_streams_sse_to_client`.
- `test_503_when_unconfigured`: if no Anthropic key, returns 503 with explanatory body.

**Notes:** Rate-limit per session (token bucket). Log usage for billing visibility.

---

#### WP-905 — LLM proxy: Bedrock
**Crate:** `server`
**Module:** `crates/server/src/routes/llm_bedrock.rs`
**Depends on:** WP-704, WP-901
**Estimate:** Medium

**Description:** Same as WP-904 but for Bedrock. AWS SDK handles signing.

---

#### WP-906 — Content delivery
**Crate:** `server`
**Module:** `crates/server/src/routes/content.rs`
**Depends on:** WP-901
**Estimate:** Small

**Description:** Serve the contents of `content/` to the web client. The web client may also bundle content directly (build-time), so this is for hot-reload during dev. Endpoints:

```
GET /api/content/catalogs/:name
GET /api/content/gigs
GET /api/content/gigs/:id
GET /api/content/locations/:id
```

---

#### WP-907 — Light auth
**Crate:** `server`
**Module:** `crates/server/src/auth.rs`
**Depends on:** WP-901
**Estimate:** Small

**Description:** Single-user mode by default — auth disabled. Optional bearer token via env var; if set, all `/api` routes require it. Future-proofed for multi-user without committing to it now.

---

#### WP-908 — Health and metrics
**Crate:** `server`
**Module:** `crates/server/src/routes/health.rs`
**Depends on:** WP-901
**Estimate:** Small

**Description:** `/healthz` (liveness), `/readyz` (DB reachable), `/metrics` (Prometheus exposition with token counts, save/load counts, LLM call latencies).


---

### Phase 10 — Integration

**Parallelism:** ~4 agents. Final assembly.

#### WP-1001 — First playable gig
**Crate:** `content`, validated end-to-end
**Module:** `content/gigs/hot_property.ron`, `content/encounters/*.ron`, `content/npcs/*.ron`, `content/locations/*.ron`
**Depends on:** WP-501, WP-604, WP-610, WP-612, WP-613, WP-705, WP-805, WP-807
**Estimate:** Large

**Description:** Author a complete Beat Chart for the "Hot Property" gig (recover stolen prototype from a Maelstrom warehouse). Hook → Warehouse Approach (cliffhanger encounter) → Warehouse Interior (combat / stealth) → Locked Network (netrun) → Climax (boss fight or escape) → Resolution (payment with Padre).

Includes:
- ~6 Beats covering all five Beat kinds.
- 2 combat encounters with full grid layouts.
- 1 NET architecture (procedurally seeded but fixed for QA).
- 4–5 NPCs (Padre, Ms. Stout the lieutenant, two Maelstrom grunts, optional ally hire).
- Mechanical hooks across all hook kinds.
- Multiple transition paths showing both success and failure routes leading to Resolution.

**Acceptance criteria:**
- A user can complete the gig end-to-end through the web UI.
- All transitions are reachable in some playthrough (covered by manual playtest matrix).
- The content validator passes.

---

#### WP-1002 — End-to-end smoke test
**Crate:** `tools/replay`
**Module:** `tools/replay/src/main.rs`, `tools/replay/tests/smoke.rs`
**Depends on:** WP-1001, WP-902
**Estimate:** Medium

**Description:** A scripted "no-LLM" playthrough of WP-1001 driving the engine directly: create character (Streetrat Solo, fixed seed) → load gig → resolve hooks deterministically → fight encounter → run net architecture → take Resolution. Replay tool consumes a YAML/RON action script and exercises the rules engine end-to-end. Used in CI as a regression test.

**Public API to add:**
```rust
pub struct ActionScript { pub seed: u64, pub actions: Vec<ScriptedAction> }
pub enum ScriptedAction { CreateCharacter(StreetratSpec), StartGig(GigId), ResolveHook { id: MechanicalHookId, choice: ChoiceSelection }, MoveTo((u16, u16)), Attack { target: EntityId, kind: AttackKind }, NetAction(NetActionKind), Pass, JackOut, EndGig }
pub fn run_script(script: ActionScript, content: &ContentBundle) -> ScriptOutcome;
```

**Acceptance criteria:**
- A canonical script for "Hot Property" runs to completion.
- The same script + same seed produces the same outcome on every run.
- CI runs the smoke script on every PR.

---

#### WP-1003 — Performance benchmarks
**Crate:** `crates/rules`, `crates/gm` (criterion benches)
**Module:** `crates/rules/benches/`
**Depends on:** WP-1002
**Estimate:** Small

**Description:** Criterion benchmarks for: skill check, attack resolution, full-round combat tick, beat transition, log digest generation, content load. Set targets and enforce in CI as soft gates (warn on regression).

**Targets (initial — adjust after first measurement):**
- Skill check: < 5 µs
- Single-shot attack incl. damage application: < 50 µs
- Round of combat (10 entities): < 1 ms
- Log digest (1k events): < 5 ms
- Load all content: < 200 ms cold

---

#### WP-1004 — Documentation and onboarding
**Crate:** workspace
**Module:** `README.md`, `docs/`
**Depends on:** WP-1001, WP-1002
**Estimate:** Medium

**Description:** Project README: what it is, how to run (local dev with LM Studio; production with server + Anthropic/Bedrock). Architecture overview (link to this doc). Contributing guide: how to add a new gig, a new weapon, a new role ability. Onboarding checklist for new contributors. Troubleshooting (LM Studio CORS, Bedrock IAM, sqlx offline mode).


---

## 5. Coordination Protocols

### 5.1 Branching and PRs

- One WP per branch. Branch name: `wp-XXX-short-description` (e.g. `wp-303-damage-pipeline`).
- Commit prefix: `[WP-XXX]` on every commit.
- One PR per WP. PR title: `[WP-XXX] Title`. PR description must include:
  1. The WP ID and a one-line summary.
  2. The rulebook pages consulted (e.g. "Pages: 187–188 (Critical Injuries)").
  3. A note on any deviation from the WP's stated public API and *why*.
  4. Confirmation that all acceptance tests pass.
- Squash-merge to main. Keep individual `[WP-XXX]` commits in the PR for review.

### 5.2 Public API stability

When a WP is merged, **its public API is locked**. Other agents may rely on it.

If you discover that a WP's stated API is wrong (a missing parameter, an enum variant that doesn't make sense, an integer width that overflows), you have three options:

1. **Coexist.** Add the new variant/parameter/method without breaking what's there. Document the addition in a `[WP-XXX-revision]` PR.
2. **Coordinate.** Open a small "design question" PR or issue describing the change and which downstream WPs need updates. Don't push the breaking change until those WPs' agents have acknowledged.
3. **Escalate.** If the conflict is fundamental (e.g. two WPs need to own the same data), raise it to the user. Don't silently break.

### 5.3 Shared test fixtures

- `crates/rules/tests/fixtures/characters/` — sample characters (a Solo, a Netrunner, a Tech, a Mortally-Wounded character, a heavily-cybered character).
- `crates/rules/tests/fixtures/seeds/` — canonical RNG seeds for deterministic tests with documented expected outcomes.
- `crates/gm/tests/fixtures/gigs/` — sample Beat Charts.
- `content/test_content/` — content bundles used by the replay tool (WP-1002).

If your WP needs a fixture that doesn't exist, add it to the appropriate folder *and* add a `// fixture: ...` comment in your test explaining what it is. Don't shadow another WP's fixture.

### 5.4 When to ask vs implement

**Just implement** when:
- The rulebook has the answer, even if it takes a careful re-read.
- The WP's notes explicitly call out the choice and you can pick a sensible default.
- The change is contained to your WP's module and doesn't ripple.

**Ask first** when:
- Two WPs have a non-obvious overlap.
- A rulebook clarification is genuinely ambiguous and the design decision matters.
- You want to add a new public type that other WPs will need to adopt.
- A WP's acceptance criteria seem wrong or unreachable.

### 5.5 Working with the rulebook

- The rulebook PDF is at `rulebook/Cyberpunk_Red_Core-Digital_v1.25.pdf` (workspace root, developer-provided — see §0.4). Read cited pages directly — don't rely on memory or this document's paraphrases.
- Cite page numbers in code comments where rules are encoded: `/// See p.187 (Critical Injuries to the Body).`
- If you find a rule the WP didn't anticipate (e.g. a special interaction between two cyberware items), encode it and note it in your PR description.
- The book occasionally has self-contradictions or RAW vs RAI tension. Default to RAW. Document the tension in the code comment.

### 5.6 Rust style

- Run `cargo fmt --all` before committing. CI enforces.
- Run `cargo clippy -- -D warnings`. CI enforces.
- No allocations in hot paths (combat resolution, derivation queries) where avoidable. `Vec`s with `with_capacity` if size is known.
- `pub(crate)` over `pub` when types don't need to leave the crate.
- Doc comments on every public item.

### 5.7 What "done" means for a WP

A WP is done when **all** of the following are true:

1. All acceptance tests in the WP pass locally.
2. The WP's full public API is implemented as specified (or an explicitly-documented variant).
3. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test --workspace` all pass.
4. WASM build succeeds where the WP touches `web` or shared crates.
5. The PR description references the WP ID and rulebook pages.
6. Doc comments on every public item.

Not done — even if functional — if any of those fail.

---

## 6. Verification Gates

Each Phase has an entry gate (what must be true to start work in this phase) and an exit gate (what must be true to declare the phase complete).

### Phase 0
- **Entry:** none.
- **Exit:** `cargo build --workspace` and `wasm-pack build crates/web` both succeed; CI green; all of WP-000 through WP-006 merged.

### Phase 1
- **Entry:** Phase 0 exited.
- **Exit:** All Phase 1 WPs merged. `cargo test -p rules --features test-support` passes including dice property tests.

### Phase 2
- **Entry:** WP-001, WP-003 merged.
- **Exit:** All catalogs load successfully. `tools/content-validator` passes against all `content/catalogs/`. Every catalog has > 0 entries (no empty catalogs slipped in).

### Phase 3
- **Entry:** Phase 1 exited; WP-202, WP-203, WP-205 merged.
- **Exit:** Combat smoke test (a fixed-seed scenario script) runs end-to-end through the rules crate. All Phase 3 WPs merged.

### Phase 4
- **Entry:** Phase 1 exited; WP-208, WP-209, WP-210 merged.
- **Exit:** A fixed-seed netrun script runs end-to-end. All Phase 4 WPs merged.

### Phase 5
- **Entry:** Phase 1 exited; WP-201, WP-204, WP-213, WP-214 merged.
- **Exit:** A character can be created via Streetrat, given Lifepath beacons, given starting cyberware (with HL applied), and serialised/deserialised intact. All Phase 5 WPs merged.

### Phase 6
- **Entry:** Phase 1 exited; WP-005 merged.
- **Exit:** A sample Beat Chart loads, validates, runs through its state machine via scripted hook outcomes, and emits a campaign log. All Phase 6 WPs merged.

### Phase 7
- **Entry:** WP-602 merged for prompt templates that depend on Beat schema; WP-608 for digest.
- **Exit:** `LmStudioProvider` connects to a running LM Studio and returns a successful response from each prompt template. (Manual integration test; automated tests use mocks.)

### Phase 8
- **Entry:** WP-001 merged for shared types; WP-202, WP-203, WP-302, WP-401 merged so visualiser components can hook to real data shapes.
- **Exit:** All major UI components render against fixture data without panicking. Combat grid, sheet, scene panel, settings all functional in `trunk serve`.

### Phase 9
- **Entry:** WP-902 merged.
- **Exit:** `cargo run -p server` brings up the API; integration tests via `reqwest` against a test server pass.

### Phase 10
- **Entry:** Phases 5–9 exited.
- **Exit:** WP-1001 gig fully playable end-to-end with LM Studio; WP-1002 smoke test passes in CI; WP-1003 benchmarks publish baseline numbers; WP-1004 docs published.

### Coverage targets

- `rules` crate: ≥ 85 % line coverage, with property tests on dice and combat math.
- `gm` crate: ≥ 80 %.
- `llm`: focus on prompt construction and parser tests; provider impls covered by mock-server tests.
- `web`: spot-check via `wasm-bindgen-test`; primary verification is manual + Playwright (Phase 10).
- `server`: ≥ 75 %; integration tests against an in-process server.

### Performance targets

See WP-1003. Soft CI gates (warn on >20 % regression). Hard gate: any benchmark > 10× the target fails CI.

---

**End of plan.**
