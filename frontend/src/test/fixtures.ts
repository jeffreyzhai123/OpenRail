// The golden fixtures sim-api generates (crates/sim-api/tests/fixtures.rs),
// loaded for tests. They're real API responses, so they're test data only.

const loaded = import.meta.glob<unknown>('../api/fixtures/*.json', {
  eager: true,
  import: 'default',
})

/** Fixture name (e.g. `sweep.json`) to its parsed contents. */
export const fixtures: Record<string, unknown> = Object.fromEntries(
  Object.entries(loaded).map(([path, value]) => [path.split('/').pop(), value]),
)

/** A deep copy, so a test can change it without affecting others. */
export function fixture(name: string): unknown {
  if (!(name in fixtures)) {
    throw new Error(`no fixture named ${name}`)
  }
  return structuredClone(fixtures[name])
}
