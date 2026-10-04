# Plan: sim-api (Axum HTTP server)

## Context
README §3 V1 needs the core loop reachable from a browser: run a scenario, share the run as a link, shrink, sweep. `sim-api` is still `println!("Hello, world!")` with no dependencies. Every library function it wraps now exists and is tested:
- `run()` with seed → plan generation;
- `HandlerKind::build`;
- `sweep()` and `shrink_run()`;
- the three scenarios.

This is S4 in `v1-mvp-plan.md`, and Person A's track in `v1-backend-task-split.md`. The HTTP contract is already written down in `frontend-plan.md` ("API contract v1"), and the frontend is being built against it.

**Status (2026-10-03):** decisions V, B and G are approved. API1 is done on branch `sim-api`; API2 is next.

## The principle behind most decisions below
**sim-api is a thin, stateless, deterministic shell.** Every endpoint is a pure function of its request: it looks up the scenario, calls sim-core, and maps the result or error to the contract. Nothing is stored. So the same request always gets the same response bytes, which makes every endpoint safe to retry, POSTs included. The only async code in the workspace lives here (CLAUDE.md), and the simulation itself still runs synchronously.

## Decisions (approved by the user, 2026-10-03)
### V: how the replay encoding is versioned
README §6.2 says to version the encoding, and V2 adds compression.

**A. A prefix outside the payload: `1.` + base64url(JSON)**
- ✅ sim-api reads the version before decoding anything. V2's compressed payload isn't JSON, so it can't carry a version that's readable first.
- ✅ An unsupported version is a clear, cheap error, which the UI turns into its "this link's encoding version isn't supported" message.
- ❌ Two characters longer.

**B. A version field inside the JSON: `base64url({"v":1, …})`**
- ✅ One opaque blob.
- ❌ Breaks as soon as V2 compresses: the version would be inside the bytes you need the version to decode.

**Chosen: A.**

### B: base64url
**A. The `base64` crate**
- ✅ Small and dependency-free, and its decoding of untrusted input is battle-tested: padding, invalid characters, trailing bits.
- ❌ One more dependency (CLAUDE.md: add one only when it materially cuts complexity).

**B. Hand-rolled**
- ✅ No dependency, like the RNG.
- ❌ About 40 lines of decoding of *untrusted* URL input, which is where hand-rolled code goes wrong. Unlike the RNG, determinism gains nothing from owning it.

**Chosen: A.**

### G: where the frontend's fixtures come from (frontend ask #7)
**A. sim-api generates them, and frontend step 1 adopts them instead of hand-writing**
- ✅ The TS types are checked against real serde output from the start. The `AchEvent` drift below is exactly what hand-written fixtures hide.
- ✅ Frontend step 8's "replace the hand-written fixtures" disappears.
- ❌ A Rust test writes into `frontend/`, but only when `UPDATE_FIXTURES=1` is set. By default it only compares.

**B. The frontend keeps hand-written fixtures until step 8, as planned**
- ✅ No cross-tree writes.
- ❌ Drift isn't caught until go-live.

**Chosen: A.**

## Deviations from README / TODO.md / frontend-plan (CLAUDE.md requires flagging these)
1. **More files than README §6.5 lists:** `lib.rs` (so integration tests can drive the router), `app.rs`, `error.rs` and `dto.rs`, next to `main.rs`, `encode.rs` and `routes/*`. `main.rs` stays a few lines.
2. **`GET /health`**, an addition to the contract, for Fly.io's health check (D1, S5).
3. **The error codes become part of the contract** (table below). `frontend-plan.md` only fixes the envelope shape.
4. **A per-request plan cap, `MAX_PLAN_FAULTS = 100`,** stricter than the shrinker's 500. It applies to every request that carries a plan, so a replay link's path stays under about 6 KB. That's §6.2's "short-plan cap".
5. **Contract drift fix:** `frontend-plan.md`'s `AchEvent` type gains `Initiated`, `Batched` and `Settled`, which scenario 3 sends. Not an approval item, just a correction.

## Errors (the envelope: `{ "error": { "code", "message" } }`)
| Code | Status | When |
|---|---|---|
| `bad_request` | 400 | Malformed JSON, wrong types, unknown fields, a seed outside `u32` |
| `invalid_replay` | 400 | The encoded string isn't `<version>.<base64url>`, isn't base64url, or doesn't decode to a replay |
| `unsupported_encoding_version` | 400 | A well-formed prefix naming a version this server doesn't read |
| `unknown_scenario` | 404 | No scenario with that id |
| `not_found` / `method_not_allowed` | 404 / 405 | Unknown route or method |
| `payload_too_large` | 413 | A body over `MAX_BODY_BYTES` (64 KiB) or a replay string over `MAX_REPLAY_LEN` (16 KiB, so even a worst-case plan at the cap fits; realistic links are about 6 KB or less) |
| `invalid_fault_plan` | 422 | `FaultError`: an unknown event id, or a time overflow |
| `plan_too_long` | 422 | Over `MAX_PLAN_FAULTS` |
| `too_many_seeds`, `seed_overflow` | 422 | `SweepError` |
| `unknown_invariant`, `does_not_fail` | 422 | `ShrinkError` |
| `timeout` | 503 | Over `REQUEST_TIMEOUT` (10 s). The frontend's single retry on 503 can succeed if the cause was load. |
| `internal` | 500 | A `SimError` that means a server-side bug (`InvalidOpening`, `Posting`, `Clock`, `TraceEncoding`), or a panicked computation |

Every error goes through the envelope, including axum's own extractor rejections and its 404/405 fallbacks, so the frontend's decoder never sees plain text.

## How it handles real network failure modes (CLAUDE.md)
- **Timeouts:** a request timeout layer answers `timeout` after 10 s. Simulation work runs in `tokio::task::spawn_blocking`, so it never stalls the async workers. A timed-out computation finishes in the background, and the caps bound how long that can be.
- **Malformed input:** a body-size limit, strict deserialization (`deny_unknown_fields`) and validation at the boundary, all answered with 4xx envelopes.
- **Retries:** every endpoint is idempotent by construction (the principle above), so the frontend's single retry is always safe.
- **Partial failures:** each response is computed in full before any byte is sent. There's no streaming in V1 (shrink progress is deferred, frontend ask #8), so a client gets either a whole response or a whole error.
- **Shutdown:** on SIGTERM, the server stops accepting connections and finishes in-flight requests, for Fly deploys.

## Pieces
Branch `sim-api`, one commit per piece. Each is done when its tests pass and `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` are green.

| Piece | What | Depends on |
|---|---|---|
| API1 | Skeleton: dependencies, lib + bin, router, error envelope, CORS, config, graceful shutdown, `GET /health` | — |
| API2 | Replay encoding (`encode.rs`), pure | Decisions V, B |
| API3 | DTOs, `GET /scenarios`, `POST /run`, plus the body limit, request timeout and JSON-rejection mapping | API1, API2 |
| API4 | `GET /replay/{encoded}` | API3 |
| API5 | `POST /shrink` and `POST /sweep` | API3 |
| API6 | Golden fixtures (frontend ask #7) | API3–API5, decision G |
| API7 | Docs and contract sync | API6 |

---

## API1: skeleton
- **Dependencies:**
  - `axum`;
  - `tokio`, with the multi-threaded runtime, macros, networking and signals;
  - `tower-http`, for CORS;
  - `serde` and `serde_json`;
  - dev: `tower` (for `oneshot`) and `http-body-util`.

  `sim-core`, `sim-scenarios` and `base64` come with the pieces that use them. No logging framework: the startup line and 5xx causes go to stderr, which Fly captures.
- **`lib.rs`** exposes `app(allowed_origin) -> Router` and `Config`. **`main.rs`** reads `PORT` and `ALLOWED_ORIGIN`, parses them with `Config::parse`, binds `0.0.0.0:$PORT`, and serves with graceful shutdown on Ctrl-C or SIGTERM.
- **`config.rs`:** `PORT` defaults to 3000, the port the Vite dev proxy targets. `ALLOWED_ORIGIN` is the static frontend's single exact origin for CORS (frontend ask #6); an empty value or `*` is rejected. When it's unset there's no CORS, which suits local development, where the Vite proxy makes requests same-origin. A bad value fails at startup, before binding.
- **`error.rs`:** `ApiError { status, code, message }` implementing `IntoResponse` as the envelope. The 404 and 405 fallbacks use it. The `From` impls for sim-core errors come with the routes that need them.
- **Moved to API3:** the request timeout, the body limit and the JSON-rejection mapping only have something to act on once the first JSON compute route exists. Adding them here would be dead code.

**Tests** drive `app()` with `oneshot`, with no network:
- `GET /health` returns 200.
- An unknown route gives a 404 envelope, and a wrong method a 405 envelope.
- CORS allows the configured origin only, and a preflight for a POST is answered.
- `Config::parse`: defaults, valid values, and rejected ports and origins.

## API2: replay encoding (`encode.rs`)
```rust
pub const ENCODING_VERSION: u32 = 1;
pub struct Replay { pub scenario_id: String, pub seed: u32, pub handler: HandlerKind, pub fault_plan: FaultPlan }
pub fn encode_run(replay: &Replay) -> String;               // "1." + base64url_nopad(serde_json::to_vec(replay))
pub fn decode_run(encoded: &str) -> Result<Replay, DecodeError>;
pub enum DecodeError { Malformed, UnsupportedVersion(String), TooLong }
```
| Decision | Why |
|---|---|
| The plan inside is always explicit, the effective plan | §6.2: a link replays the run without regenerating anything, so tuning generation never breaks old links. |
| No compression in V1 | `v1-mvp-plan.md` S4. The plan cap keeps links short, and compression is §6.2's V2 target, versioned in by decision V. |

**Tests:**
- Encode then decode round-trips, including every `FaultOp` kind and both handlers.
- Encoding is canonical: the same replay gives the same string.
- The output contains only URL-safe characters.
- An unknown version prefix gives `UnsupportedVersion`.
- Bad base64, bad JSON and a missing prefix give `Malformed`, and an over-long string gives `TooLong`.

## API3: DTOs, `GET /scenarios`, `POST /run`
**Moved here from API1:** `MAX_BODY_BYTES` (axum's `DefaultBodyLimit`), and a JSON extractor that maps every rejection to the envelope: 413 → `payload_too_large`, anything else → `bad_request`. Plus one helper that runs simulation work in `spawn_blocking` under `REQUEST_TIMEOUT`, answering `timeout` when it elapses and `internal` if the work panics.

**DTOs** (`dto.rs`) serialize in the contract's field order:
- `RunResponse { scenario_id, seed, handler, fault_plan, trace, opening, journal, ledger, invariants, trace_hash, replay }`, built from `RunResult`, the request and `encode_run`;
- `ScenarioSummary { id, name, description, accounts, workload, story_plan }`, where `accounts` comes from `initial_ledger`'s names;
- requests with `deny_unknown_fields`.

**`POST /run { scenario_id, seed, handler, fault_plan | null }`:**
1. Look up the scenario, or answer `unknown_scenario`.
2. Check the plan's length against `MAX_PLAN_FAULTS`.
3. In `spawn_blocking`, call `run(…, fault_plan.as_ref(), &|| handler.build())`. With `null`, sim-core generates the plan from the seed.
4. Respond with the effective plan and its replay link.

**Tests:**
- `GET /scenarios` lists the 3 scenarios in registry order, with accounts and story plans.
- Each scenario's story plan under naive returns the red invariants from `scenarios-plan.md`'s table, and hardened returns all green.
- `fault_plan: null` returns `generate_fault_plan(seed, workload)` as the effective plan.
- The same request gives byte-identical responses.
- Errors: `unknown_scenario`, `invalid_fault_plan`, `plan_too_long`, and `bad_request` for an unknown handler name or a negative seed.
- Limits: an oversized body gives a 413 `payload_too_large` envelope, malformed JSON a 400 `bad_request` envelope, and work over `REQUEST_TIMEOUT` a 503 `timeout` envelope.

## API4: `GET /replay/{encoded}`
Decode the link, then answer exactly as `POST /run` would for the decoded request.

**Tests:**
- A `POST /run` response's `replay` string, fed to `GET /replay`, returns a byte-identical response, including `trace_hash`. That's the "verified identical" badge's guarantee.
- `unsupported_encoding_version`, `invalid_replay`, and `payload_too_large` for an over-long string.

## API5: `POST /shrink` and `POST /sweep`
- **`/shrink { scenario_id, seed, handler, fault_plan | null, invariant }`:**
  - a `null` plan is first resolved with `generate_fault_plan`;
  - then `shrink_run` in `spawn_blocking`;
  - the response is `{ original, shrunk, invariant, candidates_tried, run }`, where `run` is the shrunk plan's `RunResponse`, with its own replay link.
- **`/sweep { scenario_id, seed_start, count }`:** `sweep()` in `spawn_blocking`, returning `{ count, naive: { failed }, hardened: { failed } }`.

**Tests:**
- A shrink of a 3-fault sweep fixture returns 1 fault, `candidates_tried: 3`, and a run that still fails the invariant.
- Shrink errors: `does_not_fail`, `unknown_invariant`, `plan_too_long`.
- A sweep of 100 seeds on each scenario reports `hardened.failed == 0`.
- Sweep errors: `too_many_seeds` and `seed_overflow`.

## API6: golden fixtures (frontend ask #7)
A test builds each canonical response through `app()` and compares it, as pretty-printed JSON with a trailing newline, to `frontend/src/api/fixtures/*.json`:
- `scenarios.json`;
- `run-<scenario>-<handler>.json`, for each scenario's story plan under each handler (6 files);
- `replay.json`, `shrink.json` and `sweep.json`.

With `UPDATE_FIXTURES=1`, the test rewrites the files instead. Without it, a missing or different file fails the test, with a message saying to run it with `UPDATE_FIXTURES=1` and review the diff. The responses are deterministic, so the fixtures are stable byte for byte.

## API7: docs and contract sync
- **`frontend-plan.md`:**
  - `AchEvent` gains `Initiated`, `Batched` and `Settled`;
  - the error-code table and `GET /health` are added;
  - step 1 uses the generated fixtures (decision G);
  - ask #7 is done.
- **README:**
  - §6.5's sim-api file map;
  - §6.2: the `1.` prefix (decision V), and no compression in V1.
- **`v1-mvp-plan.md`:** S4 is done, and the dependency table's S4 row too.

## Files
- New:
  - `crates/sim-api/src/{lib.rs, app.rs, config.rs, error.rs, dto.rs, encode.rs}`;
  - `crates/sim-api/src/routes/{mod.rs, scenarios.rs, run.rs, replay.rs, shrink.rs, sweep.rs}`;
  - `crates/sim-api/tests/*`;
  - `frontend/src/api/fixtures/*.json` (API6).
- Modify: `crates/sim-api/Cargo.toml`, `src/main.rs`. API7: `README.md`, `specs/frontend-plan.md`, `specs/v1-mvp-plan.md`.
- No changes to `sim-core` or `sim-scenarios`.

## Verification
- `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test` pass after every piece. The HTTP tests use `oneshot`, with no network or wall clock.
- **Manual:**
  1. `cargo run -p sim-api`.
  2. `curl` `GET /scenarios`, then `POST /run` on `charge-retry` with its story plan under naive: `single_capture_per_intent` should be red.
  3. Paste the returned `replay` into `GET /replay/…`: it should give the same `trace_hash`.
- After frontend step 8: the walkthrough in `frontend-plan.md` with `VITE_SIM_CLIENT=http` against a local sim-api.
