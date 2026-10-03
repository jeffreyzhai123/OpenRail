# Plan: V1 MVP roadmap (what's missing)

## Context
README §3 defines V1 as "a working, deployed, deterministic simulator proving the core loop end to end: inject a fault, watch it break the naive handler, share the exact failure as a link." This compares each V1 feature against `develop` at `43082a7`.

**Bottom line:** about 2 of the 11 V1 features are done: the money ledger with invariants, and the ACH state machine. The engine has a spec but no code. **Nothing that makes up the core loop exists yet**: handlers, faults, scenarios, shrink, sweep, the API, the UI and the deploy. Five of those pieces don't have a spec either.

## Feature status (README §3 V1)
| # | V1 feature | Status | Spec |
|---|---|---|---|
| 1 | Virtual clock, seeded RNG, event queue, trace hashing | ❌ Empty files / `unimplemented!()` | ✅ `deterministic-engine-plan.md` |
| 2 | `Money(i64)` ledger + balance invariants | ✅ Done (#1–#4 run; #5/#6 named, V4) | ✅ `ledger-plan.md` |
| 3 | ACH state machine | ✅ Done (`rails/ach.rs`), but **no V1 scenario uses it** (see D2) | ✅ |
| 4 | Naive vs hardened handler pair | ❌ `handlers/*.rs` are empty (the trait goes in `handlers/mod.rs`, defined in the A spec's E5) | ❌ |
| 5 | Fault injector: duplicate, reorder, delay, drop, crash-restart | ❌ `FaultOp` is data only, there's no `apply_fault_plan()`, and no seed → plan generation | ❌ |
| 6 | Replay-by-seed links (basic URL encoding) | ❌ No `encode_run` / `decode_run` | ❌ |
| 7 | Shrinker, single-pass greedy | ❌ `shrink.rs` is empty | ❌ |
| 8 | Sweep harness (naive vs hardened failure rate) | ❌ | ❌ |
| 9 | 2–3 playable scenarios | ❌ `sim-scenarios` is the `add()` template and doesn't even depend on `sim-core` | ❌ |
| 10 | Minimal UI: timeline, balances, invariants, Run/Shrink/Share | ❌ Step 0 scaffold only (a header in `App.tsx`) | ✅ `frontend-plan.md` (steps 1–8 open) |
| 11 | Deployed + smoke-tested against the real backend | ❌ No host chosen, no Dockerfile or config | ❌ |
| — | `sim-api` (Axum routes the UI calls) | ❌ `main.rs` is hello-world, and there are no deps (axum, tokio, serde) | ❌ (its contract lives in `frontend-plan.md`) |

## Gaps inside the existing specs
1. **`RunResult` is missing `opening` and `journal`.** `frontend-plan.md` ask #4 needs both for the timeline scrubber. `run()` consumes the `Ledger`, so sim-api can't recover them afterwards. → Now folded into `deterministic-engine-plan.md` as deviation 7 (piece E6), pending approval. Later, ask #3 also needs the effective `fault_plan`.
2. **The shrinker and sweep need a fresh handler for every run.** `run()` takes `&mut dyn EventHandler`, but a shrink or sweep runs many times. → Add a `HandlerKind { Naive, Hardened }` enum with `build() -> Box<dyn EventHandler>`, serialized as `"naive"`/`"hardened"`. That matches the frontend's `Handler` type and is what `encode_run` stores. It belongs in the handlers spec, and `run()`'s signature stays as it is.
3. **Seeds do nothing until fault generation exists** (A-spec deviation 5). Until then, every seed gives the same run, so **the sweep is meaningless** and replay-by-seed is trivial. Seed → plan generation is on the critical path, not a polish item.

## Missing specs, and the decisions each one must make
**S1: handlers + scenarios** (`handlers/naive.rs`, `hardened.rs`, `sim-scenarios`)
- Naive: posts on every Captured or Refunded event, with no deduplication and no ordering checks.
- Hardened: deduplicates by intent, and decides what to do with a refund that arrives before its capture: buffer it until the capture arrives, or reject it and post nothing.
- Scenario 1 (charge retried after timeout) and scenario 2 (refund before capture), as `Scenario { id, name, description, initial_ledger, workload }`. Openings must sum to zero (`Ledger::open`).
- `sim-scenarios` needs a `sim-core` dependency and a `scenarios()` registry, which sim-api's `GET /scenarios` uses (frontend ask #2).

**S2: fault injection** (`fault.rs`)
- What each op does to the queue: `Duplicate` (when the copy arrives), `Reorder { window }` (an `Rng::shuffle` over the next *n* events), `Delay`, `Drop`, `CrashRestart { at }`.
- **Crash-restart semantics.** Proposed: the ledger survives (it's the durable store) and the handler is rebuilt via `HandlerKind::build()`, so any in-memory deduplication is lost. The hardened handler therefore has to derive idempotency from `ledger.journal()`, which is the reason the trait passes it `&Ledger`.
- **Seed → initial `FaultPlan`** (§6.2): which ops, which events, and the rates as named constants. After generation the plan is plain data (`RunResult.fault_plan`), so shrinking never consumes RNG.

**S3: shrink + sweep** (`shrink.rs`, `simulator.rs`)
- Greedy single pass: try removing each `FaultOp` once, and keep the removal if the *same named* invariant still fails. Report `candidates_tried`. Never say "minimal" (§6.3).
- Sweep: for seeds `start..start+count` × {naive, hardened}, count the runs with any failed invariant. Cap `count` with a named constant.

**S4: sim-api + replay encoding** (`sim-api`)
- Routes: `POST /run`, `GET /replay/:encoded`, `POST /shrink`, `POST /sweep`, plus `GET /scenarios`. Use the error envelope and the DTOs from `frontend-plan.md`, with the seed as a decimal string.
- `encode_run` in V1 is "basic URL encoding" (README §3): versioned JSON → base64url, **without compression** (compression is the §6.2 / V2 target). That keeps the deps to axum, tokio, serde, tower-http (CORS) and a base64 implementation.
- CLAUDE.md's network-failure rule applies only here: body size limit, request timeout, sweep and shrink caps, and turning `SimError` into a 4xx/5xx response.
- The golden-fixture test (frontend ask #7).

**S5: deploy** (README: "pick one API host early")
- Fly.io or Railway for the API, plus static hosting for `frontend/`. Then the smoke test from `frontend-plan.md` (walkthrough with `VITE_SIM_CLIENT=http`).

## Critical path and parallel tracks
```
Engine (A spec) ──► S2 faults + seed gen ──► S3 shrink/sweep ──┐
S1 handlers + scenarios (needs only the trait + Ledger) ──────┼──► S4 sim-api ──► frontend step 8 ──► S5 deploy
Frontend steps 1–7 (against fixtures, can start now) ─────────┘
```
- **Track A (engine owner):** the A spec, then S2, then S3.
- **Track B (now free, since B's work is done):** S1 now (handlers are testable without the simulator, by design), then S4.
- **Frontend:** steps 1–7 now, in parallel. Step 8 waits for S4.

## Open decisions (for you / your partner)
- **D1:** Fly.io or Railway (S5). The README says to decide early.
- **D2:** V1 ships 2 card scenarios, and scenario 3 (late ACH return) is marked V2+, so the finished ACH state machine has no consumer in V1. Accept that, or pull scenario 3 into V1 and add the Batched/Settled `AchEvent`s?
- **D3:** Model scenario 1's "retried after timeout" as a provider `Duplicate` of the Captured webhook. V4 is where client-side idempotency-key retries get their own model, so say so in the scenario description.
- **D4:** The hardened handler's policy for a refund that arrives before its capture: buffer it or reject it (S1).

## Housekeeping (small, but blocks "done")
- `cargo fmt --check` is red on `develop` (A-spec E0).
- TODO.md: tick Step 0. README §6.1 still describes the old `EventKind` and the `ach.rs` path (A-spec E7). §6.5 was fixed in `8f711df`.
- `main` on GitHub still exists (it's the default branch) and is behind `develop`.
- No CI. README only requires it in V2, but CLAUDE.md calls the determinism check "the one test that must never go yellow", and today it only runs locally. A GitHub Actions job running the four cargo commands is cheap insurance.

## Verification (of this roadmap)
- Each ❌ above was checked against the tree at `43082a7`: empty files by size, `unimplemented!()` in `simulator.rs`, empty `[dependencies]` in `sim-api` and `sim-scenarios`, and no `.github/`, `Dockerfile` or `fly.toml`.
- V1 is done when README §3's definition holds on the deployed URL: run scenario 1 under naive → `single_capture_per_intent` goes red. Under hardened → all green. Share → the link opens with "verified identical". Shrink → a smaller plan. Sweep → a chart where naive fails more often than hardened.
