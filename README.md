# Rails Sim Playground: Technical Guide

**One line:** a deterministic, in-browser payments simulator. Inject failures (timeouts, duplicate or out-of-order webhooks, late ACH returns), watch a ledger break or hold, automatically **shrink** the failure to a smaller reproduction, and share it as a **replay-by-seed link**.

*This doc is the north star for development — self-contained, meant to be handed to a new contributor or a fresh AI session with no other context.*

---

## 1. Why we're building this

**Goals:** a portfolio project that differentiates us for infra/backend/distributed-systems interviews (payments and AI infra are preferences), is genuinely showcase-able and shareable, and builds real skill in correctness-under-concurrency, fault injection, and deterministic testing.

**Pain point:** payment bugs are rare, timing-dependent, and hard to reproduce — providers explicitly warn that webhook events can arrive duplicated and out of order, and ACH returns can land days after the original transaction, long after handler code has moved on. A naive handler can be wrong on a large fraction of realistic event orderings. Vendor sandboxes exist but are proprietary and hard to make misbehave on demand. Nothing we found combines a visual, vendor-neutral playground with replay-by-seed links, domain-specific failure shrinking, and deterministically-reproducible concurrency — that combination is the differentiator, not any single piece.

**Non-goals:** card networks, real provider integrations, authentication, real money movement, compliance claims, or a general-purpose simulation framework. This is a simulator — no real money moves, no compliance claims are made.

---

## 2. Tech stack

**Backend — Rust, three-crate workspace:**

```
rails-sim/
├── crates/
│   ├── sim-core/         # deterministic engine — zero async, zero I/O, zero Tokio
│   ├── sim-scenarios/     # scenario data, depends only on sim-core's types
│   └── sim-api/           # Axum HTTP server — the ONLY crate with async/Tokio
└── frontend/               # React/TS, talks to sim-api over HTTP
```

| Layer | Choice | Notes |
|---|---|---|
| Core engine | Rust, no external async runtime | Must stay sync/deterministic — see §5 |
| RNG | Hand-rolled seeded PRNG (mulberry32-style) | Not the `rand` crate — full control over the algorithm matters for the determinism guarantee |
| Serialization | `serde` / `serde_json` | `FaultPlan` and replay payloads must round-trip exactly |
| Hashing | `blake3` (or similar) | Trace hashing for determinism verification |
| Property testing | `proptest` | Invariant checks over randomized valid inputs |
| Concurrency verification (V3 stretch) | `loom` | Exhaustive interleaving model-checking on any real concurrent primitive — see §5.3 |
| HTTP server | Axum on Tokio | Confined entirely to `sim-api` |
| Frontend | React + TypeScript | Talks to `sim-api` over a stateless HTTP API |
| Deploy | Fly.io or Railway (API) + static hosting (frontend) | Pick one API host early, don't relitigate later |
| CI | `cargo test`, `cargo clippy`, `cargo fmt --check`, determinism check | See V2/V3 scope below |

**API design:** `sim-api` is **stateless per request** — client sends `scenario_id` + `seed` + `fault_plan`, server computes and returns the full trace, nothing persisted server-side. This keeps determinism trivially provable (same input → same output, no hidden server state) and is the default unless V3's control plane proves it needs persistence.

---

## 3. Version plan

### V1 — MVP

A working, deployed, deterministic simulator proving the core loop end to end: inject a fault, watch it break the naive handler, share the exact failure as a link.

**Features:**
- Virtual clock, seeded RNG, event queue (deterministic tie-break), trace hashing
- `Money(i64)` ledger (cents, checked arithmetic, no floats) with balance invariant checks
- ACH state machine (initiated → batched → settled → returned)
- Naive vs. hardened handler pair
- Fault injector: duplicate, reorder, delay, drop, crash-restart
- Replay-by-seed links (basic URL encoding)
- Shrinker: **single-pass greedy** version only (try removing each fault once, keep if failure persists) — not full ddmin yet
- Sweep harness (naive vs. hardened failure-rate chart)
- 2–3 playable scenarios, minimal UI (timeline, balances, invariant panel, Run/Shrink/Share)
- Deployed and smoke-tested against the real backend

### V2 — Complete (Tier 0–2 hardened)

Everything V1 rushed, done properly — no new architecture, just correctness and polish.

**Features:**
- Full property-based test coverage across all 6 invariants (§6.4)
- **10,000-seed determinism check in CI** — same seed must always produce an identical trace hash
- Full recursive **ddmin shrinker** (§6.3), replacing the greedy V1 version, with honest real-number reporting (no placeholder ratios)
- Third scenario, ACH details re-verified against Nacha documentation
- Scenario gallery as real linkable pages; self-explaining failure pages (what broke, why, in 1–2 lines, no narration required)
- CLI wrapper for running sweeps outside the browser
- CI: lint, test, determinism check, deploy on merge
- Feedback from real payments engineers incorporated before V3 scope is locked

### V3 — Full distributed-systems enhancement

Where the project becomes a genuine distributed-systems showcase on top of the correctness demo. Every feature below must preserve **100% reproducible seeds** — see §5 for why this constrains *how* each one is built.

**Features (roughly dependency-ordered):**
- Message queue abstraction (refactor of the V1/V2 event queue into a proper interface)
- Deterministically-scheduled worker pool (§5) — real concurrency, still seed-reproducible
- Semaphore-bounded concurrency (modeled as a counter inside the deterministic scheduler, not a real async primitive)
- Retry with backoff + jitter
- Circuit breaker
- Dead-letter queue (DLQ)
- Backpressure + rate limiting
- Lightweight control plane (owns scenario setup, fault scheduling, invariant checks, replay API — separate from the worker-pool data plane)
- Leader election + distributed lock (modeled as scheduler state — an expiry tick, revocable by a fault op — not a real lock service)
- Sharding + cross-shard transfers via saga pattern (the largest, highest-signal item — touches the ledger core directly, must preserve all 6 invariants across shard boundaries)
- Full CI/CD pipeline (build, test, determinism check, deploy, all automated)
- Optional stretch additions: `loom` model-checking on any real concurrent primitive; a separate, clearly-labeled real-concurrency stress-test mode for throughput/latency numbers (§5.3)

**V3 definition of done:** the 10,000-seed determinism check still passes with the worker pool enabled (proof that "real concurrency, still reproducible" holds); at minimum the queue, worker pool, retry/circuit-breaker/DLQ, and backpressure items are shipped; sharding+saga is shipped or explicitly documented as a near-term follow-up; CI/CD is live.

---

## 4. Tech stack rationale note

`sim-core` never depends on Tokio or does any real I/O — it's a plain synchronous library, testable in milliseconds, reusable from a CLI, the API, and potentially WASM later. All real async lives only in `sim-api`. This isn't just tidiness: it's the mechanism that makes the determinism guarantee in §5 possible at all.

---

## 5. Core architectural rule: simulate the scheduler, don't spawn real concurrency

### 5.1 The problem

Real threads racing on the ledger break replay-by-seed, because OS thread scheduling isn't seedable. Tokio doesn't solve this either — **Tokio's task-polling order is not guaranteed deterministic across runs**, even on a single-threaded runtime; `tokio::time::pause()` controls virtual time, not polling order. So neither `std::thread` nor `tokio::spawn` can back the simulation core if seeds must be 100% reproducible.

### 5.2 The fix

**"Workers" are plain state machines stepped by a single-threaded, synchronous scheduler loop inside `sim-core`.** No threads, no async, no runtime. The seed controls which ready worker advances next at each step — this produces real races, lost updates, and out-of-order writes, all still 100% reproducible from a seed. (Same technique used by FoundationDB's simulation testing and libraries like madsim/turmoil.)

This is the single most load-bearing architectural decision in the project — everything in V3 depends on it.

### 5.3 Why this isn't a compromise — and two additions worth making later

**Concurrency ≠ parallelism.** Concurrency is reasoning correctly over multiple possible orderings of operations; parallelism is physically executing them at the same time. Rails Sim needs the former and gets no value from the latter — a deterministic scheduler doesn't decide *whether* two operations race, only *in what order*, and a seed just picks that order reproducibly instead of leaving it to luck. This can surface more real bugs than genuine thread concurrency, since a real race's bad-interleaving window is often nanoseconds wide and only gets hit by chance, while deterministic simulation can force that exact interleaving on command. **Precedent worth citing directly:** TigerBeetle, a real production financial ledger, is built and validated almost entirely around this same deterministic-simulation approach, for exactly this reason.

Real concurrency still has a place — strictly as an addition on top of the deterministic core, never a replacement (replacing it breaks replay-by-seed and the shrinker outright, since the shrinker works by re-running a candidate and checking "does it still fail," which is meaningless without reproducibility):

1. **`loom`** — if any piece of the worker pool is ever wrapped in a genuine concurrent primitive (e.g. a real `Arc<Mutex<_>>`), run `loom` (Tokio's own verification tool) against it. It exhaustively model-checks all interleavings of real concurrent code — a different, complementary guarantee to seed-directed simulation. V3 stretch item.
2. **A separate, clearly-labeled stress-test mode** using genuine async/thread concurrency, for throughput/latency numbers only. Must be a distinct mode/binary, not a flag on the main simulator, and kept clearly apart from the reproducible-correctness demo — one measures performance, the other proves correctness; don't let them blur into one "sometimes deterministic" system.

---

## 6. Technical design reference

### 6.1 Core type contracts (Rust)

```rust
// sim-core/src/event.rs
struct SimEvent { id: EventId, time: u64, seq: u64, kind: EventKind, payload: serde_json::Value }
// EventQueue ordered strictly on (time, seq) — never on payload contents

// sim-core/src/money.rs
struct Money(i64); // cents. Checked Add/Sub. No From<f64>, ever.

// sim-core/src/fault.rs
#[derive(Serialize, Deserialize)]
enum FaultOp {
    Duplicate { event_id: EventId },
    Reorder { window: usize },
    Delay { event_id: EventId, by: u64 },
    Drop { event_id: EventId },
    CrashRestart { at: u64 },
}
type FaultPlan = Vec<FaultOp>; // must survive being encoded into a replay URL and fed through the shrinker

// sim-scenarios/src/lib.rs
struct Scenario { id: &'static str, name: &'static str, description: &'static str, initial_ledger: Vec<(String, i64)>, workload: Vec<SimEvent> }

// sim-core/src/simulator.rs — the main entrypoint everything else calls
struct RunResult { trace: Vec<SimEvent>, ledger: LedgerSnapshot, invariants: Vec<InvariantResult>, trace_hash: String }
fn run(scenario: &Scenario, seed: u64, fault_plan: Option<&FaultPlan>) -> RunResult;
```

`InvariantResult` needs a name/identity, not just pass/fail — the shrinker's stopping condition is "the *same named* invariant still fails," not just "something failed."

### 6.2 Replay-by-seed link design

- A run is fully described by `scenario_id` + `seed` + an **explicit `FaultPlan`** (the seed generates an initial plan, but after that the plan is stored as data, so shrinking can remove individual faults without perturbing unrelated randomness).
- Encoding: `encode_run(scenario_id, seed, fault_plan) -> String` — JSON → compress → base64url, held in the URL fragment (no backend storage needed, given the stateless API design in §2).
- Each run carries a **trace hash**; a replay recomputes it and shows a "verified identical" badge, or flags a determinism break.
- Version the encoding scheme so old links keep working, or warn clearly when they can't.
- Known risk: long fault plans make long URLs — use compression, a short-plan cap, and a file-export fallback if needed.

### 6.3 Shrinker (ddmin) design and honest limits

**Goal:** reduce a failing fault plan to a smaller one that still triggers the *same named* violation. Demo numbers must be real, not promised ratios.

**Full algorithm (V2 target, delta-debugging / ddmin):**
1. Predicate: re-run a candidate `FaultPlan`; "still fails" only if the *same named* invariant is violated as the original.
2. Remove faults: try dropping chunks, then individual `FaultOp`s.
3. Simplify what's left: shorten delays, reduce duplicate counts, simplify amounts.
4. Stop when no single remaining fault can be removed without the failure disappearing.

**V1 simplification:** single-pass greedy — try removing each fault once, keep the removal if the failure persists. Not full ddmin; don't claim "minimal" for this version.

**Honest limits:** ddmin guarantees **1-minimal** (removing any one remaining fault makes the test pass), not globally minimal. Each candidate is a full re-run — cap candidate runs (~500) and show progress in the UI. Removing one event can change downstream behavior, which is exactly why the fault plan is explicit data rather than re-derived from the seed each time.

### 6.4 The six ledger invariants

1. The ledger always balances (sum of entries is zero)
2. No money is created or destroyed by a fault
3. At most one captured charge per business intent (idempotency)
4. Refunds never exceed captured amount
5. After reconciliation, ledger state converges with the provider's final state
6. No ledger entry exists without a corresponding provider event

### 6.5 File-by-file map

```
sim-core/src/
  clock.rs        VirtualClock: now(), advance_to(t)
  rng.rs          seeded PRNG (hand-rolled, mulberry32-style)
  money.rs        Money(i64) newtype, checked arithmetic
  event.rs        SimEvent, EventQueue (BinaryHeap ordered on (time, seq))
  ledger.rs       Ledger { accounts: HashMap<String, Money> }, post() rejects unbalanced entries
  invariants.rs   InvariantCheck trait; the 6 invariants in §6.4
  ach.rs          AchState enum + transition()
  handler.rs      EventHandler trait
  handlers/naive.rs, handlers/hardened.rs
  fault.rs        FaultOp, FaultPlan, apply_fault_plan()
  trace.rs        hash_trace() — canonical JSON then stable hash
  simulator.rs    Simulation::run(), Simulation::sweep()
  shrink.rs       shrink() — ddmin, see §6.3

sim-scenarios/src/
  lib.rs                      Scenario struct
  scenario1_retry.rs           charge retried after timeout
  scenario2_refund_order.rs    refund event arrives before capture
  scenario3_late_return.rs     ACH return arrives after settlement (V2+)

sim-api/src/
  main.rs            Axum app — the ONLY crate with Tokio
  routes/run.rs       POST /run
  routes/replay.rs    GET /replay/:encoded
  routes/shrink.rs    POST /shrink
  routes/sweep.rs     POST /sweep
  encode.rs           encode_run() / decode_run(), see §6.2

frontend/               React/TS, calls sim-api over HTTP
  Timeline, BalancePanel, InvariantPanel, Controls, ShrinkView, SweepChart, Gallery
```

### 6.6 Concurrency design summary (see §5 for full rationale)

- `sim-core` has zero threads, zero async, zero Tokio — V3 "workers" are plain state machines stepped by a single-threaded scheduler, seed-controlled interleaving.
- Semaphores/leases (V3 backpressure, leader election) are modeled as plain counters/state inside that scheduler — never `tokio::sync::Semaphore` or a real lock service.
- `loom` and a real-concurrency stress-test mode are legitimate additions, strictly layered on top of the deterministic core, never replacing it.
- Precedent: TigerBeetle uses this same deterministic-simulation approach for a real production financial ledger.

---

*This is a simulator. It moves no real money and makes no claim about compliance or real network behavior.*