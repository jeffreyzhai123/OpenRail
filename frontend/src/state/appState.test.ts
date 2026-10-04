import { describe, expect, test } from 'vitest'
import type {
  FaultOp,
  RunResponse,
  ScenarioSummary,
  ShrinkResponse,
} from '../api/types'
import { balancesByStep } from '../lib/balances'
import { fixture } from '../test/fixtures'
import {
  initialState,
  reducer,
  runRequest,
  shrinkRequest,
  type AppState,
} from './appState'

function scenarios(): ScenarioSummary[] {
  return fixture('scenarios.json') as ScenarioSummary[]
}

function loaded(): AppState {
  return reducer(initialState, {
    type: 'scenariosLoaded',
    scenarios: scenarios(),
  })
}

function withRun(state: AppState): AppState {
  const response = fixture('run-charge-retry-naive.json') as RunResponse
  return reducer(state, {
    type: 'runSucceeded',
    response,
    balances: balancesByStep(response),
  })
}

describe('inputs', () => {
  test('loading picks the first scenario and opens on its story plan', () => {
    const state = loaded()
    const [first] = scenarios()
    expect(state.status).toBe('ready')
    expect(state.scenarioId).toBe(first.id)
    expect(state.plan).toEqual(first.story_plan)
  })

  test('picking a scenario loads its story plan and clears the old run', () => {
    const second = scenarios()[1]
    const state = reducer(withRun(loaded()), {
      type: 'scenarioPicked',
      id: second.id,
    })
    expect(state.scenarioId).toBe(second.id)
    expect(state.plan).toEqual(second.story_plan)
    expect(state.run).toBeNull()
  })

  test('an unknown scenario id changes nothing', () => {
    const state = loaded()
    expect(reducer(state, { type: 'scenarioPicked', id: 'nope' })).toBe(state)
  })

  test('changing the seed resets the plan to null', () => {
    const state = reducer(loaded(), { type: 'seedEdited', text: '42' })
    expect(state.seed).toBe(42)
    expect(state.plan).toBeNull()
  })

  test('retyping the same seed keeps the plan', () => {
    const state = reducer(loaded(), { type: 'seedEdited', text: ' 0 ' })
    expect(state.plan).toEqual(scenarios()[0].story_plan)
  })

  test("text that isn't a u32 leaves no seed, so nothing can run", () => {
    const state = reducer(loaded(), { type: 'seedEdited', text: '-1' })
    expect(state.seedText).toBe('-1')
    expect(state.seed).toBeNull()
    expect(runRequest(state)).toBeNull()
  })

  test('the request carries the current inputs', () => {
    const state = reducer(loaded(), {
      type: 'handlerPicked',
      handler: 'hardened',
    })
    expect(runRequest(state)).toEqual({
      scenario_id: scenarios()[0].id,
      seed: 0,
      handler: 'hardened',
      fault_plan: scenarios()[0].story_plan,
    })
  })
})

describe('runs', () => {
  test('a finished run shows its final step', () => {
    const state = withRun(reducer(loaded(), { type: 'runStarted' }))
    expect(state.status).toBe('ready')
    expect(state.run?.step).toBe(state.run?.response.trace.length)
  })

  test('the selected step stays within the trace', () => {
    const state = withRun(loaded())
    const last = state.run?.response.trace.length ?? -1
    expect(reducer(state, { type: 'stepSelected', step: 2 }).run?.step).toBe(2)
    expect(reducer(state, { type: 'stepSelected', step: -5 }).run?.step).toBe(0)
    expect(reducer(state, { type: 'stepSelected', step: 99 }).run?.step).toBe(
      last,
    )
  })

  test('a failure is kept for the banner, and the next run clears it', () => {
    const failed = reducer(loaded(), {
      type: 'failed',
      error: new Error('boom'),
    })
    expect(failed.status).toBe('ready')
    expect(failed.error).toBeInstanceOf(Error)
    expect(reducer(failed, { type: 'runStarted' }).error).toBeNull()
  })
})

describe('the fault plan', () => {
  const generated: FaultOp[] = [{ Delay: { event_id: 1, by: 8_690 } }]

  /** A finished run on seed 7's generated plan, which sim-api reports. */
  function afterGeneratedRun(): AppState {
    const state = reducer(loaded(), { type: 'seedEdited', text: '7' })
    const response = {
      ...(fixture('run-charge-retry-naive.json') as RunResponse),
      fault_plan: generated,
    }
    return reducer(reducer(state, { type: 'runStarted' }), {
      type: 'runSucceeded',
      response,
      balances: balancesByStep(response),
    })
  }

  test('a run on the generated plan records it as the seed plan', () => {
    const state = afterGeneratedRun()
    expect(state.plan).toBeNull()
    expect(state.seedPlan).toEqual(generated)
  })

  test('a run on an explicit plan records no seed plan', () => {
    expect(withRun(loaded()).seedPlan).toBeNull()
  })

  test('a new seed or scenario forgets the seed plan; the handler keeps it', () => {
    const state = afterGeneratedRun()
    const reseeded = reducer(state, { type: 'seedEdited', text: '8' })
    const moved = reducer(state, {
      type: 'scenarioPicked',
      id: scenarios()[1].id,
    })
    const handler = reducer(state, {
      type: 'handlerPicked',
      handler: 'hardened',
    })
    expect(reseeded.seedPlan).toBeNull()
    expect(moved.seedPlan).toBeNull()
    expect(handler.seedPlan).toEqual(generated)
  })

  test('an edit becomes the explicit plan and clears the run', () => {
    const edited = reducer(withRun(loaded()), { type: 'planEdited', plan: [] })
    expect(edited.plan).toEqual([])
    expect(edited.run).toBeNull()
    expect(runRequest(edited)?.fault_plan).toEqual([])
  })

  test('reset to seed hands the plan back to sim-api', () => {
    const state = reducer(withRun(loaded()), {
      type: 'planReset',
      to: 'seed',
    })
    expect(state.plan).toBeNull()
    expect(state.run).toBeNull()
  })

  test('reset to story loads the scenario story plan', () => {
    const edited = reducer(loaded(), { type: 'planEdited', plan: [] })
    const state = reducer(edited, { type: 'planReset', to: 'story' })
    expect(state.plan).toEqual(scenarios()[0].story_plan)
  })

  test('a reset to the plan already in use keeps the run', () => {
    const state = withRun(loaded())
    expect(reducer(state, { type: 'planReset', to: 'story' })).toBe(state)
  })
})

describe('replaying a share link', () => {
  function replayed(expected: string | null): AppState {
    const response = fixture('replay.json') as RunResponse
    const started = reducer(loaded(), { type: 'runStarted' })
    return reducer(started, {
      type: 'replaySucceeded',
      response,
      balances: balancesByStep(response),
      expected,
    })
  }

  test("the link's inputs become the current ones", () => {
    const response = fixture('replay.json') as RunResponse
    const state = replayed(response.trace_hash)
    expect(state.status).toBe('ready')
    expect(runRequest(state)).toEqual({
      scenario_id: response.scenario_id,
      seed: response.seed,
      handler: response.handler,
      fault_plan: response.fault_plan,
    })
    expect(state.seedText).toBe(String(response.seed))
    expect(state.loadedPlan).toEqual({
      plan: response.fault_plan,
      from: 'link',
    })
  })

  test('the run records what the link promised and whether it held', () => {
    const { trace_hash } = fixture('replay.json') as RunResponse
    expect(replayed(trace_hash).run?.link).toEqual({
      expected: trace_hash,
      verification: 'verified',
    })
    expect(replayed('00').run?.link?.verification).toBe('mismatch')
    expect(replayed(null).run?.link?.verification).toBe('unverified')
  })

  test('a replay starting clears the shown run; a run starting keeps it', () => {
    const shown = withRun(loaded())
    expect(reducer(shown, { type: 'replayStarted' }).run).toBeNull()
    expect(reducer(shown, { type: 'runStarted' }).run).toBe(shown.run)
  })

  test('an ordinary run carries no link', () => {
    expect(withRun(loaded()).run?.link).toBeNull()
  })

  test('a new scenario or seed forgets the link plan', () => {
    const state = replayed(null)
    expect(
      reducer(state, { type: 'scenarioPicked', id: scenarios()[1].id })
        .loadedPlan,
    ).toBeNull()
    expect(
      reducer(state, { type: 'seedEdited', text: '9' }).loadedPlan,
    ).toBeNull()
  })
})

describe('shrinking', () => {
  const invariant = 'single_entry_per_source_event'

  function shrunk(): AppState {
    const response = fixture('shrink.json') as ShrinkResponse
    const started = reducer(withRun(loaded()), {
      type: 'shrinkStarted',
      invariant,
    })
    return reducer(started, {
      type: 'shrinkSucceeded',
      response,
      balances: balancesByStep(response.run),
    })
  }

  test('the request is the current inputs plus the invariant', () => {
    expect(shrinkRequest(loaded(), invariant)).toEqual({
      ...runRequest(loaded()),
      invariant,
    })
  })

  test('a shrink in flight locks the inputs and names its invariant', () => {
    const state = reducer(withRun(loaded()), {
      type: 'shrinkStarted',
      invariant,
    })
    expect(state.status).toBe('shrinking')
    expect(state.run?.shrink).toEqual({ invariant, result: null })
  })

  test('a failed shrink leaves the run, without the shrink', () => {
    const started = reducer(withRun(loaded()), {
      type: 'shrinkStarted',
      invariant,
    })
    const state = reducer(started, { type: 'failed', error: new Error('x') })
    expect(state.run?.response).toBe(started.run?.response)
    expect(state.run?.shrink).toBeNull()
  })

  test('the result stays with its run until dismissed or replaced', () => {
    const state = shrunk()
    expect(state.status).toBe('ready')
    expect(state.run?.shrink?.result?.response.invariant).toBe(invariant)
    expect(reducer(state, { type: 'shrinkDismissed' }).run?.shrink).toBeNull()
    expect(withRun(state).run?.shrink).toBeNull()
  })

  test('loading the reduced run makes the shrunk plan current', () => {
    const response = fixture('shrink.json') as ShrinkResponse
    const state = reducer(shrunk(), { type: 'reducedRunLoaded' })
    expect(state.plan).toEqual(response.shrunk)
    expect(state.loadedPlan).toEqual({ plan: response.shrunk, from: 'reduced' })
    expect(state.run?.response).toEqual(response.run)
    expect(state.run?.shrink).toBeNull()
    expect(runRequest(state)?.fault_plan).toEqual(response.shrunk)
  })

  test('with no shrink result there is nothing to load', () => {
    const state = withRun(loaded())
    expect(reducer(state, { type: 'reducedRunLoaded' })).toBe(state)
  })
})
