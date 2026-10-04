import { expect, test } from 'vitest'
import { MAX_SEED } from '../api/types'
import { parseSeed, randomSeed } from './seed'

test.each([
  ['0', 0],
  ['42', 42],
  [' 7 ', 7],
  ['4294967295', MAX_SEED],
])('%j parses as %i', (text, seed) => {
  expect(parseSeed(text)).toBe(seed)
})

test.each([
  '',
  '-1',
  '+1',
  '1.5',
  '1e3',
  'abc',
  '4294967296',
  '99999999999999999999',
])('%j is rejected', (text) => {
  expect(parseSeed(text)).toBeNull()
})

test('a random seed is the one u32 the source fills in', () => {
  const seed = randomSeed((array) => {
    array[0] = 123_456
    return array
  })
  expect(seed).toBe(123_456)
})

test('the default source gives a u32', () => {
  const seed = randomSeed()
  expect(Number.isInteger(seed)).toBe(true)
  expect(seed).toBeGreaterThanOrEqual(0)
  expect(seed).toBeLessThanOrEqual(MAX_SEED)
})
