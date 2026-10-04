import { expect, test } from 'vitest'
import { formatCents } from './money'

// The same cases as Rust's display test (crates/sim-core/src/money.rs),
// plus the largest values a JS number holds exactly.
test.each([
  [0, '0.00'],
  [5, '0.05'],
  [-5, '-0.05'],
  [100, '1.00'],
  [-1234, '-12.34'],
  [Number.MAX_SAFE_INTEGER, '90071992547409.91'],
  [-Number.MAX_SAFE_INTEGER, '-90071992547409.91'],
])('%i cents formats as %s', (cents, expected) => {
  expect(formatCents(cents)).toBe(expected)
})

test.each([1.5, 2 ** 53, Number.NaN])('%s is rejected', (cents) => {
  expect(() => formatCents(cents)).toThrow(RangeError)
})
