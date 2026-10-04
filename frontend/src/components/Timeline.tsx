import type { KeyboardEvent } from 'react'
import type { JournalEntry, SimEvent } from '../api/types'
import { describeEntry, describeEvent } from '../lib/events'
import { formatDuration } from '../lib/time'

interface TimelineProps {
  trace: SimEvent[]
  /** Aligned with `trace`: the journal entries each delivery posted. */
  entries: JournalEntry[][]
  /** Deliveries applied: 0 is the opening, trace.length the end. */
  step: number
  onStep: (step: number) => void
}

/** The run's deliveries in order. Picking one shows the balances after it. */
export function Timeline({ trace, entries, step, onStep }: TimelineProps) {
  const seen = new Set<number>()
  const rows = trace.map((event, index) => {
    const redelivery = seen.has(event.id)
    seen.add(event.id)
    return { event, index, redelivery }
  })

  function onKeyDown(event: KeyboardEvent<HTMLOListElement>) {
    const moves: Record<string, number> = {
      ArrowLeft: step - 1,
      ArrowUp: step - 1,
      ArrowRight: step + 1,
      ArrowDown: step + 1,
      Home: 0,
      End: trace.length,
    }
    if (event.key in moves) {
      event.preventDefault()
      onStep(moves[event.key])
    }
  }

  const selected = step > 0 ? trace[step - 1] : null
  return (
    <section className="panel timeline" aria-labelledby="timeline-heading">
      <h2 id="timeline-heading">Timeline</h2>
      <p className="hint">
        Pick a delivery, or use the arrow keys, to see the balances after it.
      </p>
      <ol className="steps" aria-label="Deliveries" onKeyDown={onKeyDown}>
        <li>
          <button
            type="button"
            aria-current={step === 0 ? 'step' : undefined}
            onClick={() => onStep(0)}
          >
            <span className="what">Opening balances</span>
          </button>
        </li>
        {rows.map(({ event, index, redelivery }) => (
          <li key={event.seq}>
            <button
              type="button"
              aria-current={step === index + 1 ? 'step' : undefined}
              onClick={() => onStep(index + 1)}
            >
              <span className="time">{formatDuration(event.time)}</span>
              <span className="what">{describeEvent(event.kind)}</span>
              {redelivery && <span className="badge">redelivery</span>}
              <span className="posted">
                {postedLabel(entries[index].length)}
              </span>
            </button>
          </li>
        ))}
      </ol>
      <div className="detail" aria-live="polite">
        {selected === null ? (
          <p>Before any delivery.</p>
        ) : (
          <>
            <p>
              Event {selected.id}, delivered at {formatDuration(selected.time)}
            </p>
            {entries[step - 1].length === 0 ? (
              <p>Posted nothing.</p>
            ) : (
              <ul>
                {entries[step - 1].map((entry, index) => (
                  <li key={index}>{describeEntry(entry)}</li>
                ))}
              </ul>
            )}
          </>
        )}
      </div>
    </section>
  )
}

function postedLabel(count: number): string {
  if (count === 0) return 'posted nothing'
  return count === 1 ? 'posted 1 entry' : `posted ${count} entries`
}
