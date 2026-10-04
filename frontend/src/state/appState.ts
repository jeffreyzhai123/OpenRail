// The app's state: the run's inputs, the request status and the last run.
// A plain reducer (no global store), so every transition is testable alone.

import type {
  FaultOp,
  Handler,
  RunRequest,
  RunResponse,
  ScenarioSummary,
} from '../api/types'
import type { Balances } from '../lib/balances'
import { parseSeed } from '../lib/seed'

export interface CompletedRun {
  response: RunResponse
  /** From balancesByStep: index 0 is the opening. */
  balances: Balances[]
  /** How many deliveries the panels show applied: 0 to trace.length. */
  step: number
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
  status: 'loading' | 'ready' | 'running'
  run: CompletedRun | null
  error: unknown
}

export type Action =
  | { type: 'scenariosLoaded'; scenarios: ScenarioSummary[] }
  | { type: 'scenarioPicked'; id: string }
  | { type: 'seedEdited'; text: string }
  | { type: 'handlerPicked'; handler: Handler }
  | { type: 'runStarted' }
  | { type: 'runSucceeded'; response: RunResponse; balances: Balances[] }
  | { type: 'failed'; error: unknown }
  | { type: 'stepSelected'; step: number }

export const initialState: AppState = {
  scenarios: [],
  scenarioId: null,
  seedText: '0',
  seed: 0,
  handler: 'naive',
  plan: null,
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
        run: null,
        error: null,
      }
    }
    case 'seedEdited': {
      const seed = parseSeed(action.text)
      // A different seed means a different generated plan, so an explicit
      // plan no longer applies.
      return {
        ...state,
        seedText: action.text,
        seed,
        plan: seed === state.seed ? state.plan : null,
        run: seed === state.seed ? state.run : null,
      }
    }
    case 'handlerPicked':
      return { ...state, handler: action.handler, run: null }
    case 'runStarted':
      return { ...state, status: 'running', error: null }
    case 'runSucceeded':
      return {
        ...state,
        status: 'ready',
        run: {
          response: action.response,
          balances: action.balances,
          step: action.response.trace.length,
        },
      }
    case 'failed':
      return { ...state, status: 'ready', error: action.error }
    case 'stepSelected': {
      if (!state.run) return state
      const last = state.run.response.trace.length
      const step = Math.min(Math.max(action.step, 0), last)
      return { ...state, run: { ...state.run, step } }
    }
  }
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
