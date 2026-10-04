import { describe, expect, test } from 'vitest'
import { fixture, fixtures } from '../test/fixtures'
import {
  DecodeError,
  decodeErrorEnvelope,
  decodeRunResponse,
  decodeScenarios,
  decodeShrinkResponse,
  decodeSweepResponse,
} from './decode'

/** Which decoder each fixture's endpoint uses. */
function decoderFor(name: string): (value: unknown) => unknown {
  if (name === 'scenarios.json') return decodeScenarios
  if (name === 'shrink.json') return decodeShrinkResponse
  if (name === 'sweep.json') return decodeSweepResponse
  if (name === 'replay.json' || name.startsWith('run-'))
    return decodeRunResponse
  throw new Error(`no decoder for ${name}`)
}

/** A run fixture as a mutable object, for building bad responses. */
function run(): Record<string, unknown> {
  return fixture('run-charge-retry-naive.json') as Record<string, unknown>
}

function pathOf(decode: () => unknown): string {
  try {
    decode()
  } catch (error) {
    if (error instanceof DecodeError) return error.path
    throw error
  }
  throw new Error('expected a DecodeError')
}

describe('every fixture', () => {
  test('exists for each endpoint', () => {
    expect(Object.keys(fixtures).sort()).toEqual([
      'replay.json',
      'run-charge-retry-hardened.json',
      'run-charge-retry-naive.json',
      'run-late-ach-return-hardened.json',
      'run-late-ach-return-naive.json',
      'run-refund-before-capture-hardened.json',
      'run-refund-before-capture-naive.json',
      'scenarios.json',
      'shrink.json',
      'sweep.json',
    ])
  })

  // Decoding to an equal value proves the types model every field sim-api sends.
  test.each(Object.keys(fixtures))('%s decodes to itself', (name) => {
    expect(decoderFor(name)(fixture(name))).toEqual(fixture(name))
  })
})

describe('a run response is rejected at the field that breaks the contract', () => {
  const cases: [string, (body: Record<string, unknown>) => void, string][] = [
    ['a missing field', (body) => delete body.trace_hash, 'run.trace_hash'],
    [
      'an unknown event kind',
      (body) => ((body.trace as { kind: unknown }[])[0].kind = { Rtp: {} }),
      'run.trace[0].kind',
    ],
    [
      'an unknown fault op',
      (body) => ((body.fault_plan as unknown[])[0] = { Explode: {} }),
      'run.fault_plan[0]',
    ],
    [
      'an unsafe integer',
      (body) => {
        const journal = body.journal as { postings: { delta: unknown }[] }[]
        journal[0].postings[0].delta = 2 ** 53
      },
      'run.journal[0].postings[0].delta',
    ],
    ['a seed over u32', (body) => (body.seed = 4_294_967_296), 'run.seed'],
    ['a negative seed', (body) => (body.seed = -1), 'run.seed'],
    ['a string seed', (body) => (body.seed = '0'), 'run.seed'],
    ['an unknown handler', (body) => (body.handler = 'bogus'), 'run.handler'],
    [
      'invariants that are not an array',
      (body) => (body.invariants = 'none'),
      'run.invariants',
    ],
    [
      'posted counts that miss a trace event',
      (body) => (body.posted as unknown[]).pop(),
      'run.posted',
    ],
    [
      'posted counts that disagree with the journal',
      (body) => ((body.posted as number[])[0] += 1),
      'run.posted',
    ],
  ]

  test.each(cases)('%s', (_, breakIt, path) => {
    const body = run()
    breakIt(body)
    expect(pathOf(() => decodeRunResponse(body))).toBe(path)
  })
})

test('an ACH return code outside R01-R04 decodes as Other', () => {
  const scenarios = fixture('scenarios.json') as {
    workload: { kind: { Ach?: { Returned?: { code: unknown } } } }[]
  }[]
  const ach = scenarios.find((scenario) =>
    scenario.workload.some((event) => event.kind.Ach?.Returned),
  )
  const returned = ach?.workload.find((event) => event.kind.Ach?.Returned)
  if (!returned?.kind.Ach?.Returned)
    throw new Error('no ACH return in fixtures')
  returned.kind.Ach.Returned.code = { Other: 'R10' }

  const decoded = decodeScenarios(scenarios)
  expect(JSON.stringify(decoded)).toContain('{"Other":"R10"}')
})

test('the error envelope decodes, and anything else is rejected', () => {
  const envelope = {
    error: { code: 'unknown_scenario', message: 'no such scenario' },
  }
  expect(decodeErrorEnvelope(envelope)).toEqual(envelope)
  expect(() => decodeErrorEnvelope({ status: 'ok' })).toThrow(DecodeError)
})
