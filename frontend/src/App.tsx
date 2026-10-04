import { useEffect, useReducer, useState } from 'react'
import { Latest, isAbortError, type SimClient } from './api/client'
import { BalancePanel } from './components/BalancePanel'
import { Controls } from './components/Controls'
import { ErrorBanner } from './components/ErrorBanner'
import { InvariantPanel } from './components/InvariantPanel'
import { Timeline } from './components/Timeline'
import { balancesByStep, entriesByStep } from './lib/balances'
import { randomSeed } from './lib/seed'
import { initialState, reducer, runRequest } from './state/appState'

/** The playground. Every number it shows comes from `client`: sim-api in the
 * app, a stub serving the golden fixtures in tests. */
function App({ client }: { client: SimClient }) {
  const [state, dispatch] = useReducer(reducer, initialState)
  const [runs] = useState(() => new Latest())

  useEffect(() => {
    const controller = new AbortController()
    client.scenarios({ signal: controller.signal }).then(
      (scenarios) => dispatch({ type: 'scenariosLoaded', scenarios }),
      (error: unknown) => {
        if (!isAbortError(error)) dispatch({ type: 'failed', error })
      },
    )
    return () => controller.abort()
  }, [client])

  const request = runRequest(state)

  function startRun() {
    if (request === null) return
    dispatch({ type: 'runStarted' })
    runs
      .start((signal) => client.run(request, { signal }))
      .then(
        (response) => {
          try {
            const balances = balancesByStep(response)
            dispatch({ type: 'runSucceeded', response, balances })
          } catch (error) {
            dispatch({ type: 'failed', error })
          }
        },
        (error: unknown) => {
          // A superseded run was aborted on purpose; its replacement reports.
          if (!isAbortError(error)) dispatch({ type: 'failed', error })
        },
      )
  }

  const { run } = state
  return (
    <>
      <header className="app-header">
        <h1>Rails Sim Playground</h1>
        <p>
          Deterministic payments simulator. No real money moves, and no
          compliance claims are made.
        </p>
      </header>
      <ErrorBanner error={state.error} />
      <main className="layout">
        <Controls
          scenarios={state.scenarios}
          scenarioId={state.scenarioId}
          seedText={state.seedText}
          seed={state.seed}
          handler={state.handler}
          plan={state.plan}
          seedPlan={state.seedPlan}
          running={state.status === 'running'}
          canRun={request !== null && state.status === 'ready'}
          onScenario={(id) => dispatch({ type: 'scenarioPicked', id })}
          onSeed={(text) => dispatch({ type: 'seedEdited', text })}
          onRandomSeed={() =>
            dispatch({ type: 'seedEdited', text: String(randomSeed()) })
          }
          onHandler={(handler) => dispatch({ type: 'handlerPicked', handler })}
          onPlanChange={(plan) => dispatch({ type: 'planEdited', plan })}
          onPlanReset={(to) => dispatch({ type: 'planReset', to })}
          onRun={startRun}
        />
        <div className="results">
          {run ? (
            <>
              <InvariantPanel invariants={run.response.invariants} />
              <div className="trace">
                <Timeline
                  trace={run.response.trace}
                  entries={entriesByStep(run.response)}
                  step={run.step}
                  onStep={(step) => dispatch({ type: 'stepSelected', step })}
                />
                <BalancePanel balances={run.balances} step={run.step} />
              </div>
            </>
          ) : (
            <p className="placeholder">
              {state.status === 'loading'
                ? 'Loading scenarios…'
                : 'Pick a scenario and run it to see what happens.'}
            </p>
          )}
        </div>
      </main>
    </>
  )
}

export default App
