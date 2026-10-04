import { expect, test } from 'vitest'
import { parseWholeNumber } from './wholeNumber'

test.each([
  ['0', 0],
  [' 42 ', 42],
  ['9007199254740991', Number.MAX_SAFE_INTEGER],
])('%j is %s', (text, value) => {
  expect(parseWholeNumber(text)).toBe(value)
})

test.each([
  '',
  ' ',
  '-1',
  '+1',
  '1.5',
  '1e3',
  '0x10',
  'abc',
  '9007199254740992',
])('%j is not a whole number', (text) => {
  expect(parseWholeNumber(text)).toBeNull()
})
