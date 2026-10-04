import { describe, expect, test } from 'vitest'
import type { RunResponse, ScenarioSummary } from '../api/types'
import { balancesByStep } from '../lib/balances'
import { fixture } from '../test/fixtures'
import { initialState, reducer, runRequest, type AppState } from './appState'

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
