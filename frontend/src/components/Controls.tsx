import type { FaultOp, Handler, ScenarioSummary } from '../api/types'
import { MAX_SEED } from '../api/types'
import { describeFault } from '../lib/faultPlan'

interface ControlsProps {
  scenarios: ScenarioSummary[]
  scenarioId: string | null
  seedText: string
  seed: number | null
  handler: Handler
  plan: FaultOp[] | null
  /** The plan sim-api generated for the shown run, when `plan` is null. */
  generated: FaultOp[] | null
  running: boolean
  canRun: boolean
  onScenario: (id: string) => void
  onSeed: (text: string) => void
  onRandomSeed: () => void
  onHandler: (handler: Handler) => void
  onRun: () => void
}

const HANDLERS: [Handler, string][] = [
  ['naive', 'Naive'],
  ['hardened', 'Hardened'],
]

/** The run's inputs: scenario, seed, handler and the plan it will use. */
export function Controls(props: ControlsProps) {
  const scenario = props.scenarios.find(({ id }) => id === props.scenarioId)
  return (
    <section className="panel controls" aria-labelledby="controls-heading">
      <h2 id="controls-heading">Run</h2>

      {/* Locked while running, so a result never lands against inputs
          changed after it was asked for. */}
      <fieldset className="inputs" disabled={props.running}>
        <label className="field">
          Scenario
          <select
            value={props.scenarioId ?? ''}
            onChange={(event) => props.onScenario(event.target.value)}
          >
            {props.scenarios.map(({ id, name }) => (
              <option key={id} value={id}>
                {name}
              </option>
            ))}
          </select>
        </label>
        {scenario && <p className="description">{scenario.description}</p>}

        <div className="field">
          <label htmlFor="seed">Seed</label>
          <div className="seed-row">
            <input
              id="seed"
              value={props.seedText}
              inputMode="numeric"
              aria-invalid={props.seed === null}
              aria-describedby={props.seed === null ? 'seed-hint' : undefined}
              onChange={(event) => props.onSeed(event.target.value)}
            />
            <button type="button" onClick={props.onRandomSeed}>
              Random
            </button>
          </div>
          {props.seed === null && (
            <p id="seed-hint" className="hint">
              A seed is a whole number from 0 to {MAX_SEED}.
            </p>
          )}
        </div>

        <fieldset className="field handlers">
          <legend>Handler</legend>
          {HANDLERS.map(([handler, label]) => (
            <label key={handler}>
              <input
                type="radio"
                name="handler"
                value={handler}
                checked={props.handler === handler}
                onChange={() => props.onHandler(handler)}
              />
              {label}
            </label>
          ))}
        </fieldset>
      </fieldset>

      <div className="field">
        <h3>Faults</h3>
        <FaultList
          plan={props.plan}
          generated={props.generated}
          seed={props.seed}
        />
      </div>

      <button
        type="button"
        className="primary"
        disabled={!props.canRun}
        onClick={props.onRun}
      >
        {props.running ? 'Running…' : 'Run'}
      </button>
    </section>
  )
}

function FaultList({
  plan,
  generated,
  seed,
}: {
  plan: FaultOp[] | null
  generated: FaultOp[] | null
  seed: number | null
}) {
  if (plan === null && generated === null) {
    return (
      <p className="hint">
        {seed === null
          ? 'Generated from the seed when you run.'
          : `Generated from seed ${seed} when you run.`}
      </p>
    )
  }
  const shown = plan ?? generated ?? []
  return (
    <>
      {plan === null && <p className="hint">Generated from seed {seed}:</p>}
      {shown.length === 0 ? (
        <p className="hint">No faults.</p>
      ) : (
        <ul className="faults">
          {shown.map((op, index) => (
            <li key={index}>{describeFault(op)}</li>
          ))}
        </ul>
      )}
    </>
  )
}
