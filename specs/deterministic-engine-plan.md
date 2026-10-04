# Plan: Partner A, deterministic engine (sim-core)

## Context
Step 0 (`f65876b`) and Partner B's work (`e03a7e4`) are committed. B's seam is fixed: `Ledger::open(&[(String, i64)])`, `Ledger::post(JournalEntry)`, `invariants::check_all(&InvariantContext)`. `125cac4` made `EventKind` a composite, `Card(CardEvent) | Ach(AchEvent)`, with rail types under `rails/`. Partner A owns `rng.rs`, `clock.rs`, the `EventQueue` in `event.rs`, `trace.rs`, `simulator.rs`, and (decided with the user) the `EventHandler` trait in `handlers/mod.rs` (`8f711df` deleted the separate `handler.rs`).

What A starts from:
- `event.rs`: `EventId`, `SimEvent { id, time, seq, kind }` with `Ord` on `(time, seq)`. **Missing:** `EventQueue`.
- `simulator.rs`: `RunResult` and a `run()` signature whose body is `unimplemented!()`.
- `fault.rs`: `FaultOp` and `FaultPlan`, data only.
- `rng.rs`, `clock.rs`, `trace.rs`: empty. `handlers/mod.rs` only declares `naive` and `hardened`. `cargo fmt --check` fails on the empty files.

The work is split into pieces E0–E7 below. Each piece is one commit on a feature branch, merged into `develop` by PR. It names what it depends on, and is done when its own tests pass and `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

**Status (2026-10-03):** all pieces are done. E0–E4 merged in PR #2 (`3fc6ff3`), E5 with the handlers in `c69d2f1`, and E3 and E6 in `3f423e5`, with F3 folded into `run()`. E6's remaining tests (a real crash-restart test, replay, the golden full-run hash, Delay/Drop, the `Posting` error, trace order) landed in `ca2740a`. E7's docs sync is done. See `specs/decisions-log.md` for a consolidated view of every decision across this plan, `fault-injector-plan.md` and `v1-mvp-plan.md`.

## The principle behind most decisions below
**Every ordering is explicit, and nothing reads ambient state.** The queue orders by `(time, seq)`, ties fall back to workload slice order, JSON uses field declaration order, and the ledger uses `BTreeMap`. Nothing reads a wall clock, a randomized hasher or the environment. Second rule: **low-level code reports, `run()` decides.** The clock, queue, trace and ledger return errors, and `run()` is the one boundary that turns them into a `SimError`.

## Deviations from TODO.md / README (CLAUDE.md requires flagging these)
1. **`run()` takes a handler factory, `new_handler: &dyn Fn() -> Box<dyn EventHandler>`.** README §6.1 has no handler parameter. Approved by the user. "Posts to the ledger" needs something that maps events to entries, and naive vs hardened is V1's whole demo. It also matches `frontend-plan.md` ask #1. It's a factory rather than a borrowed handler so that a crash-restart can build a fresh one (`fault-injector-plan.md` decision C, approved).
2. **`run()` returns `Result<RunResult, SimError>`.** README §6.1 returns a bare `RunResult`. An unbalanced opening or a structurally invalid handler entry has to surface as an error at the boundary (sim-api maps it to 4xx/5xx), not as a panic.
3. **`hash_run(trace, journal) -> Result<String, serde_json::Error>` replaces `hash_trace(&[SimEvent]) -> String`.** Approved by the user (decision H). It hashes the journal as well as the events. It returns a `Result` because serde_json's API is fallible. It can't fail for today's types, but returning the error keeps the no-`unwrap` rule without a "provably impossible" argument that a future `EventKind` could quietly break.
4. **E6 applies explicit fault plans through `apply_fault_plan` (`fault-injector-plan.md` F1), crash-restarts included. `None` means no faults until F3 adds generation.** README §6.1 doesn't say how faults run. F1 already exists, so this avoids adding a `FaultsNotSupported` error that F3 would only delete.
5. **`seed` is accepted but not used yet.** Until faults exist, nothing random happens in a run. The determinism test still proves the pipeline has no hidden nondeterminism, such as hasher order or ambient state. It doesn't prove that seeded faults replay; it covers that once faults land.
6. **The `seq` in workload events is ignored.** The queue assigns `seq` at push, so a scenario's slice order is its tie-break. `Scenario.workload` stays `Vec<SimEvent>` for now.
7. **`RunResult` gains `opening` and `journal`.** **Approved by the user, 2026-10-03.** README §6.1 lists only `trace`, `ledger`, `invariants` and `trace_hash`. `run()` consumes the `Ledger`, so nothing after it can recover them, and the timeline scrubber needs both (`frontend-plan.md` ask #4, `v1-mvp-plan.md` gap 1).
8. **The seed is a `u32`.** Approved by the user (decision W). README §6.1 has `seed: u64`. mulberry32 has a 32-bit state, so a wider seed only adds a fold and collisions, and a `u32` fits in a JS number.

## Decisions (settled with the user, 2026-10-03)
Each decision below keeps the options that were weighed. The seed and the times go into replay links, and the hash is what a replay checks. Changing any of the three once links exist therefore needs a replay-encoding version bump (§6.2).

### W: seed width
mulberry32 has a 32-bit state, so there are only 2^32 distinct streams, whatever the seed's type.

**A. `u64` seed, folded into the state (as written, README §6.1)**
- ✅ Matches README §6.1, `frontend-plan.md` and `v1-mvp-plan.md` S4 as written. No other doc changes.
- ✅ If a later generator has a 64-bit state, the seed type and existing links stay the same.
- ❌ Distinct seeds can give identical runs: each stream is shared by 2^32 seeds. Only seeds ≥ 2^32 collide with another seed, so in practice this is a caveat to document rather than a bug users hit.
- ❌ A `u64` doesn't fit in a JS number, so the seed travels as a decimal string (frontend ask #5). That needs a string validator on both sides, and a random seed has to be built from two 32-bit halves.
- ❌ One more rule (the fold) to implement, test and explain.

**B. `u32` seed, used directly as the state**
- ✅ One seed, one stream. No fold, no collisions, no caveat.
- ✅ Fits in a JS number (`u32::MAX` < 2^53), so the seed is a plain JSON number and frontend ask #5 goes away. Validation becomes a range check on an integer.
- ✅ 4 billion seeds is far more than a sweep will use.
- ❌ Deviates from README §6.1 (`seed: u64`). It needs approval, plus edits to README §6.1, to `frontend-plan.md` (ask #5, the DTOs, `seed.ts` and its tests) and to `v1-mvp-plan.md` S4.
- ❌ A future 64-bit generator would change the seed type again, with another link version bump.

**Chosen: B** (deviation 8), while no code read the seed. The fold and the string seed both existed only to carry 32 bits of entropy in a 64-bit type.

### T: tick unit
`VirtualClock`'s code is the same under every option. What changes is how scenarios (S1a) set times and how fault generation (S2) picks delays.

**A. Unitless ticks (as written)**
- ✅ Nothing to decide or document.
- ❌ Each scenario, `Delay { by }`, `CrashRestart { at }` and the timeline UI picks its own meaning. A delay of 5 can mean different things in different scenarios, and the UI can't label time.
- ❌ Fault-generation settings (S2) can't be stated in real terms, such as "delay up to 2 s".

**B. 1 tick = 1 ms**
- ✅ Resolves sub-second gaps, such as two webhooks 200 ms apart that a `Reorder` or `Delay` should be able to swap.
- ✅ A `u64` covers about 584 million years, and any time below 2^53 ms (about 285,000 years) is a safe JS integer, so the frontend's `Number.isSafeInteger` check holds.
- ✅ The same unit as JS `Date` and most webhook and timeout settings, so the UI can format durations directly.
- ❌ Hand-written numbers get long. An ACH return 3 days after settlement is at `259_200_000`, so scenarios need named constants such as `MS_PER_DAY`.

**C. 1 tick = 1 second**
- ✅ Readable numbers: 3 days is `259_200`.
- ✅ Enough for "retry after a 30 s timeout" and for ACH timing.
- ❌ No sub-second resolution, so events less than a second apart share a tick. The `(time, seq)` tie-break keeps that deterministic, but `Delay` and `Reorder` lose precision. Moving to milliseconds later rescales every scenario and breaks existing links.

**Chosen: B.** Webhook reorders happen at sub-second scale, and named constants offset the readability cost.

### H: what the trace hash covers
TODO.md hashes only the popped events.

**A. Events only (as TODO.md)**
- ✅ Matches TODO.md's `hash_trace(&[SimEvent])`, so the only deviation would be returning a `Result`.
- ✅ The hash identifies what was *delivered*: naive and hardened runs of the same plan share it, which shows both handlers saw the identical event sequence.
- ✅ Fixing a handler bug doesn't change any hash, so old links keep verifying.
- ❌ The flip side: "verified identical" can show while the balances differ, e.g. on a link made before a handler change. That's a fake result.
- ❌ A handler that iterates a `HashMap`, so its journal order varies per process, passes every hash test, because nothing hashed comes from the handler. Only the in-process ×100 test sees the outcome, and it can't see a per-process difference.

**B. Events + journal**
- ✅ The hash covers the outcome: the same hash means the same events *and* the same money movements.
- ✅ E6's golden full-run hash becomes a cross-process guard on the handler and ledger path, where nondeterminism is most likely to creep in (handler state).
- ❌ Deviates from TODO.md's signature (it becomes `hash_run(trace, journal)`), so it needs approval.
- ❌ Any change to what a handler posts, including a bug fix, changes the hash for the affected runs, so old links show a mismatch. That's accurate, but the UI should say "made with a different version" rather than "determinism break", which ties into the replay-encoding version (§6.2).
- ❌ The field keeps the name `trace_hash`, because renaming it would change the frontend contract. The name becomes slightly misleading.

**C. The whole `RunResult` minus the hash**
- ✅ Nothing in the result can differ unnoticed.
- ❌ Invariant messages are prose, so rewording one would change the hash and break every link.
- ❌ Redundant: the opening is an input, and the final ledger and the invariant results follow from the opening plus the journal, which B already covers.

**Chosen: B** (deviation 3).

## Pieces
| Piece | What | Depends on | Status |
|---|---|---|---|
| E0 | Format the empty stubs | — | ✅ PR #2 |
| E1 | Seeded RNG (`rng.rs`) | E0 | ✅ PR #2 |
| E2 | Virtual clock (`clock.rs`) | E0 | ✅ PR #2 |
| E3 | Event queue (`event.rs`) | E0 | ✅ Done |
| E4 | Trace hash (`trace.rs`) | E0 | ✅ PR #2 |
| E5 | `EventHandler` trait and `HandlerKind` names (`handlers/mod.rs`) | E0 | ✅ Done |
| E6 | `run()` and the determinism tests (`simulator.rs`) | E2, E3, E4, E5, F1 | ✅ Done |
| E7 | Docs sync (what the early sync left) | E6 | ✅ Done |

E3 and E5 don't depend on each other. **Land E5 early:** it's tiny, and it unblocks the handlers track (`v1-mvp-plan.md` S1b) and lets sim-api name handlers. E1 isn't on `run()`'s path yet (deviation 5). Its first consumer is fault generation (S2).

---

## E0: Format the empty stubs
- Run `cargo fmt`. It only adds whitespace to the 6 empty files: `clock.rs`, `rng.rs`, `trace.rs`, `shrink.rs`, `handlers/naive.rs` and `handlers/hardened.rs`.
- **Done when:** `cargo fmt --check` passes.

## E1: Seeded RNG (`rng.rs`)
```rust
const MULBERRY32_INCREMENT: u32 = 0x6D2B_79F5;
pub struct Rng { state: u32 }
impl Rng {
    pub fn from_seed(seed: u32) -> Rng;                             // state = seed (decision W)
    pub fn next_u32(&mut self) -> u32;                              // mulberry32
    pub fn next_range(&mut self, range: Range<u32>) -> Option<u32>; // None if empty
    pub fn shuffle<T>(&mut self, items: &mut [T]);                  // Fisher–Yates
    fn below(&mut self, bound: NonZeroU32) -> u32;                  // uniform in 0..bound
}
```
These algorithms are part of the replay contract, because a seed's fault plan is built from them. The golden tests pin each one. All arithmetic wraps.
- `next_u32`: `state += INC; t = state; t = (t ^ t >> 15) * (t | 1); t ^= t + (t ^ t >> 7) * (t | 61); return t ^ t >> 14`.
- `below(n)`: `threshold = n.wrapping_neg() % n`, then draw until `r >= threshold` and return `r % n`.
- `next_range(a..b)`: `a + below(b - a)`, or `None` when `b <= a`.
- `shuffle`: `for i in (1..len).rev() { swap(i, below(i + 1)) }`.

| Decision | Why |
|---|---|
| The `u32` seed is the state as is | Decision W: one seed, one stream. **Honest limit:** mulberry32 seeds `s` and `s + 0x6D2B79F5` produce the same stream shifted by one. Same seed → same run still holds, and that's the guarantee replay needs. |
| `wrapping_*` arithmetic everywhere | mulberry32 is defined mod 2^32. A plain `*` would panic in debug builds. |
| `next_range` and `shuffle` share a private `below(NonZeroU32)` | "Non-empty" lives in the type, so `shuffle` never unwraps `next_range`'s `Option`. `NonZeroU32::MIN.saturating_add(i as u32)` builds `i + 1` with no `Option`. |
| `below` uses rejection sampling (arc4random_uniform style), not `%` | `% n` biases toward small values. The loop only draws from the seeded stream, so it stays deterministic. |
| `next_range` is public, though nothing in this plan calls it | TODO.md lists it, and fault generation (S2) will draw delays and event indices from ranges. `Range<u32>` only, no u64/usize variants (YAGNI). |
| `shuffle` does `assert!(items.len() <= u32::MAX as usize)` before casting indices with `as u32` | It states an invariant the type can't encode (CLAUDE.md). A trace with 4 billion events isn't a real input. |

**Tests:**
- **Golden values come from the reference implementation, not our own output.** The first 5 `next_u32` values match Tommy Ettinger's JS mulberry32, taking the u32 before the `/ 2^32`. JS and Python ports agree on these:
  - seed 0: `1144304738, 1416247, 958946056, 627933444, 2007157716`
  - seed 42: `2581720956, 1925393290, 3661312704, 2876485805, 750819978` (the seed E6 uses)
- **`below` and `shuffle` are pinned too.** A refactor that changes them, such as Lemire's method or a forward Fisher–Yates, would silently change every seed's fault plan. There's no external reference for these, so the values come from a Python port of the algorithms above. For seed 42:
  - `next_range(0..10)` ×10: `[6, 0, 4, 5, 8, 7, 0, 7, 0, 5]`
  - `shuffle` of `[0, 1, …, 9]`: `[1, 3, 9, 5, 2, 8, 4, 0, 7, 6]`
- The same seed gives identical first 1,000 outputs. The 16-value prefixes for seeds 0..100 are pairwise distinct.
- `next_range` returns `None` for an empty range, always stays in range (proptest), and a single-value range always returns its start.
- `shuffle` produces a permutation, is deterministic for a given seed, and leaves 0- and 1-element slices unchanged.

## E2: Virtual clock (`clock.rs`)
One tick is one millisecond of simulated time (decision T), and `VirtualClock`'s doc comment says so.
```rust
#[derive(Default)] pub struct VirtualClock { now: u64 }   // starts at 0 ms
impl VirtualClock { pub fn now(&self) -> u64; pub fn advance_to(&mut self, t: u64) -> Result<(), ClockError>; }
pub enum ClockError { Backwards { now: u64, requested: u64 } }
```
| Decision | Why |
|---|---|
| Moving backwards returns `Err` and leaves `now` unchanged, instead of panicking | Low-level code reports, the boundary decides (CLAUDE.md). `run()` turns it into `SimError::Clock`. |
| `advance_to(now)` is `Ok` | Several events at the same tick are normal; that's the tie-break case. |
| No `advance_by` | YAGNI. `Delay` faults will compute absolute times. |
| `ClockError` hand-writes `Display` and `Error` | Same pattern as `LedgerError`, no `thiserror`. |

**Tests:** Starts at 0. Moving forward or staying at the same tick is `Ok`. Moving backwards is `Err(Backwards { now, requested })` and `now` is unchanged.

## E3: Event queue (`event.rs`)
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

**Tests:** Same-time events pop in push order. An earlier time pushed later pops first. A proptest draws times from a small range to force ties, and checks that pops are strictly increasing in `(time, seq)` and the pop count equals the push count.

## E4: Trace hash (`trace.rs`)
```rust
/// blake3 of serde_json::to_vec(&(trace, journal)), as 64 lowercase hex chars.
pub fn hash_run(trace: &[SimEvent], journal: &[JournalEntry]) -> Result<String, serde_json::Error>;
```
| Decision | Why |
|---|---|
| Hash the journal as well as the events | Decision H. The same hash then means the same events *and* the same money movements. Invariant messages are left out because they're prose, and the final ledger and the invariant results follow from the opening plus the journal. |
| "Canonical JSON" is plain `serde_json::to_vec` | Derived `Serialize` emits fields in declaration order with externally tagged enums and exact integers. The hashed types have no maps and no floats, so the bytes are a pure function of the values. Any future map must be a `BTreeMap`. |
| A golden-hash test pins the output | Changing a field order, a variant name or a field changes every hash and breaks "verified identical" on old replay links. The golden test turns that into a deliberate decision that comes with a replay-encoding version bump (§6.2). |

**Tests:**
- Hashing the same input twice gives the same 64-hex string. An empty trace and journal hash fine.
- The golden hash for a fixed 3-event trace (Card Captured, Card Refunded, Ach Returned) and its 2 journal entries matches.
- Changing `time`, `seq`, `id` or an amount changes the hash, and so does swapping two events. Changing or dropping a journal entry also changes it.

## E5: `EventHandler` trait (`handlers/mod.rs`)
```rust
pub trait EventHandler {
    /// Journal entries this event produces. The simulator posts them.
    fn handle(&mut self, event: &SimEvent, ledger: &Ledger) -> Vec<JournalEntry>;
}

/// Which handler a run uses, as the API and replay links name it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HandlerKind { Naive, Hardened }   // `build()` arrives with the handlers (S1b)
```
| Decision | Why |
|---|---|
| Returns entries instead of mutating the ledger | `run()` is the only caller of `post()`, so it can tie a rejection to the event that caused it, and handlers can be tested without a simulator. No hidden side effects. |
| `&mut self` | The hardened handler needs memory (seen event ids, per-entry `AchState`). That's domain state, which CLAUDE.md allows. |
| Read-only `&Ledger` | A handler may look at balances or the journal, e.g. hardened checking for a prior capture, and after a crash-restart (S2) the journal is the only memory left. |
| No `Result` | A bad ordering is the handler's to handle or mishandle, and the invariants judge the result (Partner B's principle). Structurally invalid entries are caught by `post()`. |
| `HandlerKind` lands here with names only | sim-api, the replay encoding and the sweep can name a handler before any handler exists. `build() -> Box<dyn EventHandler>` comes with the handlers, and callers then pass `&\|\| kind.build()`. |
| `naive.rs` and `hardened.rs` stay empty | Not today (TODO "Explicitly NOT today"). They're `v1-mvp-plan.md` S1b. |

**Tests:** the trait has none of its own, since E6's test handler exercises it. `HandlerKind` serializes as `"naive"` and `"hardened"`, matching the frontend's `Handler` type, and an unknown name fails to deserialize.

## E6: `run()` and the determinism tests (`simulator.rs`)
```rust
pub enum SimError {
    InvalidOpening(LedgerError),
    InvalidFaultPlan(FaultError),
    Clock(ClockError),
    Posting { event: EventId, error: LedgerError },
    TraceEncoding(serde_json::Error),
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunResult {
    pub trace: Vec<SimEvent>,
    pub opening: BTreeMap<String, Money>, // deviation 7
    pub journal: Vec<JournalEntry>,       // deviation 7
    pub ledger: LedgerSnapshot,
    pub invariants: Vec<InvariantResult>,
    pub trace_hash: String,
}
pub fn run(initial_ledger: &[(String, i64)], workload: &[SimEvent], seed: u32,
           fault_plan: Option<&FaultPlan>, new_handler: &dyn Fn() -> Box<dyn EventHandler>) -> Result<RunResult, SimError>;
```
Flow:
1. `Ledger::open` → `InvalidOpening`.
2. `apply_fault_plan(workload, plan)` → `InvalidFaultPlan`. The plan is `fault_plan`, or empty for `None` until F3 generates one.
3. Push the schedule's deliveries in order. The queue assigns `seq`.
4. Build the handler with `new_handler()`, then drain the queue. Before each event, rebuild the handler for every pending crash at or before its time. Then `advance_to`, `handler.handle`, `post` each entry (→ `Posting { event }`), and append to the trace.
5. `check_all`, then `hash_run(&trace, ledger.journal())`, then build `RunResult` from `snapshot()`, `opening().clone()` and `journal().to_vec()`.

| Decision | Why |
|---|---|
| `&dyn Fn() -> Box<dyn EventHandler>`, not generic | sim-api picks naive or hardened at runtime from the request, so one compiled `run` is enough. It's a factory so that a crash-restart (`fault-injector-plan.md` F3) can build a fresh handler. Tests pass `&|| Box::new(CardHandler)`. |
| `seed` is bound as `let _ = seed;` with a one-line "why" comment, and no unused `Rng` is built | Being honest about deviation 5. It will seed `Rng` for fault-plan generation (§6.2). |
| `opening` and `journal` are cloned out of the `Ledger` | `Ledger` only lends them, and adding an `into_parts()` would touch B's file. That's one copy per run. |
| `RunResult` derives `PartialEq` and `Serialize`, not `Deserialize` | Tests compare whole results across runs, and sim-api serializes the result. Nothing reads one back yet (YAGNI). |
| `SimError::Clock` can't fire in V1 | Faults are applied before the drain and every delivery is pushed up front, so pops never go back in time. The variant guards V3, where workers push during the drain. E2's tests cover the error itself. |
| Crash-restarts are checked against each popped event's time | No `peek` is needed (E3), and a crash after the last delivery has no effect, which is correct: nothing is left to handle. |
| `SimError` hand-writes `Display` and `Error` | Same pattern as `LedgerError`. |

**Tests** use a test-only `CardHandler`: Captured → a `Capture` transfer `external:card`→`merchant` with intent `charge-{id}`, Refunded → `Refund`, everything else → no entries. The fixed workload has two same-time events and is out of time order in the slice. Clean-run tests pass `Some(&FaultPlan::new())`, not `None`: once faults land (`fault-injector-plan.md` F3), `None` means "generate a plan from the seed". Proptests use `crate::test_support::proptest_config()`.
- **Determinism smoke (the test that must never go yellow):** 100 runs with seed 42 all give an identical `RunResult` and `trace_hash`.
- **Golden full-run hash:** `run()` on the fixed workload gives a pinned `trace_hash`. This is the cross-process check, since the ×100 test runs in one process and can't see a per-process difference. Because the hash includes the journal, it also covers the handler and ledger path.
- The trace is in `(time, seq)` order, and same-time events keep their workload order.
- The clean workload ends with the expected balances, and every invariant passes. `opening` equals the input, and `journal` holds one entry per posted entry, in posting order.
- **Seam check:** a duplicated capture (new `EventId`, same `charge_id`) makes `single_capture_per_intent` fail while the others pass, end to end through `run()`.
- **Each fault breaks the naive `CardHandler` visibly:**
  - Duplicating the capture fails `single_capture_per_intent`.
  - Delaying the capture past the refund fails `refund_within_capture`.
  - A window-2 `Reorder` of capture and refund fails `refund_within_capture`.
  - Dropping the capture fails `refund_within_capture`.
- **Crash-restart:** combine `Duplicate(capture)` with a `CrashRestart` between the original and its copy.
  - A handler that remembers seen ids in memory passes without the crash and fails `single_capture_per_intent` with it.
  - A handler that checks `ledger.journal()` for the event's `source` passes both.
- Failure paths: an unbalanced opening gives `InvalidOpening`. A plan naming an unknown event gives `InvalidFaultPlan`, and nothing is posted. A handler that emits an unbalanced entry gives `Posting` with that event's id. An empty workload gives `Ok`, with an empty trace and the hash of empty input.

**Done when:** TODO.md's "Done for today when" holds. The gates are green, `run()` produces a stable `trace_hash` (both the golden and the ×100 tests), and none of the forbidden items are in `sim-core`.

## E7: Docs sync
Done on 2026-10-03. README §6.1/§6.5 (the `run()` signature, the composite `EventKind`, `rails/` paths, ms ticks, `hash_run`, `RunResult`'s new fields, `HandlerKind::build`) and every TODO.md box for this plan are synced, and `v1-mvp-plan.md` marks feature 1 done and gap 1 resolved.

---

## Files
- Modify: `crates/sim-core/src/rng.rs`, `clock.rs`, `event.rs`, `trace.rs`, `handlers/mod.rs`, `simulator.rs`. In E7: `TODO.md`, `README.md`, `specs/v1-mvp-plan.md`.
- Only `cargo fmt` touches: `shrink.rs`, `handlers/naive.rs`, `handlers/hardened.rs`.
- Not touched: B's files (`money`, `ledger`, `invariants`, `rails/*`), `lib.rs`, `Cargo.toml` (no new deps).

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every piece.
- `grep -rn "HashMap\|HashSet\|f64\|tokio\|rand::\|unimplemented!" crates/sim-core/src` finds nothing except the `From<f64>` doc comment in `money.rs`.
