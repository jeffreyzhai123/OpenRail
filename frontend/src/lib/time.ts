// Simulated milliseconds as a short duration, for fault descriptions and the
// timeline. A display concern only: times stay integer ms everywhere else.

const UNITS: [label: string, ms: number][] = [
  ['d', 86_400_000],
  ['h', 3_600_000],
  ['min', 60_000],
  ['s', 1_000],
]

/** `250` → `"250 ms"`, `1500` → `"1.5 s"`, `259200000` → `"3 d"`. */
export function formatDuration(ms: number): string {
  for (const [label, size] of UNITS) {
    if (ms >= size) {
      // At most two decimals, with trailing zeros dropped.
      return `${Number((ms / size).toFixed(2))} ${label}`
    }
  }
  return `${ms} ms`
}
