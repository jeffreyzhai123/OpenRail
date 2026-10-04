import { expect, test } from 'vitest'
import type { RunResponse } from '../api/types'
import { fixture } from '../test/fixtures'
import { parseReplayFragment, replayFragment, verify } from './replayLink'

function storyRun(): RunResponse {
  return fixture('run-charge-retry-naive.json') as RunResponse
}

test("a real run's link round-trips through the fragment", () => {
  const run = storyRun()
  const fragment = replayFragment(run.replay, run.trace_hash)
  expect(fragment).toBe(`#r=${run.replay}&h=${run.trace_hash}`)
  expect(parseReplayFragment(fragment)).toEqual({
    replay: run.replay,
    traceHash: run.trace_hash,
  })
})

test('the leading # is optional', () => {
  expect(parseReplayFragment('r=1.abc&h=ff')).toEqual({
    replay: '1.abc',
    traceHash: 'ff',
  })
})

test('a link without a hash is unverified, not invalid', () => {
  expect(parseReplayFragment('#r=1.abc')).toEqual({
    replay: '1.abc',
    traceHash: null,
  })
  expect(parseReplayFragment('#r=1.abc&h=')).toEqual({
    replay: '1.abc',
    traceHash: null,
  })
})

test.each(['', '#', '#h=ff', '#r='])('%j holds no replay', (fragment) => {
  expect(parseReplayFragment(fragment)).toBeNull()
})

test('the badge compares the hashes', () => {
  const hash = storyRun().trace_hash
  expect(verify(hash, hash)).toBe('verified')
  expect(verify(`${hash.slice(1)}0`, hash)).toBe('mismatch')
  expect(verify(null, hash)).toBe('unverified')
})
