# Plan: Sweep and shrink (sim-core `sweep.rs`, `shrink.rs`)

## Context
README §3 V1 needs a **sweep harness**, a naive-vs-hardened failure-rate chart, and a **single-pass greedy shrinker** (§6.3). This is S3 in `v1-mvp-plan.md`, and Person B's track in `v1-backend-task-split.md`, which gives sweep its own `sweep.rs`.

Everything both of them need now exists:
- `run()` with a handler factory, plus seed → plan generation (engine E6, fault F3);
- `HandlerKind::build`;
- the three scenarios, each with a story plan.

The frontend contract (`frontend-plan.md`) fixes what sim-api will expose:
- **Sweep:** `POST /sweep { scenario_id, seed_start, count }` → `{ count, naive: { failed }, hardened: { failed } }`.
- **Shrink:** `POST /shrink { scenario_id, seed, handler, fault_plan, invariant }` → `{ original, shrunk, invariant, candidates_tried, run }`.

**Sweep comes first.** It's the simpler piece, it checks everything already built (hardened should never fail on any generated plan), it tunes F2's generation rates, and it supplies the multi-fault failing plans the shrinker needs as test fixtures. Story plans have one fault each, so there's nothing to shrink in them.

**Status (2026-10-03):** all pieces (SW1, SW2, SK1, SK2, SD) are done on branch `shrink-sweep`. SK2's sweep fixtures each shrink from 3 faults to 1. SW2's sweep over seeds 0..1,000: hardened fails **0** runs on every scenario. Naive fails 53.2% on `charge-retry`, 61.8% on `refund-before-capture` and 39.9% on `late-ach-return`, all inside the 10–90% band, so F2's rates stay as they are.

## The principle behind most decisions below
**Both are thin loops over `run()`, with a pure, testable core.** The sweep's core counts failing runs over a seed range for any handler factory. The shrinker's core is a greedy pass over a plan, driven by a "does it still fail?" predicate. Neither knows about HTTP or scenarios: like `run()`, they take `initial_ledger` and `workload` slices, because sim-core can't depend on sim-scenarios. sim-api destructures a `Scenario` before calling them.

## Deviations from README / TODO.md (CLAUDE.md requires flagging these)
1. **`sweep()` lives in `sweep.rs`, not `simulator.rs`.** README §6.5 lists `simulator.rs  run(), sweep()`. Already decided in `v1-backend-task-split.md`, which gives sweep its own file. README §6.5 is synced in SD.
2. **The caps live in sim-core as named constants** (`MAX_SWEEP_SEEDS`, `MAX_SHRINK_FAULTS`), and both functions reject input over them. `v1-mvp-plan.md` S4 puts the caps in sim-api. Keeping them in sim-core gives one source of truth, and fails fast at the boundary closest to the invalid state; sim-api only maps the errors to 4xx.

## Pieces
Branch `shrink-sweep`, one commit per piece. Each is done when its tests pass and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

| Piece | What | Depends on |
|---|---|---|
| SW1 | `sweep()` and its counting core (`sweep.rs`) | — |
| SW2 | Sweep the real scenarios: hardened never fails, naive does. Record the rates and tune F2's if they're degenerate. | SW1 |
| SK1 | The greedy shrink core, over a predicate (`shrink.rs`) | — |
| SK2 | `shrink_run()`: the predicate wired to `run()`, plus scenario fixtures | SK1, SW2 (fixtures) |
| SD | Docs sync | SK2 |

SK1 doesn't depend on the sweep and could land in parallel. It's listed after SW2 only because sweep comes first.

---

## SW1: `sweep()` (`sweep.rs`)
```rust
pub const MAX_SWEEP_SEEDS: u32 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SweepResult { pub runs: u32, pub naive_failed: u32, pub hardened_failed: u32 }

pub enum SweepError {
    TooManySeeds { count: u32, max: u32 },
    SeedOverflow,                          // the last seed, seed_start + count - 1, passes u32::MAX
    Run { seed: u32, error: SimError },
}

/// Naive and hardened over seeds seed_start..seed_start + count, each with its seed's generated plan.
pub fn sweep(initial_ledger: &[(String, i64)], workload: &[SimEvent], seed_start: u32, count: u32)
    -> Result<SweepResult, SweepError>;

/// The core: how many of those runs fail any invariant, for one handler factory.
pub fn count_failing_runs(initial_ledger: &[(String, i64)], workload: &[SimEvent], seeds: impl IntoIterator<Item = u32>,
    new_handler: &dyn Fn() -> Box<dyn EventHandler>) -> Result<u32, SweepError>;
```
`lib.rs` gains `pub mod sweep;`.

| Decision | Why |
|---|---|
| `sweep()` takes `seed_start` and `count`, and the core takes any sequence of seeds | It matches the API. Overflow means the *last* seed would pass `u32::MAX`, so a one-seed sweep at `u32::MAX` is valid, which a `Range<u32>` can't express. |
| A run "fails" if any invariant fails | That's the roadmap's definition, and what the chart shows. A per-invariant breakdown is YAGNI until the UI wants it. |
| The handler pair is fixed in `sweep()`, but `count_failing_runs` takes any factory | The API compares exactly naive and hardened. The core stays testable with test handlers. |
| `run()` errors stop the sweep and name the seed | Generated plans are always valid, so an error means a handler posted a broken entry. That's a bug to surface, not a failure to count. |

**Tests** (inline workloads in sim-core, no scenarios):
- **Deterministic:** the same arguments give the same result.
- **Counts:** `runs == count`, and `0 <= failed <= runs`.
- **Real failures:** on a capture-then-refund workload over seeds 0..200, naive fails on some seeds and hardened on none.
- **A handler that never posts** fails no runs.
- **Edges:** `count == 0` gives zero runs. `count > MAX_SWEEP_SEEDS` gives `TooManySeeds`. `seed_start = u32::MAX` works with `count = 1` and gives `SeedOverflow` with `count = 2`.
- **Errors:** a handler that posts an unbalanced entry gives `Run { seed, .. }` for the first seed.

## SW2: sweep the real scenarios (`sim-scenarios/tests/sweep.rs`)
- **For every scenario, over seeds `0..MAX_SWEEP_SEEDS`:** hardened fails **0** runs, and naive fails at least one. This is V1's claim that "naive fails often and hardened never", checked against every generated plan. It's the test that would have caught the ACH-return gap on its own.
- **Record the naive failure rate per scenario** in this spec's status line. F2 calls its rates first guesses. If a scenario's naive rate is degenerate (under 10% or over 90%), propose new rates before tuning, because the chart needs contrast. Generation isn't part of the replay contract, so tuning breaks no links.

## SK1: the greedy shrink core (`shrink.rs`)
```rust
/// README §6.3's V1 shrinker: one greedy pass, front to back. Each fault of the
/// original plan is tried for removal exactly once, and the removal is kept if
/// the plan still fails. Not 1-minimal: never call the result "minimal".
pub fn shrink_plan<E>(plan: &[FaultOp], still_fails: impl FnMut(&[FaultOp]) -> Result<bool, E>)
    -> Result<(FaultPlan, usize), E>; // (shrunk plan, candidates tried)
```
How the pass works:
- Keep a working copy. At index `i`, try the copy without fault `i`.
- If it still fails, keep the removal and stay at `i`, since the next fault has shifted into place. Otherwise move to `i + 1`.
- So `candidates_tried == plan.len()`.

| Decision | Why |
|---|---|
| Driven by a predicate, not `run()` | The algorithm is testable with fake predicates, and `run()` comes in SK2. |
| The predicate can fail (`Result<bool, E>`) | A candidate run can return a `SimError`, for example from a buggy handler. It's propagated, never treated as "doesn't fail". |
| Front to back, single pass | That's the V1 definition (§6.3). Full ddmin, with chunks and simplifying what's left, is V2. |

**Tests** (fake predicates):
- **Keeps exactly what's needed:** `[A, B, C, D]`, failing iff the plan contains B and D, shrinks to `[B, D]` in original order, with 4 candidates tried.
- **Each fault is tried once:** the predicate is called `plan.len()` times.
- **Edges:** an empty plan gives `([], 0)`. If every fault is needed, the plan comes back unchanged.
- **The honest limit (§6.3), so nobody claims "minimal":** with a predicate that fails for {A, B, C}, {A, C} and {C} but not for {B, C} or {A}, the pass returns `[A, C]`, though `[C]` also fails. A is tried before B's removal makes it removable.
- **Errors:** a predicate error propagates.

## SK2: `shrink_run()` (`shrink.rs`)
```rust
pub const MAX_SHRINK_FAULTS: usize = 500;   // README §6.3's ~500 candidate runs; greedy tries one per fault

pub struct ShrinkResult {
    pub original: FaultPlan,
    pub shrunk: FaultPlan,
    pub invariant: &'static str,   // the run's own name, so no lifetime ties to the request
    pub candidates_tried: usize,
    pub run: RunResult,            // the shrunk plan's run, for "Load reduced run"
}
pub enum ShrinkError {
    PlanTooLong { len: usize, max: usize },
    UnknownInvariant(String),
    DoesNotFail(&'static str),     // the original plan doesn't fail that invariant
    Run(SimError),
}

pub fn shrink_run(initial_ledger: &[(String, i64)], workload: &[SimEvent], seed: u32,
    plan: &[FaultOp], new_handler: &dyn Fn() -> Box<dyn EventHandler>, invariant: &str)
    -> Result<ShrinkResult, ShrinkError>;
```
Flow:
1. Reject a plan over `MAX_SHRINK_FAULTS`.
2. Run the original. The named invariant must be in its results (`UnknownInvariant` otherwise) and must fail (`DoesNotFail` otherwise).
3. Shrink with the predicate "this candidate's run fails the *same named* invariant".
4. Run the shrunk plan once more for `ShrinkResult.run`.

| Decision | Why |
|---|---|
| The plan is explicit, never `None` | The shrinker works on data (§6.2). The UI always holds the effective plan, and sim-api resolves `null` with `generate_fault_plan` first. `seed` is passed through only because `run()` takes one; with an explicit plan it changes nothing. |
| A fresh handler for every candidate | The factory gives each run a clean handler, which roadmap gap 2 needed. |
| One more run at the end, instead of caching | Simpler, and `run()` is deterministic, so it's the same result. |
| `candidates_tried` counts candidate plans only | The original check and the final run aren't candidates. It matches what the UI shows ("N candidates tried"). |

**Tests:**
- **Inline, in sim-core:**
  - A plan of `[Duplicate(capture), Delay(refund), Drop(unrelated)]` that fails `single_capture_per_intent` under naive shrinks to `[Duplicate(capture)]`, with 3 candidates tried, and the result's run still fails that invariant.
  - `DoesNotFail`, `UnknownInvariant` and `PlanTooLong` each fire.
  - It's deterministic.
- **On the real scenarios** (`sim-scenarios/tests`):
  - **A story plan doesn't shrink:** shrinking it on its first failing invariant returns it unchanged. Every story fault is needed.
  - **Sweep fixtures:** for each scenario, take the first naive-failing seed whose plan has at least 2 faults, and shrink it on its first failed invariant. The shrunk plan still fails that invariant and is no longer than the original.

## SD: docs sync
- **README §6.5:** `sweep.rs  sweep(), count_failing_runs()`, and `shrink.rs  shrink_plan(), shrink_run() (V1: single-pass greedy, see §6.3)`. `simulator.rs` becomes just `run()`.
- **`v1-mvp-plan.md`:** features 7 and 8 and S3 are done, with the recorded sweep rates.
- **`frontend-plan.md`:** no contract change. `SweepResult` and `ShrinkResult` map one-to-one onto the existing DTOs.
- **This spec:** the status line.

## Files
- New: `crates/sim-core/src/sweep.rs`, `crates/sim-scenarios/tests/sweep.rs` and `crates/sim-scenarios/tests/shrink.rs`.
- Modify: `crates/sim-core/src/shrink.rs` (empty today), and `lib.rs` (`pub mod sweep;`). SD: `README.md`, `specs/v1-mvp-plan.md`.
- Possibly, in SW2 and only if the rates are degenerate and you approve: the generation constants in `fault.rs`.
- No new dependencies.

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every piece.
- SW2's sweep reports hardened at 0 failures on every scenario.
- SK1's honest-limit test documents why the UI says "reduced", never "minimal".
