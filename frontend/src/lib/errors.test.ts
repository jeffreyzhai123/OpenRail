import { expect, test } from 'vitest'
import { ApiError, NetworkError, TimeoutError } from '../api/client'
import { DecodeError } from '../api/decode'
import { BalanceMismatchError } from './balances'
import { errorMessage } from './errors'

test('codes the UI knows get its own words, whatever the message says', () => {
  const error = new ApiError(422, 'plan_too_long', 'server wording')
  expect(errorMessage(error)).toBe(
    'The fault plan is too long: sim-api accepts at most 100 faults.',
  )
})

test("other codes fall back to sim-api's message", () => {
  const error = new ApiError(
    404,
    'unknown_scenario',
    'no scenario has the id "x"',
  )
  expect(errorMessage(error)).toBe('no scenario has the id "x"')
})

test.each<[unknown, RegExp]>([
  [new TimeoutError(15_000), /didn't answer in time/],
  [new NetworkError(new TypeError('Failed to fetch')), /cargo run -p sim-api/],
  [
    new DecodeError('run.trace[0].kind', 'an event kind'),
    /at run\.trace\[0\]\.kind/,
  ],
  [new BalanceMismatchError('the fold is off'), /the fold is off/],
  ['a string', /Something went wrong/],
])('%s gets a message', (error, expected) => {
  expect(errorMessage(error)).toMatch(expected)
})
