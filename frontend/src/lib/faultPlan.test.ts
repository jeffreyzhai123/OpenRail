import { describe, expect, test } from 'vitest'
import {
  MAX_PLAN_FAULTS,
  type FaultOp,
  type ScenarioSummary,
} from '../api/types'
import { fixture } from '../test/fixtures'
import {
  addFault,
  canAddFault,
  describeFault,
  faultKind,
  faultProblem,
  faultTarget,
  plansEqual,
  removeFault,
  replaceFault,
} from './faultPlan'

const duplicate: FaultOp = { Duplicate: { event_id: 2 } }
const reorder: FaultOp = { Reorder: { event_id: 1, window: 2 } }
const delay: FaultOp = { Delay: { event_id: 3, by: 45_000 } }
const drop: FaultOp = { Drop: { event_id: 4 } }
const crash: FaultOp = { CrashRestart: { at: 1_500 } }

function chargeRetry(): ScenarioSummary {
  const scenarios = fixture('scenarios.json') as ScenarioSummary[]
  const scenario = scenarios.find(({ id }) => id === 'charge-retry')
  if (!scenario) throw new Error('charge-retry is missing from the fixtures')
  return scenario
}

test.each<[FaultOp, string, string, number | null]>([
  [duplicate, 'Duplicate', 'Redeliver event 2 30 s later', 2],
  [reorder, 'Reorder', 'Reverse the 2 deliveries starting at event 1', 1],
  [delay, 'Delay', 'Delay event 3 by 45 s', 3],
  [drop, 'Drop', 'Drop event 4', 4],
  [crash, 'CrashRestart', 'Crash and restart the handler at 1.5 s', null],
])('%j is a %s: "%s"', (op, kind, description, target) => {
  expect(faultKind(op)).toBe(kind)
  expect(describeFault(op)).toBe(description)
  expect(faultTarget(op)).toBe(target)
})

describe('editing', () => {
  test('add, remove and replace return new plans', () => {
    const plan: FaultOp[] = [duplicate, delay]
    expect(addFault(plan, drop)).toEqual([duplicate, delay, drop])
    expect(removeFault(plan, 0)).toEqual([delay])
    expect(replaceFault(plan, 1, crash)).toEqual([duplicate, crash])
    expect(plan).toEqual([duplicate, delay])
  })

  test('a plan stops growing at the server cap', () => {
    const full: FaultOp[] = Array.from({ length: MAX_PLAN_FAULTS }, () => drop)
    expect(canAddFault(full.slice(1))).toBe(true)
    expect(canAddFault(full)).toBe(false)
    expect(() => addFault(full, drop)).toThrow(RangeError)
  })

  test.each([-1, 2, 0.5])('index %s is out of range', (index) => {
    expect(() => removeFault([duplicate, delay], index)).toThrow(RangeError)
    expect(() => replaceFault([duplicate, delay], index, drop)).toThrow(
      RangeError,
    )
  })
})

describe('shape checks', () => {
  test("the scenario's own story plan has no problems", () => {
    const scenario = chargeRetry()
    for (const op of scenario.story_plan) {
      expect(faultProblem(op, scenario.workload)).toBeNull()
    }
  })

  test('a target outside the scenario is a problem', () => {
    expect(
      faultProblem({ Drop: { event_id: 99 } }, chargeRetry().workload),
    ).toBe("event 99 isn't in this scenario")
  })

  test.each<FaultOp>([
    { Delay: { event_id: 1, by: -5 } },
    { Reorder: { event_id: 1, window: 1.5 } },
    { CrashRestart: { at: Number.NaN } },
  ])('%j has a bad number', (op) => {
    expect(faultProblem(op, chargeRetry().workload)).toMatch(/whole number/)
  })

  test('a crash-restart has no target to check', () => {
    expect(faultProblem(crash, [])).toBeNull()
  })
})

test('plans compare by meaning, not by object layout', () => {
  const sameDelayOtherLayout = JSON.parse(
    '{"Delay":{"by":45000,"event_id":3}}',
  ) as FaultOp
  expect(plansEqual([delay, drop], [sameDelayOtherLayout, drop])).toBe(true)
  expect(plansEqual([delay, drop], [drop, delay])).toBe(false)
  expect(plansEqual([delay], [delay, drop])).toBe(false)
  expect(
    plansEqual([{ Drop: { event_id: 4 } }], [{ Duplicate: { event_id: 4 } }]),
  ).toBe(false)
})
