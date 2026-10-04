// Integer cents as dollars.cents, mirroring Rust's `Display for Money`
// (crates/sim-core/src/money.rs). BigInt throughout, so nothing is ever
// divided as a float.

const CENTS_PER_DOLLAR = 100n

/** `-1234` → `"-12.34"`. No currency symbol: the UI adds one. */
export function formatCents(cents: number): string {
  if (!Number.isSafeInteger(cents)) {
    throw new RangeError(`${cents} isn't a whole number of cents`)
  }
  const value = BigInt(cents)
  const negative = value < 0n
  const magnitude = negative ? -value : value
  const dollars = magnitude / CENTS_PER_DOLLAR
  const remainder = (magnitude % CENTS_PER_DOLLAR).toString().padStart(2, '0')
  return `${negative ? '-' : ''}${dollars}.${remainder}`
}
