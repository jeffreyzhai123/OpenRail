# CLAUDE.md

Any agent working on this project must follow coding guidelines and explicit rules. Any deviation from the rules must be higlighted and explicity approved by user.

Rails Sim Playground: a deterministic, in-browser payments simulator. **README.md is the design source of truth.** Read the relevant section before implementing anything.

## Hard rules (break these and replay-by-seed breaks)
- `sim-core` has **no async, no Tokio, no I/O, no threads**. Only `sim-api` may use Tokio/Axum.
- Money is `Money(i64)` cents with checked arithmetic. **Never `f64`**, and no `From<f64>`.
- RNG is hand-rolled and seeded (mulberry32-style) in `rng.rs`. **Do not add the `rand` crate.**
- `EventQueue` orders strictly on `(time, seq)`, never on payload contents.
- **No `HashMap`/`HashSet` iteration in anything that affects output or the trace hash.** Rust's default hasher is randomized per process. Use `BTreeMap` (e.g. `Ledger.accounts`, see README §6.5).
- `InvariantResult` carries a stable **name**. The shrinker matches on the *same named* invariant.

## Coding Guidelines
### Structure & types
* YAGNI/KISS: simplest thing that works, readable over clever
* Single responsibility per function/type, independently testable
* Encode invariants in types where practical; assert!/debug_assert! the ones that can't be type-encoded (the 6 ledger invariants are the canonical example here)
* Orchestration (sim-api) stays thin; domain rules live in testable sim-core logic — mirrors the crate boundary already in place
* Services are stateless by default; mutable state only when it's genuinely part of the domain
* Small, mostly-private interfaces; expose the minimum
* Composition over inheritance
* Early returns over nested conditionals
* No global mutable state in sim-core — no static mut, Lazy/OnceCell singletons, or process-global counters/caches. Anything that varies or accumulates (an ID generator, a cache) is a struct field, constructed explicitly and passed in.

### Ownership & errors
* Borrow by default; .clone() only with a clear reason, not to dodge the borrow checker
* Result/Option + ?; no unwrap()/expect() outside tests unless an invariant makes failure provably impossible
* Fail fast with clear errors at the boundary closest to the invalid state; low-level code reports, the boundary decides how to recover
* sim-api (and only sim-api) must handle real network failure modes: timeouts, malformed responses, retries, partial failures
* No hidden side effects — a function that mutates, does I/O, or reaches a clock should look like it does

### Naming & comments
* Precise, intent-revealing names; no magic numbers — name domain constants
* Comments explain why, kept terse; obvious code needs no comment

### Testing
* Test behavior, not implementation — should survive refactors
* Cover failure paths explicitly: invalid input, duplicates, reordering, timeouts 
* Tests must be as deterministic as the code they test: no wall-clock, no uncontrolled randomness, no live network, unless the test's whole point is that integration
* The determinism check (same seed → identical trace hash) is the one test that must never go yellow

### Process
* Minimal dependencies — add a crate only when it materially cuts complexity (why the PRNG and hashing are hand-rolled/small rather than pulling in rand's full surface)
* cargo fmt, cargo clippy, cargo test on every change, not just before merge
* Optimize from profiling evidence only, never speculatively

## Commands
```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check

npm --prefix frontend run check   # typecheck, eslint, prettier --check, vitest
```
