import type { FaultOp, Handler, ScenarioSummary } from '../api/types'
import { MAX_SEED } from '../api/types'
import type { LoadedPlan } from '../lib/faultPlan'
import { FaultPlanEditor } from './FaultPlanEditor'
import { ShareButton } from './ShareButton'

interface ControlsProps {
  scenarios: ScenarioSummary[]
  scenarioId: string | null
  seedText: string
  seed: number | null
  handler: Handler
  plan: FaultOp[] | null
  /** What sim-api generated for this scenario and seed, once a run showed it. */
  seedPlan: FaultOp[] | null
  /** The last plan loaded from a share link or the shrinker. */
  loadedPlan: LoadedPlan | null
  /** True while a run or a shrink is in flight. */
  locked: boolean
  running: boolean
  canRun: boolean
  onScenario: (id: string) => void
  onSeed: (text: string) => void
  onRandomSeed: () => void
  onHandler: (handler: Handler) => void
  onPlanChange: (plan: FaultOp[]) => void
  onPlanReset: (to: 'story' | 'seed') => void
  onRun: () => void
  /** The shown run's share link, or `null` with no run to share. */
  shareUrl: string | null
}

const HANDLERS: [Handler, string][] = [
  ['naive', 'Naive'],
  ['hardened', 'Hardened'],
]

/** The run's inputs: scenario, seed, handler and the fault plan. */
export function Controls(props: ControlsProps) {
  const scenario = props.scenarios.find(({ id }) => id === props.scenarioId)
  return (
    <section className="panel controls" aria-labelledby="controls-heading">
      <h2 id="controls-heading">Run</h2>

      {/* Locked while a run or shrink is in flight, so a result never lands
          against inputs changed after it was asked for. */}
      <fieldset className="inputs" disabled={props.locked}>
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

        {scenario && (
          <FaultPlanEditor
            // A new scenario has new events, so the add form starts over.
            key={scenario.id}
            plan={props.plan}
            seedPlan={props.seedPlan}
            loadedPlan={props.loadedPlan}
            storyPlan={scenario.story_plan}
            seed={props.seed}
            workload={scenario.workload}
            onChange={props.onPlanChange}
            onReset={props.onPlanReset}
          />
        )}
      </fieldset>

      <div className="actions">
        <button
          type="button"
          className="primary"
          disabled={!props.canRun}
          onClick={props.onRun}
        >
          {props.running ? 'Running…' : 'Run'}
        </button>
        {/* Keyed by link, so a new run's link starts uncopied. */}
        {props.shareUrl && (
          <ShareButton key={props.shareUrl} url={props.shareUrl} />
        )}
      </div>
    </section>
  )
}
