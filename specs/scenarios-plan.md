# Plan: Scenarios (`sim-scenarios`)

## Context
README §3 V1 needs 3 playable scenarios: 2 card and 1 ACH (`decisions-log.md` D2). `sim-scenarios` is still the cargo `add()` template, with no dependencies. This is S1a in `v1-mvp-plan.md`, and Person B's track in `v1-backend-task-split.md`.

**Does it need `run()`? No, except for one piece.** A scenario is data: an opening ledger, a workload and (decision P) a story plan, plus a registry. C1–C3 build and test against existing `sim-core` API only: `Ledger::open`, `apply_fault_plan`, `AchState::transition` and `HandlerKind::build`. Only C4, which proves end to end that naive goes red and hardened stays green, calls `run()`. That landed with engine E6 (`3f423e5`), so nothing here is blocked.

**Status (2026-10-03):** decision P is approved (`story_plan`). C1–C5 are done on branch `scenarios`. Every story-table prediction below held in C4's end-to-end tests.

## The principle behind most decisions below
**A scenario's baseline is clean, and every red comes from a fault.** Under an empty plan, both handlers pass every invariant on every scenario. Each scenario's bug comes from a small explicit fault plan, its story plan. That way the UI can show which fault broke what, the shrinker has something to work on, and it matches V1's definition: "inject a fault, watch it break the naive handler".

## Decision P: how a scenario shows its bug (approved by the user, 2026-10-03)
D3 says scenario 1's retry is a provider-side `Duplicate` of the capture webhook. A redelivery keeps the capture's own `EventId`, and `apply_fault_plan` requires unique workload ids, so the duplicate can only come from a fault, not from the workload. The same holds for scenario 3's redelivered return. V1's acceptance check ("run scenario 1 under naive → `single_capture_per_intent` goes red", `v1-mvp-plan.md`) therefore needs a fault in the run.

**A. Each scenario carries a `story_plan: FaultPlan`**
- ✅ One click shows the bug: the UI loads the story plan when a scenario is picked, and V1's acceptance check is deterministic.
- ✅ Baselines stay clean, so every failure traces to a fault the UI can name and the shrinker can remove.
- ❌ Deviates from README §6.1's `Scenario` struct, and `GET /scenarios` (frontend ask #2) gains the field. The UI needs a "load story plan" path next to "generate from seed".

**B. Bake the bug into the workload**
- ✅ No new field.
- ❌ Only possible for scenario 2, by putting the refund first. Scenarios 1 and 3 need a same-id redelivery, which a workload can't hold. Modelling scenario 1 as a new-id client retry instead contradicts D3.
- ❌ The baseline is dirty, and its "fault" is invisible to the UI and the shrinker.

**C. Describe the story in prose, and the user adds the fault by hand**
- ✅ No code.
- ❌ The demo depends on the user knowing which op to add to which event, and the acceptance check isn't one click.

**Chosen: A.**

## Deviations from README / TODO.md (CLAUDE.md requires flagging these)
1. **`Scenario` gains `story_plan: FaultPlan`** (decision P). README §6.1 doesn't have it. Approved by the user.

## The three scenarios
Times are simulated ms, written with named constants (engine decision T). Every opening has zero balances on the three accounts the handlers post to: `external:card`, `external:bank` and `merchant`. That sums to zero, so `Ledger::open` accepts it. External accounts are counterparties and may go negative.

| id (frozen: replay links store it) | Workload | Story plan | Naive under the story plan | Hardened under the story plan |
|---|---|---|---|---|
| `charge-retry` | 1: card Authorized, charge 1, $50, at 0. 2: Captured $50 at 2 s. 3: Refunded $10 at 1 h. | `Duplicate { event_id: 2 }`: the capture is redelivered 30 s later | Fails `single_capture_per_intent` and `single_entry_per_source_event` | Passes all: `already_posted` skips the copy |
| `refund-before-capture` | 1: card Authorized, charge 2, $80, at 0. 2: Captured $80 at 1 s. 3: Refunded $80 at 1.5 s. | `Reorder { event_id: 2, window: 2 }`: the refund arrives at 1 s and the capture at 1.5 s | Fails `refund_within_capture` | Passes all: it rejects the early refund (D4, temporary) |
| `late-ach-return` | 1: ACH Initiated, entry 1, $200, at 0. 2: Batched at 4 h. 3: Settled at 1 day. 4: Returned (R01) $200 at 3 days. | `Duplicate { event_id: 4 }`: the return is redelivered | Fails `refund_within_capture` and `single_entry_per_source_event` | Passes all: `already_posted` skips the copy |

What each description must say:
- **Scenario 1:** it models a provider redelivery, *not* a client idempotency-key retry (D3; V4 models those).
- **Scenario 2:** the hardened handler drops the early refund, and no V1 invariant notices. That's #5, reconciliation, in V4 (D4).
- **Scenario 3:** ACH returns can land days after settlement.

## Pieces
Branch `scenarios`, one commit per piece. Each is done when its tests pass and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

| Piece | What | Depends on | Needs `run()`? |
|---|---|---|---|
| C1 | Crate wiring, `Scenario`, registry, validation tests, scenario 1 | Decision P | No |
| C2 | Scenario 2 | C1 | No |
| C3 | Scenario 3 (ACH), plus an ACH lifecycle test | C1 | No |
| C4 | End-to-end story tests through `run()` | C1–C3, engine E6 (done) | **Yes** |
| C5 | Docs sync | C4 | No |

---

## C1: crate, registry, validation, scenario 1
```rust
// sim-scenarios/src/lib.rs
#[derive(Debug, Clone, PartialEq)]
pub struct Scenario {
    pub id: &'static str,                    // frozen: replay links store it
    pub name: &'static str,
    pub description: &'static str,
    pub initial_ledger: Vec<(String, i64)>,  // the shape Ledger::open takes
    pub workload: Vec<SimEvent>,
    pub story_plan: FaultPlan,               // decision P
}
pub fn scenarios() -> Vec<Scenario>;   // fixed order, which the UI lists them in
pub fn find(id: &str) -> Option<Scenario>;
```
- **Crate wiring:** `Cargo.toml` gains `sim-core = { path = "../sim-core" }`, and the `add()` template is deleted.
- **Private helpers in `lib.rs`:**
  - `MS_PER_SECOND`, `MS_PER_HOUR` and `MS_PER_DAY`;
  - the three account-name constants, and `fn opening()`;
  - `fn event(id, time, kind) -> SimEvent`, which sets `seq: 0`, since the queue assigns `seq` (engine deviation 6).
- **Modules:** one file per scenario, with README §6.5's names: `scenario1_retry.rs`, `scenario2_refund_order.rs` and `scenario3_late_return.rs`. Each has `pub(crate) fn scenario() -> Scenario`.

| Decision | Why |
|---|---|
| Scenarios are built by functions, not statics | `Scenario` owns `Vec`s and `String`s, so a function returns a fresh copy. There's no global state (CLAUDE.md) and no lazy-initialization dependency. The cost is a few small allocations per `GET /scenarios`. |
| No serde in `sim-scenarios` | sim-api owns the wire DTO, deriving `accounts` from `initial_ledger`. `SimEvent` and `FaultOp` already serialize. |
| Account names are local constants, checked against the handlers | This avoids editing `handlers/mod.rs`, which is outside this track. A test proves the names match what the handlers post to. |
| Ids are kebab-case and frozen | Replay links store them (§6.2), just as invariant names are frozen. |

**Tests.** Each one loops over `scenarios()`, so C2 and C3 are covered as they land:
- **Registry:** ids are unique, `find` returns each scenario by id and `None` for an unknown id, and the order is fixed.
- **Opening:** `Ledger::open` accepts it, so it's balanced with no duplicate accounts.
- **Plans apply:** `apply_fault_plan` succeeds with both the empty plan and `story_plan`. That proves the workload ids are unique, every target exists, and nothing overflows.
- **There is a story:** `story_plan` isn't empty.
- **Accounts match the handlers:** for each event, `HandlerKind::Naive.build().handle(&event, &ledger)` is called against the opened ledger, and every posting's account must be in the opening. Naive posts on every money event, and nothing calls `run()`.

## C2: scenario 2
`refund-before-capture`, as in the table. The registry tests cover it with no new test code.

## C3: scenario 3
`late-ach-return`, as in the table. One new test:
- **ACH lifecycle:** for each ACH entry, its events in time order start with `Initiated`, and each next event is a legal `AchState::transition` (`rails/ach.rs`).

## C4: end-to-end story tests (needs `run()`)
For every scenario, through `run()` with `&|| kind.build()` factories and explicit plans:
- **Empty plan:** both handlers pass every invariant. This tests the principle.
- **Story plan:** naive fails *exactly* the invariants in the table, and hardened passes all of them.

Hardened under *generated* plans is left to the sweep (S3). One gap this surfaced is fixed: hardened's ACH `Returned` branch used to post without checking for a capture, so a `Drop` of scenario 3's `Initiated` turned hardened red on `refund_within_capture`. It now rejects such a return, as it does a card refund before its capture (`19fdf3f`), and `tests/stories.rs` has an end-to-end regression test.

## C5: docs sync
- **README §6.1:** `Scenario.story_plan`, once decision P is approved. §6.5's file names already match.
- **`v1-frontend-tasks.md`:**
  - ask #2: `GET /scenarios` also returns `story_plan`;
  - the UI loads the story plan when a scenario is picked, with "reset to story plan" next to "reset to seed plan";
  - the fixtures cover 3 scenarios.
- **`v1-mvp-plan.md`:** feature 9 and S1a are done.

## Files
- Modify: `crates/sim-scenarios/Cargo.toml` and `src/lib.rs`.
- New: `src/scenario1_retry.rs`, `src/scenario2_refund_order.rs` and `src/scenario3_late_return.rs`.
- C5: `README.md`, `specs/v1-frontend-tasks.md` and `specs/v1-mvp-plan.md`.
- No changes to `sim-core`.

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every piece.
- C1–C3 are green without `run()`. C4 runs once E6 lands, and its results must match the story table above.
