import type { InvariantResult } from '../api/types'

interface InvariantPanelProps {
  invariants: InvariantResult[]
  /** Shrinks the run's plan on the named failed invariant. */
  onShrink: (name: string) => void
  /** The invariant being shrunk right now, if any. */
  shrinking: string | null
  canShrink: boolean
}

/** Each named invariant, held or failed, with sim-api's message on failure
 * and a way to shrink the plan down to what breaks it. */
export function InvariantPanel({
  invariants,
  onShrink,
  shrinking,
  canShrink,
}: InvariantPanelProps) {
  const failed = invariants.filter(({ passed }) => !passed).length
  return (
    <section className="panel invariants" aria-labelledby="invariants-heading">
      <h2 id="invariants-heading">
        {failed === 0
          ? `All ${invariants.length} invariants hold`
          : `${failed} of ${invariants.length} invariants failed`}
      </h2>
      <ul>
        {invariants.map(({ name, passed, message }) => (
          <li key={name} className={passed ? 'pass' : 'fail'}>
            <span className="status">{passed ? '✓ held' : '✗ failed'}</span>{' '}
            <code>{name}</code>
            {!passed && (
              <button
                type="button"
                className="shrink"
                aria-label={`Shrink the plan on ${name}`}
                disabled={!canShrink}
                onClick={() => onShrink(name)}
              >
                {shrinking === name ? 'Shrinking…' : 'Shrink'}
              </button>
            )}
            {message && <p className="message">{message}</p>}
          </li>
        ))}
      </ul>
    </section>
  )
}
