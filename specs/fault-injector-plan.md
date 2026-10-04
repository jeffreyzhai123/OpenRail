# Plan: Fault injector (sim-core `fault.rs`)

## Context
README §3 V1 needs a fault injector covering duplicate, reorder, delay, drop and crash-restart. §6.2 says the seed generates an initial `FaultPlan`, which is then stored as data so the shrinker can remove single faults "without perturbing unrelated randomness".

Today `fault.rs` is data only: `FaultOp` and `FaultPlan = Vec<FaultOp>`, with serde. The engine has the RNG, clock and `hash_run` (PR #2). The event queue (E3), the handler trait (E5) and `run()` (E6) are still open. When E6 lands, it applies explicit plans through F1 (engine deviation 4). This plan is S2 from `v1-mvp-plan.md`.

## The principle behind most decisions below
**The plan is data, and applying it is a pure function with no randomness.** The seed is used once, to generate a plan. After that, `apply_fault_plan(workload, plan)` turns the workload into a delivery schedule without drawing from the RNG. That is what lets the shrinker remove one fault without changing what the others do.

It also splits what has to stay stable:
- **Apply semantics are the replay contract.** A replay link stores the explicit plan and the expected hash, so changing what an op does breaks old links.
- **Generation is not.** Links don't regenerate plans, so tuning the generation rates only changes fresh runs and the sweep.

## Decisions (approved by the user, 2026-10-03)
### R: what `Reorder` does
README §6.1 has `Reorder { window: usize }`, with no anchor, and the roadmap suggested an `Rng::shuffle` over the next *n* events.

**A. Keep `Reorder { window }`: shuffle the whole schedule in chunks of `window`**
- ✅ No README change.
- ❌ One op perturbs the whole trace, so the shrinker can't narrow it down.
- ❌ It needs randomness at apply time, and so a separate RNG stream per op.

**B. `Reorder { event_id, window }`: the `window` deliveries starting at that event arrive in reverse order**
- ✅ Local and shrinkable, with no randomness at apply time. The UI can describe it: "events 3–5 arrive in reverse".
- ✅ `window: 2` is a swap, which is exactly the refund-before-capture case.
- ❌ Deviates from README §6.1, and the frontend's `FaultOp` type changes.
- ❌ Only one permutation per window. The variety comes from the generated anchor and window size instead.

**C. `Reorder { event_id, window, seed }`: a seeded shuffle of that window**
- ✅ More permutations.
- ❌ An extra field in every link, and a 2-event shuffle is a no-op half the time.
- ❌ It deviates from README §6.1 too.

**Chosen: B.**

### C: how a crash-restart replaces the handler
Engine deviation 1 left this to S2: a borrowed `&mut dyn EventHandler` can't be rebuilt mid-run.

**A. `run()` takes a factory, `new_handler: &dyn Fn() -> Box<dyn EventHandler>`**
- ✅ A crash really loses all in-memory state: the replacement is a fresh instance, so nothing survives by mistake.
- ✅ Tests pass closures that build test handlers, and sim-api passes `&|| kind.build()` (`HandlerKind`: names in engine E5, `build()` in S1b).
- ❌ Changes E6's signature. It was approved before E6 landed, so the engine plan now builds E6 with the factory.

**B. Add `fn restart(&mut self)` to `EventHandler`**
- ✅ `run()`'s signature stays the same.
- ❌ Every handler has to reset itself correctly. A buggy reset would hide exactly the bug that crash-restart exists to show.
- ❌ The trait grows a method the naive handler doesn't need.

**Chosen: A.**

### O: the order in which a plan's ops apply
**A. Fixed phases: Drop, then Delay, then Duplicate, then Reorder. Within a phase, ops apply in plan order.**
- ✅ Order only matters between Reorders. The fault editor and the shrinker can treat the plan almost as a set.
- ✅ Each op means the same thing wherever it sits in the list.
- ❌ Some combinations can't be expressed, such as delaying only a duplicate's copy.

**B. Ops apply in plan order, each to the schedule the previous ones produced**
- ✅ Fully general.
- ❌ Moving an op in the editor changes the run, which is surprising.
- ❌ Ops that target copies need their own rules anyway.

**Chosen: A.**

## Other deviations from README / TODO.md (CLAUDE.md requires flagging these)
1. **`RunResult` gains `fault_plan`, the effective plan.** README §6.1 doesn't list it. When the caller passes `None`, the generated plan has to come back, so the UI can show, edit, share and shrink it (`frontend-plan.md` ask #3). **Approved by the user, 2026-10-03**, alongside engine deviation 7.
2. **`SimError` gets `InvalidFaultPlan(FaultError)`.** E6 adds it directly, because it applies explicit plans through F1 (engine deviation 4). No `FaultsNotSupported` error is ever added.
3. **`seed` is used.** This resolves engine deviation 5.
4. **`Rng::below(NonZeroU32)` becomes public.** Generation draws from ranges that are non-empty by construction, so the `Option` from `next_range` would only add an `unwrap`.

## What each op does (the replay contract)
`apply_fault_plan` starts from the workload in slice order and applies the phases below. Between phases 3 and 4 it stable-sorts by time, so ties keep workload order. Copies come after all originals, in their originals' order rather than plan order, so permuting `Duplicate`s changes nothing.

| Op | Effect | Phase |
|---|---|---|
| `Drop { event_id }` | The event is never delivered. | 1 |
| `Delay { event_id, by }` | The event arrives `by` ms later. Overflow is an error. | 2 |
| `Duplicate { event_id }` | A copy with the **same `EventId`** and kind arrives `DUPLICATE_REDELIVERY_MS` (30 s) after the original, which may itself be delayed. This models a provider redelivering after its webhook timeout. Overflow is an error. | 3 |
| `Reorder { event_id, window }` | In delivery order, the `window` deliveries starting at the event arrive in reverse. Each one takes the time slot of the delivery it swaps with, so times stay sorted. The window is clamped at the end of the schedule, and a window of 0 or 1 does nothing. | 4 |
| `CrashRestart { at }` | Doesn't change the schedule. Before the first delivery at or after `at` ms, `run()` replaces the handler with a fresh one. The ledger survives, because it's the durable store. | — |

Rules:
- An op that targets an id not in the workload is an error.
- An op that targets a dropped event does nothing.
- Workload ids must be unique, since ops find events by id.

## Pieces
Branch `fault-injector`, one commit per piece. Each is done when its tests pass and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

| Piece | What | Depends on |
|---|---|---|
| F1 | `apply_fault_plan`, and `Reorder` gains `event_id` (`fault.rs`) | Decisions R and O |
| F2 | `generate_fault_plan` (`fault.rs`), and `Rng::below` becomes public | F1 |
| F3 | Generate a plan in `run()` and return the effective plan (`simulator.rs`) | F2, engine E6 |
| F4 | Docs sync | F3 |

F1 and F2 are pure and only need types that already exist, so they can land before E3, E5 and E6.

**Status (2026-10-03):** decisions R, C and O are approved. F1 is done on `fault-injector` (`4ac0f87`, not merged yet), and F2 is next. Engine E6 builds on F1, applying explicit plans and crash-restarts, so F3 only adds generation and the effective plan. F3 waits on E6; deviation 1 (`RunResult.fault_plan`) is approved (above). See `specs/decisions-log.md` for a consolidated view of every decision across this plan, `deterministic-engine-plan.md` and `v1-mvp-plan.md`.

---

## F1: `apply_fault_plan`
```rust
pub enum FaultOp { /* … */ Reorder { event_id: EventId, window: usize }, /* … */ } // decision R

#[derive(Debug, Clone, PartialEq)]
pub struct Delivery { pub id: EventId, pub time: u64, pub kind: EventKind }
#[derive(Debug, PartialEq)]
pub struct Schedule {
    pub deliveries: Vec<Delivery>, // delivery order: by time, ties in schedule order
    pub crashes: Vec<u64>,         // CrashRestart times, sorted
}
pub enum FaultError { UnknownEvent(EventId), DuplicateWorkloadId(EventId), TimeOverflow(EventId) }

pub fn apply_fault_plan(workload: &[SimEvent], plan: &[FaultOp]) -> Result<Schedule, FaultError>;
```
| Decision | Why |
|---|---|
| Every fault is applied before the drain, as one pure function | It's testable without a simulator, and `SimError::Clock` stays unreachable. V3's dynamic faults can push into the queue later. |
| A `Delivery` type, not a `SimEvent` with an ignored `seq` | The queue assigns `seq` (engine deviation 6). A field that's always ignored invites bugs. |
| Crash times are kept out of the event stream | A crash isn't a business event. As an `EventKind`, it would leak into the trace and the hash. |
| One private helper per phase (`drop_event`, `delay_event`, `duplicate_events`, `reverse_window`) | Each one can be tested alone. `duplicate_events` handles every `Duplicate` at once, so copies follow their originals' order. |
| `EventId` derives `Ord` | Ids key a `BTreeSet` and a `BTreeMap` here. Hashed collections are out (CLAUDE.md). |
| `FaultError` hand-writes `Display` and `Error` | Same pattern as `LedgerError` and `ClockError`. |

**Tests:**
- An empty plan gives the workload in `(time, slice)` order and no crashes. This guards E6's clean runs.
- Each op on its own:
  - `Drop` removes the event.
  - `Delay` shifts the event past a later one, and the schedule re-sorts.
  - `Duplicate` adds a copy with the same id, 30 s later.
  - `Reorder` with window 2 swaps two deliveries' events and keeps the times. Window 3 reverses three. A window past the end is clamped, and windows of 0 and 1 do nothing.
  - `CrashRestart` times come back sorted and leave the deliveries alone.
- Ops on a dropped event do nothing.
- Errors: an unknown id for each op kind, a duplicate workload id, an overflowing delay, and an overflowing redelivery.
- Proptest over random workloads and plans:
  - Deliveries are always sorted by time.
  - The delivery count is the workload size, minus dropped events, plus copies of events that weren't dropped.
  - For plans without `Reorder`, permuting the plan gives the same schedule.

## F2: `generate_fault_plan`
```rust
pub fn generate_fault_plan(seed: u32, workload: &[SimEvent]) -> FaultPlan;
```
Draw order, with every constant named in `fault.rs`:
1. For each workload event, in slice order, roll `below(100)`:
   - under 20 → `Duplicate`;
   - under 25 → `Drop`;
   - under 45 → `Delay { by: 1 + below(MAX_DELAY_MS) }`, where `MAX_DELAY_MS` is 60,000;
   - under 60 → `Reorder { window: 2 + below(2) }`, so the window is 2 or 3;
   - otherwise no fault.
2. Once per run, roll `below(100)`. Under 10 → `CrashRestart` at the time of a workload event chosen with `below(len)`. An empty workload has no crash.

A compile-time `assert!` checks that the per-event percentages sum to 100 or less. The `NonZeroU32` constants come from a `const` `unwrap`, so a zero fails to compile.

| Decision | Why |
|---|---|
| At most one op per event, from one roll | Plans stay short and free of contradictions, such as duplicating and dropping the same event, and the draw order is easy to state. |
| A crash lands on an event's time | It then falls just before a delivery, where it can matter. |
| The rates are first guesses | With 4 events, about 2.5 faults per run, and about 2% of seeds get no fault. Tune them with the sweep once the S1a scenarios and S1b handlers exist: naive should fail often and hardened never. Tuning never breaks existing links (see the principle). |

**Tests:**
- The same seed gives the same plan. Seeds 0..100 don't all give the same plan.
- Proptest over seeds and workloads:
  - Every op targets a workload id, with at most one op per event.
  - There's at most one `CrashRestart`, and it's at a workload time.
  - Delays are in `1..=60_000` and windows are 2 or 3.
  - Every generated plan applies without error.
- An empty workload gives an empty plan.
- Over seeds 0..1,000 on a 4-event workload, every op kind appears at least once. This catches a broken threshold.

## F3: generate a plan in `run()`
E6 already applies explicit plans through `apply_fault_plan`, crash-restarts included, using the handler factory (engine deviation 4, decision C). F3 adds the rest:
1. `None` now means `generate_fault_plan(seed, workload)` instead of an empty plan. The seed is finally used, which resolves engine deviation 5.
2. `RunResult` gains `pub fault_plan: FaultPlan`, the effective plan (deviation 1).

`run()`'s signature doesn't change. The per-fault and crash-restart tests live in E6, which applies explicit plans.

**Tests:**
- **Replay property:** `run(seed, None)` and `run(seed, Some(&result.fault_plan))` give identical `RunResult`s. With an explicit plan, a different seed gives the same result too: the plan, not the seed, determines the run.
- **Determinism smoke:** 100 runs with seed 42 and `None` now exercise generated faults.
- `result.fault_plan` equals `generate_fault_plan(seed, workload)` for `None`, and equals the input for `Some`.

## F4: docs sync
README §6.1 (`Reorder { event_id, window }`, the factory in `run()`) and §6.5 (`generate_fault_plan`), and the frontend's `FaultOp` type, were synced when the decisions were approved. What's left:
- README §6.1: `RunResult.fault_plan`, once deviation 1 is approved.
- `v1-mvp-plan.md`: feature 5 is done.

## What this means for the handlers (`v1-mvp-plan.md` S1b)
- **Hardened idempotency has to be durable.** It should check `ledger.journal()` for an entry whose `source` is this event (a redelivery), or for a capture on the same intent. An in-memory set fails after a `CrashRestart`, and showing that is the point of the fault.
- **D4 (a refund that arrives before its capture):** buffering it in memory loses it on a crash, and rejecting it loses it too. No V1 invariant notices a lost refund; that's #5 (reconciliation, V4). Choose D4 knowing that.
- **A `Drop` is only visible in V1 through #4**, when a refund's capture was dropped. A dropped refund breaks nothing V1 checks. Lost webhooks in general are what #5 and #6 cover in V4.

## Files
- Modify: `crates/sim-core/src/fault.rs` (F1, F2), `event.rs` (`EventId` derives `Ord`, F1), `rng.rs` (`below` becomes public, F2), `simulator.rs` (F3). F4: `README.md`, `specs/frontend-plan.md`, `specs/v1-mvp-plan.md`.
- No new dependencies.

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every piece.
- `grep -rn "HashMap\|HashSet\|f64\|tokio\|rand::" crates/sim-core/src` finds nothing new.
- The replay-property test passes: a plan taken from a run's result reproduces that run exactly. That's what a share link relies on.
- Once S3 lands, a sweep over seeds 0..100 shows naive failing more often than hardened.
