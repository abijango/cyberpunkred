# Project Status — Cyberpunk Red CRPG

**Snapshot:** 2026-05-07. Use `git log --oneline main` for the authoritative progress record; this document summarises state at the time of writing.

## Phase progress

| Phase | Theme | WPs | Status |
|---|---|---|---|
| 0 | Foundation (workspace, core types, RNG, effect skeleton) | 7 | ✅ complete |
| 1 | Core rules mechanics (dice, checks, derivation, wound states) | 7 | ✅ complete |
| 2 | Data catalogs (weapons, armor, cyberware, ICE, etc.) | 14 | ✅ complete |
| 3 | Combat subsystems (initiative, attacks, damage, criticals) | 16 | ✅ complete |
| 4 | Netrunning (architecture, abilities, ICE, demons) | 17 | ✅ complete |
| 5 | Character & progression (creation, lifepath, role abilities, IP) | ~19 | not started |
| 6 | GM layer (Beat Charts, NPCs, campaign log) | ~13 | not started |
| 7 | LLM layer (provider trait, prompts) | ~10 | not started |
| 8 | Frontend (Leptos UI) | ~15 | not started |
| 9 | Backend (Axum endpoints) | ~8 | not started |
| 10 | Integration (sample gig, smoke tests, perf, docs) | ~4 | not started |

**Test count:** 494 passing in `cpr_rules`. Workspace gates (`cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace && wasm-pack build crates/web --target web`) all green.

**Source size:** 68 .rs files, ~35.9k lines in `crates/rules/src/`. 43 content files (~412 KB) under `content/`.

## What's next: Phase 5 — Character & Progression

Phase 5 is the next major unlock. Per `IMPLEMENTATION_PLAN.md` §6 entry gates:
- Phase 5 needs Phase 1 (✅) + WP-201 skills (✅) + WP-204 cyberware (✅) + WP-213 roles (✅) + WP-214 lifepath (✅). **All satisfied.**

Phase 5 WPs (per `IMPLEMENTATION_PLAN.md` §4, around line 2552 onwards):
- **WP-501** Streetrat character creation (Medium)
- **WP-502** Edgerunner character creation (Medium)
- **WP-503** Complete Package character creation (Large)
- **WP-504** Lifepath roller (Medium)
- **WP-505** Cyberware install + Humanity Loss (Medium)
- **WP-506** Cyberpsychosis state (Small)
- **WP-507** Therapy mechanic (Medium)
- **WP-508** Improvement Point spending (Medium)
- **WP-509** IP earning (objective milestones) (Small)
- **WP-510** Role Ability: Combat Sense (Solo) (Small)
- **WP-511** Role Ability: Interface (Netrunner) (Small)
- **WP-512** Role Ability: Maker (Tech) (Medium)
- **WP-513** Role Ability: Medicine (Medtech) (Medium)
- **WP-514** Role Ability: Credibility (Media) (Small)
- **WP-515** Role Ability: Backup (Lawman) (Small)
- **WP-516** Role Ability: Resources (Exec) (Small)
- **WP-517** Role Ability: Operator (Fixer) (Small)
- **WP-518** Role Ability: Moto (Nomad) (Small)
- **WP-519** Role Ability: Charismatic Impact (Rockerboy) (Small)

**Phase 5 internal dependency**: WP-501 (Streetrat) blocks WP-502, WP-503, WP-504. Most Role Ability WPs are independent and can run in parallel with creation.

After Phase 5, Phase 6 (GM layer), Phase 7 (LLM), Phase 8 (Frontend), Phase 9 (Backend) become viable. Phase 10 (sample gig demo) is the critical-path endpoint per `IMPLEMENTATION_PLAN.md` §3.

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

## Open follow-ups / known debt

These are tracked in commit messages and PR descriptions but worth surfacing here:

### Coordination follow-ups
- **`DiceSpec` is defined three times** locally in WP-202 weapons, WP-204 cyberware, WP-208 programs (and a fourth time in WP-209 Black ICE). All same shape `{ n: u8, die: DieKind }`. Worth a one-shot consolidation PR to a single shared type in `rules::dice`.
- **`WrongProgramClass` aliases `ProgramWrongClass`** — both variants exist on `RulesError` with `{program, expected, got}` fields. Consolidate to one.
- **`NetrunNotActive` aliases `NoActiveNetrun`** — same. Consolidate.

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
```
