# Plan: Partner A, deterministic engine (sim-core)

## Context
Step 0 (`f65876b`) and Partner B's work (`e03a7e4`) are committed. B's seam is fixed: `Ledger::open(&[(String, i64)])`, `Ledger::post(JournalEntry)`, `invariants::check_all(&InvariantContext)`. `125cac4` made `EventKind` a composite, `Card(CardEvent) | Ach(AchEvent)`, with rail types under `rails/`. Partner A owns `rng.rs`, `clock.rs`, the `EventQueue` in `event.rs`, `trace.rs`, `simulator.rs`, and (decided with the user) the `EventHandler` trait in `handler.rs`.

What A starts from:
- `event.rs`: `EventId`, `SimEvent { id, time, seq, kind }` with `Ord` on `(time, seq)`. **Missing:** `EventQueue`.
- `simulator.rs`: `RunResult` and a `run()` signature whose body is `unimplemented!()`.
- `fault.rs`: `FaultOp` and `FaultPlan`, data only.
- `rng.rs`, `clock.rs`, `trace.rs`, `handler.rs`: empty. `cargo fmt --check` fails on them.

## The principle behind most decisions below
**Every ordering is explicit, and nothing reads ambient state.** The queue orders by `(time, seq)`, ties fall back to workload slice order, JSON uses field declaration order, and the ledger uses `BTreeMap`. Nothing reads a wall clock, a randomized hasher or the environment. Second rule: **low-level code reports, `run()` decides.** The clock, queue, trace and ledger return errors, and `run()` is the one boundary that turns them into a `SimError`.

## Deviations from TODO.md / README (CLAUDE.md requires flagging these)
1. **`run()` takes `handler: &mut dyn EventHandler`.** README §6.1 has no handler parameter. Approved by the user. "Posts to the ledger" needs something that maps events to entries, and naive vs hardened is V1's whole demo, so this fixes the final signature now. It also matches `frontend-plan.md` ask #1.
2. **`run()` returns `Result<RunResult, SimError>`.** README §6.1 returns a bare `RunResult`. An unbalanced opening or a structurally invalid handler entry has to surface as an error at the boundary (sim-api maps it to 4xx/5xx), not as a panic.
3. **`hash_trace` returns `Result<String, serde_json::Error>`.** The TODO says `-> String`. serde_json's API is fallible. It can't fail for today's types, but returning the error keeps the no-`unwrap` rule without a "provably impossible" argument that a future `EventKind` could quietly break.
4. **A non-empty `fault_plan` returns `Err(FaultsNotSupported)` until fault injection lands.** Ignoring it silently would make a fault-injected replay link look like it ran, which is a fake result.
5. **`seed` is accepted but not used yet.** Until faults exist, nothing random happens in a run. The determinism test still proves the pipeline has no hidden nondeterminism, such as hasher order or ambient state. It doesn't prove that seeded faults replay; it covers that once faults land.
6. **The `seq` in workload events is ignored.** The queue assigns `seq` at push, so a scenario's slice order is its tie-break. `Scenario.workload` stays `Vec<SimEvent>` for now.

## Order of work (branch `partner-a-engine`, one commit per step)
0. **`cargo fmt`** so the empty stubs pass `fmt --check` (it only adds whitespace).
1. **`rng.rs`** with tests.
2. **`clock.rs`** with tests.
3. **`EventQueue`** in `event.rs`, with tests and a proptest.
4. **`trace.rs`** with tests, including a pinned golden hash.
5. **The `EventHandler` trait and `run()`**, with the determinism smoke test and end-to-end tests. This depends on steps 2–4.
6. **Docs:** tick Step 0 and A's boxes in TODO.md, fix `docs/partner-b-plan.md` → `specs/`, and sync README §6.1 and §6.5 (the `run()` signature, the composite `EventKind`, the `rails/` paths).

Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` before each commit.

---

## `rng.rs`
```rust
const MULBERRY32_INCREMENT: u32 = 0x6D2B_79F5;
pub struct Rng { state: u32 }
impl Rng {
    pub fn from_seed(seed: u64) -> Rng;                           // state = (seed ^ (seed >> 32)) as u32
    pub fn next_u32(&mut self) -> u32;                            // mulberry32
    pub fn next_range(&mut self, range: Range<u32>) -> Option<u32>; // None if empty
    pub fn shuffle<T>(&mut self, items: &mut [T]);                // Fisher–Yates
}
```
| Decision | Why |
|---|---|
| Fold the seed by XORing its two halves | Every bit of the u64 seed affects the state, and the rule fits in one line. **Honest limit:** there are only 2^32 streams, so some distinct seeds collide (e.g. `0` and `0x1_0000_0001`). Separately, mulberry32 seeds `s` and `s + 0x6D2B79F5` produce the same stream shifted by one. Same seed → same run still holds, and that's the guarantee replay needs. |
| `wrapping_*` arithmetic everywhere | mulberry32 is defined mod 2^32. A plain `*` would panic in debug builds. |
| `next_range` uses rejection sampling (arc4random_uniform style), not `%` | `% n` biases toward small values. The loop only draws from the seeded stream, so it stays deterministic. |
| `Range<u32>` only, no u64/usize variants | YAGNI. The only consumer is `shuffle` (and later the `Reorder` window). |
| `shuffle` does `assert!(items.len() <= u32::MAX as usize)` and then casts indices with `as u32` | It states an invariant the type can't encode (CLAUDE.md). A trace with 4 billion events isn't a real input. |

## `clock.rs`
```rust
#[derive(Default)] pub struct VirtualClock { now: u64 }   // starts at tick 0
impl VirtualClock { pub fn now(&self) -> u64; pub fn advance_to(&mut self, t: u64) -> Result<(), ClockError>; }
pub enum ClockError { Backwards { now: u64, requested: u64 } }
```
| Decision | Why |
|---|---|
| Moving backwards returns `Err` and leaves `now` unchanged, instead of panicking | Low-level code reports, the boundary decides (CLAUDE.md). `run()` turns it into `SimError::Clock`. |
| `advance_to(now)` is `Ok` | Several events at the same tick are normal; that's the tie-break case. |
| No `advance_by` | YAGNI. `Delay` faults will compute absolute times. |
| `ClockError` hand-writes `Display` and `Error` | Same pattern as `LedgerError`, no `thiserror`. |

## `EventQueue` (`event.rs`)
```rust
#[derive(Default)] pub struct EventQueue { heap: BinaryHeap<Reverse<SimEvent>>, next_seq: u64 }
impl EventQueue {
    pub fn push(&mut self, id: EventId, time: u64, kind: EventKind);   // assigns seq = next_seq++
    pub fn pop(&mut self) -> Option<SimEvent>;                          // min (time, seq)
}
```
| Decision | Why |
|---|---|
| `push` takes the parts, not a `SimEvent` | The queue is the only source of `seq`, so a caller can't supply one that collides or is out of order (deviation 6). |
| `Reverse<SimEvent>` on top of the existing `(time, seq)` `Ord` | The hard rule says order on `(time, seq)`, never on payload. Note that `Ord` ignores `id` and `kind` while `Eq` compares them; that's sound here only because `seq` is unique within a queue. |
| No `len`, `is_empty` or `peek` | YAGNI. `run()` drains it with `while let Some`. |

## `trace.rs`
```rust
pub fn hash_trace(events: &[SimEvent]) -> Result<String, serde_json::Error>; // blake3(serde_json::to_vec(events)) as 64 lowercase hex chars
```
| Decision | Why |
|---|---|
| "Canonical JSON" is plain `serde_json::to_vec` | Derived `Serialize` emits fields in declaration order with externally tagged enums and exact integers. The types have no maps and no floats, so the bytes are a pure function of the values. Any future map must be a `BTreeMap`. |
| A golden-hash test pins the output | Changing a field order, a variant name or a field changes every hash and breaks "verified identical" on old replay links. The golden test turns that into a deliberate decision that comes with a replay-encoding version bump (§6.2). It's also the cross-process check, since an in-process repeat can't catch a per-process difference. |

## `handler.rs`
```rust
pub trait EventHandler {
    /// Journal entries this event produces. The simulator posts them.
    fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry>;
}
```
| Decision | Why |
|---|---|
| Returns entries instead of mutating the ledger | `run()` is the only caller of `post()`, so it can tie a rejection to the event that caused it, and handlers can be tested without a simulator. No hidden side effects. |
| `&mut self` | The hardened handler needs memory (seen event ids, per-entry `AchState`). That's domain state, which CLAUDE.md allows. |
| Read-only `&Ledger` | A handler may look at balances or the journal, e.g. hardened checking for a prior capture. It costs nothing and avoids a signature change later. |
| No `Result` | A bad ordering is the handler's to handle or mishandle, and the invariants judge the result (Partner B's principle). Structurally invalid entries are caught by `post()`. |
| `naive.rs` and `hardened.rs` stay empty | Not today (TODO "Explicitly NOT today"). |

## `simulator.rs`
```rust
pub enum SimError {
    InvalidOpening(LedgerError),
    FaultsNotSupported,
    Clock(ClockError),
    Posting { event: EventId, error: LedgerError },
    TraceEncoding(serde_json::Error),
}
#[derive(Debug, Clone, PartialEq, Serialize)] pub struct RunResult { trace, ledger, invariants, trace_hash }  // fields unchanged
pub fn run(initial_ledger: &[(String, i64)], workload: &[SimEvent], seed: u64,
           fault_plan: Option<&FaultPlan>, handler: &mut dyn EventHandler) -> Result<RunResult, SimError>;
```
Flow:
1. Reject a non-empty `fault_plan`. (`None` and `Some(&vec![])` are both fine.)
2. `Ledger::open` → `InvalidOpening`.
3. Push the workload in slice order. Each `kind` is cloned, because the queue owns its events and the workload is borrowed.
4. Drain the queue: `advance_to`, then `handler.handle`, then `post` each entry (→ `Posting { event }`), then append to the trace.
5. `check_all`, then `hash_trace`, then `snapshot`.

| Decision | Why |
|---|---|
| `&mut dyn EventHandler`, not generic | sim-api picks naive or hardened at runtime from the request, so one compiled `run` is enough. |
| `seed` is bound as `let _ = seed;` with a one-line "why" comment, and no unused `Rng` is built | Being honest about deviation 5. It will seed `Rng` for fault-plan generation (§6.2). |
| `RunResult` derives `PartialEq` and `Serialize` | Tests compare whole results across runs, and sim-api serializes the result. There's no `Deserialize` because of `InvariantResult.name: &'static str` (see B's flag). |
| `SimError` hand-writes `Display` and `Error` | Same pattern as `LedgerError`. |

## Tests (all deterministic; proptests use `crate::test_support::proptest_config()`)
- **rng:** The same seed gives identical first 1,000 outputs. A golden first-5 outputs for seed 0 pins the algorithm. The 16-value prefixes for seeds 0..100 are pairwise distinct. `next_range` returns `None` for an empty range, always stays in range (proptest), and a single-value range always returns its start. `shuffle` produces a permutation, is deterministic for a given seed, and leaves 0- and 1-element slices unchanged.
- **clock:** Starts at 0. Moving forward or staying at the same tick is `Ok`. Moving backwards is `Err(Backwards { now, requested })` and `now` is unchanged.
- **queue:** Same-time events pop in push order. An earlier time pushed later pops first. Proptest with times drawn from a small range to force ties: pops are strictly increasing in `(time, seq)`, and the pop count equals the push count.
- **trace:** Hashing the same input twice gives the same 64-hex string. The golden hash for a fixed 3-event trace (Card Captured, Card Refunded, Ach Returned) matches. Changing `time`, `seq`, `id` or an amount changes the hash, and so does swapping two events. An empty trace hashes fine.
- **simulator**, using a test-only `CardHandler` (Captured → `Capture` transfer `external:card`→`merchant` with intent `charge-{id}`, Refunded → `Refund`, everything else → no entries) and a fixed workload that has two same-time events and is out of time order in the slice:
  - **Determinism smoke (the test that must never go yellow):** 100 runs with seed 42 all give an identical `RunResult` and `trace_hash`.
  - The trace is in `(time, seq)` order, and same-time events keep their workload order.
  - The clean workload ends with the expected balances, and all 4 invariants pass.
  - **Seam check:** a duplicated capture (new `EventId`, same `charge_id`) makes `single_capture_per_intent` fail while the others pass, end to end through `run()`.
  - Failure paths: an unbalanced opening gives `InvalidOpening`. A non-empty plan gives `FaultsNotSupported`. A handler that emits an unbalanced entry gives `Posting` with that event's id. An empty workload gives `Ok`, with an empty trace and the hash of `[]`.

## Files
- Modify: `crates/sim-core/src/rng.rs`, `clock.rs`, `event.rs`, `trace.rs`, `handler.rs`, `simulator.rs`. In step 6: `TODO.md`, `README.md`.
- Only `cargo fmt` touches: `shrink.rs`, `handlers/naive.rs`, `handlers/hardened.rs`.
- Not touched: B's files (`money`, `ledger`, `invariants`, `rails/*`), `lib.rs`, `Cargo.toml` (no new deps).

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every step.
- `grep -rn "HashMap\|HashSet\|f64\|tokio\|rand::\|unimplemented!" crates/sim-core/src` finds nothing except the `From<f64>` doc comment in `money.rs`.
- TODO.md's "Done for today when" holds: the gates are green, `run()` produces a stable `trace_hash` (both the golden and the ×100 tests), and none of the forbidden items are in `sim-core`.
