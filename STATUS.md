# Project Status — Cyberpunk Red CRPG

**Snapshot:** 2026-05-10. Use `git log --oneline main` for the authoritative progress record; this document summarises state at the time of writing.

## Phase progress

| Phase | Theme | WPs | Status |
|---|---|---|---|
| 0 | Foundation (workspace, core types, RNG, effect skeleton) | 7 | ✅ complete |
| 1 | Core rules mechanics (dice, checks, derivation, wound states) | 7 | ✅ complete |
| 2 | Data catalogs (weapons, armor, cyberware, ICE, etc.) | 14 | ✅ complete |
| 3 | Combat subsystems (initiative, attacks, damage, criticals) | 16 | ✅ complete |
| 4 | Netrunning (architecture, abilities, ICE, demons) | 17 | ✅ complete |
| 5 | Character & progression (creation, lifepath, role abilities, IP) | 19 | ✅ complete |
| 6 | GM layer (Beat Charts, NPCs, campaign log) | ~13 | 🟡 in progress (4/13: Wave 1 + prep done) |
| 7 | LLM layer (provider trait, prompts) | ~10 | not started |
| 8 | Frontend (Leptos UI) | ~15 | not started |
| 9 | Backend (Axum endpoints) | ~8 | not started |
| 10 | Integration (sample gig, smoke tests, perf, docs) | ~4 | not started |

**Test count:** 634 passing in `cpr_rules` (preserved from Phase 5) + **16 in `cpr_gm`** = 650 unit tests. Workspace gates (`cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace && wasm-pack build crates/web --target web`) all green.

**Source size growth:** Phase 5 added ~9k lines across `character/creation/`, `character/lifepath.rs`, `character/cyberware.rs`, `character/cyberpsychosis.rs`, `character/therapy.rs`, `character/progression/`, and 10 sub-modules under `roles/`. Phase 6 Wave 1 adds the `cpr_gm` crate skeleton (~1.5k lines): `error.rs` (22 variants), `ids.rs` (5 newtypes via macro), `beats/schema.rs` (Gig/Beat/Transition types), `npc/entity.rs` (NpcTemplate + ActiveNpc + 12 mook archetypes), `ip/llm_bonus.rs` (capped IP bonus).

## Phase 6 progress (in flight)

The `cpr_gm` crate is now stood up. Wave structure based on dependency analysis:

| Wave | WPs | Status | Notes |
|---|---|---|---|
| Prep | WP-601 (gm scaffolding) | ✅ merged (`1f5a798`) | `GmError` 22 variants, slug-IDs, module skeleton |
| 1 | WP-602 (Beat Chart schema), WP-605 (NPC entity model), WP-609 (IP bonus cap) | ✅ all merged (`a311493`, `f0c4c0c`, `f66fb53`) | 8 acceptance tests across the wave |
| 2 | WP-603 (Beat loader/validator), WP-606 (NPC instantiation), WP-607 (Campaign log) | ⏳ next | Each depends on a Wave 1 WP |
| 3 | WP-604 (Beat state machine), WP-608 (Digest), WP-610 (Encounter loader), WP-611 (Faction tracking), WP-612 (NPC ally hiring) | pending | Depends on Wave 2 |
| 4 | WP-613 (Mechanical hook resolver) | pending | Depends on WP-604 |

**Key deviations landed in Wave 1:**
- **WP-605 renamed `NpcId` → `NpcTemplateId`** (the plan's `NpcId` for slug-based templates collides with `cpr_rules::NpcId` UUID for runtime instances). Documented as §5.2 "Coexist".
- **WP-609 added `request_cap: u32` parameter** to `award_llm_bonus_ip` — the *caller* controls the cap, not the LLM's response.
- **WP-602's `MechanicalHookKind` filled in** the `OpposedCheck`, `Negotiation`, `Ambush` fields the plan left as `/* ... */` placeholders. Additive only; downstream WP-613 should consume these.
- **Several `MechanicalHookKind` types reference `NpcId` by `String`** with `// TODO(WP-605)` markers; should be tightened to `NpcTemplateId` in a follow-up.

## What's next: Phase 6 Wave 2

Wave 2 (3 agents, all unblocked by Wave 1):
- **WP-603** Beat Chart loader and validator — depends on WP-602 ✅
- **WP-606** NPC instantiation from template — depends on WP-605 ✅
- **WP-607** Structured campaign log — depends on WP-605 ✅ (uses `NpcTemplateId`)

After Phase 6, the picture for the WP-1001 sample-gig demo:
- **Phase 7 (LLM)** — provider trait, prompts. Needs WP-602 (Beat Chart schema) and WP-608 (digest).
- **Phase 8 (Frontend)** — Leptos UI. Needs WP-001 + WP-202 + WP-203 + WP-302 + WP-401 (all ✅).
- **Phase 9 (Backend)** — Axum endpoints. Needs WP-902 (server scaffolding).
- **Phase 10** — sample gig + smoke tests + perf + docs. Needs Phases 5–9.

If you want maximum throughput, Phase 8 (Frontend) and Phase 9 (Backend) can run in parallel with Phase 6/7 since they live in different crates (`web`, `server` vs `gm`, `llm`).

## Workflow lessons (read before launching Phase 5+)

These were learned the hard way during Phase 3+4. Apply them to keep orchestration cost low.

### What works
- **Sub-agents on `model: "sonnet"`** — pinned in `IMPLEMENTATION_PLAN.md` §0.2 and `CLAUDE.md`. Sonnet is fast and reliable for WP-sized tasks.
- **Worktree isolation** (`isolation: "worktree"`) — each agent gets its own checkout, no working-tree contention.
- **Self-contained prompts** — agents start cold; the prompt must include rulebook page numbers, the public API contract, every acceptance test name, and project conventions.
- **Single-batch local consolidation** for large parallel waves — cherry-pick each branch's *new* feature files onto local main, hand-merge shared scaffolding (`error.rs`, `mod.rs` files), one local `cargo fmt && clippy && test` pass, push as one commit. **5–10× faster than per-PR rebase + CI + merge** when the queue is big.

### What hurts
- **`error.rs` collisions** — 8+ agents each invent overlapping `RulesError` variants for the same NET concepts. Consolidating after the fact is mechanical but tedious.
- **`mod.rs` cascade** — every WP adds one `pub mod foo;` line; merging PR N invalidates PRs N+1..K because they all touch the same lines.
- **CI / clippy version drift** — Sonnet's local clippy is older than CI's. Lints like `unnecessary_get_then_check`, `get_first`, `collapsible_if`, `assertions_on_constants`, `bool_comparison` pass locally but fail in CI. Always tell agents to use `!map.contains_key(&k)` over `map.get(&k).is_none()`, `.first()` over `.get(0)`, and avoid `assert!(true, ...)`.
- **Wave size 19 was too big.** 5–8 in parallel is the sweet spot when you have to manually serialise merges.

### Recommended pattern for Phase 5+
1. **Pre-stage shared types** in a single prep commit *before* spawning agents. List every `RulesError` variant the wave's WPs need, every `mod.rs` declaration, every shared trait. Land that prep commit on main first.
2. **Tell agents NOT to add to `error.rs` or `mod.rs` files** — those are pre-staged. Agents only add their feature `.rs` files.
3. **Cap parallelism at ~6 agents per wave.** Larger waves → more cross-PR conflicts → orchestration tax.
4. **Use Track 1 (cherry-pick + local consolidate)** for big batches. The path: `for branch in branches; do git checkout origin/$branch -- <new-file>; done` then hand-edit shared files, run gates, single commit, push to main, close superseded PRs.
5. **PR-per-WP is fine for small waves (≤4 agents)** because cascade conflict cost is bounded.

### Phase 5 confirmed the pattern works
Phase 5's three waves (6 + 6 + 7 agents) used the prep-commit pattern and ran with significantly lower conflict overhead than Phase 3+4 Wave 3. Most conflicts were 1-line `pub mod foo;` additions to `roles/mod.rs` — auto-mergeable on first try when only 1-2 PRs touched it; needed manual resolution only when 3+ PRs cascaded. **CI clippy lints (`unnecessary_get_then_check`, `manual_range_contains`, `get_first`)** still occasionally bite — Sonnet's local toolchain is slightly behind CI's. Always tell agents to use `(2..=8).contains(&s)` over `s >= 2 && s <= 8`, `.first()` over `.get(0)`, `.contains_key(k)` over `.get(k).is_none()`.

### Phase 6 Wave 1 — two new lessons

1. **Worktree isolation can leak between parallel sub-agents.** Two of three Wave 1 agents (WP-602, WP-609) reported sharing a worktree — the WP-602 agent saw WP-609's in-progress files in its checkout. The WP-602 commit accidentally bundled WP-609's `ip/llm_bonus.rs` and `ip/mod.rs`. **It auto-resolved only because the WP-602 agent cherry-picked WP-609's code byte-identically** — when WP-609 merged first, the duplicates became no-ops. Had the agents written even slightly different versions of the shared files, manual rebase would have been required. Mitigations to apply going forward:
   - Tell agents to `git status` before staging and **add only files in their own module path** (`git add crates/gm/src/<my-topic>/`) rather than `git add -A` or `git add .`.
   - Tell agents to verify with `git diff origin/main...HEAD` that their PR contains only their own files before pushing.
2. **Cargo.toml dev-deps need pre-staging too, not just shared types and modules.** Three Wave 1 agents independently added `uuid = { version = "1", features = ["serde"] }` to `[dev-dependencies]`. It auto-resolved because git merges identical-line-additions cleanly, but a one-character variation between agents would have required manual reconciliation. For Wave 2+, **pre-stage any dev-dep likely to be needed** in the prep commit — extend the "prep-stage shared types" pattern to include shared dev-deps.

## Open follow-ups / known debt

These are tracked in commit messages and PR descriptions but worth surfacing here:

### Coordination follow-ups
- **`DiceSpec` is defined three times** locally in WP-202 weapons, WP-204 cyberware, WP-208 programs (and a fourth time in WP-209 Black ICE). All same shape `{ n: u8, die: DieKind }`. Worth a one-shot consolidation PR to a single shared type in `rules::dice`.
- **`WrongProgramClass` aliases `ProgramWrongClass`** — both variants exist on `RulesError` with `{program, expected, got}` fields. Consolidate to one.
- **`NetrunNotActive` aliases `NoActiveNetrun`** — same. Consolidate.
- **`CatalogLoadFailed` reused as "catalog entry not found"** in WP-505 (cyberware) — semantic abuse. Add a dedicated `CatalogEntryNotFound { catalog: &'static str, slug: String }` variant in a follow-up.
- **`IpInsufficient` reused as "money insufficient"** in WP-507 therapy — money is conceptually separate. Add a dedicated `InsufficientFunds { required: Eurobucks, available: Eurobucks }` variant.

### Phase 5 deviations to revisit
- **WP-503 Complete Package** — 13 Basic Skills minimum-rank-2 enforcement reuses `RankCapReached` for too-low values, which is semantically wrong. Add `BasicSkillBelowMinimum`.
- **WP-507 Therapy** — humanity cap uses `10 × stats.emp` proxy; should use a tracked `humanity_max` field on Character once added.
- **WP-512 Maker `Field` expertise** — `maker_cost` returns `Eurobucks(0)` since RAW gives no explicit jury-rig cost; GM adjudicates.
- **WP-513 Medicine** — `pharmaceuticals_hp_healed` returns rank scaling factor, not absolute HP; the `BODY + WILL` baseline (Speedheal RAW) needs caller multiplication.
- **WP-515 Backup arrival** — RAW is d6 in Rounds; engine returns deterministic minutes for planning. Fold into a richer `BackupArrival { rolls_to_arrive: u8 }` later.
- **WP-516 Teamwork money_per_gig** — engine extrapolation; no RAW per-gig pool.
- **WP-517 Operator multiplier** — Haggle effects (bulk deal, deferred payment) at intermediate ranks return 100 (no discount); needs a separate action-level mechanic.
- **WP-518 Charismatic Impact tier brackets** — WP spec brackets differ slightly from RAW; reconcile in a future RAW-pass commit.
- **WP-519 Family Motorpool** — returns vehicle-kind buckets, not specific vehicle slugs. Per-model rank gating needs catalog-level metadata.

### Unfinished spec items
- **WP-104 `skill_base` uses `Stat::Int` stub** — superseded by `skill_base_with_stat(skill, linked_stat)` helper. Now that WP-201's skill catalog provides `linked_stat()`, `skill_base` could call it directly. Small refactor.
- **WP-401 `Floor::BlackIce.ice_per` defaults to 0** — placeholder for the catalog-wired PER. Should be populated from `Catalog<BlackIce>` when generating architectures (the catalog is already loaded).
- **WP-402 `interface_rank` denormalised** in several places (`DemonState`, `NetrunState`). One source of truth would be cleaner.
- **WP-414 uses `current_rez` as DEF proxy** for Anti-Program ICE attacks — `RezzedProgram` has no separate DEF field. Worth adding when wiring program catalog DEF stats.
- **WP-417 `unsafe_jack_out` damage_to_netrunner is u16(0) placeholder** — should call into WP-414's effect application.
- **`Grid` placeholder** in `combat/grid.rs` was replaced by WP-302; but `combat/turn_engine.rs::CombatState::start` still passes `Grid::default()` (empty grid). Real combat needs a populated grid from the scene/encounter setup.
- **`insert_at_top` in WP-301** takes `&World` then discards it. WP-417 had to use `world.clone()` to satisfy borrow rules. Consider tightening the signature.

### Workflow / infrastructure
- **Multiple `.claude/worktrees/agent-*` worktrees** are still locked from completed sub-agents. The harness will clean these up on session exit, but `git worktree list` shows them today. Not a blocker.
- **STATUS.md (this file)** is meant to be updated at the end of each major work session. If you bring up a new session and start work, update this file at the close.

## How to verify the current state

```bash
# From the repo root:
git log --oneline main | head -20             # Recent merges
cargo fmt --all -- --check                    # Formatting clean
cargo clippy --workspace --all-targets -- -D warnings  # Lints clean
cargo test --workspace                        # 494 tests pass in cpr_rules
wasm-pack build crates/web --target web       # WASM builds
```

Phase exit gates per `IMPLEMENTATION_PLAN.md` §6:
- **Phase 0** ✅: `cargo build --workspace` and `wasm-pack build crates/web` succeed; CI green; WP-000 through WP-006 merged.
- **Phase 1** ✅: All Phase 1 WPs merged; `cargo test -p rules` passes including dice property tests.
- **Phase 2** ✅: All catalogs load; every catalog has > 0 entries.
- **Phase 3** ✅: All Phase 3 WPs merged; combat smoke-test path is end-to-end through the rules crate.
- **Phase 4** ✅: All Phase 4 WPs merged; netrun path runs end-to-end.
- **Phase 5** ✅: All 19 WPs merged. Streetrat / Edgerunner / Complete Package creation, Lifepath roller, cyberware install + Humanity Loss, cyberpsychosis, therapy, IP earn/spend, all 10 Role Abilities. A character can now be created via Streetrat, given a Lifepath, given starting cyberware (with HL applied), serialised/deserialised via RON.

## Architectural state of the rules crate

```
crates/rules/src/
├── lib.rs                    # crate root, re-exports
├── error.rs                  # RulesError (16 variants)
├── types.rs                  # CharacterId, EntityId, NpcId, EffectInstanceId, DV, Eurobucks, PriceTier, Stat
├── rng.rs                    # ChaCha20Rng alias
├── dice.rs                   # d10, d6, d10_with_crits, ndn_d6
├── resolution.rs             # Resolution trait, CheckBreakdown
├── world.rs                  # World, GameClock, LocationId
├── movement.rs               # WP-107 movement primitives
├── effects/
│   ├── mod.rs                # EffectStack, ActiveEffect, EffectSource, EffectDuration, WoundState, EnvironmentalKind, ProgramId, DrugId, RoleAbilityId, SkillId (re-export)
│   └── modifier.rs           # EffectModifier (~20 variants), Hand, HpDamage
├── character/
│   ├── mod.rs                # Character struct
│   ├── data.rs               # StatBlock, SkillSet, Role, Wounds, WornArmor, ArmorPiece, InstalledCyberware, Inventory, ItemStack, ItemKind, WeaponId, AmmoKind
│   ├── derive.rs             # WP-104 current_X() accessors
│   ├── luck.rs               # WP-103 LUCK pool
│   ├── hp.rs                 # WP-105 HP/Humanity derivation
│   └── wounds.rs             # WP-106 wound transitions, death saves
├── checks/
│   ├── mod.rs
│   ├── skill_check.rs        # WP-101 SkillCheck, OpposedCheck
│   └── complementary.rs      # WP-102 ComplementaryBonus
├── catalog/
│   ├── mod.rs                # Catalog<T> generic
│   ├── skills.rs             # WP-201 SkillId enum, SkillDefinition, linked_stat()
│   ├── weapons.rs            # WP-202 Weapon, RangeBand, DamageDice, etc.
│   ├── armor.rs              # WP-203 Armor, ArmorKind closed enum
│   ├── cyberware.rs          # WP-204 Cyberware (96 entries across 8 categories)
│   ├── critical_injuries.rs  # WP-205 CriticalInjury, roll_critical_injury
│   ├── drugs.rs              # WP-206 Drug catalog
│   ├── cover.rs              # WP-207 CoverMaterial
│   ├── programs.rs           # WP-208 Program (Booster/Defender/Attacker)
│   ├── black_ice.rs          # WP-209 BlackIce
│   ├── demons.rs             # WP-210 Demon
│   ├── vehicles.rs           # WP-211 Vehicle
│   ├── night_market.rs       # WP-212 NightMarketItem (153 entries)
│   ├── roles.rs              # WP-213 RoleDefinition
│   └── lifepath.rs           # WP-214 Lifepath, all role-specific tables
├── combat/
│   ├── mod.rs
│   ├── turn_engine.rs        # WP-301 CombatState, initiative
│   ├── grid.rs               # WP-302 Grid, LOS, AoE
│   ├── damage.rs             # WP-303 apply_damage
│   ├── critical_injury.rs    # WP-305 apply_critical_injury
│   ├── ranged_single.rs      # WP-306 single-shot ranged attack (+ WP-308 aimed shot tests)
│   ├── melee.rs              # WP-307 melee attack
│   ├── autofire.rs           # WP-309 autofire
│   ├── suppressive.rs        # WP-310 suppressive fire
│   ├── shotgun_shell.rs      # WP-311 shotgun shells
│   ├── explosives.rs         # WP-312 explosives
│   ├── cover.rs              # WP-313 apply_cover
│   ├── shields.rs            # WP-314 EquippedShield
│   ├── grapple.rs            # WP-315 grapple/choke/throw
│   └── dodge.rs              # WP-316 can_elect_dodge_ranged
└── netrunning/
    ├── mod.rs
    ├── architecture.rs       # WP-401 NetArchitecture, generator
    ├── state.rs              # WP-402 NetrunState
    ├── actions.rs            # WP-403 net_actions_per_turn
    ├── black_ice.rs          # WP-414 BlackIceEncounter
    ├── demon.rs              # WP-415 DemonState
    ├── integration.rs        # WP-417 jack-out, queue insertion
    ├── virus.rs              # WP-416 DeployVirusAction
    ├── abilities/
    │   ├── mod.rs
    │   ├── scanner.rs        # WP-404
    │   ├── backdoor.rs       # WP-405
    │   ├── cloak.rs          # WP-406
    │   ├── control.rs        # WP-407
    │   ├── eye_dee.rs        # WP-408
    │   ├── pathfinder.rs     # WP-409
    │   ├── slide.rs          # WP-410
    │   └── zap.rs            # WP-411
    └── programs/
        ├── mod.rs
        ├── active.rs         # WP-412 (Boosters & Defenders)
        └── attackers.rs      # WP-413

crates/rules/src/character/  (Phase 5 additions)
├── creation/
│   ├── mod.rs
│   ├── streetrat.rs          # WP-501 Streetrat creation
│   ├── edgerunner.rs         # WP-502 Edgerunner creation
│   └── complete.rs           # WP-503 Complete Package creation
├── lifepath.rs               # WP-504 lifepath roller
├── cyberware.rs              # WP-505 install + Humanity Loss
├── cyberpsychosis.rs         # WP-506 cyberpsychosis state
├── therapy.rs                # WP-507 therapy mechanic
└── progression/
    ├── mod.rs
    ├── spend.rs              # WP-508 IP spending
    └── earn.rs               # WP-509 IP earning milestones

crates/rules/src/roles/  (Phase 5 additions)
├── mod.rs
├── combat_sense.rs           # WP-510 Solo
├── interface.rs              # WP-511 Netrunner
├── maker.rs                  # WP-512 Tech
├── medicine.rs               # WP-513 Medtech
├── credibility.rs            # WP-514 Media
├── backup.rs                 # WP-515 Lawman
├── resources.rs              # WP-516 Exec
├── operator.rs               # WP-517 Fixer
├── charismatic_impact.rs     # WP-518 Rockerboy
└── moto.rs                   # WP-519 Nomad
```
