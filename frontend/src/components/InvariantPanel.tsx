import type { InvariantResult } from '../api/types'

/** Each named invariant, held or failed, with sim-api's message on failure. */
export function InvariantPanel({
  invariants,
}: {
  invariants: InvariantResult[]
}) {
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
            {message && <p className="message">{message}</p>}
          </li>
        ))}
      </ul>
    </section>
  )
}
