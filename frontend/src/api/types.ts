// The sim-api contract. The source of truth is crates/sim-api/src/dto.rs and
// the sim-core types it serializes; these mirror serde's output exactly.
// Every number is a safe integer, which decode.ts checks at the boundary.

export type Cents = number
export type EventId = number
export type Handler = 'naive' | 'hardened'

/** The seed is a u32 (deterministic-engine-plan.md, decision W). */
export const MAX_SEED = 4_294_967_295

export type CardEvent =
  | { Authorized: { charge_id: number; amount: Cents } }
  | { Captured: { charge_id: number; amount: Cents } }
  | { Refunded: { charge_id: number; amount: Cents } }

export type AchReturnCode = 'R01' | 'R02' | 'R03' | 'R04' | { Other: string }

export type AchEvent =
  | { Initiated: { entry_id: number; amount: Cents } }
  | { Batched: { entry_id: number } }
  | { Settled: { entry_id: number } }
  | { Returned: { entry_id: number; code: AchReturnCode; amount: Cents } }

export type EventKind = { Card: CardEvent } | { Ach: AchEvent }

export interface SimEvent {
  id: EventId
  /** Simulated milliseconds. */
  time: number
  seq: number
  kind: EventKind
}

export type FaultOp =
  | { Duplicate: { event_id: EventId } }
  | { Reorder: { event_id: EventId; window: number } }
  | { Delay: { event_id: EventId; by: number } }
  | { Drop: { event_id: EventId } }
  | { CrashRestart: { at: number } }

export interface Posting {
  account: string
  delta: Cents
}

export interface JournalEntry {
  source: EventId
  intent: string
  kind: 'Capture' | 'Refund'
  postings: Posting[]
}

export interface InvariantResult {
  name: string
  passed: boolean
  message: string | null
}

export interface ScenarioSummary {
  id: string
  name: string
  description: string
  accounts: string[]
  workload: SimEvent[]
  story_plan: FaultOp[]
}

export interface RunRequest {
  scenario_id: string
  seed: number
  handler: Handler
  /** `null` asks sim-api to generate a plan from the seed. */
  fault_plan: FaultOp[] | null
}

export interface RunResponse {
  scenario_id: string
  seed: number
  handler: Handler
  /** The effective plan: the request's, or the one generated from the seed. */
  fault_plan: FaultOp[]
  trace: SimEvent[]
  /** Aligned with `trace`: how many journal entries each delivered event
   * posted, so balances can be shown after any step. */
  posted: number[]
  opening: Record<string, Cents>
  journal: JournalEntry[]
  ledger: { accounts: Record<string, Cents> }
  invariants: InvariantResult[]
  trace_hash: string
  /** encode_run's output: the share link's `r`. */
  replay: string
}

export interface ShrinkRequest {
  scenario_id: string
  seed: number
  handler: Handler
  fault_plan: FaultOp[] | null
  invariant: string
}

export interface ShrinkResponse {
  original: FaultOp[]
  shrunk: FaultOp[]
  invariant: string
  candidates_tried: number
  /** The shrunk plan's run, with its own replay link. */
  run: RunResponse
}

export interface SweepRequest {
  scenario_id: string
  seed_start: number
  count: number
}

/** Counts, not rates: the UI computes the percentages. */
export interface SweepResponse {
  count: number
  naive: { failed: number }
  hardened: { failed: number }
}

/** The body of every error response (crates/sim-api/src/error.rs). */
export interface ErrorEnvelope {
  error: { code: string; message: string }
}
