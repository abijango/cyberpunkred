# Agent Onboarding — Cyberpunk Red CRPG

You are working on a Rust workspace implementing a *Cyberpunk RED* solo CRPG. There are many other agents working in parallel, each on a discrete Work Package.

## Read these in order

1. `IMPLEMENTATION_PLAN.md` §0–§3. Skim. This is your map.
2. `IMPLEMENTATION_PLAN.md` §5 (Coordination Protocols). Read carefully. This is how you avoid stepping on other agents.
3. The Work Package you've been assigned, in §4. Read in full.
4. Every WP your assigned WP `Depends on:`. Read **only their public API** unless you have a coordination need.
5. The rulebook page references in your WP. The PDF is developer-provided (not checked in) at `rulebook/Cyberpunk_Red_Core-Digital_v1.25.pdf` (workspace root). Read the cited pages directly with the `Read` tool's `pages:` parameter; don't skim — Cyberpunk Red has subtle interactions. The page reference is the spec.

## Working rules

- One WP per branch. Branch: `wp-XXX-short-description`.
- Commit prefix: `[WP-XXX]`.
- The public API in your WP is a contract. If you have to deviate, document why in your PR.
- Tests pass before you push: `cargo fmt --check && cargo clippy -- -D warnings && cargo test --workspace`.
- WASM build passes if you touched a shared crate: `wasm-pack build crates/web --target web`.
- Doc comments on every public item, citing rulebook pages where applicable.
- `#![forbid(unsafe_code)]` in `rules` and `gm`. Don't add `unsafe` anywhere.
- No `tokio::main`, no `thread_rng`, no `std::time::Instant::now()` in `rules` or `gm`.

## Spawning sub-agents

If you delegate work via the Agent tool, **always** set `model: "sonnet"` explicitly. Do not inherit the orchestrator's model and do not use `opus` or `haiku`. This is a fixed project decision (see `IMPLEMENTATION_PLAN.md` §0.2) — apply it to every Agent call you make.

## Determinism

The single RNG type is `cpr_rules::Rng = rand_chacha::ChaCha20Rng`. Every dice-rolling function takes `rng: &mut Rng` as the last parameter. Tests use explicit seeds. The replay tool reproduces any game from its seed and action log.

## When you don't know

- **Rulebook ambiguity:** default to RAW; comment the tension; flag in PR.
- **Public API conflict with another WP:** open a coordination issue; don't push.
- **Acceptance criterion seems wrong:** flag in PR; propose the corrected version.
- **You're stuck:** stop. Don't speculate-implement. Ask.

## What's already decided

§0.2 of the plan lists every design decision the user has already made. Treat them as fixed. They are not hypotheses.

## File layout

- Your code goes where the WP says (the `Module:` field).
- Tests go in the same file inside `#[cfg(test)] mod tests { ... }` unless the WP says otherwise.
- Fixtures go in `crates/<crate>/tests/fixtures/`.
- Authored content (RON) goes under `content/`.

## Hand-off

When you are done:

1. All acceptance tests in your WP pass.
2. Your PR is open with title `[WP-XXX] Title`, description containing rulebook pages and any API deviations.
3. CI is green.

Then stop. Don't start the next WP — there's an assignment process.
