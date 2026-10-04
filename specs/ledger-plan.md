# Plan: Partner B, money and ledger domain (sim-core)

## Context
Step 0 is committed (`f65876b`): `sim-core` declares every module, deps are in (`serde`, `serde_json`, `blake3`, dev `proptest`), and the shared stubs exist. Partner B owns four files from TODO.md: `money.rs`, `ledger.rs`, `invariants.rs`, `ach.rs`. Partner A's `simulator.rs` will call B's ledger and invariant APIs, so **B's public signatures are the A/B contract** and land first.

What B starts from:
- `money.rs`: `Money(pub i64)` with `checked_add` / `checked_sub` and serde. **Missing:** `Display`, `ZERO`, negation.
- `ledger.rs`: only `LedgerSnapshot { accounts: BTreeMap<String, Money> }`. No `Ledger` yet.
- `invariants.rs`: only `InvariantResult { name: &'static str, passed, message }`.
- `ach.rs`: empty.

## The one principle behind most decisions below
**The ledger enforces only accounting structure. It does not enforce business rules.** `post()` rejects unbalanced entries, unknown accounts and overflow, because those mean "this isn't a valid ledger at all". It **does not** reject a second capture for the same intent or an over-refund. Those are invariants #3 and #4, checked *after* the run.

Why: the product exists to let the naive handler do the wrong thing and *show* it breaking. If `post()` refused a duplicate capture, the naive handler's bug would turn into a silent error instead of a red invariant on screen, and the demo would have nothing to show.

## Deviations from TODO.md / README (CLAUDE.md requires flagging these)
1. **`post()` takes a `JournalEntry`, not just a list of postings.** The TODO says `post(entries)`. The entry also carries `intent`, `kind` and `source: EventId`. Invariants #3, #4 and #6 need to know *which intent* and *which event* each money movement belongs to, and adding those fields later would change the signature A's simulator calls. Fixing it now keeps the A/B seam stable.
2. **The ledger keeps a journal** (an append-only `Vec<JournalEntry>`) in addition to `accounts`. This isn't in the README file map, but #3 and #4 can't be computed from balances alone.
3. **#5 and #6 are named but not run by `check_all()`.** The TODO says "stub by name". A stub that returns `passed: true` would be a fake pass, and the README rules out placeholder results. So the names exist as constants, and the checks join `check_all()` when provider events exist.
4. **`InvariantResult` field stays `message`.** The TODO says `detail`, but step 0 committed `message`, so the code wins and TODO.md gets a one-word fix.
5. **Opening balances must sum to zero.** Scenarios have to list a counterparty account (e.g. `external:customer`) explicitly. This isn't stated anywhere today.

## Order of work (each step is its own small PR or commit)
1. **`ledger.rs` public API and `invariants::check_all` signature.** Bodies can be partial, but the types and signatures must be final. This unblocks A first.
2. **`money.rs` additions.** Tiny, and `ledger.rs` needs them anyway, so they likely ship together with step 1.
3. **Ledger internals and the #1 and #2 checks, with tests.**
4. **The #3 and #4 checks** (the TODO's "if time allows").
5. **`ach.rs`.** Nothing depends on it today, so it goes last.

Run `cargo fmt`, `cargo clippy --all-targets -- -D warnings` and `cargo test` before each commit (CLAUDE.md).

---

## `money.rs`

| Decision | Why |
|---|---|
| Keep `Money(pub i64)` with a public field | Any `i64` cent value is a valid `Money`, so a private field protects nothing. The newtype exists to rule out floats and force checked arithmetic. Step 0 and A's code already construct it as `Money(n)`. |
| **Don't implement `Add`/`Sub`/`Neg`.** Only `checked_add`, `checked_sub`, and a new `checked_neg` | A `+` operator would panic in debug builds and wrap in release. With no operator there is no unchecked path, so every overflow has to be handled at the call site. |
| Add `pub const ZERO: Money` | CLAUDE.md says no magic numbers, and `Money(0)` shows up in every sum. |
| `Display` prints `-12.34` and `0.05`: sign, dollars, `.`, cents always 2 digits, no currency symbol | The core doesn't know the currency, so the UI adds the symbol. Use `unsigned_abs()` so `i64::MIN` formats correctly instead of overflowing on negation. |
| Add `#[serde(transparent)]` | It serializes as a bare JSON integer (the current output, but now stated explicitly). That gives exact round-trips for replay links (README §6.2) and never produces a float in JSON. |
| No `From<f64>` and no parsing | Hard rule. Amounts enter as integer cents from scenarios only. |

## `ledger.rs`

```rust
pub struct IntentId(pub String);          // newtype: can't be mixed up with account names (also String)
pub enum EntryKind { Capture, Refund }    // grows when handlers need it (e.g. AchReturn)
pub struct Posting { pub account: String, pub delta: Money }   // signed: + raises balance, − lowers it
pub struct JournalEntry { pub source: EventId, pub intent: IntentId, pub kind: EntryKind, pub postings: Vec<Posting> }
pub enum LedgerError { DuplicateAccount(String), UnknownAccount(String), EmptyEntry, Unbalanced { sum: Money }, Overflow }

pub struct Ledger { opening: BTreeMap<String, Money>, accounts: BTreeMap<String, Money>, journal: Vec<JournalEntry> }
impl Ledger {
    pub fn open(opening: &[(String, i64)]) -> Result<Ledger, LedgerError>;
    pub fn post(&mut self, entry: JournalEntry) -> Result<(), LedgerError>;
    pub fn balance(&self, account: &str) -> Option<Money>;
    pub fn journal(&self) -> &[JournalEntry];
    pub fn opening(&self) -> &BTreeMap<String, Money>;
    pub fn snapshot(&self) -> LedgerSnapshot;
}
pub fn transfer(from: &str, to: &str, amount: Money) -> Option<Vec<Posting>>; // two-legged helper
```

| Decision | Why |
|---|---|
| `BTreeMap` everywhere and a `Vec` journal | Iteration order is deterministic. This is the hard rule, and `LedgerSnapshot` feeds into `RunResult`. |
| Fields are private; `post()` is the only way to change balances | That makes #1 and #2 hold by construction, and the checks act as a backstop. |
| Signed `delta` instead of separate debit and credit fields | Simplest double-entry form: "balanced" just means the deltas sum to zero. |
| `open()` takes `&[(String, i64)]` | That's the shape of `Scenario.initial_ledger` and of A's `run()` parameter, so nobody has to convert at the seam. |
| `open()` rejects duplicate account names and opening balances that don't sum to zero | It makes "the sum of all balances is 0" literally true from the first tick. Otherwise invariant #1 would need an offset fudge. |
| `post()` rejects accounts that `open()` didn't declare (`UnknownAccount`) | Creating accounts on the fly would let a typo like `"merhcant"` silently produce a new account. CLAUDE.md says fail fast. Scenarios declare zero-balance accounts up front. |
| **`post()` is atomic:** compute every new balance into a staging `BTreeMap` with checked math first, and only then commit and append to the journal | If an overflow on the 2nd of 3 legs left the first leg applied, the ledger itself would be unbalanced, and that's exactly the kind of state it's supposed to make impossible. Staging also handles the same account appearing twice in one entry. |
| Checks run in this order: empty, unknown account, unbalanced (sum computed with checked math), overflow | This reports the root cause first. An unbalanced entry is a handler bug, while overflow is an extreme-input problem. |
| Negative balances allowed; no overdraft rule | Overdraft isn't one of the ledger invariants (README §6.4). Rejecting it would hide handler bugs, per the principle above. |
| `debug_assert!` that the total is 0 at the end of `post()` | CLAUDE.md: assert the invariants the types can't encode. It costs nothing in release builds. |
| A `transfer()` helper returns `None` if `amount` is 0 or negation overflows | Nearly every posting is two-legged, and A's simulator and B's tests would otherwise each write their own version. |
| `snapshot()` clones `accounts` | `RunResult` needs an owned value, so the clone has a clear reason (CLAUDE.md ownership rule). |
| `LedgerError` is hand-written with `Display` and `std::error::Error`, without `thiserror` | Keeps dependencies minimal (CLAUDE.md). It's five variants. |

## `invariants.rs`

```rust
pub const LEDGER_BALANCED: &str = "ledger_balanced";                  // #1
pub const MONEY_CONSERVED: &str = "money_conserved";                  // #2
pub const SINGLE_CAPTURE_PER_INTENT: &str = "single_capture_per_intent"; // #3
pub const REFUND_WITHIN_CAPTURE: &str = "refund_within_capture";      // #4
pub const RECONCILES_WITH_PROVIDER: &str = "reconciles_with_provider"; // #5, not checked yet
pub const ENTRY_HAS_PROVIDER_EVENT: &str = "entry_has_provider_event"; // #6, not checked yet

pub struct InvariantContext<'a> { pub ledger: &'a Ledger }
pub trait InvariantCheck { fn name(&self) -> &'static str; fn check(&self, ctx: &InvariantContext) -> InvariantResult; }
pub fn check_all(ctx: &InvariantContext) -> Vec<InvariantResult>;     // fixed order #1..#4
```

| Decision | Why |
|---|---|
| Names are `pub const` snake_case strings, documented as **never rename** | The shrinker matches on the name, and the names end up in replay links (§6.2). They're effectively a public API. |
| Pass an `InvariantContext` struct instead of a bare `&Ledger` | #5 and #6 will need provider state and the trace. Adding a field to the struct won't change any `check` signature or A's call site. |
| A trait with one unit struct per invariant | README §6.5 specifies the trait, and it lets tests run one check alone. V3's per-shard checks can be parameterized structs. |
| `check_all` returns results in a fixed array order | `RunResult.invariants` is serialized, so its order must be deterministic. |
| The checks run once, at the end of the run, over the **whole journal history** | This keeps the A/B seam simple (A calls `check_all` once) while still testing "always" and "never" over time. |
| **#1** `ledger_balanced`: every journal entry sums to 0, and the current balances sum to 0 | That's the literal README wording ("sum of entries is zero"). |
| **#2** `money_conserved`: opening balances plus every journal delta, replayed, equals current `accounts` exactly | It catches any money that appeared without a recorded entry. Honest note: in V1, #1 and #2 can't fail because the ledger guarantees them. They become load-bearing in V3 sharding and sagas (money in flight between shards). Expect scenarios 1 and 2 to trip #3 and #4, not these. |
| **#3** `single_capture_per_intent`: count `Capture` entries per intent and fail if any count is above 1 | This is the idempotency bug from scenario 1 (charge retried after timeout). Use a `BTreeMap<&IntentId, u32>` so the failure message is deterministic. |
| **#4** `refund_within_capture`: walk the journal **in order**, keep running totals per intent, and fail at the first point where refunded > captured | "Never exceed" is a rule over time. Scenario 2 (refund arrives before capture) is fine by the end but wrong in the middle, so a final-state-only check would miss the exact bug the scenario exists to show. The amount is the sum of an entry's positive deltas, so there's no separate `amount` field that could disagree with the postings. |
| Overflow while summing turns into `passed: false` with a message, not a panic | No `unwrap` outside tests, and the check reports the problem instead of crashing. |
| The failure message names the first offender (intent, entry index, amounts) | The README wants failure pages that explain themselves in one or two lines. |
| Leave `InvariantResult.name: &'static str` as is for now | It compiles and serializes fine. **Flag for later:** deserializing it only works from `'static` input, so `sim-api` will need `Cow<'static, str>` if it ever reads results back. Not a problem today. |

## `ach.rs`

```rust
pub enum AchState { Initiated, Batched, Settled, Returned }
impl AchState { pub const ALL: [AchState; 4]; pub fn transition(self, to: AchState) -> Result<AchState, AchError>; }
pub enum AchError { IllegalTransition { from: AchState, to: AchState } }
```

| Decision | Why |
|---|---|
| `transition` takes the *target state*, not an event enum | Webhooks arrive as "it is now settled", which maps one-to-one to target states, so there's no extra event type to keep in sync. |
| Exactly three legal moves: Initiated→Batched, Batched→Settled, Settled→Returned. `Returned` is terminal | This matches the README's linear chain. Nacha details get re-verified in V2 (README), so don't invent extra edges now. |
| A self-transition (e.g. Settled→Settled) is an `Err`, not a silent no-op | Duplicate webhooks are the core fault. The pure state machine reports the problem and the handler decides what to do (CLAUDE.md: low-level code reports, the boundary decides). The hardened handler can see `from == to` and treat it as an idempotent duplicate, while the naive one won't. |
| `Copy` + serde derives, and the type stays pure (no clock, no ledger) | It can be tested on its own. Its link to ledger postings belongs to the handlers (a later step). |

## Tests (all deterministic)
- **Proptest config:** set `rng_seed: RngSeed::Fixed(<const>)` in a shared `ProptestConfig`. Proptest defaults to a random seed, and CLAUDE.md says no uncontrolled randomness in tests. Verified that `RngSeed::Fixed` exists in proptest 1.11. Commit any `proptest-regressions/` files. Proptest pulls in `rand` only as a dev-dependency, so `sim-core`'s runtime rule isn't affected.
- **money:** `checked_*` matches `i64::checked_*` (proptest). `Display` cases: 0, 5, -5, 100, -1234, `i64::MAX`, `i64::MIN`. Proptest that `Display` with the `.` stripped parses back to the same cents.
- **ledger:**
  - `open` rejects duplicate accounts and unbalanced openings.
  - `post` rejects empty, unknown-account, unbalanced and overflowing entries.
  - **After every rejected post, the snapshot and journal are unchanged** (atomicity).
  - The same account twice in one entry nets out correctly.
  - Proptest: random balanced entries over a fixed set of 4 accounts keep the total at 0, and replaying the journal equals the balances.
  - Proptest: a balanced entry with one leg nudged by a nonzero amount is always rejected.
- **invariants:** for each of #1–#4, one passing ledger and one failing ledger, asserting on the **name** and the `passed` flag rather than the message text (test behavior, not wording). #4 includes "refund before capture, capture arrives later": it must fail even though the final totals are fine. `check_all` returns the 4 names in a fixed order.
- **ach:** an exhaustive 4×4 table over `AchState::ALL`. Exactly 3 moves are `Ok`, and all others, including the 4 self-transitions, are `IllegalTransition` with the right `from` and `to`.

## Files
- Modify: `crates/sim-core/src/money.rs`, `ledger.rs`, `invariants.rs`, `ach.rs`.
- Touch: `TODO.md` (`detail` → `message`, tick B's boxes).
- Not touched: A's files (`rng`, `clock`, `event`, `trace`, `simulator`), `lib.rs`, `Cargo.toml` (no new deps).

## Verification
- `cargo test -p sim-core`, `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check` all pass.
- `grep -rn "HashMap\|HashSet\|f64\|unwrap()\|tokio" crates/sim-core/src` finds nothing outside `#[cfg(test)]`.
- A hands-off check: a test-only ledger that posts the same `Capture` twice for one intent gives `check_all` → `single_capture_per_intent` failed, and every other check passed. This is the exact signal A's simulator will surface.
