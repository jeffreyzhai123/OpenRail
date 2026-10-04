const DIGITS = /^\d+$/

/** Typed decimal digits as a safe integer, or `null` if they aren't one.
 * Surrounding spaces are fine; signs, decimals and exponents aren't. */
export function parseWholeNumber(text: string): number | null {
  const trimmed = text.trim()
  if (!DIGITS.test(trimmed)) return null
  const value = Number(trimmed)
  return Number.isSafeInteger(value) ? value : null
}
