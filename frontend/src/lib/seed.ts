// The seed is a u32 (MAX_SEED), small enough to be an exact JS number.

import { MAX_SEED } from '../api/types'
import { parseWholeNumber } from './wholeNumber'

/** The seed field's text as a u32, or `null` if it isn't one. Surrounding
 * spaces are fine; signs, decimals, exponents and anything over u32::MAX
 * aren't. */
export function parseSeed(text: string): number | null {
  const seed = parseWholeNumber(text)
  return seed !== null && seed <= MAX_SEED ? seed : null
}

/** One uniform u32, from the browser's CSPRNG unless `fill` says otherwise. */
export function randomSeed(
  fill: (array: Uint32Array<ArrayBuffer>) => Uint32Array<ArrayBuffer> = (
    array,
  ) => crypto.getRandomValues(array),
): number {
  return fill(new Uint32Array(1))[0]
}
