import { useState } from 'react'
import { MAX_PLAN_FAULTS, type FaultOp, type SimEvent } from '../api/types'
import { describeEvent } from '../lib/events'
import {
  addFault,
  canAddFault,
  describeFault,
  faultFromDraft,
  planOrigin,
  removeFault,
  type FaultDraft,
  type FaultKind,
} from '../lib/faultPlan'
import { parseWholeNumber } from '../lib/wholeNumber'

interface FaultPlanEditorProps {
  /** `null` means sim-api generates the plan from the seed. */
  plan: FaultOp[] | null
  /** What sim-api generated for this seed, once a run has shown it. */
  seedPlan: FaultOp[] | null
  storyPlan: FaultOp[]
  seed: number | null
  workload: SimEvent[]
  onChange: (plan: FaultOp[]) => void
  onReset: (to: 'story' | 'seed') => void
}

/** The plan the next run uses: list, remove, add, or reset it to the
 * scenario's story or the seed's generated plan. */
export function FaultPlanEditor(props: FaultPlanEditorProps) {
  const shown = props.plan ?? props.seedPlan
  const origin = planOrigin(props.plan, props.storyPlan, props.seedPlan)
  const fromSeed =
    props.seed === null ? 'from the seed' : `from seed ${props.seed}`
  const label = { story: 'story plan', seed: fromSeed, edited: 'edited' }[
    origin
  ]

  return (
    <div className="field fault-plan">
      <h3>
        Faults <span className="badge">{label}</span>
      </h3>
      {shown === null ? (
        <p className="hint">
          Generated {fromSeed} when you run. Run once to edit it.
        </p>
      ) : (
        <>
          <FaultList
            plan={shown}
            onRemove={(index) => props.onChange(removeFault(shown, index))}
          />
          <AddFault
            plan={shown}
            workload={props.workload}
            onAdd={(op) => props.onChange(addFault(shown, op))}
          />
        </>
      )}
      <div className="resets">
        <button
          type="button"
          disabled={origin === 'story'}
          onClick={() => props.onReset('story')}
        >
          Reset to story plan
        </button>
        <button
          type="button"
          disabled={origin === 'seed'}
          onClick={() => props.onReset('seed')}
        >
          Reset to seed plan
        </button>
      </div>
    </div>
  )
}

function FaultList({
  plan,
  onRemove,
}: {
  plan: FaultOp[]
  onRemove: (index: number) => void
}) {
  if (plan.length === 0) return <p className="hint">No faults.</p>
  return (
    <ul className="faults">
      {plan.map((op, index) => (
        <li key={index}>
          <span>{describeFault(op)}</span>
          <button
            type="button"
            aria-label={`Remove: ${describeFault(op)}`}
            onClick={() => onRemove(index)}
          >
            Remove
          </button>
        </li>
      ))}
    </ul>
  )
}

const KINDS: [FaultKind, string][] = [
  ['Duplicate', 'Duplicate (redeliver)'],
  ['Reorder', 'Reorder'],
  ['Delay', 'Delay'],
  ['Drop', 'Drop'],
  ['CrashRestart', 'Crash and restart'],
]

/** Starting values for a new fault; the window reverses a pair. */
function initialDraft(workload: SimEvent[]): FaultDraft {
  return {
    kind: 'Duplicate',
    eventId: workload.at(0)?.id ?? 0,
    window: '2',
    by: '5000',
    at: '0',
  }
}

function AddFault({
  plan,
  workload,
  onAdd,
}: {
  plan: FaultOp[]
  workload: SimEvent[]
  onAdd: (op: FaultOp) => void
}) {
  const [draft, setDraft] = useState(() => initialDraft(workload))
  const result = faultFromDraft(draft, workload)
  const full = !canAddFault(plan)
  const edit = (change: Partial<FaultDraft>) =>
    setDraft((current) => ({ ...current, ...change }))

  let status: string
  if (full) status = `A plan holds at most ${MAX_PLAN_FAULTS} faults.`
  else if (result.ok) status = `Adds: ${describeFault(result.op)}`
  else status = `Can't add: ${result.problem}.`

  return (
    <fieldset className="add-fault">
      <legend>Add a fault</legend>
      <label>
        Type
        <select
          value={draft.kind}
          onChange={(event) => edit({ kind: event.target.value as FaultKind })}
        >
          {KINDS.map(([kind, label]) => (
            <option key={kind} value={kind}>
              {label}
            </option>
          ))}
        </select>
      </label>
      {draft.kind !== 'CrashRestart' && (
        <label>
          Event
          <select
            value={draft.eventId}
            onChange={(event) => edit({ eventId: Number(event.target.value) })}
          >
            {workload.map(({ id, kind }) => (
              <option key={id} value={id}>
                {id}: {describeEvent(kind)}
              </option>
            ))}
          </select>
        </label>
      )}
      {draft.kind === 'Reorder' && (
        <NumberField
          label="Window (deliveries)"
          value={draft.window}
          onChange={(window) => edit({ window })}
        />
      )}
      {draft.kind === 'Delay' && (
        <NumberField
          label="Delay by (ms)"
          value={draft.by}
          onChange={(by) => edit({ by })}
        />
      )}
      {draft.kind === 'CrashRestart' && (
        <NumberField
          label="At (ms)"
          value={draft.at}
          onChange={(at) => edit({ at })}
        />
      )}
      <p className="hint" aria-live="polite">
        {status}
      </p>
      <button
        type="button"
        disabled={full || !result.ok}
        onClick={() => result.ok && onAdd(result.op)}
      >
        Add fault
      </button>
    </fieldset>
  )
}

function NumberField({
  label,
  value,
  onChange,
}: {
  label: string
  value: string
  onChange: (value: string) => void
}) {
  return (
    <label>
      {label}
      <input
        value={value}
        inputMode="numeric"
        aria-invalid={parseWholeNumber(value) === null}
        onChange={(event) => onChange(event.target.value)}
      />
    </label>
  )
}
