import type { ShrinkResponse } from '../api/types'
import { describeFault, keptFaults } from '../lib/faultPlan'

interface ShrinkViewProps {
  response: ShrinkResponse
  onLoad: () => void
  onDismiss: () => void
}

/** The shrinker's result: which faults the plan still needs to break the
 * invariant. V1's shrinker is one greedy pass (README §6.3), so the result
 * is "reduced", never "minimal". */
export function ShrinkView({ response, onLoad, onDismiss }: ShrinkViewProps) {
  const { original, shrunk, invariant, candidates_tried } = response
  const kept = keptFaults(original, shrunk)
  const unchanged = shrunk.length === original.length

  return (
    <section className="panel shrink-view" aria-labelledby="shrink-heading">
      <h2 id="shrink-heading">
        Reduced plan for <code>{invariant}</code>
      </h2>
      <p>
        {unchanged
          ? `Every fault is needed: removing any one of them lets ${invariant} hold.`
          : `${shrunk.length} of ${original.length} faults still break ${invariant}.`}
      </p>
      <ol className="shrunk-faults">
        {original.map((op, index) => (
          <li key={index} className={kept[index] ? 'kept' : 'removed'}>
            <span className="mark">{kept[index] ? 'kept' : 'removed'}</span>{' '}
            {describeFault(op)}
          </li>
        ))}
      </ol>
      <p className="hint">
        {candidates_tried} {candidates_tried === 1 ? 'candidate' : 'candidates'}{' '}
        tried, in one greedy pass that tries removing each fault once. A smaller
        plan may still break it.
      </p>
      <div className="actions">
        {!unchanged && (
          <button type="button" className="primary" onClick={onLoad}>
            Load reduced run
          </button>
        )}
        <button type="button" onClick={onDismiss}>
          Close
        </button>
      </div>
    </section>
  )
}
