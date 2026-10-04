// The app's state: the run's inputs, the request status and the last run.
// A plain reducer (no global store), so every transition is testable alone.

import type {
  FaultOp,
  Handler,
  RunRequest,
  RunResponse,
  ScenarioSummary,
  ShrinkRequest,
  ShrinkResponse,
} from '../api/types'
import type { Balances } from '../lib/balances'
import { plansEqual, type LoadedPlan } from '../lib/faultPlan'
import { verify, type Verification } from '../lib/replayLink'
import { parseSeed } from '../lib/seed'

export interface CompletedRun {
  response: RunResponse
  /** From balancesByStep: index 0 is the opening. */
  balances: Balances[]
  /** How many deliveries the panels show applied: 0 to trace.length. */
  step: number
  /** Set when the run came from a share link: the hash the link promised,
   * and whether the recomputed run reproduced it. */
  link: { expected: string | null; verification: Verification } | null
  /** A shrink of this run's plan on one failed invariant: in flight while
   * `result` is null. It belongs to the run, so a new run drops it. */
  shrink: {
    invariant: string
    result: { response: ShrinkResponse; balances: Balances[] } | null
  } | null
}

export interface AppState {
  scenarios: ScenarioSummary[]
  scenarioId: string | null
  seedText: string
  /** `null` while the seed field doesn't hold a u32. */
  seed: number | null
  handler: Handler
  /** `null` means sim-api generates a plan from the seed. */
  plan: FaultOp[] | null
  /** The plan sim-api generated for this scenario and seed, once a run on
   * the generated plan has shown it. The handler doesn't change it. */
  seedPlan: FaultOp[] | null
  /** The last plan loaded from a share link or the shrinker, so the editor
   * can say where it came from. */
  loadedPlan: LoadedPlan | null
  /** Inputs lock while `running` or `shrinking`. */
  status: 'loading' | 'ready' | 'running' | 'shrinking'
  run: CompletedRun | null
  error: unknown
}

export type Action =
  | { type: 'scenariosLoaded'; scenarios: ScenarioSummary[] }
  | { type: 'scenarioPicked'; id: string }
  | { type: 'seedEdited'; text: string }
  | { type: 'handlerPicked'; handler: Handler }
  | { type: 'runStarted' }
  | { type: 'replayStarted' }
  | { type: 'runSucceeded'; response: RunResponse; balances: Balances[] }
  | {
      type: 'replaySucceeded'
      response: RunResponse
      balances: Balances[]
      /** The trace hash the link promised, if it carried one. */
      expected: string | null
    }
  | { type: 'failed'; error: unknown }
  | { type: 'stepSelected'; step: number }
  | { type: 'planEdited'; plan: FaultOp[] }
  | { type: 'planReset'; to: 'story' | 'seed' }
  | { type: 'shrinkStarted'; invariant: string }
  | {
      type: 'shrinkSucceeded'
      response: ShrinkResponse
      /** The reduced run's balances, so loading it needs no new request. */
      balances: Balances[]
    }
  | { type: 'shrinkDismissed' }
  | { type: 'reducedRunLoaded' }

export const initialState: AppState = {
  scenarios: [],
  scenarioId: null,
  seedText: '0',
  seed: 0,
  handler: 'naive',
  plan: null,
  seedPlan: null,
  loadedPlan: null,
  status: 'loading',
  run: null,
  error: null,
}

export function reducer(state: AppState, action: Action): AppState {
  switch (action.type) {
    case 'scenariosLoaded': {
      const first = action.scenarios.at(0)
      return {
        ...state,
        scenarios: action.scenarios,
        scenarioId: first?.id ?? null,
        plan: first?.story_plan ?? null,
        status: 'ready',
        error: null,
      }
    }
    case 'scenarioPicked': {
      const scenario = state.scenarios.find(({ id }) => id === action.id)
      if (!scenario) return state
      // A scenario opens on its story: the plan that shows its bug.
      return {
        ...state,
        scenarioId: scenario.id,
        plan: scenario.story_plan,
        seedPlan: null,
        loadedPlan: null,
        run: null,
        error: null,
      }
    }
    case 'seedEdited': {
      const seed = parseSeed(action.text)
      if (seed === state.seed) return { ...state, seedText: action.text }
      // A different seed means a different generated plan, so an explicit
      // plan no longer applies.
      return {
        ...state,
        seedText: action.text,
        seed,
        plan: null,
        seedPlan: null,
        loadedPlan: null,
        run: null,
      }
    }
    case 'handlerPicked':
      return { ...state, handler: action.handler, run: null }
    case 'runStarted':
      return { ...state, status: 'running', error: null }
    case 'replayStarted':
      // A link names other inputs, so the shown run no longer applies, even
      // if the link then fails to replay.
      return { ...state, status: 'running', error: null, run: null }
    case 'runSucceeded':
      return {
        ...state,
        status: 'ready',
        // Inputs are locked while running, so a null plan is still this
        // run's: the response carries what sim-api generated for it.
        seedPlan:
          state.plan === null ? action.response.fault_plan : state.seedPlan,
        run: completed(action.response, action.balances, null),
      }
    case 'replaySucceeded': {
      const { response } = action
      // The link's inputs become the current ones, so the controls show
      // exactly what was replayed and Run reproduces it.
      return {
        ...state,
        status: 'ready',
        scenarioId: response.scenario_id,
        seedText: String(response.seed),
        seed: response.seed,
        handler: response.handler,
        plan: response.fault_plan,
        seedPlan: null,
        loadedPlan: { plan: response.fault_plan, from: 'link' },
        run: completed(response, action.balances, {
          expected: action.expected,
          verification: verify(action.expected, response.trace_hash),
        }),
      }
    }
    case 'failed': {
      // A shrink that failed leaves nothing to show.
      const pending = state.run?.shrink?.result === null
      const run =
        pending && state.run ? { ...state.run, shrink: null } : state.run
      return { ...state, status: 'ready', error: action.error, run }
    }
    case 'stepSelected': {
      if (!state.run) return state
      const last = state.run.response.trace.length
      const step = Math.min(Math.max(action.step, 0), last)
      return { ...state, run: { ...state.run, step } }
    }
    case 'planEdited':
      return withPlan(state, action.plan)
    case 'planReset': {
      if (action.to === 'seed') return withPlan(state, null)
      const scenario = state.scenarios.find(({ id }) => id === state.scenarioId)
      return scenario ? withPlan(state, scenario.story_plan) : state
    }
    case 'shrinkStarted':
      if (!state.run) return state
      return {
        ...state,
        status: 'shrinking',
        error: null,
        run: {
          ...state.run,
          shrink: { invariant: action.invariant, result: null },
        },
      }
    case 'shrinkSucceeded': {
      if (!state.run) return state
      const { response, balances } = action
      return {
        ...state,
        status: 'ready',
        run: {
          ...state.run,
          shrink: {
            invariant: response.invariant,
            result: { response, balances },
          },
        },
      }
    }
    case 'shrinkDismissed':
      return state.run
        ? { ...state, run: { ...state.run, shrink: null } }
        : state
    case 'reducedRunLoaded': {
      const result = state.run?.shrink?.result
      if (!result) return state
      // sim-api already ran the reduced plan, so loading it is the same as
      // editing the plan down and pressing Run.
      const { shrunk, run } = result.response
      return {
        ...state,
        plan: shrunk,
        loadedPlan: { plan: shrunk, from: 'reduced' },
        run: completed(run, result.balances, null),
      }
    }
  }
}

function completed(
  response: RunResponse,
  balances: Balances[],
  link: CompletedRun['link'],
): CompletedRun {
  return { response, balances, step: response.trace.length, link, shrink: null }
}

/** A different plan clears the shown run, which the old plan produced. */
function withPlan(state: AppState, plan: FaultOp[] | null): AppState {
  if (samePlan(state.plan, plan)) return state
  return { ...state, plan, run: null }
}

function samePlan(a: FaultOp[] | null, b: FaultOp[] | null): boolean {
  if (a === null || b === null) return a === b
  return plansEqual(a, b)
}

/** The request for the current inputs, or `null` if they can't run yet. */
export function runRequest(state: AppState): RunRequest | null {
  if (state.scenarioId === null || state.seed === null) return null
  return {
    scenario_id: state.scenarioId,
    seed: state.seed,
    handler: state.handler,
    fault_plan: state.plan,
  }
}

/** The request to shrink the current inputs' plan on one invariant, or
 * `null` if they can't run. */
export function shrinkRequest(
  state: AppState,
  invariant: string,
): ShrinkRequest | null {
  const request = runRequest(state)
  return request && { ...request, invariant }
}
