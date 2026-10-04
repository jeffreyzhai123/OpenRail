# V1 backend task split (2 people)

No deadline assumed here — this is the task breakdown for a clean, well-designed,
runnable V1, split so two people can work without stepping on each other's files.
Sequence reflects actual dependency order, not a time estimate.

## Scope

Full V1 per README §3 and `v1-mvp-plan.md`, no cuts: engine (clock, RNG, queue,
trace, `run()`), fault injection including seed → plan generation, the money
ledger (already done), ACH brought into V1 (D2), naive/hardened handlers, three
scenarios, the greedy shrinker, the sweep harness, `sim-api`, and deploy.

**Not covered here: the frontend.** It proceeds on its own track per
`frontend-plan.md`, independently of this split. `frontend-plan.md` already
describes a mock-client mode that lets frontend work start without waiting on
`sim-api`, so the two tracks don't block each other.

## Principle

Split by **file**, not by feature-that-touches-shared-files, so merges never
collide. Where one person's work depends on the other's, it's a **function-call
dependency** — importing a finished, published module's public API — never a
shared file edit. Sequence within each person's list follows the real
dependency chain already laid out in `deterministic-engine-plan.md`,
`fault-injector-plan.md`, and `v1-mvp-plan.md`.

## Ownership

| Person | Track | Files |
|---|---|---|
| **A** | Engine + `sim-api` | `handlers/mod.rs`, `event.rs`, `simulator.rs`, `sim-api/*` |
| **B** | Fault generation + scenarios + handlers + shrink/sweep | `fault.rs`, `rails/ach.rs` (additive), `sim-scenarios/*`, `handlers/naive.rs`, `handlers/hardened.rs`, `shrink.rs`, `sweep.rs` (new) |

No file appears in both lists.

## Why `handlers/mod.rs` goes to A, but `naive.rs`/`hardened.rs` go to B

This looks like it splits one cohesive module (`handlers/`) across two people —
worth justifying given we deliberately merged the trait and its implementations
into one directory earlier precisely to keep them cohesive. The split holds up
for two separate reasons:

1. **No file-level conflict either way.** `mod.rs`, `naive.rs`, and `hardened.rs`
   are three distinct files; git merges at the file level, so this split has
   zero conflict risk regardless of who owns what. The directory grouping is
   about conceptual cohesion (one reader finds the whole handler story in one
   place); it says nothing about who should *write* which file.
2. **The trait's shape is an engine decision, not a handler-policy decision.**
   Every choice in `EventHandler`'s signature — returning `Vec<JournalEntry>`
   instead of mutating the ledger, `&mut self` for handler memory, a read-only
   `&Ledger` so a handler can query `journal()` — was decided *because of* what
   `run()` needs (centralized posting, crash-restart via a factory, durable
   idempotency from the journal). That's `deterministic-engine-plan.md` E5's own
   rationale table, and it's tightly coupled to E6 (`run()`). Whoever writes
   `run()` is the person with the full context to get the trait right the first
   time — splitting trait design away from the person who consumes it risks a
   signature that's convenient for a handler but awkward for `run()`, discovered
   only at integration.
3. **`naive.rs` and `hardened.rs` are domain policy, best owned together.**
   What each handler actually *does* (dedupe or not, how) is business logic, not
   engine mechanics — and the person deepest in fault/scenario context (B) is
   best positioned to calibrate both against the faults actually being thrown.
   Splitting these two further, one handler per person, buys no real
   parallelism (they're each small) and costs consistency: they're meant to be
   directly comparable — same scenarios, same invariants, same conventions for
   querying `ledger.journal()` — which is easier for one person holding both in
   their head at once than two people coordinating on two 20-line siblings.

So the rule in practice: **the interface goes with whoever owns the code that
calls it; the implementations go with whoever owns the domain context that
shapes them.**

## Person A — engine + `sim-api`

1. **E5 — `EventHandler` trait + `HandlerKind`** (`handlers/mod.rs`).
   Small, and it's the dependency B's handler work needs — land and publish
   this first so B isn't blocked. See rationale above for why it sits here.
2. **E3 — `EventQueue`** (`event.rs`). Independent of E5; `BinaryHeap<Reverse<SimEvent>>`,
   `push` assigns `seq`, no `peek`/`len` (YAGNI, per the existing spec).
3. **E6 — `run()`** (`simulator.rs`). The core piece: applies an explicit
   `FaultPlan` through B's `apply_fault_plan` (F1, already on the
   `fault-injector` branch — merge it first), drains the queue, posts to the
   ledger, runs `check_all`, hashes via `hash_run`. Needs E3 and E5 done, and F1
   merged.
4. **F3 — wire seed → fault-plan generation into `run()`** (`simulator.rs`).
   `None` now means "generate from the seed" instead of "no faults." Needs B's
   F2 (`generate_fault_plan`) to exist first — function-call dependency only.
5. **`sim-api`** (`sim-api/src/*`): `POST /run`, `GET /scenarios`, `POST /shrink`,
   `POST /sweep`, replay `encode_run`/`decode_run`. Needs B's scenarios (S1a),
   handlers (S1b), shrink and sweep (S3) to exist as callable library functions
   — again, imports only, no shared files. This is the natural extension of
   owning `run()` and `RunResult`.
6. **Deploy (D1: Fly.io)** — Dockerfile, deploy config, smoke test against the
   deployed `sim-api`. Last step; whoever's free does it, but it's listed under
   A since it's a direct extension of the `sim-api` work.

## Person B — fault generation, scenarios, handlers, shrink + sweep

1. **Merge F1** (`apply_fault_plan`, already implemented on the
   `fault-injector` branch) into the integration branch, so both people build
   on the same base.
2. **F2 — `generate_fault_plan`** (`fault.rs`). Pure function, draws from
   `Rng::below`; needs only F1's types, independent of A's work.
3. **ACH into V1 (D2)** — add `Batched`/`Settled` variants to `AchEvent`
   (`rails/ach.rs`; currently only has `Returned`). Small, additive, no
   change to existing `AchState`/`AchReturnCode` code or tests.
4. **S1a — scenarios** (`sim-scenarios/*`): scenario 1 (charge retried after
   timeout), scenario 2 (refund before capture), scenario 3 (ACH return after
   settlement, using step 3's new events), plus the `scenarios()` registry.
   Independent of A's work — needs only existing `sim-core` types.
5. **S1b — `naive.rs` / `hardened.rs`** (`handlers/naive.rs`, `handlers/hardened.rs`).
   Needs A's E5 (the trait) merged first. Naive: posts on every event, no
   dedup. Hardened: dedupes durably from `ledger.journal()` (by `source` for a
   redelivered webhook — this is exactly what invariant #7,
   `single_entry_per_source_event`, now guards against — or by intent for a
   retried capture). D4 (reject a premature refund) applies here, flagged
   temporary per `decisions-log.md`. Both handlers need a match arm for
   `EventKind::Ach(AchEvent::Returned)` → a `Refund`-kind posting;
   `Batched`/`Settled` are no-ops (per the `rails/ach.rs` doc comment — they
   move no money and aren't separately invariant-checked).
6. **S3 — shrink + sweep.** `shrink.rs`: the greedy single-pass shrinker
   (try removing each fault once, keep the removal if the *same named*
   invariant still fails; report `candidates_tried`; never claim "minimal").
   **`sweep.rs` (new file):** the sweep harness (naive vs. hardened failure
   rate over a seed range). README's old file map put `sweep` inside
   `simulator.rs` (`Simulation::sweep()`), but that's stale — `simulator.rs`
   is a free function (`run()`), not a `Simulation` type, and `simulator.rs`
   is A's file. Giving sweep its own file keeps the zero-overlap split clean;
   sync README §6.5 once this lands. Both call A's `run()` as a library
   function only.

## Cross-person dependencies (function calls, never shared files)

| A's task | needs from B | B's task | needs from A |
|---|---|---|---|
| E6 (`run()`) | F1 merged | S1b (handlers) | E5 merged |
| F3 (seed-gen wiring) | F2 (`generate_fault_plan`) | — | — |
| `sim-api` routes | S1a, S1b, S3 all published | — | — |

Every row is "import the other person's finished module," never a joint edit —
land-and-publish-early (E5, F1, F2) is what keeps both tracks moving instead of
stalling on each other.
