# Plan: Lightweight interactive React frontend (`frontend/`)

## Context
README §3 V1 needs a minimal UI: a timeline, balances, an invariant panel, Run/Shrink/Share buttons, a sweep chart and replay-by-seed links. The frontend is React + TS and talks to a **stateless** `sim-api` over HTTP (§2, §6.5). Today the backend can't serve a run yet. `money`, `ledger`, `invariants` and `ach` are done, but `rng`, `clock`, `trace`, the handlers, `shrink` and `simulator::run` are empty or `unimplemented!()`, `sim-api` is still hello-world, and `sim-scenarios` is a placeholder. Node isn't installed either.

**Decisions made with the user:**
1. Write the **HTTP contract down now** and build the UI against checked-in JSON fixtures behind a `SimClient` interface. Swap in the real HTTP client once `sim-api` exists.
2. This plan covers **`frontend/` plus the contract only**. The backend changes the contract needs are listed (and flagged) for whoever owns `sim-api` / `sim-core`. This plan makes no Rust edits.

## Principles
- **The frontend displays results. It never decides them.** Invariants, shrinking, the replay encoding and fault-plan generation all live server-side. The browser holds inputs (scenario, seed, handler, fault plan) and renders `RunResponse`s.
- **Minimal dependencies**, matching CLAUDE.md. Runtime deps are `react` and `react-dom` only. No router, state library, chart library, CSS framework, schema library or compression library.
- **JS has no i64.** Cents, time, seq and event ids are `number`s that the decoder checks with `Number.isSafeInteger` and rejects at the boundary otherwise. The **seed travels as a decimal string**. `formatCents` uses BigInt and never divides as a float.
- **The network boundary handles failures** (`api/client.ts`). It applies timeouts, retries idempotent calls once, validates every response, and aborts superseded requests, so a slow old response can't overwrite a newer one.

## Deviations / backend asks (CLAUDE.md: each needs explicit approval)
| # | Ask | Why |
|---|---|---|
| 1 | Add a `handler: "naive" \| "hardened"` input to run, replay and shrink, and include it in `encode_run` | README §6.1's `run()` has no handler parameter, but V1's whole demo is naive breaking while hardened holds. The core side (`run()` takes `&mut dyn EventHandler`) is in `specs/partner-a-plan.md`. |
| 2 | New `GET /scenarios` → id, name, description, accounts, workload | It isn't in §6.5's route list. The UI needs the scenario list, and the fault editor needs workload event ids to target. |
| 3 | `RunResponse.fault_plan` is the **effective** plan, i.e. the seed-generated one when the request sends `null` | §6.2 treats the plan as explicit data. The UI must show that plan so the user can edit or shrink it. |
| 4 | `RunResponse` also exposes `opening` + `journal` (`JournalEntry` is already serde) | Needed for the timeline scrubber: "balances at step k". Today `RunResult` only has the final `LedgerSnapshot`. |
| 5 | Serialize the seed as a decimal **string** on the wire | `JSON.parse` silently rounds u64 values above 2^53, which would break replay-by-seed without any error. |
| 6 | CORS for the static frontend origin (`tower-http`), unless both are served from one origin | §2 deploys the API and the static frontend separately. |
| 7 | Golden-fixture test in `sim-api` (`UPDATE_FIXTURES=1` regenerates `frontend/src/api/fixtures/*.json`) | Keeps the Rust DTOs and the TS types in lockstep without a codegen dependency. |
| 8 | **V1 shrink shows no live progress.** It shows a busy state, then "N candidates tried" | §6.3 asks for progress in the UI. Live progress needs streaming (SSE), so it's deferred to V2's ddmin (≤500 runs). V1's greedy pass is a handful of runs. |
| — | *Nice-to-have:* `InvariantResult` gets an optional structured `at: { entry_index, event_id }`, and trace events record which fault touched them | Lets the UI jump the timeline to where a check broke and badge injected events. Until then the UI shows the message text only. |

## API contract v1 (`frontend/src/api/types.ts` mirrors serde output exactly)
```ts
type Cents = number; type EventId = number;                 // safe integers, checked at decode
type CardEvent =                                            // crates/sim-core/src/rails/card.rs
  | { Authorized: { charge_id: number; amount: Cents } }
  | { Captured: { charge_id: number; amount: Cents } }
  | { Refunded: { charge_id: number; amount: Cents } };
type AchReturnCode = "R01" | "R02" | "R03" | "R04" | { Other: string };
type AchEvent = { Returned: { entry_id: number; code: AchReturnCode; amount: Cents } }; // rails/ach.rs
type EventKind = { Card: CardEvent } | { Ach: AchEvent };   // one variant per rail, see event.rs
interface SimEvent { id: EventId; time: number; seq: number; kind: EventKind }
type FaultOp =                                              // serde's default externally-tagged form
  | { Duplicate: { event_id: EventId } } | { Reorder: { window: number } }
  | { Delay: { event_id: EventId; by: number } } | { Drop: { event_id: EventId } }
  | { CrashRestart: { at: number } };
interface JournalEntry { source: EventId; intent: string; kind: "Capture" | "Refund";
                         postings: { account: string; delta: Cents }[] }
interface InvariantResult { name: string; passed: boolean; message: string | null }
type Handler = "naive" | "hardened";

GET  /scenarios            -> { id, name, description, accounts: string[], workload: SimEvent[] }[]
POST /run    { scenario_id, seed: string, handler, fault_plan: FaultOp[] | null } -> RunResponse
GET  /replay/:encoded      -> RunResponse
POST /shrink { scenario_id, seed, handler, fault_plan, invariant: string }
             -> { original: FaultOp[]; shrunk: FaultOp[]; invariant; candidates_tried: number; run: RunResponse }
POST /sweep  { scenario_id, seed_start: string, count: number }
             -> { count; naive: { failed: number }; hardened: { failed: number } }   // counts; UI computes %
RunResponse = { scenario_id, seed: string, handler, fault_plan: FaultOp[], trace: SimEvent[],
                opening: Record<string, Cents>, journal: JournalEntry[],
                ledger: { accounts: Record<string, Cents> }, invariants: InvariantResult[],
                trace_hash: string, replay: string /* encode_run output */ }
Errors: 4xx/5xx with { error: { code: string, message: string } }
```
**Replay link:** `#r=<replay>&h=<trace_hash>`, kept in the fragment (§6.2), so static hosting needs no rewrites. On load, the UI calls `GET /replay/:r` and compares the returned `trace_hash` to `h`. The badge shows **verified identical**, **determinism break**, or **unverified** (no `h`). If the server can't decode `r` because it uses an old encoding version, the UI shows a clear "this link's encoding version isn't supported" message.

## Layout of `frontend/`
```
frontend/  package.json  vite.config.ts (dev proxy /api -> :3000)  tsconfig.json
           eslint.config.js  .prettierrc  .nvmrc (Node 24 LTS)  index.html
  src/main.tsx, App.tsx            layout + useReducer (no global store)
  src/api/
    types.ts                       contract above
    decode.ts                      hand-written guards: unknown -> T | DecodeError (safe-int checks)
    client.ts                      SimClient interface + HttpClient (AbortController timeouts,
                                   1 retry on network/502/503/504 only, error envelope -> ApiError)
    fixtureClient.ts               dev/test only; echoes the requested plan; shows a "FIXTURE DATA" banner
    fixtures/*.json                scenarios, run (naive+hardened × 2 scenarios), replay, shrink, sweep
  src/lib/                         pure, unit-tested, no React
    money.ts      formatCents()    BigInt, mirrors Rust Display ("-12.34"); UI adds the "$"
    seed.ts                        validate u64 decimal string; random seed via crypto.getRandomValues
    balances.ts   balancesAt(opening, journal, k)  display-only fold; the full fold must equal
                                   ledger.accounts or it throws (contract drift → error banner)
    faultPlan.ts                   describe / add / remove / update a FaultOp; shape checks only
    replayLink.ts                  build/parse the fragment
  src/state/appState.ts            reducer: inputs, request status, last run, selected step, shrink, sweep
  src/components/
    Controls.tsx        scenario picker + description, seed input + random button, handler toggle, Run / Share
    FaultPlanEditor.tsx list ops; add (type + fields, event pickers drawn from the workload); remove;
                        "reset to seed plan"; marks a plan as edited when it diverges from the seed
    Timeline.tsx        ordered trace (time, seq, kind, id), click or ←/→ to step, event detail
    BalancePanel.tsx    balances at the selected step, change vs the previous step highlighted
    InvariantPanel.tsx  pass/fail per named invariant + message; a "Shrink" button on each failure
    ReplayBadge.tsx, ErrorBanner.tsx
    ShrinkView.tsx      original vs reduced plan, candidates tried, "Load reduced run".
                        Labelled "reduced", never "minimal" (§6.3: V1 is greedy)
    SweepChart.tsx      hand-rolled SVG: naive vs hardened failure rate, plus a table fallback
  src/styles.css                   CSS variables, light/dark, responsive stack on narrow screens
```
Dev deps: `vite`, `@vitejs/plugin-react`, `typescript`, `vitest`, `jsdom`, `@testing-library/react` + `user-event`, `eslint` + `typescript-eslint` + `eslint-plugin-react-hooks`, `prettier`.
Scripts: `dev`, `build`, `typecheck`, `lint`, `fmt`, `fmt:check`, `test`, and `check` (all four gates, the frontend equivalent of fmt/clippy/test).
Client selection: `VITE_SIM_CLIENT=fixtures|http` and `VITE_API_BASE_URL`. Production builds always use `http`.

## Order of work (each step is its own small commit)
0. **Setup.** The user installs Node 24 LTS (`brew install node@24`). Scaffold with `npm create vite@latest frontend -- --template react-ts` and strip the template. Add the tooling and scripts. Add `node_modules/` and `frontend/dist/` to `.gitignore`. Add the frontend `check` command to CLAUDE.md's Commands section.
1. **Contract.** `types.ts`, `decode.ts`, hand-written fixtures (real invariant names from `crates/sim-core/src/invariants.rs`; openings that sum to 0, per `Ledger::open`), `SimClient`, `FixtureClient`, `HttpClient`.
2. **`lib/`.** money, seed, balances, faultPlan, replayLink, each with unit tests.
3. **Core loop.** Controls, Timeline, BalancePanel, InvariantPanel and the reducer, running against fixtures.
4. **FaultPlanEditor.** Edit, then re-run with the explicit plan.
5. **Share and replay.** Fragment, `GET /replay`, ReplayBadge, unsupported-version message.
6. **ShrinkView.**
7. **SweepChart.** Load the `dataviz` skill before writing the chart code.
8. **Go live** once sim-api ships asks #1–7: switch to `HttpClient`, replace the hand-written fixtures with the golden ones, deploy as a static site, and smoke-test against the real API (README V1's last item).

## Tests (Vitest, all deterministic: no live network, no wall clock, `fetch` stubbed)
- **decode:** every fixture decodes. Rejected: missing field, unknown `EventKind`/`FaultOp` tag, unsafe integer (`2**53`), numeric seed, an `invariants` that isn't an array.
- **client:** timeout aborts and returns a timeout error. One retry on 503, then success. No retry on 400, and the error envelope reaches the UI. Malformed JSON gives a DecodeError. A superseded request is aborted, so only the latest response renders.
- **lib:** `formatCents` matches the Rust `Display` cases in `money.rs` (0, 5, -5, 100, -1234, ±2^53−1). `balancesAt` at k=0 equals opening, and a fold that disagrees with the final ledger throws. The seed validator rejects negatives, non-digits and values above u64::MAX. Replay fragments round-trip.
- **components** (Testing Library, by behaviour): Run under naive shows `single_capture_per_intent` failed, and hardened shows all passed. Scrubbing and ←/→ change the balances. Adding a `Duplicate` op sends it in the next run request. Changing the seed resets the plan to `null`. ReplayBadge shows verified / mismatch / unverified. ShrinkView never says "minimal". SweepChart renders the fixture's rates.

## Verification
- `npm --prefix frontend run check` passes. `cargo test`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` are unaffected because there are no Rust edits.
- `VITE_SIM_CLIENT=fixtures npm --prefix frontend run dev`, then walk through the UI with the `run` skill (screenshots). Pick scenario 1 with the naive handler and Run, and the invariant goes red. Scrub to the duplicate capture and watch the merchant balance jump. Switch to hardened, and everything is green. Share, open the link in a new tab, and the badge says verified. Edit `h`, and it shows a determinism break. Shrink, and the reduced plan appears. Sweep, and the chart renders. The "FIXTURE DATA" banner is visible throughout.
- **After step 8:** run the same walkthrough with `VITE_SIM_CLIENT=http` against a local `sim-api`. The sim-api golden-fixture test passes, and the frontend decode tests pass on the regenerated fixtures.
