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
- 3 playable scenarios — 2 card, 1 ACH (late return after settlement, `decisions-log.md` D2) — minimal UI (timeline, balances, invariant panel, Run/Shrink/Share)
- Deployed and smoke-tested against the real backend

### V2 — Complete (Tier 0–2 hardened)

Everything V1 rushed, done properly — no new architecture, just correctness and polish.

**Features:**
- Full property-based test coverage across all invariants (§6.4)
- **10,000-seed determinism check in CI** — same seed must always produce an identical trace hash
- Full recursive **ddmin shrinker** (§6.3), replacing the greedy V1 version, with honest real-number reporting (no placeholder ratios)
- ACH details re-verified against Nacha documentation (scenario 3 itself ships in V1, `decisions-log.md` D2)
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

### V4 — Provider event feed, reconciliation, and idempotency-key semantics

V3 hardens *how the simulator executes* (concurrency, distribution). V4 hardens *what the simulator is honest about not yet modeling*: invariants #5 and #6 (README §6.4) are currently named but unimplemented, because nothing in the system represents "the provider's view of truth" separately from the ledger. This tier builds that missing side of the boundary.

**Features:**
- **`ProviderEvent` feed** — a second, append-only event stream representing what the (simulated) provider believes happened, independent of what the handler wrote to the ledger. Lives in `sim-core`, generated alongside `SimEvent`s from the same seed, so it stays reproducible; fault ops can act on it too (e.g. a `Drop` can make a `SimEvent` arrive at the handler while the matching `ProviderEvent` still lands in the feed, which is exactly the shape of invariant #6).
- **Invariant #5 (`RECONCILES_WITH_PROVIDER`)**: after a run, replay the provider feed into its own view of expected balances and diff against `Ledger.balances()`. Needs a reconciliation key (reuse `IntentId`) to match a ledger entry to its provider event.
- **Invariant #6 (`ENTRY_HAS_PROVIDER_EVENT`)**: every `JournalEntry` must trace back to a `ProviderEvent` that actually occurred in the feed — catches a handler that posts a capture the provider never confirmed (e.g. acted on a dropped/delayed event as if it had arrived).
- **Idempotency-key-aware request handling (`sim-api`)** — distinct from the existing `Duplicate` fault op. `Duplicate` simulates the *provider* redelivering the same webhook; an idempotency key simulates the *client* retrying the same logical request (e.g. "create charge") after a timeout, expecting the same response rather than a second charge. This requires `sim-api` to hold a short-lived cache of `(idempotency_key → response)`, which is new state the server doesn't have today (§2: "stateless per request"). Resolve honestly rather than quietly dropping the stateless guarantee: either (a) treat the cache as per-request-bundle state scoped to one `/run` call (the client sends the whole request sequence, including repeated keys, up front — still stateless across separate HTTP calls), or (b) explicitly carve out a narrow, documented exception and update §2's API design note. Don't let this slide in unstated.
- A new scenario exercising client-side idempotent retry (handler receives the same `charge_id` + idempotency key twice, in-order, due to a client timeout — not a provider duplicate) to distinguish this failure mode from the existing duplicate-webhook scenarios in tests and in the gallery.

**V4 definition of done:** invariants #5 and #6 run for real (no stub) and have dedicated property tests analogous to §6.4's existing four; at least one scenario demonstrates each of (a) a reconciliation break and (b) an idempotency-key collision handled correctly by the hardened handler and incorrectly by the naive one; §2's statelessness claim is either preserved under the chosen design or explicitly amended.

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
struct EventId(u64);

// Pure business-event taxonomy — what happened. Delivery (e.g. webhook vs.
// polling) is orthogonal; every SimEvent is a webhook delivery today, so
// there is no Webhook variant. A second transport becomes a `delivery:
// DeliveryKind` field on SimEvent later, not another EventKind variant.
// One variant per payment rail: each rail owns its own event vocabulary under
// sim-core/src/rails/, so adding a rail (e.g. RTP) is an additive variant.
enum EventKind { Card(CardEvent), Ach(AchEvent) }

struct SimEvent { id: EventId, time: u64, seq: u64, kind: EventKind } // time is simulated ms
// EventQueue ordered strictly on (time, seq) — never on event contents

// sim-core/src/rails/card.rs
struct ChargeId(u64);
enum CardEvent {
    Authorized { charge_id: ChargeId, amount: Money },
    Captured { charge_id: ChargeId, amount: Money },
    Refunded { charge_id: ChargeId, amount: Money },
}

// sim-core/src/rails/ach.rs
struct AchEntryId(u64);
enum AchReturnCode { R01, R02, R03, R04, Other(String) } // revisit against NACHA docs in V2
// Batched/Settled move no money and aren't separately invariant-checked (see
// the doc comment on AchState): any illegitimate Returned they could lead to
// is already caught by refund_within_capture and single_entry_per_source_event.
enum AchEvent {
    Batched { entry_id: AchEntryId },
    Settled { entry_id: AchEntryId },
    Returned { entry_id: AchEntryId, code: AchReturnCode, amount: Money },
}

// sim-core/src/money.rs
struct Money(i64); // cents. Checked add/sub. No From<f64>, ever.

// sim-core/src/fault.rs — what each op does is the replay contract, specified in specs/fault-injector-plan.md
#[derive(Serialize, Deserialize)]
enum FaultOp {
    Duplicate { event_id: EventId },              // same EventId, redelivered 30 s later
    Reorder { event_id: EventId, window: usize }, // the window of deliveries starting at event_id arrives reversed
    Delay { event_id: EventId, by: u64 },
    Drop { event_id: EventId },
    CrashRestart { at: u64 },                     // the handler is rebuilt; the ledger survives
}
type FaultPlan = Vec<FaultOp>; // must survive being encoded into a replay URL and fed through the shrinker

// sim-scenarios/src/lib.rs
struct Scenario { id: &'static str, name: &'static str, description: &'static str, initial_ledger: Vec<(String, i64)>, workload: Vec<SimEvent>, story_plan: FaultPlan }
// The workload alone runs clean under both handlers; story_plan is the small fault plan that shows the
// scenario's bug (specs/scenarios-plan.md, decision P). `id` is frozen: replay links store it.

// sim-core/src/handlers/mod.rs
trait EventHandler {
    // Journal entries this event produces; run() is the only caller of post().
    fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry>;
}
// Which handler a run uses, as sim-api and replay links name it.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum HandlerKind { Naive, Hardened }
impl HandlerKind { fn build(self) -> Box<dyn EventHandler> } // callers pass &|| kind.build() to run()

// sim-core/src/simulator.rs — the main entrypoint everything else calls
struct RunResult {
    trace: Vec<SimEvent>,
    opening: BTreeMap<String, Money>, // run() consumes the Ledger, so nothing after it can recover these otherwise
    journal: Vec<JournalEntry>,
    fault_plan: FaultPlan,             // the effective plan: the input if explicit, or generated from the seed if None
    ledger: LedgerSnapshot,
    invariants: Vec<InvariantResult>,
    trace_hash: String,
}
// Takes initial_ledger/workload directly rather than &Scenario: Scenario is
// defined in sim-scenarios, which depends on sim-core (§2) — not the other
// way around, so sim-core can't name sim-scenarios's types. Callers in
// sim-scenarios/sim-api destructure a Scenario before calling run().
// The seed is a u32: mulberry32's whole state, and safe as a JS number.
// `new_handler` is a factory so a CrashRestart fault can rebuild the handler
// mid-run. Invalid input (an unbalanced opening, a bad fault plan) is an Err.
// fault_plan: None means generate one from the seed; Some(plan) replays it exactly.
fn run(initial_ledger: &[(String, i64)], workload: &[SimEvent], seed: u32, fault_plan: Option<&FaultPlan>,
       new_handler: &dyn Fn() -> Box<dyn EventHandler>) -> Result<RunResult, SimError>;
```

`InvariantResult` needs a name/identity, not just pass/fail — the shrinker's stopping condition is "the *same named* invariant still fails," not just "something failed."

### 6.2 Replay-by-seed link design

- A run is fully described by `scenario_id` + `seed` + an **explicit `FaultPlan`** (the seed generates an initial plan, but after that the plan is stored as data, so shrinking can remove individual faults without perturbing unrelated randomness).
- Encoding: `encode_run(scenario_id, seed, handler, fault_plan)` → `1.` + base64url(JSON), held in the URL fragment (no backend storage needed, given the stateless API design in §2). The version prefix sits outside the payload, so V2 can compress the JSON and still be told apart; V1 doesn't compress and caps plans at 100 faults instead (specs/sim-api-plan.md).
- Each run carries a **trace hash** over the delivered events *and* the ledger journal, so a match means the same money movements too; a replay recomputes it and shows a "verified identical" badge, or flags a determinism break.
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

### 6.4 The ledger invariants

1. The ledger always balances (sum of entries is zero)
2. No money is created or destroyed by a fault
3. At most one captured charge per business intent (idempotency)
4. Refunds never exceed captured amount
5. After reconciliation, ledger state converges with the provider's final state
6. No ledger entry exists without a corresponding provider event
7. No two ledger entries share the same source event (redelivery idempotency, any entry kind — not just captures; closes a gap #3 alone misses, e.g. a replayed *refund* webhook)

### 6.5 File-by-file map

```
sim-core/src/
  clock.rs        VirtualClock: now(), advance_to(t), in simulated ms
  rng.rs          seeded PRNG (hand-rolled mulberry32, u32 seed)
  money.rs        Money(i64) newtype, checked arithmetic
  event.rs        SimEvent, EventQueue (BinaryHeap ordered on (time, seq))
  ledger.rs       Ledger { accounts: BTreeMap<String, Money> }, post() rejects unbalanced entries
                  (BTreeMap, not HashMap — iteration order must be deterministic for the trace hash)
  invariants.rs   InvariantCheck trait; the invariants in §6.4
  rails/ach.rs    AchState enum + transition(), AchEvent
  rails/card.rs   CardEvent (CardState deferred past V1)
  rails/rtp.rs    stub, deferred past V3
  handlers/mod.rs EventHandler trait, HandlerKind + build()
  handlers/naive.rs     posts on every money event, no deduplication
  handlers/hardened.rs  dedupes from ledger.journal() (survives CrashRestart); rejects a refund or
                        ACH return before its capture (decisions-log.md D4, temporary)
  fault.rs        FaultOp, FaultPlan, apply_fault_plan() (pure), generate_fault_plan(seed)
  trace.rs        hash_run() — canonical JSON of (trace, journal), then blake3
  simulator.rs    run()
  shrink.rs       shrink_plan() (pure greedy pass), shrink_run() (on the same named invariant) — V1 single-pass greedy, see §6.3
  sweep.rs        sweep(), count_failing_runs() — naive vs. hardened failure rate over a seed range.
                  Its own file, not simulator.rs: keeps run()'s file solely
                  owned by the engine track (specs/v1-backend-task-split.md).

sim-scenarios/src/
  lib.rs                      Scenario struct, scenarios() registry, find(id)
  scenario1_retry.rs           charge retried after timeout
  scenario2_refund_order.rs    refund event arrives before capture
  scenario3_late_return.rs     ACH return arrives after settlement (pulled into V1, decisions-log.md D2)

sim-api/src/
  main.rs            binds $PORT, graceful shutdown — the ONLY crate with Tokio
  lib.rs, app.rs      the router (testable without a network), CORS, body limit
  config.rs           PORT and ALLOWED_ORIGIN, checked at startup
  error.rs            the { error: { code, message } } envelope and its codes
  dto.rs              request/response types, in the frontend contract's field order
  routes/mod.rs       the shared JSON extractor and spawn_blocking + timeout helper
  routes/run.rs       POST /run
  routes/scenarios.rs GET /scenarios
  routes/replay.rs    GET /replay/{encoded}
  routes/shrink.rs    POST /shrink
  routes/sweep.rs     POST /sweep
  encode.rs           encode_run() / decode_run(), see §6.2
  tests/fixtures.rs   golden fixtures for frontend/src/api/fixtures/ (UPDATE_FIXTURES=1)

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