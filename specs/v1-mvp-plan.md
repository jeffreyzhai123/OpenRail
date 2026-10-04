# Plan: V1 MVP roadmap (what's missing)

## Context
README §3 defines V1 as "a working, deployed, deterministic simulator proving the core loop end to end: inject a fault, watch it break the naive handler, share the exact failure as a link." This compares each V1 feature against `develop` at `43082a7`, with row 1 rechecked at `3fc6ff3` (PR #2).

**Bottom line:** about 2 of the 11 V1 features are done: the money ledger with invariants, and the ACH state machine. The engine is partly built: the RNG, clock and run hash are merged (PR #2), but the event queue, the handler trait and `run()` are still open. **Almost nothing that makes up the core loop exists yet**: handlers, scenarios, shrink, sweep, the API, the UI and the deploy. Fault injection has started (F1, not merged). Of those pieces, only faults and the UI have specs.

## Feature status (README §3 V1)
| # | V1 feature | Status | Spec |
|---|---|---|---|
| 1 | Virtual clock, seeded RNG, event queue, trace hashing | ✅ Done (E1–E6) | ✅ `deterministic-engine-plan.md` |
| 2 | `Money(i64)` ledger + balance invariants | ✅ Done (#1–#4 run; #5/#6 named, V4) | ✅ `ledger-plan.md` |
| 3 | ACH state machine | 🟡 State machine done (`rails/ach.rs`); D2 resolved to pull it into V1, so it still needs `Batched`/`Settled` `AchEvent`s and scenario 3 | ✅ |
| 4 | Naive vs hardened handler pair | ✅ Done (E5, S1b) | ✅ (S1b) |
| 5 | Fault injector: duplicate, reorder, delay, drop, crash-restart | 🟡 `apply_fault_plan()` done (F1, `4ac0f87` on `fault-injector`, not merged). Seed → plan generation (F2) and `run()` wiring (E6, F3) open | ✅ `fault-injector-plan.md` |
| 6 | Replay-by-seed links (basic URL encoding) | ❌ No `encode_run` / `decode_run` | ❌ |
| 7 | Shrinker, single-pass greedy | ❌ `shrink.rs` is empty | ❌ |
| 8 | Sweep harness (naive vs hardened failure rate) | ❌ | ❌ |
| 9 | 2–3 playable scenarios | ✅ Done: `charge-retry`, `refund-before-capture`, `late-ach-return`, each with a story plan, tested end to end through `run()` | ✅ `scenarios-plan.md` |
| 10 | Minimal UI: timeline, balances, invariants, Run/Shrink/Share | ❌ Step 0 scaffold only (a header in `App.tsx`) | ✅ `frontend-plan.md` (steps 1–8 open) |
| 11 | Deployed + smoke-tested against the real backend | ❌ No host chosen, no Dockerfile or config | ❌ |
| — | `sim-api` (Axum routes the UI calls) | ❌ `main.rs` is hello-world, and there are no deps (axum, tokio, serde) | ❌ (its contract lives in `frontend-plan.md`) |

## Gaps inside the existing specs
1. **`RunResult` is missing `opening` and `journal`.** `frontend-plan.md` ask #4 needs both for the timeline scrubber. `run()` consumes the `Ledger`, so sim-api can't recover them afterwards. → Now folded into `deterministic-engine-plan.md` as deviation 7 (piece E6), pending approval. Later, ask #3 also needs the effective `fault_plan`.
2. **The shrinker and sweep need a fresh handler for every run.** → Solved by `run()` taking a handler factory (`fault-injector-plan.md` decision C). `HandlerKind { Naive, Hardened }`, serialized as `"naive"`/`"hardened"`, lands with the trait in engine E5, so sim-api, the replay encoding and the sweep can name handlers early. `build() -> Box<dyn EventHandler>` lands with the handlers (S1b), and callers pass `&|| kind.build()`. The names match the frontend's `Handler` type and are what `encode_run` stores.
3. **Seeds do nothing until fault generation exists** (A-spec deviation 5). Until then, every seed gives the same run, so **the sweep is meaningless** and replay-by-seed is trivial. Seed → plan generation is on the critical path, not a polish item.

## Missing specs, and the decisions each one must make
**S1a: scenarios** (`sim-scenarios`) → done, see `scenarios-plan.md`. Each scenario carries a `story_plan` (decision P).
- Scenario 1 (charge retried after timeout), scenario 2 (refund before capture), and scenario 3 (ACH return after settlement — pulled into V1 per D2), as `Scenario { id, name, description, initial_ledger, workload }`. Openings must sum to zero (`Ledger::open`). Event ids are unique (`apply_fault_plan` targets them). Event times are simulated milliseconds (engine decision T), written with named constants such as `MS_PER_DAY`. Scenario 3 additionally needs `Batched`/`Settled` `AchEvent`s added to `rails/ach.rs` (today it only has `Returned`).
- `sim-scenarios` needs a `sim-core` dependency and a `scenarios()` registry, which sim-api's `GET /scenarios` uses (frontend ask #2).
- Each scenario's "naive fails, hardened passes" test waits for S1b. D2 and D3 are resolved (see Decisions below).

**S1b: handlers** (`handlers/naive.rs`, `hardened.rs`, `HandlerKind::build`). Needs engine E5.
- Naive: posts on every Captured or Refunded event, with no deduplication and no ordering checks.
- Hardened: deduplicates **durably, from `ledger.journal()`**: by event id, because a redelivered webhook has the same `EventId` as its journal entry's `source`, or by intent, for a retried capture. An in-memory set fails after a `CrashRestart`, which is the bug that fault exists to show (`fault-injector-plan.md`).
- **D4 (resolved, temporary): reject.** The hardened handler rejects a refund whose capture hasn't posted yet, and posts nothing. See Decisions below — this is explicitly not a settled design.

**S2: fault injection** (`fault.rs`) → now specced in `fault-injector-plan.md`. Decisions R, C and O are approved.
- Applying a plan is pure and draws no randomness, so shrinking never perturbs other faults. The seed only generates the initial plan.
- `Reorder { event_id, window }` reverses a window of deliveries (R). A crash-restart rebuilds the handler from a factory while the ledger survives (C). Ops apply in fixed phases (O).
- The hardened handler therefore has to derive idempotency from `ledger.journal()`, which is the reason the trait passes it `&Ledger`.

**S3: shrink + sweep** (`shrink.rs`, `simulator.rs`)
- Greedy single pass: try removing each `FaultOp` once, and keep the removal if the *same named* invariant still fails. Report `candidates_tried`. Never say "minimal" (§6.3).
- Sweep: for seeds `start..start+count` × {naive, hardened}, count the runs with any failed invariant. Cap `count` with a named constant.

**S4: sim-api + replay encoding** (`sim-api`)
- Routes: `POST /run`, `GET /replay/:encoded`, `POST /shrink`, `POST /sweep`, plus `GET /scenarios`. Use the error envelope and the DTOs from `frontend-plan.md`, with the seed as a `u32` JSON number (engine decision W).
- `encode_run` in V1 is "basic URL encoding" (README §3): versioned JSON → base64url, **without compression** (compression is the §6.2 / V2 target). That keeps the deps to axum, tokio, serde, tower-http (CORS) and a base64 implementation.
- CLAUDE.md's network-failure rule applies only here: body size limit, request timeout, sweep and shrink caps, and turning `SimError` into a 4xx/5xx response.
- The golden-fixture test (frontend ask #7).

**S5: deploy** (README: "pick one API host early")
- Fly.io or Railway for the API, plus static hosting for `frontend/`. Then the smoke test from `frontend-plan.md` (walkthrough with `VITE_SIM_CLIENT=http`).

## Critical path and parallel tracks
**Critical path:** E3 + E5 → E6 (also needs F1) → F3 (also needs F2) → S3 → S4 → frontend step 8 → S5.

| Work | Needs | Can start |
|---|---|---|
| E3 queue, E5 trait and `HandlerKind` names | — | Now |
| F2 seed → plan | F1 | Now (F1 is on `fault-injector`) |
| S1a scenarios, CI, frontend steps 1–7, replay-encoding spec | — | Now |
| E6 `run()` | E3, E5, F1 | After E3 and E5 |
| S1b handlers | E5 | After E5 |
| F3 | E6, F2 | After E6 |
| S3 shrink + sweep | F3 | After F3. The sweep's chart only means something once S1b exists. |
| S4 sim-api | S3, S1a, `HandlerKind::build` (S1b) | After S3 |
| Frontend step 8, golden fixtures, V1 acceptance | S4, S1b | Last |
| S5 deploy | S4, D1 | Last |

- **Track A (engine owner):** E5 first (it unblocks S1b), E3, E6, then F2/F3, then S3.
- **Track B:** S1a now, S1b once E5 lands, then S4.
- **Frontend:** steps 1–7 now, in parallel. Step 8 waits for S4.

## Decisions (resolved 2026-10-03)
Full rationale for each is in `specs/decisions-log.md`; summary here for the table/section references above.
- **D1 — API host: Fly.io.** Picked per README's "pick one API host early, don't relitigate." Low-stakes; revisit only on a real deployment blocker.
- **D2 — pull ACH into V1: yes.** The ACH state machine is already built and tested; shipping V1 without any scenario exercising it undersells the README's own headline example ("late ACH returns"). Scenario 3 moves into V1 (needs `Batched`/`Settled` `AchEvent`s, S1a).
- **D3 — confirmed as written.** Scenario 1 models a provider-side `Duplicate`, not a client idempotency-key retry; the scenario description says so explicitly, since V4 is where idempotency keys get their own model.
- **D4 — hardened handler rejects a refund that arrives before its capture, *for now*.** **This is a temporary resort, not a settled design.** Buffering would add handler-side mutable state whose own correctness isn't tested by anything yet — another unverified assumption on top of the exact class of bug the fault injector exists to surface — while rejecting is simpler and has an obvious failure signature. But neither choice is caught by any V1 invariant (both lose the refund across a crash), and the real fix is V4's reconciliation invariants (#5/#6) being able to judge whether a buffered-then-applied refund was handled correctly. **D4 needs a real discussion once V4 lands** — don't let "reject" calcify into the permanent answer just because it shipped first.

## Housekeeping (small, but blocks "done")
- `main` on GitHub still exists (it's the default branch) and is behind `develop`.
- No CI. README only requires it in V2, but CLAUDE.md calls the determinism check "the one test that must never go yellow", and today it only runs locally. A GitHub Actions job running the four cargo commands is cheap insurance.

## Verification (of this roadmap)
- Each ❌ above was checked against the tree at `43082a7`, and rechecked at `3fc6ff3`, where PR #2 touched them only with E0's whitespace: empty files by size, `unimplemented!()` in `simulator.rs`, empty `[dependencies]` in `sim-api` and `sim-scenarios`, and no `.github/`, `Dockerfile` or `fly.toml`.
- V1 is done when README §3's definition holds on the deployed URL: run scenario 1 under naive → `single_capture_per_intent` goes red. Under hardened → all green. Share → the link opens with "verified identical". Shrink → a smaller plan. Sweep → a chart where naive fails more often than hardened.
