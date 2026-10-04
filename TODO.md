## Today: 2026-09-30, sim-core foundation (V1 step 1)

> Current status lives in `specs/` (each plan has a Status section). Boxes below are ticked as their work merges.

### Step 0: both partners, together (~15 min, before splitting)
- [x] Remove the `add()` stubs from `sim-core/src/lib.rs` and declare the modules below in `lib.rs`.
- [x] Add deps to `sim-core`: `serde` (derive), `serde_json`, `blake3`; dev-dep `proptest`.
- [x] Agree on the shared types (README §6.1) and commit them as stubs so both sides compile:
      `EventId`, `SimEvent`, `EventKind`, `InvariantResult { name, passed, message }`, `LedgerSnapshot`.
- [x] Merge order: B's `money.rs` lands first, since A's `simulator.rs` consumes `Ledger`/`InvariantResult`.

### Partner A: deterministic engine
- [x] `rng.rs`: seeded PRNG from a `u64` seed (define how the seed folds into the 32-bit state); `next_u32`, `next_range`, `shuffle`. Test: same seed gives an identical sequence, and different seeds diverge.
      - Landed with a `u32` seed and no fold (`specs/deterministic-engine-plan.md` decision W).
- [x] `clock.rs`: `VirtualClock` with `now()` and `advance_to(t)`. Advancing backwards panics or returns an error.
- [ ] `event.rs`: `SimEvent`, `EventQueue` (BinaryHeap, min-ordered on `(time, seq)`, auto-incrementing `seq`). Test: tie-break by insertion order; proptest that pops come out in non-decreasing `(time, seq)`.
- [x] `trace.rs`: `hash_trace(&[SimEvent]) -> String`, canonical JSON then blake3. Test: stable across runs, and it changes when any event changes.
      - Landed as `hash_run(trace, journal)`, covering the journal too (decision H).
- [ ] `simulator.rs`: `RunResult` and a stub `run(scenario, seed, fault_plan)` that drains the queue, advances the clock, posts to the ledger, runs invariants, and returns the trace hash. Use a hard-coded in-test workload (the scenarios crate is out of scope today).
- [ ] Determinism smoke test: `run()` with the same seed ×100 gives an identical `trace_hash`.

### Partner B: money and ledger domain
- [x] `money.rs`: `Money(i64)`, `checked_add`/`checked_sub` (or `Add`/`Sub` returning `Option`/`Result`), `Display` as dollars.cents, serde. No float conversions.
- [x] `ledger.rs`: `Ledger { accounts: BTreeMap<String, Money> }`; `post(JournalEntry)` rejects unbalanced postings and overflow; `snapshot() -> LedgerSnapshot`.
- [x] `invariants.rs`: `InvariantCheck` trait + the invariants in README §6.4:
      - Fully implement #1 (ledger balances) and #2 (no money created/destroyed).
      - Implement #3 (one capture per intent) and #4 (refund ≤ captured) if time allows.
      - Stub #5 and #6 by name (they need provider events and reconciliation, which come later).
      - See specs/ledger-plan.md for the design and the deviations from this list.
- [x] `ach.rs`: `AchState` (Initiated → Batched → Settled → Returned) + `transition()`; illegal transitions return `Err`. Test the full transition table.
- [x] Proptests: random balanced postings keep the ledger at sum 0; a random unbalanced posting is always rejected.

### Done for today when
- `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo fmt --check` all pass on main.
- The `run()` stub produces a stable `trace_hash` for a fixed seed.
- No Tokio, `rand`, `f64` money, or `HashMap` iteration in `sim-core`.

### Explicitly NOT today
Fault injection, naive/hardened handlers, scenarios, shrinker, sweep, sim-api routes, replay encoding, frontend, deploy.