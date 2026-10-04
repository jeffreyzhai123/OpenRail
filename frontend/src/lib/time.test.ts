import { expect, test } from 'vitest'
import { formatDuration } from './time'

test.each([
  [0, '0 ms'],
  [250, '250 ms'],
  [1_000, '1 s'],
  [1_500, '1.5 s'],
  [30_000, '30 s'],
  [90_000, '1.5 min'],
  [3_600_000, '1 h'],
  [14_400_000, '4 h'],
  [259_200_000, '3 d'],
])('%i ms is %s', (ms, expected) => {
  expect(formatDuration(ms)).toBe(expected)
})
